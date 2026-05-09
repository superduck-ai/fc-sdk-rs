use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logger {
    #[serde(rename = "log_path", skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(rename = "show_level", skip_serializing_if = "Option::is_none")]
    pub show_level: Option<bool>,
    #[serde(rename = "show_log_origin", skip_serializing_if = "Option::is_none")]
    pub show_log_origin: Option<bool>,
}
