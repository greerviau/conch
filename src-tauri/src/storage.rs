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
    /// Whether to paste into the focused field. Off copies the result only.
    pub auto_paste: bool,
    /// Whether a repository suggestion with one candidate is applied without review.
    pub auto_accept_suggestions: bool,
    /// Whether delivered results are saved to the history on disk.
    pub keep_history: bool,
    /// Name of the input device. Empty follows the system input.
    pub microphone: String,
    /// Id of the recognition model, from `inference::RECOGNIZERS`.
    #[serde(default = "legacy_recognizer")]
    pub recognizer: String,
    /// ISO code from `languages::LANGUAGES`, or `auto`.
    pub language: String,
    pub show_overlay: bool,
    pub check_updates: bool,
}
/// Only Tiny existed before the choice, so settings saved without the field belong to a copy
/// that has Tiny installed and nothing else. A new install uses `Settings::default` instead.
fn legacy_recognizer() -> String {
    "tiny.en".into()
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            repositories: vec![],
            mappings: vec![],
            cleanup: Cleanup::default(),
            hotkey: "CommandOrControl+Shift+Space".into(),
            activation: "push".into(),
            auto_paste: true,
            auto_accept_suggestions: false,
            keep_history: false,
            microphone: String::new(),
            recognizer: "small.en".into(),
            language: "en".into(),
            show_overlay: true,
            check_updates: true,
        }
    }
}
pub fn open(dir: &Path) -> Result<rusqlite::Connection, String> {
    std::fs::create_dir_all(dir).map_err(|_| "Cannot create local data directory")?;
    let db = rusqlite::Connection::open(dir.join("conch.sqlite3"))
        .map_err(|_| "Cannot open local database")?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS indexes (repository TEXT PRIMARY KEY, value TEXT NOT NULL, refreshed INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS history (id INTEGER PRIMARY KEY AUTOINCREMENT, created INTEGER NOT NULL, raw TEXT NOT NULL, text TEXT NOT NULL);").map_err(|_| "Cannot initialize database")?;
    Ok(db)
}
pub fn load(dir: &Path) -> Result<Settings, String> {
    use rusqlite::OptionalExtension;
    let json: Option<String> = open(dir)?
        .query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))
        .optional()
        .map_err(|_| "Cannot read settings")?;
    let mut settings: Settings = json
        .map(|j| serde_json::from_str(&j).map_err(|_| "Settings are invalid".to_string()))
        .unwrap_or_else(|| Ok(Settings::default()))?;
    // A model id this build does not offer would make every settings save fail.
    if crate::inference::recognizer(&settings.recognizer).is_none() {
        settings.recognizer = Settings::default().recognizer;
    }
    if !crate::languages::known(&settings.language) {
        settings.language = Settings::default().language;
    }
    // Keep the language and move to a model that can recognize it. If that model
    // is not installed, recording asks for the download.
    if crate::inference::recognizer(&settings.recognizer)
        .is_some_and(|model| !model.supports(&settings.language))
        && let Some(model) = crate::inference::smallest_supporting(&settings.language)
    {
        settings.recognizer = model.id.into();
    }
    Ok(settings)
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
        auto_accept: settings.auto_accept_suggestions,
        language: settings.language.clone(),
        recognizer: settings.recognizer.clone(),
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

    #[test]
    fn auto_accepting_repository_suggestions_is_opt_in_and_captured_in_the_snapshot() {
        assert!(!Settings::default().auto_accept_suggestions);
        let older: Settings = serde_json::from_str(r#"{"hotkey":"Control+Space"}"#).unwrap();
        assert!(!older.auto_accept_suggestions);
        let dir = tempfile::tempdir().unwrap();
        assert!(!snapshot(dir.path(), &older).unwrap().auto_accept);
        let on: Settings = serde_json::from_str(r#"{"auto_accept_suggestions":true}"#).unwrap();
        assert!(snapshot(dir.path(), &on).unwrap().auto_accept);
    }
    #[test]
    fn keeping_history_is_opt_in() {
        assert!(!Settings::default().keep_history);
        let older: Settings = serde_json::from_str(r#"{"hotkey":"Control+Space"}"#).unwrap();
        assert!(!older.keep_history);
        let on: Settings = serde_json::from_str(r#"{"keep_history":true}"#).unwrap();
        assert!(on.keep_history);
    }
    #[test]
    fn a_new_install_defaults_to_small_en_and_settings_saved_before_the_choice_keep_tiny_en() {
        assert_eq!(Settings::default().recognizer, "small.en");
        // Only Tiny existed before the choice, so a copy that saved settings then has it installed.
        let older: Settings = serde_json::from_str(r#"{"hotkey":"Control+Space"}"#).unwrap();
        assert_eq!(older.recognizer, "tiny.en");
        let chosen: Settings = serde_json::from_str(r#"{"recognizer":"base.en"}"#).unwrap();
        assert_eq!(chosen.recognizer, "base.en");
    }

    #[test]
    fn a_first_launch_with_no_saved_settings_loads_small_en() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap().recognizer, "small.en");
    }

    #[test]
    fn automatic_paste_is_on_and_the_microphone_follows_the_system_by_default() {
        let defaults = Settings::default();
        assert!(defaults.auto_paste);
        assert_eq!(defaults.microphone, "");
        let older: Settings = serde_json::from_str(r#"{"hotkey":"Control+Space"}"#).unwrap();
        assert!(older.auto_paste);
        assert_eq!(older.microphone, "");
        let chosen: Settings =
            serde_json::from_str(r#"{"auto_paste":false,"microphone":"USB"}"#).unwrap();
        assert!(!chosen.auto_paste);
        assert_eq!(chosen.microphone, "USB");
    }

    #[test]
    fn an_unknown_stored_recognizer_loads_as_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            recognizer: "huge".into(),
            ..Default::default()
        };
        save(dir.path(), &settings).unwrap();
        assert_eq!(load(dir.path()).unwrap().recognizer, "small.en");
        settings.recognizer = "tiny.en".into();
        save(dir.path(), &settings).unwrap();
        assert_eq!(load(dir.path()).unwrap().recognizer, "tiny.en");
    }

    #[test]
    fn a_new_install_and_settings_saved_before_the_language_choice_use_english() {
        assert_eq!(Settings::default().language, "en");
        let older: Settings = serde_json::from_str(r#"{"hotkey":"Control+Space"}"#).unwrap();
        assert_eq!(older.language, "en");
        let chosen: Settings = serde_json::from_str(r#"{"language":"fr"}"#).unwrap();
        assert_eq!(chosen.language, "fr");
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap().language, "en");
    }

    #[test]
    fn an_unknown_stored_language_loads_as_english_and_known_ones_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            language: "xx".into(),
            ..Default::default()
        };
        save(dir.path(), &settings).unwrap();
        assert_eq!(load(dir.path()).unwrap().language, "en");
        for (code, recognizer) in [
            ("fr", "small"),
            ("auto", "small"),
            ("yue", "large-v3-turbo"),
        ] {
            settings.language = code.into();
            settings.recognizer = recognizer.into();
            save(dir.path(), &settings).unwrap();
            let loaded = load(dir.path()).unwrap();
            assert_eq!(
                (loaded.language.as_str(), loaded.recognizer.as_str()),
                (code, recognizer)
            );
        }
    }

    #[test]
    fn a_saved_model_that_lacks_the_saved_language_is_replaced_by_the_smallest_that_has_it() {
        let dir = tempfile::tempdir().unwrap();
        for (language, recognizer) in [("fr", "small.en"), ("auto", "base.en")] {
            let settings = Settings {
                language: language.into(),
                recognizer: recognizer.into(),
                ..Default::default()
            };
            save(dir.path(), &settings).unwrap();
            let loaded = load(dir.path()).unwrap();
            assert_eq!(loaded.language, language);
            assert_eq!(loaded.recognizer, "tiny");
        }
        for recognizer in ["small.en", "small"] {
            let cantonese = Settings {
                language: "yue".into(),
                recognizer: recognizer.into(),
                ..Default::default()
            };
            save(dir.path(), &cantonese).unwrap();
            assert_eq!(load(dir.path()).unwrap().recognizer, "large-v3-turbo");
        }
        let kept = Settings {
            language: "de".into(),
            recognizer: "large-v3-turbo".into(),
            ..Default::default()
        };
        save(dir.path(), &kept).unwrap();
        assert_eq!(load(dir.path()).unwrap().recognizer, "large-v3-turbo");
    }

    #[test]
    fn a_hesitation_list_saved_before_languages_loads_as_the_english_list() {
        let dir = tempfile::tempdir().unwrap();
        open(dir.path())
            .unwrap()
            .execute(
                "INSERT OR REPLACE INTO settings VALUES (1, ?1)",
                [r#"{"cleanup":{"fillers":["um","er"],"enhanced":"local"}}"#],
            )
            .unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.cleanup.fillers["en"], ["um", "er"]);
        assert_eq!(loaded.cleanup.fillers.len(), 1);
        assert_eq!(loaded.cleanup.enhanced, "local");
        assert_eq!(loaded.language, "en");
        save(dir.path(), &loaded).unwrap();
        assert_eq!(
            load(dir.path()).unwrap().cleanup.fillers["en"],
            ["um", "er"]
        );
        let snapshot = snapshot(dir.path(), &loaded).unwrap();
        assert_eq!(snapshot.cleanup.fillers["en"], ["um", "er"]);
    }
    #[test]
    fn the_snapshot_captures_the_language_and_the_model_the_recording_starts_with() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            language: "pt".into(),
            recognizer: "base".into(),
            ..Default::default()
        };
        let snapshot = snapshot(dir.path(), &settings).unwrap();
        assert_eq!(snapshot.language, "pt");
        assert_eq!(snapshot.recognizer, "base");
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
