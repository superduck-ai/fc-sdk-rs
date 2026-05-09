use crate::Result;
use crate::client::ClientOps;
use crate::cni::internal::NetlinkOps;
use crate::command_builder::VMCommand;
use crate::config::Config;
use crate::handlers::adapt_handlers_for_snapshot;
use crate::machine::Machine;
use crate::models::MemoryBackend;
use crate::network::CniNetworkOperations;
use crate::snapshot::SnapshotConfig;

pub trait ApplyOpt {
    fn apply(self: Box<Self>, machine: &mut Machine);
}

impl<F> ApplyOpt for F
where
    F: FnOnce(&mut Machine),
{
    fn apply(self: Box<Self>, machine: &mut Machine) {
        (*self)(machine);
    }
}

pub type Opt = Box<dyn ApplyOpt + Send + 'static>;

pub trait ApplySnapshotOpt {
    fn apply(self: Box<Self>, snapshot: &mut SnapshotConfig);
}

impl<F> ApplySnapshotOpt for F
where
    F: FnOnce(&mut SnapshotConfig),
{
    fn apply(self: Box<Self>, snapshot: &mut SnapshotConfig) {
        (*self)(snapshot);
    }
}

pub type SnapshotOpt = Box<dyn ApplySnapshotOpt + Send + 'static>;

impl Machine {
    pub fn new_with_opts(cfg: Config, opts: impl IntoIterator<Item = Opt>) -> Result<Self> {
        let mut machine = Self::new(cfg)?;
        for opt in opts {
            opt.apply(&mut machine);
        }
        Ok(machine)
    }
}

pub fn new_machine(cfg: Config, opts: impl IntoIterator<Item = Opt>) -> Result<Machine> {
    Machine::new_with_opts(cfg, opts)
}

pub fn with_client(client: Box<dyn ClientOps>) -> Opt {
    Box::new(move |machine: &mut Machine| {
        machine.client = client;
    })
}

pub fn with_cni_network_ops(cni_network_ops: Box<dyn CniNetworkOperations + Send + Sync>) -> Opt {
    Box::new(move |machine: &mut Machine| {
        machine.cni_network_ops = cni_network_ops;
    })
}

pub fn with_netlink_ops(netlink_ops: Box<dyn NetlinkOps + Send + Sync>) -> Opt {
    Box::new(move |machine: &mut Machine| {
        machine.netlink_ops = netlink_ops;
    })
}

pub fn with_logger(logger: tracing::Dispatch) -> Opt {
    Box::new(move |machine: &mut Machine| {
        machine.logger = Some(logger);
    })
}

pub fn with_process_runner(command: VMCommand) -> Opt {
    Box::new(move |machine: &mut Machine| {
        machine.command = Some(command);
    })
}

pub fn with_snapshot(
    mem_file_path: impl Into<String>,
    snapshot_path: impl Into<String>,
    opts: impl IntoIterator<Item = SnapshotOpt>,
) -> Opt {
    let mem_file_path = mem_file_path.into();
    let snapshot_path = snapshot_path.into();
    let mut opts = opts.into_iter().collect::<Vec<_>>();

    Box::new(move |machine: &mut Machine| {
        machine.cfg.snapshot = SnapshotConfig::with_paths(&mem_file_path, &snapshot_path);
        for opt in opts.drain(..) {
            opt.apply(&mut machine.cfg.snapshot);
        }
        machine.handlers = adapt_handlers_for_snapshot(machine.handlers.clone());
    })
}

pub fn with_memory_backend(
    backend_type: impl Into<String>,
    backend_path: impl Into<String>,
) -> SnapshotOpt {
    let backend_type = backend_type.into();
    let backend_path = backend_path.into();

    Box::new(move |snapshot: &mut SnapshotConfig| {
        snapshot.mem_backend = Some(MemoryBackend {
            backend_type: Some(backend_type),
            backend_path: Some(backend_path),
        });
    })
}
