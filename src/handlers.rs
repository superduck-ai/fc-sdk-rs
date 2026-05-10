use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::config::MMDSVersion;
use crate::error::{Error, Result};
use crate::machine::Machine;

pub const START_VMM_HANDLER_NAME: &str = "fcinit.StartVMM";
pub const BOOTSTRAP_LOGGING_HANDLER_NAME: &str = "fcinit.BootstrapLogging";
pub const CREATE_LOG_FILES_HANDLER_NAME: &str = "fcinit.CreateLogFilesHandler";
pub const CREATE_MACHINE_HANDLER_NAME: &str = "fcinit.CreateMachine";
pub const CREATE_BOOT_SOURCE_HANDLER_NAME: &str = "fcinit.CreateBootSource";
pub const ATTACH_DRIVES_HANDLER_NAME: &str = "fcinit.AttachDrives";
pub const CREATE_NETWORK_INTERFACES_HANDLER_NAME: &str = "fcinit.CreateNetworkInterfaces";
pub const ADD_VSOCKS_HANDLER_NAME: &str = "fcinit.AddVsocks";
pub const NEW_SET_METADATA_HANDLER_NAME: &str = "fcinit.SetMetadata";
pub const CONFIG_MMDS_HANDLER_NAME: &str = "fcinit.ConfigMmds";
pub const LINK_FILES_TO_ROOTFS_HANDLER_NAME: &str = "fcinit.LinkFilesToRootFS";
pub const SETUP_NETWORK_HANDLER_NAME: &str = "fcinit.SetupNetwork";
pub const SETUP_KERNEL_ARGS_HANDLER_NAME: &str = "fcinit.SetupKernelArgs";
pub const LOAD_SNAPSHOT_HANDLER_NAME: &str = "fcinit.LoadSnapshot";

pub const VALIDATE_CFG_HANDLER_NAME: &str = "validate.Cfg";
pub const VALIDATE_JAILER_CFG_HANDLER_NAME: &str = "validate.JailerCfg";
pub const VALIDATE_NETWORK_CFG_HANDLER_NAME: &str = "validate.NetworkCfg";
pub const VALIDATE_LOAD_SNAPSHOT_CFG_HANDLER_NAME: &str = "validate.LoadSnapshotCfg";

pub type HandlerFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
pub type HandlerFn =
    Arc<dyn for<'a> Fn(&'a mut Machine) -> HandlerFuture<'a> + Send + Sync + 'static>;

#[derive(Clone)]
pub struct Handler {
    pub name: String,
    pub func: HandlerFn,
}

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handler").field("name", &self.name).finish()
    }
}

impl Handler {
    pub fn new(
        name: impl Into<String>,
        func: impl Fn(&mut Machine) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        let func = Arc::new(func);
        Self::new_async(name, move |machine| {
            let result = func(machine);
            Box::pin(async move { result })
        })
    }

    pub fn new_async(
        name: impl Into<String>,
        func: impl for<'a> Fn(&'a mut Machine) -> HandlerFuture<'a> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            func: Arc::new(func),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HandlerList {
    pub list: Vec<Handler>,
}

impl HandlerList {
    pub fn prepend(mut self, handlers: impl IntoIterator<Item = Handler>) -> Self {
        let mut prepended = handlers.into_iter().collect::<Vec<_>>();
        prepended.extend(self.list);
        self.list = prepended;
        self
    }

    pub fn append(mut self, handlers: impl IntoIterator<Item = Handler>) -> Self {
        self.list.extend(handlers);
        self
    }

