use std::sync::{LazyLock, Mutex};

use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::{
    CommandStdio, Config, Error, FIRECRACKER_INIT_TIMEOUT_ENV, HandlerList, Machine,
    MachineConfiguration, VMCommandBuilder, new_machine, with_client, with_process_runner,
};

static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

struct EnvGuard {
    key: &'static str,
    previous_value: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous_value = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self {
            key,
            previous_value,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous_value) = self.previous_value.as_deref() {
                std::env::set_var(self.key, previous_value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

#[test]
fn test_start_returns_already_started_on_second_call() {
    let mut machine = Machine::new_with_client(
        Config {
            disable_validation: true,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        Box::new(MockClient::default()),
    )
    .unwrap();
    machine.handlers.validation = HandlerList::default();
    machine.handlers.fc_init = HandlerList::default();

    machine.start().unwrap();
    assert!(matches!(machine.start(), Err(Error::AlreadyStarted)));
}

#[test]
fn test_start_with_invalid_binary_sets_wait_error() {
    let mut machine = new_machine(
        Config {
            disable_validation: true,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(
                VMCommandBuilder::default()
                    .with_bin("/definitely/missing/firecracker")
                    .with_stdin(CommandStdio::Null)
                    .with_stdout(CommandStdio::Null)
                    .with_stderr(CommandStdio::Null)
                    .build(),
            ),
        ],
    )
    .unwrap();

    let start_error = machine.start().unwrap_err();
    assert!(matches!(&start_error, Error::Process(_)));

    let wait_error = machine.wait().unwrap_err();
    assert_eq!(start_error.to_string(), wait_error.to_string());
    assert!(machine.pid().is_err());
}

#[test]
fn test_start_without_reachable_vmm_sets_wait_error_and_reaps_process() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let _env_guard = EnvGuard::set(FIRECRACKER_INIT_TIMEOUT_ENV, "1");

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient {
                get_machine_configuration_fn: Some(Box::new(|| {
                    Err(Error::Process("socket not ready".to_string()))
                })),
                ..MockClient::default()
            })),
            with_process_runner(
                VMCommandBuilder::default()
                    .with_bin("/bin/sh")
                    .with_args(["-c", "sleep 60"])
                    .with_stdin(CommandStdio::Null)
                    .with_stdout(CommandStdio::Null)
                    .with_stderr(CommandStdio::Null)
                    .build(),
            ),
        ],
    )
    .unwrap();

    let start_error = machine.start().unwrap_err();
    assert!(
        start_error
            .to_string()
            .contains("timed out while waiting for the Firecracker VMM to become reachable")
    );

    let wait_error = machine.wait().unwrap_err();
    assert_eq!(start_error.to_string(), wait_error.to_string());
    assert!(machine.pid().is_err());
}
