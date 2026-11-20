#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod client;
pub mod command;
pub mod frame;
pub mod reply;
#[cfg(unix)]
pub mod serial;

pub use client::{spawn_reader, Client, Exchange, Policy, Received, RequestError};
pub use command::{Command, Effect, FrontBack, Func, LedEntry};
pub use frame::{crc16_modbus, hex, Decoder, DecoderStats, Frame, ParseError, PayloadTooLong, Sequence};
pub use reply::{FanSpeeds, Reply};
