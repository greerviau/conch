//! Compares the flow of the application with the one it replaced on the same
//! dictations, to look for cases where it is slower or its text is worse.
//!
//! Flow `before`: chunks are recognized while the recording runs, nothing is
//! recognized speculatively, and the whole text is cleaned after the stop.
//! Flow `now`: speculative recognition, each chunk and the tail cleaned while the
//! recording continues, and the whole text cleaned after the stop only when a
//! protected span crosses chunks.
//!
//! Each `<name>.wav` in `<clips dir>` (with `<name>.txt` holding the words
//! spoken) is fed to the chunker at `speed` times real time and followed by each
//! gap of quiet before the stop, once per flow, in alternating order.
//! Usage: cargo run --release --example regression_check -- <workers dir> <data dir> <recognition model> <clips dir> <speed> <gaps in seconds, comma separated> [every nth clip, default 1]
use conch_lib::{
    chunk::Chunker,
    core::{self, Snapshot},
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
fn percentile(values: &[f32], at: f32) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted
        .get(((sorted.len() as f32 - 1.0) * at).round() as usize)
        .copied()
        .unwrap_or(0.0)
}

struct Outcome {
    /// Seconds from the stop to the recognized text.
    to_text: f32,
    /// Seconds from the stop to the cleaned text.
    to_cleaned: f32,
    raw: String,
    cleaned: Result<String, String>,
}

