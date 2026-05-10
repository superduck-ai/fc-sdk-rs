use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::ffi::{CString, c_char, c_void};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

use crate::client::{Client, ClientOps, NoopClient};
use crate::cni::internal::{NetlinkOps, RealNetlinkOps};
use crate::command_builder::{DEFAULT_FC_BIN, VMCommand, VMCommandBuilder, seccomp_args};
use crate::config::{
    Config, DEFAULT_FIRECRACKER_INIT_TIMEOUT_SECONDS, DEFAULT_NET_NS_DIR,
    FIRECRACKER_INIT_TIMEOUT_ENV, MMDSVersion,
};
use crate::error::{Error, Result};
use crate::handlers::{
    Handlers, adapt_handlers_for_snapshot, config_validation_handler, default_handlers,
    jailer_config_validation_handler,
};
use crate::jailer::jail;
use crate::kernelargs::parse_kernel_args;
use crate::models::{
    Balloon, BalloonStats, BalloonStatsUpdate, BalloonUpdate, BootSource, FullVmConfiguration,
    InstanceActionInfo, InstanceInfo, Logger, MachineConfiguration, Metrics, MmdsConfig,
    NetworkInterfaceModel, PartialDrive, PartialNetworkInterface, RateLimiter,
    SnapshotCreateParams, SnapshotLoadParams, Vm, VsockModel,
};
use crate::network::{CleanupFn, CniNetworkOperations, RealCniNetworkOperations};
use crate::network::{NetworkInterface, NetworkInterfaces};
use crate::utils::env_value_or_default_int;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateLimiterSet {
    pub in_rate_limiter: Option<RateLimiter>,
    pub out_rate_limiter: Option<RateLimiter>,
}

static SIGNAL_PIPE_WRITE_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
static SIGNAL_FORWARD_MANAGER: OnceLock<Mutex<SignalForwardManager>> = OnceLock::new();

const SIG_ERR: usize = usize::MAX;
const SIGTERM_SIGNAL: i32 = 15;
const ESRCH_ERRNO: i32 = 3;
const CLONE_NEWNET: i32 = 0x4000_0000;

extern "C" fn forward_signal_handler(signal: i32) {
    let fd = SIGNAL_PIPE_WRITE_FD.load(Ordering::Relaxed);
    if fd < 0 || signal <= 0 || signal > u8::MAX as i32 {
        return;
    }

    let buffer = [signal as u8];
    unsafe {
        let _ = libc_write_signal(fd, buffer.as_ptr().cast(), buffer.len());
    }
}

struct SignalForwarder {
    registration_id: u64,
}

impl SignalForwarder {
    fn install(signals: &[i32], child_pid: u32) -> Result<Option<Self>> {
        if signals.is_empty() {
            return Ok(None);
        }

        let registration_id = SignalForwardManager::register(signals, child_pid)?;
        Ok(Some(Self { registration_id }))
    }

    fn shutdown(self) {
        SignalForwardManager::unregister(self.registration_id);
    }
}

#[derive(Debug, Clone)]
struct SignalRegistration {
    child_pid: u32,
    signals: Vec<i32>,
}

#[derive(Default)]
struct SignalForwardManager {
    next_registration_id: u64,
    registrations: HashMap<u64, SignalRegistration>,
    signal_refs: HashMap<i32, usize>,
    old_handlers: HashMap<i32, usize>,
    write_fd: Option<OwnedFd>,
    worker: Option<JoinHandle<()>>,
}

struct SignalForwardWorkerShutdown {
    write_fd: OwnedFd,
    worker: JoinHandle<()>,
}

impl SignalForwardWorkerShutdown {
    fn shutdown(self) {
        let sentinel = [0u8];
        unsafe {
            let _ = libc_write_signal(
                self.write_fd.as_raw_fd(),
                sentinel.as_ptr().cast(),
                sentinel.len(),
            );
        }
        drop(self.write_fd);
        let _ = self.worker.join();
    }
}

