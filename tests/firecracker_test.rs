#![allow(non_snake_case)]

use std::fs::File;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use firecracker_sdk::{Client, Drive, FirecrackerVersion, wait_for_alive_vmm};
use serde_json::json;

fn firecracker_binary() -> &'static str {
    "/data/firecracker"
}

fn make_socket_path(test_name: &str) -> (PathBuf, tempfile::TempDir) {
    let dir = tempfile::Builder::new()
        .prefix(&test_name.replace('/', "_"))
        .tempdir()
        .unwrap();
    (dir.path().join("firecracker.sock"), dir)
}

#[test]
fn TestClient() {
    let (socket_path, _dir) = make_socket_path("TestClient");
    let drive_path = socket_path.with_extension("img");
    File::create(&drive_path).unwrap();

    let mut child = Command::new(firecracker_binary())
        .args(["--api-sock", socket_path.to_str().unwrap()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut client = Client::new(socket_path.display().to_string());
    wait_for_alive_vmm(&mut client, Duration::from_secs(2)).unwrap();

    let drive = Drive {
        drive_id: Some("test".to_string()),
        is_read_only: Some(false),
        is_root_device: Some(false),
        path_on_host: Some(drive_path.display().to_string()),
        ..Drive::default()
    };

    client.put_guest_drive_by_id("test", &drive).unwrap();

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn TestGetFirecrackerVersion() {
    let (socket_path, _dir) = make_socket_path("TestGetFirecrackerVersion");

    let mut child = Command::new(firecracker_binary())
        .args(["--api-sock", socket_path.to_str().unwrap()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut client = Client::new(socket_path.display().to_string());
    wait_for_alive_vmm(&mut client, Duration::from_secs(2)).unwrap();

    let version: FirecrackerVersion = client.get_firecracker_version().unwrap();
    assert!(!version.firecracker_version.is_empty());

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn test_client_put_patch_and_get_mmds() {
    let (socket_path, _dir) = make_socket_path("test_client_put_patch_and_get_mmds");

    let mut child = Command::new(firecracker_binary())
        .args(["--api-sock", socket_path.to_str().unwrap()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut client = Client::new(socket_path.display().to_string());
    wait_for_alive_vmm(&mut client, Duration::from_secs(2)).unwrap();

    client.put_mmds(&json!({ "hello": "world" })).unwrap();
    assert_eq!(json!({ "hello": "world" }), client.get_mmds().unwrap());

    client
        .patch_mmds(&json!({ "updated": true, "nested": { "value": 123 } }))
        .unwrap();
    assert_eq!(
        json!({
            "hello": "world",
            "updated": true,
            "nested": { "value": 123 }
        }),
        client.get_mmds().unwrap()
    );

    let _ = child.kill();
    let _ = child.wait();
}
