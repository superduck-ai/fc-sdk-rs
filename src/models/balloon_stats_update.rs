use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalloonStatsUpdate {
    #[serde(
        rename = "stats_polling_intervals",
        skip_serializing_if = "Option::is_none"
    )]
    pub stats_polling_intervals: Option<i64>,
}
