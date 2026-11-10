use std::{sync::mpsc, thread::JoinHandle};

/// Recognizes chunks of a recording on a background thread while the
/// recording continues. Chunks are recognized in order; `finish` waits for the
/// last one and returns the joined text.
pub struct Transcriber {
    send: mpsc::Sender<Vec<f32>>,
    thread: JoinHandle<Result<String, String>>,
}
impl Transcriber {
    /// `warm` runs first on the background thread so the recognizer loads while
    /// the user speaks. `recognize` receives each chunk as WAV bytes.
    pub fn start(
        warm: impl FnOnce() + Send + 'static,
        recognize: impl Fn(Vec<u8>) -> Result<String, String> + Send + 'static,
    ) -> Self {
        let (send, receive) = mpsc::channel::<Vec<f32>>();
        let thread = std::thread::spawn(move || {
            warm();
            let mut parts = vec![];
            let mut failure = None;
            for samples in receive {
                if failure.is_some() {
                    continue;
                }
                match crate::audio::wav(&samples).and_then(&recognize) {
                    Ok(text) if !text.trim().is_empty() => parts.push(text.trim().to_owned()),
                    Ok(_) => (),
                    Err(error) => failure = Some(error),
                }
            }
            failure.map_or_else(|| Ok(parts.join(" ")), Err)
        });
        Self { send, thread }
    }
    pub fn push(&self, samples: Vec<f32>) {
        let _ = self.send.send(samples);
    }
    pub fn finish(self) -> Result<String, String> {
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
                move |wav| {
                    let size = wav.len();
                    events.lock().unwrap().push(format!("chunk {size}"));
                    Ok(format!(" text{size} "))
                }
            },
        );
        transcriber.push(chunk(100));
        transcriber.push(chunk(200));
        let text = transcriber.finish().unwrap();
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
            move |_| {
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
            |wav| {
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
        assert_eq!(transcriber.finish().unwrap(), "word");
    }
    #[test]
    fn no_chunks_produce_empty_text() {
        assert_eq!(
            Transcriber::start(|| (), |_| Ok("x".into()))
                .finish()
                .unwrap(),
            ""
        );
    }
}
