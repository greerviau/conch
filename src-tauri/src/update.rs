use crate::{
    app::AppState,
    inference,
    log::{self, Level},
    session::Phase,
};
use serde::Serialize;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, menu::MenuItem};
use tauri_plugin_updater::UpdaterExt;

/// How long Conch waits after a successful check before the next automatic one.
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// How long Conch waits after a failed check, for example while offline.
pub const RETRY: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum Status {
    Idle,
    Checking,
    Current,
    Available(String),
    Installing,
    Failed(String),
}

pub fn check_due(enabled: bool, due: Option<Instant>, now: Instant) -> bool {
    enabled && due.is_none_or(|due| now >= due)
}

pub fn next_due(now: Instant, succeeded: bool) -> Instant {
    now + if succeeded { INTERVAL } else { RETRY }
}

pub fn install_blocked(phase: &Phase) -> Option<&'static str> {
    match phase {
        Phase::Idle => None,
        Phase::Recording | Phase::Processing => {
            Some("Finish or cancel the current recording before updating")
        }
    }
}

pub fn menu_label(status: &Status) -> String {
    match status {
        Status::Idle | Status::Current | Status::Failed(_) => "Check for Updates".into(),
        Status::Checking => "Checking for Updates...".into(),
        Status::Available(version) => format!("Update to {version}"),
        Status::Installing => "Installing Update...".into(),
    }
}

/// Managed state for the updater: what the interface shows and when the next
/// automatic check runs.
pub struct Tracker {
    inner: Mutex<(Status, Option<Instant>)>,
}
impl Tracker {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new((Status::Idle, None)),
        }
    }
    pub fn status(&self) -> Status {
        self.inner.lock().unwrap().0.clone()
    }
    /// Moves to `next` unless a check or install is already running, in one step.
    pub fn begin(&self, next: Status) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if matches!(inner.0, Status::Checking | Status::Installing) {
            return false;
        }
        inner.0 = next;
        true
    }
    pub fn set(&self, status: Status) {
        self.inner.lock().unwrap().0 = status;
    }
    fn schedule(&self, due: Instant) {
        self.inner.lock().unwrap().1 = Some(due);
    }
    fn due(&self) -> Option<Instant> {
        self.inner.lock().unwrap().1
    }
}
impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

/// The menu bar item whose label follows the update status.
pub struct MenuEntry(pub MenuItem<tauri::Wry>);

fn show(app: &AppHandle, status: &Status) {
    if let Some(entry) = app.try_state::<MenuEntry>() {
        let _ = entry.0.set_text(menu_label(status));
        let _ = entry
            .0
            .set_enabled(!matches!(status, Status::Checking | Status::Installing));
    }
}

/// The log line for a change of update status, or nothing for a status that
/// only passes through.
fn log_line(status: &Status, version: &str) -> Option<(Level, String)> {
    match status {
        Status::Current => Some((
            Level::Info,
            format!("Checked for updates: {version} is the latest version"),
        )),
        Status::Available(found) => Some((Level::Info, format!("Update {found} is available"))),
        Status::Installing => Some((Level::Info, "Installing the update".into())),
        Status::Failed(message) => Some((Level::Warn, message.clone())),
        Status::Idle | Status::Checking => None,
    }
}
fn log_status(status: &Status) {
    if let Some((level, message)) = log_line(status, env!("CARGO_PKG_VERSION")) {
        log::record(level, "update", &message, None);
    }
}

fn begin(app: &AppHandle, status: Status) -> bool {
    let started = app.state::<Tracker>().begin(status.clone());
    if started {
        log_status(&status);
        show(app, &status);
    }
    started
}

fn finish(app: &AppHandle, status: Status) {
    log_status(&status);
    show(app, &status);
    app.state::<Tracker>().set(status);
}

async fn find(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    app.updater()
        .map_err(|error| format!("Updates are not configured in this build: {error}"))?
        .check()
        .await
        .map_err(|error| format!("The update check failed: {error}"))
}

/// Asks the update server whether a newer release exists. Blocks the calling
/// thread, so call it off the main thread.
pub fn check_now(app: &AppHandle) {
    if !begin(app, Status::Checking) {
        return;
    }
    let found = tauri::async_runtime::block_on(find(app));
    app.state::<Tracker>()
        .schedule(next_due(Instant::now(), found.is_ok()));
    finish(
        app,
        match found {
            Ok(Some(update)) => Status::Available(update.version),
            Ok(None) => Status::Current,
            Err(message) => Status::Failed(message),
        },
    );
}

