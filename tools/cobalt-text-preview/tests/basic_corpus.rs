//! Corpus regressions pin geometry and every census category, not Chrome parity.
use cobalt_text_preview::basic_preview::{prepare, Faces};
use std::path::PathBuf;

#[test]
fn frozen_pages_keep_their_reviewed_census_and_height() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let faces = Faces::load(&root.join("../../crates/kobo-text/fonts")).unwrap();
    let mut directories: Vec<_> = std::fs::read_dir(root.join("corpus"))
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| e.path().is_dir())
        .collect();
    directories.sort_by_key(std::fs::DirEntry::file_name);
    assert_eq!(
        directories.len(),
        10,
        "add a baseline for every corpus page"
    );
    for directory in directories {
        let directory = directory.path();
        let mut sheets = Vec::new();
        for index in 0..48 {
            let path = directory.join(format!("sheet-{index}.css"));
            if !path.exists() {
                break;
            }
            sheets.push(std::fs::read(path).unwrap());
        }
        let page = prepare(
            &std::fs::read(directory.join("page.html")).unwrap(),
            &sheets,
            800,
            &faces,
        )
        .unwrap();
        assert_eq!(
            page.census(),
            std::fs::read_to_string(directory.join("expected.census")).unwrap(),
            "census changed for {}. Inspect the visual result before updating the baseline",
            directory.display()
        );
    }
}

#[test]
fn strip_output_is_opaque_and_has_exact_dimensions() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let faces = Faces::load(&root.join("../../crates/kobo-text/fonts")).unwrap();
    let page = prepare(b"<!doctype html><style>p{height:600px;background:#eeeeee}</style><p>Some text and a border</p>", &[], 800, &faces).unwrap();
    assert!(page.height > 512);
    let mut rgba = Vec::new();
    page.write_rgba(&mut rgba).unwrap();
    assert_eq!(rgba.len(), (page.width * page.height * 4) as usize);
    assert!(rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
}

#[test]
fn cli_checks_missing_arguments_and_stylesheet_gaps() {
    let binary = env!("CARGO_BIN_EXE_basic-page");
    let result = std::process::Command::new(binary).output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--page is required"));
    let result = std::process::Command::new(binary)
        .arg("--help")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("--fonts"));
    let directory = std::env::temp_dir().join(format!("basic-page-gap-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("sheet-1.css"), "p{color:red}").unwrap();
    let result = std::process::Command::new(binary)
        .args([
            "--page",
            "not-read.html",
            "--output",
            "not-written",
            "--stylesheet-dir",
        ])
        .arg(&directory)
        .output()
        .unwrap();
    assert!(!result.status.success());
    std::fs::remove_dir_all(directory).unwrap();
}
