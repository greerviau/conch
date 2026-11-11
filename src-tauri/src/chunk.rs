use std::ops::Range;

pub const RATE: usize = crate::audio::RATE as usize;
const FRAME: usize = RATE / 50;
/// A chunk is never cut before this much audio is pending, so the recognizer
/// gets enough context and short dictations stay a single request.
pub const MIN_CHUNK: usize = 8 * RATE;
/// Continuous speech is cut at its quietest point once this much is pending.
pub const MAX_CHUNK: usize = 25 * RATE;
/// A pause closes a chunk only after one second of silence. The recognizer ends
/// every chunk with terminal punctuation, so a cut inside a sentence adds a
/// false sentence end ("do? today?"). Shorter pauses occur inside sentences.
const SILENCE_FRAMES: usize = 50;
const FALLBACK_WINDOW: usize = 6 * RATE;
const DIP_FRAMES: usize = 5;
/// Speech followed by this much silence is recognized speculatively. A pause
/// inside a sentence usually lasts less, and one that does not costs only the
/// discarded recognition.
const SETTLE_FRAMES: usize = 15;
/// A speculation needs at least this much speech, and speech after it must
/// last this long to discard it, so a key click or breath neither triggers nor
/// cancels one.
const SPEECH_FRAMES: usize = 5;

