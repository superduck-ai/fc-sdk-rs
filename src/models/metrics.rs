use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    #[serde(rename = "metrics_path", skip_serializing_if = "Option::is_none")]
    pub metrics_path: Option<String>,
}
