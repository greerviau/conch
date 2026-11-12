use crate::{
    audio,
    chunk::Chunker,
    core::{self, ResultText, Snapshot},
    delivery::{self, Desktop},
    history, index,
    inference::{self, Kind, Worker},
    pipeline::Transcriber,
    platform,
    session::{Phase, Session},
    storage::{self, Settings},
    timing::Stopwatch,
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
    /// The history entry that holds `result`, when it was saved.
    pub history_id: Option<i64>,
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
    /// When the recording ended, for timing the wait for the result.
    pub stopped: Option<Instant>,
    pub recorded_seconds: u64,
    pub indicator_notice: Option<(Instant, String)>,
}
#[derive(Serialize)]
pub struct View {
    settings: Settings,
    phase: Phase,
    result: Option<ResultText>,
    error: String,
    indexes: Vec<(String, storage::IndexStatus, i64)>,
    history_revision: i64,
    models: Vec<(String, bool)>,
    accessibility: bool,
    installation_path: Option<String>,
    platform: String,
    downloading: bool,
    microphone: String,
    can_retry: bool,
    shortcuts_available: bool,
    version: &'static str,
    update: crate::update::Status,
}
#[tauri::command]
pub fn view(app: AppHandle, state: State<'_, AppState>) -> Result<View, String> {
    // The interface polls this command. Read the database and ask the system
    // for permissions before taking the lock that recording and delivery need.
    let indexes = storage::index_status(&state.data)?;
    let history_revision = history::revision(&state.data)?;
    let accessibility = platform::permission();
    let microphone = platform::microphone_status().into();
    let inner = state.inner.lock().unwrap();
    Ok(View {
        settings: inner.settings.clone(),
        phase: inner.session.phase.clone(),
        result: inner.result.clone(),
        error: inner.error.clone(),
        indexes,
        history_revision,
        models: inference::MODELS
            .iter()
            .map(|m| (m.id.into(), state.data.join(m.name).is_file()))
            .collect(),
        accessibility,
        installation_path: platform::installation_path(),
        platform: std::env::consts::OS.into(),
        downloading: inner.downloading,
        microphone,
        can_retry: inner.retry.is_some(),
        shortcuts_available: app
            .try_state::<tauri_plugin_global_shortcut::GlobalShortcut<tauri::Wry>>()
            .is_some(),
        version: env!("CARGO_PKG_VERSION"),
        update: app.state::<crate::update::Tracker>().status(),
    })
}
/// Whether every enumerated setting holds a value Conch offers.
fn choices_valid(settings: &Settings) -> bool {
    matches!(settings.activation.as_str(), "push" | "toggle")
        && matches!(
            settings.cleanup.enhanced.as_str(),
            "disabled" | "managed" | "local" | "remote"
        )
        && inference::recognizer(&settings.recognizer).is_some()
}
/// Local and remote Enhanced cleanup need an allowed endpoint, and a remote
/// destination needs the user's confirmation.
fn check_enhanced(cleanup: &crate::core::Cleanup) -> Result<(), String> {
    let remote = cleanup.enhanced == "remote";
    if matches!(cleanup.enhanced.as_str(), "local" | "remote")
        && !inference::endpoint_allowed(&cleanup.endpoint, remote)
    {
        return Err(
            "Use HTTPS for remote cleanup, or a loopback endpoint for local cleanup".into(),
        );
    }
    if remote && !cleanup.remote_enabled {
        return Err("Confirm the remote destination before enabling remote cleanup".into());
    }
    Ok(())
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
    if !choices_valid(&settings) {
        return Err("Invalid settings".into());
    }
    check_enhanced(&settings.cleanup)?;
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
    let changed = inner.settings.recognizer != settings.recognizer;
    inner.settings = settings;
    drop(inner);
    if changed {
        // The old recognizer would idle for ten minutes holding its model in memory.
        inference::reap_idle_within(&state.workers, Duration::ZERO, inference::LLAMA_IDLE);
    }
    Ok(())
}
/// One page of a repository's indexed terms for the Repositories page.
#[tauri::command]
pub async fn index_terms(
    app: AppHandle,
    repository: String,
    query: String,
    kind: storage::TermKind,
    offset: usize,
    limit: usize,
) -> Result<storage::TermPage, String> {
    let dir = app.state::<AppState>().data.clone();
    tauri::async_runtime::spawn_blocking(move || {
        storage::index_terms(&dir, &repository, &query, kind, offset, limit.min(500))
    })
    .await
    .map_err(|_| "Index worker failed")?
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
    let model = recognition_model(&state.data, &inner.settings)?;
    let generation = inner.session.start()?;
    inner.stop = Some(send);
    inner.error.clear();
    inner.retry = None;
    inner.worker = inference::new_worker();
    inner.audio_ready = false;
    inner.recording_since = None;
    inner.stopped = None;
    inner.indicator_notice = None;
    let microphone = inner.settings.microphone.clone();
    let level = inner.level.clone();
    level.store(0, std::sync::atomic::Ordering::Relaxed);
    let worker = inner.worker.clone();
    drop(inner);
    std::thread::spawn(platform::prepare);
    let cleanup_worker = worker.clone();
    let warm = {
        let (workers, model, worker) = (state.workers.clone(), model.clone(), worker.clone());
        move || {
            let _ = inference::preload(&workers, &model, Kind::Whisper, &worker, false);
        }
    };
    let recognize = {
        let workers = state.workers.clone();
        let hint = core::recognition_hint(&snapshot);
        let worker = worker.clone();
        move |wav, _speculative| inference::transcribe(&workers, &model, wav, &hint, &worker, false)
    };
    // Enhanced cleanup runs on each chunk while the recording continues, so
    // only the tail is left to clean after the stop.
    let transcriber = if snapshot.cleanup.enhanced == "disabled" {
        Transcriber::start(warm, recognize)
    } else {
        let (workers, data, snapshot) =
            (state.workers.clone(), state.data.clone(), snapshot.clone());
        let worker = worker.clone();
        Transcriber::start_cleaning(warm, recognize, move |part| {
            inference::enhanced_part(inference::CleanupRequest {
                workers: &workers,
                data: &data,
                snapshot: &snapshot,
                raw: part,
                worker: &worker,
                cpu: false,
            })
        })
    };
    if snapshot.cleanup.enhanced == "managed" {
        let workers = state.workers.clone();
        let model = state.data.join(inference::CLEANUP.name);
        std::thread::spawn(move || {
            if inference::preload(&workers, &model, Kind::Llama, &cleanup_worker, false).is_ok() {
                inference::warm_cleanup(&workers, &model, &cleanup_worker);
            }
        });
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut chunker = Chunker::default();
        let captured = audio::capture(
            &microphone,
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
            |samples| {
                if let Some(range) = chunker.poll(samples) {
                    transcriber.push(samples[range].to_vec());
                } else if let Some(range) = chunker.settle(samples) {
                    transcriber.speculate(range.clone(), samples[range].to_vec());
                }
            },
        );
        let state = handle.state::<AppState>();
        match captured {
            Ok(Some(audio::Captured { wav, samples })) => {
                let reuse = chunker.settled(&samples);
                if let Some(range) = chunker.tail(&samples) {
                    transcriber.push_tail(samples[range].to_vec(), reuse);
                }
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
                        inner.stopped = Some(Instant::now());
                    }
                    inner.stop = None;
                    inner.retry = Some((wav.clone(), snapshot.clone()));
                }
                process(&handle, generation, wav, snapshot, Some(transcriber));
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
        inner.stopped = Some(Instant::now());
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
/// Path of the selected recognizer once it and the voice detection model are
/// on disk.
fn recognition_model(data: &std::path::Path, settings: &Settings) -> Result<PathBuf, String> {
    let model = inference::recognizer(&settings.recognizer)
        .ok_or("Choose a recognition model in Settings")?;
    if !inference::missing(data, model.id)?.is_empty() {
        return Err("Download the recognition model before recording".into());
    }
    Ok(data.join(model.name))
}
fn process(
    app: &AppHandle,
    generation: u64,
    wav: Vec<u8>,
    snapshot: Snapshot,
    chunked: Option<Transcriber>,
) {
    let state = app.state::<AppState>();
    let (worker, model, stopped) = {
        let mut inner = state.inner.lock().unwrap();
        if inner.session.generation != generation {
            return;
        }
        (
            inner.worker.clone(),
            recognition_model(&state.data, &inner.settings),
            inner.stopped.take(),
        )
    };
    let mut watch = Stopwatch::start(stopped.unwrap_or_else(Instant::now));
    if stopped.is_some() {
        watch.lap("capture");
    }
    let mut speculative = false;
    // Enhanced cleanup of each chunk, when it ran during the recording.
    let mut cleaned = None;
    let mut recognized_at = None;
    // Chunks were recognized during the recording. If that failed, recognize
    // the whole recording once, as Retry does.
    let recognition = match chunked.map(Transcriber::finish) {
        Some(Ok(done)) => {
            speculative = done.speculative;
            cleaned = usable_cleaning(&done.parts, done.cleaned, &snapshot);
            recognized_at = Some(done.recognized_at);
            Ok(done.text)
        }
        _ => model.and_then(|model| {
            inference::transcribe(
                &state.workers,
                &model,
                wav,
                &core::recognition_hint(&snapshot),
                &worker,
                false,
            )
        }),
    };
    let recognition_stage = if speculative {
        "recognition (speculative)"
    } else {
        "recognition"
    };
    match recognized_at {
        // Cleaning the tail ran after this and ends at `finish`.
        Some(at) => watch.lap_at(recognition_stage, at),
        None => watch.lap(recognition_stage),
    }
    if cleaned.is_some() {
        watch.lap("cleanup");
        note_model_time(&mut watch, &worker);
    }
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
    watch.lap("preparation");
    if snapshot.cleanup.enhanced != "disabled" {
        let cleaned_during_recording = cleaned.is_some();
        let outcome = cleaned.unwrap_or_else(|| {
            inference::enhanced(inference::CleanupRequest {
                workers: &state.workers,
                data: &state.data,
                snapshot: &snapshot,
                raw: &raw,
                worker: &worker,
                cpu: false,
            })
        });
        apply_enhanced(&mut result, outcome, &snapshot);
        if !cleaned_during_recording {
            watch.lap("cleanup");
            note_model_time(&mut watch, &worker);
        }
    }
    let mut inner = state.inner.lock().unwrap();
    if !inner.session.complete(generation) {
        return;
    }
    let paste = inner.settings.auto_paste;
    let delivered = delivery::deliver(&mut platform::NativeDesktop::default(), &result.text, paste);
    watch.lap("delivery");
    let summary = watch.summary();
    eprintln!("{summary}");
    match delivered {
        Ok(status) => {
            inner.indicator_notice =
                crate::indicator::delivery_notice(&status, result.text.is_empty())
                    .map(|text| (Instant::now(), text));
            result.status.push_str(&format!(". {status}"));
        }
        Err(error) => inner.error = error,
    }
    result.status.push_str(&format!(". {summary}"));
    match history::record(&state.data, inner.settings.keep_history, &result) {
        Ok(id) => inner.history_id = id,
        Err(error) => {
            inner.history_id = None;
            if inner.error.is_empty() {
                inner.error = error;
            }
        }
    }
    inner.result = Some(result);
    inner.retry = None;
}
/// Shows the time the cleanup model reported for its latest request next to
/// the cleanup stage, so a slow model shows apart from slow work around it.
fn note_model_time(watch: &mut Stopwatch, worker: &inference::Worker) {
    if let Some(time) = inference::cleanup_model_time(worker) {
        watch.note(format!("model {} ms", time.as_millis()));
    }
}
/// The cleaning done during the recording, unless a mapping or a quotation
/// spans two parts. Each part was masked alone, so such a span was not
/// protected, and the whole text is cleaned after the stop instead.
fn usable_cleaning(
    parts: &[String],
    cleaned: Option<Result<String, String>>,
    snapshot: &Snapshot,
) -> Option<Result<String, String>> {
    cleaned.filter(|_| core::parts_mask_like_whole(parts, snapshot))
}
/// Puts the outcome of Enhanced cleanup into the prepared result: the cleaned
/// text on success, or the Basic text and the reason on failure.
fn apply_enhanced(
    result: &mut core::ResultText,
    outcome: Result<String, String>,
    snapshot: &Snapshot,
) {
    match outcome {
        Ok(text) => {
            result.text = text;
            result.edits.clear();
            result.suggestions = core::suggestions(&result.text, snapshot);
            core::auto_accept(result, snapshot);
            result.status = "Enhanced cleanup completed; semantic equivalence is unverified".into();
        }
        Err(error) => result.status = error,
    }
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
    std::thread::spawn(move || process(&app, generation, wav, snapshot, None));
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
    sync_history(&state.data, &inner)
}
/// Saves the current result's text to its history entry after the result changed.
fn sync_history(data: &std::path::Path, inner: &Inner) -> Result<(), String> {
    match (inner.history_id, &inner.result) {
        (Some(id), Some(result)) => history::set_text(data, id, &result.text),
        _ => Ok(()),
    }
}
#[tauri::command]
pub fn accept(state: State<'_, AppState>, index: usize, candidate: usize) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    let result = inner.result.as_mut().ok_or("No result")?;
    core::accept_suggestion(result, index, candidate)?;
    sync_history(&state.data, &inner)
}
/// Accepts a suggestion and saves a mapping so the same phrase is replaced
/// automatically from the next recording on.
#[tauri::command]
pub fn accept_always(
    state: State<'_, AppState>,
    index: usize,
    candidate: usize,
) -> Result<(), String> {
    let mut guard = state.inner.lock().unwrap();
    let inner = &mut *guard;
    let result = inner.result.as_mut().ok_or("No result")?;
    let learned = core::mapping_for(result, index, candidate)?;
    core::accept_suggestion(result, index, candidate)?;
    let known = inner.settings.mappings.iter().any(|m| {
        m.repository == learned.repository
            && m.replacement == learned.replacement
            && m.phrase.eq_ignore_ascii_case(&learned.phrase)
    });
    if !known {
        let mut settings = inner.settings.clone();
        settings.mappings.push(learned);
        storage::save(&state.data, &settings)?;
        inner.settings = settings;
    }
    sync_history(&state.data, inner)
}
#[tauri::command]
pub fn undo(state: State<'_, AppState>, index: usize) -> Result<(), String> {
    let mut inner = state.inner.lock().unwrap();
    core::undo_edit(inner.result.as_mut().ok_or("No result")?, index)?;
    sync_history(&state.data, &inner)
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
/// One page of the history, newest first, for the History page.
#[tauri::command]
pub async fn history_page(
    app: AppHandle,
    query: String,
    offset: usize,
    limit: usize,
) -> Result<history::Page, String> {
    let dir = app.state::<AppState>().data.clone();
    tauri::async_runtime::spawn_blocking(move || {
        history::page(&dir, &query, offset, limit.min(200))
    })
    .await
    .map_err(|_| "History worker failed")?
}
#[tauri::command]
pub fn history_copy(state: State<'_, AppState>, id: i64, raw: bool) -> Result<(), String> {
    let entry = history::get(&state.data, id)?;
    platform::NativeDesktop::default().copy(if raw { &entry.raw } else { &entry.text })
}
#[tauri::command]
pub fn history_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    history::delete(&state.data, id)
}
#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> Result<(), String> {
    history::clear(&state.data)
}
#[tauri::command]
pub async fn download(app: AppHandle, id: String) -> Result<(), String> {
    let wanted = inference::missing(&app.state::<AppState>().data, &id)?;
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
            // One progress bar across every file this download needs.
            let total: u64 = wanted.iter().map(|m| m.size).sum();
            let mut finished = 0;
            for model in wanted {
                inference::download(&state.data, model, |current, _| {
                    let _ = app.emit("model-progress", (finished + current, total));
                })?;
                finished += model.size;
            }
            Ok(())
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
    inference::reap_orphans(&workers);
    let reaped = workers.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(30));
            inference::reap_idle(&reaped);
        }
    });
    app.manage(AppState {
        data,
        workers,
        inner: Mutex::new(Inner {
            settings: settings.clone(),
            session: Session::default(),
            result: None,
            history_id: None,
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
            stopped: None,
            recorded_seconds: 0,
            indicator_notice: None,
        }),
    });
    app.manage(crate::update::Tracker::new());
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
    if let Err(error) = crate::tray::setup(app.handle()) {
        app.state::<AppState>().inner.lock().unwrap().error = error;
    }
    crate::update::watch(app.handle());
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
/// Names of the input devices, read when Settings opens and not on every poll.
#[tauri::command]
pub async fn microphones() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(audio::input_names)
        .await
        .map_err(|_| "Cannot list microphones".to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleaned_parts_are_used_only_when_they_protect_what_the_whole_text_protects() {
        let snapshot = Snapshot {
            mappings: vec![core::Mapping {
                phrase: "alpha beta".into(),
                replacement: "Widget".into(),
                repository: None,
                enabled: true,
            }],
            ..Default::default()
        };
        let cleaned = Some(Ok("Say alpha beta.".to_string()));
        let parts = |texts: &[&str]| texts.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        assert_eq!(
            usable_cleaning(&parts(&["say alpha", "beta."]), cleaned.clone(), &snapshot),
            None
        );
        assert_eq!(
            usable_cleaning(&parts(&["say alpha beta."]), cleaned.clone(), &snapshot),
            cleaned
        );
        assert_eq!(
            usable_cleaning(&parts(&["say alpha", "beta."]), None, &snapshot),
            None
        );
    }
    #[test]
    fn enhanced_cleanup_replaces_the_prepared_text_and_a_failure_keeps_it() {
        let snapshot = Snapshot::default();
        let mut result = core::prepare("hello world", &snapshot);
        apply_enhanced(&mut result, Ok("Hello, world.".into()), &snapshot);
        assert_eq!(result.text, "Hello, world.");
        assert!(result.edits.is_empty());
        assert_eq!(
            result.status,
            "Enhanced cleanup completed; semantic equivalence is unverified"
        );
        let mut result = core::prepare("hello world", &snapshot);
        apply_enhanced(
            &mut result,
            Err("Cleanup failed; using Basic".into()),
            &snapshot,
        );
        assert_eq!(result.text, "hello world");
        assert_eq!(result.status, "Cleanup failed; using Basic");
    }
    #[test]
    fn settings_must_name_an_offered_recognition_model() {
        let mut settings = Settings::default();
        assert!(choices_valid(&settings));
        for id in ["base.en", "small.en"] {
            settings.recognizer = id.into();
            assert!(choices_valid(&settings));
        }
        settings.recognizer = "large-v3".into();
        assert!(!choices_valid(&settings));
    }
    #[test]
    fn enhanced_cleanup_is_disabled_or_one_of_three_providers() {
        let mut settings = Settings::default();
        for choice in ["disabled", "managed", "local", "remote"] {
            settings.cleanup.enhanced = choice.into();
            assert!(choices_valid(&settings), "{choice}");
        }
        for choice in ["", "off", "basic", "enhanced"] {
            settings.cleanup.enhanced = choice.into();
            assert!(!choices_valid(&settings), "{choice}");
        }
    }
    #[test]
    fn only_local_and_remote_cleanup_need_a_valid_endpoint() {
        let mut cleanup = crate::core::Cleanup::default();
        assert!(check_enhanced(&cleanup).is_ok());
        cleanup.enhanced = "managed".into();
        assert!(check_enhanced(&cleanup).is_ok());
        cleanup.enhanced = "local".into();
        assert!(check_enhanced(&cleanup).is_err());
        cleanup.endpoint = "http://127.0.0.1:8080/v1/chat/completions".into();
        assert!(check_enhanced(&cleanup).is_ok());
        cleanup.enhanced = "remote".into();
        cleanup.endpoint = "https://api.example.com/v1/chat/completions".into();
        assert!(
            check_enhanced(&cleanup).is_err(),
            "remote needs confirmation"
        );
        cleanup.remote_enabled = true;
        assert!(check_enhanced(&cleanup).is_ok());
    }
    #[test]
    fn recording_needs_the_selected_recognizer_and_the_voice_detection_model() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            recognizer: "base.en".into(),
            ..Default::default()
        };
        let error = recognition_model(dir.path(), &settings).unwrap_err();
        assert!(error.starts_with("Download the recognition model"));
        std::fs::write(dir.path().join(inference::RECOGNIZERS[1].name), b"x").unwrap();
        assert!(recognition_model(dir.path(), &settings).is_err());
        std::fs::write(dir.path().join(inference::VAD.name), b"x").unwrap();
        assert_eq!(
            recognition_model(dir.path(), &settings).unwrap(),
            dir.path().join("ggml-base.en.bin")
        );
        settings.recognizer = "small.en".into();
        assert!(recognition_model(dir.path(), &settings).is_err());
    }
}
