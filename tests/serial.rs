//! The UART wrapper and the stoppable reader, against a pseudo-terminal and a socket pair.
#![cfg(unix)]

use std::ffi::CStr;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::channel;
use std::time::Duration;

use m20_ctrlmcu::serial::{Access, SerialPort, StoppableReader};

#[test]
fn a_stopped_reader_returns_end_of_file_on_a_quiet_port() {
    let (_mcu, host) = UnixStream::pair().unwrap();
    let (mut reader, mut stopper) = StoppableReader::new(host).unwrap();
    let (done_tx, done_rx) = channel();
    let thread = std::thread::spawn(move || {
        let mut buffer = [0u8; 8];
        done_tx.send(reader.read(&mut buffer).unwrap()).unwrap();
    });
    assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err(), "the read should still be waiting");
    stopper.stop();
    assert_eq!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 0);
    thread.join().unwrap();
}

#[test]
fn dropping_the_stopper_also_ends_the_read() {
    let (_mcu, host) = UnixStream::pair().unwrap();
    let (mut reader, stopper) = StoppableReader::new(host).unwrap();
    let thread = std::thread::spawn(move || reader.read(&mut [0u8; 8]).unwrap());
    drop(stopper);
    assert_eq!(thread.join().unwrap(), 0);
}

#[test]
fn a_stoppable_reader_passes_bytes_through() {
    let (mut mcu, host) = UnixStream::pair().unwrap();
    let (mut reader, _stopper) = StoppableReader::new(host).unwrap();
    mcu.write_all(&[0xee, 0xef, 1]).unwrap();
    let mut buffer = [0u8; 8];
    let count = reader.read(&mut buffer).unwrap();
    assert_eq!(buffer[..count], [0xee, 0xef, 1]);
    drop(mcu);
    assert_eq!(reader.read(&mut buffer).unwrap(), 0);
}

/// A pseudo-terminal's controller and the path of its device end.
fn pty() -> (std::fs::File, String) {
    use std::os::fd::FromRawFd;
    unsafe {
        let controller = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(controller >= 0);
        assert_eq!(libc::grantpt(controller), 0);
        assert_eq!(libc::unlockpt(controller), 0);
        let name = CStr::from_ptr(libc::ptsname(controller)).to_str().unwrap().to_string();
        (std::fs::File::from_raw_fd(controller), name)
    }
}

#[test]
fn an_opened_port_blocks_on_read_and_carries_bytes_both_ways() {
    let (mut controller, path) = pty();
    let mut port = SerialPort::open(&path, 115_200, Access::ReadWrite).unwrap();
    let flags = unsafe { libc::fcntl(port.as_raw_fd(), libc::F_GETFL) };
    assert_eq!(flags & libc::O_NONBLOCK, 0, "the open's O_NONBLOCK must be cleared");
    // flush (tcdrain) returns once the other end has taken the bytes, so that end reads on its own thread.
    let mut far_end = controller.try_clone().unwrap();
    let far_reader = std::thread::spawn(move || {
        let mut buffer = [0u8; 3];
        far_end.read_exact(&mut buffer).unwrap();
        buffer
    });
    port.write_all(&[1, 2, 3]).unwrap();
    port.flush().unwrap();
    assert_eq!(far_reader.join().unwrap(), [1, 2, 3]);
    let (mut reader, mut stopper) = port.stoppable_reader().unwrap();
    controller.write_all(&[9]).unwrap();
    let mut one = [0u8; 1];
    assert_eq!(reader.read(&mut one).unwrap(), 1);
    assert_eq!(one, [9]);
    stopper.stop();
    assert_eq!(reader.read(&mut one).unwrap(), 0);
}
