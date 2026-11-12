use std::time::{Duration, Instant};

/// Times the stages between the end of a recording and the delivered text, so
/// the log can show where the wait went.
pub struct Stopwatch {
    began: Instant,
    last: Instant,
    stages: Vec<(&'static str, Duration)>,
    notes: Vec<Option<String>>,
}
impl Stopwatch {
    pub fn start(at: Instant) -> Self {
        Self {
            began: at,
            last: at,
            stages: vec![],
            notes: vec![],
        }
    }
    /// Ends the current stage under `stage`.
    pub fn lap(&mut self, stage: &'static str) {
        self.lap_at(stage, Instant::now());
    }
    /// Ends the current stage under `stage` at `now`.
    pub fn lap_at(&mut self, stage: &'static str, now: Instant) {
        self.stages
            .push((stage, now.saturating_duration_since(self.last)));
        self.notes.push(None);
        self.last = now;
    }
    /// Adds a remark to the stage just ended, shown in parentheses after it.
    pub fn note(&mut self, text: String) {
        if let Some(last) = self.notes.last_mut() {
            *last = Some(text);
        }
    }
    /// `Stop to text 412 ms`.
    pub fn headline(&self) -> String {
        format!(
            "Stop to text {} ms",
            self.last.saturating_duration_since(self.began).as_millis()
        )
    }
    /// One line per stage: `recognition 350 ms`, or `cleanup 500 ms (model 290 ms)`.
    pub fn detail(&self) -> String {
        self.stages
            .iter()
            .zip(&self.notes)
            .map(|((stage, took), note)| match note {
                Some(note) => format!("{stage} {} ms ({note})", took.as_millis()),
                None => format!("{stage} {} ms", took.as_millis()),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_headline_gives_the_total_and_the_detail_each_stage_in_order() {
        let start = Instant::now();
        let mut watch = Stopwatch::start(start);
        watch.lap_at("capture", start + Duration::from_millis(12));
        watch.lap_at("recognition", start + Duration::from_millis(362));
        watch.lap_at("delivery", start + Duration::from_millis(412));
        assert_eq!(watch.headline(), "Stop to text 412 ms");
        assert_eq!(
            watch.detail(),
            "capture 12 ms\nrecognition 350 ms\ndelivery 50 ms"
        );
    }
    #[test]
    fn a_note_follows_the_stage_it_was_added_to() {
        let start = Instant::now();
        let mut watch = Stopwatch::start(start);
        watch.lap_at("recognition", start + Duration::from_millis(100));
        watch.lap_at("cleanup", start + Duration::from_millis(600));
        watch.note("model 290 ms".into());
        watch.lap_at("delivery", start + Duration::from_millis(620));
        assert_eq!(watch.headline(), "Stop to text 620 ms");
        assert_eq!(
            watch.detail(),
            "recognition 100 ms\ncleanup 500 ms (model 290 ms)\ndelivery 20 ms"
        );
    }
    #[test]
    fn a_stage_that_ends_before_the_last_one_counts_as_zero() {
        let start = Instant::now();
        let mut watch = Stopwatch::start(start + Duration::from_millis(10));
        watch.lap_at("capture", start);
        assert_eq!(watch.headline(), "Stop to text 0 ms");
        assert_eq!(watch.detail(), "capture 0 ms");
    }
}
