#![allow(non_snake_case)]

mod real_vm_support;

use std::fs;
use std::net::Ipv4Addr;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use firecracker_sdk::fctesting::{MockClient, TestWriter};
use firecracker_sdk::{
    AsyncResultExt, Balloon, BalloonStats, BlockingFutureExt, CniNetworkOperations, CommandStdio,
    Config, DEFAULT_FORWARD_SIGNALS, FIRECRACKER_INIT_TIMEOUT_ENV, FifoLogWriter,
    FirecrackerVersion, FullVmConfiguration, HandlerList, INSTANCE_ACTION_INSTANCE_START,
    IPConfiguration, InstanceInfo, JailerConfig, Machine, MachineConfiguration,
    NaiveChrootStrategy, NetworkInterface, NetworkInterfaces, PartialDrive, RateLimiter,
    RateLimiterSet, RealCniNetworkOperations, SnapshotConfig, StaticNetworkConfiguration,
    VM_STATE_PAUSED, VM_STATE_RESUMED, VMCommandBuilder, new_machine, with_client, with_logger,
    with_process_runner, with_snapshot,
};
use ipnet::Ipv4Net;

unsafe extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
    #[link_name = "getpid"]
    fn libc_getpid() -> i32;
    #[link_name = "kill"]
    fn libc_kill(pid: i32, signal: i32) -> i32;
}

fn firecracker_binary() -> &'static str {
    "/data/firecracker"
}

fn is_root() -> bool {
    unsafe { libc_geteuid() == 0 }
}

fn wait_for_path(path: &std::path::Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }

    panic!("timed out waiting for {:?}", path);
}

fn wait_for_signal_count(path: &Path, expected: usize, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let count = fs::read_to_string(path)
            .ok()
            .map(|contents| contents.lines().filter(|line| !line.is_empty()).count())
            .unwrap_or(0);

        if count >= expected {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected} signals in {:?}",
            path
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn make_real_vm_command(socket_path: &Path, vmid: &str) -> firecracker_sdk::VMCommand {
    VMCommandBuilder::default()
        .with_bin(real_vm_support::firecracker_binary())
        .with_socket_path(socket_path.display().to_string())
        .with_args(["--id", vmid, "--no-seccomp"])
        .build()
}

fn base_real_config(socket_path: &Path, vmid: &str, rootfs_path: &Path) -> Config {
    Config {
        vmid: vmid.to_string(),
        socket_path: socket_path.display().to_string(),
        kernel_image_path: real_vm_support::kernel_path().to_string(),
        kernel_args:
            "console=ttyS0 reboot=k panic=1 pci=off nomodules root=/dev/vda rw rootfstype=ext4 init=/init"
                .to_string(),
        drives: firecracker_sdk::DrivesBuilder::new(&rootfs_path.display().to_string())
            .with_root_drive(
                &rootfs_path.display().to_string(),
                [firecracker_sdk::with_read_only(true)],
            )
            .build(),
        machine_cfg: MachineConfiguration::new(1, 512),
        disable_validation: true,
        forward_signals: Some(Vec::new()),
        ..Config::default()
    }
}

fn real_jailer_firecracker_binary() -> &'static str {
    "/data/firecracker-rs-sdk2/testdata/firecracker"
}

fn real_jailer_binary() -> &'static str {
    "/data/firecracker-rs-sdk2/testdata/jailer"
}

fn real_jailer_kernel_path() -> &'static str {
    "/data/firecracker-rs-sdk2/testdata/vmlinux"
}

fn real_jailer_assets_available() -> bool {
    real_vm_support::assets_available()
        && Path::new(real_jailer_firecracker_binary()).exists()
        && Path::new(real_jailer_binary()).exists()
        && Path::new(real_jailer_kernel_path()).exists()
}

