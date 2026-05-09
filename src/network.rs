use std::ffi::{CString, c_void};
use std::io::Write;
use std::net::Ipv4Addr;
use std::ops::{Deref, DerefMut};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use ipnet::Ipv4Net;
use serde_json::{Value, json};

use crate::cni::CniResult;
use crate::cni::internal::NetlinkOps;
use crate::cni::vmconf::static_network_conf_from;
use crate::error::{Error, Result};
use crate::kernelargs::KernelArgs;
use crate::models::RateLimiter;

pub const DEFAULT_CNI_BIN_DIR: &str = "/opt/cni/bin";
pub const DEFAULT_CNI_CONF_DIR: &str = "/etc/cni/conf.d";
pub const DEFAULT_CNI_CACHE_DIR: &str = "/var/lib/cni";

pub type CleanupFn = Box<dyn Fn() -> Result<()> + Send + Sync + 'static>;

pub trait CniNetworkOperations {
    fn initialize_netns(&self, net_ns_path: &str) -> Result<Vec<CleanupFn>>;
    fn invoke_cni(&self, config: &CniConfiguration) -> Result<(CniResult, Vec<CleanupFn>)>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RealCniNetworkOperations;

#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedCniNetworkOperations;

impl CniNetworkOperations for UnsupportedCniNetworkOperations {
    fn initialize_netns(&self, _net_ns_path: &str) -> Result<Vec<CleanupFn>> {
        Err(Error::InvalidConfig(
            "CNI operations are not configured".into(),
        ))
    }

    fn invoke_cni(&self, _config: &CniConfiguration) -> Result<(CniResult, Vec<CleanupFn>)> {
        Err(Error::InvalidConfig(
            "CNI operations are not configured".into(),
        ))
    }
}

const CLONE_NEWNET: i32 = 0x4000_0000;
const MS_BIND: u64 = 4096;
const MNT_DETACH: i32 = 2;

unsafe extern "C" {
    #[link_name = "unshare"]
    fn libc_unshare(flags: i32) -> i32;
    #[link_name = "mount"]
    fn libc_mount(
        source: *const i8,
        target: *const i8,
        fstype: *const i8,
        flags: u64,
        data: *const c_void,
    ) -> i32;
    #[link_name = "umount2"]
    fn libc_umount2(target: *const i8, flags: i32) -> i32;
}

impl RealCniNetworkOperations {
    fn invoke_network_list(
        &self,
        config: &CniConfiguration,
        command: &str,
    ) -> Result<Option<CniResult>> {
        let network_config = self.load_network_config(config)?;
        let plugins = network_config
            .get("plugins")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                Error::InvalidConfig("CNI network configuration must contain plugins".into())
            })?;

        let runtime_conf = config.as_cni_runtime_conf();
        let mut prev_result = None::<Value>;

        let plugin_iter: Box<dyn Iterator<Item = &Value>> = if command == "DEL" {
            Box::new(plugins.iter().rev())
        } else {
            Box::new(plugins.iter())
        };

        for plugin in plugin_iter {
            let plugin_input = self.plugin_input(&network_config, plugin, prev_result.as_ref())?;
            let plugin_output = self.run_plugin(config, &plugin_input, &runtime_conf, command)?;
            if command == "ADD" {
                prev_result = Some(serde_json::from_slice(&plugin_output)?);
            }
        }

