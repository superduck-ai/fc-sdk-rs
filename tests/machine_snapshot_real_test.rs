mod real_vm_support;

use std::path::Path;

use firecracker_sdk::{
    Config, MachineConfiguration, VMCommandBuilder, new_machine, with_process_runner, with_snapshot,
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
        ..Config::default()
    }
}

#[test]
fn test_real_pause_create_snapshot_and_load_snapshot() {
    if !real_vm_support::assets_available() {
        return;
    }

    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path = real_vm_support::build_sleeping_rootfs(temp_dir.path(), "snapshot-rootfs");
    let socket_path = temp_dir.path().join("machine.sock");
    let mem_path = temp_dir.path().join("vm.mem");
    let snapshot_path = temp_dir.path().join("vm.snap");

    let mut machine = new_machine(
        base_config(&socket_path, "snapshot-source", &rootfs_path),
        [with_process_runner(make_vm_command(
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
    assert!(std::fs::metadata(&mem_path).unwrap().len() > 0);
    assert!(std::fs::metadata(&snapshot_path).unwrap().len() > 0);

    let restore_socket_path = temp_dir.path().join("restore.sock");
    let restore_config = base_config(&restore_socket_path, "snapshot-restore", &rootfs_path);

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
            with_process_runner(make_vm_command(&restore_socket_path, "snapshot-restore")),
        ],
    )
    .unwrap();

    restored_machine.start().unwrap();
    restored_machine.resume_vm().unwrap();
    restored_machine.stop_vmm().unwrap();
    let _ = restored_machine.wait();
}
