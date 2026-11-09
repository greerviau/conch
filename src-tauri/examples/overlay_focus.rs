#[cfg(target_os = "macos")]
fn main() {
    let _ = conch_lib::platform::permission();
    unsafe extern "C" {
        fn conch_overlay_focus_test() -> i32;
    }
    assert_eq!(
        unsafe { conch_overlay_focus_test() },
        0,
        "Indicator changed the focused app or accepted focus"
    );
    println!("Overlay appeared, ignored pointer events, and preserved the foreground application");
}
#[cfg(not(target_os = "macos"))]
fn main() {
    println!("The native focus experiment requires macOS");
}
