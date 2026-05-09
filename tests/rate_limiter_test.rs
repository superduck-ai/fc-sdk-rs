#![allow(non_snake_case)]

use std::time::Duration;

use firecracker_sdk::{TokenBucket, TokenBucketBuilder};
use pretty_assertions::assert_eq;

#[test]
fn TestRateLimiter() {
    let bucket = TokenBucketBuilder::default()
        .with_refill_duration(Duration::from_secs(60 * 60))
        .with_bucket_size(100)
        .with_initial_size(100)
        .build();

    let expected = TokenBucket {
        one_time_burst: Some(100),
        refill_time: Some(3_600_000),
        size: Some(100),
    };

    assert_eq!(expected, bucket);
}

#[test]
fn TestRateLimiter_RefillTime() {
    let cases = [
        ("one hour", Duration::from_secs(60 * 60), 3_600_000),
        ("zero", Duration::from_secs(0), 0),
    ];

    for (_, duration, expected_millis) in cases {
        let bucket = TokenBucketBuilder::default()
            .with_refill_duration(duration)
            .build();

        assert_eq!(Some(expected_millis), bucket.refill_time);
    }
}
