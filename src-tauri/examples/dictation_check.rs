//! Drives recorded dictations through the chunker and the transcriber the way
//! the recording flow does, and reports dictations whose text lost words.
//! Each `<name>.wav` in `<clips dir>` needs `<name>.txt` with the words spoken.
//! A random 0.3 to 1.2 s of quiet follows each clip, as between the last word
//! and the key release. Set `CONCH_NO_SPECULATE=1` to skip speculative recognition.
//! Usage: cargo run --release --example dictation_check -- <workers dir> <model> <clips dir> [rounds, default 1]
use conch_lib::{chunk::Chunker, inference, pipeline::Transcriber};
use std::{path::PathBuf, time::Duration};

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
struct Noise(u64);
impl Noise {
    /// Uniform in [-1, 1).
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32 / (1u64 << 23) as f32) - 1.0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (workers, model) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let rounds: usize = args.get(4).map_or(1, |rounds| rounds.parse().unwrap());
    let speculate = std::env::var("CONCH_NO_SPECULATE").is_err();
    let rate = 16_000usize;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&args[3])
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
        .collect();
    paths.sort();
    let worker = inference::new_worker();
    inference::preload(&workers, &model, inference::Kind::Whisper, &worker, false).unwrap();
    let mut noise = Noise(7);
    let (mut truncated, mut total, mut errors, mut spoken) = (0, 0, 0, 0);
    for round in 0..rounds {
        for path in &paths {
            let mut samples: Vec<f32> = hound::WavReader::open(path)
                .unwrap()
                .samples::<i16>()
                .map(|s| s.unwrap() as f32 / i16::MAX as f32)
                .collect();
            let reference = words(&std::fs::read_to_string(path.with_extension("txt")).unwrap());
            let quiet = (rate as f32 * (0.3 + 0.9 * (noise.next() + 1.0) / 2.0)) as usize;
            samples.extend(std::iter::repeat_n(0.0, quiet));
            for sample in samples.iter_mut() {
                *sample += 0.002 * noise.next();
            }
            let transcriber = Transcriber::start(|| (), {
                let (workers, model, worker) = (workers.clone(), model.clone(), worker.clone());
                move |wav, _| inference::transcribe(&workers, &model, wav, "", "en", &worker, false)
            });
            let mut chunker = Chunker::default();
            let step = rate / 10;
            for end in (step..=samples.len()).step_by(step) {
                let heard = &samples[..end];
                if let Some(range) = chunker.poll(heard) {
                    transcriber.push(heard[range].to_vec());
                } else if speculate && let Some(range) = chunker.settle(heard) {
                    transcriber.speculate(range.clone(), heard[range].to_vec());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let reuse = if speculate {
                chunker.settled(&samples)
            } else {
                None
            };
            if let Some(range) = chunker.tail(&samples) {
                transcriber.push_tail(samples[range].to_vec(), reuse);
            }
            let outcome = transcriber.finish().unwrap();
            let got = words(&outcome.text);
            let distance = edit_distance(&reference, &got);
            total += 1;
            errors += distance;
            spoken += reference.len();
            if (got.len() as f32) < 0.85 * reference.len() as f32 {
                truncated += 1;
                println!(
                    "TRUNCATED round {round} {}: {} of {} words, reused speculation: {}\n    got: {}",
                    path.file_name().unwrap().to_string_lossy(),
                    got.len(),
                    reference.len(),
                    outcome.speculative,
                    outcome.text
                );
            }
        }
    }
    println!(
        "{total} dictations, {truncated} truncated, WER {:.1}%, speculation {}",
        100.0 * errors as f32 / spoken as f32,
        if speculate { "on" } else { "off" }
    );
    inference::shutdown(&workers);
}
