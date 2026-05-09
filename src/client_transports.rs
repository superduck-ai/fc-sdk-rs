use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use serde::Serialize;

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

    pub fn raw_request(&self, method: &str, path: &str, body: Option<&[u8]>) -> Result<Vec<u8>> {
        self.raw_request_with_timeouts(
            method,
            path,
            body,
            Some(self.request_timeout),
            Some(self.request_timeout),
        )
    }

    pub fn raw_request_with_timeouts(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
    ) -> Result<Vec<u8>> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(read_timeout)?;
        stream.set_write_timeout(write_timeout)?;
        let read_timeout_configured = read_timeout.is_some();

        let body = body.unwrap_or_default();
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nUser-Agent: firecracker-rs-sdk\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        if !body.is_empty() {
            request.push_str("Content-Type: application/json\r\n");
        }
        request.push_str("\r\n");

        stream.write_all(request.as_bytes())?;
        if !body.is_empty() {
            stream.write_all(body)?;
        }

        let mut response = Vec::new();
        let delimiter = b"\r\n\r\n";
        let header_end = loop {
            let mut chunk = [0u8; 1024];
            let read = stream
                .read(&mut chunk)
                .map_err(|error| normalize_timeout_error(error, read_timeout_configured))?;
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
                let read = stream
                    .read(&mut chunk)
                    .map_err(|error| normalize_timeout_error(error, read_timeout_configured))?;
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
            let read = stream
                .read(&mut chunk)
                .map_err(|error| normalize_timeout_error(error, read_timeout_configured))?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read]);
        }

        parse_http_response(head, &body)
    }

    pub fn raw_json_request<T: Serialize>(
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
    }

    pub fn raw_json_request_with_timeouts<T: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: &T,
        read_timeout: Option<Duration>,
        write_timeout: Option<Duration>,
    ) -> Result<Vec<u8>> {
        let encoded = serde_json::to_vec(body)?;
        self.raw_request_with_timeouts(method, path, Some(&encoded), read_timeout, write_timeout)
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

fn normalize_timeout_error(error: std::io::Error, timeout_configured: bool) -> std::io::Error {
    if timeout_configured
        && matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        )
    {
        std::io::Error::new(std::io::ErrorKind::TimedOut, error.to_string())
    } else {
        error
    }
}
