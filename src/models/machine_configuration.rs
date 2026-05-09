use serde::{Deserialize, Serialize};

use super::CpuTemplate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineConfiguration {
    #[serde(rename = "vcpu_count", skip_serializing_if = "Option::is_none")]
    pub vcpu_count: Option<i64>,
    #[serde(rename = "mem_size_mib", skip_serializing_if = "Option::is_none")]
    pub mem_size_mib: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smt: Option<bool>,
    #[serde(rename = "track_dirty_pages", skip_serializing_if = "Option::is_none")]
    pub track_dirty_pages: Option<bool>,
    #[serde(rename = "cpu_template", skip_serializing_if = "Option::is_none")]
    pub cpu_template: Option<CpuTemplate>,
}

impl MachineConfiguration {
    pub fn new(vcpu_count: i64, mem_size_mib: i64) -> Self {
        Self {
            vcpu_count: Some(vcpu_count),
            mem_size_mib: Some(mem_size_mib),
            ..Self::default()
        }
    }
}
