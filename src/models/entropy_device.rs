use serde::{Deserialize, Serialize};

use super::RateLimiter;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntropyDevice {
    #[serde(rename = "rate_limiter", skip_serializing_if = "Option::is_none")]
    pub rate_limiter: Option<RateLimiter>,
}
