use serde::{Deserialize, Serialize};

use super::RateLimiter;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialNetworkInterface {
    #[serde(rename = "iface_id", skip_serializing_if = "Option::is_none")]
    pub iface_id: Option<String>,
    #[serde(rename = "rx_rate_limiter", skip_serializing_if = "Option::is_none")]
    pub rx_rate_limiter: Option<RateLimiter>,
    #[serde(rename = "tx_rate_limiter", skip_serializing_if = "Option::is_none")]
    pub tx_rate_limiter: Option<RateLimiter>,
}
