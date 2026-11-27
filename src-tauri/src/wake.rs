//! Turns on the accessibility tree of the app being dictated into, so its
//! focused field can be read, and turns off what Conch turned on once the
//! result is delivered.

use std::path::Path;
use std::time::Instant;

/// Electron builds its tree when an assistive client sets this on the app.
pub const MANUAL: &str = "AXManualAccessibility";
/// Chromium browsers ignore `MANUAL` and build their tree, after a 2 s delay,
/// only when this is set. VoiceOver sets it on every app it runs with.
pub const ENHANCED: &str = "AXEnhancedUserInterface";
pub const ATTRIBUTE_UNSUPPORTED: i32 = -25205;
/// The app did not answer in time.
pub const CANNOT_COMPLETE: i32 = -25204;

/// The accessibility calls a wake needs, by application pid.
pub trait Apps {
    fn electron(&mut self, pid: i32) -> bool;
    /// Returns the value, or the accessibility error code.
    fn read(&mut self, pid: i32, attribute: &str) -> Result<bool, i32>;
    /// Returns the accessibility error code, 0 on success.
    fn set(&mut self, pid: i32, attribute: &str, value: bool) -> i32;
}

/// Whether the app bundle at `bundle` is built on Electron.
pub fn electron_bundle(bundle: &Path) -> bool {
    bundle
        .join("Contents/Frameworks/Electron Framework.framework")
        .is_dir()
}

/// The apps this process woke, those it turned `ENHANCED` on in, and how many
/// recordings hold them awake.
#[derive(Default)]
pub struct Wakes {
    woken: Vec<(i32, Instant)>,
    enhanced: Vec<i32>,
    holders: usize,
}

