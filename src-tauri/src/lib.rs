pub mod app;
pub mod audio;
pub mod core;
pub mod delivery;
pub mod index;
pub mod inference;
pub mod platform;
pub mod session;
pub mod storage;
pub fn run() {
    use tauri::Manager;
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(app::setup)
        .invoke_handler(tauri::generate_handler![
            app::view,
            app::indicator_state,
            app::request_microphone,
            app::request_accessibility,
            app::choose_repository,
            app::shortcut_capture,
            app::save_settings,
            app::refresh,
            app::start,
            app::finish,
            app::cancel,
            app::retry,
            app::edit,
            app::accept,
            app::undo,
            app::reject,
            app::copy,
            app::download,
            app::credential
        ])
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                window.app_handle().exit(0);
            }
        })
        .build(tauri::generate_context!())
        .expect("Conch could not start")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                let _ = app::cancel(app.clone());
            }
        });
}

pub mod indicator;
