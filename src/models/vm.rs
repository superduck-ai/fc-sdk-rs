use serde::{Deserialize, Serialize};

pub const VM_STATE_PAUSED: &str = "Paused";
pub const VM_STATE_RESUMED: &str = "Resumed";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vm {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

impl Vm {
    pub fn paused() -> Self {
        Self {
            state: Some(VM_STATE_PAUSED.to_string()),
        }
    }

    pub fn resumed() -> Self {
        Self {
            state: Some(VM_STATE_RESUMED.to_string()),
        }
    }
}
