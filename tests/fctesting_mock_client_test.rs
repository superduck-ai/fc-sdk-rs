use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::{
    AsyncResultExt, ClientOps, EntropyDevice, MachineConfiguration, RateLimiter, TokenBucket,
};
use serde_json::json;

#[test]
fn test_mock_client_supports_extended_configuration_hooks() {
    let mut client = MockClient {
        patch_machine_configuration_fn: Some(Box::new(|cfg| {
            assert_eq!(Some(true), cfg.track_dirty_pages);
            Ok(())
        })),
        put_cpu_configuration_fn: Some(Box::new(|cfg| {
            assert_eq!(json!({"reg_modifiers": {"x0": 1}}), *cfg);
            Ok(())
        })),
        put_entropy_device_fn: Some(Box::new(|device| {
            assert_eq!(
                Some(256),
                device
                    .rate_limiter
                    .as_ref()
                    .and_then(|limiter| limiter.bandwidth.as_ref())
                    .and_then(|bucket| bucket.size)
            );
            Ok(())
        })),
        ..MockClient::default()
    };

    client
        .patch_machine_configuration(&MachineConfiguration {
            track_dirty_pages: Some(true),
            ..MachineConfiguration::default()
        })
        .unwrap();
    client
        .put_cpu_configuration(&json!({"reg_modifiers": {"x0": 1}}))
        .unwrap();
    client
        .put_entropy_device(&EntropyDevice {
            rate_limiter: Some(RateLimiter {
                bandwidth: Some(TokenBucket {
                    size: Some(256),
                    ..TokenBucket::default()
                }),
                ..RateLimiter::default()
            }),
        })
        .unwrap();
}
