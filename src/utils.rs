use std::time::{Duration, Instant};

use crate::client::ClientOps;
use crate::error::{Error, Result};

pub const DEFAULT_ALIVE_VMM_CHECK_DUR: Duration = Duration::from_millis(10);

pub fn env_value_or_default_int(env_name: &str, default_value: i32) -> i32 {
    std::env::var(env_name)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value != 0)
        .unwrap_or(default_value)
}

pub async fn wait_for_alive_vmm(client: &mut dyn ClientOps, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    let mut last_error = None;

    while start.elapsed() < timeout {
        match client.get_machine_configuration().await {
            Ok(_) => return Ok(()),
            Err(error) => {
                last_error = Some(error.to_string());
            }
        }
        tokio::time::sleep(DEFAULT_ALIVE_VMM_CHECK_DUR).await;
    }

    Err(Error::Process(format!(
        "timed out while waiting for the Firecracker VMM to become reachable{}",
        last_error
            .as_ref()
            .map(|error| format!("; last error: {error}"))
            .unwrap_or_default()
    )))
}
