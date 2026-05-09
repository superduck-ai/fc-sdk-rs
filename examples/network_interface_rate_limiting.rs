use std::time::Duration;

use firecracker_sdk::{
    Config, DrivesBuilder, Machine, MachineConfiguration, NetworkInterface, NetworkInterfaces,
    StaticNetworkConfiguration, TokenBucketBuilder, new_rate_limiter,
};

fn main() -> Result<(), firecracker_sdk::Error> {
    let inbound = new_rate_limiter(
        TokenBucketBuilder::default()
            .with_initial_size(1024 * 1024)
            .with_bucket_size(1024 * 1024)
            .with_refill_duration(Duration::from_secs(30))
            .build(),
        TokenBucketBuilder::default()
            .with_initial_size(5)
            .with_bucket_size(5)
            .with_refill_duration(Duration::from_secs(5))
            .build(),
        std::iter::empty(),
    );

    let outbound = new_rate_limiter(
        TokenBucketBuilder::default()
            .with_initial_size(100)
            .with_bucket_size(1024 * 1024 * 10)
            .with_refill_duration(Duration::from_secs(30))
            .build(),
        TokenBucketBuilder::default()
            .with_initial_size(100)
            .with_bucket_size(100)
            .with_refill_duration(Duration::from_secs(5))
            .build(),
        std::iter::empty(),
    );

    let network_interfaces = NetworkInterfaces::from(vec![NetworkInterface {
        static_configuration: Some(
            StaticNetworkConfiguration::new("tap-name").with_mac_address("01-23-45-67-89-AB-CD-EF"),
        ),
        in_rate_limiter: Some(inbound),
        out_rate_limiter: Some(outbound),
        ..NetworkInterface::default()
    }]);

    let mut machine = Machine::new(Config {
        socket_path: "/tmp/firecracker.sock".to_string(),
        kernel_image_path: "/path/to/kernel".to_string(),
        drives: DrivesBuilder::new("/path/to/rootfs").build(),
        machine_cfg: MachineConfiguration::new(1, 256),
        network_interfaces,
        ..Config::default()
    })?;

    machine.start()?;
    machine.wait()?;

    Ok(())
}
