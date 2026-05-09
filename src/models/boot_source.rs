use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootSource {
    #[serde(rename = "kernel_image_path", skip_serializing_if = "Option::is_none")]
    pub kernel_image_path: Option<String>,
    #[serde(rename = "initrd_path", skip_serializing_if = "Option::is_none")]
    pub initrd_path: Option<String>,
    #[serde(rename = "boot_args", skip_serializing_if = "Option::is_none")]
    pub boot_args: Option<String>,
}
