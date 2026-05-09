use firecracker_sdk::{Config, Drive, MachineConfiguration, SnapshotConfig};

fn root_drive(path: &str) -> Drive {
    Drive {
        drive_id: Some("root".to_string()),
        is_root_device: Some(true),
        is_read_only: Some(true),
        path_on_host: Some(path.to_string()),
        ..Drive::default()
    }
}

#[test]
fn test_config_validate_requires_existing_kernel_image() {
    let error = Config {
        kernel_image_path: "/tmp/does-not-exist-vmlinux".to_string(),
        machine_cfg: MachineConfiguration::new(1, 128),
        ..Config::default()
    }
    .validate()
    .unwrap_err();

    assert_eq!(
        "invalid configuration: failed to stat kernel image path, \"/tmp/does-not-exist-vmlinux\": No such file or directory (os error 2)",
        error.to_string()
    );
}

#[test]
fn test_config_validate_checks_socket_path_and_root_drive() {
    let temp_dir = tempfile::tempdir().unwrap();
    let kernel_path = temp_dir.path().join("vmlinux.bin");
    let rootfs_path = temp_dir.path().join("rootfs.ext4");
    let socket_path = temp_dir.path().join("machine.sock");
    std::fs::File::create(&kernel_path).unwrap();
    std::fs::File::create(&rootfs_path).unwrap();
    std::fs::File::create(&socket_path).unwrap();

    let config = Config {
        socket_path: socket_path.display().to_string(),
        kernel_image_path: kernel_path.display().to_string(),
        drives: vec![root_drive(&rootfs_path.display().to_string())],
        machine_cfg: MachineConfiguration::new(1, 128),
        ..Config::default()
    };

    let error = config.validate().unwrap_err();
    assert_eq!(
        format!(
            "invalid configuration: socket {} already exists",
            socket_path.display()
        ),
        error.to_string()
    );

    std::fs::remove_file(&socket_path).unwrap();
    config.validate().unwrap();
}

#[test]
fn test_config_validate_load_snapshot_checks_snapshot_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let rootfs_path = temp_dir.path().join("rootfs.ext4");
    let mem_path = temp_dir.path().join("vm.mem");
    let snapshot_path = temp_dir.path().join("vm.snap");
    std::fs::File::create(&rootfs_path).unwrap();
    std::fs::File::create(&mem_path).unwrap();
    std::fs::File::create(&snapshot_path).unwrap();

    let config = Config {
        socket_path: temp_dir.path().join("machine.sock").display().to_string(),
        drives: vec![root_drive(&rootfs_path.display().to_string())],
        snapshot: SnapshotConfig::with_paths(
            mem_path.display().to_string(),
            snapshot_path.display().to_string(),
        ),
        ..Config::default()
    };

    config.validate_load_snapshot().unwrap();

    let missing_snapshot = Config {
        snapshot: SnapshotConfig::with_paths(
            mem_path.display().to_string(),
            temp_dir.path().join("missing.snap").display().to_string(),
        ),
        ..config.clone()
    };
    let error = missing_snapshot.validate_load_snapshot().unwrap_err();
    assert_eq!(
        format!(
            "invalid configuration: failed to stat snapshot path, {:?}: No such file or directory (os error 2)",
            temp_dir.path().join("missing.snap").display().to_string()
        ),
        error.to_string()
    );
}

#[test]
fn test_config_validate_respects_disable_validation() {
    Config {
        disable_validation: true,
        ..Config::default()
    }
    .validate()
    .unwrap();

    Config {
        disable_validation: true,
        snapshot: SnapshotConfig::with_paths("/missing.mem", "/missing.snap"),
        ..Config::default()
    }
    .validate_load_snapshot()
    .unwrap();
}