static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct EnvGuard {
    key: &'static str,
    previous_value: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous_value = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key,
            previous_value,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous_value) = self.previous_value.as_deref() {
                std::env::set_var(self.key, previous_value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

const SIGUSR1: i32 = 10;
const SIGUSR2: i32 = 12;
const SIGWINCH: i32 = 28;

#[test]
fn TestNewMachine() {
    test_new_machine_generates_vmid();
    test_new_machine_with_vmid();
    test_new_machine_sets_default_netns_for_cni();
    test_new_machine_sets_default_forward_signals_when_unspecified();
    test_new_machine_preserves_explicit_empty_forward_signals();
    test_new_machine_exposes_logger_accessor();
    test_new_machine_snapshot_adapts_handlers();
}

#[test]
fn TestJailerMicroVMExecution() {
    if !real_jailer_assets_available() {
        return;
    }

    let workspace_dir = Path::new("/tmp").join(format!("j{}", std::process::id()));
    let _ = fs::remove_dir_all(&workspace_dir);
    fs::create_dir_all(&workspace_dir).unwrap();

    let kernel_copy = workspace_dir.join("vmlinux.bin");
    fs::copy(real_jailer_kernel_path(), &kernel_copy).unwrap();
    let rootfs_path = real_vm_support::build_sleeping_rootfs(&workspace_dir, "jailer-rootfs");

    let mut machine = Machine::new(Config {
        socket_path: "a.sock".to_string(),
        kernel_image_path: kernel_copy.display().to_string(),
        kernel_args:
            "console=ttyS0 reboot=k panic=1 pci=off nomodules root=/dev/vda rw rootfstype=ext4 init=/init"
                .to_string(),
        drives: firecracker_sdk::DrivesBuilder::new(rootfs_path.display().to_string())
            .with_root_drive(
                rootfs_path.display().to_string(),
                [firecracker_sdk::with_read_only(true)],
            )
            .build(),
        machine_cfg: MachineConfiguration::new(1, 256),
        disable_validation: true,
        forward_signals: Some(Vec::new()),
        jailer_cfg: Some(JailerConfig {
            id: "b".to_string(),
            uid: Some(0),
            gid: Some(0),
            numa_node: Some(0),
            exec_file: real_jailer_firecracker_binary().to_string(),
            jailer_binary: Some(real_jailer_binary().to_string()),
            chroot_base_dir: Some(workspace_dir.display().to_string()),
            chroot_strategy: Some(std::sync::Arc::new(NaiveChrootStrategy::new(
                kernel_copy.display().to_string(),
            ))),
            cgroup_version: Some("2".to_string()),
            ..JailerConfig::default()
        }),
        ..Config::default()
    })
    .unwrap();

    machine.start().unwrap();
    assert!(machine.pid().unwrap() > 0);
    assert!(Path::new(&machine.cfg.socket_path).exists());

    machine.stop_vmm().unwrap();
    assert!(machine.wait().is_err());

    let _ = fs::remove_dir_all(&workspace_dir);
}

#[test]
fn TestMicroVMExecution() {
    test_start_vmm_pid_stop_wait();
    test_machine_get_firecracker_version_and_describe_instance();
}

#[test]
fn TestStartVMM() {
    test_start_vmm_pid_stop_wait();
    test_start_calls_instance_start_after_handlers();
}

#[test]
fn TestLogAndMetrics() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path = real_vm_support::build_sleeping_rootfs(temp_dir.path(), "log-metrics-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let log_path = temp_dir.path().join("firecracker.log");
    let metrics_path = temp_dir.path().join("firecracker.metrics");
    fs::File::create(&log_path).unwrap();
    fs::File::create(&metrics_path).unwrap();

    let mut machine = new_machine(
        Config {
            log_path: Some(log_path.display().to_string()),
            log_level: Some("Debug".to_string()),
            metrics_path: Some(metrics_path.display().to_string()),
            ..base_real_config(&socket_path, "log-metrics", &rootfs_path)
        },
        [with_process_runner(make_real_vm_command(
            &socket_path,
            "log-metrics",
        ))],
    )
    .unwrap();

    machine.start().unwrap();
    thread::sleep(Duration::from_millis(250));
    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    let log_contents = fs::read_to_string(&log_path).unwrap();
    assert!(!log_contents.trim().is_empty());

    let metrics_contents = fs::read_to_string(&metrics_path).unwrap();
    assert!(!metrics_contents.trim().is_empty());
}

#[test]
fn TestStartVMMOnce() {
    let mut machine = Machine::new_with_client(
        Config {
            disable_validation: true,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        Box::new(MockClient::default()),
    )
    .unwrap();
    machine.handlers.validation = HandlerList::default();
    machine.handlers.fc_init = HandlerList::default();

    machine.start().unwrap();
    assert!(matches!(
        machine.start().block_on(),
        Err(firecracker_sdk::Error::AlreadyStarted)
    ));
}

#[test]
fn TestStopVMMCleanup() {
    test_stop_vmm_uses_sigterm_and_allows_graceful_exit();
}

#[test]
fn TestWaitForSocket() {
    test_wait_for_socket();
}

#[test]
fn TestMicroVMExecutionWithMmdsV2() {
    test_get_and_update_metadata();
    test_set_metadata();
}

#[test]
fn TestLogFiles() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path =
        real_vm_support::build_sleeping_rootfs(temp_dir.path(), "log-fifo-writer-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let log_fifo = temp_dir.path().join("firecracker.log");
    let captured_log = temp_dir.path().join("captured.log");
    let writer = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(&captured_log)
        .unwrap();

    let mut machine = new_machine(
        Config {
            log_fifo: Some(log_fifo.display().to_string()),
            log_level: Some("Debug".to_string()),
            fifo_log_writer: Some(FifoLogWriter::new(writer)),
            ..base_real_config(&socket_path, "log-fifo-writer", &rootfs_path)
        },
        [with_process_runner(make_real_vm_command(
            &socket_path,
            "log-fifo-writer",
        ))],
    )
    .unwrap();

    machine.start().unwrap();
    thread::sleep(Duration::from_millis(250));
    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    let log_contents = fs::read_to_string(&captured_log).unwrap();
    assert!(!log_contents.trim().is_empty());
}

#[test]
fn TestCaptureFifoToFile() {
    test_capture_fifo_to_file();
}

#[test]
fn TestCaptureFifoToFile_nonblock() {
    test_capture_fifo_to_file_nonblock();
}

#[test]
fn TestSocketPathSet() {
    test_socket_path_set_on_command();
}

#[test]
fn TestPID() {
    test_start_vmm_pid_stop_wait();
    test_pid_reports_exited_process_before_wait();
}

#[test]
fn TestCaptureFifoToFile_leak() {
    test_capture_fifo_to_file_with_channel_stops_on_exit();
}

#[test]
fn TestWait() {
    test_wait_reports_external_kill_error();
}

#[test]
fn TestWaitWithInvalidBinary() {
    let mut machine = new_machine(
        Config {
            disable_validation: true,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(
                VMCommandBuilder::default()
                    .with_bin("/definitely/missing/firecracker")
                    .with_stdin(CommandStdio::Null)
                    .with_stdout(CommandStdio::Null)
                    .with_stderr(CommandStdio::Null)
                    .build(),
            ),
        ],
    )
    .unwrap();

    let start_error = machine.start().unwrap_err();
    let wait_error = machine.wait().unwrap_err();
    assert_eq!(start_error.to_string(), wait_error.to_string());
    assert!(machine.pid().is_err());
}

#[test]
fn TestWaitWithNoSocket() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let _env_guard = EnvGuard::set(FIRECRACKER_INIT_TIMEOUT_ENV, "1");

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient {
                get_machine_configuration_fn: Some(Box::new(|| {
                    Err(firecracker_sdk::Error::Process(
                        "socket not ready".to_string(),
                    ))
                })),
                ..MockClient::default()
            })),
            with_process_runner(
                VMCommandBuilder::default()
                    .with_bin("/bin/sh")
                    .with_args(["-c", "sleep 60"])
                    .with_stdin(CommandStdio::Null)
                    .with_stdout(CommandStdio::Null)
                    .with_stderr(CommandStdio::Null)
                    .build(),
            ),
        ],
    )
    .unwrap();

    let start_error = machine.start().unwrap_err();
    assert!(
        start_error
            .to_string()
            .contains("timed out while waiting for the Firecracker VMM to become reachable")
    );

    let wait_error = machine.wait().unwrap_err();
    assert_eq!(start_error.to_string(), wait_error.to_string());
    assert!(machine.pid().is_err());
}

