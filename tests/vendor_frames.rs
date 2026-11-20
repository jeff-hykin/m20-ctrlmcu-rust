//! Every frame the vendor ctrlmcu_node logged sending, rebuilt byte for byte.

use m20_ctrlmcu::{Command, Decoder, Frame, FrontBack, Func, LedEntry, Sequence};

struct Row {
    what: String,
    bytes: Vec<u8>,
}

/// Rows in file order; `None` marks a vendor restart.
fn rows() -> Vec<Option<Row>> {
    include_str!("fixtures/vendor_sent_frames.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t');
            let _time = fields.next().unwrap();
            let what = fields.next().unwrap().to_string();
            if what == "start" {
                return None;
            }
            let bytes = fields.next().unwrap().split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect();
            Some(Row { what, bytes })
        })
        .collect()
}

/// The LED entries a vendor LED frame carries.
fn led_entries(payload: &[u8]) -> Vec<LedEntry> {
    payload
        .chunks(8)
        .filter(|slot| slot[0] != 0)
        .map(|slot| LedEntry {
            kind: slot[1],
            dir: slot[2] != 0,
            num: slot[3],
            cycle: slot[4],
            color: [slot[5], slot[6]],
        })
        .collect()
}

#[test]
fn every_logged_frame_parses_with_a_good_crc() {
    let all = rows();
    assert_eq!(all.iter().flatten().count(), 623);
    for row in all.iter().flatten() {
        let (frame, used) = Frame::parse(&row.bytes).unwrap();
        assert_eq!(used, row.bytes.len());
        assert!(frame.crc_ok(), "{} {:?}", row.what, frame);
        assert_eq!(Func::from_u8(frame.func).unwrap().name(), row.what);
        assert_eq!((frame.version, frame.frame_type), (0x01, 0x17));
        assert_eq!(frame.encode(), row.bytes);
    }
}

#[test]
fn every_logged_frame_is_rebuilt_from_its_command() {
    for row in rows().iter().flatten() {
        let (frame, _) = Frame::parse(&row.bytes).unwrap();
        let func = Func::from_u8(frame.func).unwrap();
        let command = match func {
            Func::LedStatus => Command::LedStatus(led_entries(&frame.payload)),
            _ => {
                let value = i32::from_le_bytes(frame.payload[..4].try_into().unwrap());
                Command::from_vendor(func, value).unwrap()
            }
        };
        assert_eq!(command.frame(frame.sequence).encode(), row.bytes, "{command:?}");
    }
}

#[test]
fn the_sequence_restarts_at_one_and_counts_up_like_the_vendor() {
    let mut sequence: Option<Sequence> = None;
    let mut checked = 0;
    for row in rows() {
        match row {
            None => sequence = Some(Sequence::default()),
            Some(row) => {
                if let Some(sequence) = sequence.as_mut() {
                    assert_eq!(sequence.take(), row.bytes[2]);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 500, "{checked}");
}

#[test]
fn the_logged_power_frames_are_what_they_look_like() {
    let lidars_on = [0xee, 0xef, 0x02, 0x01, 0x17, 0x49, 0x04, 0x00, 0x00, 0x48, 0x01, 0x00, 0x01, 0x00];
    assert_eq!(Command::LidarPower(FrontBack::BOTH_ON).frame(2).encode(), lidars_on);
    assert_eq!(FrontBack::BOTH_ON.to_vendor(), 65537);
    let lidars_off = [0xee, 0xef, 0x43, 0x01, 0x17, 0x49, 0x04, 0x00, 0x00, 0x24, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(Command::LidarPower(FrontBack::default()).frame(0x43).encode(), lidars_off);
    let cameras_off = [0xee, 0xef, 0x44, 0x01, 0x17, 0x51, 0x04, 0x00, 0x00, 0x24, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(Command::VideoPower(FrontBack::default()).frame(0x44).encode(), cameras_off);
}

#[test]
fn the_decoder_finds_frames_in_noise_and_split_reads() {
    let mut stream = vec![0x00, 0xee, 0x13, 0xef];
    let frames: Vec<Vec<u8>> = rows().into_iter().flatten().take(40).map(|row| row.bytes).collect();
    for (index, frame) in frames.iter().enumerate() {
        stream.extend_from_slice(frame);
        if index % 3 == 0 {
            stream.extend_from_slice(&[0xee, 0x55]);
        }
    }
    let mut decoder = Decoder::new(256, 4096);
    let mut found = Vec::new();
    for chunk in stream.chunks(5) {
        decoder.push(chunk);
        while let Some(frame) = decoder.next_frame() {
            found.push(frame.encode());
        }
    }
    assert_eq!(found, frames);
}

#[test]
fn an_absurd_length_is_taken_as_a_false_header() {
    let good = rows().into_iter().flatten().next().unwrap().bytes;
    let mut stream = vec![0xee, 0xef, 0, 1, 0x17, 0x40, 0xff, 0xff, 0, 0];
    stream.extend_from_slice(&good);
    let mut decoder = Decoder::new(256, 4096);
    decoder.push(&stream);
    assert_eq!(decoder.next_frame().unwrap().encode(), good);
    assert_eq!(decoder.stats().skipped_bytes, 10);
}
