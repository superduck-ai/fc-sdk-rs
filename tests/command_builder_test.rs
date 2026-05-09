#![allow(non_snake_case)]

use firecracker_sdk::{CommandStdio, VMCommandBuilder};
use pretty_assertions::assert_eq;

#[test]
fn test_vm_command_builder_immutable() {
    let builder = VMCommandBuilder::default();
    let _ = builder
        .clone()
        .with_socket_path("foo")
        .with_args(["baz", "qux"])
        .add_args(["moo", "cow"]);

    assert_eq!(None, builder.socket_path_args());
    assert_eq!(None, builder.args());
}

#[test]
fn test_vm_command_builder_chaining() {
    let builder = VMCommandBuilder::default()
        .with_socket_path("socket-path")
        .with_bin("bin");

    assert_eq!(
        Some(vec!["--api-sock".to_string(), "socket-path".to_string()]),
        builder.socket_path_args()
    );
    assert_eq!("bin", builder.bin());
}

#[test]
fn TestVMCommandBuilder() {
    let command = VMCommandBuilder::default()
        .with_socket_path("socket-path")
        .with_bin("bin")
        .with_stdout(CommandStdio::Null)
        .with_stderr(CommandStdio::Null)
        .with_stdin(CommandStdio::Null)
        .with_args(["foo"])
        .add_args(["--bar", "baz"])
        .build();

    let expected_args = vec![
        "--api-sock".to_string(),
        "socket-path".to_string(),
        "foo".to_string(),
        "--bar".to_string(),
        "baz".to_string(),
    ];

    assert_eq!("bin", command.bin);
    assert_eq!(expected_args, command.args);
    assert_eq!(CommandStdio::Null, command.stdout);
    assert_eq!(CommandStdio::Null, command.stderr);
    assert_eq!(CommandStdio::Null, command.stdin);
}

#[test]
fn test_vm_command_builder_build_with_stdio_paths() {
    let command = VMCommandBuilder::default()
        .with_stdout_path("/tmp/stdout.log")
        .with_stderr_path("/tmp/stderr.log")
        .with_stdin_path("/tmp/stdin.log")
        .build();

    assert_eq!(
        CommandStdio::Path("/tmp/stdout.log".to_string()),
        command.stdout
    );
    assert_eq!(
        CommandStdio::Path("/tmp/stderr.log".to_string()),
        command.stderr
    );
    assert_eq!(
        CommandStdio::Path("/tmp/stdin.log".to_string()),
        command.stdin
    );
}
