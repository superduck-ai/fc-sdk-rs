#![allow(non_snake_case)]

mod real_vm_support;

use std::os::unix::fs::MetadataExt;
use std::path::Path;

use firecracker_sdk::{
    AsyncResultExt, BlockingFutureExt, CommandStdio, Config, DEFAULT_JAILER_BIN,
    DEFAULT_JAILER_PATH, DEFAULT_SOCKET_PATH, Drive, JailerCommandBuilder, JailerConfig, Machine,
    NaiveChrootStrategy, NoopClient, ROOTFS_FOLDER_NAME, get_numa_cpuset,
};

fn real_firecracker_binary() -> &'static str {
    "/data/firecracker-rs-sdk2/testdata/firecracker"
}

fn real_jailer_binary() -> &'static str {
    "/data/firecracker-rs-sdk2/testdata/jailer"
}

fn real_kernel_path() -> &'static str {
    "/data/firecracker-rs-sdk2/testdata/vmlinux"
}

fn real_jailer_assets_available() -> bool {
    real_vm_support::assets_available()
        && Path::new(real_firecracker_binary()).exists()
        && Path::new(real_jailer_binary()).exists()
        && Path::new(real_kernel_path()).exists()
}

fn cpuset_args(node: i32) -> Vec<String> {
    let cpuset = get_numa_cpuset(node);
    if cpuset.is_empty() {
        Vec::new()
    } else {
        vec![
            "--cgroup".to_string(),
            format!("cpuset.mems={node}"),
            "--cgroup".to_string(),
            format!("cpuset.cpus={cpuset}"),
        ]
    }
}

fn base_jailer_config() -> JailerConfig {
    JailerConfig {
        id: "my-test-id".to_string(),
        uid: Some(123),
        gid: Some(100),
        numa_node: Some(0),
        exec_file: "/path/to/firecracker".to_string(),
        chroot_strategy: Some(std::sync::Arc::new(NaiveChrootStrategy::new(
            "kernel-image-path",
        ))),
        ..JailerConfig::default()
    }
}

#[test]
fn TestJailerBuilder() {
    let cases = vec![
        ("required fields", base_jailer_config(), None, {
            let mut expected = vec![
                DEFAULT_JAILER_BIN.to_string(),
                "--id".to_string(),
                "my-test-id".to_string(),
                "--uid".to_string(),
                "123".to_string(),
                "--gid".to_string(),
                "100".to_string(),
                "--exec-file".to_string(),
                "/path/to/firecracker".to_string(),
            ];
            expected.extend(cpuset_args(0));
            expected
        }),
        (
            "other jailer binary name",
            JailerConfig {
                jailer_binary: Some("imprisoner".to_string()),
                ..base_jailer_config()
            },
            None,
            {
                let mut expected = vec![
                    "imprisoner".to_string(),
                    "--id".to_string(),
                    "my-test-id".to_string(),
                    "--uid".to_string(),
                    "123".to_string(),
                    "--gid".to_string(),
                    "100".to_string(),
                    "--exec-file".to_string(),
                    "/path/to/firecracker".to_string(),
                ];
                expected.extend(cpuset_args(0));
                expected
            },
        ),
        (
            "optional fields",
            JailerConfig {
                jailer_binary: Some("/path/to/the/jailer".to_string()),
                chroot_base_dir: Some("/tmp".to_string()),
                cgroup_version: Some("2".to_string()),
                cgroup_args: vec!["cpu.shares=10".to_string()],
                parent_cgroup: Some("/path/to/parent-cgroup".to_string()),
                ..base_jailer_config()
            },
            Some("/path/to/netns"),
            {
                let mut expected = vec![
                    "/path/to/the/jailer".to_string(),
                    "--id".to_string(),
                    "my-test-id".to_string(),
                    "--uid".to_string(),
                    "123".to_string(),
                    "--gid".to_string(),
                    "100".to_string(),
                    "--exec-file".to_string(),
                    "/path/to/firecracker".to_string(),
                ];
                expected.extend(cpuset_args(0));
                expected.extend([
                    "--cgroup".to_string(),
                    "cpu.shares=10".to_string(),
                    "--cgroup-version".to_string(),
                    "2".to_string(),
                    "--parent-cgroup".to_string(),
                    "/path/to/parent-cgroup".to_string(),
                    "--chroot-base-dir".to_string(),
                    "/tmp".to_string(),
                    "--netns".to_string(),
                    "/path/to/netns".to_string(),
                ]);
                expected
            },
        ),
    ];

    for (_, jailer_cfg, netns, expected) in cases {
        let mut builder = JailerCommandBuilder::new()
            .with_id(jailer_cfg.id.clone())
            .with_uid(jailer_cfg.uid.unwrap())
            .with_gid(jailer_cfg.gid.unwrap())
            .with_numa_node(jailer_cfg.numa_node.unwrap())
            .with_exec_file(jailer_cfg.exec_file.clone());

        if let Some(jailer_binary) = jailer_cfg.jailer_binary.clone() {
            builder = builder.with_bin(jailer_binary);
        }
        if !jailer_cfg.cgroup_args.is_empty() {
            builder = builder.with_cgroup_args(jailer_cfg.cgroup_args.clone());
        }
        if let Some(chroot_base_dir) = jailer_cfg.chroot_base_dir.clone() {
            builder = builder.with_chroot_base_dir(chroot_base_dir);
        }
        if let Some(cgroup_version) = jailer_cfg.cgroup_version.clone() {
            builder = builder.with_cgroup_version(cgroup_version);
        }
        if let Some(parent_cgroup) = jailer_cfg.parent_cgroup.clone() {
            builder = builder.with_parent_cgroup(parent_cgroup);
        }
        if let Some(netns) = netns {
            builder = builder.with_netns(netns);
        }

        let command = builder.build();
        assert_eq!(expected, command.argv());
    }
}

