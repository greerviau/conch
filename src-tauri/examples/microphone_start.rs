//! Measures how long a recording waits for the microphone: from the call to
//! `audio::capture` until the first audio that is not silence arrives, which
//! is when the recording indicator turns from starting to listening.
//! The first recording opens the input. `prepare` then opens it again on a new
//! `Microphone`, and the later recordings reuse that kept stream where the
//! platform keeps it. `GAP` sets the seconds between recordings (default 1);
//! a Bluetooth headset is slower after a longer gap.
//!
//! cargo run --release --example microphone_start -- ["Microphone name"]
use conch_lib::audio;
use std::sync::{Arc, atomic::AtomicU32, mpsc};
use std::time::{Duration, Instant};
fn main() {
    let wanted = std::env::args().nth(1).unwrap_or_default();
    let gap = std::env::var("GAP")
        .ok()
        .and_then(|gap| gap.parse().ok())
        .unwrap_or(1);
    let mut microphone = audio::Microphone::default();
    for round in 0..4 {
        if round == 1 {
            microphone = audio::Microphone::default();
            let started = Instant::now();
            microphone.prepare(&wanted);
            println!("prepare: {} ms", started.elapsed().as_millis());
        }
        let (stop, receive) = mpsc::channel();
        let started = Instant::now();
        let mut ready = None;
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            let _ = stop.send(false);
        });
        let result = audio::capture(
            &microphone,
            &wanted,
            receive,
            || ready = Some(started.elapsed()),
            Arc::new(AtomicU32::new(0)),
            |_| (),
        );
        stopper.join().unwrap();
        println!(
            "recording {round}: first audio {}, {}",
            ready.map_or("never".into(), |at| format!("{} ms", at.as_millis())),
            if result.is_ok() { "ok" } else { "failed" }
        );
        std::thread::sleep(Duration::from_secs(gap));
    }
}
