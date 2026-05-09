use std::ffi::{CString, c_char};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use crate::command_builder::{CommandStdio, VMCommand, VMCommandBuilder, seccomp_args};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::handlers::{
    CREATE_LOG_FILES_HANDLER_NAME, Handler, Handlers, LINK_FILES_TO_ROOTFS_HANDLER_NAME,
};
use crate::machine::Machine;

pub const DEFAULT_JAILER_PATH: &str = "/srv/jailer";
pub const DEFAULT_JAILER_BIN: &str = "jailer";
pub const ROOTFS_FOLDER_NAME: &str = "root";
pub const DEFAULT_SOCKET_PATH: &str = "/run/firecracker.socket";
pub const LINK_FILES_HANDLER_NAME: &str = LINK_FILES_TO_ROOTFS_HANDLER_NAME;

unsafe extern "C" {
    #[link_name = "chown"]
    fn libc_chown(path: *const c_char, owner: u32, group: u32) -> i32;
}

pub trait HandlersAdapter: Send + Sync {
    fn adapt_handlers(&self, handlers: &mut Handlers) -> Result<()>;
}

#[derive(Clone, Default)]
pub struct JailerConfig {
    pub gid: Option<i32>,
    pub uid: Option<i32>,
    pub id: String,
    pub numa_node: Option<i32>,
    pub exec_file: String,
    pub jailer_binary: Option<String>,
    pub chroot_base_dir: Option<String>,
    pub daemonize: bool,
    pub chroot_strategy: Option<Arc<dyn HandlersAdapter>>,
    pub cgroup_version: Option<String>,
    pub cgroup_args: Vec<String>,
    pub parent_cgroup: Option<String>,
    pub stdin: CommandStdio,
    pub stdout: CommandStdio,
    pub stderr: CommandStdio,
}

impl fmt::Debug for JailerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JailerConfig")
            .field("gid", &self.gid)
            .field("uid", &self.uid)
            .field("id", &self.id)
            .field("numa_node", &self.numa_node)
            .field("exec_file", &self.exec_file)
            .field("jailer_binary", &self.jailer_binary)
            .field("chroot_base_dir", &self.chroot_base_dir)
            .field("daemonize", &self.daemonize)
            .field("cgroup_version", &self.cgroup_version)
            .field("cgroup_args", &self.cgroup_args)
            .field("parent_cgroup", &self.parent_cgroup)
            .field("stdin", &self.stdin)
            .field("stdout", &self.stdout)
            .field("stderr", &self.stderr)
            .finish()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JailerCommandBuilder {
    bin: Option<String>,
    id: String,
    uid: i32,
    gid: i32,
    exec_file: String,
    node: i32,
    chroot_base_dir: Option<String>,
    netns: Option<String>,
    daemonize: bool,
    firecracker_args: Vec<String>,
    cgroup_version: Option<String>,
    cgroup_args: Vec<String>,
    parent_cgroup: Option<String>,
    stdin: CommandStdio,
    stdout: CommandStdio,
    stderr: CommandStdio,
}

impl JailerCommandBuilder {
    pub fn new() -> Self {
        Self::default().with_bin(DEFAULT_JAILER_BIN)
    }

    pub fn args(&self) -> Vec<String> {
        let mut args = vec![
            "--id".to_string(),
            self.id.clone(),
            "--uid".to_string(),
            self.uid.to_string(),
            "--gid".to_string(),
            self.gid.to_string(),
            "--exec-file".to_string(),
            self.exec_file.clone(),
        ];

        let cpuset = get_numa_cpuset(self.node);
        if !cpuset.is_empty() {
            args.extend([
                "--cgroup".to_string(),
                format!("cpuset.mems={}", self.node),
                "--cgroup".to_string(),
                format!("cpuset.cpus={cpuset}"),
            ]);
        }

        for cgroup_arg in &self.cgroup_args {
            args.extend(["--cgroup".to_string(), cgroup_arg.clone()]);
        }

        if let Some(cgroup_version) = &self.cgroup_version {
            args.extend(["--cgroup-version".to_string(), cgroup_version.clone()]);
        }
        if let Some(parent_cgroup) = &self.parent_cgroup {
            args.extend(["--parent-cgroup".to_string(), parent_cgroup.clone()]);
        }
        if let Some(chroot_base_dir) = &self.chroot_base_dir {
            args.extend(["--chroot-base-dir".to_string(), chroot_base_dir.clone()]);
        }
        if let Some(netns) = &self.netns {
            args.extend(["--netns".to_string(), netns.clone()]);
        }
        if self.daemonize {
            args.push("--daemonize".to_string());
        }
        if !self.firecracker_args.is_empty() {
            args.push("--".to_string());
            args.extend(self.firecracker_args.clone());
        }

        args
    }

