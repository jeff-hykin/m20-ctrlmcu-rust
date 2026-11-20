# M20 body control MCU (ctrlmcu) protocol

Source: the vendor `ctrlmcu_node` (node_driver v1.1.11, `/opt/robot/share/node_driver/bin/ctrlmcu_node`, sha1
`14a841ed0c06ca7805e29cba2fcaf92de37cf3d2`, aarch64, not stripped), disassembled; its client `basic_server`
(sha1 `75dd214c…`); and every frame `ctrlmcu_node` logged sending on one robot from 2026-11-04 to 11-20
(`tests/fixtures/vendor_sent_frames.tsv`). **C** = confirmed from the code or the logs, **G** = guessed.

## Link

- C: `/dev/ttyS3` on the AOS board, opened `O_RDWR | O_NOCTTY | O_SYNC`, then a 1 s sleep and `tcflush(TCIOFLUSH)`.
- C: 115200 baud, 8 data bits, no parity, 1 stop bit, no RTS/CTS, raw (`c_iflag`, `c_oflag`, `c_lflag` cleared),
  `CLOCAL | CREAD`, `VMIN 0`, `VTIME 10` (reads return after 1 s of silence).

## Frame

```
offset 0  1  2    3     4     5     6..7        8..9          10..
       EE EF seq  0x01  type  func  length LE   CRC-16 LE     payload (length bytes)
```

- C: CRC-16/MODBUS (reflected poly `0xA001`, init `0xFFFF`, no final xor) over the payload only, sent low byte first.
  An empty payload carries `FF FF`. All 623 logged frames check.
- C: host frames: byte 3 = `0x01`, type = `0x17`.
- C: sequence: a per-process counter starting at 1, +1 per frame, 255 wraps to 1 (0 is skipped). Every logged run
  starts at 1 and counts without a gap.
- C: no tail or terminator.
- C: the MCU's replies use the same layout, with type `0x21` (the vendor finds `EE EF`, reads length from bytes 6-7
  little-endian and takes the payload from byte 10; it ignores the reply's sequence, type, function code and CRC).
  Captured with the vendor's `open-print` on 2026-11-21 (tests/fixtures/vendor_received_frames.tsv): 18 replies to
  `/LED/STATUS`, each `EE EF seq 01 21 40 00 00 FF FF`: the request's function code echoed, an empty payload (CRC
  `FF FF`), and a sequence counting 1, 2, ... like the host's own from its start (G: the request's sequence echoed).
  No fan-speed or external-load-current reply was captured: nothing called those services in that window.
- C: an unused vendor `parse_frame` reads the length big-endian; the path that is actually used reads it
  little-endian.

## Commands (host to MCU)

Every command but the LEDs arrives at the vendor as a `drdds::srv::StdSrvInt32` service call (`int32 command` in,
`int32 result` out) on the topic shown; LEDs arrive as a `drdds::msg::LedStatusLight` message.

