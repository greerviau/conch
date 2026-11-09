use crate::delivery::Desktop;
pub struct NativeDesktop;
impl Desktop for NativeDesktop {
    type Focus = Focus;
    fn focus(&mut self) -> Option<Focus> {
        focused()
    }
    fn copy(&mut self, text: &str) -> Result<(), String> {
        arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(text))
            .map_err(|_| "Clipboard unavailable".into())
    }
    fn paste(&mut self) -> Result<(), String> {
        paste()
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[derive(PartialEq)]
pub struct Focus;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn focused() -> Option<Focus> {
    None
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
    pub fn focused() -> Option<Focus> {
        if !permission() {
            return None;
        }
        let system = Focus(unsafe { AXUIElementCreateSystemWide() });
        let element = attr(system.0, "AXFocusedUIElement")?;
        let role = string_attr(element.0, "AXRole")?;
        if !matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox") {
            return None;
        }
        if string_attr(element.0, "AXSubrole").as_deref() == Some("AXSecureTextField") {
            return None;
        }
        let mut pid = 0;
        if unsafe { AXUIElementGetPid(element.0, &mut pid) } != 0
            || pid == std::process::id() as i32
        {
            return None;
        }
        let mut settable = 0;
        let key = CFString::new("AXValue");
        if unsafe {
            AXUIElementIsAttributeSettable(
                element.0,
                key.as_concrete_TypeRef().cast(),
                &mut settable,
            )
        } != 0
            || settable == 0
        {
            return None;
        }
        Some(element)
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
use windows::{focused, paste};
#[cfg(target_os = "windows")]
pub fn permission() -> bool {
    true
}