/// Splits a growing recording into chunks at pauses so earlier chunks can be
/// recognized while the user keeps speaking.
#[derive(Default)]
pub struct Chunker {
    emitted: usize,
    offered: Option<Offer>,
}
/// Audio handed out by `settle`, with the level that separated its speech from
/// its silence. The same level decides at the stop whether more speech followed,
/// because a level recomputed over the longer recording can call quiet speech
/// silent.
struct Offer {
    range: Range<usize>,
    threshold: f32,
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
    /// Returns the audio since the last chunk when speech has just ended, so it
    /// can be recognized before the recording stops. Each stretch of speech is
    /// offered once.
    pub fn settle(&mut self, samples: &[f32]) -> Option<Range<usize>> {
        let energy = frame_energy(samples.get(self.emitted..)?);
        let threshold = settle_threshold(&energy)?;
        let (speech, ended) = speech_extent(&energy, threshold)?;
        if speech < SPEECH_FRAMES || energy.len() - ended < SETTLE_FRAMES {
            return None;
        }
        let spoken_to = self.emitted + ended * FRAME;
        if self.offered.as_ref().is_some_and(|offered| {
            offered.range.start == self.emitted && spoken_to <= offered.range.end
        }) {
            return None;
        }
        let range = self.emitted..self.emitted + energy.len() * FRAME;
        self.offered = Some(Offer {
            range: range.clone(),
            threshold,
        });
        Some(range)
    }
    /// Returns the range last offered by `settle` when no speech follows it, so
    /// its text can stand in for the tail.
    pub fn settled(&self, samples: &[f32]) -> Option<Range<usize>> {
        let offer = self
            .offered
            .as_ref()
            .filter(|o| o.range.start == self.emitted)?;
        let energy = frame_energy(samples.get(self.emitted..)?);
        let after = energy.get((offer.range.end - offer.range.start) / FRAME..)?;
        (after
            .iter()
            .filter(|level| **level >= offer.threshold)
            .count()
            < SPEECH_FRAMES)
            .then(|| offer.range.clone())
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
/// Level separating speech from silence for `settle`: three times the mean of
/// the quietest 300 ms, within the bounds of `silence_threshold`. A percentile
/// of all frames, as `silence_threshold` takes, reads a pause shorter than a
/// tenth of the recording as speech and so counts quiet speech as silence.
/// `None` when the audio is shorter than 300 ms.
fn settle_threshold(energy: &[f32]) -> Option<f32> {
    let quietest = energy
        .windows(SETTLE_FRAMES)
        .map(|window| window.iter().sum::<f32>() / SETTLE_FRAMES as f32)
        .min_by(f32::total_cmp)?;
    Some((quietest * 3.0).clamp(0.004, 0.03))
}
/// The number of frames at or above `threshold`, and the index after the last
/// one, or `None` if there are none.
fn speech_extent(energy: &[f32], threshold: f32) -> Option<(usize, usize)> {
    let last = energy.iter().rposition(|level| *level >= threshold)?;
    let loud = energy.iter().filter(|level| **level >= threshold).count();
    Some((loud, last + 1))
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
    fn a_pause_inside_a_sentence_does_not_close_a_chunk() {
        let samples = join(&[speech(9.0), quiet(0.6, 0.0), speech(2.0)]);
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
    #[test]
    fn speech_followed_by_silence_is_offered_once() {
        let samples = join(&[speech(3.0), quiet(0.4, 0.0)]);
        let mut chunker = Chunker::default();
        let range = chunker.settle(&samples).expect("settled speech is offered");
        assert_eq!(range.start, 0);
        assert!(
            range.end > 3 * RATE && range.end <= samples.len(),
            "{range:?}"
        );
        assert_eq!(chunker.settle(&samples), None);
    }
    #[test]
    fn a_pause_shorter_than_the_settle_time_offers_nothing() {
        let samples = join(&[speech(3.0), quiet(0.2, 0.0)]);
        assert_eq!(Chunker::default().settle(&samples), None);
    }
    #[test]
    fn silence_and_clicks_offer_nothing() {
        let mut chunker = Chunker::default();
        assert_eq!(chunker.settle(&quiet(2.0, 0.0)), None);
        let click = join(&[quiet(1.0, 0.0), speech(0.03), quiet(0.5, 0.0)]);
        assert_eq!(chunker.settle(&click), None);
    }
    #[test]
    fn speech_after_an_offer_is_offered_again() {
        let first = join(&[speech(3.0), quiet(0.4, 0.0)]);
        let mut chunker = Chunker::default();
        let earlier = chunker.settle(&first).unwrap();
        let second = join(&[first, speech(2.0), quiet(0.4, 0.0)]);
        let later = chunker.settle(&second).expect("new speech settles again");
        assert_eq!(later.start, 0);
        assert!(later.end > earlier.end + 2 * RATE, "{earlier:?} {later:?}");
    }
    #[test]
    fn the_offer_stands_in_for_the_tail_while_only_silence_follows() {
        let offered = join(&[speech(3.0), quiet(0.4, 0.0)]);
        let mut chunker = Chunker::default();
        let range = chunker.settle(&offered).unwrap();
        let stopped = join(&[offered, quiet(0.6, 0.0)]);
        assert_eq!(chunker.settled(&stopped), Some(range));
    }
    #[test]
    fn speech_after_the_offer_discards_it() {
        let offered = join(&[speech(3.0), quiet(0.4, 0.0)]);
        let mut chunker = Chunker::default();
        chunker.settle(&offered).unwrap();
        let stopped = join(&[offered, speech(1.0)]);
        assert_eq!(chunker.settled(&stopped), None);
    }
    #[test]
    fn a_key_click_after_the_offer_does_not_discard_it() {
        let offered = join(&[speech(3.0), quiet(0.4, 0.0)]);
        let mut chunker = Chunker::default();
        let range = chunker.settle(&offered).unwrap();
        let stopped = join(&[offered, speech(0.03), quiet(0.1, 0.0)]);
        assert_eq!(chunker.settled(&stopped), Some(range));
    }
    #[test]
    fn nothing_is_offered_before_the_first_settle() {
        let samples = join(&[speech(3.0), quiet(0.4, 0.0)]);
        assert_eq!(Chunker::default().settled(&samples), None);
    }
    #[test]
    fn a_committed_chunk_discards_the_offer_and_later_offers_start_after_it() {
        let offered = join(&[speech(9.0), quiet(0.4, 0.0)]);
        let mut chunker = Chunker::default();
        chunker.settle(&offered).unwrap();
        let paused = join(&[offered, quiet(0.8, 0.0)]);
        let chunk = chunker.poll(&paused).expect("the pause closes a chunk");
        assert_eq!(chunker.settled(&paused), None);
        let next = join(&[paused, speech(2.0), quiet(0.4, 0.0)]);
        let range = chunker.settle(&next).expect("later speech settles");
        assert_eq!(range.start, chunk.end);
    }
    #[test]
    fn a_recording_shorter_than_one_frame_offers_nothing() {
        let mut chunker = Chunker::default();
        assert_eq!(chunker.settle(&[]), None);
        assert_eq!(chunker.settle(&[0.0; 100]), None);
        assert_eq!(chunker.settled(&[]), None);
        assert_eq!(chunker.settled(&[0.0; 100]), None);
    }
    fn tone(seconds: f32, amplitude: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize)
            .map(|i| amplitude * ((i as f32) * 0.3).sin())
            .collect()
    }
    #[test]
    fn quiet_speech_after_the_offer_discards_it_however_the_recording_is_balanced() {
        // The pause is under a tenth of the 4.6 s recording at the stop, so a
        // threshold taken from the whole recording would count 0.02-level
        // speech as silence and the words would be lost.
        let offered = join(&[tone(3.0, 0.014), quiet(0.4, 0.0)]);
        let mut chunker = Chunker::default();
        chunker
            .settle(&offered)
            .expect("quiet speech then silence settles");
        let stopped = join(&[offered, tone(1.2, 0.028)]);
        assert_eq!(chunker.settled(&stopped), None);
    }
    #[test]
    fn quiet_speech_alone_is_not_offered_as_silence() {
        // 0.3 s of near silence in 3.3 s: the pause is shorter than a tenth of
        // the recording, so a percentile threshold would call the speech silent.
        let samples = join(&[tone(3.0, 0.014), quiet(0.3, 0.0)]);
        assert!(Chunker::default().settle(&samples).is_some());
    }
}
