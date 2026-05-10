use std::time::Duration;

use firecracker_sdk::{
    Config, DrivesBuilder, Machine, MachineConfiguration, TokenBucketBuilder, new_rate_limiter,
    with_rate_limiter,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), firecracker_sdk::Error> {
    let limiter = new_rate_limiter(
        TokenBucketBuilder::default()
            .with_initial_size(1024 * 1024)
            .with_bucket_size(1024 * 1024)
            .with_refill_duration(Duration::from_millis(500))
            .build(),
        TokenBucketBuilder::default().build(),
        std::iter::empty(),
    );

    let drives = DrivesBuilder::new("/path/to/rootfs")
        .add_drive("/path/to/drive1.img", true, std::iter::empty())
        .add_drive("/path/to/drive2.img", false, [with_rate_limiter(limiter)])
        .build();

    let mut machine = Machine::new(Config {
        socket_path: "/tmp/firecracker.sock".to_string(),
        kernel_image_path: "/path/to/kernel".to_string(),
        drives,
        machine_cfg: MachineConfiguration::new(1, 256),
        ..Config::default()
    })?;

    machine.start().await?;
    machine.wait().await?;

    Ok(())
}
