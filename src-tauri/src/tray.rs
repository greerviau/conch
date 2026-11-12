use crate::lock::Locked;
use crate::{app::AppState, session::Phase};
use std::time::Duration;
use tauri::{
    AppHandle, Manager,
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

/// Managed state that exists only when the menu bar icon was created. Without
/// it, closing the window quits the app, so Conch is never left running with
/// no way to reach it.
pub struct Active;

pub struct Presentation {
    pub title: &'static str,
    pub status: &'static str,
}
pub fn presentation(phase: &Phase, microphone_ready: bool) -> Presentation {
    match (phase, microphone_ready) {
        (Phase::Idle, _) => Presentation {
            title: "",
            status: "Ready",
        },
        (Phase::Recording, false) => Presentation {
            title: "\u{25cf}",
            status: "Opening microphone",
        },
        (Phase::Recording, true) => Presentation {
            title: "\u{25cf}",
            status: "Listening",
        },
        (Phase::Processing, _) => Presentation {
            title: "\u{2026}",
            status: "Preparing text",
        },
    }
}
pub fn version_label(version: &str) -> String {
    format!("Conch {version}")
}
pub fn setup(app: &AppHandle) -> Result<(), String> {
    let failed = |_: tauri::Error| "Cannot create the menu bar icon".to_string();
    let status = MenuItem::with_id(app, "status", "Ready", false, None::<&str>).map_err(failed)?;
    let show = MenuItem::with_id(app, "show", "Show Conch", true, None::<&str>).map_err(failed)?;
    let update = MenuItem::with_id(
        app,
        "update",
        crate::update::menu_label(&crate::update::Status::Idle),
        true,
        None::<&str>,
    )
    .map_err(failed)?;
    let version = MenuItem::with_id(
        app,
        "version",
        version_label(&app.package_info().version.to_string()),
        false,
        None::<&str>,
    )
    .map_err(failed)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Conch", true, None::<&str>).map_err(failed)?;
    let separator = PredefinedMenuItem::separator(app).map_err(failed)?;
    let footer = PredefinedMenuItem::separator(app).map_err(failed)?;
    let menu = Menu::with_items(
        app,
        &[
            &status, &separator, &show, &update, &footer, &version, &quit,
        ],
    )
    .map_err(failed)?;
    let tray = TrayIconBuilder::with_id("conch")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png")).map_err(failed)?)
        .icon_as_template(true)
        .tooltip("Conch")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "update" => crate::update::menu_selected(app),
            "quit" => app.exit(0),
            _ => (),
        })
        .build(app)
        .map_err(failed)?;
    app.manage(Active);
    app.manage(crate::update::MenuEntry(update));
    let app = app.clone();
    std::thread::spawn(move || {
        let mut shown = None;
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let now = {
                let state = app.state::<AppState>();
                let inner = state.inner.locked();
                (inner.session.phase.clone(), inner.audio_ready)
            };
            if shown.as_ref() == Some(&now) {
                continue;
            }
            let look = presentation(&now.0, now.1);
            let _ = tray.set_title(Some(look.title));
            let _ = tray.set_tooltip(Some(format!("Conch - {}", look.status)));
            let _ = status.set_text(look.status);
            shown = Some(now);
        }
    });
    Ok(())
}
/// Shows the window and, on macOS, restores the Dock icon.
pub fn show_main(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
/// Hides the window and, on macOS, removes the Dock icon. Recording, the
/// shortcut, and the menu bar icon keep working.
pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Phase;
    #[test]
    fn menu_shows_the_version() {
        assert_eq!(version_label("0.1.1"), "Conch 0.1.1");
    }
    #[test]
    fn menu_bar_shows_recording_and_processing_but_stays_quiet_when_idle() {
        assert_eq!(presentation(&Phase::Idle, false).title, "");
        assert_eq!(presentation(&Phase::Idle, false).status, "Ready");
        assert_eq!(presentation(&Phase::Recording, false).title, "\u{25cf}");
        assert_eq!(
            presentation(&Phase::Recording, false).status,
            "Opening microphone"
        );
        assert_eq!(presentation(&Phase::Recording, true).status, "Listening");
        assert_eq!(presentation(&Phase::Processing, true).title, "\u{2026}");
        assert_eq!(
            presentation(&Phase::Processing, true).status,
            "Preparing text"
        );
    }
}