#[test]
fn TestJail() {
    let cases = vec![
        (
            "required fields",
            Config {
                vmid: "vmid".to_string(),
                jailer_cfg: Some(base_jailer_config()),
                ..Config::default()
            },
            {
                let mut expected = vec![
                    DEFAULT_JAILER_BIN.to_string(),
                    "--id".to_string(),
                    "my-test-id".to_string(),
                    "--uid".to_string(),
                    "123".to_string(),
                    "--gid".to_string(),
                    "100".to_string(),
                    "--exec-file".to_string(),
                    "/path/to/firecracker".to_string(),
                ];
                expected.extend(cpuset_args(0));
                expected.extend([
                    "--".to_string(),
                    "--no-seccomp".to_string(),
                    "--api-sock".to_string(),
                    DEFAULT_SOCKET_PATH.to_string(),
                ]);
                expected
            },
            format!(
                "{DEFAULT_JAILER_PATH}/firecracker/my-test-id/{ROOTFS_FOLDER_NAME}/run/firecracker.socket"
            ),
        ),
        (
            "other jailer binary name",
            Config {
                vmid: "vmid".to_string(),
                jailer_cfg: Some(JailerConfig {
                    jailer_binary: Some("imprisoner".to_string()),
                    ..base_jailer_config()
                }),
                ..Config::default()
            },
            {
                let mut expected = vec![
                    "imprisoner".to_string(),
                    "--id".to_string(),
                    "my-test-id".to_string(),
                    "--uid".to_string(),
                    "123".to_string(),
                    "--gid".to_string(),
                    "100".to_string(),
                    "--exec-file".to_string(),
                    "/path/to/firecracker".to_string(),
                ];
                expected.extend(cpuset_args(0));
                expected.extend([
                    "--".to_string(),
                    "--no-seccomp".to_string(),
                    "--api-sock".to_string(),
                    DEFAULT_SOCKET_PATH.to_string(),
                ]);
                expected
            },
            format!(
                "{DEFAULT_JAILER_PATH}/firecracker/my-test-id/{ROOTFS_FOLDER_NAME}/run/firecracker.socket"
            ),
        ),
        (
            "optional fields",
            Config {
                vmid: "vmid".to_string(),
                net_ns: Some("/path/to/netns".to_string()),
                jailer_cfg: Some(JailerConfig {
                    jailer_binary: Some("/path/to/the/jailer".to_string()),
                    chroot_base_dir: Some("/tmp".to_string()),
                    cgroup_version: Some("2".to_string()),
                    parent_cgroup: Some("/path/to/parent-cgroup".to_string()),
                    ..base_jailer_config()
                }),
                ..Config::default()
            },
            {
                let mut expected = vec![
                    "/path/to/the/jailer".to_string(),
                    "--id".to_string(),
                    "my-test-id".to_string(),
                    "--uid".to_string(),
                    "123".to_string(),
                    "--gid".to_string(),
                    "100".to_string(),
                    "--exec-file".to_string(),
                    "/path/to/firecracker".to_string(),
                ];
                expected.extend(cpuset_args(0));
                expected.extend([
                    "--cgroup-version".to_string(),
                    "2".to_string(),
                    "--parent-cgroup".to_string(),
                    "/path/to/parent-cgroup".to_string(),
                    "--chroot-base-dir".to_string(),
                    "/tmp".to_string(),
                    "--netns".to_string(),
                    "/path/to/netns".to_string(),
                    "--".to_string(),
                    "--no-seccomp".to_string(),
                    "--api-sock".to_string(),
                    DEFAULT_SOCKET_PATH.to_string(),
                ]);
                expected
            },
            format!("/tmp/firecracker/my-test-id/{ROOTFS_FOLDER_NAME}/run/firecracker.socket"),
        ),
        (
            "custom socket path",
            Config {
                socket_path: "api.sock".to_string(),
                vmid: "vmid".to_string(),
                jailer_cfg: Some(base_jailer_config()),
                ..Config::default()
            },
            {
                let mut expected = vec![
                    DEFAULT_JAILER_BIN.to_string(),
                    "--id".to_string(),
                    "my-test-id".to_string(),
                    "--uid".to_string(),
                    "123".to_string(),
                    "--gid".to_string(),
                    "100".to_string(),
                    "--exec-file".to_string(),
                    "/path/to/firecracker".to_string(),
                ];
                expected.extend(cpuset_args(0));
                expected.extend([
                    "--".to_string(),
                    "--no-seccomp".to_string(),
                    "--api-sock".to_string(),
                    "api.sock".to_string(),
                ]);
                expected
            },
            format!("{DEFAULT_JAILER_PATH}/firecracker/my-test-id/{ROOTFS_FOLDER_NAME}/api.sock"),
        ),
    ];

    for (_, config, expected_args, expected_socket_path) in cases {
        let machine = Machine::new_with_client(config, Box::new(NoopClient)).unwrap();
        let command = machine.command.as_ref().unwrap();

        assert_eq!(expected_args, command.argv());
        assert_eq!(expected_socket_path, machine.cfg.socket_path);
        assert!(
            machine
                .handlers
                .fc_init
                .has(firecracker_sdk::LINK_FILES_TO_ROOTFS_HANDLER_NAME)
        );
    }
}