struct Setup {
    workers: PathBuf,
    data: PathBuf,
    model: PathBuf,
    snapshot: Snapshot,
    worker: inference::Worker,
    speed: f32,
    /// Off when `CONCH_NO_SPECULATE` is set, to measure the rest of the new flow.
    speculate: bool,
}
impl Setup {
    fn recognize(
        &self,
        trace: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
        origin: Instant,
    ) -> impl Fn(Vec<u8>, bool) -> Result<String, String> + Send + 'static + use<> {
        let (workers, model, worker) = (
            self.workers.clone(),
            self.model.clone(),
            self.worker.clone(),
        );
        move |wav, speculative| {
            let started = origin.elapsed();
            let seconds = wav.len() / 32_000;
            let text = inference::transcribe(&workers, &model, wav, "", "en", &worker, false);
            if let Some(trace) = &trace {
                trace.lock().unwrap().push(format!(
                    "{} of {seconds} s audio: {:.2} s to {:.2} s",
                    if speculative {
                        "speculation"
                    } else {
                        "request"
                    },
                    started.as_secs_f32(),
                    origin.elapsed().as_secs_f32()
                ));
            }
            text
        }
    }
    fn clean_whole(&self, raw: &str) -> Result<String, String> {
        inference::enhanced(CleanupRequest {
            workers: &self.workers,
            data: &self.data,
            snapshot: &self.snapshot,
            raw,
            worker: &self.worker,
            cpu: false,
        })
    }
    fn run(&self, samples: &[f32], now: bool) -> Outcome {
        let rate = 16_000usize;
        let step = rate / 10;
        let origin = Instant::now();
        let trace = std::env::var("CONCH_TRACE")
            .is_ok()
            .then(|| std::sync::Arc::new(std::sync::Mutex::new(vec![])));
        let transcriber = if now {
            let (workers, data, snapshot, worker) = (
                self.workers.clone(),
                self.data.clone(),
                self.snapshot.clone(),
                self.worker.clone(),
            );
            Transcriber::start_cleaning(
                || (),
                self.recognize(trace.clone(), origin),
                move |part| {
                    inference::enhanced_part(CleanupRequest {
                        workers: &workers,
                        data: &data,
                        snapshot: &snapshot,
                        raw: part,
                        worker: &worker,
                        cpu: false,
                    })
                },
            )
        } else {
            Transcriber::start(|| (), self.recognize(trace.clone(), origin))
        };
        let mut chunker = Chunker::default();
        for end in (step..=samples.len()).step_by(step) {
            let heard = &samples[..end];
            if let Some(range) = chunker.poll(heard) {
                transcriber.push(heard[range].to_vec());
            } else if now
                && self.speculate
                && let Some(range) = chunker.settle(heard)
            {
                transcriber.speculate(range.clone(), heard[range].to_vec());
            }
            std::thread::sleep(Duration::from_secs_f32(0.1 / self.speed));
        }
        let stopped = Instant::now();
        if trace.is_some() {
            println!(
                "  [{}] stop at {:.2} s",
                if now { "now" } else { "before" },
                origin.elapsed().as_secs_f32()
            );
        }
        let reuse = if now && self.speculate {
            chunker.settled(samples)
        } else {
            None
        };
        if let Some(range) = chunker.tail(samples) {
            transcriber.push_tail(samples[range].to_vec(), reuse);
        }
        let done = transcriber.finish().unwrap();
        if let Some(trace) = &trace {
            for line in trace.lock().unwrap().iter() {
                println!("    {line}");
            }
        }
        let to_text = (done.recognized_at - stopped).as_secs_f32();
        let during = done
            .cleaned
            .filter(|_| core::parts_mask_like_whole(&done.parts, &self.snapshot));
        let cleaned = during.unwrap_or_else(|| self.clean_whole(&done.text));
        Outcome {
            to_text,
            to_cleaned: stopped.elapsed().as_secs_f32(),
            raw: done.text,
            cleaned,
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let snapshot = Snapshot {
        enhanced: "managed".into(),
        ..Default::default()
    };
    let setup = Setup {
        workers: PathBuf::from(&args[1]),
        data: PathBuf::from(&args[2]),
        model: PathBuf::from(&args[3]),
        snapshot,
        worker: inference::new_worker(),
        speed: args[5].parse().unwrap(),
        speculate: std::env::var("CONCH_NO_SPECULATE").is_err(),
    };
    let gaps: Vec<f32> = args[6].split(',').map(|gap| gap.parse().unwrap()).collect();
    let every: usize = args.get(7).map_or(1, |n| n.parse().unwrap());
    inference::preload(
        &setup.workers,
        &setup.model,
        inference::Kind::Whisper,
        &setup.worker,
        false,
    )
    .unwrap();
    inference::preload(
        &setup.workers,
        &setup.data.join(inference::CLEANUP.name),
        inference::Kind::Llama,
        &setup.worker,
        false,
    )
    .unwrap();
    inference::warm_cleanup(
        &setup.workers,
        &setup.data.join(inference::CLEANUP.name),
        &setup.worker,
    );
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&args[4])
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
        .collect();
    paths.sort();
    let paths: Vec<_> = paths.into_iter().step_by(every).collect();
    let rate = 16_000usize;
    // Per run: (clip, gap, before, now)
    let mut diff_cleaned = vec![];
    let (mut before_cleaned, mut now_cleaned) = (vec![], vec![]);
    let (mut before_text, mut now_text) = (vec![], vec![]);
    let (mut same_cleaned, mut compared, mut runs) = (0, 0, 0);
    let (mut errors_before, mut errors_now, mut spoken) = (0, 0, 0);
    let (mut lost_before, mut lost_now) = (0, 0);
    let mut worse = vec![];
    for (index, path) in paths.iter().enumerate() {
        let clip: Vec<f32> = hound::WavReader::open(path)
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / i16::MAX as f32)
            .collect();
        let reference = words(&std::fs::read_to_string(path.with_extension("txt")).unwrap());
        for (g, gap) in gaps.iter().enumerate() {
            let mut samples = clip.clone();
            samples.extend(std::iter::repeat_n(0.0, (gap * rate as f32) as usize));
            let (before, now);
            if (index + g) % 2 == 0 {
                before = setup.run(&samples, false);
                now = setup.run(&samples, true);
            } else {
                now = setup.run(&samples, true);
                before = setup.run(&samples, false);
            }
            runs += 1;
            before_cleaned.push(before.to_cleaned);
            now_cleaned.push(now.to_cleaned);
            before_text.push(before.to_text);
            now_text.push(now.to_text);
            errors_before += edit_distance(&reference, &words(&before.raw));
            errors_now += edit_distance(&reference, &words(&now.raw));
            spoken += reference.len();
            let lost = |text: &str| (words(text).len() as f32) < 0.85 * reference.len() as f32;
            lost_before += usize::from(lost(&before.raw));
            lost_now += usize::from(lost(&now.raw));
            let name = path.file_name().unwrap().to_string_lossy();
            let delta = now.to_cleaned - before.to_cleaned;
            if delta > 0.15 {
                worse.push(format!(
                    "{name} gap {gap}: now {:.0} ms vs before {:.0} ms to cleaned text (recognition {:.0} vs {:.0} ms)",
                    1000.0 * now.to_cleaned,
                    1000.0 * before.to_cleaned,
                    1000.0 * now.to_text,
                    1000.0 * before.to_text
                ));
            }
            if let (Ok(a), Ok(b)) = (&before.cleaned, &now.cleaned) {
                compared += 1;
                if a == b {
                    same_cleaned += 1;
                } else {
                    diff_cleaned.push(format!(
                        "{name} gap {gap}\n    before: {a}\n    now:    {b}"
                    ));
                }
            } else {
                diff_cleaned.push(format!(
                    "{name} gap {gap}: a cleanup failed (before {:?}, now {:?})",
                    before.cleaned.as_ref().err(),
                    now.cleaned.as_ref().err()
                ));
            }
            println!(
                "{name:22} gap {gap:.2}: to cleaned text before {:5.0} ms, now {:5.0} ms | to text before {:5.0}, now {:5.0}",
                1000.0 * before.to_cleaned,
                1000.0 * now.to_cleaned,
                1000.0 * before.to_text,
                1000.0 * now.to_text
            );
        }
    }
    let stats = |label: &str, before: &[f32], now: &[f32]| {
        println!(
            "{label}: median {:.0} -> {:.0} ms, 90th percentile {:.0} -> {:.0} ms, worst {:.0} -> {:.0} ms",
            1000.0 * percentile(before, 0.5),
            1000.0 * percentile(now, 0.5),
            1000.0 * percentile(before, 0.9),
            1000.0 * percentile(now, 0.9),
            1000.0 * percentile(before, 1.0),
            1000.0 * percentile(now, 1.0)
        );
    };
    println!("\n{runs} runs at {}x", setup.speed);
    stats("stop to cleaned text", &before_cleaned, &now_cleaned);
    stats("stop to recognized text", &before_text, &now_text);
    println!(
        "slower than before by more than 150 ms: {} of {runs}",
        worse.len()
    );
    for line in &worse {
        println!("  {line}");
    }
    println!(
        "recognized text: word error rate before {:.1}%, now {:.1}%; dictations that lost over 15% of their words: before {lost_before}, now {lost_now}",
        100.0 * errors_before as f32 / spoken.max(1) as f32,
        100.0 * errors_now as f32 / spoken.max(1) as f32
    );
    println!(
        "cleaned text identical: {same_cleaned} of {compared}; {} differ or failed",
        diff_cleaned.len()
    );
    for line in &diff_cleaned {
        println!("  {line}");
    }
    inference::shutdown(&setup.workers);
}
