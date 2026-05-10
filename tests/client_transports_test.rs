#![allow(non_snake_case)]

use std::future::Future;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::{LazyLock, Mutex};
use std::thread;

use std::time::Duration;

use firecracker_sdk::{
    AsyncResultExt, Balloon, BalloonStatsUpdate, BalloonUpdate, BootSource, Client, Drive,
    EntropyDevice, Error, FIRECRACKER_REQUEST_TIMEOUT_ENV, InstanceActionInfo, Logger,
    MachineConfiguration, MemoryBackend, Metrics, MmdsConfig, NetworkInterfaceModel,
    PartialNetworkInterface, RateLimiter, RequestOptions, SnapshotCreateParams, SnapshotLoadParams,
    TokenBucket, Vm, VsockModel, new_unix_socket_transport, with_init_timeout, with_read_timeout,
    with_request_timeout, with_unix_socket_transport, without_read_timeout,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

fn read_http_request(stream: &mut std::os::unix::net::UnixStream) -> std::io::Result<String> {
    let mut request = Vec::new();
    let delimiter = b"\r\n\r\n";
    let header_end = loop {
        let mut chunk = [0u8; 256];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading HTTP request headers",
            ));
        }

        request.extend_from_slice(&chunk[..read]);
        if let Some(position) = request
            .windows(delimiter.len())
            .position(|window| window == delimiter)
        {
            break position;
        }
    };

    let content_length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or_default();

    let body_start = header_end + delimiter.len();
    while request.len() < body_start + content_length {
        let mut chunk = vec![0u8; body_start + content_length - request.len()];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading HTTP request body",
            ));
        }
        request.extend_from_slice(&chunk[..read]);
    }

    Ok(String::from_utf8(request).unwrap())
}

fn split_http_request(request: &str) -> (&str, &str) {
    request.split_once("\r\n\r\n").unwrap()
}

async fn read_http_request_async(stream: &mut tokio::net::UnixStream) -> std::io::Result<String> {
    let mut request = Vec::new();
    let delimiter = b"\r\n\r\n";
    let header_end = loop {
        let mut chunk = [0u8; 256];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading HTTP request headers",
            ));
        }

        request.extend_from_slice(&chunk[..read]);
        if let Some(position) = request
            .windows(delimiter.len())
            .position(|window| window == delimiter)
        {
            break position;
        }
    };

    let content_length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or_default();

    let body_start = header_end + delimiter.len();
    while request.len() < body_start + content_length {
        let mut chunk = vec![0u8; body_start + content_length - request.len()];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading HTTP request body",
            ));
        }
        request.extend_from_slice(&chunk[..read]);
    }

    Ok(String::from_utf8(request).unwrap())
}

fn run_single_request<T, F, G>(response: Vec<u8>, assert_request: F, client_call: G) -> T
where
    F: FnOnce(String) + Send + 'static,
    G: FnOnce(&Client) -> T,
{
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-single-request.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        assert_request(request);
        stream.write_all(&response).unwrap();
    });

    let client = Client::new(socket_path.display().to_string());
    let result = client_call(&client);
    handle.join().unwrap();
    result
}

async fn run_single_request_async<T, F, G, Fut>(
    response: Vec<u8>,
    assert_request: F,
    client_call: G,
) -> T
where
    F: FnOnce(String) + Send + 'static,
    G: FnOnce(Client) -> Fut,
    Fut: Future<Output = T>,
{
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-single-request-async.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request_async(&mut stream).await.unwrap();
        assert_request(request);
        stream.write_all(&response).await.unwrap();
    });

    let client = Client::new(socket_path.display().to_string());
    let result = client_call(client).await;
    handle.await.unwrap();
    result
}

#[test]
fn test_client_raw_request_uses_unix_socket() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        assert!(request.starts_with("PUT /test-operation HTTP/1.1\r\n"));
        assert!(request.contains("Content-Type: application/json\r\n"));
        assert!(request.ends_with("\r\n\r\n{}"));

        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });

    let client = Client::new(socket_path.display().to_string());
    client
        .raw_json_request("PUT", "/test-operation", &serde_json::json!({}))
        .unwrap();

    handle.join().unwrap();
}

