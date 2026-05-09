use serde::{Deserialize, Serialize};

pub const MMDS_VERSION_V1: &str = "V1";
pub const MMDS_VERSION_V2: &str = "V2";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MmdsConfig {
    #[serde(rename = "ipv4_address", skip_serializing_if = "Option::is_none")]
    pub ipv4_address: Option<String>,
    #[serde(rename = "network_interfaces", default)]
    pub network_interfaces: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}
