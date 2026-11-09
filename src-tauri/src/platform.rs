use crate::delivery::Desktop;
#[derive(Default)]
pub struct NativeDesktop {
    reason: &'static str,
}
impl Desktop for NativeDesktop {
    type Focus = Focus;
    fn focus(&mut self) -> Option<Focus> {
        match focused() {
            Ok(focus) => Some(focus),
            Err(reason) => {
                self.reason = reason;
                None
            }
        }
    }
    fn copy(&mut self, text: &str) -> Result<(), String> {
        static CLIPBOARD: std::sync::Mutex<Option<arboard::Clipboard>> =
            std::sync::Mutex::new(None);
        let mut clipboard = CLIPBOARD.lock().map_err(|_| "Clipboard unavailable")?;
        if clipboard.is_none() {
            *clipboard = Some(arboard::Clipboard::new().map_err(|_| "Clipboard unavailable")?);
        }
        clipboard
            .as_mut()
            .unwrap()
            .set_text(text)
            .map_err(|_| "Clipboard unavailable".into())
    }
    fn fallback_reason(&self) -> &str {
        self.reason
    }
    fn paste(&mut self) -> Result<(), String> {
        paste()
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(PartialEq)]
pub struct Focus;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn focused() -> Result<Focus, &'static str> {
    Err("Automatic paste is unavailable on this desktop; paste the copied text manually")
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn paste() -> Result<(), String> {
    Err("Automatic paste is unavailable on this desktop".into())
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn permission() -> bool {
    false
}

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::{
        base::{CFEqual, CFRelease, CFTypeRef, TCFType},
        string::CFString,
    };
    use std::ffi::c_void;
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            name: *const c_void,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementIsAttributeSettable(
            element: CFTypeRef,
            name: *const c_void,
            settable: *mut u8,
        ) -> i32;
        fn AXUIElementGetPid(element: CFTypeRef, pid: *mut i32) -> i32;
        fn AXIsProcessTrusted() -> bool;
        fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
    }
    pub struct Focus(CFTypeRef);
    impl Drop for Focus {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.0);
            }
        }
    }
    impl PartialEq for Focus {
        fn eq(&self, other: &Self) -> bool {
            unsafe { CFEqual(self.0, other.0) != 0 }
        }
    }
    fn attr(element: CFTypeRef, key: &str) -> Option<Focus> {
        let mut value = std::ptr::null();
        let key = CFString::new(key);
        let code = unsafe {
            AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef().cast(), &mut value)
        };
        if code == 0 && !value.is_null() {
            Some(Focus(value))
        } else {
            None
        }
    }
    fn string_attr(element: CFTypeRef, key: &str) -> Option<String> {
        let value = attr(element, key)?;
        use core_foundation::base::CFGetTypeID;
        if unsafe { CFGetTypeID(value.0) } != CFString::type_id() {
            return None;
        }
        Some(unsafe { CFString::wrap_under_get_rule(value.0.cast()) }.to_string())
    }
    pub fn permission() -> bool {
        unsafe { AXIsProcessTrusted() }
    }
    pub fn focused() -> Result<Focus, &'static str> {
        if !permission() {
            return Err(
                "Enable Conch in Privacy & Security / Accessibility to paste automatically",
            );
        }
        let system = Focus(unsafe { AXUIElementCreateSystemWide() });
        unsafe {
            AXUIElementSetMessagingTimeout(system.0, 0.25);
        }

        let element = attr(system.0, "AXFocusedUIElement")
            .ok_or("The focused app does not expose its text input")?;
        unsafe {
            AXUIElementSetMessagingTimeout(element.0, 0.25);
        }
        let role = string_attr(element.0, "AXRole")
            .ok_or("The focused element has no accessible text role")?;
        let subrole = string_attr(element.0, "AXSubrole").unwrap_or_default();
        if subrole == "AXSecureTextField" {
            return Err("Secure fields receive clipboard output only");
        }
        let mut pid = 0;
        if unsafe { AXUIElementGetPid(element.0, &mut pid) } != 0 {
            return Err("Cannot identify the focused application");
        }
        let enabled = attr(element.0, "AXEnabled").and_then(|value| {
            use core_foundation::{base::CFGetTypeID, boolean::CFBoolean};
            if unsafe { CFGetTypeID(value.0) } != CFBoolean::type_id() {
                return None;
            }
            Some(unsafe { CFBoolean::wrap_under_get_rule(value.0.cast()) }.into())
        });
        let settable = |name: &str| {
            let mut value = 0;
            let key = CFString::new(name);
            unsafe {
                AXUIElementIsAttributeSettable(
                    element.0,
                    key.as_concrete_TypeRef().cast(),
                    &mut value,
                ) == 0
                    && value != 0
            }
        };
        let input = super::InputEvidence {
            role: &role,
            subrole: &subrole,
            enabled,
            own_process: pid == std::process::id() as i32,
            value_settable: settable("AXValue"),
            selected_text_settable: settable("AXSelectedText"),
        };
        if !super::eligible_input(&input) {
            return Err(
                "The focused control is not an enabled editable input; paste the copied text manually",
            );
        }
        Ok(element)
    }
    pub fn paste() -> Result<(), String> {
        use core_graphics::{
            event::{CGEvent, CGEventFlags, CGEventTapLocation},
            event_source::{CGEventSource, CGEventSourceStateID},
        };
        let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
            .map_err(|_| "Cannot create paste event")?;
        let down = CGEvent::new_keyboard_event(source.clone(), 9, true)
            .map_err(|_| "Cannot create paste event")?;
        let up = CGEvent::new_keyboard_event(source, 9, false)
            .map_err(|_| "Cannot create paste event")?;
        down.set_flags(CGEventFlags::CGEventFlagCommand);
        up.set_flags(CGEventFlags::CGEventFlagCommand);
        down.post(CGEventTapLocation::HID);
        up.post(CGEventTapLocation::HID);
        Ok(())
    }
}
#[cfg(target_os = "macos")]
pub use macos::{Focus, permission};
#[cfg(target_os = "macos")]
use macos::{focused, paste};

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::Focus;
#[cfg(target_os = "windows")]
use windows::paste;
#[cfg(target_os = "windows")]
fn focused() -> Result<Focus, &'static str> {
    windows::focused().ok_or("The focused control does not expose a writable UI Automation input")
}

