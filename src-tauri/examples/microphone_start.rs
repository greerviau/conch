//! Measures how long a recording waits for the microphone: from the call to
//! `audio::capture` until it is ready, and until the first audio arrives.
//! The first recording opens the input. `prepare` then opens it again on a new
//! `Microphone`, and the later recordings reuse that kept stream where the
//! platform keeps it.
//!
//! cargo run --release --example microphone_start -- ["Microphone name"]
use conch_lib::audio;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
fn main() {
    let wanted = std::env::args().nth(1).unwrap_or_default();
    let mut microphone = audio::Microphone::default();
    for round in 0..4 {
        if round == 1 {
            microphone = audio::Microphone::default();
            let started = Instant::now();
            microphone.prepare(&wanted);
            println!("prepare: {} ms", started.elapsed().as_millis());
        }
        let level = Arc::new(AtomicU32::new(0));
        let (stop, receive) = mpsc::channel();
        let started = Instant::now();
        let watched = level.clone();
        let first_audio = std::thread::spawn(move || {
            while watched.load(Ordering::Relaxed) == 0 && started.elapsed() < Duration::from_secs(5)
            {
                std::thread::sleep(Duration::from_millis(1));
            }
            started.elapsed()
        });
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
            level,
            |_| (),
        );
        stopper.join().unwrap();
        println!(
            "recording {round}: ready {:?} ms, first audio {} ms, {}",
            ready.map(|at| at.as_millis()),
            first_audio.join().unwrap().as_millis(),
            if result.is_ok() { "ok" } else { "failed" }
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}
