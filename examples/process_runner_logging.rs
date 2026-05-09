use firecracker_sdk::{
    Config, DrivesBuilder, MachineConfiguration, VMCommandBuilder, new_machine, with_process_runner,
};

fn main() -> Result<(), firecracker_sdk::Error> {
    let socket_path = "/tmp/firecracker.sock";
    let command = VMCommandBuilder::default()
        .with_bin("firecracker")
        .with_socket_path(socket_path)
        .with_stdout_path("/tmp/stdout.log")
        .with_stderr_path("/tmp/stderr.log")
        .build();

    let mut machine = new_machine(
        Config {
            socket_path: socket_path.to_string(),
            kernel_image_path: "/path/to/kernel".to_string(),
            drives: DrivesBuilder::new("/path/to/rootfs").build(),
            machine_cfg: MachineConfiguration::new(1, 256),
            ..Config::default()
        },
        [with_process_runner(command)],
    )?;

    machine.start()?;
    machine.wait()?;

    Ok(())
}