    pub fn bin(&self) -> &str {
        self.bin.as_deref().unwrap_or(DEFAULT_JAILER_BIN)
    }

    pub fn with_bin(mut self, bin: impl Into<String>) -> Self {
        self.bin = Some(bin.into());
        self
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_uid(mut self, uid: i32) -> Self {
        self.uid = uid;
        self
    }

    pub fn with_gid(mut self, gid: i32) -> Self {
        self.gid = gid;
        self
    }

    pub fn with_exec_file(mut self, exec_file: impl Into<String>) -> Self {
        self.exec_file = exec_file.into();
        self
    }

    pub fn with_numa_node(mut self, node: i32) -> Self {
        self.node = node;
        self
    }

    pub fn with_cgroup_args<I, S>(mut self, cgroup_args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.cgroup_args = cgroup_args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_parent_cgroup(mut self, parent_cgroup: impl Into<String>) -> Self {
        self.parent_cgroup = Some(parent_cgroup.into());
        self
    }

    pub fn with_chroot_base_dir(mut self, path: impl Into<String>) -> Self {
        self.chroot_base_dir = Some(path.into());
        self
    }

    pub fn with_netns(mut self, path: impl Into<String>) -> Self {
        self.netns = Some(path.into());
        self
    }

    pub fn with_daemonize(mut self, daemonize: bool) -> Self {
        self.daemonize = daemonize;
        self
    }

    pub fn with_firecracker_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.firecracker_args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_cgroup_version(mut self, version: impl Into<String>) -> Self {
        self.cgroup_version = Some(version.into());
        self
    }

    pub fn with_stdin(mut self, stdin: CommandStdio) -> Self {
        self.stdin = stdin;
        self
    }

    pub fn with_stdout(mut self, stdout: CommandStdio) -> Self {
        self.stdout = stdout;
        self
    }

    pub fn with_stderr(mut self, stderr: CommandStdio) -> Self {
        self.stderr = stderr;
        self
    }

    pub fn build(&self) -> VMCommand {
        VMCommandBuilder::default()
            .with_bin(self.bin().to_string())
            .with_args(self.args())
            .with_stdin(self.stdin.clone())
            .with_stdout(self.stdout.clone())
            .with_stderr(self.stderr.clone())
            .build()
    }
}

pub fn get_numa_cpuset(node: i32) -> String {
    std::fs::read_to_string(format!("/sys/devices/system/node/node{node}/cpulist"))
        .map(|value| value.trim_end_matches('\n').to_string())
        .unwrap_or_default()
}

fn join_chroot_path(rootfs: &str, socket_path: &str) -> String {
    let sanitized = socket_path.trim_start_matches('/');
    Path::new(rootfs).join(sanitized).display().to_string()
}

fn jailer_rootfs(jailer_cfg: &JailerConfig) -> std::path::PathBuf {
    let chroot_base = jailer_cfg
        .chroot_base_dir
        .clone()
        .unwrap_or_else(|| DEFAULT_JAILER_PATH.to_string());

    Path::new(&chroot_base)
        .join(
            Path::new(&jailer_cfg.exec_file)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("firecracker"),
        )
        .join(&jailer_cfg.id)
        .join(ROOTFS_FOLDER_NAME)
}

fn path_cstring(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_encoded_bytes()).map_err(|_| {
        Error::InvalidConfig(format!(
            "path contains interior NUL byte: {:?}",
            path.display().to_string()
        ))
    })
}

fn chown_path(path: &Path, uid: u32, gid: u32) -> Result<()> {
    let path = path_cstring(path)?;
    if unsafe { libc_chown(path.as_ptr(), uid, gid) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

pub fn jail(machine: &mut Machine, cfg: &mut Config) -> Result<()> {
    let jailer_cfg = cfg
        .jailer_cfg
        .as_ref()
        .ok_or_else(|| Error::InvalidConfig("jailer config was not set for use".into()))?
        .clone();

    let workspace_dir = jailer_rootfs(&jailer_cfg);

    let machine_socket_path = if cfg.socket_path.is_empty() {
        DEFAULT_SOCKET_PATH.to_string()
    } else {
        cfg.socket_path.clone()
    };
    cfg.socket_path = join_chroot_path(&workspace_dir.display().to_string(), &machine_socket_path);

    let mut firecracker_args = seccomp_args(cfg.seccomp.enabled, cfg.seccomp.filter.as_deref());
    firecracker_args.extend(["--api-sock".to_string(), machine_socket_path]);

    let mut builder = JailerCommandBuilder::new()
        .with_id(jailer_cfg.id)
        .with_uid(jailer_cfg.uid.unwrap_or_default())
        .with_gid(jailer_cfg.gid.unwrap_or_default())
        .with_numa_node(jailer_cfg.numa_node.unwrap_or_default())
        .with_exec_file(jailer_cfg.exec_file)
        .with_cgroup_args(jailer_cfg.cgroup_args)
        .with_firecracker_args(firecracker_args)
        .with_daemonize(jailer_cfg.daemonize)
        .with_stdin(jailer_cfg.stdin.clone())
        .with_stdout(jailer_cfg.stdout.clone())
        .with_stderr(jailer_cfg.stderr.clone());

    if let Some(bin) = jailer_cfg.jailer_binary {
        builder = builder.with_bin(bin);
    }
    if let Some(chroot_base_dir) = jailer_cfg.chroot_base_dir {
        builder = builder.with_chroot_base_dir(chroot_base_dir);
    }
    if let Some(cgroup_version) = jailer_cfg.cgroup_version {
        builder = builder.with_cgroup_version(cgroup_version);
    }
    if let Some(parent_cgroup) = jailer_cfg.parent_cgroup {
        builder = builder.with_parent_cgroup(parent_cgroup);
    }
    if let Some(netns) = &cfg.net_ns {
        builder = builder.with_netns(netns.clone());
    }

    machine.command = Some(builder.build());
    if let Some(strategy) = jailer_cfg.chroot_strategy {
        strategy.adapt_handlers(&mut machine.handlers)?;
    }

    Ok(())
}

pub fn link_files_handler(kernel_image_file_name: impl Into<String>) -> Handler {
    let kernel_image_file_name = kernel_image_file_name.into();
    Handler::new(LINK_FILES_TO_ROOTFS_HANDLER_NAME, move |machine| {
        let jailer_cfg = machine
            .cfg
            .jailer_cfg
            .clone()
            .ok_or_else(|| Error::InvalidConfig("jailer config was not set for use".into()))?;
        let rootfs = jailer_rootfs(&jailer_cfg);

        std::fs::hard_link(
            &machine.cfg.kernel_image_path,
            rootfs.join(&kernel_image_file_name),
        )?;

        let initrd_file_name = if let Some(initrd_path) = machine
            .cfg
            .initrd_path
            .as_deref()
            .filter(|path| !path.is_empty())
        {
            let file_name = Path::new(initrd_path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("initrd")
                .to_string();
            std::fs::hard_link(initrd_path, rootfs.join(&file_name))?;
            Some(file_name)
        } else {
            None
        };

        for drive in &mut machine.cfg.drives {
            if let Some(host_path) = drive
                .path_on_host
                .as_deref()
                .filter(|path| !path.is_empty())
            {
                let drive_file_name = Path::new(host_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("drive")
                    .to_string();
                std::fs::hard_link(host_path, rootfs.join(&drive_file_name))?;
                drive.path_on_host = Some(drive_file_name);
            }
        }

        machine.cfg.kernel_image_path = kernel_image_file_name.clone();
        if let Some(initrd_file_name) = initrd_file_name {
            machine.cfg.initrd_path = Some(initrd_file_name);
        }

        for fifo_path in [&mut machine.cfg.log_fifo, &mut machine.cfg.metrics_fifo] {
            if let Some(host_path) = fifo_path.as_deref().filter(|path| !path.is_empty()) {
                let file_name = Path::new(host_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("fifo")
                    .to_string();
                let destination = rootfs.join(&file_name);
                std::fs::hard_link(host_path, &destination)?;
                chown_path(
                    &destination,
                    jailer_cfg.uid.unwrap_or_default() as u32,
                    jailer_cfg.gid.unwrap_or_default() as u32,
                )?;
                *fifo_path = Some(file_name);
            }
        }

        Ok(())
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NaiveChrootStrategy {
    pub kernel_image_path: String,
}

impl NaiveChrootStrategy {
    pub fn new(kernel_image_path: impl Into<String>) -> Self {
        Self {
            kernel_image_path: kernel_image_path.into(),
        }
    }
}

impl HandlersAdapter for NaiveChrootStrategy {
    fn adapt_handlers(&self, handlers: &mut Handlers) -> Result<()> {
        if !handlers.fc_init.has(CREATE_LOG_FILES_HANDLER_NAME) {
            return Err(Error::InvalidConfig(
                "required handler is missing from FcInit's list".into(),
            ));
        }

        let kernel_file_name = Path::new(&self.kernel_image_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("vmlinux")
            .to_string();

        handlers.fc_init = handlers.fc_init.clone().append_after(
            CREATE_LOG_FILES_HANDLER_NAME,
            link_files_handler(kernel_file_name),
        );
        Ok(())
    }
}
