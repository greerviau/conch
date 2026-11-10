use crate::{app::AppState, inference, session::Phase};
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
}
impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

/// The menu bar item whose label follows the update status.
pub struct MenuEntry(pub MenuItem<tauri::Wry>);

fn set_status(app: &AppHandle, status: Status) {
    if let Some(entry) = app.try_state::<MenuEntry>() {
        let _ = entry.0.set_text(menu_label(&status));
        let _ = entry
            .0
            .set_enabled(!matches!(status, Status::Checking | Status::Installing));
    }
    app.state::<Tracker>().inner.lock().unwrap().0 = status;
}

fn busy(app: &AppHandle) -> bool {
    matches!(
        app.state::<Tracker>().status(),
        Status::Checking | Status::Installing
    )
}

async fn find(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    app.updater()
        .map_err(|_| "Updates are not configured in this build".to_string())?
        .check()
        .await
        .map_err(|_| "Cannot reach the update server".to_string())
}

/// Asks the update server whether a newer release exists. Blocks the calling
/// thread, so call it off the main thread.
pub fn check_now(app: &AppHandle) {
    if busy(app) {
        return;
    }
    set_status(app, Status::Checking);
    let found = tauri::async_runtime::block_on(find(app));
    let status = match &found {
        Ok(Some(update)) => Status::Available(update.version.clone()),
        Ok(None) => Status::Current,
        Err(message) => Status::Failed(message.clone()),
    };
    app.state::<Tracker>().inner.lock().unwrap().1 = Some(next_due(Instant::now(), found.is_ok()));
    set_status(app, status);
}

/// Downloads and installs the available update, waits for any recording to
/// finish, and restarts Conch. Blocks the calling thread.
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
    if busy(app) {
        return Err("An update is already in progress".into());
    }
    set_status(app, Status::Installing);
    let installed = tauri::async_runtime::block_on(async {
        let update = find(app).await?.ok_or("Conch is up to date")?;
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|_| "The update could not be installed".to_string())
    });
    if let Err(message) = installed {
        set_status(app, Status::Failed(message.clone()));
        return Err(message);
    }
    // The new version is on disk. A dictation that began during the download
    // finishes first.
    while install_blocked(&phase(app)).is_some() {
        std::thread::sleep(Duration::from_millis(200));
    }
    // `restart` exits without delivering the exit event, so stop the workers here.
    let _ = crate::app::cancel(app.clone());
    inference::shutdown(&app.state::<AppState>().workers);
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
            let due = app.state::<Tracker>().inner.lock().unwrap().1;
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
}
