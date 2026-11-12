use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix time in milliseconds.
    pub time: i64,
    pub level: Level,
    pub source: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Page {
    pub items: Vec<Entry>,
    /// Entries that match the filters.
    pub total: usize,
    /// Entries in the log before filtering.
    pub all: usize,
    /// Every source that has an entry, sorted.
    pub sources: Vec<String>,
    /// Size of the log files on disk.
    pub bytes: u64,
}

/// A quoted absolute path. Quotes delimit it, so it may contain spaces. Group 2
/// is the file name.
static QUOTED_PATH: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"(['"])(?:/|[A-Za-z]:[\\/]|\\\\)[^'"]*[\\/]([^'"\\/]*)['"]"#).unwrap()
});
/// A `file://` URL. Group 1 is the file name.
static FILE_URL: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"file://[^\s'"]*/([^\s'"/]*)"#).unwrap());
/// An absolute Unix path with at least one directory, after the start or any
/// character that cannot continue a word or path. That keeps `http://host/a`,
/// `~/a/b`, and a lone `/inference` out. Group 2 is the file name.
static UNIX_PATH: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"(^|[^\w.~/+@%-])/(?:[\w.@+-]+/)+([\w.@+-]*)"#).unwrap());
/// An absolute Windows path with at least one directory; a directory name may
/// contain spaces. Group 1 is the file name.
static WINDOWS_PATH: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"[A-Za-z]:\\(?:[^\\'"<>|]+\\)+([^\\\s'"<>|]*)"#).unwrap());
/// A UNC path. Group 1 is the file name.
static UNC_PATH: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r#"\\\\[^\\\s'"]+\\(?:[^\\'"<>|]+\\)*([^\\\s'"<>|]*)"#).unwrap()
});

/// Size at which `conch.log` rolls over to `conch.log.1`, which replaces the
/// previous `conch.log.1`. The two files hold about 4 MB.
const LIMIT: u64 = 2 * 1024 * 1024;
const FILE: &str = "conch.log";
const ROLLED: &str = "conch.log.1";
const MESSAGE_MAX: usize = 2_000;
const DETAIL_MAX: usize = 8_000;

/// The diagnostic log: one JSON object per line in the data directory. It
/// never holds audio, transcripts, result text, or source text. Callers pass
/// only what Conch did and why, and `Log` writes the home directory as `~`.
pub struct Log {
    dir: PathBuf,
    limit: u64,
    home: Option<String>,
    writer: Mutex<()>,
    revision: AtomicU64,
}