        prev_result
            .map(serde_json::from_value)
            .transpose()
            .map_err(Into::into)
    }

    fn load_network_config(&self, config: &CniConfiguration) -> Result<Value> {
        let parsed = if let Some(raw_config) = &config.network_config {
            serde_json::from_str(raw_config)?
        } else {
            self.load_network_config_from_dir(config)?
        };

        self.normalize_network_config(parsed, config.network_name.as_deref())
    }

    fn load_network_config_from_dir(&self, config: &CniConfiguration) -> Result<Value> {
        let conf_dir = config
            .conf_dir
            .as_deref()
            .ok_or_else(|| Error::InvalidConfig("missing CNI conf dir".into()))?;
        let network_name = config
            .network_name
            .as_deref()
            .ok_or_else(|| Error::InvalidConfig("missing CNI network name".into()))?;

        for extension in ["conflist", "conf", "json"] {
            let candidate = Path::new(conf_dir).join(format!("{network_name}.{extension}"));
            if candidate.exists() {
                return serde_json::from_slice(&std::fs::read(candidate)?).map_err(Into::into);
            }
        }

        for entry in std::fs::read_dir(conf_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let parsed: Value = match serde_json::from_slice(&std::fs::read(&path)?) {
                Ok(value) => value,
                Err(_) => continue,
            };

            if parsed
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name == network_name)
            {
                return Ok(parsed);
            }
        }

        Err(Error::InvalidConfig(format!(
            "failed to load CNI configuration for network {network_name:?} from {conf_dir:?}"
        )))
    }

    fn normalize_network_config(&self, config: Value, default_name: Option<&str>) -> Result<Value> {
        if config.get("plugins").and_then(Value::as_array).is_some() {
            return Ok(config);
        }

        if config.get("type").is_some() {
            let cni_version = config
                .get("cniVersion")
                .cloned()
                .unwrap_or_else(|| Value::String("0.3.1".to_string()));
            let name = config
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| default_name.map(ToOwned::to_owned))
                .ok_or_else(|| {
                    Error::InvalidConfig(
                        "single-plugin CNI configuration must include a network name".into(),
                    )
                })?;

            return Ok(json!({
                "cniVersion": cni_version,
                "name": name,
                "plugins": [config],
            }));
        }

        Err(Error::InvalidConfig(
            "invalid CNI network configuration".into(),
        ))
    }

    fn plugin_input(
        &self,
        network_config: &Value,
        plugin: &Value,
        prev_result: Option<&Value>,
    ) -> Result<Value> {
        let mut plugin_input = plugin.clone();
        let network_config = network_config.as_object().ok_or_else(|| {
            Error::InvalidConfig("CNI network configuration must be a JSON object".into())
        })?;
        let plugin_object = plugin_input.as_object_mut().ok_or_else(|| {
            Error::InvalidConfig("CNI plugin configuration must be a JSON object".into())
        })?;

        for key in ["cniVersion", "name"] {
            if !plugin_object.contains_key(key) {
                if let Some(value) = network_config.get(key) {
                    plugin_object.insert(key.to_string(), value.clone());
                }
            }
        }

        if let Some(prev_result) = prev_result {
            plugin_object.insert("prevResult".to_string(), prev_result.clone());
        }

        Ok(plugin_input)
    }

    fn run_plugin(
        &self,
        config: &CniConfiguration,
        plugin_input: &Value,
        runtime_conf: &CniRuntimeConf,
        command: &str,
    ) -> Result<Vec<u8>> {
        let plugin_type = plugin_input
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::InvalidConfig("CNI plugin type is not set".into()))?;
        let plugin_path = self.find_plugin(plugin_type, &config.bin_path)?;

        let mut child = Command::new(plugin_path)
            .env("CNI_COMMAND", command)
            .env(
                "CNI_CONTAINERID",
                runtime_conf.container_id.as_deref().unwrap_or_default(),
            )
            .env(
                "CNI_NETNS",
                runtime_conf.net_ns.as_deref().unwrap_or_default(),
            )
            .env(
                "CNI_IFNAME",
                runtime_conf.if_name.as_deref().unwrap_or_default(),
            )
            .env("CNI_PATH", config.bin_path.join(":"))
            .env("CNI_ARGS", Self::format_cni_args(&runtime_conf.args))
            .env(
                "CNI_CACHE_DIR",
                config.cache_dir.as_deref().unwrap_or_default(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| Error::Process(error.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&serde_json::to_vec(plugin_input)?)?;
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                format!(
                    "CNI plugin {plugin_type:?} exited with status {}",
                    output.status
                )
            } else {
                format!("CNI plugin {plugin_type:?} failed: {stderr}")
            };
            return Err(Error::Process(message));
        }

        Ok(output.stdout)
    }

    fn find_plugin(&self, plugin_type: &str, bin_path: &[String]) -> Result<PathBuf> {
        for dir in bin_path {
            let candidate = Path::new(dir).join(plugin_type);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }

        Err(Error::InvalidConfig(format!(
            "failed to find CNI plugin binary {plugin_type:?} in {:?}",
            bin_path
        )))
    }

    fn format_cni_args(args: &[(String, String)]) -> String {
        args.iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(";")
    }

    fn path_to_cstring(path: &Path) -> Result<CString> {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| Error::InvalidConfig(format!("invalid path for C string: {:?}", path)))
    }

    fn cstring(value: &str) -> Result<CString> {
        CString::new(value)
            .map_err(|_| Error::InvalidConfig(format!("invalid string for C string: {value:?}")))
    }
}

