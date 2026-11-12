use crate::lock::Locked;
use crate::session::Phase;
use serde::Serialize;
#[derive(Clone, Serialize)]
pub struct Indicator {
    pub visible: bool,
    /// "recording", "processing", or "notice" (a delivery outcome or error).
    pub mode: String,
    pub label: String,
    pub hint: String,
    pub level: f32,
    pub seconds: u64,
    /// The active profile's name while recording with more than one profile, so
    /// it can be switched; empty otherwise.
    pub profile: String,
    /// The profiles to switch to, while the list is open.
    pub profiles: Vec<ProfileChoice>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProfileChoice {
    pub id: String,
    pub name: String,
    pub active: bool,
    /// Whether its recognition model is installed. A recording cannot wait for a download.
    pub available: bool,
}
impl Indicator {
    /// Whether the indicator takes clicks. Otherwise clicks pass through to the app below.
    pub fn interactive(&self) -> bool {
        self.visible && self.mode == "recording" && !self.profile.is_empty()
    }
}
/// The profile name and the open list for the indicator. Only a recording with
/// more than one profile shows them.
pub fn profiles(
    phase: &Phase,
    settings: &crate::storage::Settings,
    menu_open: bool,
    installed: impl Fn(&str) -> bool,
) -> (String, Vec<ProfileChoice>) {
    if *phase != Phase::Recording || settings.profiles.len() < 2 {
        return (String::new(), vec![]);
    }
    let active = settings.active();
    let list = if menu_open {
        settings
            .profiles
            .iter()
            .map(|p| ProfileChoice {
                id: p.id.clone(),
                name: p.name.clone(),
                active: p.id == active.id,
                available: p.id == active.id || installed(&p.recognizer),
            })
            .collect()
    } else {
        vec![]
    };
    (active.name.clone(), list)
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
        mode: match phase {
            Phase::Recording => "recording",
            Phase::Processing => "processing",
            Phase::Idle => "notice",
        }
        .into(),
        label: label.into(),
        hint: hint.into(),
        level: if *phase == Phase::Recording {
            level.clamp(0.0, 1.0)
        } else {
            0.0
        },
        seconds,
        profile: String::new(),
        profiles: vec![],
    }
}

