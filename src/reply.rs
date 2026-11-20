//! What the MCU sends back. The vendor host takes the first whole frame after its request as the reply, whatever its
//! function code, sequence or CRC, and only looks inside it for the two queries.

use crate::command::Func;
use crate::frame::Frame;

/// Fan speeds from a [`Func::FanSpeedQuery`] reply: three little-endian u32s in this order (unit unconfirmed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FanSpeeds {
    pub tail: u32,
    pub mid: u32,
    pub head: u32,
}

/// A reply read the way the vendor reads it for the request that drew it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    FanSpeeds(FanSpeeds),
    /// A [`Func::ExtLoadCurrentQuery`] reply: one little-endian u32 (unit unconfirmed).
    ExtLoadCurrent(u32),
    /// Any reply to a command: the vendor only checks that a frame came back.
    Ack,
    /// A query reply whose payload was not the expected size.
    WrongSize {
        expected: usize,
        got: usize,
    },
}

impl Reply {
    pub fn read(request: Func, frame: &Frame) -> Self {
        let payload = &frame.payload;
        let word = |at: usize| u32::from_le_bytes([payload[at], payload[at + 1], payload[at + 2], payload[at + 3]]);
        match request {
            Func::FanSpeedQuery if payload.len() == 12 => {
                Reply::FanSpeeds(FanSpeeds { tail: word(0), mid: word(4), head: word(8) })
            }
            Func::FanSpeedQuery => Reply::WrongSize { expected: 12, got: payload.len() },
            Func::ExtLoadCurrentQuery if payload.len() == 4 => Reply::ExtLoadCurrent(word(0)),
            Func::ExtLoadCurrentQuery => Reply::WrongSize { expected: 4, got: payload.len() },
            _ => Reply::Ack,
        }
    }
}