impl Wakes {
    pub const fn new() -> Self {
        Self {
            woken: Vec::new(),
            enhanced: Vec::new(),
            holders: 0,
        }
    }
    /// Called when a recording starts; `release` is called when it ends.
    pub fn hold(&mut self) {
        self.holders += 1;
    }
    /// Sets the wake attribute of `pid`: `MANUAL` for Electron, `ENHANCED`
    /// otherwise. Returns when this process woke the app, which is earlier
    /// than now when it already had. Returns `None` when no recording holds a
    /// wake, the app needed none, or the request failed; a failed request is
    /// retried on the next call.
    pub fn wake(&mut self, apps: &mut impl Apps, pid: i32) -> Option<Instant> {
        if self.holders == 0 {
            return None;
        }
        if let Some((_, at)) = self.woken.iter().find(|(woken, _)| *woken == pid) {
            return Some(*at);
        }
        let attribute = if apps.electron(pid) { MANUAL } else { ENHANCED };
        // An app too busy to answer may have it on already, from VoiceOver;
        // turning it off later would take its tree away.
        if attribute == ENHANCED
            && matches!(apps.read(pid, ENHANCED), Ok(true) | Err(CANNOT_COMPLETE))
        {
            return None;
        }
        let code = apps.set(pid, attribute, true);
        if code != 0 && code != ATTRIBUTE_UNSUPPORTED {
            return None;
        }
        if attribute == ENHANCED && code == 0 {
            self.enhanced.push(pid);
        }
        let now = Instant::now();
        self.woken.push((pid, now));
        Some(now)
    }
    /// Ends a recording's hold. When none remains, turns `ENHANCED` off in the
    /// apps this process turned it on in, so a browser does not keep its full
    /// accessibility tree between recordings, and forgets them so the next
    /// recording wakes them again.
    pub fn release(&mut self, apps: &mut impl Apps) {
        self.holders = self.holders.saturating_sub(1);
        if self.holders > 0 {
            return;
        }
        for pid in std::mem::take(&mut self.enhanced) {
            apps.set(pid, ENHANCED, false);
            self.woken.retain(|(woken, _)| *woken != pid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// An app that builds its tree when it sees its wake attribute set.
    #[derive(Default)]
    struct App {
        electron: bool,
        values: HashMap<String, bool>,
        sets: Vec<(String, bool)>,
        error: i32,
        read_error: Option<i32>,
    }
    impl App {
        fn tree(&self) -> bool {
            let wake = if self.electron { MANUAL } else { ENHANCED };
            self.values.get(wake) == Some(&true)
        }
    }
    impl Apps for App {
        fn electron(&mut self, _: i32) -> bool {
            self.electron
        }
        fn read(&mut self, _: i32, attribute: &str) -> Result<bool, i32> {
            if let Some(code) = self.read_error {
                return Err(code);
            }
            self.values.get(attribute).copied().ok_or(-25212)
        }
        fn set(&mut self, _: i32, attribute: &str, value: bool) -> i32 {
            self.sets.push((attribute.into(), value));
            if self.error == 0 {
                self.values.insert(attribute.into(), value);
            }
            self.error
        }
    }

    #[test]
    fn a_chromium_browser_builds_its_tree_until_the_result_is_delivered() {
        let (mut wakes, mut brave) = (Wakes::default(), App::default());
        wakes.hold();
        assert!(wakes.wake(&mut brave, 7).is_some());
        assert!(brave.tree());
        assert!(wakes.wake(&mut brave, 7).is_some());
        assert_eq!(brave.sets, [(ENHANCED.to_string(), true)]);
        wakes.release(&mut brave);
        assert!(!brave.tree());
        wakes.hold();
        assert!(wakes.wake(&mut brave, 7).is_some());
        assert!(brave.tree());
    }

    #[test]
    fn the_tree_stays_until_the_last_overlapping_recording_ends() {
        let (mut wakes, mut brave) = (Wakes::default(), App::default());
        wakes.hold();
        wakes.wake(&mut brave, 7);
        wakes.hold();
        wakes.release(&mut brave);
        assert!(brave.tree());
        assert!(wakes.wake(&mut brave, 7).is_some());
        wakes.release(&mut brave);
        assert!(!brave.tree());
        assert_eq!(
            brave.sets,
            [(ENHANCED.to_string(), true), (ENHANCED.to_string(), false)]
        );
    }

    #[test]
    fn nothing_is_woken_outside_a_recording() {
        let (mut wakes, mut brave) = (Wakes::default(), App::default());
        assert_eq!(wakes.wake(&mut brave, 7), None);
        wakes.hold();
        wakes.release(&mut brave);
        assert_eq!(wakes.wake(&mut brave, 7), None);
        assert!(brave.sets.is_empty());
    }

    #[test]
    fn an_electron_app_is_woken_once_and_left_awake() {
        let mut wakes = Wakes::default();
        let mut code = App {
            electron: true,
            ..Default::default()
        };
        wakes.hold();
        let at = wakes.wake(&mut code, 3).unwrap();
        wakes.release(&mut code);
        wakes.hold();
        assert_eq!(wakes.wake(&mut code, 3), Some(at));
        assert!(code.tree());
        assert_eq!(code.sets, [(MANUAL.to_string(), true)]);
    }

    #[test]
    fn an_app_that_voiceover_already_enhanced_is_left_as_it_was() {
        let mut wakes = Wakes::default();
        let mut app = App::default();
        app.values.insert(ENHANCED.into(), true);
        wakes.hold();
        assert_eq!(wakes.wake(&mut app, 5), None);
        wakes.release(&mut app);
        assert!(app.sets.is_empty());
        assert!(app.tree());
    }

    #[test]
    fn an_app_too_busy_to_answer_is_not_claimed() {
        let mut wakes = Wakes::default();
        let mut app = App {
            read_error: Some(CANNOT_COMPLETE),
            ..Default::default()
        };
        app.values.insert(ENHANCED.into(), true);
        wakes.hold();
        assert_eq!(wakes.wake(&mut app, 5), None);
        app.read_error = None;
        wakes.release(&mut app);
        assert!(app.sets.is_empty());
        assert!(app.tree());
    }

    #[test]
    fn a_failed_wake_is_retried_and_an_unsupported_one_is_not() {
        let mut wakes = Wakes::default();
        let mut app = App {
            error: CANNOT_COMPLETE,
            ..Default::default()
        };
        wakes.hold();
        assert_eq!(wakes.wake(&mut app, 9), None);
        app.error = 0;
        assert!(wakes.wake(&mut app, 9).is_some());
        assert_eq!(app.sets.len(), 2);

        let mut wakes = Wakes::default();
        let mut native = App {
            error: ATTRIBUTE_UNSUPPORTED,
            ..Default::default()
        };
        wakes.hold();
        assert!(wakes.wake(&mut native, 11).is_some());
        wakes.release(&mut native);
        wakes.hold();
        assert!(wakes.wake(&mut native, 11).is_some());
        assert_eq!(native.sets, [(ENHANCED.to_string(), true)]);
    }

    #[test]
    fn electron_bundles_ship_the_electron_framework() {
        let dir = tempfile::tempdir().unwrap();
        let frameworks = dir.path().join("Contents/Frameworks");
        std::fs::create_dir_all(frameworks.join("Brave Browser Framework.framework")).unwrap();
        assert!(!electron_bundle(dir.path()));
        std::fs::create_dir_all(frameworks.join("Electron Framework.framework")).unwrap();
        assert!(electron_bundle(dir.path()));
    }
}