    pub fn append_after(mut self, after_name: &str, handler: Handler) -> Self {
        let mut new_list = Vec::with_capacity(self.list.len() + 1);
        let mut inserted = false;

        for existing in self.list.iter().cloned() {
            let should_insert = existing.name == after_name;
            new_list.push(existing);
            if should_insert {
                new_list.push(handler.clone());
                inserted = true;
            }
        }

        if inserted {
            self.list = new_list;
        }
        self
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn has(&self, name: &str) -> bool {
        self.list.iter().any(|handler| handler.name == name)
    }

    pub fn swap(mut self, handler: Handler) -> Self {
        self.list = self
            .list
            .into_iter()
            .map(|existing| {
                if existing.name == handler.name {
                    handler.clone()
                } else {
                    existing
                }
            })
            .collect();
        self
    }

    pub fn swappend(self, handler: Handler) -> Self {
        if self.has(&handler.name) {
            self.swap(handler)
        } else {
            self.append([handler])
        }
    }

    pub fn remove(mut self, name: &str) -> Self {
        self.list.retain(|handler| handler.name != name);
        self
    }

    pub fn clear(mut self) -> Self {
        self.list.clear();
        self
    }

    pub async fn run(&self, machine: &mut Machine) -> Result<()> {
        for handler in &self.list {
            (handler.func)(machine).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Handlers {
    pub validation: HandlerList,
    pub fc_init: HandlerList,
}

impl Handlers {
    pub async fn run(&self, machine: &mut Machine) -> Result<()> {
        if !machine.cfg.disable_validation {
            self.validation.run(machine).await?;
        }
        self.fc_init.run(machine).await
    }
}

pub fn config_validation_handler() -> Handler {
    Handler::new(VALIDATE_CFG_HANDLER_NAME, |machine| machine.cfg.validate())
}

pub fn load_snapshot_config_validation_handler() -> Handler {
    Handler::new(VALIDATE_LOAD_SNAPSHOT_CFG_HANDLER_NAME, |machine| {
        if machine.cfg.snapshot.get_mem_backend_path().is_none()
            || machine.cfg.snapshot.snapshot_path.is_none()
        {
            return Err(Error::InvalidConfig(
                "snapshot load requires both memory backend and snapshot path".into(),
            ));
        }
        machine.cfg.validate_load_snapshot()
    })
}

pub fn jailer_config_validation_handler() -> Handler {
    Handler::new(VALIDATE_JAILER_CFG_HANDLER_NAME, |machine| {
        let Some(jailer_cfg) = machine.cfg.jailer_cfg.as_ref() else {
            return Ok(());
        };

        if !machine.cfg.root_drive_present() {
            return Err(Error::InvalidConfig(
                "A root drive must be present in the drive list".into(),
            ));
        }

        if jailer_cfg.chroot_strategy.is_none() {
            return Err(Error::InvalidConfig("ChrootStrategy cannot be nil".into()));
        }

        if jailer_cfg.exec_file.is_empty() {
            return Err(Error::InvalidConfig(
                "exec file must be specified when using jailer mode".into(),
            ));
        }
        if jailer_cfg.id.is_empty() {
            return Err(Error::InvalidConfig(
                "id must be specified when using jailer mode".into(),
            ));
        }
        if jailer_cfg.gid.is_none() {
            return Err(Error::InvalidConfig(
                "GID must be specified when using jailer mode".into(),
            ));
        }
        if jailer_cfg.uid.is_none() {
            return Err(Error::InvalidConfig(
                "UID must be specified when using jailer mode".into(),
            ));
        }
        if jailer_cfg.numa_node.is_none() {
            return Err(Error::InvalidConfig(
                "NUMA node must be specified when using jailer mode".into(),
            ));
        }

        Ok(())
    })
}

pub fn network_config_validation_handler() -> Handler {
    Handler::new(VALIDATE_NETWORK_CFG_HANDLER_NAME, |machine| {
        machine.cfg.validate_network()
    })
}

pub fn start_vmm_handler() -> Handler {
    Handler::new_async(START_VMM_HANDLER_NAME, |machine| {
        Box::pin(async move { machine.start_vmm().await })
    })
}

pub fn create_log_files_handler() -> Handler {
    Handler::new(CREATE_LOG_FILES_HANDLER_NAME, |machine| {
        machine.create_log_files()
    })
}

pub fn bootstrap_logging_handler() -> Handler {
    Handler::new_async(BOOTSTRAP_LOGGING_HANDLER_NAME, |machine| {
        Box::pin(async move {
            machine.setup_logging().await?;
            machine.setup_metrics().await
        })
    })
}

pub fn create_machine_handler() -> Handler {
    Handler::new_async(CREATE_MACHINE_HANDLER_NAME, |machine| {
        Box::pin(async move { machine.create_machine().await })
    })
}

pub fn create_boot_source_handler() -> Handler {
    Handler::new_async(CREATE_BOOT_SOURCE_HANDLER_NAME, |machine| {
        Box::pin(async move {
            let image = machine.cfg.kernel_image_path.clone();
            let initrd = machine.cfg.initrd_path.clone();
            let kernel_args = machine.cfg.kernel_args.clone();
            machine
                .create_boot_source(&image, initrd.as_deref(), Some(&kernel_args))
                .await
        })
    })
}

pub fn attach_drives_handler() -> Handler {
    Handler::new_async(ATTACH_DRIVES_HANDLER_NAME, |machine| {
        Box::pin(async move { machine.attach_drives().await })
    })
}

pub fn create_network_interfaces_handler() -> Handler {
    Handler::new_async(CREATE_NETWORK_INTERFACES_HANDLER_NAME, |machine| {
        Box::pin(async move { machine.create_network_interfaces().await })
    })
}

pub fn add_vsocks_handler() -> Handler {
    Handler::new_async(ADD_VSOCKS_HANDLER_NAME, |machine| {
        Box::pin(async move { machine.add_vsocks().await })
    })
}

pub fn new_set_metadata_handler(metadata: serde_json::Value) -> Handler {
    Handler::new_async(NEW_SET_METADATA_HANDLER_NAME, move |machine| {
        let metadata = metadata.clone();
        Box::pin(async move { machine.set_metadata(&metadata).await })
    })
}

pub fn config_mmds_handler() -> Handler {
    Handler::new_async(CONFIG_MMDS_HANDLER_NAME, |machine| {
        Box::pin(async move {
            let address = machine.cfg.mmds_address;
            let ifaces = machine.cfg.network_interfaces.clone();
            let version = machine.cfg.mmds_version;
            machine.set_mmds_config(address, &ifaces, version).await
        })
    })
}

pub fn setup_network_handler() -> Handler {
    Handler::new(SETUP_NETWORK_HANDLER_NAME, |machine| {
        machine.setup_network()
    })
}

pub fn setup_kernel_args_handler() -> Handler {
    Handler::new(SETUP_KERNEL_ARGS_HANDLER_NAME, |machine| {
        machine.setup_kernel_args()
    })
}

pub fn load_snapshot_handler() -> Handler {
    Handler::new_async(LOAD_SNAPSHOT_HANDLER_NAME, |machine| {
        Box::pin(async move { machine.load_snapshot().await })
    })
}

pub fn default_handlers() -> Handlers {
    Handlers {
        validation: HandlerList::default().append([network_config_validation_handler()]),
        fc_init: HandlerList::default().append([
            setup_network_handler(),
            setup_kernel_args_handler(),
            start_vmm_handler(),
            create_log_files_handler(),
            bootstrap_logging_handler(),
            create_machine_handler(),
            create_boot_source_handler(),
            attach_drives_handler(),
            create_network_interfaces_handler(),
            add_vsocks_handler(),
            config_mmds_handler(),
        ]),
    }
}

pub fn adapt_handlers_for_snapshot(mut handlers: Handlers) -> Handlers {
    for name in [
        SETUP_KERNEL_ARGS_HANDLER_NAME,
        CREATE_MACHINE_HANDLER_NAME,
        CREATE_BOOT_SOURCE_HANDLER_NAME,
        ATTACH_DRIVES_HANDLER_NAME,
        CREATE_NETWORK_INTERFACES_HANDLER_NAME,
        CONFIG_MMDS_HANDLER_NAME,
    ] {
        handlers.fc_init = handlers.fc_init.remove(name);
    }

    handlers.validation = handlers
        .validation
        .remove(VALIDATE_CFG_HANDLER_NAME)
        .remove(VALIDATE_LOAD_SNAPSHOT_CFG_HANDLER_NAME)
        .append([load_snapshot_config_validation_handler()]);
    handlers.fc_init = handlers
        .fc_init
        .remove(LOAD_SNAPSHOT_HANDLER_NAME)
        .append([load_snapshot_handler()]);
    handlers
}

pub fn version_to_model(version: MMDSVersion) -> &'static str {
    match version {
        MMDSVersion::V1 => crate::models::MMDS_VERSION_V1,
        MMDSVersion::V2 => crate::models::MMDS_VERSION_V2,
    }
}
