use crate::core::Vocabulary;
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
pub fn scan(root: &Path, repository: &str, exclusions: &[String]) -> Result<Index, String> {
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
        let Ok(bytes) = std::fs::read(path) else {
            result.incomplete = true;
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }
        let Ok(source) = std::str::from_utf8(&bytes) else {
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
        let language = match path.extension().and_then(|s| s.to_str()) {
            Some("py") => tree_sitter_python::LANGUAGE.into(),
            Some("js" | "jsx" | "mjs" | "cjs") => tree_sitter_javascript::LANGUAGE.into(),
            Some("ts") => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Some("tsx") => tree_sitter_typescript::LANGUAGE_TSX.into(),
            _ => {
                result.skipped += 1;
                continue;
            }
        };
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language)
            .map_err(|_| "Cannot initialize symbol parser")?;
        let Some(tree) = parser.parse(source, None) else {
            result.skipped += 1;
            continue;
        };
        if tree.root_node().has_error() {
            result.skipped += 1;
            continue;
        }
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
                add(text.into());
            }
            let mut cursor = node.walk();
            pending.extend(node.named_children(&mut cursor));
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