impl CniNetworkOperations for RealCniNetworkOperations {
    fn initialize_netns(&self, net_ns_path: &str) -> Result<Vec<CleanupFn>> {
        let path = Path::new(net_ns_path);
        if path.exists() {
            return Ok(Vec::new());
        }

        let mut cleanup_funcs: Vec<CleanupFn> = Vec::new();
        let parent_dir = path
            .parent()
            .ok_or_else(|| Error::InvalidConfig(format!("invalid netns path {net_ns_path:?}")))?;
        if !parent_dir.exists() {
            std::fs::create_dir_all(parent_dir)?;
            let parent_dir = parent_dir.to_path_buf();
            cleanup_funcs.push(Box::new(move || match std::fs::remove_dir(&parent_dir) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }));
        }

        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        let path_for_cleanup = path.to_path_buf();
        cleanup_funcs.push(Box::new(move || {
            match std::fs::remove_file(&path_for_cleanup) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        }));

        let mount_target = path.to_path_buf();
        let mount_result = thread::spawn(move || -> Result<()> {
            let source = Self::cstring("/proc/thread-self/ns/net")?;
            let target = Self::path_to_cstring(&mount_target)?;
            let none = Self::cstring("none")?;

            if unsafe { libc_unshare(CLONE_NEWNET) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }

            if unsafe {
                libc_mount(
                    source.as_ptr(),
                    target.as_ptr(),
                    none.as_ptr(),
                    MS_BIND,
                    std::ptr::null(),
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().into());
            }

            Ok(())
        })
        .join()
        .map_err(|_| Error::Process("failed joining netns initialization thread".into()))?;

        if let Err(error) = mount_result {
            while let Some(cleanup) = cleanup_funcs.pop() {
                let _ = cleanup();
            }
            return Err(error);
        }

        let unmount_path = path.to_path_buf();
        cleanup_funcs.push(Box::new(move || {
            let target = Self::path_to_cstring(&unmount_path)?;
            if unsafe { libc_umount2(target.as_ptr(), MNT_DETACH) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(22) || error.raw_os_error() == Some(2) {
                    return Ok(());
                }
                return Err(error.into());
            }
            Ok(())
        }));

