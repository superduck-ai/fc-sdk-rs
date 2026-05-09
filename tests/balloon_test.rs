#![allow(non_snake_case)]

use firecracker_sdk::{Balloon, BalloonDevice, with_stats_polling_intervals};
use pretty_assertions::assert_eq;

fn expected_balloon() -> Balloon {
    Balloon {
        amount_mib: Some(6),
        deflate_on_oom: Some(true),
        stats_polling_intervals: 1,
    }
}

#[test]
fn TestNewBalloonDevice() {
    let balloon = BalloonDevice::new(6, true, vec![with_stats_polling_intervals(1)]).build();
    assert_eq!(expected_balloon(), balloon);
}

#[test]
fn TestUpdateAmountMiB() {
    let balloon = BalloonDevice::new(1, true, vec![with_stats_polling_intervals(1)])
        .update_amount_mib(6)
        .build();

    assert_eq!(expected_balloon(), balloon);
}

#[test]
fn TestUpdateStatsPollingIntervals() {
    let balloon = BalloonDevice::new(6, true, Vec::new())
        .update_stats_polling_intervals(1)
        .build();

    assert_eq!(expected_balloon(), balloon);
}
