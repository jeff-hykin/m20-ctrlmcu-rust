//! The MCU's UART as the vendor sets it up: raw 8N1, no flow control, 115200 baud. Reads block until a byte arrives.

use std::fs::File;
use std::io::{self, PipeReader, PipeWriter, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;

/// The vendor's device and rate (protocol constants, from `ctrlmcu_node`).
pub const VENDOR_DEVICE: &str = "/dev/ttyS3";
pub const VENDOR_BAUD: u32 = 115_200;

/// `ReadOnly` opens the device `O_RDONLY`, so nothing can be written through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}

#[derive(Debug)]
pub struct SerialPort {
    file: File,
}

impl SerialPort {
    /// Open and configure; pending input is discarded, nothing is sent.
    pub fn open(path: &str, baud: u32, access: Access) -> io::Result<Self> {
        // O_NONBLOCK so the open cannot wait for carrier detect while CLOCAL is still unset; cleared once configured.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(access == Access::ReadWrite)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(path)?;
        configure_raw(&file, baud)?;
        set_blocking(&file)?;
        Ok(Self { file })
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self { file: self.file.try_clone()? })
    }

    /// A clone of this port whose blocking read can be ended from another thread (see [`StoppableReader`]).
    pub fn stoppable_reader(&self) -> io::Result<(StoppableReader<SerialPort>, ReadStopper)> {
        StoppableReader::new(self.try_clone()?)
    }
}

impl AsFd for SerialPort {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl AsRawFd for SerialPort {
    fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

impl Read for SerialPort {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}

/// Ends a [`StoppableReader`]'s read: on [`ReadStopper::stop`] or when dropped.
#[derive(Debug)]
pub struct ReadStopper {
    wake: Option<PipeWriter>,
}

impl ReadStopper {
    pub fn stop(&mut self) {
        // Closing the write end wakes the reader's poll with a hangup.
        self.wake = None;
    }
}

/// Wraps a blocking fd reader: each read waits (poll, no timeout) for either input or its [`ReadStopper`], and
/// returns `Ok(0)` once stopped, so a reader thread on a quiet port can still be ended and joined.
#[derive(Debug)]
pub struct StoppableReader<R> {
    inner: R,
    wake: PipeReader,
}

impl<R: Read + AsRawFd> StoppableReader<R> {
    pub fn new(inner: R) -> io::Result<(Self, ReadStopper)> {
        let (wake_reader, wake_writer) = io::pipe()?;
        Ok((Self { inner, wake: wake_reader }, ReadStopper { wake: Some(wake_writer) }))
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + AsRawFd> Read for StoppableReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut fds = [
                libc::pollfd { fd: self.inner.as_raw_fd(), events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            let [port, wake] = fds;
            if wake.revents != 0 {
                return Ok(0);
            }
            if port.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "poll: the port's fd is not open"));
            }
            if port.revents != 0 {
                // POLLIN, POLLHUP or POLLERR: the read itself returns the data, the EOF or the error.
                match self.inner.read(buffer) {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    result => return result,
                }
            }
        }
    }
}

impl Write for SerialPort {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Returns once the UART has shifted out everything written.
        retrying(|| unsafe { libc::tcdrain(self.file.as_raw_fd()) })
    }
}

fn check(result: libc::c_int) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// `call` again while it fails with EINTR.
fn retrying(mut call: impl FnMut() -> libc::c_int) -> io::Result<()> {
    loop {
        match check(call()) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

fn set_blocking(file: &File) -> io::Result<()> {
    let fd = file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    check(flags)?;
    check(unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) })
}

fn configure_raw(file: &File, baud: u32) -> io::Result<()> {
    let fd = file.as_raw_fd();
    let speed = speed_of(baud)?;
    unsafe {
        let mut tio: libc::termios = std::mem::zeroed();
        check(libc::tcgetattr(fd, &mut tio))?;
        libc::cfmakeraw(&mut tio);
        tio.c_cflag |= libc::CLOCAL | libc::CREAD;
        tio.c_cflag &= !(libc::CSTOPB | libc::PARENB | libc::CRTSCTS);
        tio.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);
        tio.c_cc[libc::VMIN] = 1;
        tio.c_cc[libc::VTIME] = 0;
        check(libc::cfsetispeed(&mut tio, speed))?;
        check(libc::cfsetospeed(&mut tio, speed))?;
        retrying(|| libc::tcsetattr(fd, libc::TCSANOW, &tio))?;
        check(libc::tcflush(fd, libc::TCIFLUSH))?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn speed_of(baud: u32) -> io::Result<libc::speed_t> {
    Ok(match baud {
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        230400 => libc::B230400,
        460800 => libc::B460800,
        921600 => libc::B921600,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("unsupported baud rate {baud}"))),
    })
}

// BSD-style termios takes the rate itself
#[cfg(not(target_os = "linux"))]
fn speed_of(baud: u32) -> io::Result<libc::speed_t> {
    // speed_t is at least 32 bits wide here, so this never truncates.
    Ok(baud as libc::speed_t)
}
