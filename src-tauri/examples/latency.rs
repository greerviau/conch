//! Measures stop-to-text latency for a long recording: one request after the
//! recording ends versus chunks recognized while it is still running.
//! Usage: cargo run --release --example latency -- <workers dir> <model> <wav> <seconds>
use conch_lib::{chunk::Chunker, inference, pipeline::Transcriber};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (workers, model) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let clip: Vec<f32> = hound::WavReader::open(&args[3])
        .unwrap()
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();
    let seconds: usize = args[4].parse().unwrap();
    let rate = 16_000;
    let mut samples = vec![];
    while samples.len() < seconds * rate {
        samples.extend_from_slice(&clip);
        samples.extend(std::iter::repeat_n(0.0, rate));
    }
    samples.truncate(seconds * rate);
    let worker = inference::new_worker();
    let started = Instant::now();
    inference::preload(&workers, &model, inference::Kind::Whisper, &worker, false).unwrap();
    println!("worker start: {:.2}s", started.elapsed().as_secs_f32());

    let wav = conch_lib::audio::wav(&samples).unwrap();
    let started = Instant::now();
    let whole = inference::transcribe(&workers, &model, wav, "", &worker, false).unwrap();
    if let Ok(dir) = std::env::var("CONCH_LATENCY_DUMP") {
        std::fs::write(format!("{dir}/whole.txt"), &whole).unwrap();
    }
    println!(
        "single request after stop: {:.2}s ({} chars)",
        started.elapsed().as_secs_f32(),
        whole.len()
    );

    let transcriber = Transcriber::start(|| (), {
        let (workers, model, worker) = (workers.clone(), model.clone(), worker.clone());
        move |wav| inference::transcribe(&workers, &model, wav, "", &worker, false)
    });
    let mut chunker = Chunker::default();
    let step = rate / 10;
    let mut chunks = 0;
    for end in (step..=samples.len()).step_by(step) {
        if let Some(range) = chunker.poll(&samples[..end]) {
            transcriber.push(samples[range].to_vec());
            chunks += 1;
        }
        std::thread::sleep(Duration::from_millis(25)); // 4x real time
    }
    let stopped = Instant::now();
    if let Some(range) = chunker.tail(&samples) {
        transcriber.push(samples[range].to_vec());
        chunks += 1;
    }
    let text = transcriber.finish().unwrap();
    println!(
        "chunked: {chunks} chunks, stop-to-text {:.2}s ({} chars)",
        stopped.elapsed().as_secs_f32(),
        text.len()
    );
    if let Ok(dir) = std::env::var("CONCH_LATENCY_DUMP") {
        std::fs::write(format!("{dir}/chunked.txt"), &text).unwrap();
    }
    inference::shutdown(&workers);
}
