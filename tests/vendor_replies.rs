//! Every reply the vendor ctrlmcu_node logged receiving from the MCU (its `open-print` mode), read as the vendor reads it.

use m20_ctrlmcu::{Decoder, Frame, Func, Reply, MCU_FRAME_TYPE};

/// Reply frames in file order; `None` marks a vendor restart.
fn rows() -> Vec<Option<Vec<u8>>> {
    include_str!("fixtures/vendor_received_frames.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let mut fields = line.split('\t').skip(1);
            match fields.next().unwrap() {
                "start" => None,
                _ => Some(fields.next().unwrap().split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect()),
            }
        })
        .collect()
}

fn replies() -> Vec<Frame> {
    rows().into_iter().flatten().map(|bytes| Frame::parse(&bytes).unwrap().0).collect()
}

#[test]
fn every_logged_reply_is_a_whole_frame_with_the_mcus_type_and_a_good_crc() {
    let frames = replies();
    assert_eq!(frames.len(), 18);
    for frame in &frames {
        assert_eq!((frame.version, frame.frame_type), (0x01, MCU_FRAME_TYPE));
        assert!(frame.crc_ok());
        assert_eq!(frame.encode().len(), 10);
    }
}

#[test]
fn an_led_command_is_answered_by_an_empty_frame_echoing_its_function() {
    for frame in replies() {
        assert_eq!(frame.func, Func::LedStatus as u8);
        // an empty payload's CRC-16/MODBUS is its init value
        assert_eq!((frame.payload.len(), frame.crc), (0, 0xffff));
        assert_eq!(Reply::read(Func::LedStatus, &frame), Reply::Ack);
    }
}

#[test]
fn the_reply_sequence_counts_from_one_with_the_hosts_requests() {
    let sequences: Vec<u8> = replies().iter().map(|frame| frame.sequence).collect();
    assert_eq!(sequences, (1..=18).collect::<Vec<u8>>());
}

#[test]
fn the_decoder_reads_the_replies_back_to_back() {
    let stream: Vec<u8> = rows().into_iter().flatten().flatten().collect();
    let mut decoder = Decoder::new(256, 4096);
    decoder.push(&stream);
    let mut found = Vec::new();
    while let Some(frame) = decoder.next_frame() {
        found.push(frame);
    }
    assert_eq!(found, replies());
}
