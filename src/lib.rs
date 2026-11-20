#![doc = include_str!("../README.md")]

pub mod client;
pub mod command;
pub mod frame;
pub mod reply;
#[cfg(unix)]
pub mod serial;

pub use client::{spawn_reader, Client, Exchange, Policy, Received, RequestError};
pub use command::{Command, Effect, FrontBack, Func, LedEntry};
pub use frame::{crc16_modbus, hex, Decoder, DecoderStats, Frame, ParseError, Sequence};
pub use reply::{FanSpeeds, Reply};
