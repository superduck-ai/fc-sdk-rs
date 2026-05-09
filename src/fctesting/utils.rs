pub const ROOT_DISABLE_ENV_NAME: &str = "DISABLE_ROOT_TESTS";
pub const LOG_LEVEL_ENV_NAME: &str = "FC_TEST_LOG_LEVEL";

pub fn root_tests_disabled() -> bool {
    std::env::var(ROOT_DISABLE_ENV_NAME)
        .map(|value| !value.is_empty())
        .unwrap_or(false)
}

pub fn kvm_is_writable() -> bool {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/kvm")
        .is_ok()
}

pub fn require_root() -> std::result::Result<(), String> {
    if root_tests_disabled() {
        return Err("skipping test that requires root".to_string());
    }

    if std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|uid| uid.trim() == "0")
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(format!(
            "This test must be run as root. To disable tests that require root, run the tests with the {} environment variable set.",
            ROOT_DISABLE_ENV_NAME
        ))
    }
}

pub fn parse_log_level(level: &str) -> Option<tracing::Level> {
    match level.to_ascii_lowercase().as_str() {
        "error" => Some(tracing::Level::ERROR),
        "warn" | "warning" => Some(tracing::Level::WARN),
        "info" => Some(tracing::Level::INFO),
        "debug" => Some(tracing::Level::DEBUG),
        "trace" => Some(tracing::Level::TRACE),
        _ => None,
    }
}

pub fn new_log_entry() -> tracing::Dispatch {
    if let Ok(level) = std::env::var(LOG_LEVEL_ENV_NAME) {
        assert!(
            parse_log_level(&level).is_some(),
            "Failed to parse {:?} as log level",
            level
        );
    }

    tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default())
}

#[cfg(test)]
mod tests {
    use super::parse_log_level;

    #[test]
    fn test_parse_log_level() {
        assert_eq!(Some(tracing::Level::DEBUG), parse_log_level("debug"));
        assert_eq!(Some(tracing::Level::WARN), parse_log_level("warning"));
        assert_eq!(None, parse_log_level("wat"));
    }
}
