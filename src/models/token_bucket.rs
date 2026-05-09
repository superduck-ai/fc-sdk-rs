use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenBucket {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(rename = "one_time_burst", skip_serializing_if = "Option::is_none")]
    pub one_time_burst: Option<i64>,
    #[serde(rename = "refill_time", skip_serializing_if = "Option::is_none")]
    pub refill_time: Option<i64>,
}
