//! Measures stop-to-text latency. A long recording compares one request after
//! the recording ends with chunks recognized while it is still running. A short
//! dictation (`<wav>` alone, then `<gap>` seconds of silence before the stop)
//! compares recognizing the audio after the stop with speculative recognition.
//! Usage: cargo run --release --example latency -- <workers dir> <model> <wav> <seconds> [gap seconds, default 0.5]
//! The voice detection model `ggml-silero-v5.1.2.bin` must sit next to `<model>`.
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
        move |wav, _| inference::transcribe(&workers, &model, wav, "", &worker, false)
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
        transcriber.push_tail(samples[range].to_vec(), None);
        chunks += 1;
    }
    let text = transcriber.finish().unwrap().text;
    println!(
        "chunked: {chunks} chunks, stop-to-text {:.2}s ({} chars)",
        stopped.elapsed().as_secs_f32(),
        text.len()
    );
    if let Ok(dir) = std::env::var("CONCH_LATENCY_DUMP") {
        std::fs::write(format!("{dir}/chunked.txt"), &text).unwrap();
    }
    let gap: f32 = args.get(5).map_or(0.5, |gap| gap.parse().unwrap());
    let mut short = clip.clone();
    short.extend(std::iter::repeat_n(0.0, (gap * rate as f32) as usize));
    let start = |recognize_worker: &inference::Worker| {
        let (workers, model, worker) = (workers.clone(), model.clone(), recognize_worker.clone());
        Transcriber::start(
            || (),
            move |wav, _| inference::transcribe(&workers, &model, wav, "", &worker, false),
        )
    };
    let transcriber = start(&worker);
    let stopped = Instant::now();
    transcriber.push_tail(short.clone(), None);
    let text = transcriber.finish().unwrap().text;
    println!(
        "short dictation, recognized after the stop: {:.2}s ({} chars)",
        stopped.elapsed().as_secs_f32(),
        text.len()
    );
    let transcriber = start(&worker);
    let mut chunker = Chunker::default();
    let mut settled_at = None;
    for end in (step..=short.len()).step_by(step) {
        if let Some(range) = chunker.settle(&short[..end]) {
            transcriber.speculate(range.clone(), short[range].to_vec());
            settled_at = Some(end);
        }
    }
    let settled_at = settled_at.expect("the gap must be at least 0.3 s so the speech settles");
    // The user keeps the key down until `gap` has passed since the speech ended.
    let waited = Duration::from_secs_f32((short.len() - settled_at) as f32 / rate as f32);
    std::thread::sleep(waited);
    let stopped = Instant::now();
    let reuse = chunker.settled(&short);
    if let Some(range) = chunker.tail(&short) {
        transcriber.push_tail(short[range].to_vec(), reuse);
    }
    let outcome = transcriber.finish().unwrap();
    println!(
        "short dictation, speculative (settled {:.2}s before the stop): {:.2}s ({} chars, reused: {})",
        waited.as_secs_f32(),
        stopped.elapsed().as_secs_f32(),
        outcome.text.len(),
        outcome.speculative
    );
    inference::shutdown(&workers);
}
