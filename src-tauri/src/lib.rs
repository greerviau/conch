pub mod app;
pub mod audio;
pub mod core;
pub mod delivery;
pub mod index;
pub mod inference;
pub mod pipeline;
pub mod platform;
pub mod session;
pub mod spoken;
pub mod storage;
pub fn run() {
    use tauri::Manager;
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(app::setup)
        .invoke_handler(tauri::generate_handler![
            app::view,
            app::indicator_state,
            app::microphones,
            app::request_microphone,
            app::request_accessibility,
            app::repair_accessibility,
            app::restart_for_accessibility,
            app::choose_repository,
            app::shortcut_capture,
            app::set_hotkey,
            app::save_settings,
            app::refresh,
            app::index_terms,
            app::start,
            app::finish,
            app::cancel,
            app::retry,
            app::edit,
            app::accept,
            app::accept_always,
            app::undo,
            app::reject,
            app::copy,
            app::history_page,
            app::history_copy,
            app::history_delete,
            app::history_clear,
            app::download,
            app::credential,
            update::check_update,
            update::install_update
        ])
        .on_window_event(|window, event| {
            if window.label() == "main"
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                let app = window.app_handle();
                if app.try_state::<tray::Active>().is_some() {
                    api.prevent_close();
                    tray::hide_main(app);
                } else {
                    app.exit(0);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("Conch could not start")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = event
            {
                tray::show_main(app);
            }
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                let _ = app::cancel(app.clone());
                inference::shutdown(&app.state::<app::AppState>().workers);
            }
        });
}

pub mod indicator;

pub mod accessibility;

pub mod shortcuts;

pub mod chunk;

pub mod tray;

pub mod update;

pub mod history;
