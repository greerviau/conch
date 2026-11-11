use crate::{core::ResultText, storage};
use serde::Serialize;
use std::path::Path;

/// Entries kept. Adding one past this limit deletes the oldest.
pub const LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub id: i64,
    /// Unix time the result was delivered.
    pub created: i64,
    pub text: String,
    pub raw: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Page {
    pub items: Vec<Entry>,
    pub total: usize,
}

/// Saves a delivered result when Keep history is on and the result has text.
pub fn record(dir: &Path, enabled: bool, result: &ResultText) -> Result<Option<i64>, String> {
    if !enabled || result.text.trim().is_empty() {
        return Ok(None);
    }
    add_within(dir, &result.raw, &result.text, LIMIT).map(Some)
}

fn add_within(dir: &Path, raw: &str, text: &str, limit: usize) -> Result<i64, String> {
    let mut db = storage::open(dir)?;
    let tx = db.transaction().map_err(|_| "Cannot save history")?;
    tx.execute(
        "INSERT INTO history (created, raw, text) VALUES (unixepoch(), ?1, ?2)",
        rusqlite::params![raw, text],
    )
    .map_err(|_| "Cannot save history")?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT ?1)",
        [limit as i64],
    )
    .map_err(|_| "Cannot trim history")?;
    tx.commit().map_err(|_| "Cannot save history")?;
    Ok(id)
}

/// Replaces the text of an entry after the result was edited.
pub fn set_text(dir: &Path, id: i64, text: &str) -> Result<(), String> {
    storage::open(dir)?
        .execute(
            "UPDATE history SET text=?2 WHERE id=?1",
            rusqlite::params![id, text],
        )
        .map_err(|_| "Cannot update history")?;
    Ok(())
}

fn entry(row: &rusqlite::Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        created: row.get(1)?,
        text: row.get(2)?,
        raw: row.get(3)?,
    })
}

/// One page of entries, newest first, whose text or raw transcript contains
/// `query`. `total` counts every match, not only the page. SQLite folds the
/// case of ASCII letters only, which covers the English recognition models.
pub fn page(dir: &Path, query: &str, offset: usize, limit: usize) -> Result<Page, String> {
    let db = storage::open(dir)?;
    let query = query.trim().to_lowercase();
    let matching = "instr(lower(text), ?1) > 0 OR instr(lower(raw), ?1) > 0";
    let total: i64 = db
        .query_row(
            &format!("SELECT COUNT(*) FROM history WHERE {matching}"),
            [&query],
            |r| r.get(0),
        )
        .map_err(|_| "Cannot read history")?;
    let mut statement = db
        .prepare(&format!(
            "SELECT id, created, text, raw FROM history WHERE {matching} \
             ORDER BY id DESC LIMIT ?2 OFFSET ?3"
        ))
        .map_err(|_| "Cannot read history")?;
    let items = statement
        .query_map(rusqlite::params![query, limit as i64, offset as i64], entry)
        .map_err(|_| "Cannot read history")?
        .collect::<Result<_, _>>()
        .map_err(|_| "Cannot read history")?;
    Ok(Page {
        items,
        total: total as usize,
    })
}

pub fn get(dir: &Path, id: i64) -> Result<Entry, String> {
    storage::open(dir)?
        .query_row(
            "SELECT id, created, text, raw FROM history WHERE id=?1",
            [id],
            entry,
        )
        .map_err(|_| "History entry is unavailable".into())
}

pub fn delete(dir: &Path, id: i64) -> Result<(), String> {
    storage::open(dir)?
        .execute("DELETE FROM history WHERE id=?1", [id])
        .map_err(|_| "Cannot delete history entry")?;
    Ok(())
}

pub fn clear(dir: &Path) -> Result<(), String> {
    storage::open(dir)?
        .execute("DELETE FROM history", [])
        .map_err(|_| "Cannot clear history")?;
    Ok(())
}

