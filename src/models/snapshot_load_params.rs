use serde::{Deserialize, Serialize};

use super::MemoryBackend;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotLoadParams {
    #[serde(rename = "mem_file_path", skip_serializing_if = "Option::is_none")]
    pub mem_file_path: Option<String>,
    #[serde(rename = "mem_backend", skip_serializing_if = "Option::is_none")]
    pub mem_backend: Option<MemoryBackend>,
    #[serde(rename = "snapshot_path", skip_serializing_if = "Option::is_none")]
    pub snapshot_path: Option<String>,
    #[serde(rename = "enable_diff_snapshots")]
    pub enable_diff_snapshots: bool,
    #[serde(rename = "resume_vm")]
    pub resume_vm: bool,
}
