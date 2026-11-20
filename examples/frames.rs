//! Print the bytes each command would put on the wire, without opening anything.

use m20_ctrlmcu::{hex, Command, FrontBack, LedEntry, Sequence};

fn main() {
    let mut sequence = Sequence::default();
    let commands = [
        Command::FanSpeedQuery,
        Command::ExtLoadCurrentQuery,
        Command::LedStatus(vec![LedEntry { cycle: 1, color: [1, 0], ..LedEntry::default() }; 2]),
        Command::HostLed(true),
        Command::ExtLed(true),
        Command::FillLight(FrontBack::default()),
        Command::FanSpeed(100),
        Command::LidarPower(FrontBack::BOTH_ON),
        Command::VideoPower(FrontBack::BOTH_ON),
        Command::ExtLoadPower(true),
        Command::FpPower(true),
        Command::GpsPower(1),
    ];
    for command in commands {
        let func = command.func();
        println!("{:<24} {:?}  {}", func.name(), command.effect(), hex(&command.encode(&mut sequence)));
    }
}
