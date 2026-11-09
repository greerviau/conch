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
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(app::shortcut)
                .build(),
        )
        .setup(app::setup)
        .invoke_handler(tauri::generate_handler![
            app::view,
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
