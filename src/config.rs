use std::fmt;
use std::io::Write;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

use crate::drives::DrivesBuilder;
use crate::jailer::JailerConfig;
use crate::models::{Drive, MachineConfiguration};
use crate::network::NetworkInterfaces;
use crate::snapshot::SnapshotConfig;
use crate::vsock::VsockDevice;

pub const DEFAULT_NET_NS_DIR: &str = "/var/run/netns";
pub const FIRECRACKER_INIT_TIMEOUT_ENV: &str = "FIRECRACKER_GO_SDK_INIT_TIMEOUT_SECONDS";
pub const DEFAULT_FIRECRACKER_INIT_TIMEOUT_SECONDS: u64 = 3;
pub const DEFAULT_FORWARD_SIGNALS: &[i32] = &[2, 3, 15, 1, 6];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeccompConfig {
    pub enabled: bool,
    pub filter: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MMDSVersion {
    #[default]
    V1,
    V2,
}

#[derive(Clone)]
pub struct FifoLogWriter {
    inner: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl FifoLogWriter {
    pub fn new<W>(writer: W) -> Self
    where
        W: Write + Send + 'static,
    {
        Self {
            inner: Arc::new(Mutex::new(Box::new(writer))),
        }
    }
}

impl fmt::Debug for FifoLogWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FifoLogWriter").finish_non_exhaustive()
    }
}

impl Write for FifoLogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner
            .lock()
            .map_err(|_| std::io::Error::other("fifo log writer mutex poisoned"))?
            .write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner
            .lock()
            .map_err(|_| std::io::Error::other("fifo log writer mutex poisoned"))?
            .flush()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub socket_path: String,
    pub log_path: Option<String>,
    pub log_fifo: Option<String>,
    pub log_level: Option<String>,
    pub metrics_path: Option<String>,
    pub metrics_fifo: Option<String>,
    pub kernel_image_path: String,
    pub initrd_path: Option<String>,
    pub kernel_args: String,
    pub drives: Vec<Drive>,
    pub network_interfaces: NetworkInterfaces,
    pub fifo_log_writer: Option<FifoLogWriter>,
    pub vsock_devices: Vec<VsockDevice>,
    pub machine_cfg: MachineConfiguration,
    pub disable_validation: bool,
    pub jailer_cfg: Option<JailerConfig>,
    pub vmid: String,
    pub net_ns: Option<String>,
    pub forward_signals: Option<Vec<i32>>,
    pub seccomp: SeccompConfig,
    pub mmds_address: Option<Ipv4Addr>,
    pub mmds_version: MMDSVersion,
    pub snapshot: SnapshotConfig,
}

impl Config {
    pub fn has_snapshot(&self) -> bool {
        self.snapshot.get_mem_backend_path().is_some() || self.snapshot.snapshot_path.is_some()
    }

    pub fn validate(&self) -> crate::Result<()> {
        if self.disable_validation {
            return Ok(());
        }

        Self::validate_existing_path("kernel image path", &self.kernel_image_path)?;

        if let Some(initrd_path) = &self.initrd_path {
            Self::validate_existing_path("initrd image path", initrd_path)?;
        }

        for drive in &self.drives {
            if drive.is_root_device == Some(true) {
                Self::validate_existing_path(
                    "host drive path",
                    drive.path_on_host.as_deref().unwrap_or_default(),
                )?;
                break;
            }
        }

        if std::fs::metadata(&self.socket_path).is_ok() {
            return Err(crate::Error::InvalidConfig(format!(
                "socket {} already exists",
                self.socket_path
            )));
        }

        if self.machine_cfg.vcpu_count.unwrap_or_default() < 1 {
            return Err(crate::Error::InvalidConfig(
                "machine needs a nonzero VcpuCount".into(),
            ));
        }

        if self.machine_cfg.mem_size_mib.unwrap_or_default() < 1 {
            return Err(crate::Error::InvalidConfig(
                "machine needs a nonzero amount of memory".into(),
            ));
        }

        Ok(())
    }

    pub fn validate_load_snapshot(&self) -> crate::Result<()> {
        if self.disable_validation {
            return Ok(());
        }

        for drive in &self.drives {
            Self::validate_existing_path(
                "drive path",
                drive.path_on_host.as_deref().unwrap_or_default(),
            )?;
        }

        if std::fs::metadata(&self.socket_path).is_ok() {
            return Err(crate::Error::InvalidConfig(format!(
                "socket {} already exists",
                self.socket_path
            )));
        }

        Self::validate_existing_path(
            "snapshot memory path",
            self.snapshot.get_mem_backend_path().unwrap_or_default(),
        )?;
        Self::validate_existing_path(
            "snapshot path",
            self.snapshot.snapshot_path.as_deref().unwrap_or_default(),
        )?;

        Ok(())
    }

    pub fn validate_network(&self) -> crate::Result<()> {
        self.network_interfaces
            .validate(&crate::kernelargs::parse_kernel_args(&self.kernel_args))
    }

    pub fn root_drive_present(&self) -> bool {
        self.initrd_path.is_some()
            || self
                .drives
                .iter()
                .any(|drive| drive.is_root_device == Some(true))
    }

    pub fn sample(root_drive_path: &str) -> Self {
        Self {
            drives: DrivesBuilder::new(root_drive_path).build(),
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Self::default()
        }
    }

    pub fn normalized_forward_signals(&self) -> Vec<i32> {
        self.forward_signals
            .clone()
            .unwrap_or_else(|| DEFAULT_FORWARD_SIGNALS.to_vec())
    }

    fn validate_existing_path(kind: &str, path: &str) -> crate::Result<()> {
        std::fs::metadata(path).map_err(|error| {
            crate::Error::InvalidConfig(format!("failed to stat {kind}, {:?}: {error}", path))
        })?;
        Ok(())
    }
}