#[test]
fn test_jailer_builder_required_fields() {
    let command = JailerCommandBuilder::new()
        .with_id("my-test-id")
        .with_uid(123)
        .with_gid(100)
        .with_numa_node(0)
        .with_exec_file("/path/to/firecracker")
        .build();

    let mut expected = vec![
        DEFAULT_JAILER_BIN.to_string(),
        "--id".to_string(),
        "my-test-id".to_string(),
        "--uid".to_string(),
        "123".to_string(),
        "--gid".to_string(),
        "100".to_string(),
        "--exec-file".to_string(),
        "/path/to/firecracker".to_string(),
    ];
    expected.extend(cpuset_args(0));

    assert_eq!(expected, command.argv());
}

#[test]
fn test_jailer_builder_optional_fields() {
    let command = JailerCommandBuilder::new()
        .with_bin("/path/to/the/jailer")
        .with_id("my-test-id")
        .with_uid(123)
        .with_gid(100)
        .with_numa_node(0)
        .with_exec_file("/path/to/firecracker")
        .with_cgroup_args(["cpu.shares=10"])
        .with_cgroup_version("2")
        .with_parent_cgroup("/path/to/parent-cgroup")
        .with_chroot_base_dir("/tmp")
        .with_netns("/path/to/netns")
        .build();

    let mut expected = vec![
        "/path/to/the/jailer".to_string(),
        "--id".to_string(),
        "my-test-id".to_string(),
        "--uid".to_string(),
        "123".to_string(),
        "--gid".to_string(),
        "100".to_string(),
        "--exec-file".to_string(),
        "/path/to/firecracker".to_string(),
    ];
    expected.extend(cpuset_args(0));
    expected.extend([
        "--cgroup".to_string(),
        "cpu.shares=10".to_string(),
        "--cgroup-version".to_string(),
        "2".to_string(),
        "--parent-cgroup".to_string(),
        "/path/to/parent-cgroup".to_string(),
        "--chroot-base-dir".to_string(),
        "/tmp".to_string(),
        "--netns".to_string(),
        "/path/to/netns".to_string(),
    ]);

    assert_eq!(expected, command.argv());
}

