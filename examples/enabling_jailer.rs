use std::sync::Arc;

use firecracker_sdk::{
    Config, DrivesBuilder, JailerConfig, Machine, MachineConfiguration, NaiveChrootStrategy,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), firecracker_sdk::Error> {
    let kernel_image_path = "/path/to/kernel-image";

    let mut machine = Machine::new(Config {
        socket_path: "api.socket".to_string(),
        kernel_image_path: kernel_image_path.to_string(),
        kernel_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        drives: DrivesBuilder::new("/path/to/rootfs").build(),
        log_level: Some("Debug".to_string()),
        machine_cfg: MachineConfiguration::new(1, 256),
        jailer_cfg: Some(JailerConfig {
            uid: Some(123),
            gid: Some(100),
            id: "my-jailer-test".to_string(),
            numa_node: Some(0),
            chroot_base_dir: Some("/path/to/jailer-workspace".to_string()),
            chroot_strategy: Some(Arc::new(NaiveChrootStrategy::new(kernel_image_path))),
            exec_file: "/path/to/firecracker-binary".to_string(),
            ..JailerConfig::default()
        }),
        ..Config::default()
    })?;

    machine.start().await?;
    machine.wait().await?;

    Ok(())
}
