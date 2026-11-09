use serde::Serialize;
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Recording,
    Processing,
}
#[derive(Clone, Debug)]
pub struct Session {
    pub phase: Phase,
    pub generation: u64,
    pub pressed: bool,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            generation: 0,
            pressed: false,
        }
    }
}
impl Session {
    pub fn start(&mut self) -> Result<u64, String> {
        if self.phase != Phase::Idle {
            return Err("A recording or transcription is already active".into());
        }
        self.generation += 1;
        self.phase = Phase::Recording;
        Ok(self.generation)
    }
    pub fn finish(&mut self) -> bool {
        if self.phase != Phase::Recording {
            return false;
        }
        self.phase = Phase::Processing;
        true
    }
    pub fn cancel(&mut self) {
        self.generation += 1;
        self.phase = Phase::Idle;
        self.pressed = false;
    }
    pub fn complete(&mut self, generation: u64) -> bool {
        if self.generation != generation || self.phase != Phase::Processing {
            return false;
        }
        self.phase = Phase::Idle;
        true
    }
    pub fn key(&mut self, down: bool, toggle: bool) -> Option<&'static str> {
        if self.pressed == down {
            return None;
        }
        self.pressed = down;
        if down && self.phase == Phase::Idle {
            Some("start")
        } else if self.phase == Phase::Recording && (down == toggle) {
            Some("finish")
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_rejects_late_results_and_processing_rejects_recording() {
        let mut s = Session::default();
        let id = s.start().unwrap();
        assert!(s.finish());
        assert!(s.start().is_err());
        s.cancel();
        assert!(!s.complete(id));
        let next = s.start().unwrap();
        assert!(s.finish());
        assert!(s.complete(next));
        assert!(!s.complete(next));
    }
    #[test]
    fn hotkeys_ignore_repeat_and_push_release_finishes() {
        let mut s = Session::default();
        assert_eq!(s.key(true, false), Some("start"));
        s.start().unwrap();
        assert_eq!(s.key(true, false), None);
        assert_eq!(s.key(false, false), Some("finish"));
        s.finish();
        assert_eq!(s.key(false, false), None);
        s.cancel();
        assert_eq!(s.key(true, true), Some("start"));
        s.start().unwrap();
        assert_eq!(s.key(false, true), None);
        assert_eq!(s.key(true, true), Some("finish"));
    }
}
