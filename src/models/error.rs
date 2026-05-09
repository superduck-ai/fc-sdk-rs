use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Error {
    #[serde(rename = "fault_message", skip_serializing_if = "Option::is_none")]
    pub fault_message: Option<String>,
}
