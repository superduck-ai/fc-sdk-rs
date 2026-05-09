use std::error::Error as StdError;
use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialConfig {
    pub dial_timeout: Duration,
    pub retry_timeout: Duration,
    pub retry_interval: Duration,
    pub connect_msg_timeout: Duration,
    pub ack_msg_timeout: Duration,
}

impl Default for DialConfig {
    fn default() -> Self {
        Self {
            dial_timeout: Duration::from_millis(100),
            retry_timeout: Duration::from_secs(20),
            retry_interval: Duration::from_millis(100),
            connect_msg_timeout: Duration::from_millis(100),
            ack_msg_timeout: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialOption {
    DialTimeout(Duration),
    RetryTimeout(Duration),
    RetryInterval(Duration),
    ConnectionMsgTimeout(Duration),
    AckMsgTimeout(Duration),
}

impl DialOption {
    fn apply(self, config: &mut DialConfig) {
        match self {
            Self::DialTimeout(value) => config.dial_timeout = value,
            Self::RetryTimeout(value) => config.retry_timeout = value,
            Self::RetryInterval(value) => config.retry_interval = value,
            Self::ConnectionMsgTimeout(value) => config.connect_msg_timeout = value,
            Self::AckMsgTimeout(value) => config.ack_msg_timeout = value,
        }
    }
}

pub fn with_dial_timeout(timeout: Duration) -> DialOption {
    DialOption::DialTimeout(timeout)
}

pub fn with_retry_timeout(timeout: Duration) -> DialOption {
    DialOption::RetryTimeout(timeout)
}

pub fn with_retry_interval(interval: Duration) -> DialOption {
    DialOption::RetryInterval(interval)
}

pub fn with_connection_msg_timeout(timeout: Duration) -> DialOption {
    DialOption::ConnectionMsgTimeout(timeout)
}

pub fn with_ack_msg_timeout(timeout: Duration) -> DialOption {
    DialOption::AckMsgTimeout(timeout)
}

pub fn dial(path: impl AsRef<Path>, port: u32) -> io::Result<UnixStream> {
    dial_with_config(path, port, DialConfig::default())
}

pub fn dial_with_options(
    path: impl AsRef<Path>,
    port: u32,
    options: impl IntoIterator<Item = DialOption>,
) -> io::Result<UnixStream> {
    let mut config = DialConfig::default();
    for option in options {
        option.apply(&mut config);
    }
    dial_with_config(path, port, config)
}

pub fn dial_with_config(
    path: impl AsRef<Path>,
    port: u32,
    config: DialConfig,
) -> io::Result<UnixStream> {
    let path = path.as_ref();
    let deadline = Instant::now() + config.retry_timeout;

    loop {
        match try_connect(path, port, config) {
            Ok(stream) => return Ok(stream),
            Err(error) if is_temporary_net_error(&error) && Instant::now() < deadline => {
                let _ = error;
                thread::sleep(config.retry_interval);
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn connect_message(port: u32) -> String {
    format!("CONNECT {port}\n")
}

#[derive(Debug)]
pub struct ConnectMessageError {
    cause: io::Error,
}

impl ConnectMessageError {
    pub fn new(cause: io::Error) -> Self {
        Self { cause }
    }
}

impl fmt::Display for ConnectMessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vsock connect message failure: {}", self.cause)
    }
}

impl StdError for ConnectMessageError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.cause)
    }
}

#[derive(Debug)]
pub struct AckError {
    cause: io::Error,
}

impl AckError {
    pub fn new(cause: io::Error) -> Self {
        Self { cause }
    }
}

impl fmt::Display for AckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vsock ack message failure: {}", self.cause)
    }
}

impl StdError for AckError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.cause)
    }
}

pub fn is_temporary_net_error(error: &(dyn StdError + 'static)) -> bool {
    if error.downcast_ref::<AckError>().is_some() {
        return true;
    }

    let Some(error) = error.downcast_ref::<io::Error>() else {
        return false;
    };

    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotFound
            | io::ErrorKind::AddrNotAvailable
    ) {
        return true;
    }

    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<AckError>())
        .is_some()
}

fn try_connect(path: &Path, port: u32, config: DialConfig) -> io::Result<UnixStream> {
    let mut stream = UnixStream::connect(path)?;

    let message = connect_message(port);
    if let Err(error) = try_conn_write(&mut stream, message.as_bytes(), config.connect_msg_timeout)
    {
        return Err(io::Error::other(ConnectMessageError::new(io::Error::new(
            error.kind(),
            format!(
                "failed to write {message:?} within {:?}: {error}",
                config.connect_msg_timeout
            ),
        ))));
    }

    let line =
        try_conn_read_until(&mut stream, b'\n', config.ack_msg_timeout).map_err(|error| {
            io::Error::other(AckError::new(io::Error::new(
                error.kind(),
                format!(
                    "failed to read \"OK <port>\" within {:?}: {error}",
                    config.ack_msg_timeout
                ),
            )))
        })?;

    if !line.starts_with("OK ") {
        return Err(io::Error::other(AckError::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected to read \"OK <port>\", but instead read {line:?}"),
        ))));
    }

    Ok(stream)
}

fn try_conn_read_until(stream: &mut UnixStream, end: u8, timeout: Duration) -> io::Result<String> {
    stream.set_read_timeout(Some(timeout))?;

    let result = (|| {
        let mut buf = Vec::with_capacity(32);
        loop {
            let mut byte = [0u8; 1];
            let read = stream.read(&mut byte)?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF while waiting for vsock ack",
                ));
            }

            buf.push(byte[0]);
            if byte[0] == end {
                return String::from_utf8(buf).map_err(|error| {
                    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
                });
            }
        }
    })();

    let _ = stream.set_read_timeout(None);
    result
}

fn try_conn_write(
    stream: &mut UnixStream,
    expected_write: &[u8],
    timeout: Duration,
) -> io::Result<()> {
    stream.set_write_timeout(Some(timeout))?;

    let result = (|| {
        let bytes_written = stream.write(expected_write)?;
        if bytes_written != expected_write.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                format!(
                    "incomplete write, expected {} bytes but wrote {}",
                    expected_write.len(),
                    bytes_written
                ),
            ));
        }

        Ok(())
    })();

    let _ = stream.set_write_timeout(None);
    result
}
