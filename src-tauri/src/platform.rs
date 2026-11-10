use crate::delivery::Desktop;
#[derive(Default)]
pub struct NativeDesktop {
    reason: String,
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
        &self.reason
    }
    fn paste(&mut self) -> Result<(), String> {
        paste()
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(PartialEq)]
pub struct Focus;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn focused() -> Result<Focus, String> {
    Err("Automatic paste is unavailable on this desktop; paste the copied text manually".into())
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
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementSetAttributeValue(
            element: CFTypeRef,
            name: *const c_void,
            value: CFTypeRef,
        ) -> i32;
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
    fn attr_result(element: CFTypeRef, key: &str) -> Result<Focus, i32> {
        let mut value = std::ptr::null();
        let key = CFString::new(key);
        let code = unsafe {
            AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef().cast(), &mut value)
        };
        if code == 0 && !value.is_null() {
            Ok(Focus(value))
        } else {
            Err(if code == 0 { -25212 } else { code })
        }
    }
    fn attr(element: CFTypeRef, key: &str) -> Option<Focus> {
        attr_result(element, key).ok()
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
    const ATTRIBUTE_UNSUPPORTED: i32 = -25205;
    /// How long a freshly woken Electron app gets to build its tree.
    const WAKE_GRACE: std::time::Duration = std::time::Duration::from_secs(2);
    static WOKEN: std::sync::Mutex<Vec<(i32, std::time::Instant)>> =
        std::sync::Mutex::new(Vec::new());
    unsafe extern "C" {
        fn conch_frontmost_pid() -> i32;
        fn conch_frontmost_name(buffer: *mut u8, size: i32);
    }
    /// The frontmost application, from the window server rather than the
    /// accessibility tree. The system-wide accessibility element cannot name the
    /// focused application of an Electron app that has not built its tree.
    fn frontmost() -> Option<i32> {
        let pid = unsafe { conch_frontmost_pid() };
        (pid > 0 && pid != std::process::id() as i32).then_some(pid)
    }
    /// Electron and Chromium build no accessibility tree for a process until an
    /// assistive client sets AXManualAccessibility on its application element.
    /// Returns when this process woke the application, if it has not already.
    /// A failed request is retried on the next call.
    fn wake_application(pid: i32) -> Option<std::time::Instant> {
        use core_foundation::boolean::CFBoolean;
        let mut woken = WOKEN.lock().unwrap();
        if let Some((_, at)) = woken.iter().find(|(woken_pid, _)| *woken_pid == pid) {
            return Some(*at);
        }
        let element = Focus(unsafe { AXUIElementCreateApplication(pid) });
        let key = CFString::new("AXManualAccessibility");
        let code = unsafe {
            AXUIElementSetAttributeValue(
                element.0,
                key.as_concrete_TypeRef().cast(),
                CFBoolean::true_value().as_concrete_TypeRef().cast(),
            )
        };
        if code != 0 && code != ATTRIBUTE_UNSUPPORTED {
            return None;
        }
        let now = std::time::Instant::now();
        woken.push((pid, now));
        Some(now)
    }
    /// "Code, pid 123" for the frontmost application, for failure messages.
    fn focused_application() -> String {
        let mut name = [0u8; 128];
        unsafe { conch_frontmost_name(name.as_mut_ptr(), name.len() as i32) };
        let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
        let name = String::from_utf8_lossy(&name[..end]);
        let pid = unsafe { conch_frontmost_pid() };
        format!(
            "{}, pid {pid}",
            if name.is_empty() { "unnamed" } else { &name }
        )
    }
    /// Called when recording starts so the target app has built its tree by the
    /// time the transcript is ready.
    pub fn prepare() {
        if permission()
            && let Some(pid) = frontmost()
        {
            wake_application(pid);
        }
    }
    pub fn focused() -> Result<Focus, String> {
        if !permission() {
            return Err(
                "Enable Conch in Privacy & Security / Accessibility to paste automatically".into(),
            );
        }
        let pid = frontmost().ok_or("The focused app is Conch or cannot be identified")?;
        let woke = wake_application(pid);
        let application = Focus(unsafe { AXUIElementCreateApplication(pid) });
        let system = Focus(unsafe { AXUIElementCreateSystemWide() });
        for element in [&application, &system] {
            unsafe {
                AXUIElementSetMessagingTimeout(element.0, 0.5);
            }
        }
        let deadline = std::time::Instant::now()
            .max(woke.map_or(std::time::Instant::now(), |at| at + WAKE_GRACE))
            + std::time::Duration::from_millis(250);
        let element = loop {
            match attr_result(application.0, "AXFocusedUIElement")
                .or_else(|_| attr_result(system.0, "AXFocusedUIElement"))
            {
                Ok(element) => break element,
                Err(code) if std::time::Instant::now() >= deadline => {
                    return Err(format!(
                        "The focused app ({}) does not expose its text input (accessibility error {code})",
                        focused_application()
                    ));
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        };
        unsafe {
            AXUIElementSetMessagingTimeout(element.0, 0.25);
        }
        let role = string_attr(element.0, "AXRole")
            .ok_or("The focused element has no accessible text role")?;
        let subrole = string_attr(element.0, "AXSubrole").unwrap_or_default();
        if subrole == "AXSecureTextField" {
            return Err("Secure fields receive clipboard output only".into());
        }
        let mut pid = 0;
        if unsafe { AXUIElementGetPid(element.0, &mut pid) } != 0 {
            return Err("Cannot identify the focused application".into());
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
            selected_range_settable: settable("AXSelectedTextRange"),
            web_editable: attr(element.0, "AXEditableAncestor").is_some()
                || attr(element.0, "AXHighestEditableAncestor").is_some(),
        };
        if !super::eligible_input(&input) {
            return Err(format!(
                "The focused control in {} is not an editable input (role {role}, subrole {}, value settable {}, selection settable {}, range settable {}, web editable {}); paste the copied text manually",
                focused_application(),
                if subrole.is_empty() { "none" } else { &subrole },
                input.value_settable,
                input.selected_text_settable,
                input.selected_range_settable,
                input.web_editable,
            ));
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
pub use macos::{Focus, permission, prepare};
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
fn focused() -> Result<Focus, String> {
    windows::focused()
        .ok_or_else(|| "The focused control does not expose a writable UI Automation input".into())
}

#[cfg(target_os = "windows")]
pub fn permission() -> bool {
    true
}
#[cfg(not(target_os = "macos"))]
pub fn prepare() {}

#[derive(Default)]
pub struct InputEvidence<'a> {
    pub role: &'a str,
    pub subrole: &'a str,
    pub enabled: Option<bool>,
    pub own_process: bool,
    pub value_settable: bool,
    pub selected_text_settable: bool,
    pub selected_range_settable: bool,
    pub web_editable: bool,
}
pub fn eligible_input(input: &InputEvidence<'_>) -> bool {
    matches!(input.role, "AXTextField" | "AXTextArea" | "AXComboBox")
        && input.subrole != "AXSecureTextField"
        && input.enabled != Some(false)
        && !input.own_process
        && (input.value_settable
            || input.selected_text_settable
            || (input.web_editable && input.selected_range_settable))
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
    {
        unsafe { conch_request_accessibility() };
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("This desktop supports clipboard output; paste manually after recording".into())
    }
}

pub fn installation_path() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let exe = std::env::current_exe().ok()?;
        let bundle = exe.parent()?.parent()?.parent()?;
        if bundle
            .extension()
            .is_some_and(|extension| extension == "app")
        {
            return Some(bundle.to_string_lossy().into_owned());
        }
    }
    None
}
pub fn repair_accessibility() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::accessibility::repair_with(
            |program, args| {
                let status = std::process::Command::new(program)
                    .args(args)
                    .output()
                    .map_err(|_| "Cannot start the macOS permission repair")?;
                if !status.status.success() {
                    return Err("macOS could not remove Conch's Accessibility entry. Remove it in Settings, then add the current copy again".into());
                }
                Ok(())
            },
            request_accessibility,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Accessibility entry repair is available on macOS only".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn web_contenteditable_accepts_editable_range_without_value_setter() {
        assert!(eligible_input(&InputEvidence {
            role: "AXTextArea",
            enabled: Some(true),
            web_editable: true,
            selected_range_settable: true,
            ..Default::default()
        }));
    }
    #[test]
    fn selectable_readonly_native_text_is_not_enough_to_allow_paste() {
        assert!(!eligible_input(&InputEvidence {
            role: "AXTextArea",
            enabled: Some(true),
            selected_range_settable: true,
            ..Default::default()
        }));
    }
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