#[test]
fn TestSignalForwarding() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output_path = temp_dir.path().join("signals.out");
    let ready_path = temp_dir.path().join("ready");
    let socket_path = temp_dir.path().join("machine.sock");
    let script_path = temp_dir.path().join("signal-recorder.sh");

    fs::write(
        &script_path,
        format!(
            r#"#!/bin/sh
output_file="$1"
ready_file="$2"
socket_path="$3"

: > "$output_file"
trap 'printf "%s\n" {sigusr1} >> "$output_file"' USR1
trap 'printf "%s\n" {sigusr2} >> "$output_file"' USR2
trap 'printf "%s\n" {sigwinch} >> "$output_file"' WINCH
: > "$socket_path"
: > "$ready_file"

while :
do
    sleep 1
done
"#,
            sigusr1 = SIGUSR1,
            sigusr2 = SIGUSR2,
            sigwinch = SIGWINCH,
        ),
    )
    .unwrap();

    let mut permissions = fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).unwrap();

    let command = VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args([
            script_path.display().to_string(),
            output_path.display().to_string(),
            ready_path.display().to_string(),
            socket_path.display().to_string(),
        ])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .build();

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            socket_path: socket_path.display().to_string(),
            machine_cfg: MachineConfiguration::new(1, 128),
            forward_signals: Some(vec![SIGUSR1, SIGUSR2]),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(command),
        ],
    )
    .unwrap();

    machine.start_vmm().unwrap();
    wait_for_path(&ready_path, Duration::from_secs(2));

    let pid = unsafe { libc_getpid() };
    assert_eq!(0, unsafe { libc_kill(pid, SIGUSR1) });
    assert_eq!(0, unsafe { libc_kill(pid, SIGWINCH) });
    assert_eq!(0, unsafe { libc_kill(pid, SIGUSR2) });

    wait_for_signal_count(&output_path, 2, Duration::from_secs(2));

    machine.stop_vmm().unwrap();
    assert!(machine.wait().is_err());

    let mut received_signals = fs::read_to_string(&output_path)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.parse::<i32>().unwrap())
        .collect::<Vec<_>>();
    received_signals.sort_unstable();

    assert_eq!(vec![SIGUSR1, SIGUSR2], received_signals);
}

#[test]
fn TestPauseResume() {
    test_pause_and_resume_vm();
}

#[test]
fn TestCreateSnapshot() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path = real_vm_support::build_sleeping_rootfs(temp_dir.path(), "snapshot-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let mem_path = temp_dir.path().join("vm.mem");
    let snapshot_path = temp_dir.path().join("vm.snap");

    let mut machine = new_machine(
        base_real_config(&socket_path, "snapshot-source", &rootfs_path),
        [with_process_runner(make_real_vm_command(
            &socket_path,
            "snapshot-source",
        ))],
    )
    .unwrap();

    machine.start().unwrap();
    machine.pause_vm().unwrap();
    machine
        .create_snapshot(
            &mem_path.display().to_string(),
            &snapshot_path.display().to_string(),
        )
        .unwrap();
    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    assert!(mem_path.exists());
    assert!(snapshot_path.exists());
    assert!(fs::metadata(&mem_path).unwrap().len() > 0);
    assert!(fs::metadata(&snapshot_path).unwrap().len() > 0);
}

#[test]
fn TestLoadSnapshot() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path = real_vm_support::build_sleeping_rootfs(temp_dir.path(), "snapshot-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let mem_path = temp_dir.path().join("vm.mem");
    let snapshot_path = temp_dir.path().join("vm.snap");

    let mut machine = new_machine(
        base_real_config(&socket_path, "snapshot-source", &rootfs_path),
        [with_process_runner(make_real_vm_command(
            &socket_path,
            "snapshot-source",
        ))],
    )
    .unwrap();

    machine.start().unwrap();
    machine.pause_vm().unwrap();
    machine
        .create_snapshot(
            &mem_path.display().to_string(),
            &snapshot_path.display().to_string(),
        )
        .unwrap();
    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    let restore_socket_path = temp_dir.path().join("restore.sock");
    let restore_config = base_real_config(&restore_socket_path, "snapshot-restore", &rootfs_path);

    let mut restored_machine = new_machine(
        restore_config,
        [
            with_snapshot(
                "",
                snapshot_path.display().to_string(),
                [firecracker_sdk::with_memory_backend(
                    "File",
                    mem_path.display().to_string(),
                )],
            ),
            with_process_runner(make_real_vm_command(
                &restore_socket_path,
                "snapshot-restore",
            )),
        ],
    )
    .unwrap();

    restored_machine.start().unwrap();
    restored_machine.resume_vm().unwrap();
    restored_machine.stop_vmm().unwrap();
    let _ = restored_machine.wait();
}

#[test]
fn test_new_machine_generates_vmid() {
    let machine = Machine::new(Config {
        machine_cfg: MachineConfiguration::new(1, 100),
        ..Config::default()
    })
    .unwrap();
    assert!(!machine.cfg.vmid.is_empty());
}

