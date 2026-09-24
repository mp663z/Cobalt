use super::*;
use kobo_ui::TextScale;
use kobo_web_document::{parse_document, Limits, Url};

fn fixture(name: &str) -> Document {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kobo-web-document/tests/fixtures")
        .join(format!("{name}.html"));
    let bytes = std::fs::read(path).expect("fixture");
    parse_document(
        &bytes,
        &Url::parse("https://fixtures.example/page.html").expect("url"),
        &Limits::DEFAULT,
    )
}

fn text_of_piece(piece: &Piece) -> String {
    match piece {
        Piece::Heading { text, .. }
        | Piece::Quote { text, .. }
        | Piece::Preformatted(text)
        | Piece::Note(text) => text.clone(),
        Piece::Prose(runs) => runs.iter().map(|run| run.text.as_str()).collect(),
        Piece::Table { rows, .. } => rows
            .iter()
            .filter(|row| !row.header)
            .map(|row| row.cells.join(" "))
            .collect::<Vec<_>>()
            .join(" "),
        Piece::Rule => String::new(),
    }
}

fn words(pieces: &[Piece]) -> Vec<String> {
    pieces
        .iter()
        .flat_map(|piece| {
            text_of_piece(piece)
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A stand-in for the panel: a page holds `budget` characters, and each
/// piece costs its text plus a line.
fn budget(budget: usize) -> impl FnMut(&[Piece]) -> bool {
    move |pieces| {
        pieces
            .iter()
            .map(|piece| text_of_piece(piece).len() + 40)
            .sum::<usize>()
            <= budget
    }
}

/// Pages rejoined, with the words of cut prose rejoined across the cut.
fn page_words(layout: &Layout) -> Vec<String> {
    let all: Vec<Piece> = layout.pages.iter().flatten().cloned().collect();
    words(&all)
}

fn document_words(document: &Document) -> Vec<String> {
    words(&pieces(document).0)
}

#[test]
fn packing_keeps_every_word_in_order() {
    for name in ["basic", "malformed", "sanitize", "latin1", "links"] {
        let document = fixture(name);
        for size in [200, 300, 500, 2_000] {
            let layout = paginate(&document, budget(size));
            assert_eq!(
                page_words(&layout),
                document_words(&document),
                "{name} at {size}"
            );
            assert!(!layout.truncated);
        }
    }
}

#[test]
fn no_page_ends_on_a_heading_it_could_have_carried() {
    let document = fixture("basic");
    for size in [200, 300, 400, 500] {
        let layout = paginate(&document, budget(size));
        for (index, page) in layout.pages.iter().enumerate() {
            if index + 1 < layout.pages.len() && page.len() > 1 {
                assert!(
                    !page.last().is_some_and(Piece::is_heading),
                    "page {index} at {size} ends on a heading: {page:?}"
                );
            }
        }
    }
}

#[test]
fn every_link_is_reachable_in_reading_order() {
    for name in ["basic", "links"] {
        let document = fixture(name);
        let layout = paginate(&document, budget(300));
        let mut seen: Vec<usize> = Vec::new();
        for piece in layout.pages.iter().flatten() {
            for link in piece.links() {
                if seen.last() != Some(&link) {
                    seen.push(link);
                }
            }
        }
        // Links in headings and quotes are reached from the link list; the
        // rest are drawn in the text.
        let expected: Vec<usize> = pieces(&document).0.iter().flat_map(Piece::links).collect();
        assert_eq!(seen, expected, "{name}");
        assert!(layout
            .pages
            .iter()
            .flatten()
            .all(|piece| piece.links().len() <= MAX_LINKS_PER_PIECE));
    }
}

#[test]
fn a_paragraph_with_many_links_is_carried_as_several_pieces() {
    let (flat, _) = pieces(&fixture("links"));
    let prose: Vec<&Piece> = flat
        .iter()
        .filter(|p| matches!(p, Piece::Prose(_)))
        .collect();
    let linked: Vec<usize> = prose.iter().flat_map(|piece| piece.links()).collect();
    assert_eq!(linked, (0..40).collect::<Vec<_>>());
    assert!(prose
        .iter()
        .all(|piece| piece.links().len() <= MAX_LINKS_PER_PIECE));
}

#[test]
fn a_word_wider_than_any_page_is_cut_and_packing_ends() {
    let long = "x".repeat(5_000);
    let document = parse_document(
        format!("<p>before {long} after</p>").as_bytes(),
        &Url::parse("https://x.example/").expect("url"),
        &Limits::DEFAULT,
    );
    let layout = paginate(&document, budget(400));
    let joined: String = layout.pages.iter().flatten().map(text_of_piece).collect();
    assert_eq!(joined.matches('x').count(), 5_000);
    assert!(layout.pages.len() > 10);
}

#[test]
fn a_page_that_can_never_fit_is_drawn_alone_rather_than_looping() {
    let document = fixture("basic");
    let layout = paginate(&document, |_| false);
    assert_eq!(layout.pages.len(), pieces(&document).0.len());
}

#[test]
fn page_count_is_capped() {
    let html = "<p>w</p>".repeat(5_000);
    let document = parse_document(
        html.as_bytes(),
        &Url::parse("https://x.example/").expect("url"),
        &Limits::DEFAULT,
    );
    let layout = paginate(&document, |pieces| pieces.len() <= 1);
    assert!(layout.truncated);
    assert_eq!(layout.pages.len(), MAX_PAGES);
}

#[test]
fn anchors_name_the_page_their_block_starts_on() {
    let document = fixture("basic");
    let layout = paginate(&document, budget(300));
    assert_eq!(layout.page_of("top"), Some(0));
    let history = layout.page_of("history").expect("history anchor");
    assert!(layout.pages[history]
        .iter()
        .any(|piece| matches!(piece, Piece::Heading { text, .. } if text == "A short history")));
    assert_eq!(layout.page_of("missing"), None);
}

#[test]
fn packing_is_deterministic() {
    let document = fixture("basic");
    assert_eq!(
        paginate(&document, budget(300)),
        paginate(&document, budget(300))
    );
}

#[test]
fn link_actions_round_trip() {
    assert_eq!(link_of(&link_action(0)), Some(0));
    assert_eq!(link_of(&link_action(3_999)), Some(3_999));
    assert_eq!(link_of("links"), None);
    assert_eq!(link_of("link-x"), None);
}

// Real panels from here: the toolkit's own layout decides what fits.

fn panels() -> Vec<(String, DisplayMetrics)> {
    let mut out = Vec::new();
    for profile in kobo_profile::SUPPORTED_PROFILES {
        for scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
            out.push((
                format!("{} {scale:?}", profile.id),
                DisplayMetrics {
                    width: i32::try_from(profile.width).expect("width"),
                    height: i32::try_from(profile.height).expect("height"),
                    pixels_per_inch: i32::from(profile.pixels_per_inch),
                    text_scale: scale,
                },
            ));
        }
    }
    out
}

#[test]
fn fixture_pages_fit_every_supported_panel_at_every_text_size() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    for name in ["basic", "malformed", "links"] {
        let document = fixture(name);
        let title = document.title.clone().unwrap_or_default();
        for (panel, metrics) in panels() {
            let layout = paginate_for(&document, &title, &metrics);
            assert_eq!(
                page_words(&layout),
                document_words(&document),
                "{name} on {panel}"
            );
            for (index, page) in layout.pages.iter().enumerate() {
                let screen = page_screen(&title, page, index, Some(layout.pages.len())).build();
                let issues: Vec<_> = screen
                    .diagnostics(&metrics, &Chrome::measuring(true))
                    .issues
                    .into_iter()
                    .filter(|issue| issue.severity == kobo_ui::DiagnosticSeverity::Error)
                    .collect();
                assert!(
                    issues.is_empty(),
                    "{name} page {index} on {panel}: {issues:?}"
                );
            }
        }
    }
}

#[test]
fn larger_text_takes_more_pages() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let document = fixture("basic");
    let at = |text_scale| {
        paginate_for(
            &document,
            "t",
            &DisplayMetrics {
                text_scale,
                ..kobo_ui::CLARA_BW_METRICS
            },
        )
        .pages
        .len()
    };
    let (normal, large, extra) = (
        at(TextScale::Default),
        at(TextScale::Large),
        at(TextScale::ExtraLarge),
    );
    assert!(
        normal <= large && large <= extra && normal < extra,
        "{normal} {large} {extra}"
    );
}