/// Downloads the available update, waits for any recording to finish, stops
/// the workers, installs, and restarts Conch. Blocks the calling thread.
pub fn install_now(app: &AppHandle) -> Result<(), String> {
    let phase = |app: &AppHandle| {
        app.state::<AppState>()
            .inner
            .lock()
            .unwrap()
            .session
            .phase
            .clone()
    };
    if let Some(reason) = install_blocked(&phase(app)) {
        return Err(reason.into());
    }
    if !begin(app, Status::Installing) {
        return Err("An update is already in progress".into());
    }
    let downloaded = tauri::async_runtime::block_on(async {
        let Some(update) = find(app).await? else {
            return Ok(None);
        };
        let bytes = update
            .download(|_, _| {}, || {})
            .await
            .map_err(|error| format!("The update could not be downloaded: {error}"))?;
        Ok::<_, String>(Some((update, bytes)))
    });
    let (update, bytes) = match downloaded {
        Ok(Some(found)) => found,
        Ok(None) => {
            finish(app, Status::Current);
            return Err("Conch is up to date".into());
        }
        Err(message) => {
            finish(app, Status::Failed(message.clone()));
            return Err(message);
        }
    };
    // A dictation that began during the download finishes first.
    while install_blocked(&phase(app)).is_some() {
        std::thread::sleep(Duration::from_millis(200));
    }
    // Windows exits when its installer starts, and `restart` exits without the
    // exit event, so stop the workers before either runs. They would otherwise
    // hold the files the installer replaces.
    let _ = crate::app::cancel(app.clone());
    inference::shutdown(&app.state::<AppState>().workers);
    if let Err(error) = update.install(bytes) {
        let message = format!("The update could not be installed: {error}");
        finish(app, Status::Failed(message.clone()));
        return Err(message);
    }
    app.restart()
}

/// Checks shortly after launch and then on the schedule in `next_due`, for as
/// long as the user leaves automatic checks on.
pub fn watch(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        loop {
            let enabled = app
                .state::<AppState>()
                .inner
                .lock()
                .unwrap()
                .settings
                .check_updates;
            let due = app.state::<Tracker>().due();
            if check_due(enabled, due, Instant::now()) {
                check_now(&app);
            }
            std::thread::sleep(Duration::from_secs(60));
        }
    });
}

/// Runs when the user chooses the menu bar item: installs a known update or
/// looks for one.
pub fn menu_selected(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || match app.state::<Tracker>().status() {
        Status::Available(_) => {
            let _ = install_now(&app);
        }
        _ => check_now(&app),
    });
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || check_now(&app))
        .await
        .map_err(|_| "Update check failed".to_string())
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || install_now(&app))
        .await
        .map_err(|_| "Update failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_checks_run_at_launch_and_then_when_the_scheduled_time_arrives() {
        let now = Instant::now();
        assert!(check_due(true, None, now));
        assert!(!check_due(true, Some(now + INTERVAL), now));
        assert!(check_due(true, Some(now + INTERVAL), now + INTERVAL));
    }

    #[test]
    fn disabling_automatic_checks_stops_them_even_when_one_is_overdue() {
        let now = Instant::now();
        assert!(!check_due(false, None, now));
        assert!(!check_due(false, Some(now), now + INTERVAL * 3));
    }

    #[test]
    fn a_failed_check_is_retried_within_the_hour_and_a_successful_one_in_a_day() {
        let now = Instant::now();
        assert_eq!(next_due(now, true), now + INTERVAL);
        assert_eq!(next_due(now, false), now + RETRY);
        assert!(RETRY < INTERVAL);
    }

    #[test]
    fn installing_waits_until_no_recording_or_result_is_in_progress() {
        assert_eq!(install_blocked(&Phase::Idle), None);
        assert!(install_blocked(&Phase::Recording).is_some());
        assert!(install_blocked(&Phase::Processing).is_some());
    }

    #[test]
    fn the_menu_item_offers_the_next_action_for_each_status() {
        assert_eq!(menu_label(&Status::Idle), "Check for Updates");
        assert_eq!(menu_label(&Status::Current), "Check for Updates");
        assert_eq!(
            menu_label(&Status::Failed("offline".into())),
            "Check for Updates"
        );
        assert_eq!(menu_label(&Status::Checking), "Checking for Updates...");
        assert_eq!(
            menu_label(&Status::Available("0.3.0".into())),
            "Update to 0.3.0"
        );
        assert_eq!(menu_label(&Status::Installing), "Installing Update...");
    }

    #[test]
    fn only_one_check_or_install_runs_at_a_time() {
        let tracker = Tracker::new();
        assert!(tracker.begin(Status::Checking));
        assert!(!tracker.begin(Status::Checking));
        assert!(!tracker.begin(Status::Installing));
        tracker.set(Status::Available("0.3.0".into()));
        assert!(tracker.begin(Status::Installing));
        assert!(!tracker.begin(Status::Checking));
        tracker.set(Status::Failed("offline".into()));
        assert!(tracker.begin(Status::Checking));
    }

    #[test]
    fn the_interface_receives_the_status_as_a_tagged_value() {
        let json = serde_json::to_value(Status::Available("0.3.0".into())).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state": "available", "detail": "0.3.0"})
        );
        assert_eq!(
            serde_json::to_value(Status::Current).unwrap(),
            serde_json::json!({"state": "current"})
        );
    }
    #[test]
    fn each_outcome_of_an_update_is_logged_and_a_passing_status_is_not() {
        let line = |status| log_line(&status, "0.3.0");
        assert_eq!(
            line(Status::Current),
            Some((
                Level::Info,
                "Checked for updates: 0.3.0 is the latest version".into()
            ))
        );
        assert_eq!(
            line(Status::Available("0.4.0".into())),
            Some((Level::Info, "Update 0.4.0 is available".into()))
        );
        assert_eq!(
            line(Status::Installing),
            Some((Level::Info, "Installing the update".into()))
        );
        assert_eq!(
            line(Status::Failed("The update check failed: offline".into())),
            Some((Level::Warn, "The update check failed: offline".into()))
        );
        assert_eq!(line(Status::Checking), None);
        assert_eq!(line(Status::Idle), None);
    }
}
