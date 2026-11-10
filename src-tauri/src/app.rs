use crate::{
    audio,
    core::{self, ResultText, Snapshot},
    delivery::{self, Desktop},
    index,
    inference::{self, Worker},
    platform,
    session::{Phase, Session},
    storage::{self, Settings},
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicU32, mpsc},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub struct AppState {
    pub data: PathBuf,
    pub workers: PathBuf,
    pub inner: Mutex<Inner>,
}
pub struct Inner {
    pub settings: Settings,
    pub session: Session,
    pub result: Option<ResultText>,
    pub error: String,
    pub stop: Option<mpsc::Sender<bool>>,
    pub worker: Worker,
    pub retry: Option<(Vec<u8>, Snapshot)>,
    pub downloading: bool,
    pub capturing_shortcut: bool,
    pub paused_shortcut: Option<Shortcut>,
    pub audio_ready: bool,
    pub level: Arc<AtomicU32>,
    pub recording_since: Option<Instant>,
    pub recorded_seconds: u64,
    pub indicator_notice: Option<(Instant, String)>,
}
#[derive(Serialize)]
pub struct View {
    settings: Settings,
    phase: Phase,
    result: Option<ResultText>,
    error: String,
    indexes: Vec<(String, index::Index, i64)>,
    models: Vec<(String, bool)>,
    accessibility: bool,
    installation_path: Option<String>,
    platform: String,
    downloading: bool,
    microphone: String,
    can_retry: bool,
    shortcuts_available: bool,
}
#[tauri::command]
pub fn view(app: AppHandle, state: State<'_, AppState>) -> Result<View, String> {
    let inner = state.inner.lock().unwrap();
    let mut indexes = storage::indexes(&state.data)?;
    for (_, index, _) in &mut indexes {
        index.entries.clear();
    }
    Ok(View {
        settings: inner.settings.clone(),
        phase: inner.session.phase.clone(),
        result: inner.result.clone(),
        error: inner.error.clone(),
        indexes,
        models: inference::MODELS
            .iter()
            .map(|m| (m.name.into(), state.data.join(m.name).is_file()))
            .collect(),
        accessibility: platform::permission(),
        installation_path: platform::installation_path(),
        platform: std::env::consts::OS.into(),
        downloading: inner.downloading,
        microphone: platform::microphone_status().into(),
        can_retry: inner.retry.is_some(),
        shortcuts_available: app
            .try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>()
            .is_some(),
    })
}
#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: Settings,
) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    if inner.session.phase != Phase::Idle {
        return Err("Finish or cancel before saving settings".into());
    }
    if inner.capturing_shortcut {
        return Err("Finish choosing a shortcut before saving".into());
    }
    if !matches!(settings.activation.as_str(), "push" | "toggle")
        || !matches!(settings.cleanup.mode.as_str(), "off" | "basic" | "enhanced")
        || !matches!(
            settings.cleanup.provider.as_str(),
            "managed" | "local" | "remote"
        )
    {
        return Err("Invalid settings".into());
    }
    if settings.cleanup.provider != "managed"
        && !inference::endpoint_allowed(
            &settings.cleanup.endpoint,
            settings.cleanup.provider == "remote",
        )
    {
        return Err(
            "Use HTTPS for remote cleanup, or a loopback endpoint for local cleanup".into(),
        );
    }
    if settings.cleanup.provider == "remote" && !settings.cleanup.remote_enabled {
        return Err("Confirm the remote destination before enabling remote cleanup".into());
    }
    for repository in &mut settings.repositories {
        if !repository.root.is_absolute() {
            return Err("Repository roots must be absolute paths".into());
        }
        if repository.id.is_empty() {
            repository.id = uuid::Uuid::new_v4().to_string();
        }
    }
    settings.mappings.retain(|m| {
        m.repository
            .as_ref()
            .is_none_or(|id| settings.repositories.iter().any(|r| &r.id == id))
    });
    let shortcuts = app
        .try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>()
        .is_some();
    if !shortcuts && settings.hotkey != inner.settings.hotkey {
        return Err(
            "Global shortcuts are unavailable on this desktop; use the recording controls".into(),
        );
    }
    if shortcuts {
        crate::shortcuts::replace(
            &mut crate::shortcuts::NativeRegistry(&app),
            &inner.settings.hotkey,
            &settings.hotkey,
            || storage::save(&state.data, &settings),
        )?;
    } else {
        storage::save(&state.data, &settings)?;
    }
    inner.settings = settings;
    Ok(())
}
#[tauri::command]
pub async fn refresh(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || refresh_now(&app))
        .await
        .map_err(|_| "Index worker failed")?
}
fn refresh_now(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let repositories = state.inner.lock().unwrap().settings.repositories.clone();
    for repository in repositories {
        let index = index::scan(&repository.root, &repository.id, &repository.exclusions)?;
        let inner = state.inner.lock().unwrap();
        if inner.settings.repositories.iter().any(|r| {
            r.id == repository.id
                && r.root == repository.root
                && r.exclusions == repository.exclusions
        }) {
            storage::put_index(&state.data, &repository.id, &index)?;
        }
    }
    Ok(())
}
#[tauri::command]
pub fn start(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (send, receive) = mpsc::channel();
    let mut inner = state.inner.lock().unwrap();
    if inner.capturing_shortcut {
        return Err("Finish choosing your shortcut before recording".into());
    }
    if cfg!(target_os = "macos") && platform::microphone_status() != "granted" {
        return Err("Enable Conch microphone access in Setup before recording".into());
    }
    let snapshot = storage::snapshot(&state.data, &inner.settings)?;
    if !state.data.join(inference::MODELS[0].name).is_file() {
        return Err("Download the recognition model before recording".into());
    }
    let generation = inner.session.start()?;
    inner.stop = Some(send);
    inner.error.clear();
    inner.retry = None;
    inner.worker = inference::new_worker();
    inner.audio_ready = false;
    inner.recording_since = None;
    inner.indicator_notice = None;
    let level = inner.level.clone();
    level.store(0, std::sync::atomic::Ordering::Relaxed);
    drop(inner);
    std::thread::spawn(platform::prepare);
    let handle = app.clone();
    std::thread::spawn(move || {
        let captured = audio::capture(
            receive,
            || {
                let state = handle.state::<AppState>();
                let mut inner = state.inner.lock().unwrap();
                if inner.session.generation == generation {
                    inner.audio_ready = true;
                    inner.recording_since = Some(Instant::now());
                }
            },
            level,
        );
        let state = handle.state::<AppState>();
        match captured {
            Ok(Some(wav)) => {
                {
                    let mut inner = state.inner.lock().unwrap();
                    if inner.session.generation != generation {
                        return;
                    }
                    inner.recorded_seconds = inner
                        .recording_since
                        .map(|at| at.elapsed().as_secs())
                        .unwrap_or(0);
                    if inner.session.phase == Phase::Recording {
                        inner.session.finish();
                    }
                    inner.stop = None;
                    inner.retry = Some((wav.clone(), snapshot.clone()));
                }
                process(&handle, generation, wav, snapshot);
            }
            Ok(None) => (),
            Err(error) => {
                let mut inner = state.inner.lock().unwrap();
                if inner.session.generation == generation {
                    inner.session.cancel();
                    inner.indicator_notice = Some((Instant::now(), error.clone()));
                    inner.error = error;
                    inner.stop = None;
                }
            }
        }
    });
    Ok(())
}
#[tauri::command]
pub fn finish(state: State<'_, AppState>) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    inner.recorded_seconds = inner
        .recording_since
        .map(|at| at.elapsed().as_secs())
        .unwrap_or(0);
    if inner.session.finish()
        && let Some(stop) = inner.stop.take()
    {
        let _ = stop.send(true);
    }
    Ok(())
}
#[tauri::command]
pub fn cancel(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let worker = {
        let mut inner = state.inner.lock().unwrap();
        inner.session.cancel();
        inner.indicator_notice = None;
        inner.retry = None;
        if let Some(stop) = inner.stop.take() {
            let _ = stop.send(false);
        }
        inner.worker.clone()
    };
    inference::stop(&worker);
    Ok(())
}
fn process(app: &AppHandle, generation: u64, wav: Vec<u8>, snapshot: Snapshot) {
    let state = app.state::<AppState>();
    let worker = {
        let inner = state.inner.lock().unwrap();
        if inner.session.generation != generation {
            return;
        }
        inner.worker.clone()
    };
    let recognition = inference::transcribe(
        &state.workers,
        &state.data.join(inference::MODELS[0].name),
        wav,
        &worker,
        false,
    );
    let raw = match recognition {
        Ok(raw) if !raw.trim().is_empty() => raw,
        Ok(_) => {
            failed(
                app,
                generation,
                "No speech recognized; clipboard unchanged".into(),
            );
            return;
        }
        Err(error) => {
            failed(app, generation, error);
            return;
        }
    };
    if state.inner.lock().unwrap().session.generation != generation {
        return;
    }
    let mut result = core::prepare(&raw, &snapshot);
    if snapshot.cleanup.mode == "enhanced" {
        match inference::enhanced(inference::CleanupRequest {
            workers: &state.workers,
            data: &state.data,
            snapshot: &snapshot,
            raw: &raw,
            worker: &worker,
            cpu: false,
        }) {
            Ok(text) => {
                result.text = text;
                result.status =
                    "Enhanced cleanup completed; semantic equivalence is unverified".into();
            }
            Err(error) => result.status = error,
        }
    }
    result.suggestions = core::suggestions(&result.text, &snapshot);
    let mut inner = state.inner.lock().unwrap();
    if !inner.session.complete(generation) {
        return;
    }
    match delivery::deliver(&mut platform::NativeDesktop::default(), &result.text) {
        Ok(status) => {
            inner.indicator_notice = Some((
                Instant::now(),
                if status.starts_with("Copied; paste requested") {
                    "Paste sent".into()
                } else if result.text.is_empty() {
                    "No text to copy".into()
                } else {
                    "Copied - paste manually".into()
                },
            ));
            result.status.push_str(&format!(". {status}"));
        }
        Err(error) => inner.error = error,
    }
    inner.result = Some(result);
    inner.retry = None;
}
fn failed(app: &AppHandle, generation: u64, error: String) {
    let state = app.state::<AppState>();
    let mut inner = state.inner.lock().unwrap();
    if inner.session.generation == generation {
        inner.session.phase = Phase::Idle;
        inner.indicator_notice = Some((Instant::now(), error.clone()));
        inner.error = error;
    }
}
#[tauri::command]
pub fn retry(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mut inner = state.inner.lock().unwrap();
    let (wav, snapshot) = inner.retry.clone().ok_or("No failed recording to retry")?;
    let generation = inner.session.start()?;
    inner.session.finish();
    inner.error.clear();
    inner.worker = inference::new_worker();
    drop(inner);
    std::thread::spawn(move || process(&app, generation, wav, snapshot));
    Ok(())
}
#[tauri::command]
pub fn edit(state: State<'_, AppState>, text: String) -> Result<(), String> {
    if text.len() > 100_000 {
        return Err("Result is too long".into());
    }
    let mut inner = state.inner.lock().unwrap();
    if inner.session.phase != Phase::Idle {
        return Err("Finish or cancel before editing the result".into());
    }
    let result = inner.result.as_mut().ok_or("No result")?;
    core::edit_result(result, text);
    Ok(())
}
#[tauri::command]
pub fn accept(state: State<'_, AppState>, index: usize, candidate: usize) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    let result = inner.result.as_mut().ok_or("No result")?;
    core::accept_suggestion(result, index, candidate)
}
#[tauri::command]
pub fn undo(state: State<'_, AppState>, index: usize) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    core::undo_edit(inner.result.as_mut().ok_or("No result")?, index)
}
#[tauri::command]
pub fn reject(state: State<'_, AppState>, index: usize) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    let result = inner.result.as_mut().ok_or("No result")?;
    if index >= result.suggestions.len() {
        return Err("Suggestion is unavailable".into());
    }
    result.suggestions.remove(index);
    Ok(())
}
#[tauri::command]
pub fn copy(state: State<'_, AppState>, raw: bool) -> Result<(), String> {
    let inner = state.inner.lock().unwrap();
    let result = inner.result.as_ref().ok_or("No result")?;
    platform::NativeDesktop::default().copy(if raw { &result.raw } else { &result.text })
}
#[tauri::command]
pub async fn download(app: AppHandle, model: usize) -> Result<(), String> {
    let descriptor = *inference::MODELS.get(model).ok_or("Unknown model")?;
    {
        let state = app.state::<AppState>();
        let mut inner = state.inner.lock().unwrap();
        if inner.downloading {
            return Err("A download is already active".into());
        }
        inner.downloading = true;
    }
    let result = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || {
            let state = app.state::<AppState>();
            inference::download(&state.data, descriptor, |current, total| {
                let _ = app.emit("model-progress", (current, total));
            })
            .map(|_| ())
        }
    })
    .await
    .map_err(|_| "Model download worker failed".to_string())
    .and_then(|r| r);
    app.state::<AppState>().inner.lock().unwrap().downloading = false;
    result
}
#[tauri::command]
pub fn credential(endpoint: String, secret: Option<String>) -> Result<(), String> {
    if !inference::endpoint_allowed(&endpoint, true) {
        return Err("Invalid remote destination".into());
    }
    let entry = keyring::Entry::new("app.conch.desktop", &endpoint)
        .map_err(|_| "Credential store unavailable")?;
    match secret {
        Some(secret) if !secret.is_empty() => entry
            .set_password(&secret)
            .map_err(|_| "Cannot save credential".into()),
        _ => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Cannot remove credential".into()),
        },
    }
}
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data = app.path().app_local_data_dir()?;
    let settings = storage::load(&data)?;
    let workers = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("workers")
    } else {
        app.path().resource_dir()?.join("workers")
    };
    app.manage(AppState {
        data,
        workers,
        inner: Mutex::new(Inner {
            settings: settings.clone(),
            session: Session::default(),
            result: None,
            error: String::new(),
            stop: None,
            worker: inference::new_worker(),
            retry: None,
            downloading: false,
            capturing_shortcut: false,
            paused_shortcut: None,
            audio_ready: false,
            level: Arc::new(AtomicU32::new(0)),
            recording_since: None,
            recorded_seconds: 0,
            indicator_notice: None,
        }),
    });
    if let Err(error) = crate::indicator::setup(app.handle()) {
        app.state::<AppState>().inner.lock().unwrap().error = error;
    }
    let supported_session = !cfg!(target_os = "linux")
        || (std::env::var_os("WAYLAND_DISPLAY").is_none()
            && std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("wayland"));
    let shortcuts = supported_session
        && app
            .handle()
            .plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(shortcut)
                    .build(),
            )
            .is_ok();
    if !shortcuts
        || app
            .global_shortcut()
            .register(settings.hotkey.as_str())
            .is_err()
    {
        app.state::<AppState>().inner.lock().unwrap().error =
            "Global hotkey unavailable; use the recording controls".into();
    }
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        use notify::Watcher;
        let (send, receive) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = send.send(event);
        })
        .ok();
        let mut roots: Vec<PathBuf> = vec![];
        loop {
            if let Err(error) = refresh_now(&handle) {
                handle.state::<AppState>().inner.lock().unwrap().error =
                    format!("Index refresh failed: {error}");
            }
            if let Some(watcher) = watcher.as_mut() {
                let current: Vec<_> = handle
                    .state::<AppState>()
                    .inner
                    .lock()
                    .unwrap()
                    .settings
                    .repositories
                    .iter()
                    .map(|r| r.root.clone())
                    .collect();
                if roots != current {
                    for root in &roots {
                        let _ = watcher.unwatch(root);
                    }
                    for root in &current {
                        let _ = watcher.watch(root, notify::RecursiveMode::Recursive);
                    }
                    roots = current;
                }
            }
            let _ = receive.recv_timeout(Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(500));
            while receive.try_recv().is_ok() {}
        }
    });
    Ok(())
}
pub fn shortcut(
    app: &AppHandle,
    _shortcut: &Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
    let state = app.state::<AppState>();
    let action = {
        let mut inner = state.inner.lock().unwrap();
        if inner.capturing_shortcut {
            return;
        }
        let toggle = inner.settings.activation == "toggle";
        inner
            .session
            .key(event.state == ShortcutState::Pressed, toggle)
    };
    match action {
        Some("start") => {
            if let Err(error) = start(app.clone()) {
                let state = app.state::<AppState>();
                let mut inner = state.inner.lock().unwrap();
                inner.indicator_notice = Some((Instant::now(), error.clone()));
                inner.error = error;
            }
        }
        Some("finish") => {
            let _ = finish(state);
        }
        _ => (),
    }
}