impl SignalForwardManager {
    fn global() -> &'static Mutex<Self> {
        SIGNAL_FORWARD_MANAGER.get_or_init(|| Mutex::new(Self::default()))
    }

    fn register(signals: &[i32], child_pid: u32) -> Result<u64> {
        let mut shutdown = None;
        let registration = {
            let mut manager = Self::global()
                .lock()
                .map_err(|_| Error::Process("signal forwarding manager mutex poisoned".into()))?;
            let result = manager.register_inner(signals, child_pid);
            if result.is_err() {
                shutdown = manager.take_shutdown_if_idle();
            }
            result
        };

        if let Some(shutdown) = shutdown {
            shutdown.shutdown();
        }

        registration
    }

    fn unregister(registration_id: u64) {
        let shutdown = match Self::global().lock() {
            Ok(mut manager) => {
                manager.unregister_inner(registration_id);
                manager.take_shutdown_if_idle()
            }
            Err(_) => None,
        };

        if let Some(shutdown) = shutdown {
            shutdown.shutdown();
        }
    }

    fn dispatch_signal(signal: i32) {
        let targets = match Self::global().lock() {
            Ok(manager) => manager
                .registrations
                .values()
                .filter(|registration| registration.signals.contains(&signal))
                .map(|registration| registration.child_pid)
                .collect::<Vec<_>>(),
            Err(_) => return,
        };

        for pid in targets {
            unsafe {
                let _ = libc_kill(pid as i32, signal);
            }
        }
    }

    fn register_inner(&mut self, signals: &[i32], child_pid: u32) -> Result<u64> {
        self.ensure_worker()?;

        let mut applied_signals = Vec::with_capacity(signals.len());
        for &signal in signals {
            if signal <= 0 || signal > u8::MAX as i32 {
                self.release_signals(&applied_signals);
                return Err(Error::Process(format!(
                    "signal {signal} cannot be forwarded"
                )));
            }

            let ref_count = self.signal_refs.entry(signal).or_default();
            if *ref_count == 0 {
                let previous =
                    unsafe { libc_signal(signal, forward_signal_handler as *const () as usize) };
                if previous == SIG_ERR {
                    let error = std::io::Error::last_os_error().into();
                    self.release_signals(&applied_signals);
                    return Err(error);
                }
                self.old_handlers.insert(signal, previous);
            }

            *ref_count += 1;
            applied_signals.push(signal);
        }

        let registration_id = self.next_registration_id;
        self.next_registration_id = self.next_registration_id.wrapping_add(1);
        self.registrations.insert(
            registration_id,
            SignalRegistration {
                child_pid,
                signals: signals.to_vec(),
            },
        );
        Ok(registration_id)
    }

    fn unregister_inner(&mut self, registration_id: u64) {
        if let Some(registration) = self.registrations.remove(&registration_id) {
            self.release_signals(&registration.signals);
        }
    }

    fn ensure_worker(&mut self) -> Result<()> {
        if self.worker.is_some() {
            return Ok(());
        }

        let mut fds = [0i32; 2];
        if unsafe { libc_pipe(fds.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        let read_fd = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let write_fd = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        SIGNAL_PIPE_WRITE_FD.store(write_fd.as_raw_fd(), Ordering::SeqCst);

        let worker = thread::spawn(move || {
            let mut buffer = [0u8; 64];
            loop {
                let read = unsafe {
                    libc_read_signal(
                        read_fd.as_raw_fd(),
                        buffer.as_mut_ptr().cast(),
                        buffer.len(),
                    )
                };

                if read < 0 {
                    continue;
                }

                if read == 0 {
                    return;
                }

                for signal in &buffer[..read as usize] {
                    if *signal == 0 {
                        return;
                    }
                    SignalForwardManager::dispatch_signal(*signal as i32);
                }
            }
        });

        self.write_fd = Some(write_fd);
        self.worker = Some(worker);
        Ok(())
    }

    fn release_signals(&mut self, signals: &[i32]) {
        for &signal in signals.iter().rev() {
            let Some(ref_count) = self.signal_refs.get_mut(&signal) else {
                continue;
            };

            *ref_count -= 1;
            if *ref_count > 0 {
                continue;
            }

            self.signal_refs.remove(&signal);
            if let Some(previous) = self.old_handlers.remove(&signal) {
                unsafe {
                    let _ = libc_signal(signal, previous);
                }
            }
        }
    }

    fn take_shutdown_if_idle(&mut self) -> Option<SignalForwardWorkerShutdown> {
        if !self.registrations.is_empty() || !self.signal_refs.is_empty() {
            return None;
        }

        let write_fd = self.write_fd.take()?;
        let worker = self.worker.take()?;
        SIGNAL_PIPE_WRITE_FD.store(-1, Ordering::SeqCst);
        Some(SignalForwardWorkerShutdown { write_fd, worker })
    }
}

unsafe extern "C" {
    #[link_name = "pipe"]
    fn libc_pipe(fds: *mut i32) -> i32;
    #[link_name = "mkfifo"]
    fn libc_mkfifo(path: *const c_char, mode: u32) -> i32;
    #[link_name = "setns"]
    fn libc_setns(fd: i32, nstype: i32) -> i32;
    #[link_name = "signal"]
    fn libc_signal(signal: i32, handler: usize) -> usize;
    #[link_name = "read"]
    fn libc_read_signal(fd: i32, buf: *mut c_void, count: usize) -> isize;
    #[link_name = "write"]
    fn libc_write_signal(fd: i32, buf: *const c_void, count: usize) -> isize;
    #[link_name = "kill"]
    fn libc_kill(pid: i32, signal: i32) -> i32;
}

pub struct Machine {
    pub cfg: Config,
    pub handlers: Handlers,
    pub client: Box<dyn ClientOps>,
    pub cni_network_ops: Box<dyn CniNetworkOperations + Send + Sync>,
    pub netlink_ops: Box<dyn NetlinkOps + Send + Sync>,
    pub command: Option<VMCommand>,
    pub process: Option<Child>,
    pub machine_config: MachineConfiguration,
    pub logger: Option<tracing::Dispatch>,
    pub exit_event: Arc<AtomicBool>,
    signal_forwarder: Option<SignalForwarder>,
    cleanup_funcs: Vec<CleanupFn>,
    cleanup_done: bool,
    pub started: bool,
    pub fatal_err: Option<Error>,
}

impl Machine {
    pub fn new(cfg: Config) -> Result<Self> {
        let mut machine = Self::new_with_client(cfg, Box::new(NoopClient))?;
        machine.client = Box::new(Client::new(machine.cfg.socket_path.clone()));
        Ok(machine)
    }

    pub fn new_with_client(mut cfg: Config, client: Box<dyn ClientOps>) -> Result<Self> {
        if cfg.vmid.is_empty() {
            cfg.vmid = uuid::Uuid::new_v4().to_string();
        }

        if cfg.forward_signals.is_none() {
            cfg.forward_signals = Some(cfg.normalized_forward_signals());
        }

        let mut handlers = default_handlers();
        if cfg.jailer_cfg.is_some() {
            handlers.validation = handlers
                .validation
                .append([jailer_config_validation_handler()]);
        } else {
            handlers.validation = handlers.validation.append([config_validation_handler()]);
        }

        if cfg.has_snapshot() {
            handlers = adapt_handlers_for_snapshot(handlers);
        }

        if cfg.net_ns.is_none() && cfg.network_interfaces.cni_interface().is_some() {
            cfg.net_ns = Some(format!("{DEFAULT_NET_NS_DIR}/{}", cfg.vmid));
        }

        let mut machine = Self {
            machine_config: cfg.machine_cfg.clone(),
            cfg,
            handlers,
            client,
            cni_network_ops: Box::new(RealCniNetworkOperations),
            netlink_ops: Box::new(RealNetlinkOps),
            command: None,
            process: None,
            started: false,
            logger: None,
            exit_event: Arc::new(AtomicBool::new(false)),
            signal_forwarder: None,
            cleanup_funcs: Vec::new(),
            cleanup_done: false,
            fatal_err: None,
        };

        if machine.cfg.jailer_cfg.is_some() {
            let mut cfg = std::mem::take(&mut machine.cfg);
            jail(&mut machine, &mut cfg)?;
            machine.cfg = cfg;
        } else {
            let mut args = vec!["--id".to_string(), machine.cfg.vmid.clone()];
            args.extend(seccomp_args(
                machine.cfg.seccomp.enabled,
                machine.cfg.seccomp.filter.as_deref(),
            ));
            machine.command = Some(
                VMCommandBuilder::default()
                    .with_bin(DEFAULT_FC_BIN)
                    .with_socket_path(machine.cfg.socket_path.clone())
                    .with_args(args)
                    .build(),
            );
        }

        Ok(machine)
    }

    pub fn log_file(&self) -> Option<&str> {
        self.cfg.log_fifo.as_deref()
    }

    pub fn logger(&self) -> Option<&tracing::Dispatch> {
        self.logger.as_ref()
    }

    pub fn log_level(&self) -> Option<&str> {
        self.cfg.log_level.as_deref()
    }

    pub async fn start(&mut self) -> Result<()> {
        if self.started {
            return Err(Error::AlreadyStarted);
        }
        self.started = true;
        let handlers = self.handlers.clone();
        if let Err(error) = handlers.run(self).await {
            return Err(self.abort_start(error).await);
        }

        if let Err(error) = self.start_instance().await {
            return Err(self.abort_start(error).await);
        }

        Ok(())
    }

    pub async fn start_vmm(&mut self) -> Result<()> {
        let command = self
            .command
            .clone()
            .ok_or_else(|| Error::Process("machine command is not configured".into()))?;

        self.exit_event.store(false, Ordering::SeqCst);

        let mut child = Command::new(&command.bin);
        child.args(&command.args);
        child.stdin(Self::command_stdio(&command.stdin, true)?);
        child.stdout(Self::command_stdio(&command.stdout, false)?);
        child.stderr(Self::command_stdio(&command.stderr, false)?);
        self.configure_command_netns(&mut child)?;

        let spawned = child
            .spawn()
            .map_err(|error| Error::Process(error.to_string()))?;
        self.process = Some(spawned);
        self.push_cleanup_func(Box::new({
            let socket_path = self.cfg.socket_path.clone();
            move || match std::fs::remove_file(&socket_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        }));

        if let Err(error) = self.install_signal_forwarder() {
            return Err(self.abort_start(error).await);
        }

        let init_timeout_secs = env_value_or_default_int(
            FIRECRACKER_INIT_TIMEOUT_ENV,
            DEFAULT_FIRECRACKER_INIT_TIMEOUT_SECONDS as i32,
        ) as u64;

        let wait_result = self
            .wait_for_vmm_ready(Duration::from_secs(init_timeout_secs))
            .await;

        if let Err(error) = wait_result {
            return Err(self.abort_start(error).await);
        }

        Ok(())
    }

    pub async fn stop_vmm(&mut self) -> Result<()> {
        let stop_result = self.send_sigterm_to_vmm();

        if stop_result.is_ok() && (self.process.is_some() || !self.cleanup_done) {
            if let Err(error) = self.finalize_process_exit().await {
                self.remember_terminal_error(error);
            }
        }

        stop_result
    }

    pub async fn wait(&mut self) -> Result<()> {
        if self.process.is_some() || !self.cleanup_done {
            if let Err(error) = self.finalize_process_exit().await {
                self.remember_terminal_error(error);
            }
        }

        if let Some(error) = self.fatal_err.as_ref() {
            return Err(Self::copy_error(error));
        }

        Ok(())
    }

    pub fn pid(&mut self) -> Result<u32> {
        let pid = self
            .process
            .as_ref()
            .and_then(Child::id)
            .ok_or_else(|| Error::Process("machine is not running".into()))?;

        let exited_status = if let Some(child) = self.process.as_mut() {
            child.try_wait()?
        } else {
            None
        };

        if let Some(status) = exited_status {
            self.process.take();
            if let Err(error) = self.finalize_observed_exit_status(status) {
                self.remember_terminal_error(error);
            }
            return Err(Error::Process("machine process has exited".into()));
        }

        Ok(pid)
    }

    pub fn default_net_ns_path(&self) -> String {
        format!("{}/{}", DEFAULT_NET_NS_DIR, self.cfg.vmid)
    }

    pub fn signal_exit(&self) {
        self.exit_event.store(true, Ordering::SeqCst);
    }

    fn send_sigterm_to_vmm(&mut self) -> Result<()> {
        let Some(child) = self.process.as_mut() else {
            return Ok(());
        };

        if child.try_wait()?.is_some() {
            return Ok(());
        }

        let Some(pid) = child.id() else {
            return Ok(());
        };

        if unsafe { libc_kill(pid as i32, SIGTERM_SIGNAL) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(ESRCH_ERRNO) {
                return Err(error.into());
            }
        }

        Ok(())
    }

    fn finalize_exit_result(&mut self, exit_result: Result<()>) -> Result<()> {
        self.shutdown_signal_forwarder();
        self.signal_exit();
        let cleanup_result = self.do_cleanup();

        if let Err(error) = exit_result {
            return Err(Self::join_errors(error, cleanup_result.err()));
        }

        if let Err(error) = cleanup_result {
            return Err(error);
        }

        Ok(())
    }

    async fn finalize_process_exit(&mut self) -> Result<()> {
        let wait_result = self.wait_for_process_exit().await;
        self.finalize_exit_result(wait_result)
    }

    fn finalize_observed_exit_status(&mut self, status: ExitStatus) -> Result<()> {
        self.finalize_exit_result(Self::exit_status_result(status))
    }

    fn remember_terminal_error(&mut self, error: Error) {
        if self.fatal_err.is_none() {
            self.fatal_err = Some(Self::copy_error(&error));
        }
    }

    async fn wait_for_process_exit(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            Self::exit_status_result(child.wait().await?)
        } else {
            Ok(())
        }
    }

    fn exit_status_result(status: ExitStatus) -> Result<()> {
        if status.success() {
            Ok(())
        } else {
            Err(Error::Process(format!("firecracker exited: {status}")))
        }
    }

    fn command_stdio(spec: &crate::command_builder::CommandStdio, stdin: bool) -> Result<Stdio> {
        Ok(match spec {
            crate::command_builder::CommandStdio::Inherit => Stdio::inherit(),
            crate::command_builder::CommandStdio::Null => Stdio::null(),
            crate::command_builder::CommandStdio::Path(path) => {
                let file = if stdin {
                    std::fs::OpenOptions::new().read(true).open(path)?
                } else {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .write(true)
                        .open(path)?
                };
                Stdio::from(file)
            }
        })
    }

    fn configure_command_netns(&self, child: &mut Command) -> Result<()> {
        let Some(net_ns_path) = self.cfg.net_ns.as_deref() else {
            return Ok(());
        };

        if self.cfg.jailer_cfg.is_some() {
            return Ok(());
        }

        let netns_handle = std::fs::File::open(net_ns_path)?;
        unsafe {
            child.pre_exec(move || {
                if libc_setns(netns_handle.as_raw_fd(), CLONE_NEWNET) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        Ok(())
    }

    pub fn create_log_files(&mut self) -> Result<()> {
        let metrics_fifo = self.cfg.metrics_fifo.clone();
        let metrics_path = self.cfg.metrics_path.clone();
        let log_fifo = self.cfg.log_fifo.clone();
        let log_path = self.cfg.log_path.clone();
        let fifo_log_writer = self.cfg.fifo_log_writer.clone();

        self.create_fifo_or_file(metrics_fifo.as_deref(), metrics_path.as_deref())?;
        self.create_fifo_or_file(log_fifo.as_deref(), log_path.as_deref())?;

        if let (Some(log_fifo), Some(writer)) =
            (log_fifo.filter(|path| !path.is_empty()), fifo_log_writer)
        {
            if let Err(error) = self.capture_fifo_to_file(&log_fifo, writer) {
                self.warn(&format!(
                    "capture_fifo_to_file({log_fifo:?}) returned {error}. continuing anyway."
                ));
            }
        }

        Ok(())
    }

    fn warn(&self, message: &str) {
        if let Some(dispatch) = self.logger.as_ref() {
            tracing::dispatcher::with_default(dispatch, || tracing::warn!("{message}"));
        }
    }

    fn create_fifo_or_file(
        &mut self,
        fifo_path: Option<&str>,
        file_path: Option<&str>,
    ) -> Result<()> {
        if let Some(fifo_path) = fifo_path.filter(|path| !path.is_empty()) {
            Self::create_fifo(fifo_path)?;
            self.push_cleanup_func(Box::new({
                let fifo_path = fifo_path.to_string();
                move || match std::fs::remove_file(&fifo_path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(error.into()),
                }
            }));
        } else if let Some(file_path) = file_path.filter(|path| !path.is_empty()) {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .write(true)
                .open(file_path)?;
        }

        Ok(())
    }

    fn create_fifo(path: &str) -> Result<()> {
        let path = Self::path_cstring(path)?;
        if unsafe { libc_mkfifo(path.as_ptr(), 0o700) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn path_cstring(path: &str) -> Result<CString> {
        CString::new(path).map_err(|_| {
            Error::InvalidConfig(format!("path contains interior NUL byte: {:?}", path))
        })
    }

    async fn wait_for_vmm_ready(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;

        loop {
            let exited_status = if let Some(child) = self.process.as_mut() {
                child.try_wait()?
            } else {
                None
            };

            if let Some(status) = exited_status {
                self.process.take();
                return Err(Error::Process(format!(
                    "firecracker exited before creating API socket: {status}"
                )));
            }

            if tokio::fs::metadata(&self.cfg.socket_path).await.is_ok()
                && self.client.get_machine_configuration().await.is_ok()
            {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out while waiting for the Firecracker VMM to become reachable",
                )
                .into());
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn push_cleanup_func(&mut self, cleanup: CleanupFn) {
        self.cleanup_done = false;
        self.cleanup_funcs.push(cleanup);
    }

    fn push_cleanup_funcs(&mut self, cleanups: impl IntoIterator<Item = CleanupFn>) {
        self.cleanup_done = false;
        self.cleanup_funcs.extend(cleanups);
    }

    fn do_cleanup(&mut self) -> Result<()> {
        if self.cleanup_done {
            return Ok(());
        }

        self.cleanup_done = true;
        let mut errors = Vec::new();
        while let Some(cleanup) = self.cleanup_funcs.pop() {
            if let Err(error) = cleanup() {
                errors.push(error.to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::Process(format!(
                "cleanup failed: {}",
                errors.join("; ")
            )))
        }
    }

    fn join_errors(primary: Error, secondary: Option<Error>) -> Error {
        if let Some(secondary) = secondary {
            Error::Process(format!("{primary}; cleanup error: {secondary}"))
        } else {
            primary
        }
    }

    fn copy_error(error: &Error) -> Error {
        match error {
            Error::Io(error) => Error::Io(std::io::Error::new(error.kind(), error.to_string())),
            Error::Json(error) => Error::Process(error.to_string()),
            Error::InvalidConfig(message) => Error::InvalidConfig(message.clone()),
            Error::Process(message) => Error::Process(message.clone()),
            Error::Api { status, body } => Error::Api {
                status: *status,
                body: body.clone(),
            },
            Error::AlreadyStarted => Error::AlreadyStarted,
        }
    }

    async fn abort_start(&mut self, error: Error) -> Error {
        self.signal_exit();

        let stop_error = if self.process.is_some() {
            self.stop_vmm().await.err()
        } else {
            None
        };
        self.shutdown_signal_forwarder();
        let wait_error = self.wait_for_process_exit().await.err().map(Error::from);
        let cleanup_error = self.do_cleanup().err();

        let final_error = if stop_error.is_none() && wait_error.is_none() && cleanup_error.is_none()
        {
            error
        } else {
            let mut messages = vec![error.to_string()];
            if let Some(stop_error) = stop_error {
                messages.push(format!("stop error: {stop_error}"));
            }
            if let Some(wait_error) = wait_error {
                messages.push(format!("wait error: {wait_error}"));
            }
            if let Some(cleanup_error) = cleanup_error {
                messages.push(format!("cleanup error: {cleanup_error}"));
            }
            Error::Process(messages.join("; "))
        };

        let return_error = Self::copy_error(&final_error);
        self.fatal_err = Some(final_error);
        return_error
    }

    fn install_signal_forwarder(&mut self) -> Result<()> {
        let Some(signals) = self.cfg.forward_signals.as_deref() else {
            return Ok(());
        };

        if self.signal_forwarder.is_some() || signals.is_empty() {
            return Ok(());
        }

        let child_pid = self
            .process
            .as_ref()
            .ok_or_else(|| Error::Process("machine is not running".into()))?
            .id()
            .ok_or_else(|| Error::Process("machine process has exited".into()))?;

        self.signal_forwarder = SignalForwarder::install(signals, child_pid)?;

        Ok(())
    }

    fn shutdown_signal_forwarder(&mut self) {
        if let Some(forwarder) = self.signal_forwarder.take() {
            forwarder.shutdown();
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        #[cfg(target_arch = "aarch64")]
        {
            return self.stop_vmm().await;
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            self.client
                .create_sync_action(&InstanceActionInfo {
                    action_type: Some(crate::models::INSTANCE_ACTION_SEND_CTRL_ALT_DEL.to_string()),
                })
                .await
        }
    }

    pub fn setup_network(&mut self) -> Result<()> {
        if self.cfg.network_interfaces.cni_interface().is_none() {
            return Ok(());
        }

        let net_ns_path = self
            .cfg
            .net_ns
            .clone()
            .unwrap_or_else(|| self.default_net_ns_path());
        self.cfg.net_ns = Some(net_ns_path.clone());

        let cleanup_funcs = self.cfg.network_interfaces.setup_cni(
            &self.cfg.vmid,
            &net_ns_path,
            &*self.cni_network_ops,
            &*self.netlink_ops,
        )?;
        self.push_cleanup_funcs(cleanup_funcs);
        Ok(())
    }

    pub async fn wait_for_socket(
        &mut self,
        timeout: Duration,
        exitchan: &mpsc::Receiver<Error>,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;

        loop {
            match exitchan.try_recv() {
                Ok(error) => return Err(error),
                Err(mpsc::TryRecvError::Disconnected) | Err(mpsc::TryRecvError::Empty) => {}
            }

            if tokio::fs::metadata(&self.cfg.socket_path).await.is_ok()
                && self.client.get_machine_configuration().await.is_ok()
            {
                return Ok(());
            }

            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "deadline exceeded while waiting for firecracker socket",
                )
                .into());
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub fn setup_kernel_args(&mut self) -> Result<()> {
        let mut kernel_args = parse_kernel_args(&self.cfg.kernel_args);
        if let Some(static_ip_interface) = self.cfg.network_interfaces.static_ip_interface() {
            if let Some(ip_configuration) = static_ip_interface
                .static_configuration
                .as_ref()
                .and_then(|config| config.ip_configuration.as_ref())
            {
                kernel_args.insert("ip".to_string(), Some(ip_configuration.ip_boot_param()));
            }
        }
        self.cfg.kernel_args = kernel_args.to_string();
        Ok(())
    }

    pub async fn setup_logging(&mut self) -> Result<()> {
        let path = self
            .cfg
            .log_fifo
            .clone()
            .or_else(|| self.cfg.log_path.clone());

        let Some(path) = path else {
            return Ok(());
        };

        let logger = Logger {
            log_path: Some(path),
            level: self.cfg.log_level.clone(),
            show_level: Some(true),
            show_log_origin: Some(false),
        };
        self.client.put_logger(&logger).await
    }

    pub async fn setup_metrics(&mut self) -> Result<()> {
        let path = self
            .cfg
            .metrics_fifo
            .clone()
            .or_else(|| self.cfg.metrics_path.clone());

        let Some(path) = path else {
            return Ok(());
        };

        self.client
            .put_metrics(&Metrics {
                metrics_path: Some(path),
            })
            .await
    }

    pub fn capture_fifo_to_file<W>(&self, fifo_path: &str, writer: W) -> Result<()>
    where
        W: Write + Send + 'static,
    {
        let (done, _recv) = mpsc::channel();
        self.capture_fifo_to_file_with_channel(fifo_path, writer, done)
    }

    pub fn capture_fifo_to_file_with_channel<W>(
        &self,
        fifo_path: &str,
        mut writer: W,
        done: mpsc::Sender<std::io::Result<()>>,
    ) -> Result<()>
    where
        W: Write + Send + 'static,
    {
        const O_NONBLOCK: i32 = 0o00004000;

        let fifo_pipe = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(fifo_path)?;

        let fifo_path = fifo_path.to_string();
        let exit_event = Arc::clone(&self.exit_event);
        thread::spawn(move || {
            let mut fifo_pipe = fifo_pipe;
            let mut buf = [0u8; 8192];
            let mut seen_data = false;
            let mut idle_since = None::<Instant>;

            loop {
                if exit_event.load(Ordering::SeqCst) {
                    break;
                }

                match fifo_pipe.read(&mut buf) {
                    Ok(0) => {
                        if seen_data {
                            let idle_start = idle_since.get_or_insert_with(Instant::now);
                            if idle_start.elapsed() >= Duration::from_millis(100) {
                                break;
                            }
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Ok(read) => {
                        seen_data = true;
                        idle_since = None;
                        if let Err(error) = writer.write_all(&buf[..read]) {
                            let _ = done.send(Err(error));
                            let _ = std::fs::remove_file(&fifo_path);
                            return;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if seen_data {
                            let idle_start = idle_since.get_or_insert_with(Instant::now);
                            if idle_start.elapsed() >= Duration::from_millis(100) {
                                break;
                            }
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => {
                        let _ = done.send(Err(error));
                        let _ = std::fs::remove_file(&fifo_path);
                        return;
                    }
                }
            }

            let _ = std::fs::remove_file(&fifo_path);
            let _ = done.send(Ok(()));
        });

        Ok(())
    }

    pub async fn refresh_machine_configuration(&mut self) -> Result<()> {
        self.machine_config = self.client.get_machine_configuration().await?;
        Ok(())
    }

    pub async fn create_machine(&mut self) -> Result<()> {
        self.client
            .put_machine_configuration(&self.cfg.machine_cfg)
            .await?;
        self.refresh_machine_configuration().await
    }

    pub async fn create_boot_source(
        &mut self,
        image_path: &str,
        initrd_path: Option<&str>,
        kernel_args: Option<&str>,
    ) -> Result<()> {
        self.client
            .put_guest_boot_source(&BootSource {
                kernel_image_path: Some(image_path.to_string()),
                initrd_path: initrd_path.map(ToOwned::to_owned),
                boot_args: kernel_args.map(ToOwned::to_owned),
            })
            .await
    }

    pub async fn create_network_interface(
        &mut self,
        iface: &NetworkInterface,
        index: usize,
    ) -> Result<()> {
        let static_config = iface.static_configuration.as_ref().ok_or_else(|| {
            Error::InvalidConfig("invalid nil state for network interface".into())
        })?;
        let iface_id = index.to_string();
        self.client
            .put_guest_network_interface_by_id(
                &iface_id,
                &NetworkInterfaceModel {
                    iface_id: Some(iface_id.clone()),
                    guest_mac: static_config.mac_address.clone(),
                    host_dev_name: Some(static_config.host_dev_name.clone()),
                    rx_rate_limiter: iface.in_rate_limiter.clone(),
                    tx_rate_limiter: iface.out_rate_limiter.clone(),
                },
            )
            .await
    }

    pub async fn create_network_interfaces(&mut self) -> Result<()> {
        let interfaces = self.cfg.network_interfaces.clone();
        for (index, iface) in interfaces.iter().cloned().enumerate() {
            self.create_network_interface(&iface, index + 1).await?;
        }
        Ok(())
    }

    pub async fn attach_drive(&mut self, drive: &crate::models::Drive) -> Result<()> {
        let drive_id = drive.drive_id.as_deref().unwrap_or_default().to_string();
        self.client.put_guest_drive_by_id(&drive_id, drive).await
    }

    pub async fn attach_drives(&mut self) -> Result<()> {
        for drive in self.cfg.drives.clone() {
            self.attach_drive(&drive).await?;
        }
        Ok(())
    }

    pub async fn add_vsock(&mut self, device: &crate::vsock::VsockDevice) -> Result<()> {
        self.client
            .put_guest_vsock(&VsockModel {
                vsock_id: Some(device.id.clone()),
                guest_cid: Some(device.cid as i64),
                uds_path: Some(device.path.clone()),
            })
            .await
    }

    pub async fn add_vsocks(&mut self) -> Result<()> {
        for device in self.cfg.vsock_devices.clone() {
            self.add_vsock(&device).await?;
        }
        Ok(())
    }

    pub async fn set_metadata(&mut self, metadata: &serde_json::Value) -> Result<()> {
        self.client.put_mmds(metadata).await
    }

    pub async fn update_metadata(&mut self, metadata: &serde_json::Value) -> Result<()> {
        self.client.patch_mmds(metadata).await
    }

    pub async fn get_metadata<T: DeserializeOwned>(&mut self) -> Result<T> {
        let value = self.client.get_mmds().await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn set_mmds_config(
        &mut self,
        address: Option<std::net::Ipv4Addr>,
        ifaces: &NetworkInterfaces,
        version: MMDSVersion,
    ) -> Result<()> {
        let network_interfaces = ifaces
            .iter()
            .enumerate()
            .filter_map(|(index, iface)| iface.allow_mmds.then(|| (index + 1).to_string()))
            .collect::<Vec<_>>();

        if network_interfaces.is_empty() {
            return Ok(());
        }

        self.client
            .put_mmds_config(&MmdsConfig {
                ipv4_address: address.map(|address| address.to_string()),
                network_interfaces,
                version: Some(match version {
                    MMDSVersion::V1 => crate::models::MMDS_VERSION_V1.to_string(),
                    MMDSVersion::V2 => crate::models::MMDS_VERSION_V2.to_string(),
                }),
            })
            .await
    }

    pub async fn get_firecracker_version(&mut self) -> Result<String> {
        Ok(self
            .client
            .get_firecracker_version()
            .await?
            .firecracker_version)
    }

    pub async fn describe_instance_info(&mut self) -> Result<InstanceInfo> {
        self.client.describe_instance().await
    }

    pub async fn update_guest_drive(&mut self, drive_id: &str, path_on_host: &str) -> Result<()> {
        self.client
            .patch_guest_drive_by_id(
                drive_id,
                &PartialDrive {
                    drive_id: Some(drive_id.to_string()),
                    path_on_host: Some(path_on_host.to_string()),
                },
            )
            .await
    }

    pub async fn update_guest_network_interface_rate_limit(
        &mut self,
        iface_id: &str,
        rate_limiters: RateLimiterSet,
    ) -> Result<()> {
        self.client
            .patch_guest_network_interface_by_id(
                iface_id,
                &PartialNetworkInterface {
                    iface_id: Some(iface_id.to_string()),
                    rx_rate_limiter: rate_limiters.in_rate_limiter,
                    tx_rate_limiter: rate_limiters.out_rate_limiter,
                },
            )
            .await
    }

    pub async fn pause_vm(&mut self) -> Result<()> {
        self.client.patch_vm(&Vm::paused()).await
    }

    pub async fn resume_vm(&mut self) -> Result<()> {
        self.client.patch_vm(&Vm::resumed()).await
    }

    pub async fn create_snapshot(
        &mut self,
        mem_file_path: &str,
        snapshot_path: &str,
    ) -> Result<()> {
        self.client
            .create_snapshot(&SnapshotCreateParams {
                mem_file_path: Some(mem_file_path.to_string()),
                snapshot_path: Some(snapshot_path.to_string()),
            })
            .await
    }

    pub async fn load_snapshot(&mut self) -> Result<()> {
        let snapshot = &self.cfg.snapshot;
        self.client
            .load_snapshot(&SnapshotLoadParams {
                mem_file_path: snapshot.mem_file_path.clone(),
                mem_backend: snapshot.mem_backend.clone(),
                snapshot_path: snapshot.snapshot_path.clone(),
                enable_diff_snapshots: snapshot.enable_diff_snapshots,
                resume_vm: snapshot.resume_vm,
            })
            .await
    }

    pub async fn create_balloon(
        &mut self,
        amount_mib: i64,
        deflate_on_oom: bool,
        stats_polling_intervals: i64,
    ) -> Result<()> {
        self.client
            .put_balloon(&Balloon {
                amount_mib: Some(amount_mib),
                deflate_on_oom: Some(deflate_on_oom),
                stats_polling_intervals,
            })
            .await
    }

    pub async fn get_balloon_config(&mut self) -> Result<Balloon> {
        self.client.get_balloon_config().await
    }

    pub async fn update_balloon(&mut self, amount_mib: i64) -> Result<()> {
        self.client
            .patch_balloon(&BalloonUpdate {
                amount_mib: Some(amount_mib),
            })
            .await
    }

    pub async fn get_balloon_stats(&mut self) -> Result<BalloonStats> {
        self.client.get_balloon_stats().await
    }

    pub async fn update_balloon_stats(&mut self, stats_polling_intervals: i64) -> Result<()> {
        self.client
            .patch_balloon_stats_interval(&BalloonStatsUpdate {
                stats_polling_intervals: Some(stats_polling_intervals),
            })
            .await
    }

    pub async fn get_export_vm_config(&mut self) -> Result<FullVmConfiguration> {
        self.client.get_export_vm_config().await
    }

    async fn start_instance(&mut self) -> Result<()> {
        if self.cfg.has_snapshot() {
            return Ok(());
        }

        self.client
            .create_sync_action(&InstanceActionInfo {
                action_type: Some(crate::models::INSTANCE_ACTION_INSTANCE_START.to_string()),
            })
            .await
    }
}
