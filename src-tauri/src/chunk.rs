use std::ops::Range;

pub const RATE: usize = crate::audio::RATE as usize;
const FRAME: usize = RATE / 50;
/// A chunk is never cut before this much audio is pending, so the recognizer
/// gets enough context and short dictations stay a single request.
pub const MIN_CHUNK: usize = 8 * RATE;
/// Continuous speech is cut at its quietest point once this much is pending.
pub const MAX_CHUNK: usize = 25 * RATE;
const SILENCE_FRAMES: usize = 20;
const FALLBACK_WINDOW: usize = 6 * RATE;
const DIP_FRAMES: usize = 5;

/// Splits a growing recording into chunks at pauses so earlier chunks can be
/// recognized while the user keeps speaking.
#[derive(Default)]
pub struct Chunker {
    emitted: usize,
}
impl Chunker {
    /// Returns the next finished chunk, if the audio so far contains one.
    /// `samples` is the whole recording; ranges index into it.
    pub fn poll(&mut self, samples: &[f32]) -> Option<Range<usize>> {
        let pending = samples.get(self.emitted..)?;
        if pending.len() < MIN_CHUNK + SILENCE_FRAMES * FRAME {
            return None;
        }
        let energy = frame_energy(pending);
        let floor = silence_threshold(&energy);
        let cut = pause_cut(&energy, floor).or_else(|| {
            (pending.len() >= MAX_CHUNK).then(|| quietest_cut(&energy, pending.len()))
        })?;
        let range = self.emitted..self.emitted + cut;
        self.emitted = range.end;
        Some(range)
    }
    /// Returns the audio after the last chunk, once recording has stopped.
    pub fn tail(&mut self, samples: &[f32]) -> Option<Range<usize>> {
        if samples.len() <= self.emitted {
            return None;
        }
        let range = self.emitted..samples.len();
        self.emitted = samples.len();
        Some(range)
    }
}
fn frame_energy(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks_exact(FRAME)
        .map(|frame| (frame.iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt())
        .collect()
}
/// Frames quieter than three times the 10th-percentile level count as pauses.
/// The bounds keep a noisy room from hiding pauses and a silent microphone
/// from turning breaths into speech.
fn silence_threshold(energy: &[f32]) -> f32 {
    let mut sorted = energy.to_vec();
    sorted.sort_by(f32::total_cmp);
    (sorted[sorted.len() / 10] * 3.0).clamp(0.004, 0.03)
}
/// Offset in samples of the middle of the first pause that begins after
/// `MIN_CHUNK` of audio and lasts at least `SILENCE_FRAMES` frames.
fn pause_cut(energy: &[f32], threshold: f32) -> Option<usize> {
    let mut run_start = None;
    for (index, level) in energy.iter().enumerate() {
        if *level < threshold {
            let start = *run_start.get_or_insert(index);
            if index + 1 - start >= SILENCE_FRAMES {
                let middle = (start + index + 1).div_ceil(2) * FRAME;
                if middle >= MIN_CHUNK {
                    return Some(middle);
                }
            }
        } else {
            run_start = None;
        }
    }
    None
}
fn quietest_cut(energy: &[f32], length: usize) -> usize {
    let from = length.saturating_sub(FALLBACK_WINDOW) / FRAME;
    let to = energy.len().saturating_sub(DIP_FRAMES);
    let quietest = (from..=to.max(from))
        .min_by(|a, b| {
            let level = |at: usize| {
                energy[at..(at + DIP_FRAMES).min(energy.len())]
                    .iter()
                    .sum::<f32>()
            };
            level(*a).total_cmp(&level(*b))
        })
        .unwrap_or(from);
    (quietest + DIP_FRAMES / 2) * FRAME
}
#[cfg(test)]
mod tests {
    use super::*;
    fn speech(seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| 0.1 * ((i as f32) * 0.3).sin())
            .collect()
    }
    fn quiet(seconds: f32, level: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| level * if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect()
    }
    fn join(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.concat()
    }
    #[test]
    fn short_recordings_stay_one_chunk() {
        let samples = join(&[speech(5.0), quiet(1.0, 0.0), speech(1.0)]);
        let mut chunker = Chunker::default();
        assert_eq!(chunker.poll(&samples), None);
        assert_eq!(chunker.tail(&samples), Some(0..samples.len()));
    }
    #[test]
    fn cuts_in_the_middle_of_the_first_pause_after_the_minimum() {
        let samples = join(&[speech(9.0), quiet(1.0, 0.0), speech(0.5)]);
        let mut chunker = Chunker::default();
        let range = chunker.poll(&samples).expect("pause should close a chunk");
        assert_eq!(range.start, 0);
        assert!(range.end > 9 * RATE && range.end < 10 * RATE, "{range:?}");
        assert_eq!(chunker.poll(&samples), None);
        assert_eq!(chunker.tail(&samples), Some(range.end..samples.len()));
    }
    #[test]
    fn waits_for_a_long_enough_pause() {
        let samples = join(&[speech(9.0), quiet(0.2, 0.0), speech(2.0)]);
        assert_eq!(Chunker::default().poll(&samples), None);
    }
    #[test]
    fn finds_pauses_in_a_noisy_room() {
        let samples = join(&[speech(9.0), quiet(1.0, 0.003), speech(0.5)]);
        let range = Chunker::default().poll(&samples).unwrap();
        assert!(range.end > 9 * RATE && range.end < 10 * RATE, "{range:?}");
    }
    #[test]
    fn continuous_speech_is_cut_at_its_quietest_point_before_the_maximum() {
        let samples = join(&[speech(22.0), quiet(0.1, 0.02), speech(5.0)]);
        let range = Chunker::default().poll(&samples).unwrap();
        assert!(range.end > 21 * RATE && range.end < 23 * RATE, "{range:?}");
    }
    #[test]
    fn chunks_and_tail_cover_the_recording_without_gaps() {
        let samples = join(&[
            speech(9.0),
            quiet(1.0, 0.0),
            speech(9.0),
            quiet(1.0, 0.0),
            speech(3.0),
        ]);
        let mut chunker = Chunker::default();
        let mut covered = 0;
        for end in (RATE..=samples.len()).step_by(RATE / 10) {
            if let Some(range) = chunker.poll(&samples[..end]) {
                assert_eq!(range.start, covered);
                covered = range.end;
            }
        }
        let tail = chunker.tail(&samples).unwrap();
        assert_eq!(tail, covered..samples.len());
        assert!(covered > 18 * RATE);
    }
}
