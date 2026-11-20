//! What the host asks of the MCU: one function code per vendor DDS topic, the payload built the way `ctrlmcu_node`
//! builds it from that topic's `int32` command.

use std::fmt;
use std::time::Duration;

use crate::frame::{Frame, Sequence};

/// Function codes (frame byte 5) the vendor host sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Func {
    /// Status (colour) LEDs, two 8-byte entries; vendor topic `/LED/STATUS` (`LedStatusLight`).
    LedStatus = 0x40,
    /// Fill (flood) lights front/back; `/LED/FILL`.
    FillLight = 0x41,
    /// Fan speed setpoint; `/FAN/SPEED`.
    FanSpeed = 0x42,
    /// Fan speed query (12-byte reply: tail, mid, head); `/FAN/STATUS`.
    FanSpeedQuery = 0x43,
    /// External-load current query (4-byte reply); `/EXT_LOAD/CUR`.
    ExtLoadCurrentQuery = 0x44,
    /// External-load power switch; `/EXT_LOAD/POWER`.
    ExtLoadPower = 0x45,
    /// Main (host power) indicator LED; `/LED/HOST/POWER`.
    HostLed = 0x46,
    /// External-load indicator LED; `/LED/EXT/POWER`.
    ExtLed = 0x47,
    /// Image-transmission / pairing ("FP") radio power; `/FP/POWER`.
    FpPower = 0x48,
    /// Front and rear lidar power; `/LIDAR/POWER`.
    LidarPower = 0x49,
    /// GPS power; `/GPS/POWER`.
    GpsPower = 0x50,
    /// Front and rear wide-angle camera power; `/VIDEO/POWER`.
    VideoPower = 0x51,
}

/// What a command does to the robot, for gating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Reads only (fan speed, load current).
    Query,
    /// LEDs and lights.
    Light,
    Fan,
    /// Switches real power (lidars, cameras, external load, FP radio, GPS).
    Power,
}

impl Func {
    pub const ALL: [Func; 12] = [
        Func::LedStatus,
        Func::FillLight,
        Func::FanSpeed,
        Func::FanSpeedQuery,
        Func::ExtLoadCurrentQuery,
        Func::ExtLoadPower,
        Func::HostLed,
        Func::ExtLed,
        Func::FpPower,
        Func::LidarPower,
        Func::GpsPower,
        Func::VideoPower,
    ];

    pub fn from_u8(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|func| *func as u8 == code)
    }

    /// The snake_case name used for config and module ports.
    pub fn name(self) -> &'static str {
        match self {
            Func::LedStatus => "led_status",
            Func::FillLight => "fill_light",
            Func::FanSpeed => "fan_speed",
            Func::FanSpeedQuery => "fan_speed_query",
            Func::ExtLoadCurrentQuery => "ext_load_current_query",
            Func::ExtLoadPower => "ext_load_power",
            Func::HostLed => "host_led",
            Func::ExtLed => "ext_led",
            Func::FpPower => "fp_power",
            Func::LidarPower => "lidar_power",
            Func::GpsPower => "gps_power",
            Func::VideoPower => "video_power",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|func| func.name() == name)
    }

    /// The vendor DDS topic `ctrlmcu_node` serves this on.
    pub fn vendor_topic(self) -> &'static str {
        match self {
            Func::LedStatus => "/LED/STATUS",
            Func::FillLight => "/LED/FILL",
            Func::FanSpeed => "/FAN/SPEED",
            Func::FanSpeedQuery => "/FAN/STATUS",
            Func::ExtLoadCurrentQuery => "/EXT_LOAD/CUR",
            Func::ExtLoadPower => "/EXT_LOAD/POWER",
            Func::HostLed => "/LED/HOST/POWER",
            Func::ExtLed => "/LED/EXT/POWER",
            Func::FpPower => "/FP/POWER",
            Func::LidarPower => "/LIDAR/POWER",
            Func::GpsPower => "/GPS/POWER",
            Func::VideoPower => "/VIDEO/POWER",
        }
    }

    pub fn effect(self) -> Effect {
        match self {
            Func::FanSpeedQuery | Func::ExtLoadCurrentQuery => Effect::Query,
            Func::LedStatus | Func::FillLight | Func::HostLed | Func::ExtLed => Effect::Light,
            Func::FanSpeed => Effect::Fan,
            Func::ExtLoadPower | Func::FpPower | Func::LidarPower | Func::GpsPower | Func::VideoPower => Effect::Power,
        }
    }

    /// How long the vendor waits for the reply.
    pub fn vendor_timeout(self) -> Duration {
        match self {
            Func::FillLight | Func::FanSpeed | Func::FanSpeedQuery => Duration::from_millis(2000),
            _ => Duration::from_millis(1000),
        }
    }
}

impl fmt::Display for Func {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:#04x})", self.name(), *self as u8)
    }
}

