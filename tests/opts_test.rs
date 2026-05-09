use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::{
    CREATE_BOOT_SOURCE_HANDLER_NAME, CREATE_MACHINE_HANDLER_NAME, Config, FirecrackerVersion,
    LOAD_SNAPSHOT_HANDLER_NAME, Machine, MachineConfiguration, SnapshotConfig, VMCommandBuilder,
    new_machine, with_client, with_memory_backend, with_process_runner, with_snapshot,
};

#[test]
fn test_new_machine_with_process_runner_option() {
    let command = VMCommandBuilder::default()
        .with_bin("/custom/firecracker")
        .with_socket_path("/tmp/custom.sock")
        .with_args(["--id", "custom-vm"])
        .build();

    let machine = new_machine(
        Config {
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [with_process_runner(command.clone())],
    )
    .unwrap();

    assert_eq!(Some(command), machine.command);
}

#[test]
fn test_new_machine_with_client_option() {
    let mut machine = new_machine(
        Config {
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [with_client(Box::new(MockClient {
            get_firecracker_version_fn: Some(Box::new(|| {
                Ok(FirecrackerVersion {
                    firecracker_version: "1.2.3".to_string(),
                })
            })),
            ..MockClient::default()
        }))],
    )
    .unwrap();

    assert_eq!("1.2.3", machine.get_firecracker_version().unwrap());
}

#[test]
fn test_with_snapshot_option_updates_config_and_handlers() {
    let machine = Machine::new_with_opts(
        Config {
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [with_snapshot(
            "",
            "/tmp/snapshot",
            [with_memory_backend("File", "/tmp/memory")],
        )],
    )
    .unwrap();

    assert_eq!(None, machine.cfg.snapshot.mem_file_path);
    assert_eq!(
        Some("/tmp/snapshot".to_string()),
        machine.cfg.snapshot.snapshot_path
    );
    assert_eq!(
        Some("/tmp/memory"),
        machine.cfg.snapshot.get_mem_backend_path()
    );
    assert_eq!(
        Some("File".to_string()),
        machine
            .cfg
            .snapshot
            .mem_backend
            .as_ref()
            .and_then(|backend| backend.backend_type.clone())
    );

    let handler_names = machine
        .handlers
        .fc_init
        .list
        .iter()
        .map(|handler| handler.name.as_str())
        .collect::<Vec<_>>();

    assert!(handler_names.contains(&LOAD_SNAPSHOT_HANDLER_NAME));
    assert!(!handler_names.contains(&CREATE_MACHINE_HANDLER_NAME));
    assert!(!handler_names.contains(&CREATE_BOOT_SOURCE_HANDLER_NAME));
}

#[test]
fn test_with_snapshot_option_does_not_duplicate_snapshot_handlers() {
    let machine = Machine::new_with_opts(
        Config {
            machine_cfg: MachineConfiguration::new(1, 128),
            snapshot: SnapshotConfig::with_paths("", "/tmp/snapshot"),
            ..Config::default()
        },
        [with_snapshot(
            "",
            "/tmp/snapshot",
            [with_memory_backend("File", "/tmp/memory")],
        )],
    )
    .unwrap();

    let load_snapshot_handler_count = machine
        .handlers
        .fc_init
        .list
        .iter()
        .filter(|handler| handler.name == LOAD_SNAPSHOT_HANDLER_NAME)
        .count();

    assert_eq!(1, load_snapshot_handler_count);
}
