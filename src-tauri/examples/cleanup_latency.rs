//! Measures the wait for Enhanced cleanup after the stop. Each `<name>.wav` in
//! `<clips dir>` (with `<name>.txt` holding the words spoken) is fed to the
//! chunker at real time, with 0.6 s of quiet before the stop. Chunks are
//! recognized and cleaned while the recording runs, as the app does. After the
//! stop it also cleans the whole raw text once, which is what the app did
//! before cleaning ran per chunk, and compares the two texts.
//! Pass `after` as the last argument to measure only that earlier flow: nothing is
//! cleaned until the stop, and the time covers recognition plus one cleanup of the
//! whole text. The comparison inside the default run reuses the llama-server's
//! prompt cache from the per-chunk cleaning of the same text, so its whole-text
//! time is lower than the real one; use the `after` run for that.
//! Usage: cargo run --release --example cleanup_latency -- <workers dir> <data dir> <recognition model> <clips dir> [speed, default 1] [after]
//! `<data dir>` holds the managed cleanup model, as the application data directory does.
use conch_lib::{
    chunk::Chunker,
    core::Snapshot,
    inference::{self, CleanupRequest},
    pipeline::Transcriber,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}
fn edit_distance(a: &[String], b: &[String]) -> usize {
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            current.push(
                (previous[j + 1] + 1)
                    .min(current[j] + 1)
                    .min(previous[j] + usize::from(x != y)),
            );
        }
        previous = current;
    }
    previous[b.len()]
}
fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let workers = PathBuf::from(&args[1]);
    let (data, model) = (PathBuf::from(&args[2]), PathBuf::from(&args[3]));
    let speed: f32 = args.get(5).map_or(1.0, |speed| speed.parse().unwrap());
    let rate = 16_000usize;
    let mut snapshot = Snapshot::default();
    snapshot.cleanup.enhanced = "managed".into();
    let worker = inference::new_worker();
    inference::preload(&workers, &model, inference::Kind::Whisper, &worker, false).unwrap();
    inference::preload(
        &workers,
        &data.join(inference::CLEANUP.name),
        inference::Kind::Llama,
        &worker,
        false,
    )
    .unwrap();
    let after = args.get(6).is_some_and(|mode| mode == "after");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&args[4])
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
        .collect();
    paths.sort();
    if after {
        let mut totals = vec![];
        for path in &paths {
            let mut samples: Vec<f32> = hound::WavReader::open(path)
                .unwrap()
                .samples::<i16>()
                .map(|s| s.unwrap() as f32 / i16::MAX as f32)
                .collect();
            samples.extend(std::iter::repeat_n(0.0, rate * 6 / 10));
            let transcriber = Transcriber::start(|| (), {
                let (workers, model, worker) = (workers.clone(), model.clone(), worker.clone());
                move |wav, _| inference::transcribe(&workers, &model, wav, "", &worker, false)
            });
            let mut chunker = Chunker::default();
            let (mut chunks, step) = (0, rate / 10);
            for end in (step..=samples.len()).step_by(step) {
                let heard = &samples[..end];
                if let Some(range) = chunker.poll(heard) {
                    transcriber.push(heard[range].to_vec());
                    chunks += 1;
                } else if let Some(range) = chunker.settle(heard) {
                    transcriber.speculate(range.clone(), heard[range].to_vec());
                }
                std::thread::sleep(Duration::from_secs_f32(0.1 / speed));
            }
            let stopped = Instant::now();
            let reuse = chunker.settled(&samples);
            if let Some(range) = chunker.tail(&samples) {
                transcriber.push_tail(samples[range].to_vec(), reuse);
            }
            let done = transcriber.finish().unwrap();
            let recognized = stopped.elapsed();
            let cleaned = inference::enhanced(CleanupRequest {
                workers: &workers,
                data: &data,
                snapshot: &snapshot,
                raw: &done.text,
                worker: &worker,
                cpu: false,
            });
            totals.push(stopped.elapsed().as_secs_f32());
            println!(
                "{:20} {chunks} chunks | cleaned after the stop: {:4.0} ms (recognition {:4.0}, cleanup {:4.0}) ok: {}",
                path.file_name().unwrap().to_string_lossy(),
                1000.0 * stopped.elapsed().as_secs_f32(),
                1000.0 * recognized.as_secs_f32(),
                1000.0 * (stopped.elapsed() - recognized).as_secs_f32(),
                cleaned.is_ok()
            );
        }
        println!(
            "{} dictations at {speed}x. After the stop, median {:.0} ms with one cleanup of the whole text",
            paths.len(),
            1000.0 * median(&mut totals)
        );
        inference::shutdown(&workers);
        return;
    }
    let (mut during, mut whole, mut recognition_wait, mut tail_clean) =
        (vec![], vec![], vec![], vec![]);
    let (mut same, mut compared, mut errors_chunked, mut errors_whole, mut spoken) =
        (0, 0, 0, 0, 0);
    for path in &paths {
        let mut samples: Vec<f32> = hound::WavReader::open(path)
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        let reference = std::fs::read_to_string(path.with_extension("txt")).unwrap();
        samples.extend(std::iter::repeat_n(0.0, rate * 6 / 10));
        let transcriber = Transcriber::start_cleaning(
            || (),
            {
                let (workers, model, worker) = (workers.clone(), model.clone(), worker.clone());
                move |wav, _| inference::transcribe(&workers, &model, wav, "", &worker, false)
            },
            {
                let (workers, data, snapshot, worker) = (
                    workers.clone(),
                    data.clone(),
                    snapshot.clone(),
                    worker.clone(),
                );
                move |part| {
                    inference::enhanced_part(CleanupRequest {
                        workers: &workers,
                        data: &data,
                        snapshot: &snapshot,
                        raw: part,
                        worker: &worker,
                        cpu: false,
                    })
                }
            },
        );
        let mut chunker = Chunker::default();
        let (mut chunks, step) = (0, rate / 10);
        for end in (step..=samples.len()).step_by(step) {
            let heard = &samples[..end];
            if let Some(range) = chunker.poll(heard) {
                transcriber.push(heard[range].to_vec());
                chunks += 1;
            } else if let Some(range) = chunker.settle(heard) {
                transcriber.speculate(range.clone(), heard[range].to_vec());
            }
            std::thread::sleep(Duration::from_secs_f32(0.1 / speed));
        }
        let stopped = Instant::now();
        let reuse = chunker.settled(&samples);
        if let Some(range) = chunker.tail(&samples) {
            transcriber.push_tail(samples[range].to_vec(), reuse);
        }
        let done = transcriber.finish().unwrap();
        let (waited, recognized) = (stopped.elapsed(), done.recognized_at - stopped);
        let chunked = done.cleaned.clone().unwrap();
        // What the app did before: one cleanup of the whole text after the stop.
        let started = Instant::now();
        let single = inference::enhanced(CleanupRequest {
            workers: &workers,
            data: &data,
            snapshot: &snapshot,
            raw: &done.text,
            worker: &worker,
            cpu: false,
        });
        let single_took = started.elapsed();
        during.push(waited.as_secs_f32());
        recognition_wait.push(recognized.as_secs_f32());
        tail_clean.push((waited - recognized).as_secs_f32());
        whole.push(single_took.as_secs_f32());
        let name = path.file_name().unwrap().to_string_lossy();
        println!(
            "{name:20} {chunks} chunks | per chunk: {:4.0} ms after the stop (recognition {:4.0}, tail cleanup {:4.0}) | whole text after the stop: {:4.0} ms",
            1000.0 * waited.as_secs_f32(),
            1000.0 * recognized.as_secs_f32(),
            1000.0 * (waited - recognized).as_secs_f32(),
            1000.0 * single_took.as_secs_f32()
        );
        if let (Ok(chunked), Ok(single)) = (chunked, single) {
            compared += 1;
            let reference = words(&reference);
            errors_chunked += edit_distance(&reference, &words(&chunked));
            errors_whole += edit_distance(&reference, &words(&single));
            spoken += reference.len();
            if chunked == single {
                same += 1;
            } else {
                println!("    per chunk: {chunked}\n    whole:     {single}");
            }
        } else {
            println!("    a cleanup failed; skipped in the comparison");
        }
    }
    println!(
        "{} dictations at {speed}x. After the stop, median: {:.0} ms with per-chunk cleaning (recognition {:.0} ms + tail cleanup {:.0} ms); recognition plus a whole-text cleanup {:.0} ms",
        paths.len(),
        1000.0 * median(&mut during),
        1000.0 * median(&mut recognition_wait),
        1000.0 * median(&mut tail_clean),
        1000.0 * (median(&mut recognition_wait) + median(&mut whole)),
    );
    println!(
        "cleaned texts identical in {same} of {compared}; word error rate against the spoken words: per chunk {:.1}%, whole text {:.1}%",
        100.0 * errors_chunked as f32 / spoken.max(1) as f32,
        100.0 * errors_whole as f32 / spoken.max(1) as f32
    );
    inference::shutdown(&workers);
}
