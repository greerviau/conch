use windows::Win32::{
    System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    },
    UI::{
        Accessibility::{CUIAutomation, IUIAutomation, UIA_EditControlTypeId},
        Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_CONTROL,
            VK_V,
        },
    },
};
#[derive(PartialEq)]
pub struct Focus(Vec<i32>);
struct Apartment;
impl Apartment {
    fn new() -> Option<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
        }
        Some(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
pub fn focused() -> Option<Focus> {
    let _apartment = Apartment::new()?;
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let element = automation.GetFocusedElement().ok()?;
        if element.CurrentProcessId().ok()? == std::process::id() as i32
            || !element.CurrentIsEnabled().ok()?.as_bool()
            || element.CurrentIsPassword().ok()?.as_bool()
            || element.CurrentControlType().ok()? != UIA_EditControlTypeId
        {
            return None;
        }
        let value: windows::Win32::UI::Accessibility::IUIAutomationValuePattern = element
            .GetCurrentPatternAs(windows::Win32::UI::Accessibility::UIA_ValuePatternId)
            .ok()?;
        if value.CurrentIsReadOnly().ok()?.as_bool() {
            return None;
        }
        let array = element.GetRuntimeId().ok()?;
        use windows::Win32::System::Ole::{
            SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
            SafeArrayUnaccessData,
        };
        let low = SafeArrayGetLBound(array, 1).ok()?;
        let high = SafeArrayGetUBound(array, 1).ok()?;
        if high < low || high - low > 128 {
            let _ = SafeArrayDestroy(array);
            return None;
        }
        let mut data = std::ptr::null_mut();
        SafeArrayAccessData(array, &mut data).ok()?;
        let identity =
            std::slice::from_raw_parts(data.cast::<i32>(), (high - low + 1) as usize).to_vec();
        let _ = SafeArrayUnaccessData(array);
        let _ = SafeArrayDestroy(array);
        Some(Focus(identity))
    }
}
pub fn paste() -> Result<(), String> {
    let key = |vk, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let keys = [
        key(VK_CONTROL, Default::default()),
        key(VK_V, Default::default()),
        key(VK_V, KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    if unsafe { SendInput(&keys, std::mem::size_of::<INPUT>() as i32) } != keys.len() as u32 {
        return Err("Paste event was rejected".into());
    }
    Ok(())
}
