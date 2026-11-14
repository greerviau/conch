use crate::{
    core::{Cleanup, Mapping, Snapshot, Vocabulary},
    index::Index,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub root: PathBuf,
    pub exclusions: Vec<String>,
}
/// A named set of the choices a recording uses: the recognition language and
/// model, Enhanced cleanup, and the repositories that supply context.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// ISO code from `languages::LANGUAGES`, or `auto`.
    pub language: String,
    /// Id of the recognition model, from `inference::RECOGNIZERS`.
    pub recognizer: String,
    /// Enhanced cleanup: `disabled`, `managed`, `anthropic`, `local`, or `remote`.
    pub enhanced: String,
    /// Ids of the repositories whose vocabulary and mappings a recording uses.
    pub repositories: Vec<String>,
    /// Whether a repository suggestion with one candidate is applied without review.
    pub auto_accept: bool,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            id: "default".into(),
            name: "Default".into(),
            language: "en".into(),
            recognizer: "small.en".into(),
            enhanced: "disabled".into(),
            repositories: vec![],
            auto_accept: false,
        }
    }
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
    /// Whether delivered results are saved to the history on disk.
    pub keep_history: bool,
    /// Name of the input device. Empty follows the system input.
    pub microphone: String,
    pub show_overlay: bool,
    pub check_updates: bool,
    /// Never empty once loaded or saved.
    pub profiles: Vec<Profile>,
    /// Id of the profile a recording uses.
    pub active_profile: String,
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
            keep_history: false,
            microphone: String::new(),
            show_overlay: true,
            check_updates: true,
            profiles: vec![Profile::default()],
            active_profile: Profile::default().id,
        }
    }
}
impl Settings {
    /// The profile a recording uses: the active one, or the first when the id is unknown.
    pub fn active(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.id == self.active_profile)
            .or(self.profiles.first())
            .expect("settings keep at least one profile")
    }
    pub fn active_mut(&mut self) -> &mut Profile {
        let index = self
            .profiles
            .iter()
            .position(|p| p.id == self.active_profile)
            .unwrap_or(0);
        &mut self.profiles[index]
    }
}
/// Settings saved before profiles held the language, model, Enhanced cleanup,
/// repository selection, and Auto-accept at the top level. They become one
/// profile named Default.
fn profile_before_profiles(json: &serde_json::Value) -> Profile {
    let text = |value: &serde_json::Value| value.as_str().map(str::to_owned);
    let defaults = Profile::default();
    Profile {
        language: text(&json["language"]).unwrap_or(defaults.language),
        // Only Tiny existed before the model choice, so a copy that saved settings
        // without one has Tiny installed and nothing else.
        recognizer: text(&json["recognizer"]).unwrap_or_else(|| "tiny.en".into()),
        enhanced: text(&json["cleanup"]["enhanced"]).unwrap_or(defaults.enhanced),
        repositories: json["repositories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["selected"].as_bool().unwrap_or(false))
            .filter_map(|r| text(&r["id"]))
            .collect(),
        auto_accept: json["auto_accept_suggestions"].as_bool().unwrap_or(false),
        ..defaults
    }
}
/// Keeps each profile's choices to values this build offers, its repositories to
/// configured ones, and at least one profile with a valid active id.
pub fn normalize(settings: &mut Settings) {
    let known: Vec<String> = settings.repositories.iter().map(|r| r.id.clone()).collect();
    for profile in &mut settings.profiles {
        // A model id this build does not offer would make every settings save fail.
        if crate::inference::recognizer(&profile.recognizer).is_none() {
            profile.recognizer = Profile::default().recognizer;
        }
        if !crate::languages::known(&profile.language) {
            profile.language = Profile::default().language;
        }
        // Keep the language and move to a model that can recognize it. If that
        // model is not installed, recording asks for the download.
        if crate::inference::recognizer(&profile.recognizer)
            .is_some_and(|model| !model.supports(&profile.language))
            && let Some(model) = crate::inference::smallest_supporting(&profile.language)
        {
            profile.recognizer = model.id.into();
        }
        if !ENHANCED.contains(&profile.enhanced.as_str()) {
            profile.enhanced = Profile::default().enhanced;
        }
        profile.repositories.retain(|id| known.contains(id));
        profile.repositories.dedup();
    }
    if settings.profiles.is_empty() {
        settings.profiles.push(Profile::default());
    }
    if !settings
        .profiles
        .iter()
        .any(|p| p.id == settings.active_profile)
    {
        settings.active_profile = settings.profiles[0].id.clone();
    }
}
/// The Enhanced cleanup choices a profile can hold.
pub const ENHANCED: [&str; 5] = ["disabled", "managed", "anthropic", "local", "remote"];
/// The `user_version` of the current schema. Version 1 keeps each index's
/// status in columns, so reading the status does not parse the entries.
const SCHEMA: i64 = 1;
pub fn open(dir: &Path) -> Result<rusqlite::Connection, String> {
    std::fs::create_dir_all(dir).map_err(|_| "Cannot create local data directory")?;
    let mut db = rusqlite::Connection::open(dir.join("conch.sqlite3"))
        .map_err(|_| "Cannot open local database")?;
    let version = |db: &rusqlite::Connection| -> Result<i64, String> {
        db.query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|_| "Cannot read database version".into())
    };
    if version(&db)? >= SCHEMA {
        return Ok(db);
    }
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| "Cannot initialize database")?;
    if version(&tx)? < SCHEMA {
        // Indexes are rebuilt from the repositories, so an older table is dropped.
        tx.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS history (id INTEGER PRIMARY KEY AUTOINCREMENT, created INTEGER NOT NULL, raw TEXT NOT NULL, text TEXT NOT NULL);
             DROP TABLE IF EXISTS indexes;
             CREATE TABLE indexes (repository TEXT PRIMARY KEY, value TEXT NOT NULL, refreshed INTEGER NOT NULL, available INTEGER NOT NULL, incomplete INTEGER NOT NULL, skipped INTEGER NOT NULL, entries INTEGER NOT NULL);
             PRAGMA user_version = {SCHEMA};"
        ))
        .map_err(|_| "Cannot initialize database")?;
    }
    tx.commit().map_err(|_| "Cannot initialize database")?;
    Ok(db)
}
pub fn load(dir: &Path) -> Result<Settings, String> {
    use rusqlite::OptionalExtension;
    let json: Option<String> = open(dir)?
        .query_row("SELECT value FROM settings WHERE id=1", [], |r| r.get(0))
        .optional()
        .map_err(|_| "Cannot read settings")?;
    let Some(json) = json else {
        return Ok(Settings::default());
    };
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|_| "Settings are invalid".to_string())?;
    let mut settings: Settings =
        serde_json::from_value(value.clone()).map_err(|_| "Settings are invalid".to_string())?;
    if value.get("profiles").is_none() {
        let profile = profile_before_profiles(&value);
        settings.active_profile = profile.id.clone();
        settings.profiles = vec![profile];
    }
    normalize(&mut settings);
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
            "INSERT OR REPLACE INTO indexes VALUES (?1,?2,unixepoch(),?3,?4,?5,?6)",
            rusqlite::params![
                repository,
                serde_json::to_string(index).map_err(|_| "Cannot encode index")?,
                index.available,
                index.incomplete,
                index.skipped as i64,
                index.entries.len() as i64
            ],
        )
        .map_err(|_| "Cannot save index")?;
    Ok(())
}
/// Each repository's vocabulary index by repository id.
pub type Indexes = HashMap<String, Arc<Index>>;
pub fn indexes(dir: &Path) -> Result<Indexes, String> {
    let db = open(dir)?;
    let mut statement = db
        .prepare("SELECT repository,value FROM indexes")
        .map_err(|_| "Cannot read indexes")?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|_| "Cannot read indexes")?;
    rows.map(|r| {
        let (id, json) = r.map_err(|_| "Cannot read index")?;
        let index = serde_json::from_str(&json).map_err(|_| "Invalid index")?;
        Ok((id, Arc::new(index)))
    })
    .collect()
}
/// The context snapshot for a recording that starts with `settings`, from the
/// vocabulary indexes held in memory.
pub fn snapshot(settings: &Settings, indexes: &Indexes) -> Snapshot {
    let profile = settings.active();
    let selected: Vec<_> = settings
        .repositories
        .iter()
        .filter(|r| profile.repositories.contains(&r.id) && r.root.is_dir())
        .map(|r| r.id.as_str())
        .collect();
    let vocabulary = selected
        .iter()
        .filter_map(|id| indexes.get(*id))
        .filter(|index| index.available)
        .flat_map(|index| index.entries.iter().cloned())
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
    Snapshot {
        mappings,
        vocabulary,
        cleanup: settings.cleanup.clone(),
        enhanced: profile.enhanced.clone(),
        auto_accept: profile.auto_accept,
        language: profile.language.clone(),
        recognizer: profile.recognizer.clone(),
    }
}
/// What the interface shows for a repository index.
#[derive(Clone, Debug, Serialize)]
pub struct IndexStatus {
    pub available: bool,
    pub incomplete: bool,
    pub skipped: usize,
    pub entries: usize,
}
pub fn index_status(dir: &Path) -> Result<Vec<(String, IndexStatus, i64)>, String> {
    let db = open(dir)?;
    let mut statement = db
        .prepare("SELECT repository,available,incomplete,skipped,entries,refreshed FROM indexes")
        .map_err(|_| "Cannot read indexes")?;
    statement
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                IndexStatus {
                    available: r.get(1)?,
                    incomplete: r.get(2)?,
                    skipped: r.get::<_, i64>(3)? as usize,
                    entries: r.get::<_, i64>(4)? as usize,
                },
                r.get(5)?,
            ))
        })
        .map_err(|_| "Cannot read indexes")?
        .collect::<Result<_, _>>()
        .map_err(|_| "Cannot read index".into())
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
    fn status_is_read_without_parsing_the_stored_entries() {
        let dir = tempfile::tempdir().unwrap();
        sample(dir.path());
        open(dir.path())
            .unwrap()
            .execute("UPDATE indexes SET value='not json'", [])
            .unwrap();
        let status = index_status(dir.path()).unwrap();
        assert_eq!(status[0].1.entries, 6);
        assert!(status[0].1.available && !status[0].1.incomplete);
    }
    #[test]
    fn an_index_table_without_status_columns_is_rebuilt_and_history_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        rusqlite::Connection::open(dir.path().join("conch.sqlite3"))
            .unwrap()
            .execute_batch(
                "CREATE TABLE indexes (repository TEXT PRIMARY KEY, value TEXT NOT NULL, refreshed INTEGER NOT NULL);
                 INSERT INTO indexes VALUES ('r', '{\"entries\":[],\"skipped\":0,\"incomplete\":false,\"available\":true}', 0);
                 CREATE TABLE history (id INTEGER PRIMARY KEY AUTOINCREMENT, created INTEGER NOT NULL, raw TEXT NOT NULL, text TEXT NOT NULL);
                 INSERT INTO history (created, raw, text) VALUES (1, 'raw', 'text');",
            )
            .unwrap();
        assert!(index_status(dir.path()).unwrap().is_empty());
        assert_eq!(crate::history::revision(dir.path()).unwrap(), 1);
        sample(dir.path());
        assert_eq!(index_status(dir.path()).unwrap()[0].1.entries, 6);
    }
    fn with_profile(profile: Profile) -> Settings {
        Settings {
            active_profile: profile.id.clone(),
            profiles: vec![profile],
            ..Default::default()
        }
    }
    fn stored(dir: &Path, json: &str) {
        open(dir)
            .unwrap()
            .execute("INSERT OR REPLACE INTO settings VALUES (1, ?1)", [json])
            .unwrap();
    }
    #[test]
    fn the_snapshot_takes_vocabulary_from_the_active_profiles_available_indexes_in_memory() {
        let root = tempfile::tempdir().unwrap();
        let repository = |id: &str| Repository {
            id: id.into(),
            root: root.path().into(),
            exclusions: vec![],
        };
        let mut settings = with_profile(Profile {
            repositories: vec!["a".into(), "c".into()],
            ..Default::default()
        });
        settings.repositories = vec![repository("a"), repository("b"), repository("c")];
        let index = |text: &str, available: bool| {
            Arc::new(Index {
                entries: vec![term(text, "x.ts")],
                available,
                ..Default::default()
            })
        };
        let indexes = Indexes::from([
            ("a".to_string(), index("Alpha", true)),
            ("b".to_string(), index("Beta", true)),
            ("c".to_string(), index("Gamma", false)),
        ]);
        let texts: Vec<_> = snapshot(&settings, &indexes)
            .vocabulary
            .into_iter()
            .map(|v| v.text)
            .collect();
        assert_eq!(texts, ["Alpha"]);
    }
    #[test]
    fn the_snapshot_uses_the_active_profile_and_ignores_the_others() {
        let root = tempfile::tempdir().unwrap();
        let mut settings = Settings {
            repositories: ["code", "infra"]
                .map(|id| Repository {
                    id: id.into(),
                    root: root.path().into(),
                    exclusions: vec![],
                })
                .to_vec(),
            mappings: ["code", "infra"]
                .map(|id| Mapping {
                    phrase: id.into(),
                    replacement: id.to_uppercase(),
                    repository: Some(id.into()),
                    enabled: true,
                })
                .to_vec(),
            profiles: vec![
                Profile {
                    id: "writing".into(),
                    name: "Writing".into(),
                    enhanced: "managed".into(),
                    ..Default::default()
                },
                Profile {
                    id: "code".into(),
                    name: "Code".into(),
                    recognizer: "tiny.en".into(),
                    repositories: vec!["code".into()],
                    auto_accept: true,
                    ..Default::default()
                },
            ],
            active_profile: "code".into(),
            ..Default::default()
        };
        let indexes = Indexes::from(["code", "infra"].map(|id| {
            (
                id.to_string(),
                Arc::new(Index {
                    entries: vec![term(&format!("{id}Term"), "x.ts")],
                    available: true,
                    ..Default::default()
                }),
            )
        }));
        let code = snapshot(&settings, &indexes);
        assert_eq!(code.recognizer, "tiny.en");
        assert_eq!(code.enhanced, "disabled");
        assert!(code.auto_accept);
        assert_eq!(code.vocabulary[0].text, "codeTerm");
        assert_eq!(code.vocabulary.len(), 1);
        assert_eq!(code.mappings.len(), 1);
        assert_eq!(code.mappings[0].replacement, "CODE");
        settings.active_profile = "writing".into();
        let writing = snapshot(&settings, &indexes);
        assert_eq!(writing.recognizer, "small.en");
        assert_eq!(writing.enhanced, "managed");
        assert!(!writing.auto_accept);
        assert!(writing.vocabulary.is_empty() && writing.mappings.is_empty());
    }
    #[test]
    fn a_new_install_has_one_default_profile_with_small_english_and_no_repositories() {
        let dir = tempfile::tempdir().unwrap();
        let settings = load(dir.path()).unwrap();
        assert_eq!(settings.profiles.len(), 1);
        let profile = settings.active();
        assert_eq!(profile.name, "Default");
        assert_eq!(
            (profile.language.as_str(), profile.recognizer.as_str()),
            ("en", "small.en")
        );
        assert_eq!(profile.enhanced, "disabled");
        assert!(profile.repositories.is_empty() && !profile.auto_accept);
    }
    #[test]
    fn settings_saved_before_profiles_become_one_default_profile() {
        let dir = tempfile::tempdir().unwrap();
        stored(
            dir.path(),
            r#"{"repositories":[{"id":"a","root":"/a","selected":true,"exclusions":[]},{"id":"b","root":"/b","selected":false,"exclusions":[]}],"cleanup":{"enhanced":"local","endpoint":"http://127.0.0.1:1/v1"},"auto_accept_suggestions":true,"recognizer":"base","language":"fr","hotkey":"Control+Space"}"#,
        );
        let settings = load(dir.path()).unwrap();
        assert_eq!(settings.profiles.len(), 1);
        assert_eq!(settings.active_profile, settings.profiles[0].id);
        assert_eq!(
            settings.profiles[0],
            Profile {
                language: "fr".into(),
                recognizer: "base".into(),
                enhanced: "local".into(),
                repositories: vec!["a".into()],
                auto_accept: true,
                ..Default::default()
            }
        );
        assert_eq!(settings.cleanup.endpoint, "http://127.0.0.1:1/v1");
        assert_eq!(settings.hotkey, "Control+Space");
        assert_eq!(settings.repositories.len(), 2);
    }
    #[test]
    fn settings_saved_before_the_model_choice_keep_tiny_english() {
        // Only Tiny existed before the choice, so a copy that saved settings then has it installed.
        let dir = tempfile::tempdir().unwrap();
        stored(dir.path(), r#"{"hotkey":"Control+Space"}"#);
        let settings = load(dir.path()).unwrap();
        assert_eq!(settings.active().recognizer, "tiny.en");
        assert_eq!(settings.active().language, "en");
    }
    #[test]
    fn loading_keeps_one_profile_a_valid_active_id_and_only_configured_repositories() {
        let dir = tempfile::tempdir().unwrap();
        stored(
            dir.path(),
            r#"{"profiles":[],"active_profile":"gone","repositories":[]}"#,
        );
        let settings = load(dir.path()).unwrap();
        assert_eq!(settings.profiles, vec![Profile::default()]);
        assert_eq!(settings.active_profile, "default");
        stored(
            dir.path(),
            r#"{"repositories":[{"id":"a","root":"/a","exclusions":[]}],"profiles":[{"id":"p","name":"P","repositories":["a","gone"],"enhanced":"basic"},{"id":"q","name":"Q"}],"active_profile":"missing"}"#,
        );
        let settings = load(dir.path()).unwrap();
        assert_eq!(settings.active_profile, "p");
        assert_eq!(settings.profiles[0].repositories, ["a"]);
        assert_eq!(settings.profiles[0].enhanced, "disabled");
        assert_eq!(settings.profiles[1].recognizer, "small.en");
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
    fn keeping_history_is_opt_in() {
        assert!(!Settings::default().keep_history);
        let older: Settings = serde_json::from_str(r#"{"hotkey":"Control+Space"}"#).unwrap();
        assert!(!older.keep_history);
        let on: Settings = serde_json::from_str(r#"{"keep_history":true}"#).unwrap();
        assert!(on.keep_history);
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
        let mut settings = with_profile(Profile {
            recognizer: "huge".into(),
            ..Default::default()
        });
        save(dir.path(), &settings).unwrap();
        assert_eq!(load(dir.path()).unwrap().active().recognizer, "small.en");
        settings.active_mut().recognizer = "tiny.en".into();
        save(dir.path(), &settings).unwrap();
        assert_eq!(load(dir.path()).unwrap().active().recognizer, "tiny.en");
    }
    #[test]
    fn an_unknown_stored_language_loads_as_english_and_known_ones_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = with_profile(Profile {
            language: "xx".into(),
            ..Default::default()
        });
        save(dir.path(), &settings).unwrap();
        assert_eq!(load(dir.path()).unwrap().active().language, "en");
        for (code, recognizer) in [
            ("fr", "small"),
            ("auto", "small"),
            ("yue", "large-v3-turbo"),
        ] {
            settings.active_mut().language = code.into();
            settings.active_mut().recognizer = recognizer.into();
            save(dir.path(), &settings).unwrap();
            let loaded = load(dir.path()).unwrap();
            let profile = loaded.active();
            assert_eq!(
                (profile.language.as_str(), profile.recognizer.as_str()),
                (code, recognizer)
            );
        }
    }
    #[test]
    fn a_saved_model_that_lacks_the_saved_language_is_replaced_by_the_smallest_that_has_it() {
        let dir = tempfile::tempdir().unwrap();
        let saved = |language: &str, recognizer: &str| {
            save(
                dir.path(),
                &with_profile(Profile {
                    language: language.into(),
                    recognizer: recognizer.into(),
                    ..Default::default()
                }),
            )
            .unwrap();
            load(dir.path()).unwrap().active().clone()
        };
        for (language, recognizer) in [("fr", "small.en"), ("auto", "base.en")] {
            let loaded = saved(language, recognizer);
            assert_eq!(loaded.language, language);
            assert_eq!(loaded.recognizer, "tiny");
        }
        for recognizer in ["small.en", "small"] {
            assert_eq!(saved("yue", recognizer).recognizer, "large-v3-turbo");
        }
        assert_eq!(saved("de", "large-v3-turbo").recognizer, "large-v3-turbo");
    }
    #[test]
    fn a_hesitation_list_saved_before_languages_loads_as_the_english_list() {
        let dir = tempfile::tempdir().unwrap();
        stored(
            dir.path(),
            r#"{"cleanup":{"fillers":["um","er"],"enhanced":"local"}}"#,
        );
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.cleanup.fillers["en"], ["um", "er"]);
        assert_eq!(loaded.cleanup.fillers.len(), 1);
        assert_eq!(loaded.active().enhanced, "local");
        assert_eq!(loaded.active().language, "en");
        save(dir.path(), &loaded).unwrap();
        assert_eq!(
            load(dir.path()).unwrap().cleanup.fillers["en"],
            ["um", "er"]
        );
        let snapshot = snapshot(&loaded, &Indexes::new());
        assert_eq!(snapshot.cleanup.fillers["en"], ["um", "er"]);
    }
    #[test]
    fn the_snapshot_captures_the_language_and_the_model_of_the_active_profile() {
        let settings = with_profile(Profile {
            language: "pt".into(),
            recognizer: "base".into(),
            ..Default::default()
        });
        let snapshot = snapshot(&settings, &Indexes::new());
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
