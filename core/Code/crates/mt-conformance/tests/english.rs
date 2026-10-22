// The source language of the core is English ([C-15]): an outside reviewer reads this tree
// without a translator. A line may carry Cyrillic only as data -- a test's own input, a folder an
// earlier build created -- and then it says so with CYRILLIC-DATA-OK on that very line.

use std::fs;
use std::path::{Path, PathBuf};

const MARK: &str = "CYRILLIC-DATA-OK";
const CODE: &[&str] = &[
    "rs", "toml", "sh", "py", "command", "json", "txt", "yml", "yaml",
];
const SKIP: &[&str] = &["target", ".git"];

// By code point, so the guard needs no exemption for itself.
fn is_cyrillic(c: char) -> bool {
    ('\u{0400}'..='\u{04FF}').contains(&c)
}

fn offends(line: &str) -> bool {
    !line.contains(MARK) && line.chars().any(is_cyrillic)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .expect("a directory of the tree is readable")
        .map(|e| e.expect("a directory entry is readable").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !SKIP.contains(&name) {
                walk(&path, out);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| CODE.contains(&e))
        {
            out.push(path);
        }
    }
}

#[test]
fn the_guard_flags_cyrillic_and_nothing_else() {
    let zhe = '\u{0436}';
    // An implementation that ignores the mark fails the second line; one that flags any letter
    // outside ASCII fails the third (c with cedilla is Latin); a range cut short fails the last
    // three.
    assert!(offends(&format!("let x = \"{zhe}\";")));
    assert!(!offends(&format!("let x = \"{zhe}\"; // {MARK}: data")));
    assert!(!offends("let x = \"\u{00E7}a\";"));
    assert!(!offends("let x = 1;"));
    assert!(offends("\u{04FF}"));
    assert!(offends("\u{0400}"));
    assert!(!offends("\u{0500}"));
}

#[test]
fn the_source_of_the_core_is_english() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(
        files
            .iter()
            .any(|f| f.ends_with("mt-conformance/tests/english.rs")),
        "the walk must see the tree it guards, this file included"
    );
    let mut bad = Vec::new();
    for file in &files {
        let text = fs::read_to_string(file).expect("a file of code is UTF-8");
        for (i, line) in text.lines().enumerate() {
            if offends(line) {
                bad.push(format!("{}:{}: {}", file.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} lines carry Cyrillic; translate, or mark real data with {MARK}:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
