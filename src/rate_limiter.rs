use std::time::Duration;

use crate::models::{RateLimiter, TokenBucket};

pub type RateLimiterOpt = Box<dyn Fn(&mut RateLimiter) + Send + Sync + 'static>;

pub fn new_rate_limiter(
    bandwidth: TokenBucket,
    ops: TokenBucket,
    opts: impl IntoIterator<Item = RateLimiterOpt>,
) -> RateLimiter {
    let mut limiter = RateLimiter {
        bandwidth: Some(bandwidth),
        ops: Some(ops),
    };

    for opt in opts {
        opt(&mut limiter);
    }

    limiter
}

#[derive(Debug, Clone, Default)]
pub struct TokenBucketBuilder {
    bucket: TokenBucket,
}

impl TokenBucketBuilder {
    pub fn with_bucket_size(mut self, size: i64) -> Self {
        self.bucket.size = Some(size);
        self
    }

    pub fn with_refill_duration(mut self, duration: Duration) -> Self {
        self.bucket.refill_time = Some(duration.as_millis() as i64);
        self
    }

    pub fn with_initial_size(mut self, size: i64) -> Self {
        self.bucket.one_time_burst = Some(size);
        self
    }

    pub fn build(self) -> TokenBucket {
        self.bucket
    }
}