#[test]
fn test_new_machine_with_vmid() {
    let machine = Machine::new(Config {
        vmid: "my-custom-id".to_string(),
        machine_cfg: MachineConfiguration::new(1, 100),
        ..Config::default()
    })
    .unwrap();
    assert_eq!("my-custom-id", machine.cfg.vmid);
}

#[test]
fn test_socket_path_set_on_command() {
    let machine = Machine::new(Config {
        socket_path: "foo/bar".to_string(),
        machine_cfg: MachineConfiguration::new(1, 100),
        ..Config::default()
    })
    .unwrap();

    let args = &machine.command.as_ref().unwrap().args;
    let index = args.iter().position(|arg| arg == "--api-sock").unwrap();
    assert_eq!("foo/bar", args[index + 1]);
}

#[test]
fn test_new_machine_sets_default_netns_for_cni() {
    let machine = Machine::new(Config {
        vmid: "vm-123".to_string(),
        machine_cfg: MachineConfiguration::new(1, 100),
        network_interfaces: NetworkInterfaces::from(vec![NetworkInterface {
            cni_configuration: Some(firecracker_sdk::CniConfiguration {
                network_name: Some("fcnet".to_string()),
                ..firecracker_sdk::CniConfiguration::default()
            }),
            ..NetworkInterface::default()
        }]),
        ..Config::default()
    })
    .unwrap();

    assert_eq!(
        Some("/var/run/netns/vm-123".to_string()),
        machine.cfg.net_ns
    );
}

#[test]
fn test_new_machine_sets_default_forward_signals_when_unspecified() {
    let machine = Machine::new(Config {
        machine_cfg: MachineConfiguration::new(1, 100),
        ..Config::default()
    })
    .unwrap();

    assert_eq!(
        Some(DEFAULT_FORWARD_SIGNALS.to_vec()),
        machine.cfg.forward_signals
    );
}

#[test]
fn test_new_machine_preserves_explicit_empty_forward_signals() {
    let machine = Machine::new(Config {
        machine_cfg: MachineConfiguration::new(1, 100),
        forward_signals: Some(Vec::new()),
        ..Config::default()
    })
    .unwrap();

    assert_eq!(Some(Vec::new()), machine.cfg.forward_signals);
}

#[test]
fn test_new_machine_exposes_logger_accessor() {
    let dispatch = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let machine = new_machine(
        Config {
            machine_cfg: MachineConfiguration::new(1, 100),
            ..Config::default()
        },
        [with_logger(dispatch)],
    )
    .unwrap();

    assert!(machine.logger().is_some());
}

#[test]
fn test_new_machine_snapshot_adapts_handlers() {
    let machine = Machine::new(Config {
        machine_cfg: MachineConfiguration::new(1, 100),
        snapshot: SnapshotConfig::with_paths("/tmp/mem", "/tmp/snapshot"),
        ..Config::default()
    })
    .unwrap();

    let handler_names = machine
        .handlers
        .fc_init
        .list
        .iter()
        .map(|handler| handler.name.as_str())
        .collect::<Vec<_>>();

    assert!(handler_names.contains(&firecracker_sdk::LOAD_SNAPSHOT_HANDLER_NAME));
    assert!(!handler_names.contains(&firecracker_sdk::CREATE_MACHINE_HANDLER_NAME));
    assert!(!handler_names.contains(&firecracker_sdk::CREATE_BOOT_SOURCE_HANDLER_NAME));
}

#[test]
fn test_setup_kernel_args_adds_ip_boot_param() {
    let mut machine = Machine::new(Config {
        machine_cfg: MachineConfiguration::new(1, 100),
        kernel_args: "console=ttyS0".to_string(),
        network_interfaces: NetworkInterfaces::from(vec![NetworkInterface {
            static_configuration: Some(
                StaticNetworkConfiguration::new("tap0").with_ip_configuration(
                    IPConfiguration::new(
                        "192.0.2.2/24".parse::<Ipv4Net>().unwrap(),
                        Ipv4Addr::new(192, 0, 2, 1),
                    )
                    .with_if_name("eth0"),
                ),
            ),
            ..NetworkInterface::default()
        }]),
        ..Config::default()
    })
    .unwrap();

    machine.setup_kernel_args().unwrap();
    assert!(machine.cfg.kernel_args.contains("console=ttyS0"));
    assert!(
        machine
            .cfg
            .kernel_args
            .contains("ip=192.0.2.2::192.0.2.1:255.255.255.0::eth0:off")
    );
}

#[test]
fn test_start_vmm_pid_stop_wait() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("machine.sock");

    let mut machine = Machine::new(Config {
        socket_path: socket_path.display().to_string(),
        machine_cfg: MachineConfiguration::new(1, 128),
        disable_validation: true,
        ..Config::default()
    })
    .unwrap();

    machine.command.as_mut().unwrap().bin = firecracker_binary().to_string();

    machine.start_vmm().unwrap();
    assert!(machine.pid().unwrap() > 0);

    machine.stop_vmm().unwrap();
    assert!(machine.wait().is_err());
    assert!(machine.pid().is_err());
}

#[test]
fn test_pid_reports_exited_process_before_wait() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("machine.sock");
    let socket_path_str = socket_path.display().to_string();

    let command = VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args([
            "-c",
            "touch \"$1\"; exec sleep 0.2",
            "sh",
            socket_path_str.as_str(),
        ])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .build();

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            forward_signals: Some(Vec::new()),
            socket_path: socket_path_str,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(command),
        ],
    )
    .unwrap();

    machine.start_vmm().unwrap();
    assert!(machine.pid().unwrap() > 0);

    std::thread::sleep(Duration::from_millis(350));

    let pid_error = machine.pid().unwrap_err();
    assert_eq!(
        "process error: machine process has exited",
        pid_error.to_string()
    );
    machine.wait().unwrap();
}

