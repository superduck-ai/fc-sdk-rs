use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::thread;
use std::time::Instant;

use super::{DialConfig, DialOption, is_temporary_net_error};

const AF_VSOCK: i32 = 40;
const SOCK_STREAM: i32 = 1;
const VMADDR_CID_ANY: u32 = u32::MAX;
const DEFAULT_BACKLOG: i32 = 128;

#[repr(C)]
struct SockAddrVm {
    svm_family: u16,
    svm_reserved1: u16,
    svm_port: u32,
    svm_cid: u32,
    svm_flags: u8,
    svm_zero: [u8; 3],
}

impl SockAddrVm {
    fn new(port: u32, cid: u32) -> Self {
        Self {
            svm_family: AF_VSOCK as u16,
            svm_reserved1: 0,
            svm_port: port,
            svm_cid: cid,
            svm_flags: 0,
            svm_zero: [0; 3],
        }
    }
}

pub struct VsockListener {
    fd: OwnedFd,
    port: u32,
    config: DialConfig,
}

impl std::fmt::Debug for VsockListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VsockListener")
            .field("fd", &self.fd.as_raw_fd())
            .field("port", &self.port)
            .field("config", &self.config)
            .finish()
    }
}

#[derive(Debug)]
pub struct VsockStream {
    fd: OwnedFd,
}

impl AsRawFd for VsockListener {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl AsRawFd for VsockStream {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl io::Read for VsockStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = unsafe { libc_read(self.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if read < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(read as usize)
        }
    }
}

impl io::Write for VsockStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = unsafe { libc_write(self.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
        if written < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(written as usize)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn listen(port: u32) -> io::Result<VsockListener> {
    listen_with_config(port, DialConfig::default())
}

pub fn listen_with_options(
    port: u32,
    options: impl IntoIterator<Item = DialOption>,
) -> io::Result<VsockListener> {
    let mut config = DialConfig::default();
    for option in options {
        match option {
            DialOption::DialTimeout(_) => {}
            DialOption::RetryTimeout(value) => config.retry_timeout = value,
            DialOption::RetryInterval(value) => config.retry_interval = value,
            DialOption::ConnectionMsgTimeout(value) => config.connect_msg_timeout = value,
            DialOption::AckMsgTimeout(value) => config.ack_msg_timeout = value,
        }
    }

    listen_with_config(port, config)
}

pub fn listen_with_config(port: u32, config: DialConfig) -> io::Result<VsockListener> {
    let fd = cvt_raw(unsafe { libc_socket(AF_VSOCK, SOCK_STREAM, 0) })?;
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };

    let addr = SockAddrVm::new(port, VMADDR_CID_ANY);
    cvt(unsafe {
        libc_bind(
            fd.as_raw_fd(),
            (&addr as *const SockAddrVm).cast(),
            std::mem::size_of::<SockAddrVm>() as u32,
        )
    })?;
    cvt(unsafe { libc_listen(fd.as_raw_fd(), DEFAULT_BACKLOG) })?;

    Ok(VsockListener { fd, port, config })
}

impl VsockListener {
    pub fn port(&self) -> u32 {
        self.port
    }

    pub fn accept(&self) -> io::Result<VsockStream> {
        accept_with_retry(self, self.config)
    }
}

trait AcceptOps {
    type Stream;

    fn accept_once(&self) -> io::Result<Self::Stream>;
}

impl AcceptOps for VsockListener {
    type Stream = VsockStream;

    fn accept_once(&self) -> io::Result<Self::Stream> {
        let fd = cvt_raw(unsafe {
            libc_accept(self.as_raw_fd(), std::ptr::null_mut(), std::ptr::null_mut())
        })?;
        Ok(VsockStream {
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
        })
    }
}

fn accept_with_retry<L>(listener: &L, config: DialConfig) -> io::Result<L::Stream>
where
    L: AcceptOps,
{
    let deadline = Instant::now() + config.retry_timeout;

    loop {
        match listener.accept_once() {
            Ok(stream) => return Ok(stream),
            Err(error) if is_temporary_net_error(&error) && Instant::now() < deadline => {
                thread::sleep(config.retry_interval);
            }
            Err(error) => return Err(error),
        }
    }
}

fn cvt(result: i32) -> io::Result<()> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn cvt_raw(result: i32) -> io::Result<i32> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

unsafe extern "C" {
    #[link_name = "socket"]
    fn libc_socket(domain: i32, ty: i32, protocol: i32) -> i32;
    #[link_name = "bind"]
    fn libc_bind(fd: i32, addr: *const std::ffi::c_void, len: u32) -> i32;
    #[link_name = "listen"]
    fn libc_listen(fd: i32, backlog: i32) -> i32;
    #[link_name = "accept"]
    fn libc_accept(fd: i32, addr: *mut std::ffi::c_void, len: *mut u32) -> i32;
    #[link_name = "read"]
    fn libc_read(fd: i32, buf: *mut std::ffi::c_void, count: usize) -> isize;
    #[link_name = "write"]
    fn libc_write(fd: i32, buf: *const std::ffi::c_void, count: usize) -> isize;
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{AcceptOps, DialConfig, accept_with_retry};

    struct FakeListener {
        responses: Arc<Mutex<VecDeque<std::io::Result<u8>>>>,
    }

    impl FakeListener {
        fn new(responses: impl IntoIterator<Item = std::io::Result<u8>>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses.into_iter().collect())),
            }
        }
    }

    impl AcceptOps for FakeListener {
        type Stream = u8;

        fn accept_once(&self) -> std::io::Result<Self::Stream> {
            self.responses.lock().unwrap().pop_front().unwrap()
        }
    }

    #[test]
    fn test_accept_with_retry_retries_temporary_errors() {
        let listener = FakeListener::new([
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "try again",
            )),
            Ok(7),
        ]);

        let stream = accept_with_retry(
            &listener,
            DialConfig {
                retry_timeout: Duration::from_millis(50),
                retry_interval: Duration::from_millis(5),
                ..DialConfig::default()
            },
        )
        .unwrap();

        assert_eq!(7, stream);
    }

    #[test]
    fn test_accept_with_retry_returns_non_temporary_errors() {
        let listener = FakeListener::new([Err(std::io::Error::other("boom"))]);
        let error = accept_with_retry(
            &listener,
            DialConfig {
                retry_timeout: Duration::from_millis(20),
                retry_interval: Duration::from_millis(5),
                ..DialConfig::default()
            },
        )
        .unwrap_err();

        assert_eq!(std::io::ErrorKind::Other, error.kind());
    }
}
