mod real_vm_support;

use std::path::Path;
use std::thread;
use std::time::Duration;

use firecracker_sdk::{
    Config, FifoLogWriter, MachineConfiguration, VMCommandBuilder, new_machine,
    with_process_runner, with_snapshot,
};

fn make_vm_command(socket_path: &Path, vmid: &str) -> firecracker_sdk::VMCommand {
    VMCommandBuilder::default()
        .with_bin(real_vm_support::firecracker_binary())
        .with_socket_path(socket_path.display().to_string())
        .with_args(["--id", vmid, "--no-seccomp"])
        .build()
}

fn base_config(socket_path: &Path, vmid: &str, rootfs_path: &Path) -> Config {
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

#[test]
fn test_real_log_and_metrics_paths_write_files() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path = real_vm_support::build_sleeping_rootfs(temp_dir.path(), "log-metrics-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let log_path = temp_dir.path().join("firecracker.log");
    let metrics_path = temp_dir.path().join("firecracker.metrics");
    std::fs::File::create(&log_path).unwrap();
    std::fs::File::create(&metrics_path).unwrap();

    let mut machine = new_machine(
        Config {
            log_path: Some(log_path.display().to_string()),
            log_level: Some("Debug".to_string()),
            metrics_path: Some(metrics_path.display().to_string()),
            ..base_config(&socket_path, "log-metrics", &rootfs_path)
        },
        [with_process_runner(make_vm_command(
            &socket_path,
            "log-metrics",
        ))],
    )
    .unwrap();

    machine.start().unwrap();
    thread::sleep(Duration::from_millis(250));
    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    let log_contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(!log_contents.trim().is_empty());
    assert!(log_contents.contains("firecracker") || log_contents.contains("VMM"));

    let metrics_contents = std::fs::read_to_string(&metrics_path).unwrap();
    assert!(!metrics_contents.trim().is_empty());
    assert!(metrics_contents.contains("api_server"));
}

#[test]
fn test_real_log_fifo_writer_captures_vmm_output() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path =
        real_vm_support::build_sleeping_rootfs(temp_dir.path(), "log-fifo-writer-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let log_fifo = temp_dir.path().join("firecracker.log");
    let captured_log = temp_dir.path().join("captured.log");
    let writer = std::fs::OpenOptions::new()
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
            ..base_config(&socket_path, "log-fifo-writer", &rootfs_path)
        },
        [with_process_runner(make_vm_command(
            &socket_path,
            "log-fifo-writer",
        ))],
    )
    .unwrap();

    machine.start().unwrap();
    thread::sleep(Duration::from_millis(250));
    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    let log_contents = std::fs::read_to_string(&captured_log).unwrap();
    assert!(!log_contents.trim().is_empty());
}

#[test]
fn test_real_pause_resume_requires_running_machine() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path =
        real_vm_support::build_sleeping_rootfs(temp_dir.path(), "pause-resume-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");

    let mut machine = new_machine(
        base_config(&socket_path, "pause-resume", &rootfs_path),
        [with_process_runner(make_vm_command(
            &socket_path,
            "pause-resume",
        ))],
    )
    .unwrap();

    assert!(machine.pause_vm().is_err());
    assert!(machine.resume_vm().is_err());

    machine.start().unwrap();
    machine.pause_vm().unwrap();
    machine.pause_vm().unwrap();
    machine.resume_vm().unwrap();
    machine.resume_vm().unwrap();
    machine.pause_vm().unwrap();
    machine.resume_vm().unwrap();

    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    assert!(machine.pause_vm().is_err());
    assert!(machine.resume_vm().is_err());
}

#[test]
fn test_real_create_snapshot_requires_paused_vm() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path =
        real_vm_support::build_sleeping_rootfs(temp_dir.path(), "snapshot-unpaused-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let mem_path = temp_dir.path().join("vm.mem");
    let snapshot_path = temp_dir.path().join("vm.snap");

    let mut machine = new_machine(
        base_config(&socket_path, "snapshot-unpaused", &rootfs_path),
        [with_process_runner(make_vm_command(
            &socket_path,
            "snapshot-unpaused",
        ))],
    )
    .unwrap();

    machine.start().unwrap();
    let error = machine.create_snapshot(
        &mem_path.display().to_string(),
        &snapshot_path.display().to_string(),
    );
    assert!(error.is_err());

    machine.stop_vmm().unwrap();
    let _ = machine.wait();

    assert!(!mem_path.exists() || std::fs::metadata(&mem_path).unwrap().len() == 0);
    assert!(!snapshot_path.exists() || std::fs::metadata(&snapshot_path).unwrap().len() == 0);
}

#[test]
fn test_real_load_snapshot_without_existing_files_fails() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path =
        real_vm_support::build_sleeping_rootfs(temp_dir.path(), "snapshot-missing-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let mem_path = temp_dir.path().join("missing.mem");
    let snapshot_path = temp_dir.path().join("missing.snap");

    let mut machine = new_machine(
        base_config(&socket_path, "snapshot-missing", &rootfs_path),
        [
            with_snapshot(
                "",
                snapshot_path.display().to_string(),
                [firecracker_sdk::with_memory_backend(
                    "File",
                    mem_path.display().to_string(),
                )],
            ),
            with_process_runner(make_vm_command(&socket_path, "snapshot-missing")),
        ],
    )
    .unwrap();

    assert!(machine.start().is_err());
}