#[test]
fn test_wait_reports_external_kill_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("machine.sock");
    let socket_path_str = socket_path.display().to_string();

    let command = VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args([
            "-c",
            "touch \"$1\"; exec sleep 60",
            "sh",
            socket_path_str.as_str(),
        ])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .build();

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            forward_signals: Some(Vec::new()),
            socket_path: socket_path_str,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(command),
        ],
    )
    .unwrap();

    machine.start_vmm().unwrap();
    let pid = machine.pid().unwrap();
    assert!(
        Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );

    let wait_error = machine.wait().unwrap_err().to_string();
    assert!(wait_error.contains("firecracker exited:"));
    assert!(machine.pid().is_err());
    assert_eq!(wait_error, machine.wait().unwrap_err().to_string());
}

#[test]
fn test_stop_vmm_uses_sigterm_and_allows_graceful_exit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("machine.sock");
    let term_path = temp_dir.path().join("term.log");

    let socket_path_str = socket_path.display().to_string();
    let term_path_str = term_path.display().to_string();

    let command = VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args([
            "-c",
            "trap 'echo term > \"$2\"; exit 0' TERM; touch \"$1\"; while :; do sleep 1; done",
            "sh",
            socket_path_str.as_str(),
            term_path_str.as_str(),
        ])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .build();

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            forward_signals: Some(Vec::new()),
            socket_path: socket_path_str,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(command),
        ],
    )
    .unwrap();

    machine.start_vmm().unwrap();
    machine.stop_vmm().unwrap();
    machine.wait().unwrap();

    assert_eq!("term\n", fs::read_to_string(term_path).unwrap());
}