/// One `drdds::msg::SIL` entry of a `LedStatusLight`, as it goes on the wire.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LedEntry {
    pub kind: u8,
    pub dir: bool,
    pub num: u8,
    pub cycle: u8,
    /// The vendor copies at most the first two colour bytes.
    pub color: [u8; 2],
}

/// A front/back pair packed the way basic_server packs it: `back | front << 16`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrontBack {
    pub front: u16,
    pub back: u16,
}

impl FrontBack {
    pub const BOTH_ON: FrontBack = FrontBack { front: 1, back: 1 };

    pub fn from_vendor(value: i32) -> Self {
        let value = value as u32;
        Self { front: (value >> 16) as u16, back: value as u16 }
    }

    pub fn to_vendor(self) -> i32 {
        ((self.front as u32) << 16 | self.back as u32) as i32
    }
}

/// A request to the MCU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Up to two entries (more are ignored, as the vendor does); a missing entry goes out as zeros.
    LedStatus(Vec<LedEntry>),
    FillLight(FrontBack),
    /// The vendor sends the low 16 bits of its int32 (basic_server's default reading is 100).
    FanSpeed(i16),
    FanSpeedQuery,
    ExtLoadCurrentQuery,
    ExtLoadPower(bool),
    HostLed(bool),
    ExtLed(bool),
    FpPower(bool),
    LidarPower(FrontBack),
    GpsPower(i16),
    VideoPower(FrontBack),
}

impl Command {
    /// The command `ctrlmcu_node` builds from an int32 on `func`'s topic (not for [`Func::LedStatus`]).
    pub fn from_vendor(func: Func, value: i32) -> Option<Self> {
        Some(match func {
            Func::LedStatus => return None,
            Func::FillLight => Command::FillLight(FrontBack::from_vendor(value)),
            Func::FanSpeed => Command::FanSpeed(value as i16),
            Func::FanSpeedQuery => Command::FanSpeedQuery,
            Func::ExtLoadCurrentQuery => Command::ExtLoadCurrentQuery,
            Func::ExtLoadPower => Command::ExtLoadPower(value != 0),
            Func::HostLed => Command::HostLed(value != 0),
            Func::ExtLed => Command::ExtLed(value != 0),
            Func::FpPower => Command::FpPower(value != 0),
            Func::LidarPower => Command::LidarPower(FrontBack::from_vendor(value)),
            Func::GpsPower => Command::GpsPower(value as i16),
            Func::VideoPower => Command::VideoPower(FrontBack::from_vendor(value)),
        })
    }

    pub fn func(&self) -> Func {
        match self {
            Command::LedStatus(_) => Func::LedStatus,
            Command::FillLight(_) => Func::FillLight,
            Command::FanSpeed(_) => Func::FanSpeed,
            Command::FanSpeedQuery => Func::FanSpeedQuery,
            Command::ExtLoadCurrentQuery => Func::ExtLoadCurrentQuery,
            Command::ExtLoadPower(_) => Func::ExtLoadPower,
            Command::HostLed(_) => Func::HostLed,
            Command::ExtLed(_) => Func::ExtLed,
            Command::FpPower(_) => Func::FpPower,
            Command::LidarPower(_) => Func::LidarPower,
            Command::GpsPower(_) => Func::GpsPower,
            Command::VideoPower(_) => Func::VideoPower,
        }
    }

    pub fn effect(&self) -> Effect {
        self.func().effect()
    }

    pub fn payload(&self) -> Vec<u8> {
        let switch = |on: bool| vec![u8::from(on), 0];
        match self {
            Command::LedStatus(entries) => {
                let mut bytes = vec![0u8; 16];
                for (index, entry) in entries.iter().take(2).enumerate() {
                    let slot = &mut bytes[index * 8..index * 8 + 8];
                    slot[0] = index as u8 + 1;
                    slot[1] = entry.kind;
                    slot[2] = u8::from(entry.dir);
                    slot[3] = entry.num;
                    slot[4] = entry.cycle;
                    slot[5] = entry.color[0];
                    slot[6] = entry.color[1];
                }
                bytes
            }
            Command::FillLight(pair) | Command::LidarPower(pair) | Command::VideoPower(pair) => {
                pair.to_vendor().to_le_bytes().to_vec()
            }
            Command::FanSpeed(value) | Command::GpsPower(value) => value.to_le_bytes().to_vec(),
            Command::FanSpeedQuery | Command::ExtLoadCurrentQuery => Vec::new(),
            Command::ExtLoadPower(on) | Command::HostLed(on) | Command::ExtLed(on) | Command::FpPower(on) => {
                switch(*on)
            }
        }
    }

    pub fn frame(&self, sequence: u8) -> Frame {
        Frame::host(sequence, self.func() as u8, self.payload())
    }

    /// Encode with the next number from `sequence`.
    pub fn encode(&self, sequence: &mut Sequence) -> Vec<u8> {
        self.frame(sequence.take()).encode()
    }
}
