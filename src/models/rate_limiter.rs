use serde::{Deserialize, Serialize};

use super::TokenBucket;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimiter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bandwidth: Option<TokenBucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ops: Option<TokenBucket>,
}
