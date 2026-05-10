use std::io;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct UnixSocketTransport {
    socket_path: String,
    request_timeout: Duration,
}

impl UnixSocketTransport {
    pub fn new(socket_path: impl Into<String>, request_timeout: Duration) -> Self {
        Self {
            socket_path: socket_path.into(),
            request_timeout,
        }
    }

    pub fn socket_path(&self) -> &str {
        &self.socket_path
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub async fn raw_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        self.raw_request_with_timeouts(
            method,
            path,
            body,
            Some(self.request_timeout),
            Some(self.request_timeout),
        )
        .await
    }

    pub async fn raw_request_with_timeouts(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
    ) -> Result<Vec<u8>> {
        let mut stream = UnixStream::connect(&self.socket_path).await?;

        let body = body.unwrap_or_default();
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nUser-Agent: firecracker-rs-sdk\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        if !body.is_empty() {
            request.push_str("Content-Type: application/json\r\n");
        }
        request.push_str("\r\n");

        write_all_with_timeout(&mut stream, request.as_bytes(), write_timeout).await?;
        if !body.is_empty() {
            write_all_with_timeout(&mut stream, body, write_timeout).await?;
        }

        let mut response = Vec::new();
        let delimiter = b"\r\n\r\n";
        let header_end = loop {
            let mut chunk = [0u8; 1024];
            let read = read_with_timeout(&mut stream, &mut chunk, read_timeout).await?;
            if read == 0 {
                return Err(Error::Api {
                    status: 0,
                    body: "unexpected EOF while reading HTTP response headers".into(),
                });
            }
            response.extend_from_slice(&chunk[..read]);
            if let Some(position) = response
                .windows(delimiter.len())
                .position(|window| window == delimiter)
            {
                break position;
            }
        };

        let (head, body_with_delimiter) = response.split_at(header_end);
        let mut body = body_with_delimiter[delimiter.len()..].to_vec();
        let status = parse_http_status(head)?;
        let head_text = String::from_utf8_lossy(head);
        let content_length = head_text.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        });

        if let Some(content_length) = content_length {
            while body.len() < content_length {
                let mut chunk = vec![0u8; content_length - body.len()];
                let read = read_with_timeout(&mut stream, &mut chunk, read_timeout).await?;
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read]);
            }

            return parse_http_response(head, &body[..content_length.min(body.len())]);
        }

        if response_must_not_have_body(method, status) {
            return parse_http_response(head, &[]);
        }

        loop {
            let mut chunk = [0u8; 1024];
            let read = read_with_timeout(&mut stream, &mut chunk, read_timeout).await?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read]);
        }

        parse_http_response(head, &body)
    }

    pub async fn raw_json_request<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
    ) -> Result<Vec<u8>> {
        self.raw_json_request_with_timeouts(
            method,
            path,
            body,
            Some(self.request_timeout),
            Some(self.request_timeout),
        )
        .await
    }

    pub async fn raw_json_request_with_timeouts<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
    ) -> Result<Vec<u8>> {
        let encoded = serde_json::to_vec(body)?;
        self.raw_request_with_timeouts(method, path, Some(&encoded), read_timeout, write_timeout)
            .await
    }
}

pub fn new_unix_socket_transport(
    socket_path: impl Into<String>,
    request_timeout: Duration,
) -> UnixSocketTransport {
    UnixSocketTransport::new(socket_path, request_timeout)
}

fn parse_http_response(head: &[u8], body: &[u8]) -> Result<Vec<u8>> {
    let status = parse_http_status(head)?;

    if !(200..300).contains(&status) {
        return Err(Error::Api {
            status,
            body: String::from_utf8_lossy(body).to_string(),
        });
    }

    Ok(body.to_vec())
}

fn parse_http_status(head: &[u8]) -> Result<u16> {
    let head = String::from_utf8_lossy(head);
    let mut lines = head.lines();
    let status_line = lines.next().ok_or_else(|| Error::Api {
        status: 0,
        body: "invalid HTTP response: missing status line".into(),
    })?;

    status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| Error::Api {
            status: 0,
            body: "invalid HTTP response: malformed status line".into(),
        })?
        .parse::<u16>()
        .map_err(|_| Error::Api {
            status: 0,
            body: "invalid HTTP response: malformed status code".into(),
        })
}

fn response_must_not_have_body(method: &str, status: u16) -> bool {
    method.eq_ignore_ascii_case("HEAD")
        || status == 204
        || status == 304
        || (100..200).contains(&status)
}

async fn read_with_timeout(
    stream: &mut UnixStream,
    buffer: &mut [u8],
    timeout: Option<Duration>,
) -> io::Result<usize> {
    run_with_timeout(timeout, "read", async { stream.read(buffer).await }).await
}

async fn write_all_with_timeout(
    stream: &mut UnixStream,
    buffer: &[u8],
    timeout: Option<Duration>,
) -> io::Result<()> {
    run_with_timeout(timeout, "write", async { stream.write_all(buffer).await }).await
}

async fn run_with_timeout<F, T>(
    timeout: Option<Duration>,
    operation: &str,
    future: F,
) -> io::Result<T>
where
    F: std::future::Future<Output = io::Result<T>>,
{
    match timeout {
        Some(duration) => time::timeout(duration, future).await.map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{operation} timed out after {:?}", duration),
            )
        })?,
        None => future.await,
    }
}