pub fn state(app: &tauri::AppHandle) -> Indicator {
    use tauri::Manager;
    let state = app.state::<crate::app::AppState>();
    let inner = state.inner.locked();
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
    (value.profile, value.profiles) = profiles(
        &inner.session.phase,
        &inner.settings,
        inner.indicator_menu,
        |id| {
            crate::inference::recognizer(id).is_some_and(|model| {
                crate::inference::missing(&state.data, model.id).is_ok_and(|m| m.is_empty())
            })
        },
    );
    value
}
/// Logical size of the indicator window. Recording and processing show only an
/// animation and the timer, so the window is a small pill. A notice carries
/// text and needs the wider window.
pub const HEIGHT: f64 = 44.0;
const PILL_WIDTH: f64 = 104.0;
const NOTICE_WIDTH: f64 = 320.0;
/// The profile icon, its gap, and the room each character of the name takes.
const PROFILE_SPACE: f64 = 30.0;
const PROFILE_CHARACTER: f64 = 7.5;
const PROFILE_WIDTH_LIMIT: f64 = 280.0;
const LIST_WIDTH: f64 = 220.0;
const LIST_ROW: f64 = 30.0;
/// The list's padding above its first row and below its last.
const LIST_PADDING: f64 = 12.0;
/// The notice shown after delivery, if any. A paste that was requested needs no
/// message: the text appeared where the user was typing. The other outcomes ask
/// the user to act or explain why nothing was copied.
pub fn delivery_notice(status: &str, text_empty: bool) -> Option<String> {
    if status.starts_with("Copied; paste requested") {
        None
    } else if text_empty {
        Some("No text to copy".into())
    } else {
        Some("Copied - paste manually".into())
    }
}
/// Logical width and height of the window for this state. The profile name
/// widens the pill, and the open list grows the window upward.
pub fn size_for(value: &Indicator) -> (f64, f64) {
    if value.mode == "notice" {
        return (NOTICE_WIDTH, HEIGHT);
    }
    let mut width = PILL_WIDTH;
    if !value.profile.is_empty() {
        width = (width + PROFILE_SPACE + PROFILE_CHARACTER * value.profile.chars().count() as f64)
            .min(PROFILE_WIDTH_LIMIT);
    }
    if value.profiles.is_empty() {
        return (width, HEIGHT);
    }
    (
        width.max(LIST_WIDTH),
        HEIGHT + LIST_PADDING * 2.0 + LIST_ROW * value.profiles.len() as f64,
    )
}
pub fn placement(
    screen: tauri::PhysicalRect<i32, u32>,
    work_area: tauri::PhysicalRect<i32, u32>,
    scale: f64,
    (logical_width, logical_height): (f64, f64),
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
    let width = ((logical_width * scale).round() as u32).min(area.size.width);
    let height = ((logical_height * scale).round() as u32).min(area.size.height);
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
    .inner_size(PILL_WIDTH, HEIGHT)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focusable(false)
    .focused(false)
    // The first click on the profile name opens the list, though the window is never key.
    .accept_first_mouse(true)
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
        let mut was_interactive = false;
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
                        size_for(&value),
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
            // Send the state before the window is shown or after it is hidden,
            // so the webview never holds the previous dictation's mode when the
            // window appears.
            if value.visible || was_visible {
                let _ = app.emit_to("recording", "recording-state", value.clone());
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
            let interactive = value.interactive();
            if interactive != was_interactive {
                let window = window.clone();
                let _ = app.run_on_main_thread(move || {
                    #[cfg(target_os = "macos")]
                    if let Ok(native) = window.ns_window() {
                        unsafe {
                            crate::platform::conch_overlay_interactive(native, interactive);
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = window.set_ignore_cursor_events(!interactive);
                    }
                });
                was_interactive = interactive;
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
        let popup = placement(screen, work, 1.0, (PILL_WIDTH, HEIGHT));
        assert_eq!(popup.position.y + popup.size.height as i32, 790);
        assert!(popup.position.y >= work.position.y);
    }
    #[test]
    fn popup_handles_retina_negative_origins_and_side_docks() {
        let screen = area(-2880, -1800, 2880, 1800);
        let work = area(-2700, -1750, 2700, 1600);
        let popup = placement(screen, work, 2.0, (PILL_WIDTH, HEIGHT));
        assert_eq!(popup.position.y + popup.size.height as i32, -198);
        assert_eq!(popup.position.x, -2700 + (2700 - 2 * PILL_WIDTH as i32) / 2);
        assert!(popup.position.x >= work.position.x);
    }
    #[test]
    fn popup_clamps_to_small_fractionally_scaled_work_areas() {
        let screen = area(0, 0, 640, 480);
        let work = area(200, 20, 300, 90);
        let popup = placement(screen, work, 1.25, (PILL_WIDTH, HEIGHT));
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
    fn shown(mode: &str) -> Indicator {
        Indicator {
            mode: mode.into(),
            ..presentation(&Phase::Recording, true, false, 0.0, 0, None)
        }
    }
    #[test]
    fn only_notices_use_the_wide_window() {
        assert_eq!(size_for(&shown("recording")), (PILL_WIDTH, HEIGHT));
        assert_eq!(size_for(&shown("processing")), (PILL_WIDTH, HEIGHT));
        assert_eq!(size_for(&shown("notice")), (NOTICE_WIDTH, HEIGHT));
        let screen = area(0, 0, 1440, 900);
        let notice = placement(screen, screen, 1.0, size_for(&shown("notice")));
        assert_eq!(notice.size.width, 320);
        assert_eq!(notice.position.x, (1440 - 320) / 2);
    }
    #[test]
    fn indicator_mode_distinguishes_recording_processing_and_notices() {
        let mode = |phase, notice| presentation(&phase, true, false, 0.5, 4, notice).mode;
        assert_eq!(mode(Phase::Recording, None), "recording");
        assert_eq!(mode(Phase::Processing, None), "processing");
        assert_eq!(mode(Phase::Idle, Some("Copied")), "notice");
    }
    #[test]
    fn a_requested_paste_leaves_no_notice_and_other_outcomes_keep_theirs() {
        let paste = "Copied; paste requested once (insertion is not confirmed)";
        assert_eq!(delivery_notice(paste, false), None);
        assert_eq!(
            delivery_notice("Copied; paste is unavailable", false).as_deref(),
            Some("Copied - paste manually")
        );
        assert_eq!(
            delivery_notice("Copied", true).as_deref(),
            Some("No text to copy")
        );
    }
    fn two_profiles() -> crate::storage::Settings {
        use crate::storage::{Profile, Settings};
        Settings {
            profiles: vec![
                Profile::default(),
                Profile {
                    id: "code".into(),
                    name: "conch".into(),
                    recognizer: "tiny.en".into(),
                    ..Default::default()
                },
            ],
            active_profile: "code".into(),
            ..Default::default()
        }
    }
    #[test]
    fn one_profile_shows_no_name_and_takes_no_clicks() {
        let settings = crate::storage::Settings::default();
        let (name, list) = profiles(&Phase::Recording, &settings, true, |_| true);
        assert_eq!((name.as_str(), list.len()), ("", 0));
        let value = presentation(&Phase::Recording, true, false, 0.0, 0, None);
        assert!(!value.interactive());
        assert_eq!(size_for(&value), (PILL_WIDTH, HEIGHT));
    }
    #[test]
    fn a_recording_with_profiles_names_the_active_one_and_lists_them_when_open() {
        let settings = two_profiles();
        let (name, list) = profiles(&Phase::Recording, &settings, false, |_| true);
        assert_eq!(name, "conch");
        assert!(list.is_empty());
        let (_, list) = profiles(&Phase::Recording, &settings, true, |id| id != "small.en");
        assert_eq!(
            list,
            vec![
                ProfileChoice {
                    id: "default".into(),
                    name: "Default".into(),
                    active: false,
                    available: false,
                },
                ProfileChoice {
                    id: "code".into(),
                    name: "conch".into(),
                    active: true,
                    available: true,
                },
            ]
        );
    }
    #[test]
    fn processing_shows_no_profile_and_passes_clicks_through() {
        let settings = two_profiles();
        let (name, list) = profiles(&Phase::Processing, &settings, true, |_| true);
        assert!(name.is_empty() && list.is_empty());
    }
    #[test]
    fn the_name_widens_the_pill_and_the_open_list_grows_the_window_upward() {
        let mut value = presentation(&Phase::Recording, true, false, 0.0, 0, None);
        value.profile = "conch".into();
        assert!(value.interactive());
        let (width, height) = size_for(&value);
        assert!(width > PILL_WIDTH && height == HEIGHT);
        value.profile = "a very long profile name that keeps going".into();
        assert_eq!(size_for(&value).0, PROFILE_WIDTH_LIMIT);
        value.profiles = profiles(&Phase::Recording, &two_profiles(), true, |_| true).1;
        let (width, height) = size_for(&value);
        assert!(width >= LIST_WIDTH);
        assert_eq!(height, HEIGHT + LIST_PADDING * 2.0 + LIST_ROW * 2.0);
        let screen = area(0, 0, 1440, 900);
        let open = placement(screen, screen, 1.0, (width, height));
        let closed = placement(screen, screen, 1.0, (width, HEIGHT));
        assert_eq!(
            open.position.y + open.size.height as i32,
            closed.position.y + closed.size.height as i32
        );
    }
}
