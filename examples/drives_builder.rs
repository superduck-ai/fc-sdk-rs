use firecracker_sdk::{Config, DrivesBuilder, Machine, MachineConfiguration};

fn main() -> Result<(), firecracker_sdk::Error> {
    let drives = DrivesBuilder::new("/path/to/rootfs")
        .add_drive("/first/path/drive.img", true, std::iter::empty())
        .add_drive("/second/path/drive.img", false, std::iter::empty())
        .build();

    let mut machine = Machine::new(Config {
        socket_path: "/tmp/firecracker.sock".to_string(),
        kernel_image_path: "/path/to/kernel".to_string(),
        drives,
        machine_cfg: MachineConfiguration::new(1, 256),
        ..Config::default()
    })?;

    machine.start()?;
    machine.wait()?;

    Ok(())
}
