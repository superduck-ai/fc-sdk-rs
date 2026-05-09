use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceInfo {
    #[serde(rename = "app_name", skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(rename = "vmm_version", skip_serializing_if = "Option::is_none")]
    pub vmm_version: Option<String>,
    #[serde(flatten, default)]
    pub raw: BTreeMap<String, serde_json::Value>,
}
