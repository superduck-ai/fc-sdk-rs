use firecracker_sdk::{
    Config, Machine, MachineConfiguration, new_machine, with_memory_backend, with_snapshot,
};

fn main() -> Result<(), firecracker_sdk::Error> {
    let _base_machine = Machine::new(Config {
        socket_path: "/tmp/firecracker.sock".to_string(),
        kernel_image_path: "/path/to/kernel".to_string(),
        machine_cfg: MachineConfiguration::new(1, 256),
        ..Config::default()
    })?;

    let _snapshot_machine = new_machine(
        Config {
            socket_path: "/tmp/firecracker.sock".to_string(),
            machine_cfg: MachineConfiguration::new(1, 256),
            ..Config::default()
        },
        [with_snapshot(
            "",
            "/tmp/vm.snap",
            [with_memory_backend("File", "/tmp/vm.mem")],
        )],
    )?;

    Ok(())
}
