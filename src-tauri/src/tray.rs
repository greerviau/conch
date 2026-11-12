use crate::lock::Locked;
use crate::{app::AppState, session::Phase};
use std::time::Duration;
use tauri::{
    AppHandle, Manager,
    image::Image,
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
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
/// One profile in the menu bar submenu.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileItem {
    pub id: String,
    pub name: String,
    pub checked: bool,
    pub enabled: bool,
}
/// The submenu's title and items, or `None` with one profile, when the menu
/// shows no Profile item. During a recording a profile whose model is not
/// installed cannot be chosen, because the recording cannot wait for a download.
pub fn profile_menu(
    settings: &crate::storage::Settings,
    phase: &Phase,
    installed: impl Fn(&str) -> bool,
) -> Option<(String, Vec<ProfileItem>)> {
    if settings.profiles.len() < 2 {
        return None;
    }
    let active = settings.active();
    let items = settings
        .profiles
        .iter()
        .map(|p| ProfileItem {
            id: p.id.clone(),
            name: p.name.clone(),
            checked: p.id == active.id,
            enabled: *phase != Phase::Processing
                && (*phase != Phase::Recording || p.id == active.id || installed(&p.recognizer)),
        })
        .collect();
    Some((format!("Profile: {}", active.name), items))
}
const PROFILE_PREFIX: &str = "profile:";
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
    // Rebuilds the menu with the Profile submenu, or without it for one profile.
    let rebuild = {
        let app = app.clone();
        let (status, show, update, version, quit) = (
            status.clone(),
            show.clone(),
            update.clone(),
            version.clone(),
            quit.clone(),
        );
        move |profiles: &Option<(String, Vec<ProfileItem>)>| -> tauri::Result<Menu<tauri::Wry>> {
            let separator = PredefinedMenuItem::separator(&app)?;
            let footer = PredefinedMenuItem::separator(&app)?;
            let mut items: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = vec![
                Box::new(status.clone()),
                Box::new(separator),
                Box::new(show.clone()),
            ];
            if let Some((title, choices)) = profiles {
                let submenu = Submenu::with_id(&app, "profiles", title, true)?;
                for choice in choices {
                    submenu.append(&CheckMenuItem::with_id(
                        &app,
                        format!("{PROFILE_PREFIX}{}", choice.id),
                        &choice.name,
                        choice.enabled,
                        choice.checked,
                        None::<&str>,
                    )?)?;
                }
                submenu.append(&PredefinedMenuItem::separator(&app)?)?;
                submenu.append(&MenuItem::with_id(
                    &app,
                    "edit_profiles",
                    "Edit Profiles...",
                    true,
                    None::<&str>,
                )?)?;
                items.push(Box::new(PredefinedMenuItem::separator(&app)?));
                items.push(Box::new(submenu));
                items.push(Box::new(PredefinedMenuItem::separator(&app)?));
            }
            items.push(Box::new(update.clone()));
            items.push(Box::new(footer));
            items.push(Box::new(version.clone()));
            items.push(Box::new(quit.clone()));
            let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = items.iter().map(|i| i.as_ref()).collect();
            Menu::with_items(&app, &refs)
        }
    };
    let tray = TrayIconBuilder::with_id("conch")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png")).map_err(failed)?)
        .icon_as_template(true)
        .tooltip("Conch")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "edit_profiles" => {
                show_main(app);
                use tauri::Emitter;
                let _ = app.emit_to("main", "open-tab", "Profiles");
            }
            "update" => crate::update::menu_selected(app),
            "quit" => app.exit(0),
            id => {
                if let Some(profile) = id.strip_prefix(PROFILE_PREFIX)
                    && let Err(error) =
                        crate::app::switch_profile(&app.state::<AppState>(), profile)
                {
                    crate::log::warn("app", &error);
                }
            }
        })
        .build(app)
        .map_err(failed)?;
    app.manage(Active);
    app.manage(crate::update::MenuEntry(update));
    let app = app.clone();
    std::thread::spawn(move || {
        let mut shown = None;
        let mut shown_profiles = None;
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let (now, profiles) = {
                let state = app.state::<AppState>();
                let inner = state.inner.locked();
                let profiles = profile_menu(&inner.settings, &inner.session.phase, |id| {
                    crate::inference::recognizer(id).is_some_and(|model| {
                        crate::inference::missing(&state.data, model.id).is_ok_and(|m| m.is_empty())
                    })
                });
                ((inner.session.phase.clone(), inner.audio_ready), profiles)
            };
            if shown_profiles.as_ref() != Some(&profiles) {
                match rebuild(&profiles) {
                    Ok(menu) => {
                        let _ = tray.set_menu(Some(menu));
                    }
                    Err(error) => {
                        crate::log::warn("app", &format!("Cannot update the menu: {error}"))
                    }
                }
                shown_profiles = Some(profiles);
            }
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
    #[test]
    fn the_menu_has_no_profile_item_with_one_profile() {
        let settings = crate::storage::Settings::default();
        assert_eq!(profile_menu(&settings, &Phase::Idle, |_| true), None);
    }
    #[test]
    fn the_profile_submenu_names_the_active_profile_and_checks_it() {
        use crate::storage::{Profile, Settings};
        let settings = Settings {
            profiles: vec![
                Profile::default(),
                Profile {
                    id: "code".into(),
                    name: "conch".into(),
                    recognizer: "tiny.en".into(),
                    ..Default::default()
                },
            ],
            active_profile: "default".into(),
            ..Default::default()
        };
        let (title, items) = profile_menu(&settings, &Phase::Idle, |_| false).unwrap();
        assert_eq!(title, "Profile: Default");
        assert_eq!(
            items
                .iter()
                .map(|i| (i.name.as_str(), i.checked, i.enabled))
                .collect::<Vec<_>>(),
            [("Default", true, true), ("conch", false, true)]
        );
        let (_, recording) =
            profile_menu(&settings, &Phase::Recording, |id| id != "tiny.en").unwrap();
        assert!(
            !recording[1].enabled,
            "a recording cannot wait for a download"
        );
        let (_, processing) = profile_menu(&settings, &Phase::Processing, |_| true).unwrap();
        assert!(processing.iter().all(|i| !i.enabled));
    }
}
