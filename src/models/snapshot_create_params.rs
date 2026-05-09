use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCreateParams {
    #[serde(rename = "mem_file_path", skip_serializing_if = "Option::is_none")]
    pub mem_file_path: Option<String>,
    #[serde(rename = "snapshot_path", skip_serializing_if = "Option::is_none")]
    pub snapshot_path: Option<String>,
}
