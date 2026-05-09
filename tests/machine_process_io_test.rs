use std::fs;

use firecracker_sdk::fctesting::MockClient;
use firecracker_sdk::{
    CommandStdio, Config, MachineConfiguration, VMCommandBuilder, new_machine, with_client,
    with_process_runner,
};

#[test]
fn test_start_vmm_redirects_process_stdout_and_stderr_to_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("machine.sock");
    let stdout_path = temp_dir.path().join("stdout.log");
    let stderr_path = temp_dir.path().join("stderr.log");

    let socket_path_str = socket_path.display().to_string();
    let stdout_path_str = stdout_path.display().to_string();
    let stderr_path_str = stderr_path.display().to_string();

    let command = VMCommandBuilder::default()
        .with_bin("/bin/sh")
        .with_args([
            "-c",
            "touch \"$1\"; echo stdout-line; echo stderr-line 1>&2; sleep 1",
            "sh",
            socket_path_str.as_str(),
        ])
        .with_stdin(CommandStdio::Null)
        .with_stdout_path(&stdout_path_str)
        .with_stderr_path(&stderr_path_str)
        .build();

    let mut machine = new_machine(
        Config {
            disable_validation: true,
            socket_path: socket_path_str,
            machine_cfg: MachineConfiguration::new(1, 128),
            ..Config::default()
        },
        [
            with_client(Box::new(MockClient::default())),
            with_process_runner(command),
        ],
    )
    .unwrap();

    machine.start_vmm().unwrap();
    machine.wait().unwrap();

    assert_eq!("stdout-line\n", fs::read_to_string(stdout_path).unwrap());
    assert_eq!("stderr-line\n", fs::read_to_string(stderr_path).unwrap());
}
