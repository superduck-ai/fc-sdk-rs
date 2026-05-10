#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::{
    AsyncResultExt, CommandStdio, Config, MachineConfiguration, VMCommandBuilder, new_machine,
    with_client, with_process_runner,
};

const SIGUSR1: i32 = 10;
const SIGUSR2: i32 = 12;
const SIGWINCH: i32 = 28;

unsafe extern "C" {
    #[link_name = "getpid"]
    fn libc_getpid() -> i32;
    #[link_name = "kill"]
    fn libc_kill(pid: i32, signal: i32) -> i32;
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for {:?}",
            path
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_signal_count(path: &Path, expected: usize, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let count = fs::read_to_string(path)
            .ok()
            .map(|contents| contents.lines().filter(|line| !line.is_empty()).count())
            .unwrap_or(0);

        if count >= expected {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected} signals in {:?}",
            path
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn test_start_vmm_forwards_only_configured_signals() {
    let temp_dir = tempfile::tempdir().unwrap();
    let output_path = temp_dir.path().join("signals.out");
    let ready_path = temp_dir.path().join("ready");
    let socket_path = temp_dir.path().join("machine.sock");
    let script_path = temp_dir.path().join("signal-recorder.sh");

    fs::write(
        &script_path,
        format!(
            r#"#!/bin/sh
output_file="$1"
ready_file="$2"
socket_path="$3"

: > "$output_file"
trap 'printf "%s\n" {sigusr1} >> "$output_file"' USR1
trap 'printf "%s\n" {sigusr2} >> "$output_file"' USR2
trap 'printf "%s\n" {sigwinch} >> "$output_file"' WINCH
: > "$socket_path"
: > "$ready_file"

while :
do
    sleep 1
done
"#,
            sigusr1 = SIGUSR1,
            sigusr2 = SIGUSR2,
            sigwinch = SIGWINCH,
        ),
    )
    .unwrap();

    let mut permissions = fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).unwrap();

    let command = VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args([
            script_path.display().to_string(),
            output_path.display().to_string(),
            ready_path.display().to_string(),
            socket_path.display().to_string(),
        ])
        .with_stdin(CommandStdio::Null)
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .build();

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            socket_path: socket_path.display().to_string(),
            machine_cfg: MachineConfiguration::new(1, 128),
            forward_signals: Some(vec![SIGUSR1, SIGUSR2]),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(command),
        ],
    )
    .unwrap();

    machine.start_vmm().unwrap();
    wait_for_path(&ready_path, Duration::from_secs(2));

    let pid = unsafe { libc_getpid() };
    assert_eq!(0, unsafe { libc_kill(pid, SIGUSR1) });
    assert_eq!(0, unsafe { libc_kill(pid, SIGWINCH) });
    assert_eq!(0, unsafe { libc_kill(pid, SIGUSR2) });

    wait_for_signal_count(&output_path, 2, Duration::from_secs(2));

    machine.stop_vmm().unwrap();
    assert!(machine.wait().is_err());

    let mut received_signals = fs::read_to_string(&output_path)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.parse::<i32>().unwrap())
        .collect::<Vec<_>>();
    received_signals.sort_unstable();

    assert_eq!(vec![SIGUSR1, SIGUSR2], received_signals);
}

#[test]
fn test_start_vmm_forwards_signals_to_multiple_running_machines() {
    let temp_dir = tempfile::tempdir().unwrap();
    let script_path = temp_dir.path().join("signal-recorder.sh");

    fs::write(
        &script_path,
        format!(
            r#"#!/bin/sh
output_file="$1"
ready_file="$2"
socket_path="$3"

: > "$output_file"
trap 'printf "%s\n" {sigusr1} >> "$output_file"' USR1
: > "$socket_path"
: > "$ready_file"

while :
do
    sleep 1
done
"#,
            sigusr1 = SIGUSR1,
        ),
    )
    .unwrap();

    let mut permissions = fs::metadata(&script_path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).unwrap();

    let build_machine = |name: &str| {
        let output_path = temp_dir.path().join(format!("{name}.out"));
        let ready_path = temp_dir.path().join(format!("{name}.ready"));
        let socket_path = temp_dir.path().join(format!("{name}.sock"));

        let command = VMCommandBuilder::default()
            .with_bin("/bin/sh")
            .with_args([
                script_path.display().to_string(),
                output_path.display().to_string(),
                ready_path.display().to_string(),
                socket_path.display().to_string(),
            ])
            .with_stdin(CommandStdio::Null)
            .with_stdout(CommandStdio::Null)
            .with_stderr(CommandStdio::Null)
            .build();

        let machine = new_machine(
            Config {
                disable_validation: true,
                socket_path: socket_path.display().to_string(),
                machine_cfg: MachineConfiguration::new(1, 128),
                forward_signals: Some(vec![SIGUSR1]),
                ..Config::default()
            },
            [
                with_client(Box::new(MockClient::default())),
                with_process_runner(command),
            ],
        )
        .unwrap();

        (machine, output_path, ready_path)
    };

    let (mut machine_a, output_path_a, ready_path_a) = build_machine("machine-a");
    let (mut machine_b, output_path_b, ready_path_b) = build_machine("machine-b");

    machine_a.start_vmm().unwrap();
    machine_b.start_vmm().unwrap();
    wait_for_path(&ready_path_a, Duration::from_secs(2));
    wait_for_path(&ready_path_b, Duration::from_secs(2));

    let pid = unsafe { libc_getpid() };
    assert_eq!(0, unsafe { libc_kill(pid, SIGUSR1) });

    wait_for_signal_count(&output_path_a, 1, Duration::from_secs(2));
    wait_for_signal_count(&output_path_b, 1, Duration::from_secs(2));

    machine_a.stop_vmm().unwrap();
    machine_b.stop_vmm().unwrap();
    assert!(machine_a.wait().is_err());
    assert!(machine_b.wait().is_err());

    let received_signals_a = fs::read_to_string(&output_path_a)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.parse::<i32>().unwrap())
        .collect::<Vec<_>>();
    let received_signals_b = fs::read_to_string(&output_path_b)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.parse::<i32>().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(vec![SIGUSR1], received_signals_a);
    assert_eq!(vec![SIGUSR1], received_signals_b);
}
