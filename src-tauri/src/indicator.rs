use crate::session::Phase;
use serde::Serialize;
#[derive(Clone, Serialize)]
pub struct Indicator {
    pub visible: bool,
    pub label: String,
    pub hint: String,
    pub level: f32,
    pub seconds: u64,
}
pub fn presentation(
    phase: &Phase,
    ready: bool,
    toggle: bool,
    level: f32,
    seconds: u64,
    notice: Option<&str>,
) -> Indicator {
    let (label, hint) = match phase {
        Phase::Recording if !ready => ("Opening microphone", "Waiting for audio access"),
        Phase::Recording => (
            "Listening",
            if toggle {
                "Press shortcut to finish"
            } else {
                "Release shortcut to finish"
            },
        ),
        Phase::Processing => ("Preparing text", "You can keep working"),
        Phase::Idle => (notice.unwrap_or(""), ""),
    };
    Indicator {
        visible: *phase != Phase::Idle || notice.is_some(),
        label: label.into(),
        hint: hint.into(),
        level: if *phase == Phase::Recording {
            level.clamp(0.0, 1.0)
        } else {
            0.0
        },
        seconds,
    }
}

pub fn state(app: &tauri::AppHandle) -> Indicator {
    use tauri::Manager;
    let state = app.state::<crate::app::AppState>();
    let inner = state.inner.lock().unwrap();
    let notice = inner
        .indicator_notice
        .as_ref()
        .filter(|(time, _)| time.elapsed() < std::time::Duration::from_secs(3))
        .map(|(_, text)| text.as_str());
    let mut value = presentation(
        &inner.session.phase,
        inner.audio_ready,
        inner.settings.activation == "toggle",
        f32::from_bits(inner.level.load(std::sync::atomic::Ordering::Relaxed)),
        if inner.session.phase == Phase::Recording {
            inner
                .recording_since
                .map(|at| at.elapsed().as_secs())
                .unwrap_or(0)
        } else {
            inner.recorded_seconds
        },
        notice,
    );
    value.visible &= inner.settings.show_overlay;
    value
}
pub fn placement(
    screen: tauri::PhysicalRect<i32, u32>,
    work_area: tauri::PhysicalRect<i32, u32>,
    scale: f64,
) -> tauri::PhysicalRect<i32, u32> {
    let area = if work_area.size.width == 0 || work_area.size.height == 0 {
        screen
    } else {
        work_area
    };
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let width = ((320.0 * scale).round() as u32).min(area.size.width);
    let height = ((68.0 * scale).round() as u32).min(area.size.height);
    let margin = ((24.0 * scale).round() as u32).min(area.size.height.saturating_sub(height));
    tauri::PhysicalRect {
        position: tauri::PhysicalPosition::new(
            area.position.x + ((area.size.width - width) / 2) as i32,
            area.position.y + (area.size.height - height - margin) as i32,
        ),
        size: tauri::PhysicalSize::new(width, height),
    }
}
pub fn setup(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let window = WebviewWindowBuilder::new(
        app,
        "recording",
        WebviewUrl::App("index.html?overlay".into()),
    )
    .title("Conch recording")
    .inner_size(320.0, 68.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focusable(false)
    .focused(false)
    .visible(false)
    .shadow(false)
    .build()
    .map_err(|_| "Cannot create recording indicator")?;
    window
        .set_ignore_cursor_events(true)
        .map_err(|_| "Cannot configure recording indicator")?;
    let app = app.clone();
    std::thread::spawn(move || {
        use tauri::Emitter;
        let mut was_visible = false;
        let mut last_placement = None;
        loop {
            let value = state(&app);
            if value.visible {
                let monitor = if was_visible {
                    window.current_monitor().ok().flatten()
                } else {
                    app.cursor_position()
                        .ok()
                        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
                }
                .or_else(|| app.primary_monitor().ok().flatten());
                if let Some(monitor) = monitor {
                    let bounds = placement(
                        tauri::PhysicalRect {
                            position: *monitor.position(),
                            size: *monitor.size(),
                        },
                        *monitor.work_area(),
                        monitor.scale_factor(),
                    );
                    let key = (
                        bounds.position.x,
                        bounds.position.y,
                        bounds.size.width,
                        bounds.size.height,
                    );
                    if last_placement != Some(key) {
                        let _ = window.set_position(bounds.position);
                        let _ = window.set_size(tauri::LogicalSize::new(
                            bounds.size.width as f64 / monitor.scale_factor(),
                            bounds.size.height as f64 / monitor.scale_factor(),
                        ));
                        last_placement = Some(key);
                    }
                }
            }
            if value.visible != was_visible {
                let window = window.clone();
                let visible = value.visible;
                let _ = app.run_on_main_thread(move || {
                    #[cfg(target_os = "macos")]
                    if let Ok(native) = window.ns_window() {
                        unsafe {
                            crate::platform::conch_overlay_visibility(native, visible);
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = if visible {
                            window.show()
                        } else {
                            window.hide()
                        };
                    }
                });
                was_visible = visible;
                if !visible {
                    last_placement = None;
                }
            }
            if value.visible {
                let _ = app.emit_to("recording", "recording-state", value);
            }
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn area(x: i32, y: i32, width: u32, height: u32) -> tauri::PhysicalRect<i32, u32> {
        tauri::PhysicalRect {
            position: (x, y).into(),
            size: (width, height).into(),
        }
    }
    #[test]
    fn popup_stays_above_bottom_dock_and_inside_work_area() {
        let screen = area(0, 0, 1440, 900);
        let work = area(0, 25, 1440, 789);
        let popup = placement(screen, work, 1.0);
        assert_eq!(popup.position.y + popup.size.height as i32, 790);
        assert!(popup.position.y >= work.position.y);
    }
    #[test]
    fn popup_handles_retina_negative_origins_and_side_docks() {
        let screen = area(-2880, -1800, 2880, 1800);
        let work = area(-2700, -1750, 2700, 1600);
        let popup = placement(screen, work, 2.0);
        assert_eq!(popup.position.y + popup.size.height as i32, -198);
        assert_eq!(popup.position.x, -1670);
        assert!(popup.position.x >= work.position.x);
    }
    #[test]
    fn popup_clamps_to_small_fractionally_scaled_work_areas() {
        let screen = area(0, 0, 640, 480);
        let work = area(200, 20, 300, 90);
        let popup = placement(screen, work, 1.25);
        assert!(popup.position.x >= work.position.x);
        assert!(popup.position.x as i64 + popup.size.width as i64 <= 500);
        assert!(popup.position.y as i64 + popup.size.height as i64 <= 110);
    }
    #[test]
    fn recording_and_processing_remain_visible_without_text_or_controls() {
        let p = presentation(&Phase::Recording, true, false, 0.5, 12, None);
        assert!(p.visible);
        assert_eq!(p.label, "Listening");
        assert_eq!(p.hint, "Release shortcut to finish");
        assert_eq!(p.seconds, 12);
        let p = presentation(&Phase::Processing, true, false, 0.0, 12, None);
        assert!(p.visible);
        assert_eq!(p.label, "Preparing text");
        assert!(!presentation(&Phase::Idle, false, false, 0.0, 0, None).visible);
    }
    #[test]
    fn toggle_and_microphone_startup_have_different_instructions() {
        assert_eq!(
            presentation(&Phase::Recording, false, false, 0.0, 0, None).label,
            "Opening microphone"
        );
        assert_eq!(
            presentation(&Phase::Recording, true, true, 0.0, 0, None).hint,
            "Press shortcut to finish"
        );
    }
}
