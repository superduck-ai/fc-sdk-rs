use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalloonUpdate {
    #[serde(rename = "amount_mib", skip_serializing_if = "Option::is_none")]
    pub amount_mib: Option<i64>,
}
