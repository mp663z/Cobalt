//! Regenerate corpus expected.census baselines after a reviewed fix.
//! Prints the diff-relevant lines first; only rewrites with --write.
use cobalt_text_preview::basic_preview::{prepare, Faces};
use std::path::PathBuf;

fn main() {
    let write = std::env::args().any(|a| a == "--write");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let faces = Faces::load(&root.join("../../crates/kobo-text/fonts")).unwrap();
    let mut directories: Vec<_> = std::fs::read_dir(root.join("corpus"))
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    directories.sort();
    for directory in directories {
        let mut sheets = Vec::new();
        for index in 0..48 {
            let path = directory.join(format!("sheet-{index}.css"));
            if !path.exists() { break; }
            sheets.push(std::fs::read(path).unwrap());
        }
        let page = prepare(&std::fs::read(directory.join("page.html")).unwrap(), &sheets, 800, &faces).unwrap();
        let new = page.census();
        let path = directory.join("expected.census");
        let old = std::fs::read_to_string(&path).unwrap();
        if old != new {
            println!("CHANGED {}", directory.display());
            if write { std::fs::write(&path, &new).unwrap(); }
        } else {
            println!("same    {}", directory.display());
        }
    }
}