impl Log {
    pub fn open(dir: &Path) -> Log {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok();
        Log::with(dir, LIMIT, home.as_deref())
    }
    fn with(dir: &Path, limit: u64, home: Option<&str>) -> Log {
        Log {
            dir: dir.to_owned(),
            limit,
            home: home.filter(|home| home.len() > 1).map(str::to_owned),
            writer: Mutex::new(()),
            revision: AtomicU64::new(0),
        }
    }
    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }
    fn rolled(&self) -> PathBuf {
        self.dir.join(ROLLED)
    }
    /// A number that changes whenever the log changes, so the interface knows
    /// when to reload the page.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
    /// Writes the home directory as `~` and reduces every other absolute path
    /// to `.../file name`, so a log shared for a bug report names no private
    /// directory.
    fn scrub(&self, text: &str) -> String {
        let text = match &self.home {
            Some(home) => {
                let text = text
                    .replace(&format!("{home}/"), "~/")
                    .replace(&format!("{home}\\"), "~\\");
                match text.strip_suffix(home.as_str()) {
                    Some(rest) => format!("{rest}~"),
                    None => text,
                }
            }
            None => text.to_owned(),
        };
        let text = QUOTED_PATH.replace_all(&text, "${1}.../${2}${1}");
        let text = FILE_URL.replace_all(&text, "file:///.../${1}");
        let text = UNIX_PATH.replace_all(&text, "${1}.../${2}");
        let text = WINDOWS_PATH.replace_all(&text, ".../${1}");
        UNC_PATH.replace_all(&text, ".../${1}").into_owned()
    }
    /// Appends an entry. A failure to write is reported on stderr and never
    /// reaches the caller, because logging must not break dictation.
    pub fn record(&self, level: Level, source: &str, message: &str, detail: Option<&str>) {
        let entry = Entry {
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis() as i64),
            level,
            source: source.to_owned(),
            message: cut(&self.scrub(message), MESSAGE_MAX),
            detail: detail.map(|detail| cut(&self.scrub(detail), DETAIL_MAX)),
        };
        let Ok(mut line) = serde_json::to_string(&entry) else {
            return;
        };
        line.push('\n');
        let _guard = self.writer.lock().unwrap();
        if let Err(error) = self.append(line.as_bytes()) {
            eprintln!("Cannot write the log: {error}");
        }
        self.revision.fetch_add(1, Ordering::Relaxed);
    }
    fn append(&self, line: &[u8]) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path();
        let size = std::fs::metadata(&path).map_or(0, |meta| meta.len());
        if size > 0 && size + line.len() as u64 > self.limit {
            // A rename onto an existing file fails on Windows.
            let _ = std::fs::remove_file(self.rolled());
            std::fs::rename(&path, self.rolled())?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(line)
    }
    /// Every readable entry, oldest first. A line damaged by a crash is skipped.
    fn read(&self) -> Vec<Entry> {
        [self.rolled(), self.path()]
            .iter()
            .filter_map(|path| std::fs::read_to_string(path).ok())
            .flat_map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
    /// One page of entries, newest first. `level` is `all`, `warn` (warnings
    /// and errors), or `error`; `source` is `all` or a source name; `query`
    /// matches the message, detail, or source without regard to case.
    pub fn page(
        &self,
        level: &str,
        source: &str,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Page, String> {
        // Hold the writer lock so a rollover cannot land between the two files.
        let (mut entries, bytes) = {
            let _guard = self.writer.lock().unwrap();
            let bytes: u64 = [self.rolled(), self.path()]
                .iter()
                .filter_map(|path| std::fs::metadata(path).ok())
                .map(|meta| meta.len())
                .sum();
            (self.read(), bytes)
        };
        entries.reverse();
        let all = entries.len();
        let mut sources: Vec<String> = entries.iter().map(|e| e.source.clone()).collect();
        sources.sort();
        sources.dedup();
        let query = query.trim().to_lowercase();
        let matching: Vec<Entry> = entries
            .into_iter()
            .filter(|e| match level {
                "warn" => e.level != Level::Info,
                "error" => e.level == Level::Error,
                _ => true,
            })
            .filter(|e| source == "all" || e.source == source)
            .filter(|e| {
                query.is_empty()
                    || e.message.to_lowercase().contains(&query)
                    || e.source.to_lowercase().contains(&query)
                    || e.detail
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&query))
            })
            .collect();
        let total = matching.len();
        Ok(Page {
            items: matching.into_iter().skip(offset).take(limit).collect(),
            total,
            all,
            sources,
            bytes,
        })
    }
    pub fn clear(&self) -> Result<(), String> {
        let _guard = self.writer.lock().unwrap();
        for path in [self.path(), self.rolled()] {
            match std::fs::remove_file(path) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(_) => return Err("Cannot clear the logs".into()),
            }
        }
        self.revision.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// Keeps the first `max` characters, ending with `...` when text was cut.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(max - 3).collect();
    kept.push_str("...");
    kept
}

/// Shows `conch.log` in the system file manager, or the data directory when
/// nothing was logged yet.
pub fn reveal(log: &Log) -> Result<(), String> {
    let file = log.path();
    let select = file.is_file();
    let target = if select { file } else { log.dir.clone() };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        if select {
            command.arg("-R");
        }
        command.arg(&target);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("explorer");
        if select {
            command.arg(format!("/select,{}", target.display()));
        } else {
            command.arg(&target);
        }
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(if select {
            log.dir.as_path()
        } else {
            target.as_path()
        });
        command
    };
    command
        .spawn()
        .map(drop)
        .map_err(|_| "Cannot open the file manager".into())
}

static GLOBAL: OnceLock<Log> = OnceLock::new();

