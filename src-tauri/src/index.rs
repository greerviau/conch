use crate::core::Vocabulary;
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub entries: Vec<Vocabulary>,
    pub skipped: usize,
    pub incomplete: bool,
    pub available: bool,
}
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
pub const MAX_ENTRIES: usize = 50_000;
/// Build output and dependency folders the scan does not enter.
pub const SKIPPED_DIRECTORIES: [&str; 6] = [
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
];
/// What a scan read from each file, so the next scan of the repository reads and
/// parses only the files whose size or modification time changed.
#[derive(Default)]
pub struct ScanCache {
    files: std::collections::HashMap<std::path::PathBuf, Scanned>,
    /// Files the latest scan read, as opposed to took from the cache.
    pub read: usize,
}
struct Scanned {
    modified: std::time::SystemTime,
    length: u64,
    /// The declared names, `None` when the file has no supported parser or did not
    /// parse, or nothing at all for a file that is not text.
    text: Option<Option<Vec<String>>>,
}
/// A file changed this recently is read again by the next scan: a second change
/// within the resolution of the file system's clock would keep the same time.
const SETTLED: std::time::Duration = std::time::Duration::from_secs(2);
pub fn scan(root: &Path, repository: &str, exclusions: &[String]) -> Result<Index, String> {
    scan_with(root, repository, exclusions, &mut ScanCache::default())
}
/// The declared names of a text file, or `None` without a supported parser or when the
/// file does not parse.
fn symbols(path: &Path, source: &str) -> Result<Option<Vec<String>>, String> {
    let language = match path.extension().and_then(|s| s.to_str()) {
        Some("py") => tree_sitter_python::LANGUAGE.into(),
        Some("js" | "jsx" | "mjs" | "cjs") => tree_sitter_javascript::LANGUAGE.into(),
        Some("ts") => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Some("tsx") => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => return Ok(None),
    };
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language)
        .map_err(|_| "Cannot initialize symbol parser")?;
    let Some(tree) = parser.parse(source, None) else {
        return Ok(None);
    };
    if tree.root_node().has_error() {
        return Ok(None);
    }
    let mut names = vec![];
    let mut pending = vec![tree.root_node()];
    while let Some(node) = pending.pop() {
        if matches!(
            node.kind(),
            "function_definition"
                | "class_definition"
                | "function_declaration"
                | "class_declaration"
                | "method_definition"
                | "variable_declarator"
                | "interface_declaration"
                | "type_alias_declaration"
                | "enum_declaration"
        ) && let Some(name) = node.child_by_field_name("name")
            && let Ok(text) = name.utf8_text(source.as_bytes())
        {
            names.push(text.to_owned());
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    Ok(Some(names))
}
/// `scan`, taking what an earlier scan read from the files that have not changed since
/// from `cache` and leaving what this one read in it.
pub fn scan_with(
    root: &Path,
    repository: &str,
    exclusions: &[String],
    cache: &mut ScanCache,
) -> Result<Index, String> {
    let mut known = std::mem::take(&mut cache.files);
    cache.read = 0;
    let started = std::time::SystemTime::now();
    if !root.is_dir() {
        return Ok(Index::default());
    }
    let root = root.canonicalize().map_err(|_| "Cannot open repository")?;
    let mut overrides = ignore::overrides::OverrideBuilder::new(&root);
    for pattern in exclusions {
        overrides
            .add(&format!("!{pattern}"))
            .map_err(|_| "Invalid exclusion pattern")?;
    }
    let walker = ignore::WalkBuilder::new(&root)
        .hidden(true)
        .require_git(false)
        .follow_links(false)
        .overrides(overrides.build().map_err(|_| "Invalid exclusions")?)
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .is_none_or(|name| !SKIPPED_DIRECTORIES.contains(&name))
        })
        .build();
    let mut result = Index {
        available: true,
        ..Default::default()
    };
    for entry in walker {
        let Ok(entry) = entry else {
            result.incomplete = true;
            continue;
        };
        if !entry.file_type().is_some_and(|f| f.is_file()) {
            continue;
        }
        if result.entries.len() >= MAX_ENTRIES {
            result.incomplete = true;
            break;
        }
        let path = entry.path();
        if !path.canonicalize().is_ok_and(|p| p.starts_with(&root)) {
            continue;
        }
        let Ok(meta) = path.metadata() else {
            result.incomplete = true;
            continue;
        };
        if meta.len() > MAX_FILE_BYTES {
            result.skipped += 1;
            continue;
        }
        let unchanged = known.remove(path).filter(|scanned| {
            scanned.length == meta.len() && meta.modified().is_ok_and(|at| at == scanned.modified)
        });
        let scanned = match unchanged {
            Some(scanned) => scanned,
            None => {
                let Ok(bytes) = std::fs::read(path) else {
                    result.incomplete = true;
                    continue;
                };
                cache.read += 1;
                let text = match std::str::from_utf8(&bytes) {
                    Ok(source) if !bytes.contains(&0) => Some(symbols(path, source)?),
                    _ => None,
                };
                Scanned {
                    // A file whose time cannot be read is read again by every scan.
                    modified: meta.modified().unwrap_or(started),
                    length: meta.len(),
                    text,
                }
            }
        };
        let text = scanned.text.clone();
        if started
            .duration_since(scanned.modified)
            .is_ok_and(|age| age >= SETTLED)
        {
            cache.files.insert(path.to_owned(), scanned);
        }
        let Some(declared) = text else {
            continue;
        };
        let file = path
            .strip_prefix(&root)
            .map_err(|_| "Invalid repository path")?
            .to_string_lossy()
            .replace('\\', "/");
        let mut add = |text: String| {
            if result.entries.len() < MAX_ENTRIES {
                result.entries.push(Vocabulary {
                    text,
                    repository: repository.into(),
                    file: file.clone(),
                });
            } else {
                result.incomplete = true;
            }
        };
        add(file.clone());
        if let Some(name) = path.file_name().and_then(|s| s.to_str())
            && name != file
        {
            add(name.into());
        }
        match declared {
            Some(names) => names.into_iter().for_each(&mut add),
            None => result.skipped += 1,
        }
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexes_paths_and_declarations_with_origins_but_excludes_ignored_and_binary() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".gitignore"), "ignored.py\n").unwrap();
        std::fs::write(d.path().join("ignored.py"), "def Secret(): pass").unwrap();
        std::fs::write(d.path().join("skip.py"), "def Excluded(): pass").unwrap();
        std::fs::write(
            d.path().join("main.py"),
            "def max_groups():\n pass\nclass Runner: pass",
        )
        .unwrap();
        std::fs::write(
            d.path().join("module.ts"),
            "export const maxGroups = 2; export class Controller {}",
        )
        .unwrap();
        std::fs::write(d.path().join("blob"), [0, 1, 2]).unwrap();
        let r = scan(d.path(), "r", &["skip.py".into()]).unwrap();
        assert!(r.available);
        assert!(
            r.entries
                .iter()
                .any(|e| e.text == "max_groups" && e.file == "main.py" && e.repository == "r")
        );
        assert!(r.entries.iter().any(|e| e.text == "Controller"));
        assert!(
            !r.entries
                .iter()
                .any(|e| e.file.contains("ignored") || e.file.contains("skip") || e.file == "blob")
        );
        std::fs::remove_file(d.path().join("main.py")).unwrap();
        assert!(
            !scan(d.path(), "r", &[])
                .unwrap()
                .entries
                .iter()
                .any(|e| e.text == "max_groups")
        );
    }
    /// Writes a file and dates it `age` seconds in the past.
    fn write_aged(path: &Path, text: &str, age: u64) {
        std::fs::write(path, text).unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age))
            .unwrap();
    }
    #[test]
    fn a_rescan_reads_only_the_files_that_changed_and_finds_what_a_full_scan_finds() {
        let d = tempfile::tempdir().unwrap();
        write_aged(&d.path().join("main.py"), "def max_groups():\n pass", 600);
        write_aged(
            &d.path().join("module.ts"),
            "export class Controller {}",
            600,
        );
        write_aged(&d.path().join("notes.md"), "plain text", 600);
        write_aged(&d.path().join("broken.py"), "def (:", 600);
        std::fs::write(d.path().join("blob"), [0, 1, 2]).unwrap();
        let mut cache = ScanCache::default();
        let scan_again = |cache: &mut ScanCache| {
            let found = scan_with(d.path(), "r", &[], cache).unwrap();
            assert_eq!(found, scan(d.path(), "r", &[]).unwrap());
            found
        };
        let first = scan_again(&mut cache);
        assert_eq!(cache.read, 5);
        assert_eq!(first.skipped, 2);
        // The file written just now is read again; the others are not.
        assert_eq!(scan_again(&mut cache), first);
        assert_eq!(cache.read, 1);
        // A changed file is read, and its old names are gone.
        write_aged(&d.path().join("main.py"), "class Runner: pass", 500);
        let changed = scan_again(&mut cache);
        assert_eq!(cache.read, 2);
        assert!(changed.entries.iter().any(|e| e.text == "Runner"));
        assert!(!changed.entries.iter().any(|e| e.text == "max_groups"));
        // A removed file leaves the index, and comes back when it is restored.
        std::fs::remove_file(d.path().join("module.ts")).unwrap();
        assert!(
            !scan_again(&mut cache)
                .entries
                .iter()
                .any(|e| e.text == "Controller")
        );
        write_aged(
            &d.path().join("module.ts"),
            "export class Controller {}",
            600,
        );
        assert!(
            scan_again(&mut cache)
                .entries
                .iter()
                .any(|e| e.text == "Controller")
        );
        assert_eq!(cache.read, 2);
    }
    #[test]
    fn the_same_size_and_an_older_time_still_count_as_a_change() {
        let d = tempfile::tempdir().unwrap();
        write_aged(&d.path().join("main.py"), "class Alpha: pass", 500);
        let mut cache = ScanCache::default();
        scan_with(d.path(), "r", &[], &mut cache).unwrap();
        write_aged(&d.path().join("main.py"), "class Gamma: pass", 900);
        let found = scan_with(d.path(), "r", &[], &mut cache).unwrap();
        assert_eq!(cache.read, 1);
        assert!(found.entries.iter().any(|e| e.text == "Gamma"));
    }
    #[cfg(unix)]
    #[test]
    fn does_not_follow_external_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        std::fs::write(external.path().join("private.py"), "class Private: pass").unwrap();
        std::os::unix::fs::symlink(external.path(), root.path().join("link")).unwrap();
        let r = scan(root.path(), "r", &[]).unwrap();
        assert!(r.entries.is_empty());
        assert!(
            !scan(&root.path().join("missing"), "r", &[])
                .unwrap()
                .available
        );
    }
}
