use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialDrive {
    #[serde(rename = "drive_id", skip_serializing_if = "Option::is_none")]
    pub drive_id: Option<String>,
    #[serde(rename = "path_on_host", skip_serializing_if = "Option::is_none")]
    pub path_on_host: Option<String>,
}
