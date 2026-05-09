use serde::{Deserialize, Serialize};

use super::{
    Balloon, BootSource, Drive, Logger, MachineConfiguration, Metrics, MmdsConfig,
    NetworkInterfaceModel, VsockModel,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullVmConfiguration {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balloon: Option<Balloon>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drives: Vec<Drive>,
    #[serde(rename = "boot-source", skip_serializing_if = "Option::is_none")]
    pub boot_source: Option<BootSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<Logger>,
    #[serde(rename = "machine-config", skip_serializing_if = "Option::is_none")]
    pub machine_config: Option<MachineConfiguration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Metrics>,
    #[serde(rename = "mmds-config", skip_serializing_if = "Option::is_none")]
    pub mmds_config: Option<MmdsConfig>,
    #[serde(
        rename = "network-interfaces",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub network_interfaces: Vec<NetworkInterfaceModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vsock: Option<VsockModel>,
}
