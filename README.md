# m20-ctrlmcu

The Deep Robotics M20 (Lynx) body control MCU in Rust: the serial protocol the vendor `ctrlmcu_node` speaks on the AOS
board's `/dev/ttyS3` (115200 8N1) for the status LEDs, fill lights, fans, fan-speed and external-load current
readings, and the lidar, camera, external-load, FP-radio and GPS power switches. Reverse-engineered from the vendor
binary and its logs; see [PROTOCOL.md](PROTOCOL.md).

- `frame`: `EE EF | seq | 01 | 17 | func | len LE | CRC-16/MODBUS LE | payload`, a stream decoder, the vendor's
  sequence counter.
- `command`: one function code per vendor DDS topic, payloads built from that topic's `int32` exactly as the vendor
  builds them (`Command::from_vendor`).
- `client`: request/reply over a writer and a reader thread; a `Policy` refuses lights, fans or power before a byte
  is written (only queries are allowed by default).
- `serial` (unix): the UART, read-only or read-write, and a reader that can be stopped while it waits.

```rust
use m20_ctrlmcu::{Command, FrontBack, Sequence};

let mut sequence = Sequence::default();
let bytes = Command::LidarPower(FrontBack::BOTH_ON).encode(&mut sequence);
assert_eq!(bytes, [0xee, 0xef, 0x01, 0x01, 0x17, 0x49, 0x04, 0x00, 0x00, 0x48, 0x01, 0x00, 0x01, 0x00]);
```

Examples: `listen` (opens the port read-only and prints every frame), `frames` (prints what each command would
send, without opening anything).

License: Apache-2.0