#[cfg(target_os = "windows")]
pub fn permission() -> bool {
    true
}

#[derive(Default)]
pub struct InputEvidence<'a> {
    pub role: &'a str,
    pub subrole: &'a str,
    pub enabled: Option<bool>,
    pub own_process: bool,
    pub value_settable: bool,
    pub selected_text_settable: bool,
}
pub fn eligible_input(input: &InputEvidence<'_>) -> bool {
    matches!(input.role, "AXTextField" | "AXTextArea" | "AXComboBox")
        && input.subrole != "AXSecureTextField"
        && input.enabled != Some(false)
        && !input.own_process
        && (input.value_settable || input.selected_text_settable)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_text_replacement_when_whole_value_setter_is_unavailable() {
        assert!(eligible_input(&InputEvidence {
            role: "AXTextArea",
            selected_text_settable: true,
            ..Default::default()
        }));
    }
    #[test]
    fn excludes_secure_disabled_readonly_and_own_inputs() {
        let mut input = InputEvidence {
            role: "AXTextField",
            value_settable: true,
            ..Default::default()
        };
        assert!(eligible_input(&input));
        input.subrole = "AXSecureTextField";
        assert!(!eligible_input(&input));
        input.subrole = "";
        input.enabled = Some(false);
        assert!(!eligible_input(&input));
        input.enabled = Some(true);
        input.own_process = true;
        assert!(!eligible_input(&input));
        input.own_process = false;
        input.value_settable = false;
        assert!(!eligible_input(&input));
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn conch_microphone_status() -> i32;
    fn conch_request_microphone() -> i32;
    fn conch_request_accessibility();
    fn conch_open_microphone_settings();
    pub fn conch_overlay_visibility(window: *mut std::ffi::c_void, visible: bool);
}
pub fn microphone_status() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        match unsafe { conch_microphone_status() } {
            3 => "granted",
            1 | 2 => "denied",
            _ => "unknown",
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        "system"
    }
}
pub fn request_microphone() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let result = unsafe { conch_request_microphone() };
        if result == 1 || result == 2 {
            unsafe { conch_open_microphone_settings() };
            return Err(
                "Allow Conch in Microphone privacy settings, then try recording again".into(),
            );
        }
    }
    Ok(())
}
pub fn request_accessibility() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    unsafe {
        conch_request_accessibility();
    }
    #[cfg(target_os = "windows")]
    {
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        return Err(
            "This desktop supports clipboard output; paste manually after recording".into(),
        );
    }
    #[cfg(target_os = "macos")]
    Ok(())
}
