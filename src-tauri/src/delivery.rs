pub trait Desktop {
    type Focus: PartialEq;
    fn focus(&mut self) -> Option<Self::Focus>;
    fn copy(&mut self, text: &str) -> Result<(), String>;
    fn fallback_reason(&self) -> &str {
        "Focused input is unsupported or permission is missing"
    }
    fn paste(&mut self) -> Result<(), String>;
}
pub fn deliver<D: Desktop>(desktop: &mut D, text: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok("Nothing remains; clipboard unchanged".into());
    }
    let focus = desktop.focus();
    desktop.copy(text)?;
    if let Some(before) = focus {
        if desktop.focus() == Some(before) {
            return Ok(match desktop.paste() {
                Ok(()) => "Copied; paste requested once (insertion is not confirmed)".into(),
                Err(_) => "Clipboard fallback: paste failed".into(),
            });
        }
        return Ok("Clipboard fallback: focus changed".into());
    }
    Ok(format!("Clipboard fallback: {}", desktop.fallback_reason()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        focus: Vec<Option<u32>>,
        copies: usize,
        pastes: usize,
        fail: bool,
    }
    impl Desktop for Fake {
        type Focus = u32;
        fn focus(&mut self) -> Option<u32> {
            self.focus.remove(0)
        }
        fn copy(&mut self, _: &str) -> Result<(), String> {
            self.copies += 1;
            Ok(())
        }
        fn paste(&mut self) -> Result<(), String> {
            self.pastes += 1;
            if self.fail {
                Err("failed".into())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn copies_and_pastes_once_only_if_same_eligible_focus() {
        let mut d = Fake {
            focus: vec![Some(1), Some(1)],
            ..Default::default()
        };
        deliver(&mut d, "hello").unwrap();
        assert_eq!((d.copies, d.pastes), (1, 1));
        let mut d = Fake {
            focus: vec![Some(1), Some(2)],
            ..Default::default()
        };
        deliver(&mut d, "hello").unwrap();
        assert_eq!((d.copies, d.pastes), (1, 0));
        let mut d = Fake {
            focus: vec![None],
            ..Default::default()
        };
        deliver(&mut d, "hello").unwrap();
        assert_eq!((d.copies, d.pastes), (1, 0));
    }
    #[test]
    fn empty_results_leave_clipboard_unchanged_and_failed_pastes_are_not_retried() {
        let mut d = Fake::default();
        deliver(&mut d, "").unwrap();
        assert_eq!(d.copies, 0);
        let mut d = Fake {
            focus: vec![Some(1), Some(1)],
            fail: true,
            ..Default::default()
        };
        assert!(deliver(&mut d, "text").unwrap().contains("Clipboard"));
        assert_eq!(d.pastes, 1);
    }
}
