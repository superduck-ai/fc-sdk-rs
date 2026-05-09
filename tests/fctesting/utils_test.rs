#![allow(non_snake_case)]

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use firecracker_sdk::fctesting::{LOG_LEVEL_ENV_NAME, TestWriter, new_log_entry, parse_log_level};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn test_test_writer() {
    let mut buf = Vec::new();
    let mut writer = TestWriter {
        write_fn: Box::new(move |bytes| {
            buf.extend_from_slice(bytes);
            Ok(bytes.len())
        }),
    };

    writer.write_all(b"hello world").unwrap();
}

#[test]
fn test_new_log_entry_does_not_panic_for_valid_level() {
    let _dispatch = new_log_entry();
    assert_eq!(Some(tracing::Level::DEBUG), parse_log_level("debug"));
}

#[test]
fn TestLoggingPanic() {
    let _guard = env_lock().lock().unwrap();
    unsafe {
        std::env::set_var(LOG_LEVEL_ENV_NAME, "debug");
    }
    let _dispatch = new_log_entry();
    unsafe {
        std::env::remove_var(LOG_LEVEL_ENV_NAME);
    }
}