/// The id of the newest entry, or 0 when there is none. It changes whenever a
/// result is added, so the interface knows when to reload the list.
pub fn revision(dir: &Path) -> Result<i64, String> {
    storage::open(dir)?
        .query_row("SELECT COALESCE(MAX(id), 0) FROM history", [], |r| r.get(0))
        .map_err(|_| "Cannot read history".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(raw: &str, text: &str) -> ResultText {
        ResultText {
            raw: raw.into(),
            text: text.into(),
            ..Default::default()
        }
    }
    fn texts(page: &Page) -> Vec<&str> {
        page.items.iter().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn a_recorded_result_is_listed_with_its_raw_transcript_and_time() {
        let dir = tempfile::tempdir().unwrap();
        let id = record(
            dir.path(),
            true,
            &result("um open main dot rs", "open main.rs"),
        )
        .unwrap()
        .unwrap();
        let page = page(dir.path(), "", 0, 10).unwrap();
        assert_eq!(page.total, 1);
        let entry = &page.items[0];
        assert_eq!(entry.id, id);
        assert_eq!(entry.text, "open main.rs");
        assert_eq!(entry.raw, "um open main dot rs");
        assert!(entry.created > 1_700_000_000);
    }

    #[test]
    fn nothing_is_saved_when_keep_history_is_off_or_the_result_is_blank() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(record(dir.path(), false, &result("a", "a")).unwrap(), None);
        assert_eq!(record(dir.path(), true, &result("um", "  ")).unwrap(), None);
        assert_eq!(page(dir.path(), "", 0, 10).unwrap().total, 0);
        assert_eq!(revision(dir.path()).unwrap(), 0);
    }

    #[test]
    fn entries_come_newest_first_in_pages_with_the_total() {
        let dir = tempfile::tempdir().unwrap();
        for n in 1..=5 {
            record(dir.path(), true, &result("", &format!("entry {n}"))).unwrap();
        }
        let first = page(dir.path(), "", 0, 2).unwrap();
        assert_eq!(
            (first.total, texts(&first)),
            (5, vec!["entry 5", "entry 4"])
        );
        let last = page(dir.path(), "", 4, 2).unwrap();
        assert_eq!(texts(&last), ["entry 1"]);
        assert_eq!(page(dir.path(), "", 9, 2).unwrap().items.len(), 0);
    }

    #[test]
    fn search_ignores_case_matches_the_raw_transcript_and_counts_only_matches() {
        let dir = tempfile::tempdir().unwrap();
        record(
            dir.path(),
            true,
            &result("fetch user profile", "fetchUserProfile"),
        )
        .unwrap();
        record(dir.path(), true, &result("hello", "Hello, 100% sure")).unwrap();
        record(dir.path(), true, &result("other", "other")).unwrap();
        let by_text = page(dir.path(), "USERPROFILE", 0, 10).unwrap();
        assert_eq!(texts(&by_text), ["fetchUserProfile"]);
        let by_raw = page(dir.path(), "user profile", 0, 10).unwrap();
        assert_eq!(by_raw.total, 1);
        // LIKE wildcards in the query are plain characters.
        assert_eq!(
            texts(&page(dir.path(), "100%", 0, 10).unwrap()),
            ["Hello, 100% sure"]
        );
        assert_eq!(page(dir.path(), "_", 0, 10).unwrap().total, 0);
        assert_eq!(page(dir.path(), "  ", 0, 10).unwrap().total, 3);
    }

    #[test]
    fn editing_the_result_updates_the_text_and_keeps_the_raw_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let id = record(dir.path(), true, &result("raw words", "first"))
            .unwrap()
            .unwrap();
        set_text(dir.path(), id, "second").unwrap();
        let entry = get(dir.path(), id).unwrap();
        assert_eq!(
            (entry.text.as_str(), entry.raw.as_str()),
            ("second", "raw words")
        );
    }

    #[test]
    fn one_entry_or_every_entry_can_be_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let a = record(dir.path(), true, &result("", "a")).unwrap().unwrap();
        record(dir.path(), true, &result("", "b")).unwrap();
        delete(dir.path(), a).unwrap();
        assert_eq!(texts(&page(dir.path(), "", 0, 10).unwrap()), ["b"]);
        assert!(get(dir.path(), a).is_err());
        clear(dir.path()).unwrap();
        assert_eq!(page(dir.path(), "", 0, 10).unwrap().total, 0);
    }

    #[test]
    fn the_revision_is_the_newest_id() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(revision(dir.path()).unwrap(), 0);
        let a = record(dir.path(), true, &result("", "a")).unwrap().unwrap();
        assert_eq!(revision(dir.path()).unwrap(), a);
        let b = record(dir.path(), true, &result("", "b")).unwrap().unwrap();
        assert!(b > a);
        assert_eq!(revision(dir.path()).unwrap(), b);
    }

    #[test]
    fn adding_past_the_limit_deletes_the_oldest_entries() {
        let dir = tempfile::tempdir().unwrap();
        for n in 1..=4 {
            add_within(dir.path(), "", &format!("entry {n}"), 3).unwrap();
        }
        let all = page(dir.path(), "", 0, 10).unwrap();
        assert_eq!(
            (all.total, texts(&all)),
            (3, vec!["entry 4", "entry 3", "entry 2"])
        );
        assert_eq!(LIMIT, 500);
    }
}
