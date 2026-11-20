//! Frames and decoder settings at the edges of what the 16-bit length and usize can hold.

use m20_ctrlmcu::{Decoder, Frame, PayloadTooLong};

#[test]
fn an_oversized_payload_is_refused_or_cut_not_mislabelled() {
    let frame = Frame::host(1, 0x40, vec![7; 70_000]);
    assert_eq!(frame.try_encode(), Err(PayloadTooLong(70_000)));
    let bytes = frame.encode();
    assert_eq!(bytes[6..8], [0xff, 0xff]);
    assert_eq!(bytes.len(), 10 + 65_535);
    let small = Frame::host(1, 0x40, vec![7; 4]);
    assert_eq!(small.try_encode().unwrap(), small.encode());
}

#[test]
fn a_huge_max_payload_does_not_overflow() {
    let mut decoder = Decoder::new(usize::MAX, 0);
    decoder.push(&Frame::host(3, 0x43, vec![]).encode());
    assert_eq!(decoder.next_frame().map(|frame| frame.sequence), Some(3));
}