#[test]
fn test_client_new_with_opts_overrides_transport_and_timeouts() {
    let client = Client::new_with_opts(
        "/tmp/original.sock",
        [
            with_request_timeout(Duration::from_millis(250)),
            with_init_timeout(Duration::from_secs(9)),
            with_unix_socket_transport(new_unix_socket_transport(
                "/tmp/override.sock",
                Duration::from_secs(2),
            )),
        ],
    );

    assert_eq!("/tmp/override.sock", client.socket_path());
    assert_eq!(Duration::from_secs(2), client.request_timeout());
    assert_eq!(Duration::from_secs(9), client.init_timeout());
}

#[test]
fn test_client_patch_vm_with_options_overrides_read_timeout() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-read-timeout-override.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        let (head, body) = split_http_request(&request);
        assert!(head.starts_with("PATCH /vm HTTP/1.1\r\n"));
        assert_eq!(r#"{"state":"Paused"}"#, body);

        thread::sleep(Duration::from_millis(150));
        let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    });

    let client = Client::new_with_opts(
        socket_path.display().to_string(),
        [with_request_timeout(Duration::from_secs(1))],
    );
    let error = client
        .patch_vm_with_options(
            &Vm::paused(),
            RequestOptions::from_opts(vec![with_read_timeout(Duration::from_millis(50))]),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        Error::Io(ref error) if error.kind() == std::io::ErrorKind::TimedOut
    ));

    handle.join().unwrap();
}

#[test]
fn test_client_patch_vm_with_options_can_disable_read_timeout() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-read-timeout-disabled.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        let (head, body) = split_http_request(&request);
        assert!(head.starts_with("PATCH /vm HTTP/1.1\r\n"));
        assert_eq!(r#"{"state":"Paused"}"#, body);

        thread::sleep(Duration::from_millis(150));
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });

    let client = Client::new_with_opts(
        socket_path.display().to_string(),
        [with_request_timeout(Duration::from_millis(50))],
    );
    client
        .patch_vm_with_options(
            &Vm::paused(),
            RequestOptions::from_opts(vec![without_read_timeout()]),
        )
        .unwrap();

    handle.join().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_client_raw_request_uses_unix_socket_async_runtime() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-async.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request_async(&mut stream).await.unwrap();
        assert!(request.starts_with("PUT /test-operation HTTP/1.1\r\n"));
        assert!(request.contains("Content-Type: application/json\r\n"));
        assert!(request.ends_with("\r\n\r\n{}"));

        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
    });

    let client = Client::new(socket_path.display().to_string());
    client
        .raw_json_request("PUT", "/test-operation", &serde_json::json!({}))
        .await
        .unwrap();

    handle.await.unwrap();
}

#[test]
fn TestNewUnixSocketTransport() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-direct.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        assert!(request.starts_with("PUT /test-operation HTTP/1.1\r\n"));

        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });

    let transport = new_unix_socket_transport(
        socket_path.display().to_string(),
        Duration::from_millis(100),
    );
    transport
        .raw_json_request("PUT", "/test-operation", &serde_json::json!({}))
        .unwrap();

    handle.join().unwrap();
}

#[test]
fn test_transport_returns_timed_out_for_slow_response() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-timeout.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        assert!(request.starts_with("GET /slow HTTP/1.1\r\n"));

        thread::sleep(Duration::from_millis(250));
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    });

    let transport = new_unix_socket_transport(
        socket_path.display().to_string(),
        Duration::from_millis(100),
    );
    let error = transport.raw_request("GET", "/slow", None).unwrap_err();
    assert!(matches!(
        error,
        Error::Io(ref error) if error.kind() == std::io::ErrorKind::TimedOut
    ));

    handle.join().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn test_transport_returns_timed_out_for_slow_response_async_runtime() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-timeout-async.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request_async(&mut stream).await.unwrap();
        assert!(request.starts_with("GET /slow HTTP/1.1\r\n"));

        tokio::time::sleep(Duration::from_millis(250)).await;
        let _ = stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
            .await;
    });

    let transport = new_unix_socket_transport(
        socket_path.display().to_string(),
        Duration::from_millis(100),
    );
    let error = transport
        .raw_request("GET", "/slow", None)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        Error::Io(ref error) if error.kind() == std::io::ErrorKind::TimedOut
    ));

    handle.await.unwrap();
}

