use tauri_plugin_global_shortcut::Shortcut;
pub trait Registry {
    fn registered(&self, key: Shortcut) -> bool;
    fn register(&mut self, key: Shortcut) -> Result<(), String>;
    fn unregister(&mut self, key: Shortcut) -> Result<(), String>;
}
pub fn replace(
    registry: &mut impl Registry,
    old: &str,
    new: &str,
    save: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let new: Shortcut = new.parse().map_err(|_| "Invalid shortcut")?;
    let old = old.parse::<Shortcut>().ok();
    let already_registered = registry.registered(new);
    let remove_old = old.filter(|key| key.id() != new.id() && registry.registered(*key));
    if !already_registered {
        registry.register(new)?;
    }
    if let Some(old) = remove_old
        && let Err(error) = registry.unregister(old)
    {
        if !already_registered {
            let _ = registry.unregister(new);
        }
        return Err(error);
    }
    if let Err(error) = save() {
        let restore = remove_old.map(|old| registry.register(old)).transpose();
        let cleanup = if !already_registered {
            registry.unregister(new)
        } else {
            Ok(())
        };
        if restore.is_err() || cleanup.is_err() {
            return Err(format!(
                "{error}. Shortcut rollback failed; choose a new shortcut"
            ));
        }
        return Err(error);
    }
    Ok(())
}
pub fn pause(registry: &mut impl Registry, value: &str) -> Result<Option<Shortcut>, String> {
    let key = value
        .parse::<Shortcut>()
        .ok()
        .filter(|key| registry.registered(*key));
    if let Some(key) = key {
        registry.unregister(key)?;
    }
    Ok(key)
}
pub fn resume(registry: &mut impl Registry, key: Option<Shortcut>) -> Result<(), String> {
    if let Some(key) = key.filter(|key| !registry.registered(*key)) {
        registry.register(key)?;
    }
    Ok(())
}
pub struct NativeRegistry<'a>(pub &'a tauri::AppHandle);
impl Registry for NativeRegistry<'_> {
    fn registered(&self, key: Shortcut) -> bool {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        self.0.global_shortcut().is_registered(key)
    }
    fn register(&mut self, key: Shortcut) -> Result<(), String> {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        self.0.global_shortcut().register(key).map_err(|_| {
            "Shortcut is in use or cannot be registered. Choose another combination".into()
        })
    }
    fn unregister(&mut self, key: Shortcut) -> Result<(), String> {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        self.0
            .global_shortcut()
            .unregister(key)
            .map_err(|_| "Cannot unregister the previous shortcut".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    #[derive(Default)]
    struct Fake {
        keys: HashSet<u32>,
        fail_register: bool,
    }
    impl Registry for Fake {
        fn registered(&self, key: Shortcut) -> bool {
            self.keys.contains(&key.id())
        }
        fn register(&mut self, key: Shortcut) -> Result<(), String> {
            if self.fail_register || !self.keys.insert(key.id()) {
                return Err("Already registered".into());
            }
            Ok(())
        }
        fn unregister(&mut self, key: Shortcut) -> Result<(), String> {
            self.keys.remove(&key.id());
            Ok(())
        }
    }
    fn key(value: &str) -> Shortcut {
        value.parse().unwrap()
    }
    #[test]
    fn equivalent_spellings_do_not_unregister_the_active_shortcut() {
        let old = "Control+Shift+KeyK";
        let new = "Shift+Control+K";
        let mut registry = Fake::default();
        registry.register(key(old)).unwrap();
        replace(&mut registry, old, new, || Ok(())).unwrap();
        assert!(registry.registered(key(new)));
        assert_eq!(registry.keys.len(), 1);
    }
    #[test]
    fn failed_persistence_restores_only_the_previous_shortcut() {
        let mut registry = Fake::default();
        let old = "Control+KeyK";
        let new = "Control+KeyJ";
        registry.register(key(old)).unwrap();
        assert!(replace(&mut registry, old, new, || Err("Cannot save".into())).is_err());
        assert!(registry.registered(key(old)));
        assert!(!registry.registered(key(new)));
    }
    #[test]
    fn unavailable_old_shortcut_is_not_restored_when_capture_ends() {
        let mut registry = Fake::default();
        let paused = pause(&mut registry, "Control+KeyK").unwrap();
        assert!(paused.is_none());
        resume(&mut registry, paused).unwrap();
        assert!(registry.keys.is_empty());
    }
    #[test]
    fn replacement_registers_only_new_shortcut_and_saves_once() {
        let mut registry = Fake::default();
        let old = "Control+KeyK";
        let new = "Control+KeyJ";
        registry.register(key(old)).unwrap();
        let saved = std::cell::Cell::new(false);
        replace(&mut registry, old, new, || {
            saved.set(true);
            Ok(())
        })
        .unwrap();
        assert!(saved.get());
        assert!(!registry.registered(key(old)));
        assert!(registry.registered(key(new)));
    }
}
