#![allow(non_snake_case)]

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::thread;
use std::time::Duration;

use firecracker_sdk::{
    AckError, ConnectMessageError, dial_with_options, is_temporary_net_error, with_ack_msg_timeout,
    with_retry_interval, with_retry_timeout,
};

#[test]
fn TestTemporaryNetErr() {
    let ack_error = std::io::Error::other(AckError::new(std::io::Error::other("ack")));
    assert!(is_temporary_net_error(&ack_error));

    let connect_error =
        std::io::Error::other(ConnectMessageError::new(std::io::Error::other("connect")));
    assert!(!is_temporary_net_error(&connect_error));

    let random_error = std::io::Error::other("something else");
    assert!(!is_temporary_net_error(&random_error));
}

#[test]
fn test_dial_sends_connect_message_and_returns_connected_stream() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("vsock.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();

        let mut request = String::new();
        loop {
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0] as char);
            if byte[0] == b'\n' {
                break;
            }
        }

        assert_eq!("CONNECT 52\n", request);
        stream.write_all(b"OK 1734\n").unwrap();

        let mut payload = [0u8; 5];
        stream.read_exact(&mut payload).unwrap();
        assert_eq!(b"hello", &payload);
    });

    let mut stream = dial_with_options(
        &socket_path,
        52,
        [
            with_retry_timeout(Duration::from_millis(100)),
            with_retry_interval(Duration::from_millis(10)),
            with_ack_msg_timeout(Duration::from_millis(100)),
        ],
    )
    .unwrap();

    stream.write_all(b"hello").unwrap();
    handle.join().unwrap();
}

#[test]
fn test_dial_retries_after_temporary_ack_failure() {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("retry.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();

    let handle = thread::spawn(move || {
        for attempt in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();

            let mut request = String::new();
            loop {
                let mut byte = [0u8; 1];
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0] as char);
                if byte[0] == b'\n' {
                    break;
                }
            }

            assert_eq!("CONNECT 7000\n", request);
            if attempt == 0 {
                stream.write_all(b"ERR\n").unwrap();
            } else {
                stream.write_all(b"OK 2048\n").unwrap();
            }
        }
    });

    let stream = dial_with_options(
        &socket_path,
        7000,
        [
            with_retry_timeout(Duration::from_millis(200)),
            with_retry_interval(Duration::from_millis(10)),
            with_ack_msg_timeout(Duration::from_millis(50)),
        ],
    );

    assert!(stream.is_ok());
    handle.join().unwrap();
}
