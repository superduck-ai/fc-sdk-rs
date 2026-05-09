use serde::{Deserialize, Serialize};

pub const INSTANCE_ACTION_INSTANCE_START: &str = "InstanceStart";
pub const INSTANCE_ACTION_SEND_CTRL_ALT_DEL: &str = "SendCtrlAltDel";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceActionInfo {
    #[serde(rename = "action_type", skip_serializing_if = "Option::is_none")]
    pub action_type: Option<String>,
}