#[test]
fn pages_made_one_at_a_time_match_pages_made_at_once() {
    let document = fixture("basic");
    let whole = paginate(&document, budget(300));
    let mut paginator = Paginator::for_document(&document);
    let mut fits = budget(300);
    assert!(paginator.next_page(&mut fits));
    assert_eq!(paginator.pages()[0], whole.pages[0]);
    assert_eq!(paginator.page_of("top"), Some(0));
    assert!(paginator.has_fragment("history"));
    while paginator.next_page(&mut fits) {}
    assert!(paginator.done());
    assert_eq!(paginator.into_layout(), whole);
}

#[test]
fn every_link_in_the_document_is_offered_on_some_page() {
    let html = r#"<h2><a href="/h">Heading link</a></h2>
        <blockquote><p>Quoted <a href="/q">link</a></p></blockquote>
        <table><tr><th>A</th></tr><tr><td><a href="/t">cell link</a></td></tr></table>
        <a href="/i"><img src="/i.png" alt="linked picture"></a>
        <p>Plain <a href="/p">link</a>.</p>"#;
    let document = parse_document(
        html.as_bytes(),
        &Url::parse("https://x.example/").expect("url"),
        &Limits::DEFAULT,
    );
    for size in [120, 400, 5_000] {
        let layout = paginate(&document, budget(size));
        let mut offered: Vec<usize> = Vec::new();
        for link in layout.pages.iter().flatten().flat_map(Piece::all_links) {
            if !offered.contains(&link) {
                offered.push(link);
            }
        }
        assert_eq!(
            offered,
            (0..document.links.len()).collect::<Vec<_>>(),
            "at {size}: {:?}",
            document.links
        );
    }
}
