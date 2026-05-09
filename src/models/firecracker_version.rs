use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirecrackerVersion {
    #[serde(rename = "firecracker_version")]
    pub firecracker_version: String,
}
