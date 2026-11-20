//! The client against an in-memory port: what it writes, what it refuses, how it reads replies.

use std::io::{self, Write};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use m20_ctrlmcu::{
    spawn_reader, Client, Command, Decoder, FanSpeeds, Frame, FrontBack, Policy, Received, Reply, RequestError,
};

#[derive(Clone, Default)]
struct Wire(Arc<Mutex<Vec<u8>>>);

impl Write for Wire {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn reply(func: u8, payload: Vec<u8>) -> Received {
    Received { frame: Frame::host(1, func, payload), host_time: SystemTime::now() }
}

fn client(policy: Policy) -> (Client<Wire>, Wire, Sender<Received>) {
    let wire = Wire::default();
    let (sender, receiver) = channel();
    (Client::new(wire.clone(), receiver, policy), wire, sender)
}

const SHORT: Duration = Duration::from_millis(50);

#[test]
fn only_queries_are_allowed_by_default() {
    let (mut client, wire, _sender) = client(Policy::default());
    for command in [
        Command::LidarPower(FrontBack::BOTH_ON),
        Command::LidarPower(FrontBack::default()),
        Command::VideoPower(FrontBack::BOTH_ON),
        Command::ExtLoadPower(true),
        Command::FpPower(false),
        Command::GpsPower(1),
        Command::FanSpeed(100),
        Command::HostLed(true),
        Command::LedStatus(Vec::new()),
    ] {
        assert!(matches!(client.request(&command, SHORT), Err(RequestError::Refused(_))), "{command:?}");
    }
    assert!(wire.0.lock().unwrap().is_empty());
}

#[test]
fn power_needs_its_own_permission() {
    let policy = Policy { allow_lights: true, allow_fans: true, allow_power: false };
    let (mut client, wire, _sender) = client(policy);
    let refused = client.request(&Command::LidarPower(FrontBack::BOTH_ON), SHORT);
    assert!(matches!(refused, Err(RequestError::Refused(_))));
    assert!(wire.0.lock().unwrap().is_empty());
}

#[test]
fn frames_waiting_before_a_request_are_unsolicited_not_its_reply() {
    let (mut client, wire, sender) = client(Policy::default());
    sender.send(reply(0x43, vec![0; 12])).unwrap();
    match client.request(&Command::FanSpeedQuery, SHORT) {
        Err(RequestError::Timeout { unsolicited, .. }) => assert_eq!(unsolicited.len(), 1),
        other => panic!("{other:?}"),
    }
    assert_eq!(*wire.0.lock().unwrap(), [0xee, 0xef, 0x01, 0x01, 0x17, 0x43, 0x00, 0x00, 0xff, 0xff]);
}

#[test]
fn a_fan_speed_query_reads_tail_mid_head() {
    let payload: Vec<u8> = [1200u32, 3400, 5600].iter().flat_map(|v| v.to_le_bytes()).collect();
    let exchange = exchange_with_reply(Policy::default(), &Command::FanSpeedQuery, Frame::host(1, 0x43, payload));
    assert_eq!(exchange.decoded, Reply::FanSpeeds(FanSpeeds { tail: 1200, mid: 3400, head: 5600 }));
}

#[test]
fn replies_are_decoded_for_the_request_that_drew_them() {
    let fans = Frame::host(7, 0x43, [10u32, 20, 30].iter().flat_map(|v| v.to_le_bytes()).collect());
    assert_eq!(
        Reply::read(m20_ctrlmcu::Func::FanSpeedQuery, &fans),
        Reply::FanSpeeds(FanSpeeds { tail: 10, mid: 20, head: 30 })
    );
    let current = Frame::host(7, 0x44, 250u32.to_le_bytes().to_vec());
    assert_eq!(Reply::read(m20_ctrlmcu::Func::ExtLoadCurrentQuery, &current), Reply::ExtLoadCurrent(250));
    assert_eq!(Reply::read(m20_ctrlmcu::Func::ExtLoadCurrentQuery, &fans), Reply::WrongSize { expected: 4, got: 12 });
    assert_eq!(Reply::read(m20_ctrlmcu::Func::LidarPower, &fans), Reply::Ack);
}

/// Run one request against a fake MCU that writes `answer` into the port shortly after.
fn exchange_with_reply(policy: Policy, command: &Command, answer: Frame) -> m20_ctrlmcu::Exchange {
    let (frames_tx, frames_rx) = channel();
    let (errors_tx, _errors_rx) = channel();
    let (mut mcu, port) = pipe();
    spawn_reader(port, Decoder::new(256, 4096), 64, frames_tx, errors_tx);
    let mut client = Client::new(Wire::default(), frames_rx, policy);
    let replier = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        mcu.write_all(&answer.encode()).unwrap();
    });
    let exchange = client.request(command, Duration::from_secs(2)).unwrap();
    replier.join().unwrap();
    exchange
}

#[test]
fn a_reply_arriving_after_the_write_completes_the_request() {
    let policy = Policy { allow_lights: true, ..Policy::default() };
    let exchange = exchange_with_reply(policy, &Command::HostLed(true), Frame::host(1, 0x46, vec![0, 0]));
    assert_eq!(exchange.request.encode(), [0xee, 0xef, 0x01, 0x01, 0x17, 0x46, 0x02, 0x00, 0x00, 0x20, 0x01, 0x00]);
    assert_eq!(exchange.reply.frame.func, 0x46);
    assert_eq!(exchange.decoded, Reply::Ack);
    assert!(exchange.unsolicited.is_empty());
}

/// An in-process byte pipe: writes on one end are reads on the other.
fn pipe() -> (PipeWriter, PipeReader) {
    let (sender, receiver) = channel::<Vec<u8>>();
    (PipeWriter(sender), PipeReader(receiver))
}

struct PipeWriter(Sender<Vec<u8>>);

impl Write for PipeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.send(bytes.to_vec()).map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PipeReader(std::sync::mpsc::Receiver<Vec<u8>>);

impl io::Read for PipeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.0.recv() {
            Ok(bytes) => {
                buffer[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
            Err(_) => Ok(0),
        }
    }
}
