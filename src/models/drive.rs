use serde::{Deserialize, Serialize};

use super::RateLimiter;

pub const DRIVE_CACHE_TYPE_WRITEBACK: &str = "Writeback";
pub const DRIVE_IO_ENGINE_ASYNC: &str = "Async";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drive {
    #[serde(rename = "drive_id", skip_serializing_if = "Option::is_none")]
    pub drive_id: Option<String>,
    #[serde(rename = "path_on_host", skip_serializing_if = "Option::is_none")]
    pub path_on_host: Option<String>,
    #[serde(rename = "is_root_device", skip_serializing_if = "Option::is_none")]
    pub is_root_device: Option<bool>,
    #[serde(rename = "is_read_only", skip_serializing_if = "Option::is_none")]
    pub is_read_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partuuid: Option<String>,
    #[serde(rename = "rate_limiter", skip_serializing_if = "Option::is_none")]
    pub rate_limiter: Option<RateLimiter>,
    #[serde(rename = "cache_type", skip_serializing_if = "Option::is_none")]
    pub cache_type: Option<String>,
    #[serde(rename = "io_engine", skip_serializing_if = "Option::is_none")]
    pub io_engine: Option<String>,
}
