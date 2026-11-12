use std::{
    collections::VecDeque,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

enum Job {
    Chunk(Vec<f32>),
    Speculate {
        range: Range<usize>,
        samples: Vec<f32>,
    },
    Tail {
        samples: Vec<f32>,
        reuse: Option<Range<usize>>,
    },
}

/// Cleans the text of one chunk or tail.
type Cleaner = Box<dyn Fn(&str) -> Result<String, String> + Send>;

/// The text of a recording and whether the tail came from a speculation.
#[derive(Debug)]
pub struct Recognized {
    pub text: String,
    pub speculative: bool,
    /// The text of each chunk and of the tail, in order, as joined into `text`.
    pub parts: Vec<String>,
    /// When the last part was recognized, before its cleaning finished.
    pub recognized_at: std::time::Instant,
    /// The cleaned parts joined, or the first cleaning failure; `None` when the
    /// transcriber was started without a cleaner.
    pub cleaned: Option<Result<String, String>>,
}

/// Recognizes chunks of a recording on a background thread while the
/// recording continues. Chunks are recognized in order; `finish` waits for the
/// last one and returns the joined text.
///
/// The audio since the last chunk can also be recognized speculatively once
/// speech ends. If the recording then stops without more speech, the tail
/// reuses that text instead of being recognized again.
pub struct Transcriber {
    send: mpsc::Sender<Job>,
    thread: JoinHandle<Result<Recognized, String>>,
}
impl Transcriber {
    /// `warm` runs first on the background thread so the recognizer loads while
    /// the user speaks. `recognize` receives each chunk as WAV bytes and `true`
    /// when the audio is a speculation, whose text may still be discarded, and
    /// `false` for a chunk or a tail, whose text is final.
    pub fn start(
        warm: impl FnOnce() + Send + 'static,
        recognize: impl Fn(Vec<u8>, bool) -> Result<String, String> + Send + 'static,
    ) -> Self {
        Self::spawn(warm, recognize, None)
    }
    /// Like `start`, and `clean` also runs on a second thread on the text of each
    /// chunk and of the tail as soon as it is recognized, so only the tail's
    /// cleaning is left to wait for after the stop. After the first failure the
    /// later parts are not cleaned.
    pub fn start_cleaning(
        warm: impl FnOnce() + Send + 'static,
        recognize: impl Fn(Vec<u8>, bool) -> Result<String, String> + Send + 'static,
        clean: impl Fn(&str) -> Result<String, String> + Send + 'static,
    ) -> Self {
        Self::spawn(warm, recognize, Some(Box::new(clean)))
    }
    fn spawn(
        warm: impl FnOnce() + Send + 'static,
        recognize: impl Fn(Vec<u8>, bool) -> Result<String, String> + Send + 'static,
        clean: Option<Cleaner>,
    ) -> Self {
        let (send, receive) = mpsc::channel::<Job>();
        let thread = std::thread::spawn(move || {
            warm();
            // Set when recognition fails, so parts still queued for cleaning are
            // skipped.
            let abandoned = Arc::new(AtomicBool::new(false));
            let cleaner = clean.map(|clean| {
                let (send, receive) = mpsc::channel::<String>();
                let thread = std::thread::spawn({
                    let abandoned = abandoned.clone();
                    move || {
                        let mut cleaned: Vec<String> = vec![];
                        let mut failure = None;
                        for part in receive {
                            if failure.is_some() || abandoned.load(Ordering::Relaxed) {
                                continue;
                            }
                            match clean(&part) {
                                Ok(text) if !text.trim().is_empty() => {
                                    cleaned.push(text.trim().to_owned())
                                }
                                Ok(_) => (),
                                Err(error) => failure = Some(error),
                            }
                        }
                        failure.map_or_else(|| Ok(cleaned.join(" ")), Err)
                    }
                });
                (send, thread)
            });
            let run = |samples: Vec<f32>, speculative: bool| {
                crate::audio::wav(&samples).and_then(|wav| recognize(wav, speculative))
            };
            let mut parts = vec![];
            let mut failure = None;
            let mut speculation: Option<(Range<usize>, String)> = None;
            let mut speculative = false;
            let mut queue = VecDeque::new();
            loop {
                if queue.is_empty() {
                    match receive.recv() {
                        Ok(job) => queue.push_back(job),
                        Err(_) => break,
                    }
                }
                queue.extend(receive.try_iter());
                let job = queue.pop_front().expect("the queue was filled above");
                if failure.is_some() {
                    continue;
                }
                // A later speculation or chunk makes this speculation useless,
                // so do not spend the recognizer on it.
                if matches!(job, Job::Speculate { .. })
                    && queue
                        .iter()
                        .any(|next| matches!(next, Job::Speculate { .. } | Job::Chunk(_)))
                {
                    continue;
                }
                let result = match job {
                    Job::Chunk(samples) => {
                        speculation = None;
                        run(samples, false)
                    }
                    Job::Speculate { range, samples } => {
                        speculation = run(samples, true).ok().map(|text| (range, text));
                        continue;
                    }
                    Job::Tail { samples, reuse } => {
                        match reuse
                            .and_then(|range| speculation.take().filter(|(at, _)| *at == range))
                        {
                            Some((_, text)) => {
                                speculative = true;
                                Ok(text)
                            }
                            None => run(samples, false),
                        }
                    }
                };
                match result {
                    Ok(text) if !text.trim().is_empty() => {
                        let text = text.trim().to_owned();
                        if let Some((send, _)) = &cleaner {
                            let _ = send.send(text.clone());
                        }
                        parts.push(text);
                    }
                    Ok(_) => (),
                    Err(error) => failure = Some(error),
                }
            }
            let recognized_at = std::time::Instant::now();
            if let Some(error) = failure {
                // The recording is recognized again as a whole, so the cleaning
                // of its parts is not needed. A cleaning already in progress
                // ends on its own.
                abandoned.store(true, Ordering::Relaxed);
                return Err(error);
            }
            let cleaned = cleaner.map(|(send, thread)| {
                drop(send);
                thread
                    .join()
                    .unwrap_or_else(|_| Err("Cleanup worker failed".into()))
            });
            Ok(Recognized {
                text: parts.join(" "),
                parts,
                speculative,
                recognized_at,
                cleaned,
            })
        });
        Self { send, thread }
    }
    pub fn push(&self, samples: Vec<f32>) {
        let _ = self.send.send(Job::Chunk(samples));
    }
    /// Recognizes `samples`, the audio since the last chunk up to `range.end`,
    /// before the recording stops. Its failure is ignored; the tail recognizes
    /// the audio again.
    pub fn speculate(&self, range: Range<usize>, samples: Vec<f32>) {
        let _ = self.send.send(Job::Speculate { range, samples });
    }
    /// Adds the audio after the last chunk. When `reuse` names the range of an
    /// earlier speculation that is still current, its text replaces recognizing
    /// `samples`.
    pub fn push_tail(&self, samples: Vec<f32>, reuse: Option<Range<usize>>) {
        let _ = self.send.send(Job::Tail { samples, reuse });
    }
    pub fn finish(self) -> Result<Recognized, String> {
        drop(self.send);
        self.thread
            .join()
            .map_err(|_| "Recognition worker failed".to_string())?
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    fn chunk(length: usize) -> Vec<f32> {
        vec![0.0; length]
    }
    #[test]
    fn warms_first_then_joins_chunk_text_in_order() {
        let events = Arc::new(Mutex::new(vec![]));
        let transcriber = Transcriber::start(
            {
                let events = events.clone();
                move || events.lock().unwrap().push("warm".to_string())
            },
            {
                let events = events.clone();
                move |wav, _| {
                    let size = wav.len();
                    events.lock().unwrap().push(format!("chunk {size}"));
                    Ok(format!(" text{size} "))
                }
            },
        );
        transcriber.push(chunk(100));
        transcriber.push(chunk(200));
        let text = transcriber.finish().unwrap().text;
        let sizes: Vec<_> = events.lock().unwrap()[1..]
            .iter()
            .map(|e| e.strip_prefix("chunk ").unwrap().to_string())
            .collect();
        assert_eq!(events.lock().unwrap()[0], "warm");
        assert_eq!(text, format!("text{} text{}", sizes[0], sizes[1]));
        assert!(sizes[0] < sizes[1]);
    }
    #[test]
    fn a_failed_chunk_fails_the_whole_recording_and_skips_later_chunks() {
        let calls = Arc::new(Mutex::new(0));
        let transcriber = Transcriber::start(|| (), {
            let calls = calls.clone();
            move |_, _| {
                *calls.lock().unwrap() += 1;
                Err("worker died".into())
            }
        });
        transcriber.push(chunk(10));
        transcriber.push(chunk(10));
        assert_eq!(transcriber.finish().unwrap_err(), "worker died");
        assert_eq!(*calls.lock().unwrap(), 1);
    }
    #[test]
    fn blank_chunks_add_no_separator() {
        let transcriber = Transcriber::start(
            || (),
            |wav, _| {
                Ok(if wav.len() > 100 {
                    "word".into()
                } else {
                    "  ".into()
                })
            },
        );
        transcriber.push(chunk(10));
        transcriber.push(chunk(200));
        transcriber.push(chunk(10));
        assert_eq!(transcriber.finish().unwrap().text, "word");
    }
    #[test]
    fn no_chunks_produce_empty_text() {
        assert_eq!(
            Transcriber::start(|| (), |_, _| Ok("x".into()))
                .finish()
                .unwrap()
                .text,
            ""
        );
    }
    fn sizes(lengths: &[usize]) -> Vec<usize> {
        lengths
            .iter()
            .map(|n| crate::audio::wav(&chunk(*n)).unwrap().len())
            .collect()
    }
    /// Recognizes each request as `text<bytes>` and records the sizes.
    fn recording(
        calls: &Arc<Mutex<Vec<usize>>>,
    ) -> impl Fn(Vec<u8>, bool) -> Result<String, String> + use<> {
        let calls = calls.clone();
        move |wav, _| {
            calls.lock().unwrap().push(wav.len());
            Ok(format!("text{}", wav.len()))
        }
    }
    #[test]
    fn a_current_speculation_replaces_recognizing_the_tail() {
        let calls = Arc::new(Mutex::new(vec![]));
        let transcriber = Transcriber::start(|| (), recording(&calls));
        transcriber.speculate(0..100, chunk(100));
        transcriber.push_tail(chunk(120), Some(0..100));
        let outcome = transcriber.finish().unwrap();
        let [speculated] = sizes(&[100])[..] else {
            unreachable!()
        };
        assert_eq!(*calls.lock().unwrap(), vec![speculated]);
        assert_eq!(outcome.text, format!("text{speculated}"));
        assert!(outcome.speculative);
    }
    #[test]
    fn a_tail_without_a_matching_speculation_is_recognized() {
        let calls = Arc::new(Mutex::new(vec![]));
        let transcriber = Transcriber::start(|| (), recording(&calls));
        transcriber.speculate(0..100, chunk(100));
        transcriber.push_tail(chunk(200), None);
        let outcome = transcriber.finish().unwrap();
        let [speculated, tail] = sizes(&[100, 200])[..] else {
            unreachable!()
        };
        assert_eq!(*calls.lock().unwrap(), vec![speculated, tail]);
        assert_eq!(outcome.text, format!("text{tail}"));
        assert!(!outcome.speculative);
    }
    #[test]
    fn a_chunk_pushed_after_a_speculation_discards_it() {
        let calls = Arc::new(Mutex::new(vec![]));
        let transcriber = Transcriber::start(|| (), recording(&calls));
        transcriber.speculate(0..100, chunk(100));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while calls.lock().unwrap().is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "the speculation never ran"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        transcriber.push(chunk(300));
        transcriber.push_tail(chunk(50), Some(0..100));
        let outcome = transcriber.finish().unwrap();
        let [speculated, chunked, tail] = sizes(&[100, 300, 50])[..] else {
            unreachable!()
        };
        assert_eq!(*calls.lock().unwrap(), vec![speculated, chunked, tail]);
        assert_eq!(outcome.text, format!("text{chunked} text{tail}"));
        assert!(!outcome.speculative);
    }
    #[test]
    fn a_failed_speculation_is_recognized_again_by_the_tail() {
        let calls = Arc::new(Mutex::new(0));
        let transcriber = Transcriber::start(|| (), {
            let calls = calls.clone();
            move |_, _| {
                let mut calls = calls.lock().unwrap();
                *calls += 1;
                if *calls == 1 {
                    Err("worker died".into())
                } else {
                    Ok("tail text".into())
                }
            }
        });
        transcriber.speculate(0..100, chunk(100));
        transcriber.push_tail(chunk(120), Some(0..100));
        let outcome = transcriber.finish().unwrap();
        assert_eq!(outcome.text, "tail text");
        assert!(!outcome.speculative);
        assert_eq!(*calls.lock().unwrap(), 2);
    }
    #[test]
    fn a_failed_tail_after_a_speculation_fails_the_recording() {
        let transcriber = Transcriber::start(|| (), |_, _| Err("worker died".into()));
        transcriber.speculate(0..100, chunk(100));
        transcriber.push_tail(chunk(120), Some(0..100));
        assert_eq!(transcriber.finish().unwrap_err(), "worker died");
    }
    #[test]
    fn a_speculation_replaced_while_queued_is_not_recognized() {
        let calls = Arc::new(Mutex::new(vec![]));
        let (release, hold) = mpsc::channel::<()>();
        let transcriber = Transcriber::start(|| (), {
            let calls = calls.clone();
            let hold = Mutex::new(hold);
            move |wav, _| {
                if calls.lock().unwrap().is_empty() {
                    hold.lock().unwrap().recv().unwrap();
                }
                calls.lock().unwrap().push(wav.len());
                Ok(format!("text{}", wav.len()))
            }
        });
        transcriber.push(chunk(100));
        transcriber.speculate(100..300, chunk(200));
        transcriber.speculate(100..600, chunk(500));
        transcriber.push_tail(chunk(520), Some(100..600));
        release.send(()).unwrap();
        let outcome = transcriber.finish().unwrap();
        let [first, newest] = sizes(&[100, 500])[..] else {
            unreachable!()
        };
        assert_eq!(*calls.lock().unwrap(), vec![first, newest]);
        assert!(outcome.speculative);
        assert_eq!(outcome.text, format!("text{first} text{newest}"));
    }
    #[test]
    fn only_speculations_are_marked_speculative() {
        let flags = Arc::new(Mutex::new(vec![]));
        let transcriber = Transcriber::start(|| (), {
            let flags = flags.clone();
            move |wav, speculative| {
                flags.lock().unwrap().push((wav.len(), speculative));
                Ok("text".into())
            }
        });
        transcriber.push(chunk(100));
        transcriber.speculate(100..300, chunk(200));
        transcriber.push_tail(chunk(300), None);
        transcriber.finish().unwrap();
        let [chunked, speculated, tail] = sizes(&[100, 200, 300])[..] else {
            unreachable!()
        };
        assert_eq!(
            *flags.lock().unwrap(),
            vec![(chunked, false), (speculated, true), (tail, false)]
        );
    }
    /// Records the text it is given and returns it in capitals.
    fn cleaning(
        calls: &Arc<Mutex<Vec<String>>>,
    ) -> impl Fn(&str) -> Result<String, String> + Send + use<> {
        let calls = calls.clone();
        move |text| {
            calls.lock().unwrap().push(text.to_owned());
            Ok(text.to_uppercase())
        }
    }
    fn wait_for<T>(what: &str, mut ready: impl FnMut() -> Option<T>) -> T {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(found) = ready() {
                return found;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{what} never happened"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    #[test]
    fn each_chunk_and_the_tail_are_cleaned_in_order() {
        let cleaned = Arc::new(Mutex::new(vec![]));
        let transcriber = Transcriber::start_cleaning(
            || (),
            |wav, _| Ok(format!("text{}", wav.len())),
            cleaning(&cleaned),
        );
        transcriber.push(chunk(100));
        transcriber.push(chunk(200));
        transcriber.push_tail(chunk(300), None);
        let outcome = transcriber.finish().unwrap();
        let [a, b, c] = sizes(&[100, 200, 300])[..] else {
            unreachable!()
        };
        let texts = vec![format!("text{a}"), format!("text{b}"), format!("text{c}")];
        assert_eq!(*cleaned.lock().unwrap(), texts);
        assert_eq!(outcome.text, texts.join(" "));
        assert_eq!(outcome.cleaned, Some(Ok(texts.join(" ").to_uppercase())));
    }
    #[test]
    fn a_chunk_is_cleaned_while_the_recording_continues() {
        let cleaned = Arc::new(Mutex::new(vec![]));
        let transcriber = Transcriber::start_cleaning(
            || (),
            |_, _| Ok("spoken words".into()),
            cleaning(&cleaned),
        );
        transcriber.push(chunk(100));
        wait_for("cleaning the chunk", || {
            (!cleaned.lock().unwrap().is_empty()).then_some(())
        });
        assert_eq!(*cleaned.lock().unwrap(), vec!["spoken words".to_string()]);
        transcriber.push_tail(chunk(100), None);
        assert_eq!(
            transcriber.finish().unwrap().cleaned,
            Some(Ok("SPOKEN WORDS SPOKEN WORDS".into()))
        );
    }
    #[test]
    fn speculative_text_is_cleaned_only_when_it_becomes_the_tail() {
        let cleaned = Arc::new(Mutex::new(vec![]));
        let transcriber = Transcriber::start_cleaning(
            || (),
            |wav, _| Ok(format!("text{}", wav.len())),
            cleaning(&cleaned),
        );
        transcriber.speculate(0..100, chunk(100));
        transcriber.push(chunk(300));
        transcriber.speculate(300..400, chunk(100));
        transcriber.push_tail(chunk(150), Some(300..400));
        transcriber.finish().unwrap();
        let [chunked, speculated] = sizes(&[300, 100])[..] else {
            unreachable!()
        };
        assert_eq!(
            *cleaned.lock().unwrap(),
            vec![format!("text{chunked}"), format!("text{speculated}")]
        );
    }
    #[test]
    fn a_failed_cleaning_is_reported_and_later_parts_are_not_cleaned() {
        let calls = Arc::new(Mutex::new(0));
        let transcriber = Transcriber::start_cleaning(|| (), |_, _| Ok("words".into()), {
            let calls = calls.clone();
            move |_| {
                *calls.lock().unwrap() += 1;
                Err("cleanup failed".into())
            }
        });
        transcriber.push(chunk(100));
        transcriber.push(chunk(100));
        transcriber.push_tail(chunk(100), None);
        let outcome = transcriber.finish().unwrap();
        assert_eq!(outcome.text, "words words words");
        assert_eq!(outcome.cleaned, Some(Err("cleanup failed".into())));
        assert_eq!(*calls.lock().unwrap(), 1);
    }
    #[test]
    fn a_part_that_cleans_to_nothing_adds_no_separator() {
        let transcriber = Transcriber::start_cleaning(
            || (),
            |wav, _| Ok(format!("text{}", wav.len())),
            |text| {
                Ok(if text.ends_with(&sizes(&[100])[0].to_string()) {
                    "".into()
                } else {
                    text.to_uppercase()
                })
            },
        );
        transcriber.push(chunk(100));
        transcriber.push_tail(chunk(200), None);
        let tail = sizes(&[200])[0];
        assert_eq!(
            transcriber.finish().unwrap().cleaned,
            Some(Ok(format!("TEXT{tail}")))
        );
    }
    #[test]
    fn a_failed_recognition_fails_the_recording_without_waiting_for_cleaning() {
        let transcriber = Transcriber::start_cleaning(
            || (),
            |_, _| Err("worker died".into()),
            |text| Ok(text.into()),
        );
        transcriber.push(chunk(100));
        assert_eq!(transcriber.finish().unwrap_err(), "worker died");
    }
    #[test]
    fn without_a_cleaner_nothing_is_cleaned() {
        let transcriber = Transcriber::start(|| (), |_, _| Ok("words".into()));
        transcriber.push_tail(chunk(100), None);
        assert!(transcriber.finish().unwrap().cleaned.is_none());
    }
    #[test]
    fn recognition_is_timed_before_the_tail_is_cleaned() {
        let transcriber = Transcriber::start_cleaning(
            || (),
            |_, _| Ok("words".into()),
            |text| {
                std::thread::sleep(std::time::Duration::from_millis(150));
                Ok(text.into())
            },
        );
        transcriber.push_tail(chunk(100), None);
        let outcome = transcriber.finish().unwrap();
        assert!(outcome.recognized_at.elapsed() >= std::time::Duration::from_millis(100));
    }
    #[test]
    fn a_recognition_failure_does_not_wait_for_a_cleaning_in_progress() {
        let (release, hold) = mpsc::channel::<()>();
        let (started_send, started) = mpsc::channel::<()>();
        let calls = Arc::new(Mutex::new(0));
        let transcriber = Transcriber::start_cleaning(
            || (),
            {
                let calls = calls.clone();
                move |_, _| {
                    let mut calls = calls.lock().unwrap();
                    *calls += 1;
                    if *calls == 1 {
                        Ok("first".into())
                    } else {
                        Err("worker died".into())
                    }
                }
            },
            {
                let hold = Mutex::new(hold);
                let started = Mutex::new(started_send);
                move |text| {
                    started.lock().unwrap().send(()).unwrap();
                    let _ = hold.lock().unwrap().recv();
                    Ok(text.into())
                }
            },
        );
        transcriber.push(chunk(100));
        started
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the first chunk was never cleaned");
        transcriber.push(chunk(100));
        let (done_send, done) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done_send.send(transcriber.finish());
        });
        let outcome = done.recv_timeout(std::time::Duration::from_secs(2));
        let _ = release.send(());
        let outcome = outcome.expect("finish waited for the cleaning that was in progress");
        assert_eq!(outcome.unwrap_err(), "worker died");
    }
    #[test]
    fn parts_still_queued_are_not_cleaned_after_a_recognition_failure() {
        let (release, hold) = mpsc::channel::<()>();
        let (started_send, started) = mpsc::channel::<()>();
        let cleaned = Arc::new(Mutex::new(vec![]));
        let calls = Arc::new(Mutex::new(0));
        let transcriber = Transcriber::start_cleaning(
            || (),
            {
                let calls = calls.clone();
                move |_, _| {
                    let mut calls = calls.lock().unwrap();
                    *calls += 1;
                    match *calls {
                        1 | 2 => Ok(format!("part{calls}")),
                        _ => Err("worker died".into()),
                    }
                }
            },
            {
                let hold = Mutex::new(hold);
                let started = Mutex::new(started_send);
                let cleaned = cleaned.clone();
                move |text| {
                    cleaned.lock().unwrap().push(text.to_owned());
                    let _ = started.lock().unwrap().send(());
                    let _ = hold.lock().unwrap().recv();
                    Ok(text.into())
                }
            },
        );
        transcriber.push(chunk(100));
        started
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the first chunk was never cleaned");
        transcriber.push(chunk(100));
        transcriber.push(chunk(100));
        assert_eq!(transcriber.finish().unwrap_err(), "worker died");
        let _ = release.send(());
        let _ = release.send(());
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(*cleaned.lock().unwrap(), vec!["part1".to_string()]);
    }
    #[test]
    fn the_parts_of_the_recording_are_returned_in_order() {
        let transcriber = Transcriber::start(|| (), |wav, _| Ok(format!("text{}", wav.len())));
        transcriber.push(chunk(100));
        transcriber.push_tail(chunk(200), None);
        let [a, b] = sizes(&[100, 200])[..] else {
            unreachable!()
        };
        assert_eq!(
            transcriber.finish().unwrap().parts,
            vec![format!("text{a}"), format!("text{b}")]
        );
    }
}
