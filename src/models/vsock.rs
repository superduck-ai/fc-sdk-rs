use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VsockModel {
    #[serde(rename = "vsock_id", skip_serializing_if = "Option::is_none")]
    pub vsock_id: Option<String>,
    #[serde(rename = "guest_cid", skip_serializing_if = "Option::is_none")]
    pub guest_cid: Option<i64>,
    #[serde(rename = "uds_path", skip_serializing_if = "Option::is_none")]
    pub uds_path: Option<String>,
}
