use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalloonStats {
    #[serde(flatten)]
    pub raw: BTreeMap<String, serde_json::Value>,
}
