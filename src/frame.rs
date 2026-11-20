//! The wire frame: `EE EF | seq | 0x01 | type | func | len (u16 LE) | crc (u16 LE) | payload`, CRC-16/MODBUS over the
//! payload only. The constants below are protocol constants, from the vendor's `ProtocolHandler` (see PROTOCOL.md).

use std::fmt;

/// First two bytes of every frame.
pub const HEADER: [u8; 2] = [0xee, 0xef];
/// Bytes before the payload.
pub const HEADER_LEN: usize = 10;
/// Byte 3 of every frame the vendor host builds.
pub const VERSION: u8 = 0x01;
/// Byte 4 (frame type) of every frame the vendor host builds.
pub const HOST_FRAME_TYPE: u8 = 0x17;

/// CRC-16/MODBUS (reflected poly 0xA001, init 0xFFFF), as `ProtocolHandler::build_frame` computes it.
pub fn crc16_modbus(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &byte in bytes {
        crc ^= byte as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xa001 } else { crc >> 1 };
        }
    }
    crc
}

/// One frame in either direction.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub sequence: u8,
    pub version: u8,
    pub frame_type: u8,
    pub func: u8,
    /// The CRC as carried on the wire (see [`Frame::crc_ok`]).
    pub crc: u16,
    pub payload: Vec<u8>,
}

impl Frame {
    /// A host-to-MCU frame with the vendor's version and frame type and a fresh CRC.
    pub fn host(sequence: u8, func: u8, payload: Vec<u8>) -> Self {
        let crc = crc16_modbus(&payload);
        Self { sequence, version: VERSION, frame_type: HOST_FRAME_TYPE, func, crc, payload }
    }

    /// The frame's bytes. A payload over 65535 bytes cannot be framed: [`Frame::try_encode`] refuses one, this writes
    /// only its first 65535 bytes (every [`crate::Command`] payload is 16 bytes or fewer).
    pub fn encode(&self) -> Vec<u8> {
        let length = u16::try_from(self.payload.len()).unwrap_or(u16::MAX);
        let payload = &self.payload[..usize::from(length)];
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
        bytes.extend_from_slice(&HEADER);
        bytes.extend_from_slice(&[self.sequence, self.version, self.frame_type, self.func]);
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&self.crc.to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    /// The frame's bytes, or an error when the payload does not fit the 16-bit length.
    pub fn try_encode(&self) -> Result<Vec<u8>, PayloadTooLong> {
        if self.payload.len() > usize::from(u16::MAX) {
            return Err(PayloadTooLong(self.payload.len()));
        }
        Ok(self.encode())
    }

    /// Whether the carried CRC matches the payload (the vendor host never checks replies).
    pub fn crc_ok(&self) -> bool {
        self.crc == crc16_modbus(&self.payload)
    }

    /// One whole frame from the start of `bytes`, and how many bytes it took.
    pub fn parse(bytes: &[u8]) -> Result<(Self, usize), ParseError> {
        if bytes.len() < HEADER_LEN {
            return Err(ParseError::Short);
        }
        if bytes[..2] != HEADER {
            return Err(ParseError::BadHeader);
        }
        let length = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let total = HEADER_LEN + length;
        if bytes.len() < total {
            return Err(ParseError::Short);
        }
        let frame = Self {
            sequence: bytes[2],
            version: bytes[3],
            frame_type: bytes[4],
            func: bytes[5],
            crc: u16::from_le_bytes([bytes[8], bytes[9]]),
            payload: bytes[HEADER_LEN..total].to_vec(),
        };
        Ok((frame, total))
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Frame {{ seq {}, version {:#04x}, type {:#04x}, func {:#04x}, crc {:#06x}{}, payload [{}] }}",
            self.sequence,
            self.version,
            self.frame_type,
            self.func,
            self.crc,
            if self.crc_ok() { "" } else { " (mismatch)" },
            hex(&self.payload)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Short,
    BadHeader,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Short => "not a whole frame yet",
            Self::BadHeader => "no EE EF header",
        })
    }
}

impl std::error::Error for ParseError {}

/// A payload (of this many bytes) over the 65535 the frame's 16-bit length can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayloadTooLong(pub usize);

impl fmt::Display for PayloadTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a {}-byte payload does not fit a frame (65535 at most)", self.0)
    }
}

impl std::error::Error for PayloadTooLong {}

/// Bytes as `ee ef 01 ...`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// The vendor's sequence counter: starts at 1, wraps 255 -> 1, never 0.
#[derive(Clone, Copy, Debug)]
pub struct Sequence(u8);

impl Default for Sequence {
    fn default() -> Self {
        Self(1)
    }
}

impl Sequence {
    pub fn take(&mut self) -> u8 {
        let current = self.0;
        self.0 = match self.0.wrapping_add(1) {
            0 => 1,
            next => next,
        };
        current
    }
}

/// Counts of what the stream decoder skipped or dropped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecoderStats {
    pub frames: u64,
    /// Bytes before a header, or of a header whose length was over the limit.
    pub skipped_bytes: u64,
    /// Bytes dropped because the buffer was full without a whole frame.
    pub overflow_bytes: u64,
}

/// Splits a byte stream into frames.
#[derive(Debug)]
pub struct Decoder {
    buffer: Vec<u8>,
    max_payload: usize,
    max_buffered: usize,
    stats: DecoderStats,
}

impl Decoder {
    /// `max_payload`: a longer declared length is taken as a false header; `max_buffered`: bytes held waiting.
    pub fn new(max_payload: usize, max_buffered: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_payload,
            max_buffered: max_buffered.max(HEADER_LEN.saturating_add(max_payload)),
            stats: DecoderStats::default(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > self.max_buffered {
            let excess = self.buffer.len() - self.max_buffered;
            self.buffer.drain(..excess);
            self.stats.overflow_bytes += excess as u64;
        }
    }

    pub fn next_frame(&mut self) -> Option<Frame> {
        loop {
            let start = self.buffer.windows(2).position(|pair| pair == HEADER);
            let start = match start {
                Some(start) => start,
                None => {
                    // Keep a trailing 0xEE that may begin the next header.
                    let keep = usize::from(self.buffer.last() == Some(&HEADER[0]));
                    let skipped = self.buffer.len() - keep;
                    self.buffer.drain(..skipped);
                    self.stats.skipped_bytes += skipped as u64;
                    return None;
                }
            };
            if start > 0 {
                self.buffer.drain(..start);
                self.stats.skipped_bytes += start as u64;
            }
            if self.buffer.len() >= HEADER_LEN {
                let length = u16::from_le_bytes([self.buffer[6], self.buffer[7]]) as usize;
                if length > self.max_payload {
                    self.buffer.drain(..1);
                    self.stats.skipped_bytes += 1;
                    continue;
                }
            }
            return match Frame::parse(&self.buffer) {
                Ok((frame, used)) => {
                    self.buffer.drain(..used);
                    self.stats.frames += 1;
                    Some(frame)
                }
                Err(_) => None,
            };
        }
    }

    /// Bytes held that are not yet a whole frame.
    pub fn pending(&self) -> &[u8] {
        &self.buffer
    }

    pub fn stats(&self) -> DecoderStats {
        self.stats
    }
}