#[test]
fn test_start_calls_instance_start_after_handlers() {
    let seen_actions = Arc::new(Mutex::new(Vec::new()));
    let client = MockClient {
        create_sync_action_fn: Some(Box::new({
            let seen_actions = seen_actions.clone();
            move |action| {
                seen_actions
                    .lock()
                    .unwrap()
                    .push(action.action_type.clone().unwrap());
                Ok(())
            }
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(
        Config {
            disable_validation: true,
            ..Config::default()
        },
        Box::new(client),
    )
    .unwrap();
    machine.handlers.validation = HandlerList::default();
    machine.handlers.fc_init = HandlerList::default();

    machine.start().unwrap();

    assert_eq!(
        vec![INSTANCE_ACTION_INSTANCE_START.to_string()],
        *seen_actions.lock().unwrap()
    );
}

#[test]
fn test_load_snapshot_uses_snapshot_config() {
    let client = MockClient {
        load_snapshot_fn: Some(Box::new(|snapshot| {
            assert_eq!(Some("/tmp/mem".to_string()), snapshot.mem_file_path);
            assert_eq!(Some("/tmp/snapshot".to_string()), snapshot.snapshot_path);
            assert!(snapshot.enable_diff_snapshots);
            assert!(snapshot.resume_vm);
            Ok(())
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(
        Config {
            snapshot: SnapshotConfig {
                mem_file_path: Some("/tmp/mem".to_string()),
                snapshot_path: Some("/tmp/snapshot".to_string()),
                enable_diff_snapshots: true,
                resume_vm: true,
                ..SnapshotConfig::default()
            },
            ..Config::default()
        },
        Box::new(client),
    )
    .unwrap();

    machine.load_snapshot().unwrap();
}

#[test]
fn test_get_and_update_metadata() {
    #[derive(Debug, serde::Deserialize, PartialEq, Eq)]
    struct Metadata {
        hello: String,
    }

    let client = MockClient {
        get_mmds_fn: Some(Box::new(|| Ok(serde_json::json!({ "hello": "world" })))),
        patch_mmds_fn: Some(Box::new(|metadata| {
            assert_eq!(serde_json::json!({ "updated": true }), *metadata);
            Ok(())
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(Config::default(), Box::new(client)).unwrap();

    let metadata = machine.get_metadata::<Metadata>().unwrap();
    assert_eq!(
        Metadata {
            hello: "world".to_string()
        },
        metadata
    );

    machine
        .update_metadata(&serde_json::json!({ "updated": true }))
        .unwrap();
}

#[test]
fn test_set_metadata() {
    let client = MockClient {
        put_mmds_fn: Some(Box::new(|metadata| {
            assert_eq!(serde_json::json!({ "hello": "world" }), *metadata);
            Ok(())
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(Config::default(), Box::new(client)).unwrap();
    machine
        .set_metadata(&serde_json::json!({ "hello": "world" }))
        .unwrap();
}

#[test]
fn test_machine_get_firecracker_version_and_describe_instance() {
    let client = MockClient {
        get_firecracker_version_fn: Some(Box::new(|| {
            Ok(FirecrackerVersion {
                firecracker_version: "1.2.3".to_string(),
            })
        })),
        describe_instance_fn: Some(Box::new(|| {
            Ok(InstanceInfo {
                app_name: Some("firecracker".to_string()),
                id: Some("vm-123".to_string()),
                state: Some("Running".to_string()),
                vmm_version: Some("1.2.3".to_string()),
                ..InstanceInfo::default()
            })
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(Config::default(), Box::new(client)).unwrap();
    assert_eq!("1.2.3", machine.get_firecracker_version().unwrap());
    assert_eq!(
        InstanceInfo {
            app_name: Some("firecracker".to_string()),
            id: Some("vm-123".to_string()),
            state: Some("Running".to_string()),
            vmm_version: Some("1.2.3".to_string()),
            ..InstanceInfo::default()
        },
        machine.describe_instance_info().unwrap()
    );
}

#[test]
fn test_pause_and_resume_vm() {
    let states = Arc::new(Mutex::new(Vec::new()));
    let client = MockClient {
        patch_vm_fn: Some(Box::new({
            let states = states.clone();
            move |vm| {
                states.lock().unwrap().push(vm.state.clone().unwrap());
                Ok(())
            }
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(Config::default(), Box::new(client)).unwrap();
    machine.pause_vm().unwrap();
    machine.resume_vm().unwrap();

    assert_eq!(
        vec![VM_STATE_PAUSED.to_string(), VM_STATE_RESUMED.to_string()],
        *states.lock().unwrap()
    );
}

#[test]
fn test_balloon_methods_and_export_vm_config() {
    let client = MockClient {
        put_balloon_fn: Some(Box::new(|balloon| {
            assert_eq!(Some(10), balloon.amount_mib);
            assert_eq!(Some(true), balloon.deflate_on_oom);
            assert_eq!(1, balloon.stats_polling_intervals);
            Ok(())
        })),
        get_balloon_config_fn: Some(Box::new(|| {
            Ok(Balloon {
                amount_mib: Some(10),
                deflate_on_oom: Some(true),
                stats_polling_intervals: 1,
            })
        })),
        patch_balloon_fn: Some(Box::new(|update| {
            assert_eq!(Some(6), update.amount_mib);
            Ok(())
        })),
        get_balloon_stats_fn: Some(Box::new(|| {
            Ok(BalloonStats {
                raw: std::collections::BTreeMap::from([(
                    "available_memory".to_string(),
                    serde_json::json!(123),
                )]),
            })
        })),
        patch_balloon_stats_interval_fn: Some(Box::new(|update| {
            assert_eq!(Some(6), update.stats_polling_intervals);
            Ok(())
        })),
        get_export_vm_config_fn: Some(Box::new(|| {
            Ok(FullVmConfiguration {
                balloon: Some(Balloon {
                    amount_mib: Some(10),
                    deflate_on_oom: Some(true),
                    stats_polling_intervals: 1,
                }),
                ..FullVmConfiguration::default()
            })
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(Config::default(), Box::new(client)).unwrap();
    machine.create_balloon(10, true, 1).unwrap();
    assert_eq!(
        Balloon {
            amount_mib: Some(10),
            deflate_on_oom: Some(true),
            stats_polling_intervals: 1,
        },
        machine.get_balloon_config().unwrap()
    );
    machine.update_balloon(6).unwrap();
    assert_eq!(
        BalloonStats {
            raw: std::collections::BTreeMap::from([(
                "available_memory".to_string(),
                serde_json::json!(123),
            )]),
        },
        machine.get_balloon_stats().unwrap()
    );
    machine.update_balloon_stats(6).unwrap();
    assert_eq!(
        FullVmConfiguration {
            balloon: Some(Balloon {
                amount_mib: Some(10),
                deflate_on_oom: Some(true),
                stats_polling_intervals: 1,
            }),
            ..FullVmConfiguration::default()
        },
        machine.get_export_vm_config().unwrap()
    );
}

#[test]
fn test_update_guest_drive_and_network_rate_limit() {
    let seen_drive = Arc::new(Mutex::new(None::<PartialDrive>));
    let seen_network = Arc::new(Mutex::new(None::<RateLimiterSet>));
    let client = MockClient {
        patch_guest_drive_by_id_fn: Some(Box::new({
            let seen_drive = seen_drive.clone();
            move |drive_id, drive| {
                assert_eq!("root", drive_id);
                *seen_drive.lock().unwrap() = Some(drive.clone());
                Ok(())
            }
        })),
        patch_guest_network_interface_by_id_fn: Some(Box::new({
            let seen_network = seen_network.clone();
            move |iface_id, iface| {
                assert_eq!("1", iface_id);
                *seen_network.lock().unwrap() = Some(RateLimiterSet {
                    in_rate_limiter: iface.rx_rate_limiter.clone(),
                    out_rate_limiter: iface.tx_rate_limiter.clone(),
                });
                Ok(())
            }
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(Config::default(), Box::new(client)).unwrap();
    machine.update_guest_drive("root", "/tmp/rootfs").unwrap();
    machine
        .update_guest_network_interface_rate_limit(
            "1",
            RateLimiterSet {
                in_rate_limiter: Some(RateLimiter::default()),
                out_rate_limiter: Some(RateLimiter::default()),
            },
        )
        .unwrap();

    assert_eq!(
        Some(PartialDrive {
            drive_id: Some("root".to_string()),
            path_on_host: Some("/tmp/rootfs".to_string()),
        }),
        seen_drive.lock().unwrap().clone()
    );
    assert_eq!(
        Some(RateLimiterSet {
            in_rate_limiter: Some(RateLimiter::default()),
            out_rate_limiter: Some(RateLimiter::default()),
        }),
        seen_network.lock().unwrap().clone()
    );
}

#[test]
fn test_option_aware_machine_wrappers_call_through() {
    let seen_drive = Arc::new(Mutex::new(None::<PartialDrive>));
    let seen_network = Arc::new(Mutex::new(None::<RateLimiterSet>));
    let seen_states = Arc::new(Mutex::new(Vec::new()));
    let seen_snapshot = Arc::new(Mutex::new(None::<SnapshotConfig>));
    let seen_balloon = Arc::new(Mutex::new(None::<Balloon>));
    let seen_balloon_update = Arc::new(Mutex::new(None::<i64>));
    let seen_balloon_stats = Arc::new(Mutex::new(None::<i64>));
    let client = MockClient {
        patch_guest_drive_by_id_fn: Some(Box::new({
            let seen_drive = seen_drive.clone();
            move |drive_id, drive| {
                assert_eq!("root", drive_id);
                *seen_drive.lock().unwrap() = Some(drive.clone());
                Ok(())
            }
        })),
        patch_guest_network_interface_by_id_fn: Some(Box::new({
            let seen_network = seen_network.clone();
            move |iface_id, iface| {
                assert_eq!("eth0", iface_id);
                *seen_network.lock().unwrap() = Some(RateLimiterSet {
                    in_rate_limiter: iface.rx_rate_limiter.clone(),
                    out_rate_limiter: iface.tx_rate_limiter.clone(),
                });
                Ok(())
            }
        })),
        patch_vm_fn: Some(Box::new({
            let seen_states = seen_states.clone();
            move |vm| {
                seen_states.lock().unwrap().push(vm.state.clone().unwrap());
                Ok(())
            }
        })),
        create_snapshot_fn: Some(Box::new({
            let seen_snapshot = seen_snapshot.clone();
            move |snapshot| {
                *seen_snapshot.lock().unwrap() = Some(SnapshotConfig {
                    mem_file_path: snapshot.mem_file_path.clone(),
                    snapshot_path: snapshot.snapshot_path.clone(),
                    ..SnapshotConfig::default()
                });
                Ok(())
            }
        })),
        put_balloon_fn: Some(Box::new({
            let seen_balloon = seen_balloon.clone();
            move |balloon| {
                *seen_balloon.lock().unwrap() = Some(balloon.clone());
                Ok(())
            }
        })),
        patch_balloon_fn: Some(Box::new({
            let seen_balloon_update = seen_balloon_update.clone();
            move |update| {
                *seen_balloon_update.lock().unwrap() = update.amount_mib;
                Ok(())
            }
        })),
        patch_balloon_stats_interval_fn: Some(Box::new({
            let seen_balloon_stats = seen_balloon_stats.clone();
            move |update| {
                *seen_balloon_stats.lock().unwrap() = update.stats_polling_intervals;
                Ok(())
            }
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(Config::default(), Box::new(client)).unwrap();
    machine
        .update_guest_drive_with_opts(
            "root",
            "/tmp/rootfs",
            vec![firecracker_sdk::with_read_timeout(Duration::from_millis(5))],
        )
        .unwrap();
    machine
        .update_guest_network_interface_rate_limit_with_options(
            "eth0",
            RateLimiterSet {
                in_rate_limiter: Some(RateLimiter::default()),
                out_rate_limiter: Some(RateLimiter::default()),
            },
            firecracker_sdk::RequestOptions::from_opts(vec![
                firecracker_sdk::without_read_timeout(),
            ]),
        )
        .unwrap();
    machine
        .pause_vm_with_opts(vec![firecracker_sdk::with_read_timeout(
            Duration::from_millis(5),
        )])
        .unwrap();
    machine
        .resume_vm_with_options(firecracker_sdk::RequestOptions::from_opts(vec![
            firecracker_sdk::without_write_timeout(),
        ]))
        .unwrap();
    machine
        .create_snapshot_with_opts(
            "/tmp/mem",
            "/tmp/snapshot",
            vec![firecracker_sdk::without_read_timeout()],
        )
        .unwrap();
    machine
        .create_balloon_with_opts(10, true, 1, vec![firecracker_sdk::without_write_timeout()])
        .unwrap();
    machine
        .update_balloon_with_options(
            6,
            firecracker_sdk::RequestOptions::from_opts(vec![
                firecracker_sdk::without_read_timeout(),
            ]),
        )
        .unwrap();
    machine
        .update_balloon_stats_with_opts(
            7,
            vec![firecracker_sdk::with_read_timeout(Duration::from_millis(5))],
        )
        .unwrap();

    assert_eq!(
        Some(PartialDrive {
            drive_id: Some("root".to_string()),
            path_on_host: Some("/tmp/rootfs".to_string()),
        }),
        seen_drive.lock().unwrap().clone()
    );
    assert_eq!(
        Some(RateLimiterSet {
            in_rate_limiter: Some(RateLimiter::default()),
            out_rate_limiter: Some(RateLimiter::default()),
        }),
        seen_network.lock().unwrap().clone()
    );
    assert_eq!(
        vec![VM_STATE_PAUSED.to_string(), VM_STATE_RESUMED.to_string()],
        *seen_states.lock().unwrap()
    );
    assert_eq!(
        Some(SnapshotConfig {
            mem_file_path: Some("/tmp/mem".to_string()),
            snapshot_path: Some("/tmp/snapshot".to_string()),
            ..SnapshotConfig::default()
        }),
        seen_snapshot.lock().unwrap().clone()
    );
    assert_eq!(
        Some(Balloon {
            amount_mib: Some(10),
            deflate_on_oom: Some(true),
            stats_polling_intervals: 1,
        }),
        seen_balloon.lock().unwrap().clone()
    );
    assert_eq!(Some(6), *seen_balloon_update.lock().unwrap());
    assert_eq!(Some(7), *seen_balloon_stats.lock().unwrap());
}

#[test]
fn test_machine_iface_option_aware_methods_dispatch() {
    let seen_states = Arc::new(Mutex::new(Vec::new()));
    let client = MockClient {
        patch_vm_fn: Some(Box::new({
            let seen_states = seen_states.clone();
            move |vm| {
                seen_states.lock().unwrap().push(vm.state.clone().unwrap());
                Ok(())
            }
        })),
        ..MockClient::default()
    };

    let mut machine = Machine::new_with_client(Config::default(), Box::new(client)).unwrap();
    let machine_iface: &mut dyn firecracker_sdk::MachineIface = &mut machine;
    machine_iface
        .pause_vm_with_options(firecracker_sdk::RequestOptions::from_opts(vec![
            firecracker_sdk::without_read_timeout(),
        ]))
        .unwrap();
    machine_iface
        .resume_vm_with_options(firecracker_sdk::RequestOptions::from_opts(vec![
            firecracker_sdk::without_write_timeout(),
        ]))
        .unwrap();

    assert_eq!(
        vec![VM_STATE_PAUSED.to_string(), VM_STATE_RESUMED.to_string()],
        *seen_states.lock().unwrap()
    );
}

#[test]
fn test_default_net_ns_path() {
    let machine = Machine::new(Config {
        vmid: "vm-123".to_string(),
        machine_cfg: MachineConfiguration::new(1, 128),
        ..Config::default()
    })
    .unwrap();

    assert_eq!("/var/run/netns/vm-123", machine.default_net_ns_path());
}

#[test]
fn test_start_vmm_enters_configured_netns_when_not_using_jailer() {
    if !is_root() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let net_ns_path = temp_dir.path().join("machine.netns");
    let socket_path = temp_dir.path().join("machine.sock");
    let observed_path = temp_dir.path().join("observed-netns.txt");
    let cleanups = RealCniNetworkOperations
        .initialize_netns(net_ns_path.to_str().unwrap())
        .unwrap();

    let socket_path_str = socket_path.display().to_string();
    let observed_path_str = observed_path.display().to_string();
    let command = VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args([
            "-c",
            "touch \"$1\"; stat -Lc '%i' /proc/self/ns/net > \"$2\"; sleep 60",
            "sh",
            socket_path_str.as_str(),
            observed_path_str.as_str(),
        ])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .build();

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            socket_path: socket_path_str,
            net_ns: Some(net_ns_path.display().to_string()),
            machine_cfg: MachineConfiguration::new(1, 128),
            forward_signals: Some(Vec::new()),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient {
                get_machine_configuration_fn: Some(Box::new(
                    || Ok(MachineConfiguration::default()),
                )),
                ..MockClient::default()
            })),
            with_process_runner(command),
        ],
    )
    .unwrap();

    machine.start_vmm().unwrap();
    wait_for_path(&observed_path, Duration::from_secs(1));

    let observed_inode = fs::read_to_string(&observed_path)
        .unwrap()
        .trim()
        .parse::<u64>()
        .unwrap();
    let target_inode = fs::metadata(&net_ns_path).unwrap().ino();
    assert_eq!(target_inode, observed_inode);

    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    for cleanup in cleanups.into_iter().rev() {
        cleanup().unwrap();
    }
}

#[test]
fn test_wait_for_socket() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("wait.sock");
    let (tx, rx) = mpsc::channel();

    let mut machine = Machine::new_with_client(
        Config {
            socket_path: socket_path.display().to_string(),
            ..Config::default()
        },
        Box::new(MockClient {
            get_machine_configuration_fn: Some(Box::new(|| Ok(MachineConfiguration::default()))),
            ..MockClient::default()
        }),
    )
    .unwrap();

    let socket_file = socket_path.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = std::fs::File::create(socket_file);
    });

    machine
        .wait_for_socket(Duration::from_millis(500), &rx)
        .unwrap();

    let timeout_error = Machine::new_with_client(
        Config {
            socket_path: socket_path.with_extension("missing").display().to_string(),
            ..Config::default()
        },
        Box::new(MockClient::default()),
    )
    .unwrap()
    .wait_for_socket(Duration::from_millis(50), &rx)
    .unwrap_err();
    assert!(matches!(
        timeout_error,
        firecracker_sdk::Error::Io(ref error) if error.kind() == std::io::ErrorKind::TimedOut
    ));

    tx.send(firecracker_sdk::Error::Process("expected exit".to_string()))
        .unwrap();
    let exit_error = machine
        .wait_for_socket(Duration::from_millis(50), &rx)
        .unwrap_err();
    assert_eq!("process error: expected exit", exit_error.to_string());
}

#[test]
fn test_capture_fifo_to_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fifo_path = temp_dir.path().join("capture.fifo");
    let status = Command::new("mkfifo").arg(&fifo_path).status().unwrap();
    assert!(status.success());

    let expected = b"Hello world!".to_vec();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel();
    let writer = TestWriter {
        write_fn: Box::new({
            let seen = seen.clone();
            move |bytes| {
                seen.lock().unwrap().extend_from_slice(bytes);
                let _ = tx.send(());
                Ok(bytes.len())
            }
        }),
    };

    let machine =
        Machine::new_with_client(Config::default(), Box::new(MockClient::default())).unwrap();
    machine
        .capture_fifo_to_file(fifo_path.to_str().unwrap(), writer)
        .unwrap();

    let mut fifo = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&fifo_path)
        .unwrap();
    use std::io::Write as _;
    fifo.write_all(&expected).unwrap();

    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(expected, *seen.lock().unwrap());
}

#[test]
fn test_capture_fifo_to_file_nonblock() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fifo_path = temp_dir.path().join("capture-nonblock.fifo");
    let status = Command::new("mkfifo").arg(&fifo_path).status().unwrap();
    assert!(status.success());

    let expected = b"Hello world!".to_vec();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel();
    let writer = TestWriter {
        write_fn: Box::new({
            let seen = seen.clone();
            move |bytes| {
                seen.lock().unwrap().extend_from_slice(bytes);
                let _ = tx.send(());
                Ok(bytes.len())
            }
        }),
    };

    let machine =
        Machine::new_with_client(Config::default(), Box::new(MockClient::default())).unwrap();
    machine
        .capture_fifo_to_file(fifo_path.to_str().unwrap(), writer)
        .unwrap();

    std::thread::sleep(Duration::from_millis(250));

    let mut fifo = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&fifo_path)
        .unwrap();
    use std::io::Write as _;
    fifo.write_all(&expected).unwrap();

    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(expected, *seen.lock().unwrap());
}

#[test]
fn test_capture_fifo_to_file_with_channel_stops_on_exit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let fifo_path = temp_dir.path().join("capture-with-channel.fifo");
    let status = Command::new("mkfifo").arg(&fifo_path).status().unwrap();
    assert!(status.success());

    let machine =
        Machine::new_with_client(Config::default(), Box::new(MockClient::default())).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    machine
        .capture_fifo_to_file_with_channel(
            fifo_path.to_str().unwrap(),
            TestWriter {
                write_fn: Box::new(|bytes| Ok(bytes.len())),
            },
            done_tx,
        )
        .unwrap();

    machine.signal_exit();
    let result = done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(result.is_ok());
}