| func | vendor topic | payload (from the int32 `v`) | vendor reply wait | what (vendor's Chinese label) |
|---|---|---|---|---|
| `0x40` | `/LED/STATUS` (subscription) | 16 bytes: 2 × 8-byte LED entries | 1000 ms | colour LEDs (彩灯控制) |
| `0x41` | `/LED/FILL` | `v` as i32 LE | 2000 ms | fill lights (补光灯控制) |
| `0x42` | `/FAN/SPEED` | low 16 bits of `v`, LE | 2000 ms | fan setpoint (风扇控制) |
| `0x43` | `/FAN/STATUS` | empty | 2000 ms | fan speed query |
| `0x44` | `/EXT_LOAD/CUR` | empty | 1000 ms | external load current query (外部负载电流查询) |
| `0x45` | `/EXT_LOAD/POWER` | `[v != 0, 0]` | 1000 ms | external load power (外部负载电压开关) |
| `0x46` | `/LED/HOST/POWER` | `[v != 0, 0]` | 1000 ms | main indicator LED (主指示灯开关) |
| `0x47` | `/LED/EXT/POWER` | `[v != 0, 0]` | 1000 ms | external LED (外部LED开关) |
| `0x48` | `/FP/POWER` | `[v != 0, 0]` | 1000 ms | image-link / pairing radio (图传模块开关, 对频模块) |
| `0x49` | `/LIDAR/POWER` | `v` as i32 LE | 1000 ms | front + rear lidar power (头尾雷达使能) |
| `0x50` | `/GPS/POWER` | low 16 bits of `v`, LE | 1000 ms | GPS power |
| `0x51` | `/VIDEO/POWER` | `v` as i32 LE | 1000 ms | front + rear wide-angle camera power (广角相机开关) |

- C: front/back pairs (lidar, video): basic_server packs `v = back | front << 16`, so the wire bytes are
  `back_lo back_hi front_lo front_hi`; `1/1` = `0x00010001` = `01 00 01 00`. G: fill lights pack the same way.
- G: fan setpoint is a percentage (basic_server reports a default of 100; the vendor self-test sends 25).
- C: LED entry `i` (0 or 1) of the message's `data` vector becomes
  `[i + 1, type, dir, num, cycle, color[0], color[1], 0]`; at most two entries and two colour bytes are used, a
  missing entry is all zeros. G: field meanings (type looks like a pattern, color like a colour index).
- Observed LED frames (most common first): entry 1 `type 0, cycle 1, color 3`, entry 2 off; both `cycle 1, color 1`;
  entry 1 `cycle 1 color 1`, entry 2 off; both `type 1 num 3 cycle 1 color 2`; others in the fixture.

## Replies (MCU to host)

- C: the vendor holds one mutex around each exchange: write, sleep 10 ms, read in ≤128-byte chunks until a whole
  frame has arrived or the wait above runs out. The first whole frame is the reply.
- C: the MCU does reply: every logged request got a frame back (logged results 0, about 10 ms after the write,
  which is the vendor's own 10 ms sleep).
- C: the service `result` is the vendor's `SerialError`: 0 = a frame came back, 3 = timeout; G: 1 = write failed,
  2 = port not open, 4 = read error.
- C: fan speed query reply payload is 12 bytes, three u32 LE: tail, mid, head. G: unit rpm.
- C: external load current reply payload is 4 bytes, one u32 LE. G: unit. The vendor never reports it: it hands
  this reply to its fan-speed parser (a copy-paste bug), which rejects anything that is not 12 bytes.
- C: neither reading is returned over DDS; the vendor only logs them, and `result` stays the `SerialError`.
- C: the vendor also knows a reply code `0x32` (MCU version) but never sends a request for it.

## Behaviour

- C: startup: refuse to run if any non-loopback IPv4 address ends in `.104` or `.106` (meant for the GOS/NOS
  boards; also trips on AOS's tailscale address 100.106.168.106), start the logger, open the port, create the DDS
  participant, subscribe `/LED/STATUS`, serve the 11 services, then sleep in 100 ms steps until SIGINT/SIGTERM.
- C: nothing is sent at startup, there is no reader thread, no polling and no heartbeat: every frame is a reply to
  a DDS event. Power and fan states are whatever the MCU holds.
- C: the MCU keeps its outputs without the host: on 2026-11-20 both lidars streamed at 9.8 Hz while `ctrlmcu_node`
  was crash-looping (18:17-21:19 CST).
- C: `ctrlmcu_node test-serial` instead cycles every command above once, 6 s apart (self-test).

## Who drives it

- C: only basic_server (its `DeviceControl`) talks to these topics.
  - Setting the usage mode (navigation) sends `/LIDAR/POWER 0x00010001` (both on); never off.
  - Sleep: `/LED/FILL 0`, `/LIDAR/POWER 0`, `/VIDEO/POWER 0` (and joint power, not through this MCU); wake: the
    saved lidar and video values back. Seen once in the logs (2026-11-04 02:32:58: fill 0, lidars 0, cameras 0).
  - The app's SetDevice JSON can set every switch and the fan; it refuses to turn the lidars off in navigation mode.
  - Its LED thread publishes `/LED/STATUS` on changes of motion state, errors, sleep, usage mode and gait.
  - Its reported defaults: fan 100, load power 1, host LED 1, external LED 1, FP 1, lidar 1/1, GPS 1, video 1/1,
    fill 0/0.
