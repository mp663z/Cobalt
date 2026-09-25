//! Real pages, saved and shrunk, read the way the browser reads them.
//!
//! Each page in `tests/corpus` has its expected reading next to it in
//! `tests/corpus/expected`: the title and visible text, every link in order,
//! and how many pages it takes on each supported reader at each text size.
//! Where the pages came from, and when, is in `tests/corpus/SOURCES.md`.
//!
//! A change to parsing or layout that moves any of these shows up here as a
//! diff to read. When the change is meant, write the new readings with
//! `BLESS_CORPUS=1 cargo test --release -p kobo-browser corpus` and commit
//! them with it. Page counts are checked in release builds only: paging the
//! long Wikipedia article nine times over takes minutes unoptimised.

use kobo_sdk::DisplayMetrics;
use kobo_ui::TextScale;
use kobo_web_document::{parse_document, Limits, Url};
use kobo_web_layout::paginate_for;
use std::fmt::Write as _;

/// The saved pages and the address each was fetched from.
const PAGES: [(&str, &str); 8] = [
    (
        "wikipedia-e-reader",
        "https://en.wikipedia.org/wiki/E-reader",
    ),
    (
        "wikipedia-search",
        "https://en.wikipedia.org/w/index.php?search=e-ink+display&fulltext=1&ns0=1",
    ),
    (
        "rust-book-installation",
        "https://doc.rust-lang.org/book/ch01-01-installation.html",
    ),
    (
        "rust-std-option",
        "https://doc.rust-lang.org/std/option/index.html",
    ),
    (
        "python-tutorial-intro",
        "https://docs.python.org/3/tutorial/introduction.html",
    ),
    (
        "rust-blog-post",
        "https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/",
    ),
    ("cut-short", "https://en.wikipedia.org/wiki/E-reader"),
    (
        "windows-1252",
        "https://docs.python.org/3/tutorial/introduction.html",
    ),
];

fn corpus(path: &str) -> String {
    format!("{}/tests/corpus/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// Width, height and pixels per inch.
type Screen = (u32, u32, u16);

/// One panel per distinct screen and text size. Readers that share a screen
/// (the two Clara BW revisions, say) page alike, so they are named together.
fn panels() -> Vec<(String, DisplayMetrics)> {
    let mut screens: Vec<(Screen, Vec<&str>)> = Vec::new();
    for profile in kobo_profile::SUPPORTED_PROFILES {
        let screen = (profile.width, profile.height, profile.pixels_per_inch);
        match screens.iter_mut().find(|(known, _)| *known == screen) {
            Some((_, ids)) => ids.push(profile.id),
            None => screens.push((screen, vec![profile.id])),
        }
    }
    let mut out = Vec::new();
    for ((width, height, pixels_per_inch), ids) in screens {
        for scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
            out.push((
                format!("{} {scale:?}", ids.join(",")),
                DisplayMetrics {
                    width: i32::try_from(width).expect("width"),
                    height: i32::try_from(height).expect("height"),
                    pixels_per_inch: i32::from(pixels_per_inch),
                    text_scale: scale,
                },
            ));
        }
    }
    out
}

/// Compares `actual` with the expected file, or writes it when blessing.
fn check(name: &str, kind: &str, actual: &str, failures: &mut Vec<String>) {
    let path = corpus(&format!("expected/{name}.{kind}"));
    if std::env::var_os("BLESS_CORPUS").is_some() {
        std::fs::write(&path, actual).unwrap_or_else(|error| panic!("{path}: {error}"));
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    if expected != actual {
        let line = expected
            .lines()
            .zip(actual.lines())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
        failures.push(format!(
            "{name}.{kind} differs from line {}:\n  expected: {:?}\n  actual:   {:?}",
            line + 1,
            expected.lines().nth(line).unwrap_or("<end>"),
            actual.lines().nth(line).unwrap_or("<end>"),
        ));
    }
}

fn read(name: &str, address: &str) -> (kobo_web_document::Document, String) {
    let path = corpus(&format!("{name}.html"));
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let url = Url::parse(address).expect("address");
    let document = parse_document(&bytes, &url, &Limits::DEFAULT);
    let title = document
        .title
        .clone()
        .unwrap_or_else(|| url.host().to_owned());
    (document, title)
}

#[test]
fn corpus_text_and_links_read_as_expected() {
    let mut failures = Vec::new();
    for (name, address) in PAGES {
        let (document, title) = read(name, address);
        let mut text = format!("title: {title}\n");
        for warning in &document.warnings {
            let _ = writeln!(text, "warning: {warning:?}");
        }
        text.push('\n');
        text.push_str(&document.visible_text());
        check(name, "text", &text, &mut failures);

        let mut links = String::new();
        for link in &document.links {
            let _ = writeln!(links, "{}\t{}", link.text, link.target);
        }
        check(name, "links", &links, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "pages the whole corpus on every screen; run with --release"
)]
fn corpus_page_counts_on_every_reader_and_text_size() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut failures = Vec::new();
    for (name, address) in PAGES {
        let (document, title) = read(name, address);
        let mut counts = String::new();
        for (panel, metrics) in panels() {
            let layout = paginate_for(&document, &title, &metrics);
            assert!(!layout.pages.is_empty(), "{name} on {panel}: no pages");
            assert!(
                layout.pages.iter().all(|page| !page.is_empty()),
                "{name} on {panel}: an empty page"
            );
            let _ = writeln!(
                counts,
                "{panel}\t{}{}",
                layout.pages.len(),
                if layout.truncated { " truncated" } else { "" }
            );
        }
        check(name, "pages", &counts, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_page_in_windows_1252_reads_like_the_same_page_in_utf8() {
    let address = "https://docs.python.org/3/tutorial/introduction.html";
    let (utf8, _) = read("python-tutorial-intro", address);
    let (legacy, _) = read("windows-1252", address);
    assert!(legacy
        .warnings
        .contains(&kobo_web_document::Warning::NotUtf8));
    assert_eq!(legacy.visible_text(), utf8.visible_text());
    assert_eq!(legacy.links, utf8.links);
}

#[test]
fn a_page_cut_short_keeps_what_arrived() {
    let address = "https://en.wikipedia.org/wiki/E-reader";
    let (whole, _) = read("wikipedia-e-reader", address);
    let (cut, title) = read("cut-short", address);
    assert_eq!(title, "E-reader - Wikipedia");
    assert!(cut.links.len() < whole.links.len());
    assert_eq!(cut.links[..], whole.links[..cut.links.len()]);
}
