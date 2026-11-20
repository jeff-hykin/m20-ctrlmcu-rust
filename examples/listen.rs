//! Open the MCU's UART read-only (nothing can be written) and print every frame and any stray bytes, for a while.
//!
//! `listen [device] [seconds]`, defaults `/dev/ttyS3` and 60.

#[cfg(unix)]
fn main() -> std::io::Result<()> {
    use std::io::Read;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use m20_ctrlmcu::serial::{Access, SerialPort, VENDOR_BAUD, VENDOR_DEVICE};
    use m20_ctrlmcu::{hex, Decoder, Func};

    let mut args = std::env::args().skip(1);
    let device = args.next().unwrap_or_else(|| VENDOR_DEVICE.to_string());
    let seconds: f64 = args.next().map(|s| s.parse().expect("seconds")).unwrap_or(60.0);
    let mut port = SerialPort::open(&device, VENDOR_BAUD, Access::ReadOnly)?;
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    eprintln!("listening on {device} (read-only) for {seconds} s");
    // The read blocks until bytes arrive, so a quiet port is watched from another thread and the process exits.
    std::thread::spawn(move || {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        eprintln!("done: nothing more within {seconds} s");
        std::process::exit(0);
    });
    let mut decoder = Decoder::new(256, 4096);
    let mut chunk = [0u8; 256];
    let mut total = 0usize;
    loop {
        let count = port.read(&mut chunk)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
        total += count;
        println!("{now:.6} raw {count} bytes (total {total}): {}", hex(&chunk[..count]));
        decoder.push(&chunk[..count]);
        while let Some(frame) = decoder.next_frame() {
            let name = Func::from_u8(frame.func).map(|f| f.name()).unwrap_or("unknown");
            println!("{now:.6} frame {name}: {frame:?}");
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("unix only");
}
