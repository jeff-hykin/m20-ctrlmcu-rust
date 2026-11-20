//! The MCU's UART as the vendor sets it up: raw 8N1, no flow control, 115200 baud. Reads block until a byte arrives.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

/// The vendor's device and rate.
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
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(access == Access::ReadWrite)
            .custom_flags(libc::O_NOCTTY)
            .open(path)?;
        configure_raw(&file, baud)?;
        Ok(Self { file })
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self { file: self.file.try_clone()? })
    }
}

impl Read for SerialPort {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}

impl Write for SerialPort {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Returns once the UART has shifted out everything written.
        check(unsafe { libc::tcdrain(self.file.as_raw_fd()) })
    }
}

fn check(result: libc::c_int) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
        check(libc::tcsetattr(fd, libc::TCSANOW, &tio))?;
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
    Ok(baud as libc::speed_t)
}
