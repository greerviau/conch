use std::time::{Duration, Instant};

/// Times the stages between the end of a recording and the delivered text, so
/// the status line can show where the wait went.
pub struct Stopwatch {
    began: Instant,
    last: Instant,
    stages: Vec<(&'static str, Duration)>,
}
impl Stopwatch {
    pub fn start(at: Instant) -> Self {
        Self {
            began: at,
            last: at,
            stages: vec![],
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
        self.last = now;
    }
    /// `Stop to text 412 ms (capture 12 ms, recognition 350 ms, ...)`.
    pub fn summary(&self) -> String {
        let stages: Vec<String> = self
            .stages
            .iter()
            .map(|(stage, took)| format!("{stage} {} ms", took.as_millis()))
            .collect();
        format!(
            "Stop to text {} ms ({})",
            self.last.saturating_duration_since(self.began).as_millis(),
            stages.join(", ")
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_summary_gives_the_total_and_each_stage_in_order() {
        let start = Instant::now();
        let mut watch = Stopwatch::start(start);
        watch.lap_at("capture", start + Duration::from_millis(12));
        watch.lap_at("recognition", start + Duration::from_millis(362));
        watch.lap_at("delivery", start + Duration::from_millis(412));
        assert_eq!(
            watch.summary(),
            "Stop to text 412 ms (capture 12 ms, recognition 350 ms, delivery 50 ms)"
        );
    }
    #[test]
    fn a_stage_that_ends_before_the_last_one_counts_as_zero() {
        let start = Instant::now();
        let mut watch = Stopwatch::start(start + Duration::from_millis(10));
        watch.lap_at("capture", start);
        assert_eq!(watch.summary(), "Stop to text 0 ms (capture 0 ms)");
    }
}