#[test]
fn test_transport_can_disable_read_timeout_for_long_requests() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-no-read-timeout.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        assert!(request.starts_with("PUT /snapshot/create HTTP/1.1\r\n"));

        thread::sleep(Duration::from_millis(250));
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
    });

    let transport = new_unix_socket_transport(
        socket_path.display().to_string(),
        Duration::from_millis(100),
    );
    transport
        .raw_json_request_with_timeouts(
            "PUT",
            "/snapshot/create",
            &serde_json::json!({}),
            None,
            Some(Duration::from_millis(100)),
        )
        .unwrap();

    handle.join().unwrap();
}

#[test]
fn test_transport_reads_response_body_without_content_length_until_eof() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-no-content-length.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        assert!(request.starts_with("GET /streaming HTTP/1.1\r\n"));

        stream
            .write_all(b"HTTP/1.1 200 OK\r\n\r\n{\"partial\":")
            .unwrap();
        thread::sleep(Duration::from_millis(20));
        stream.write_all(b"true}").unwrap();
    });

    let transport = new_unix_socket_transport(
        socket_path.display().to_string(),
        Duration::from_millis(100),
    );
    let body = transport.raw_request("GET", "/streaming", None).unwrap();
    assert_eq!(br#"{"partial":true}"#, body.as_slice());

    handle.join().unwrap();
}

#[test]
fn test_transport_does_not_wait_for_eof_on_no_content_response() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-no-content.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        assert!(request.starts_with("PUT /empty HTTP/1.1\r\n"));

        stream
            .write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
            .unwrap();
        thread::sleep(Duration::from_millis(250));
    });

    let transport = new_unix_socket_transport(
        socket_path.display().to_string(),
        Duration::from_millis(100),
    );
    let body = transport
        .raw_json_request("PUT", "/empty", &serde_json::json!({}))
        .unwrap();
    assert!(body.is_empty());

    handle.join().unwrap();
}

#[test]
fn test_client_put_guest_drive_by_id_uses_half_request_timeout() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let _env_guard = EnvGuard::set(FIRECRACKER_REQUEST_TIMEOUT_ENV, "200");

    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("transport-drive-timeout.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_http_request(&mut stream).unwrap();
        assert!(request.starts_with("PUT /drives/test HTTP/1.1\r\n"));

        thread::sleep(Duration::from_millis(150));
        let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    });

    let client = Client::new(socket_path.display().to_string());
    let error = client
        .put_guest_drive_by_id(
            "test",
            &Drive {
                drive_id: Some("test".to_string()),
                ..Drive::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        Error::Io(ref error) if error.kind() == std::io::ErrorKind::TimedOut
    ));

    handle.join().unwrap();
}