/// Starts the process-wide log in `dir`. Until it runs, the functions below
/// record nothing, which keeps unit tests of other modules free of files.
pub fn init(dir: &Path) -> &'static Log {
    GLOBAL.get_or_init(|| Log::open(dir))
}
pub fn get() -> Option<&'static Log> {
    GLOBAL.get()
}
pub fn record(level: Level, source: &str, message: &str, detail: Option<&str>) {
    if let Some(log) = get() {
        log.record(level, source, message, detail);
    }
}
pub fn info(source: &str, message: &str) {
    record(Level::Info, source, message, None);
}
pub fn warn(source: &str, message: &str) {
    record(Level::Warn, source, message, None);
}
pub fn error(source: &str, message: &str) {
    record(Level::Error, source, message, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(limit: u64) -> (tempfile::TempDir, Log) {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::with(dir.path(), limit, Some("/Users/alex"));
        (dir, log)
    }
    fn messages(page: &Page) -> Vec<&str> {
        page.items.iter().map(|e| e.message.as_str()).collect()
    }
    fn list(log: &Log) -> Page {
        log.page("all", "all", "", 0, 100).unwrap()
    }

    #[test]
    fn entries_are_listed_newest_first_with_their_level_source_and_detail() {
        let (_dir, log) = log(1 << 20);
        log.record(Level::Info, "app", "started", None);
        log.record(Level::Error, "cleanup", "failed", Some("attempt 1 of 1"));
        let page = list(&log);
        assert_eq!(messages(&page), ["failed", "started"]);
        assert_eq!(page.items[0].level, Level::Error);
        assert_eq!(page.items[0].source, "cleanup");
        assert_eq!(page.items[0].detail.as_deref(), Some("attempt 1 of 1"));
        assert_eq!(page.items[1].detail, None);
        assert!(page.items[0].time >= page.items[1].time && page.items[1].time > 1_700_000_000_000);
        assert_eq!((page.total, page.all), (2, 2));
    }

    #[test]
    fn an_empty_log_lists_nothing() {
        let (_dir, log) = log(1 << 20);
        let page = list(&log);
        assert!(page.items.is_empty());
        assert_eq!((page.total, page.all, page.bytes), (0, 0, 0));
    }

    #[test]
    fn the_home_directory_is_written_as_a_tilde() {
        let (_dir, log) = log(1 << 20);
        log.record(
            Level::Warn,
            "recognizer",
            "loading /Users/alex/Library/model.bin",
            Some("also /Users/alex/Projects/app"),
        );
        let entry = &list(&log).items[0];
        assert_eq!(entry.message, "loading ~/Library/model.bin");
        assert_eq!(entry.detail.as_deref(), Some("also ~/Projects/app"));
        let file = std::fs::read_to_string(log.path()).unwrap();
        assert!(!file.contains("/Users/alex"));
    }

    #[test]
    fn long_text_is_cut_so_one_entry_cannot_fill_the_file() {
        let (_dir, log) = log(1 << 20);
        log.record(
            Level::Info,
            "app",
            &"m".repeat(5_000),
            Some(&"d".repeat(20_000)),
        );
        let entry = &list(&log).items[0];
        assert!(entry.message.chars().count() <= 2_000);
        assert!(entry.detail.as_ref().unwrap().chars().count() <= 8_000);
        assert!(entry.message.ends_with("..."));
    }

    #[test]
    fn the_level_filter_keeps_warnings_with_errors_or_only_errors() {
        let (_dir, log) = log(1 << 20);
        log.record(Level::Info, "app", "info", None);
        log.record(Level::Warn, "app", "warn", None);
        log.record(Level::Error, "app", "error", None);
        let at = |level: &str| log.page(level, "all", "", 0, 100).unwrap();
        assert_eq!(messages(&at("warn")), ["error", "warn"]);
        assert_eq!(messages(&at("error")), ["error"]);
        assert_eq!(at("error").all, 3);
    }

    #[test]
    fn the_source_filter_and_search_narrow_the_list() {
        let (_dir, log) = log(1 << 20);
        log.record(Level::Info, "update", "Checked for updates", None);
        log.record(Level::Info, "index", "Indexed conch", Some("38 SKIPPED"));
        log.record(Level::Info, "index", "Indexed other", None);
        let find = |source: &str, query: &str| {
            let page = log.page("all", source, query, 0, 100).unwrap();
            (messages(&page).join(","), page.total)
        };
        assert_eq!(find("index", ""), ("Indexed other,Indexed conch".into(), 2));
        assert_eq!(find("all", "CHECKED"), ("Checked for updates".into(), 1));
        assert_eq!(find("all", "skipped"), ("Indexed conch".into(), 1));
        assert_eq!(find("index", "conch"), ("Indexed conch".into(), 1));
        assert_eq!(find("all", "nothing"), ("".into(), 0));
    }

    #[test]
    fn the_sources_cover_every_entry_not_only_the_filtered_ones() {
        let (_dir, log) = log(1 << 20);
        log.record(Level::Info, "update", "a", None);
        log.record(Level::Info, "index", "b", None);
        log.record(Level::Info, "index", "c", None);
        let page = log.page("all", "index", "", 0, 100).unwrap();
        assert_eq!(page.sources, ["index", "update"]);
    }

    #[test]
    fn pages_slice_the_newest_first_list_and_report_the_total() {
        let (_dir, log) = log(1 << 20);
        for n in 1..=5 {
            log.record(Level::Info, "app", &format!("entry {n}"), None);
        }
        let page = log.page("all", "all", "", 2, 2).unwrap();
        assert_eq!(messages(&page), ["entry 3", "entry 2"]);
        assert_eq!(page.total, 5);
    }

    #[test]
    fn the_file_rolls_over_at_the_limit_and_drops_the_oldest_file() {
        let (dir, log) = log(600);
        for n in 1..=40 {
            log.record(Level::Info, "app", &format!("entry {n:02}"), None);
        }
        assert!(dir.path().join("conch.log.1").is_file());
        let page = list(&log);
        assert_eq!(page.items[0].message, "entry 40");
        assert!(page.total < 40, "the oldest entries are gone");
        let expected: Vec<String> = (41 - page.total..=40)
            .rev()
            .map(|n| format!("entry {n:02}"))
            .collect();
        assert_eq!(messages(&page), expected, "the newest run is unbroken");
        assert!(page.bytes <= 2 * 600 + 200, "two files of about the limit");
        assert!(!dir.path().join("conch.log.2").exists());
    }

    #[test]
    fn a_damaged_line_is_skipped() {
        let (_dir, log) = log(1 << 20);
        log.record(Level::Info, "app", "before", None);
        std::fs::OpenOptions::new()
            .append(true)
            .open(log.path())
            .unwrap()
            .write_all(b"{\"time\":1,\"level\"\n")
            .unwrap();
        log.record(Level::Info, "app", "after", None);
        assert_eq!(messages(&list(&log)), ["after", "before"]);
    }

    #[test]
    fn clearing_removes_both_files_and_every_entry() {
        let (dir, log) = log(600);
        for n in 1..=40 {
            log.record(Level::Info, "app", &format!("entry {n}"), None);
        }
        log.clear().unwrap();
        assert!(!dir.path().join("conch.log").exists());
        assert!(!dir.path().join("conch.log.1").exists());
        assert_eq!(list(&log).all, 0);
        log.record(Level::Info, "app", "again", None);
        assert_eq!(messages(&list(&log)), ["again"]);
    }

    #[test]
    fn the_revision_changes_when_an_entry_is_added_or_the_log_is_cleared() {
        let (_dir, log) = log(1 << 20);
        let start = log.revision();
        log.record(Level::Info, "app", "a", None);
        let added = log.revision();
        assert_ne!(added, start);
        log.clear().unwrap();
        assert_ne!(log.revision(), added);
    }
    fn scrubbed(text: &str) -> String {
        let (_dir, log) = log(1 << 20);
        log.record(Level::Info, "app", text, None);
        list(&log).items[0].message.clone()
    }

    #[test]
    fn a_path_outside_the_home_directory_is_reduced_to_its_file_name() {
        for (text, want) in [
            (
                "loading '/mnt/data/models/small.en.bin' failed",
                "loading '.../small.en.bin' failed",
            ),
            (
                "see /opt/app/bin/tool, then (/var/log/conch/a.log)",
                "see .../tool, then (.../a.log)",
            ),
            ("error:/mnt/private/model.bin", "error:.../model.bin"),
            (
                "opening /Users/alex/Projects/app/main.rs",
                "opening ~/Projects/app/main.rs",
            ),
        ] {
            assert_eq!(scrubbed(text), want);
        }
    }

    #[test]
    fn a_quoted_path_is_reduced_even_when_it_contains_spaces() {
        assert_eq!(
            scrubbed("loading model from '/Private Data/client/model.bin'"),
            "loading model from '.../model.bin'"
        );
        assert_eq!(
            scrubbed(r#"cannot open "C:\Private Data\client\model.bin" now"#),
            r#"cannot open ".../model.bin" now"#
        );
    }

    #[test]
    fn windows_unc_and_file_url_paths_are_reduced() {
        for (text, want) in [
            (r"C:\Users\bob\Data\x.bin failed", ".../x.bin failed"),
            (
                r"C:\Private Data\client\model.bin failed",
                ".../model.bin failed",
            ),
            (
                r"\\server\share\private\model.bin failed",
                ".../model.bin failed",
            ),
            (
                "file:///mnt/private/model.bin failed",
                "file:///.../model.bin failed",
            ),
        ] {
            assert_eq!(scrubbed(text), want);
        }
    }

    #[test]
    fn a_url_host_and_a_single_slash_word_are_left_alone() {
        assert_eq!(
            scrubbed("GET http://127.0.0.1:8765/inference returned 500 for /tmp"),
            "GET http://127.0.0.1:8765/inference returned 500 for /tmp"
        );
        assert_eq!(scrubbed("path ~/a/b stays"), "path ~/a/b stays");
    }

    #[test]
    fn a_page_read_during_a_rollover_never_misses_the_rolled_file() {
        let (_dir, log) = log(400);
        let log = std::sync::Arc::new(log);
        let writer = {
            let log = log.clone();
            std::thread::spawn(move || {
                for n in 0..4000 {
                    log.record(Level::Info, "app", &format!("{n}"), None);
                }
            })
        };
        let mut reads = 0;
        while !writer.is_finished() {
            let page = log.page("all", "all", "", 0, 1000).unwrap();
            let seen: Vec<i64> = page
                .items
                .iter()
                .map(|e| e.message.parse().unwrap())
                .collect();
            for pair in seen.windows(2) {
                assert_eq!(pair[0], pair[1] + 1, "a gap in {seen:?}");
            }
            reads += 1;
        }
        writer.join().unwrap();
        assert!(reads > 0);
    }
}
