use crate::{
    core::{Cleanup, Mapping, Snapshot, Vocabulary},
    index::Index,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub root: PathBuf,
    pub selected: bool,
    pub exclusions: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub repositories: Vec<Repository>,
    pub mappings: Vec<Mapping>,
    pub cleanup: Cleanup,
    pub hotkey: String,
    pub activation: String,
    pub show_overlay: bool,
    pub check_updates: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            repositories: vec![],
            mappings: vec![],
            cleanup: Cleanup::default(),
            hotkey: "CommandOrControl+Shift+Space".into(),
            activation: "push".into(),
            show_overlay: true,
            check_updates: true,
        }
    }
}
pub fn open(dir: &Path) -> Result<rusqlite::Connection, String> {
    std::fs::create_dir_all(dir).map_err(|_| "Cannot create local data directory")?;
    let db = rusqlite::Connection::open(dir.join("conch.sqlite3"))
        .map_err(|_| "Cannot open local database")?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS indexes (repository TEXT PRIMARY KEY, value TEXT NOT NULL, refreshed INTEGER NOT NULL);").map_err(|_| "Cannot initialize database")?;
    Ok(db)
}
pub fn load(dir: &Path) -> Result<Settings, String> {
    use rusqlite::OptionalExtension;
    let json: Option<String> = open(dir)?
        .query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))
        .optional()
        .map_err(|_| "Cannot read settings")?;
    json.map(|j| serde_json::from_str(&j).map_err(|_| "Settings are invalid".into()))
        .unwrap_or_else(|| Ok(Settings::default()))
}
pub fn save(dir: &Path, settings: &Settings) -> Result<(), String> {
    let mut db = open(dir)?;
    let tx = db.transaction().map_err(|_| "Cannot save settings")?;
    tx.execute(
        "INSERT OR REPLACE INTO settings VALUES (1,?1)",
        [serde_json::to_string(settings).map_err(|_| "Cannot encode settings")?],
    )
    .map_err(|_| "Cannot save settings")?;
    let ids: Vec<String> = {
        let mut statement = tx
            .prepare("SELECT repository FROM indexes")
            .map_err(|_| "Cannot read index list")?;
        statement
            .query_map([], |r| r.get(0))
            .map_err(|_| "Cannot read index list")?
            .collect::<Result<_, _>>()
            .map_err(|_| "Cannot read index list")?
    };
    for id in ids {
        if !settings.repositories.iter().any(|r| r.id == id) {
            tx.execute("DELETE FROM indexes WHERE repository=?1", [id])
                .map_err(|_| "Cannot remove index")?;
        }
    }
    tx.commit().map_err(|_| "Cannot commit settings".into())
}
pub fn put_index(dir: &Path, repository: &str, index: &Index) -> Result<(), String> {
    open(dir)?
        .execute(
            "INSERT OR REPLACE INTO indexes VALUES (?1,?2,unixepoch())",
            rusqlite::params![
                repository,
                serde_json::to_string(index).map_err(|_| "Cannot encode index")?
            ],
        )
        .map_err(|_| "Cannot save index")?;
    Ok(())
}
pub fn indexes(dir: &Path) -> Result<Vec<(String, Index, i64)>, String> {
    let db = open(dir)?;
    let mut statement = db
        .prepare("SELECT repository,value,refreshed FROM indexes")
        .map_err(|_| "Cannot read indexes")?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(|_| "Cannot read indexes")?;
    rows.map(|r| {
        let (id, json, time) = r.map_err(|_| "Cannot read index")?;
        Ok((
            id,
            serde_json::from_str(&json).map_err(|_| "Invalid index")?,
            time,
        ))
    })
    .collect()
}
pub fn snapshot(dir: &Path, settings: &Settings) -> Result<Snapshot, String> {
    let selected: Vec<_> = settings
        .repositories
        .iter()
        .filter(|r| r.selected && r.root.is_dir())
        .map(|r| r.id.as_str())
        .collect();
    let vocabulary = indexes(dir)?
        .into_iter()
        .filter(|(id, index, _)| selected.contains(&id.as_str()) && index.available)
        .flat_map(|(_, index, _)| index.entries)
        .collect();
    let mappings = settings
        .mappings
        .iter()
        .filter(|m| {
            m.repository
                .as_ref()
                .is_none_or(|r| selected.contains(&r.as_str()))
        })
        .cloned()
        .collect();
    Ok(Snapshot {
        mappings,
        vocabulary,
        cleanup: settings.cleanup.clone(),
    })
}
/// What the interface shows for a repository index. Deserializing this skips
/// the vocabulary entries, which are the bulk of the stored JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexStatus {
    pub available: bool,
    pub incomplete: bool,
    pub skipped: usize,
    #[serde(default, deserialize_with = "count_entries")]
    pub entries: usize,
}
/// Counts a JSON list without allocating its elements.
fn count_entries<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
    struct Count;
    impl<'de> serde::de::Visitor<'de> for Count {
        type Value = usize;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a list")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut list: A) -> Result<usize, A::Error> {
            let mut count = 0;
            while list.next_element::<serde::de::IgnoredAny>()?.is_some() {
                count += 1;
            }
            Ok(count)
        }
    }
    deserializer.deserialize_seq(Count)
}
pub fn index_status(dir: &Path) -> Result<Vec<(String, IndexStatus, i64)>, String> {
    let db = open(dir)?;
    let mut statement = db
        .prepare("SELECT repository,value,refreshed FROM indexes")
        .map_err(|_| "Cannot read indexes")?;
    let rows = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(|_| "Cannot read indexes")?;
    rows.map(|r| {
        let (id, json, time) = r.map_err(|_| "Cannot read index")?;
        Ok((
            id,
            serde_json::from_str(&json).map_err(|_| "Invalid index")?,
            time,
        ))
    })
    .collect()
}
/// Which indexed terms the repository page lists. A term is a file term when
/// it is a relative path or a file name; every other term is a declared symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TermKind {
    #[default]
    All,
    Files,
    Symbols,
}
#[derive(Clone, Debug, Serialize)]
pub struct TermPage {
    pub items: Vec<Vocabulary>,
    pub total: usize,
}
fn is_file_term(term: &Vocabulary) -> bool {
    term.text == term.file || term.file.rsplit('/').next() == Some(term.text.as_str())
}
/// One page of a repository's indexed terms that match `query` (a
/// case-insensitive substring of the term) and `kind`. `total` counts every
/// match, not only the page.
pub fn index_terms(
    dir: &Path,
    repository: &str,
    query: &str,
    kind: TermKind,
    offset: usize,
    limit: usize,
) -> Result<TermPage, String> {
    use rusqlite::OptionalExtension;
    let json: Option<String> = open(dir)?
        .query_row(
            "SELECT value FROM indexes WHERE repository=?1",
            [repository],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "Cannot read index")?;
    let Some(json) = json else {
        return Ok(TermPage {
            items: Vec::new(),
            total: 0,
        });
    };
    let index: Index = serde_json::from_str(&json).map_err(|_| "Invalid index")?;
    let query = query.trim().to_lowercase();
    let mut total = 0;
    let mut items = Vec::new();
    for term in index.entries {
        let wanted = match kind {
            TermKind::All => true,
            TermKind::Files => is_file_term(&term),
            TermKind::Symbols => !is_file_term(&term),
        };
        if !wanted || !term.text.to_lowercase().contains(&query) {
            continue;
        }
        if total >= offset && items.len() < limit {
            items.push(term);
        }
        total += 1;
    }
    Ok(TermPage { items, total })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_reports_index_flags_without_needing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let index = Index {
            entries: vec![Vocabulary {
                text: "maxGroups".into(),
                repository: "r".into(),
                file: "a.ts".into(),
            }],
            skipped: 3,
            incomplete: true,
            available: true,
        };
        put_index(dir.path(), "r", &index).unwrap();
        let status = index_status(dir.path()).unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].0, "r");
        assert!(status[0].1.available && status[0].1.incomplete);
        assert_eq!(status[0].1.skipped, 3);
    }
    #[test]
    fn automatic_update_checks_are_on_by_default_and_for_settings_saved_before_they_existed() {
        assert!(Settings::default().check_updates);
        let older = r#"{"hotkey":"Control+Space","activation":"push"}"#;
        let loaded: Settings = serde_json::from_str(older).unwrap();
        assert!(loaded.check_updates);
        let off: Settings = serde_json::from_str(r#"{"check_updates":false}"#).unwrap();
        assert!(!off.check_updates);
    }

    fn term(text: &str, file: &str) -> Vocabulary {
        Vocabulary {
            text: text.into(),
            repository: "r".into(),
            file: file.into(),
        }
    }
    fn sample(dir: &Path) {
        let index = Index {
            entries: vec![
                term("api/users.ts", "api/users.ts"),
                term("users.ts", "api/users.ts"),
                term("fetchUserProfile", "api/users.ts"),
                term("main.py", "main.py"),
                term("FetchError", "lib/http.ts"),
                term("fetch_user_flags", "scripts/flags.py"),
            ],
            skipped: 0,
            incomplete: false,
            available: true,
        };
        put_index(dir, "r", &index).unwrap();
    }
    #[test]
    fn status_counts_entries_without_loading_them() {
        let dir = tempfile::tempdir().unwrap();
        sample(dir.path());
        assert_eq!(index_status(dir.path()).unwrap()[0].1.entries, 6);
    }
    #[test]
    fn terms_come_back_in_pages_with_the_total() {
        let dir = tempfile::tempdir().unwrap();
        sample(dir.path());
        let page = index_terms(dir.path(), "r", "", TermKind::All, 2, 3).unwrap();
        assert_eq!(page.total, 6);
        let texts: Vec<_> = page.items.iter().map(|v| v.text.as_str()).collect();
        assert_eq!(texts, ["fetchUserProfile", "main.py", "FetchError"]);
        let past_end = index_terms(dir.path(), "r", "", TermKind::All, 60, 3).unwrap();
        assert_eq!((past_end.items.len(), past_end.total), (0, 6));
    }
    #[test]
    fn terms_search_ignores_case_and_counts_only_matches() {
        let dir = tempfile::tempdir().unwrap();
        sample(dir.path());
        let page = index_terms(dir.path(), "r", "FETCH", TermKind::All, 0, 10).unwrap();
        assert_eq!(page.total, 3);
        assert!(
            page.items
                .iter()
                .all(|v| v.text.to_lowercase().contains("fetch"))
        );
    }
    #[test]
    fn terms_filter_separates_file_names_from_symbols() {
        let dir = tempfile::tempdir().unwrap();
        sample(dir.path());
        let files = index_terms(dir.path(), "r", "", TermKind::Files, 0, 10).unwrap();
        let texts: Vec<_> = files.items.iter().map(|v| v.text.as_str()).collect();
        assert_eq!(texts, ["api/users.ts", "users.ts", "main.py"]);
        let symbols = index_terms(dir.path(), "r", "", TermKind::Symbols, 0, 10).unwrap();
        assert_eq!(symbols.total, 3);
    }
    #[test]
    fn terms_of_an_unindexed_repository_are_empty() {
        let dir = tempfile::tempdir().unwrap();
        let page = index_terms(dir.path(), "missing", "", TermKind::All, 0, 10).unwrap();
        assert_eq!((page.items.len(), page.total), (0, 0));
    }
}