#[test]
fn test_client_direct_methods_cover_write_routes_and_payloads() {
    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /logger HTTP/1.1\r\n"));
            assert_eq!(
                json!({"log_path": "/tmp/firecracker.log"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_logger(&Logger {
                    log_path: Some("/tmp/firecracker.log".to_string()),
                    ..Logger::default()
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /metrics HTTP/1.1\r\n"));
            assert_eq!(
                json!({"metrics_path": "/tmp/firecracker.metrics"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_metrics(&Metrics {
                    metrics_path: Some("/tmp/firecracker.metrics".to_string()),
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /machine-config HTTP/1.1\r\n"));
            assert_eq!(
                json!({"vcpu_count": 2, "mem_size_mib": 512}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_machine_configuration(&MachineConfiguration::new(2, 512))
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PATCH /machine-config HTTP/1.1\r\n"));
            assert_eq!(
                json!({"track_dirty_pages": true}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .patch_machine_configuration(&MachineConfiguration {
                    track_dirty_pages: Some(true),
                    ..MachineConfiguration::default()
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /boot-source HTTP/1.1\r\n"));
            assert_eq!(
                json!({"kernel_image_path": "/kernel", "boot_args": "console=ttyS0"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_guest_boot_source(&BootSource {
                    kernel_image_path: Some("/kernel".to_string()),
                    boot_args: Some("console=ttyS0".to_string()),
                    ..BootSource::default()
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /cpu-config HTTP/1.1\r\n"));
            assert_eq!(
                json!({"cpuid_modifiers": {"leaf_1": {"eax": 1}}}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_cpu_configuration(&json!({"cpuid_modifiers": {"leaf_1": {"eax": 1}}}))
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /entropy HTTP/1.1\r\n"));
            assert_eq!(
                json!({"rate_limiter": {"bandwidth": {"size": 1024, "refill_time": 100}}}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_entropy_device(&EntropyDevice {
                    rate_limiter: Some(RateLimiter {
                        bandwidth: Some(TokenBucket {
                            size: Some(1024),
                            refill_time: Some(100),
                            ..TokenBucket::default()
                        }),
                        ..RateLimiter::default()
                    }),
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /network-interfaces/eth0 HTTP/1.1\r\n"));
            assert_eq!(
                json!({"iface_id": "eth0", "host_dev_name": "tap0"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_guest_network_interface_by_id(
                    "eth0",
                    &NetworkInterfaceModel {
                        iface_id: Some("eth0".to_string()),
                        host_dev_name: Some("tap0".to_string()),
                        ..NetworkInterfaceModel::default()
                    },
                )
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PATCH /network-interfaces/eth0 HTTP/1.1\r\n"));
            assert_eq!(
                json!({"iface_id": "eth0"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .patch_guest_network_interface_by_id(
                    "eth0",
                    &PartialNetworkInterface {
                        iface_id: Some("eth0".to_string()),
                        ..PartialNetworkInterface::default()
                    },
                )
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /vsock HTTP/1.1\r\n"));
            assert_eq!(
                json!({"vsock_id": "agent", "guest_cid": 3, "uds_path": "/tmp/v.sock"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_guest_vsock(&VsockModel {
                    vsock_id: Some("agent".to_string()),
                    guest_cid: Some(3),
                    uds_path: Some("/tmp/v.sock".to_string()),
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PATCH /vm HTTP/1.1\r\n"));
            assert_eq!(
                json!({"state": "Paused"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client.patch_vm(&Vm::paused()).unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /actions HTTP/1.1\r\n"));
            assert_eq!(
                json!({"action_type": "InstanceStart"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .create_sync_action(&InstanceActionInfo {
                    action_type: Some("InstanceStart".to_string()),
                })
                .unwrap();
        },
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_client_direct_methods_cover_async_await_callers() {
    run_single_request_async(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PATCH /vm HTTP/1.1\r\n"));
            assert_eq!(
                json!({"state": "Paused"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| async move {
            client.patch_vm(&Vm::paused()).await.unwrap();
        },
    )
    .await;
}

#[test]
fn test_client_direct_methods_cover_snapshot_mmds_and_balloon_routes() {
    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /snapshot/create HTTP/1.1\r\n"));
            assert_eq!(
                json!({"mem_file_path": "/tmp/vm.mem", "snapshot_path": "/tmp/vm.snap"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .create_snapshot(&SnapshotCreateParams {
                    mem_file_path: Some("/tmp/vm.mem".to_string()),
                    snapshot_path: Some("/tmp/vm.snap".to_string()),
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /snapshot/load HTTP/1.1\r\n"));
            assert_eq!(
                json!({
                    "mem_backend": {
                        "backend_type": "File",
                        "backend_path": "/tmp/vm.mem"
                    },
                    "snapshot_path": "/tmp/vm.snap",
                    "enable_diff_snapshots": true,
                    "resume_vm": true
                }),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .load_snapshot(&SnapshotLoadParams {
                    mem_backend: Some(MemoryBackend {
                        backend_type: Some("File".to_string()),
                        backend_path: Some("/tmp/vm.mem".to_string()),
                    }),
                    snapshot_path: Some("/tmp/vm.snap".to_string()),
                    enable_diff_snapshots: true,
                    resume_vm: true,
                    ..SnapshotLoadParams::default()
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /mmds/config HTTP/1.1\r\n"));
            assert_eq!(
                json!({"ipv4_address": "169.254.169.254", "network_interfaces": ["1"], "version": "V2"}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_mmds_config(&MmdsConfig {
                    ipv4_address: Some("169.254.169.254".to_string()),
                    network_interfaces: vec!["1".to_string()],
                    version: Some("V2".to_string()),
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PUT /balloon HTTP/1.1\r\n"));
            assert_eq!(
                json!({"amount_mib": 64, "deflate_on_oom": true, "stats_polling_intervals": 0}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .put_balloon(&Balloon {
                    amount_mib: Some(64),
                    deflate_on_oom: Some(true),
                    ..Balloon::default()
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PATCH /balloon HTTP/1.1\r\n"));
            assert_eq!(
                json!({"amount_mib": 96}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .patch_balloon(&BalloonUpdate {
                    amount_mib: Some(96),
                })
                .unwrap();
        },
    );

    run_single_request(
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_vec(),
        |request| {
            let (headers, body) = split_http_request(&request);
            assert!(headers.starts_with("PATCH /balloon/statistics HTTP/1.1\r\n"));
            assert_eq!(
                json!({"stats_polling_intervals": 5}),
                serde_json::from_str::<Value>(body).unwrap()
            );
        },
        |client| {
            client
                .patch_balloon_stats_interval(&BalloonStatsUpdate {
                    stats_polling_intervals: Some(5),
                })
                .unwrap();
        },
    );
}

#[test]
fn test_client_direct_read_methods_and_aliases_parse_responses() {
    let version = run_single_request(
        b"HTTP/1.1 200 OK\r\nContent-Length: 31\r\n\r\n{\"firecracker_version\":\"1.6.0\"}"
            .to_vec(),
        |request| {
            assert!(request.starts_with("GET /version HTTP/1.1\r\n"));
        },
        |client| client.get_firecracker_version().unwrap(),
    );
    assert_eq!("1.6.0", version.firecracker_version);

    let machine_config = run_single_request(
        b"HTTP/1.1 200 OK\r\nContent-Length: 36\r\n\r\n{\"vcpu_count\":2,\"mem_size_mib\":512}"
            .to_vec(),
        |request| {
            assert!(request.starts_with("GET /machine-config HTTP/1.1\r\n"));
        },
        |client| client.get_machine_configuration().unwrap(),
    );
    assert_eq!(Some(2), machine_config.vcpu_count);
    assert_eq!(Some(512), machine_config.mem_size_mib);

    let instance_info = run_single_request(
        b"HTTP/1.1 200 OK\r\nContent-Length: 58\r\n\r\n{\"app_name\":\"firecracker\",\"id\":\"vm1\",\"state\":\"Running\"}"
            .to_vec(),
        |request| {
            assert!(request.starts_with("GET / HTTP/1.1\r\n"));
        },
        |client| client.get_instance_info().unwrap(),
    );
    assert_eq!(Some("vm1".to_string()), instance_info.id);
    assert_eq!(Some("Running".to_string()), instance_info.state);

    let balloon = run_single_request(
        b"HTTP/1.1 200 OK\r\nContent-Length: 43\r\n\r\n{\"amount_mib\":64,\"deflate_on_oom\":true}"
            .to_vec(),
        |request| {
            assert!(request.starts_with("GET /balloon HTTP/1.1\r\n"));
        },
        |client| client.describe_balloon_config().unwrap(),
    );
    assert_eq!(Some(64), balloon.amount_mib);
    assert_eq!(Some(true), balloon.deflate_on_oom);

    let balloon_stats = run_single_request(
        b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\n{\"swap_in\":1234}".to_vec(),
        |request| {
            assert!(request.starts_with("GET /balloon/statistics HTTP/1.1\r\n"));
        },
        |client| client.describe_balloon_stats().unwrap(),
    );
    assert_eq!(Some(&json!(1234)), balloon_stats.raw.get("swap_in"));

    let exported = run_single_request(
        b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n{\"drives\":[]}".to_vec(),
        |request| {
            assert!(request.starts_with("GET /vm/config HTTP/1.1\r\n"));
        },
        |client| client.get_export_vm_config().unwrap(),
    );
    assert!(exported.drives.is_empty());
}