        Ok(cleanup_funcs)
    }

    fn invoke_cni(&self, config: &CniConfiguration) -> Result<(CniResult, Vec<CleanupFn>)> {
        let delete_result = self.invoke_network_list(config, "DEL");
        if delete_result.is_err() && !config.force {
            return Err(Error::Process(format!(
                "failed to delete pre-existing CNI network: {}",
                delete_result.unwrap_err()
            )));
        }

        let cleanup_config = config.clone();
        let cleanup = Box::new(move || {
            RealCniNetworkOperations
                .invoke_network_list(&cleanup_config, "DEL")
                .map(|_| ())
        }) as CleanupFn;

        match self.invoke_network_list(config, "ADD")? {
            Some(result) => Ok((result, vec![cleanup])),
            None => Err(Error::Process(
                "CNI ADD did not return a result payload".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkInterfaces(Vec<NetworkInterface>);

impl NetworkInterfaces {
    pub fn validate(&self, kernel_args: &KernelArgs) -> Result<()> {
        for iface in &self.0 {
            let has_cni = iface.cni_configuration.is_some();
            let has_static = iface.static_configuration.is_some();
            let has_static_ip = iface
                .static_configuration
                .as_ref()
                .and_then(|static_config| static_config.ip_configuration.as_ref())
                .is_some();

            if !has_cni && !has_static {
                return Err(Error::InvalidConfig(
                    "must specify at least one of CNIConfiguration or StaticConfiguration".into(),
                ));
            }

            if has_cni && has_static {
                return Err(Error::InvalidConfig(
                    "cannot provide both CNIConfiguration and StaticConfiguration".into(),
                ));
            }

            if has_cni || has_static_ip {
                if self.0.len() > 1 {
                    return Err(Error::InvalidConfig(
                        "cannot specify CNIConfiguration or IPConfiguration when multiple network interfaces are provided".into(),
                    ));
                }

                if kernel_args.contains_key("ip") {
                    return Err(Error::InvalidConfig(
                        "CNIConfiguration or IPConfiguration cannot be specified when \"ip=\" is already present in kernel boot args".into(),
                    ));
                }
            }

            if let Some(cni) = &iface.cni_configuration {
                cni.validate()?;
            }

            if let Some(static_config) = &iface.static_configuration {
                static_config.validate()?;
            }
        }

        Ok(())
    }

    pub fn cni_interface(&self) -> Option<&NetworkInterface> {
        self.0
            .iter()
            .find(|iface| iface.cni_configuration.is_some())
    }

    pub fn cni_interface_mut(&mut self) -> Option<&mut NetworkInterface> {
        self.0
            .iter_mut()
            .find(|iface| iface.cni_configuration.is_some())
    }

    pub fn static_ip_interface(&self) -> Option<&NetworkInterface> {
        self.0.iter().find(|iface| {
            iface
                .static_configuration
                .as_ref()
                .and_then(|config| config.ip_configuration.as_ref())
                .is_some()
        })
    }

    pub fn apply_cni_result(
        &mut self,
        result: &CniResult,
        netlink_ops: &dyn NetlinkOps,
    ) -> Result<()> {
        let Some(iface) = self.cni_interface_mut() else {
            return Ok(());
        };

        iface.apply_cni_result(result, netlink_ops)
    }

    pub fn setup_cni(
        &mut self,
        vm_id: &str,
        net_ns_path: &str,
        cni_ops: &dyn CniNetworkOperations,
        netlink_ops: &dyn NetlinkOps,
    ) -> Result<Vec<CleanupFn>> {
        let Some(iface) = self.cni_interface_mut() else {
            return Ok(Vec::new());
        };

        let cni_configuration = iface
            .cni_configuration
            .as_mut()
            .ok_or_else(|| Error::InvalidConfig("missing CNIConfiguration".into()))?;
        cni_configuration.container_id = Some(vm_id.to_string());
        cni_configuration.net_ns_path = Some(net_ns_path.to_string());
        cni_configuration.set_defaults();

        let mut cleanup_funcs = cni_ops.initialize_netns(net_ns_path)?;
        let (result, mut cni_cleanup_funcs) = cni_ops.invoke_cni(cni_configuration)?;
        cleanup_funcs.append(&mut cni_cleanup_funcs);
        iface.apply_cni_result(&result, netlink_ops)?;
        Ok(cleanup_funcs)
    }
}

impl Deref for NetworkInterfaces {
    type Target = Vec<NetworkInterface>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for NetworkInterfaces {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<Vec<NetworkInterface>> for NetworkInterfaces {
    fn from(value: Vec<NetworkInterface>) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkInterface {
    pub static_configuration: Option<StaticNetworkConfiguration>,
    pub cni_configuration: Option<CniConfiguration>,
    pub allow_mmds: bool,
    pub in_rate_limiter: Option<RateLimiter>,
    pub out_rate_limiter: Option<RateLimiter>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CniConfiguration {
    pub network_name: Option<String>,
    pub network_config: Option<String>,
    pub if_name: Option<String>,
    pub vm_if_name: Option<String>,
    pub args: Vec<(String, String)>,
    pub bin_path: Vec<String>,
    pub conf_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub container_id: Option<String>,
    pub net_ns_path: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CniRuntimeConf {
    pub container_id: Option<String>,
    pub net_ns: Option<String>,
    pub if_name: Option<String>,
    pub args: Vec<(String, String)>,
}

impl CniConfiguration {
    pub fn validate(&self) -> Result<()> {
        match (&self.network_name, &self.network_config) {
            (None, None) => Err(Error::InvalidConfig(
                "must specify either NetworkName or NetworkConfig in CNIConfiguration".into(),
            )),
            (Some(_), Some(_)) => Err(Error::InvalidConfig(
                "must not specify both NetworkName and NetworkConfig in CNIConfiguration".into(),
            )),
            _ => Ok(()),
        }
    }

    pub fn set_defaults(&mut self) {
        if self.bin_path.is_empty() {
            self.bin_path = vec![DEFAULT_CNI_BIN_DIR.to_string()];
        }

        if self.conf_dir.is_none() {
            self.conf_dir = Some(DEFAULT_CNI_CONF_DIR.to_string());
        }

        if self.cache_dir.is_none() {
            self.cache_dir = Some(format!(
                "{}/{}",
                DEFAULT_CNI_CACHE_DIR,
                self.container_id.clone().unwrap_or_default()
            ));
        }
    }

    pub fn as_cni_runtime_conf(&self) -> CniRuntimeConf {
        CniRuntimeConf {
            container_id: self.container_id.clone(),
            net_ns: self.net_ns_path.clone(),
            if_name: self.if_name.clone(),
            args: self.args.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticNetworkConfiguration {
    pub mac_address: Option<String>,
    pub host_dev_name: String,
    pub ip_configuration: Option<IPConfiguration>,
}

impl StaticNetworkConfiguration {
    pub fn new(host_dev_name: impl Into<String>) -> Self {
        Self {
            host_dev_name: host_dev_name.into(),
            ..Self::default()
        }
    }

    pub fn with_mac_address(mut self, mac_address: impl Into<String>) -> Self {
        self.mac_address = Some(mac_address.into());
        self
    }

    pub fn with_ip_configuration(mut self, ip_configuration: IPConfiguration) -> Self {
        self.ip_configuration = Some(ip_configuration);
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.host_dev_name.is_empty() {
            return Err(Error::InvalidConfig(
                "HostDevName must be provided if StaticNetworkConfiguration is provided".into(),
            ));
        }

        if let Some(ip_configuration) = &self.ip_configuration {
            ip_configuration.validate()?;
        }

        Ok(())
    }
}

impl NetworkInterface {
    pub fn apply_cni_result(
        &mut self,
        result: &CniResult,
        netlink_ops: &dyn NetlinkOps,
    ) -> Result<()> {
        if self.static_configuration.is_some() {
            return Ok(());
        }

        let cni_configuration = self
            .cni_configuration
            .as_ref()
            .ok_or_else(|| Error::InvalidConfig("missing CNIConfiguration".into()))?;
        let container_id = cni_configuration
            .container_id
            .as_deref()
            .ok_or_else(|| Error::InvalidConfig("missing CNI container id".into()))?;

        let mut vm_net_conf = static_network_conf_from(result, container_id, netlink_ops)
            .map_err(|error| Error::InvalidConfig(error.to_string()))?;

        if vm_net_conf.vm_nameservers.len() > 2 {
            vm_net_conf.vm_nameservers.truncate(2);
        }

        self.static_configuration = Some(StaticNetworkConfiguration {
            host_dev_name: vm_net_conf.tap_name,
            mac_address: vm_net_conf.vm_mac_addr,
            ip_configuration: vm_net_conf.vm_ip_config.map(|ip_config| IPConfiguration {
                ip_addr: ip_config.address,
                gateway: ip_config.gateway,
                nameservers: vm_net_conf.vm_nameservers,
                if_name: cni_configuration.vm_if_name.clone(),
            }),
        });

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IPConfiguration {
    pub ip_addr: Ipv4Net,
    pub gateway: Ipv4Addr,
    pub nameservers: Vec<String>,
    pub if_name: Option<String>,
}

impl IPConfiguration {
    pub fn new(ip_addr: Ipv4Net, gateway: Ipv4Addr) -> Self {
        Self {
            ip_addr,
            gateway,
            nameservers: Vec::new(),
            if_name: None,
        }
    }

    pub fn with_nameservers<I, S>(mut self, nameservers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.nameservers = nameservers.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_if_name(mut self, if_name: impl Into<String>) -> Self {
        self.if_name = Some(if_name.into());
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.nameservers.len() > 2 {
            return Err(Error::InvalidConfig(
                "cannot specify more than 2 nameservers".into(),
            ));
        }

        Ok(())
    }

    pub fn ip_boot_param(&self) -> String {
        let mask = self.ip_addr.netmask().octets();
        let nameserver_1 = self.nameservers.first().cloned().unwrap_or_default();
        let nameserver_2 = self.nameservers.get(1).cloned().unwrap_or_default();

        [
            self.ip_addr.addr().to_string(),
            String::new(),
            self.gateway.to_string(),
            format!("{}.{}.{}.{}", mask[0], mask[1], mask[2], mask[3]),
            String::new(),
            self.if_name.clone().unwrap_or_default(),
            "off".to_string(),
            nameserver_1,
            nameserver_2,
            String::new(),
        ]
        .join(":")
    }
}
