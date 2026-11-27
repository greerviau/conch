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

/// The accessibility calls a wake needs, by application pid.
pub trait Apps {
    fn electron(&mut self, pid: i32) -> bool;
    fn read(&mut self, pid: i32, attribute: &str) -> Option<bool>;
    /// Returns the accessibility error code, 0 on success.
    fn set(&mut self, pid: i32, attribute: &str, value: bool) -> i32;
}

/// Whether the app bundle at `bundle` is built on Electron.
pub fn electron_bundle(bundle: &Path) -> bool {
    bundle
        .join("Contents/Frameworks/Electron Framework.framework")
        .is_dir()
}

/// The apps this process woke, and those it turned `ENHANCED` on in.
#[derive(Default)]
pub struct Wakes {
    woken: Vec<(i32, Instant)>,
    enhanced: Vec<i32>,
}

impl Wakes {
    pub const fn new() -> Self {
        Self {
            woken: Vec::new(),
            enhanced: Vec::new(),
        }
    }
    /// Sets the wake attribute of `pid`: `MANUAL` for Electron, `ENHANCED`
    /// otherwise. Returns when this process woke the app, which is earlier
    /// than now when it already had. Returns `None` when the app needed no
    /// wake or the request failed; a failed request is retried on the next call.
    pub fn wake(&mut self, apps: &mut impl Apps, pid: i32) -> Option<Instant> {
        if let Some((_, at)) = self.woken.iter().find(|(woken, _)| *woken == pid) {
            return Some(*at);
        }
        let attribute = if apps.electron(pid) { MANUAL } else { ENHANCED };
        if attribute == ENHANCED && apps.read(pid, ENHANCED) == Some(true) {
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
    /// Turns `ENHANCED` off in the apps this process turned it on in, so a
    /// browser does not keep its full accessibility tree between recordings,
    /// and forgets them so the next recording wakes them again.
    pub fn release(&mut self, apps: &mut impl Apps) {
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
        fn read(&mut self, _: i32, attribute: &str) -> Option<bool> {
            self.values.get(attribute).copied()
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
        assert!(wakes.wake(&mut brave, 7).is_some());
        assert!(brave.tree());
        assert!(wakes.wake(&mut brave, 7).is_some());
        assert_eq!(brave.sets, [(ENHANCED.to_string(), true)]);
        wakes.release(&mut brave);
        assert!(!brave.tree());
        assert!(wakes.wake(&mut brave, 7).is_some());
        assert!(brave.tree());
    }

    #[test]
    fn an_electron_app_is_woken_once_and_left_awake() {
        let mut wakes = Wakes::default();
        let mut code = App {
            electron: true,
            ..Default::default()
        };
        let at = wakes.wake(&mut code, 3).unwrap();
        wakes.release(&mut code);
        assert_eq!(wakes.wake(&mut code, 3), Some(at));
        assert!(code.tree());
        assert_eq!(code.sets, [(MANUAL.to_string(), true)]);
    }

    #[test]
    fn an_app_that_voiceover_already_enhanced_is_left_as_it_was() {
        let mut wakes = Wakes::default();
        let mut app = App::default();
        app.values.insert(ENHANCED.into(), true);
        assert_eq!(wakes.wake(&mut app, 5), None);
        wakes.release(&mut app);
        assert!(app.sets.is_empty());
        assert!(app.tree());
    }

    #[test]
    fn a_failed_wake_is_retried_and_an_unsupported_one_is_not() {
        let mut wakes = Wakes::default();
        let mut app = App {
            error: -25204,
            ..Default::default()
        };
        assert_eq!(wakes.wake(&mut app, 9), None);
        app.error = 0;
        assert!(wakes.wake(&mut app, 9).is_some());
        assert_eq!(app.sets.len(), 2);

        let mut wakes = Wakes::default();
        let mut native = App {
            error: ATTRIBUTE_UNSUPPORTED,
            ..Default::default()
        };
        assert!(wakes.wake(&mut native, 11).is_some());
        wakes.release(&mut native);
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
