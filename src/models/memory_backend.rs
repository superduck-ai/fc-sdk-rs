use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryBackend {
    #[serde(rename = "backend_type", skip_serializing_if = "Option::is_none")]
    pub backend_type: Option<String>,
    #[serde(rename = "backend_path", skip_serializing_if = "Option::is_none")]
    pub backend_path: Option<String>,
}
