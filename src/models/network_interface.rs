use serde::{Deserialize, Serialize};

use super::RateLimiter;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkInterfaceModel {
    #[serde(rename = "iface_id", skip_serializing_if = "Option::is_none")]
    pub iface_id: Option<String>,
    #[serde(rename = "guest_mac", skip_serializing_if = "Option::is_none")]
    pub guest_mac: Option<String>,
    #[serde(rename = "host_dev_name", skip_serializing_if = "Option::is_none")]
    pub host_dev_name: Option<String>,
    #[serde(rename = "rx_rate_limiter", skip_serializing_if = "Option::is_none")]
    pub rx_rate_limiter: Option<RateLimiter>,
    #[serde(rename = "tx_rate_limiter", skip_serializing_if = "Option::is_none")]
    pub tx_rate_limiter: Option<RateLimiter>,
}