#[test]
fn test_machine_new_with_jailer_sets_command_and_socket_path() {
    let config = Config {
        vmid: "vmid".to_string(),
        jailer_cfg: Some(JailerConfig {
            id: "my-test-id".to_string(),
            uid: Some(123),
            gid: Some(100),
            numa_node: Some(0),
            exec_file: "/path/to/firecracker".to_string(),
            chroot_strategy: Some(std::sync::Arc::new(NaiveChrootStrategy::new(
                "kernel-image-path",
            ))),
            ..JailerConfig::default()
        }),
        ..Config::default()
    };

    let machine = Machine::new_with_client(config, Box::new(NoopClient)).unwrap();
    let command = machine.command.as_ref().unwrap();

    let mut expected = vec![
        DEFAULT_JAILER_BIN.to_string(),
        "--id".to_string(),
        "my-test-id".to_string(),
        "--uid".to_string(),
        "123".to_string(),
        "--gid".to_string(),
        "100".to_string(),
        "--exec-file".to_string(),
        "/path/to/firecracker".to_string(),
    ];
    expected.extend(cpuset_args(0));
    expected.extend([
        "--".to_string(),
        "--no-seccomp".to_string(),
        "--api-sock".to_string(),
        DEFAULT_SOCKET_PATH.to_string(),
    ]);

    assert_eq!(expected, command.argv());
    assert_eq!(
        format!(
            "{DEFAULT_JAILER_PATH}/firecracker/my-test-id/{ROOTFS_FOLDER_NAME}/run/firecracker.socket"
        ),
        machine.cfg.socket_path
    );
    assert!(
        machine
            .handlers
            .fc_init
            .has(firecracker_sdk::LINK_FILES_TO_ROOTFS_HANDLER_NAME)
    );
}

#[test]
fn test_machine_new_with_jailer_custom_socket_path() {
    let config = Config {
        socket_path: "api.sock".to_string(),
        vmid: "vmid".to_string(),
        jailer_cfg: Some(JailerConfig {
            id: "my-test-id".to_string(),
            uid: Some(123),
            gid: Some(100),
            numa_node: Some(0),
            exec_file: "/path/to/firecracker".to_string(),
            chroot_strategy: Some(std::sync::Arc::new(NaiveChrootStrategy::new(
                "kernel-image-path",
            ))),
            ..JailerConfig::default()
        }),
        ..Config::default()
    };

    let machine = Machine::new_with_client(config, Box::new(NoopClient)).unwrap();
    assert_eq!(
        format!("{DEFAULT_JAILER_PATH}/firecracker/my-test-id/{ROOTFS_FOLDER_NAME}/api.sock"),
        machine.cfg.socket_path
    );
}

#[test]
fn test_jailer_builder_preserves_stdio_configuration() {
    let command = JailerCommandBuilder::new()
        .with_id("my-test-id")
        .with_uid(123)
        .with_gid(100)
        .with_numa_node(0)
        .with_exec_file("/path/to/firecracker")
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Path("/tmp/jailer.stdout".to_string()))
        .with_stderr(CommandStdio::Path("/tmp/jailer.stderr".to_string()))
        .build();

    assert_eq!(CommandStdio::Null, command.stdin);
    assert_eq!(
        CommandStdio::Path("/tmp/jailer.stdout".to_string()),
        command.stdout
    );
    assert_eq!(
        CommandStdio::Path("/tmp/jailer.stderr".to_string()),
        command.stderr
    );
}

