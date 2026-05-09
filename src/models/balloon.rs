use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balloon {
    #[serde(rename = "amount_mib", skip_serializing_if = "Option::is_none")]
    pub amount_mib: Option<i64>,
    #[serde(rename = "deflate_on_oom", skip_serializing_if = "Option::is_none")]
    pub deflate_on_oom: Option<bool>,
    #[serde(rename = "stats_polling_intervals", default)]
    pub stats_polling_intervals: i64,
}
