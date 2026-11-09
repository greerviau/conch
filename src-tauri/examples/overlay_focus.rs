#[cfg(target_os = "macos")]
fn main() {
    let _ = conch_lib::platform::permission();
    unsafe extern "C" {
        fn conch_overlay_focus_test() -> i32;
        fn conch_screen_geometry(values: *mut f64) -> i32;
        fn conch_overlay_bounds_test(x: f64, y: f64, width: f64, height: f64) -> i32;
    }
    assert_eq!(
        unsafe { conch_overlay_focus_test() },
        0,
        "Indicator changed the focused app or accepted focus"
    );
    let mut geometry = [0.0; 9];
    assert_eq!(unsafe { conch_screen_geometry(geometry.as_mut_ptr()) }, 0);
    let scale = geometry[8];
    let screen = tauri::PhysicalRect {
        position: (0, 0).into(),
        size: (
            (geometry[2] * scale).round() as u32,
            (geometry[3] * scale).round() as u32,
        )
            .into(),
    };
    let work = tauri::PhysicalRect {
        position: (
            ((geometry[4] - geometry[0]) * scale).round() as i32,
            ((geometry[1] + geometry[3] - geometry[5] - geometry[7]) * scale).round() as i32,
        )
            .into(),
        size: (
            (geometry[6] * scale).round() as u32,
            (geometry[7] * scale).round() as u32,
        )
            .into(),
    };
    let bounds = conch_lib::indicator::placement(screen, work, scale);
    assert_eq!(
        unsafe {
            conch_overlay_bounds_test(
                bounds.position.x as f64,
                bounds.position.y as f64,
                bounds.size.width as f64,
                bounds.size.height as f64,
            )
        },
        0,
        "Overlay is outside the usable screen area or overlaps the Dock clearance"
    );
    println!("Overlay fits the native work area with 24-point Dock clearance");
    println!("Overlay appeared, ignored pointer events, and preserved the foreground application");
}
#[cfg(not(target_os = "macos"))]
fn main() {
    println!("The native focus experiment requires macOS");
}