#[tauri::command]
pub fn indicator_state(app: AppHandle) -> crate::indicator::Indicator {
    crate::indicator::state(&app)
}
#[tauri::command]
pub async fn request_microphone() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(platform::request_microphone)
        .await
        .map_err(|_| "Microphone permission request failed")?
}
#[tauri::command]
pub fn request_accessibility() -> Result<(), String> {
    platform::request_accessibility()
}
#[tauri::command]
pub async fn choose_repository(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose a repository")
            .blocking_pick_folder()
            .map(|path| {
                path.into_path()
                    .map(|p| p.to_string_lossy().to_string())
                    .map_err(|_| "Unsupported repository path".to_string())
            })
            .transpose()
    })
    .await
    .map_err(|_| "Folder picker failed")?
}
#[tauri::command]
pub fn shortcut_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    active: bool,
) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    if inner.session.phase != Phase::Idle {
        return Err("Finish or cancel your recording before choosing a shortcut".into());
    }
    let available = app
        .try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>()
        .is_some();
    if active {
        if inner.capturing_shortcut {
            return Ok(());
        }
        inner.paused_shortcut = if available {
            crate::shortcuts::pause(
                &mut crate::shortcuts::NativeRegistry(&app),
                &inner.settings.hotkey,
            )?
        } else {
            None
        };
        inner.capturing_shortcut = true;
    } else if inner.capturing_shortcut {
        inner.capturing_shortcut = false;
        inner.session.pressed = false;
        let paused = inner.paused_shortcut.take();
        if available {
            crate::shortcuts::resume(&mut crate::shortcuts::NativeRegistry(&app), paused)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn repair_accessibility(state: State<'_, AppState>) -> Result<(), String> {
    let inner = state.inner.lock().unwrap();
    if inner.session.phase != Phase::Idle || inner.downloading || inner.capturing_shortcut {
        return Err("Finish active work before repairing Accessibility access".into());
    }
    platform::repair_accessibility()
}
#[tauri::command]
pub fn restart_for_accessibility(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    {
        let inner = state.inner.lock().unwrap();
        if inner.session.phase != Phase::Idle || inner.downloading || inner.capturing_shortcut {
            return Err("Finish active work before restarting Conch".into());
        }
    }
    app.request_restart();
    Ok(())
}

#[tauri::command]
pub fn set_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    hotkey: String,
) -> Result<String, String> {
    let mut inner = state.inner.lock().unwrap();
    if inner.session.phase != Phase::Idle {
        return Err("Finish or cancel your recording before changing the shortcut".into());
    }
    if app
        .try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>()
        .is_none()
    {
        return Err("Global shortcuts are unavailable on this desktop".into());
    }
    let mut settings = inner.settings.clone();
    settings.hotkey = hotkey;
    crate::shortcuts::replace(
        &mut crate::shortcuts::NativeRegistry(&app),
        &inner.settings.hotkey,
        &settings.hotkey,
        || storage::save(&state.data, &settings),
    )?;
    inner.settings = settings;
    inner.capturing_shortcut = false;
    inner.paused_shortcut = None;
    inner.session.pressed = false;
    Ok(inner.settings.hotkey.clone())
}
