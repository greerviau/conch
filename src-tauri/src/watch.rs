//! When the background loop in `app::setup` re-indexes repositories.
use crate::storage::Repository;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How often repositories are re-indexed when the file watcher is unavailable,
/// and how long a failed refresh waits before the next attempt.
pub const POLL: Duration = Duration::from_secs(30);

/// Whether a file event can change a vocabulary index under one of `roots`.
pub fn relevant(event: &notify::Result<notify::Event>, roots: &[PathBuf]) -> bool {
    let Ok(event) = event else {
        return true;
    };
    !event.kind.is_access()
        && (event.paths.is_empty() || event.paths.iter().any(|path| indexed(path, roots)))
}

fn indexed(path: &Path, roots: &[PathBuf]) -> bool {
    let Some(relative) = roots.iter().find_map(|root| path.strip_prefix(root).ok()) else {
        return true;
    };
    relative.components().all(|part| {
        part.as_os_str().to_str().is_none_or(|name| {
            !crate::index::SKIPPED_DIRECTORIES.contains(&name)
                && (!name.starts_with('.') || matches!(name, ".gitignore" | ".ignore"))
        })
    })
}

/// What a refresh indexes for one repository. Selection does not change an index.
type Key = (String, PathBuf, Vec<String>);

fn keys(repositories: &[Repository]) -> Vec<Key> {
    repositories
        .iter()
        .map(|r| (r.id.clone(), r.root.clone(), r.exclusions.clone()))
        .collect()
}

#[derive(Default)]
pub struct Schedule {
    indexed: Option<Vec<Key>>,
    changed_files: bool,
    last: Option<Instant>,
    retry_at: Option<Instant>,
}

impl Schedule {
    /// Whether to re-index now: the repositories differ from the last refresh,
    /// an indexed file changed, or, without a watcher, `POLL` has passed. After a
    /// failure with the same repositories, it waits `POLL` before trying again.
    pub fn due(&self, repositories: &[Repository], watching: bool, now: Instant) -> bool {
        if self.indexed.as_ref() != Some(&keys(repositories)) {
            return true;
        }
        if let Some(at) = self.retry_at {
            return now >= at;
        }
        self.changed_files || (!watching && self.last.is_none_or(|last| now - last >= POLL))
    }
    pub fn refreshed(&mut self, repositories: &[Repository], ok: bool, now: Instant) {
        self.indexed = Some(keys(repositories));
        self.changed_files = false;
        self.last = Some(now);
        self.retry_at = (!ok).then(|| now + POLL);
    }
    pub fn files_changed(&mut self) {
        self.changed_files = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{
        Event, EventKind,
        event::{AccessKind, CreateKind, ModifyKind},
    };

    fn modified(path: &str) -> notify::Result<Event> {
        Ok(Event::new(EventKind::Modify(ModifyKind::Any)).add_path(path.into()))
    }

    #[test]
    fn changes_inside_skipped_and_hidden_directories_do_not_affect_the_index() {
        let roots = [PathBuf::from("/home/me/.config/app")];
        assert!(relevant(
            &modified("/home/me/.config/app/src/main.ts"),
            &roots
        ));
        assert!(relevant(
            &modified("/home/me/.config/app/.gitignore"),
            &roots
        ));
        for path in [
            "/home/me/.config/app/.git/index",
            "/home/me/.config/app/target/debug/build.log",
            "/home/me/.config/app/web/node_modules/x/index.js",
            "/home/me/.config/app/src/.DS_Store",
        ] {
            assert!(!relevant(&modified(path), &roots), "{path}");
        }
    }

    #[test]
    fn reads_are_ignored_and_errors_and_unknown_paths_count_as_changes() {
        let roots = [PathBuf::from("/repo")];
        let read = Ok(Event::new(EventKind::Access(AccessKind::Any)).add_path("/repo/a.ts".into()));
        assert!(!relevant(&read, &roots));
        assert!(relevant(&Err(notify::Error::generic("overflow")), &roots));
        assert!(relevant(&Ok(Event::new(EventKind::Any)), &roots));
        let outside =
            Ok(Event::new(EventKind::Create(CreateKind::File))
                .add_path("/private/repo/a.ts".into()));
        assert!(relevant(&outside, &roots));
    }

    fn repository(root: &str) -> Repository {
        Repository {
            id: "r".into(),
            root: root.into(),
            exclusions: vec![],
        }
    }

    #[test]
    fn a_watched_repository_is_reindexed_only_after_a_change() {
        let now = Instant::now();
        let mut repositories = vec![repository("/repo")];
        let mut schedule = Schedule::default();
        assert!(schedule.due(&repositories, true, now));
        schedule.refreshed(&repositories, true, now);
        assert!(!schedule.due(&repositories, true, now + POLL * 10));
        schedule.files_changed();
        assert!(schedule.due(&repositories, true, now));
        schedule.refreshed(&repositories, true, now);
        repositories[0].exclusions.push("*.gen.ts".into());
        assert!(schedule.due(&repositories, true, now));
    }

    #[test]
    fn without_a_watcher_repositories_are_reindexed_every_poll() {
        let now = Instant::now();
        let repositories = vec![repository("/repo")];
        let mut schedule = Schedule::default();
        schedule.refreshed(&repositories, true, now);
        assert!(!schedule.due(&repositories, false, now + POLL / 2));
        assert!(schedule.due(&repositories, false, now + POLL));
    }

    #[test]
    fn a_failed_refresh_is_retried_after_a_poll_or_when_the_repositories_change() {
        let now = Instant::now();
        let repositories = vec![repository("/repo")];
        let mut schedule = Schedule::default();
        schedule.refreshed(&repositories, false, now);
        schedule.files_changed();
        assert!(!schedule.due(&repositories, true, now + POLL / 2));
        assert!(schedule.due(&repositories, true, now + POLL));
        assert!(schedule.due(&[repository("/other")], true, now));
    }
}