#[test]
fn test_link_files_handler_links_assets_into_rootfs_and_rewrites_paths() {
    let temp_dir = tempfile::tempdir().unwrap();
    let chroot_base_dir = temp_dir.path().join("jailer");
    let rootfs = chroot_base_dir
        .join("firecracker")
        .join("my-test-id")
        .join(ROOTFS_FOLDER_NAME);
    std::fs::create_dir_all(&rootfs).unwrap();

    let kernel_path = temp_dir.path().join("vmlinux.bin");
    let initrd_path = temp_dir.path().join("initrd.img");
    let drive_path = temp_dir.path().join("rootfs.ext4");
    std::fs::write(&kernel_path, b"kernel").unwrap();
    std::fs::write(&initrd_path, b"initrd").unwrap();
    std::fs::write(&drive_path, b"drive").unwrap();

    let mut machine = Machine::new_with_client(
        Config {
            kernel_image_path: kernel_path.display().to_string(),
            initrd_path: Some(initrd_path.display().to_string()),
            drives: vec![Drive {
                drive_id: Some("root".to_string()),
                path_on_host: Some(drive_path.display().to_string()),
                is_root_device: Some(true),
                is_read_only: Some(false),
                ..Drive::default()
            }],
            jailer_cfg: Some(JailerConfig {
                id: "my-test-id".to_string(),
                uid: Some(0),
                gid: Some(0),
                numa_node: Some(0),
                exec_file: "/usr/local/bin/firecracker".to_string(),
                chroot_base_dir: Some(chroot_base_dir.display().to_string()),
                chroot_strategy: Some(std::sync::Arc::new(NaiveChrootStrategy::new(
                    kernel_path.display().to_string(),
                ))),
                ..JailerConfig::default()
            }),
            ..Config::default()
        },
        Box::new(NoopClient),
    )
    .unwrap();

    let handler = machine
        .handlers
        .fc_init
        .list
        .iter()
        .find(|handler| handler.name == firecracker_sdk::LINK_FILES_TO_ROOTFS_HANDLER_NAME)
        .cloned()
        .unwrap();

    (handler.func)(&mut machine).unwrap();

    let linked_kernel = rootfs.join("vmlinux.bin");
    let linked_initrd = rootfs.join("initrd.img");
    let linked_drive = rootfs.join("rootfs.ext4");

    assert_eq!("vmlinux.bin", machine.cfg.kernel_image_path);
    assert_eq!(Some("initrd.img".to_string()), machine.cfg.initrd_path);
    assert_eq!(
        Some("rootfs.ext4".to_string()),
        machine.cfg.drives[0].path_on_host
    );

    assert!(linked_kernel.exists());
    assert!(linked_initrd.exists());
    assert!(linked_drive.exists());
    assert_eq!(
        std::fs::metadata(&kernel_path).unwrap().ino(),
        std::fs::metadata(&linked_kernel).unwrap().ino()
    );
    assert_eq!(
        std::fs::metadata(&initrd_path).unwrap().ino(),
        std::fs::metadata(&linked_initrd).unwrap().ino()
    );
    assert_eq!(
        std::fs::metadata(&drive_path).unwrap().ino(),
        std::fs::metadata(&linked_drive).unwrap().ino()
    );
}

#[test]
fn test_real_jailer_execution_starts_vm() {
    if !real_jailer_assets_available() {
        return;
    }

    let workspace_dir = Path::new("/tmp").join(format!("j{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&workspace_dir);
    std::fs::create_dir_all(&workspace_dir).unwrap();

    let kernel_copy = workspace_dir.join("vmlinux.bin");
    std::fs::copy(real_kernel_path(), &kernel_copy).unwrap();
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
        machine_cfg: firecracker_sdk::MachineConfiguration::new(1, 256),
        disable_validation: true,
        forward_signals: Some(Vec::new()),
        jailer_cfg: Some(JailerConfig {
            id: "b".to_string(),
            uid: Some(0),
            gid: Some(0),
            numa_node: Some(0),
            exec_file: real_firecracker_binary().to_string(),
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

    if let Err(error) = machine.start().block_on() {
        let socket_metadata = std::fs::metadata(&machine.cfg.socket_path);
        let socket_exists = socket_metadata.is_ok();
        let client_result = machine.client.get_machine_configuration().block_on();
        panic!(
            "start failed: {error}; socket_path={}; socket_exists={socket_exists}; client_result={client_result:?}",
            machine.cfg.socket_path
        );
    }
    assert!(machine.pid().unwrap() > 0);
    assert!(Path::new(&machine.cfg.socket_path).exists());

    machine.stop_vmm().unwrap();
    assert!(machine.wait().is_err());

    let _ = std::fs::remove_dir_all(&workspace_dir);
}
