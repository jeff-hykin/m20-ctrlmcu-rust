//! Request/reply over any byte sink plus a channel of received frames, one request at a time, as the vendor host
//! does (it holds a mutex around write + wait). Commands whose [`Effect`] the [`Policy`] does not allow are refused
//! before a byte is written.

use std::fmt;
use std::io::{self, Read, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use crate::command::{Command, Effect};
use crate::frame::{Decoder, Frame, Sequence};
use crate::reply::Reply;

/// A frame off the port and when the read holding its last byte returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    pub frame: Frame,
    pub host_time: SystemTime,
}

/// Which kinds of command may be written. Everything but queries is off by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    pub allow_lights: bool,
    pub allow_fans: bool,
    /// Lidar, camera, external-load, FP radio and GPS power.
    pub allow_power: bool,
}

impl Policy {
    pub fn allows(&self, effect: Effect) -> bool {
        match effect {
            Effect::Query => true,
            Effect::Light => self.allow_lights,
            Effect::Fan => self.allow_fans,
            Effect::Power => self.allow_power,
        }
    }
}

/// One request and what came back.
#[derive(Clone, Debug)]
pub struct Exchange {
    pub request: Frame,
    pub reply: Received,
    pub decoded: Reply,
    /// Frames that arrived while no request was waiting.
    pub unsolicited: Vec<Received>,
    pub round_trip: Duration,
}

#[derive(Debug)]
pub enum RequestError {
    /// The policy does not allow this command's effect; nothing was written.
    Refused(Command),
    Write(io::Error),
    /// Nothing came back in time.
    Timeout {
        request: Frame,
        unsolicited: Vec<Received>,
    },
    /// The reader has stopped.
    Disconnected,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(command) => write!(f, "refused by policy: {:?} ({})", command, command.func()),
            Self::Write(error) => write!(f, "write failed: {error}"),
            Self::Timeout { request, .. } => write!(f, "no reply to {request:?}"),
            Self::Disconnected => f.write_str("the reader has stopped"),
        }
    }
}

impl std::error::Error for RequestError {}

impl RequestError {
    /// The vendor's `SerialError` code for this failure (0 is success), which it returns as the service result.
    pub fn vendor_code(&self) -> i32 {
        match self {
            Self::Refused(_) => -1,
            Self::Write(_) => 1,
            Self::Timeout { .. } => 3,
            Self::Disconnected => 4,
        }
    }
}

pub struct Client<W: Write> {
    writer: W,
    frames: Receiver<Received>,
    sequence: Sequence,
    policy: Policy,
}

impl<W: Write> Client<W> {
    pub fn new(writer: W, frames: Receiver<Received>, policy: Policy) -> Self {
        Self { writer, frames, sequence: Sequence::default(), policy }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// Frames that arrived with no request waiting, without blocking.
    pub fn drain(&mut self) -> Result<Vec<Received>, RequestError> {
        let mut drained = Vec::new();
        loop {
            match self.frames.try_recv() {
                Ok(received) => drained.push(received),
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(drained),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return if drained.is_empty() { Err(RequestError::Disconnected) } else { Ok(drained) }
                }
            }
        }
    }

    /// Write `command` and wait up to `timeout` for the next frame, which is taken as its reply.
    pub fn request(&mut self, command: &Command, timeout: Duration) -> Result<Exchange, RequestError> {
        if !self.policy.allows(command.effect()) {
            return Err(RequestError::Refused(command.clone()));
        }
        let unsolicited = self.drain()?;
        let request = command.frame(self.sequence.take());
        let started = Instant::now();
        self.writer.write_all(&request.encode()).and_then(|()| self.writer.flush()).map_err(RequestError::Write)?;
        match self.frames.recv_timeout(timeout) {
            Ok(reply) => Ok(Exchange {
                decoded: Reply::read(command.func(), &reply.frame),
                request,
                reply,
                unsolicited,
                round_trip: started.elapsed(),
            }),
            Err(RecvTimeoutError::Timeout) => Err(RequestError::Timeout { request, unsolicited }),
            Err(RecvTimeoutError::Disconnected) => Err(RequestError::Disconnected),
        }
    }
}

/// Decode `port` on a thread, sending each frame on; the thread ends on a read error, at EOF, or when the receiver is
/// gone and the next frame arrives. To end it on a quiet port, pass a [`crate::serial::StoppableReader`] and stop it.
pub fn spawn_reader<R: Read + Send + 'static>(
    mut port: R,
    mut decoder: Decoder,
    read_chunk_bytes: usize,
    frames: Sender<Received>,
    errors: Sender<io::Error>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = vec![0u8; read_chunk_bytes.max(1)];
        loop {
            let count = match port.read(&mut chunk) {
                Ok(0) => {
                    let _ = errors.send(io::Error::new(io::ErrorKind::UnexpectedEof, "serial device closed"));
                    return;
                }
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let _ = errors.send(error);
                    return;
                }
            };
            let host_time = SystemTime::now();
            decoder.push(&chunk[..count]);
            while let Some(frame) = decoder.next_frame() {
                if frames.send(Received { frame, host_time }).is_err() {
                    return;
                }
            }
        }
    })
}
