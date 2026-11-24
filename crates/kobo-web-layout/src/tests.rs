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
        | Piece::ColouredPreformatted{text,..}
        | Piece::Note(text) => text.clone(),
        Piece::Prose(runs) => runs.iter().map(|run| run.text.as_str()).collect(),
        Piece::Table { rows, .. } => rows
            .iter()
            .filter(|row| !row.header)
            .map(|row| row.cells.join(" "))
            .collect::<Vec<_>>()
            .join(" "),
        Piece::Rule | Piece::Picture { .. } | Piece::Form { .. } => String::new(),
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

fn html(markup: &str) -> Document {
    parse_document(
        markup.as_bytes(),
        &Url::parse("https://fixtures.example/page.html").expect("url"),
        &Limits::DEFAULT,
    )
}

#[test]
fn an_image_with_a_declared_size_gets_room_and_keeps_its_description() {
    let document = html(
        "<p>Before.</p><img src=a.png width=640 height=480 alt=\"A chart\">\
         <img src=pixel.gif width=1 height=1><img src=b.png alt=\"No size\"><p>After.</p>",
    );
    let (pieces, _) = pieces(&document);
    let pictures: Vec<_> = pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Picture {
                image,
                width,
                height,
            } => Some((*image, *width, *height)),
            _ => None,
        })
        .collect();
    assert_eq!(
        pictures,
        vec![(0, 640, 480)],
        "only the sized, visible image"
    );
    let text: Vec<String> = pieces.iter().map(text_of_piece).collect();
    assert!(text.contains(&"Image: A chart".to_owned()));
    assert!(text.contains(&"Image: No size".to_owned()));
    assert_eq!(document.images()[0].src.path(), "/a.png");
}

#[test]
fn a_picture_is_never_squeezed_into_the_foot_of_a_page() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let mut markup = String::new();
    for n in 0..40 {
        markup.push_str(&format!(
            "<p>Paragraph {n} with enough words to fill a line or two.</p>\
             <img src=p{n}.png width=800 height=600 alt=\"Plate {n}\">"
        ));
    }
    let document = html(&markup);
    for (panel, metrics) in panels() {
        let layout = paginate_for(&document, "Plates", &metrics);
        let whole = {
            let alone = page_screen(
                "Plates",
                &[Piece::Picture {
                    image: 0,
                    width: 800,
                    height: 600,
                }],
                0,
                Some(1),
            )
            .build();
            picture_heights(&alone, &metrics)[0].1
        };
        let mut seen = 0;
        for (index, page) in layout.pages.iter().enumerate() {
            let screen = page_screen("Plates", page, index, Some(layout.pages.len())).build();
            for (_, height) in picture_heights(&screen, &metrics) {
                assert_eq!(height, whole, "page {index} on {panel}");
                seen += 1;
            }
        }
        assert_eq!(seen, 40, "every picture is on some page on {panel}");
    }
}

#[test]
fn a_page_is_found_again_by_its_place_in_the_text() {
    let pieces: Vec<Piece> = (0..20).map(|n| Piece::Note(format!("piece {n}"))).collect();
    let mut three = Paginator::new(pieces.clone(), Vec::new());
    let mut fits = |page: &[Piece]| page.len() <= 3;
    while three.next_page(&mut fits) {}
    // Pieces 0-2 on page 0, 3-5 on page 1, and so on.
    assert_eq!(three.place_of(2), Some(6));
    let mut five = Paginator::new(pieces, Vec::new());
    let mut fits = |page: &[Piece]| page.len() <= 5;
    while five.next_page(&mut fits) {}
    // The same words, cut into bigger pages, are on page 1.
    assert_eq!(five.page_of_place(6), Some(1));
    assert_eq!(five.page_of_place(99), Some(five.pages().len() - 1));
    assert_eq!(five.place_of(0), Some(0));
}

fn code_panels() -> Vec<(DisplayMetrics, String)> {
    let mut out = Vec::new();
    for base in [
        kobo_ui::CLARA_BW_METRICS,
        DisplayMetrics {
            width: 1264,
            height: 1680,
            pixels_per_inch: 300,
            ..kobo_ui::CLARA_BW_METRICS
        },
        DisplayMetrics {
            width: 1404,
            height: 1872,
            pixels_per_inch: 227,
            ..kobo_ui::CLARA_BW_METRICS
        },
    ] {
        for scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
            out.push((
                DisplayMetrics {
                    text_scale: scale,
                    ..base
                },
                format!("{}x{} {scale:?}", base.width, base.height),
            ));
        }
    }
    out
}

fn code_nodes(screen: &Screen, metrics: &DisplayMetrics, marker: &str) -> Vec<Vec<String>> {
    let layout=screen.layout_with(metrics,&Chrome::measuring(true));
    let Some(first)=layout.nodes.iter().find(|n|matches!(n.kind,kobo_ui::LayoutKind::MonoRichText(_))&&n.text_lines.iter().any(|line|line.contains(marker)))else{return Vec::new()};
    let mut rows=Vec::<String>::new();
    for n in layout.nodes.iter().filter(|n|n.id==first.id&&matches!(n.kind,kobo_ui::LayoutKind::MonoRichText(_))&&!n.text_lines.is_empty()) {
        let index=((n.rect.y-first.rect.y)/n.rect.height.max(1))as usize;
        rows.resize(rows.len().max(index+1),String::new());
        rows[index].push_str(&n.text_lines[0]);
        assert_eq!(n.rect.width,kobo_ui::with_reading_scale(screen.text_scale.unwrap_or(metrics.text_scale),||kobo_ui::measure_text_in(&n.text_lines[0],kobo_ui::FontSize::Body,kobo_ui::Face::ReadingMono).0));
    }
    vec![rows]
}

#[test]
fn browser_code_retains_hard_line_indents_and_wraps_continuations_flush() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).unwrap();
    for (metrics, name) in code_panels() {
        let pieces = [Piece::Preformatted(
            "fn turn() {\n    *page += 1;\n        deeper();\n}".into(),
        )];
        let screen = page_screen_with_metrics("Code", &pieces, 0, Some(1), None, &metrics).build();
        // Rich runs share one paragraph; recover blank rows from their y positions.
        // Indentation is source text, never extra paragraph spacing.
        let nodes = code_nodes(&screen, &metrics, "fn turn()");
        assert_eq!(nodes.len(), 1, "{name}: one node for the block");
        assert_eq!(
            nodes[0],
            vec!["fn turn() {", "    *page += 1;", "        deeper();", "}"],
            "{name}"
        );
        assert!(fits(&screen, &metrics), "{name}");
    }
}

#[test]
fn browser_code_keeps_a_blank_line_as_one_line_and_still_fits() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).unwrap();
    for (metrics, name) in code_panels() {
        let pieces = [Piece::Preformatted("a();\n\nb();".into())];
        let screen = page_screen_with_metrics("Code", &pieces, 0, Some(1), None, &metrics).build();
        let nodes = code_nodes(&screen, &metrics, "a();");
        assert_eq!(nodes.len(), 1, "{name}");
        assert_eq!(
            nodes[0],
            vec!["a();", "", "b();"],
            "{name}: blank line kept"
        );
        assert!(
            fits(&screen, &metrics),
            "{name}: no hidden node for the blank line"
        );
    }
}

#[test]
fn browser_code_wrap_continuations_do_not_inherit_the_indent() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).unwrap();
    let long = format!("    {}", "word ".repeat(80));
    for (metrics, name) in code_panels() {
        let pieces = [Piece::Preformatted(long.clone())];
        let screen = page_screen_with_metrics("Code", &pieces, 0, Some(1), None, &metrics).build();
        let nodes = code_nodes(&screen, &metrics, "word");
        assert_eq!(nodes.len(), 1, "{name}");
        assert!(nodes[0].len() > 1, "{name}: the line wrapped");
        assert!(
            nodes[0][0].starts_with("    word"),
            "{name}: hard-line indent kept"
        );
        assert!(
            nodes[0][1..].iter().all(|line| !line.starts_with(' ')),
            "{name}: continuation rows start at the left edge: {:?}",
            nodes[0]
        );
    }
}

#[test]
fn browser_code_has_exactly_one_mono_line_height_per_source_line() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).unwrap();
    let code = "fn main() {\n    let v = vec![1, 2, 3];\n    for x in &v {\n        println!(\"{x}\");\n    }\n\n}";
    for (metrics, name) in code_panels() {
        let pieces = [Piece::Preformatted(code.into())];
        let browser = page_screen_with_metrics("Code", &pieces, 0, Some(1), None, &metrics).build();
        let plain = kobo_ui::with_reading_scale(metrics.text_scale, || {
            page_screen_with_metrics("Code", &[], 0, Some(1), None, &metrics)
                .rich_text(code,Vec::new(),ParagraphPresentation{monospaced:true,..Default::default()})
                .build()
        });
        let height = |screen: &Screen| {
            screen
                .layout_with(&metrics, &Chrome::measuring(true))
                .flow_height
        };
        assert_eq!(
            height(&browser),
            height(&plain),
            "{name}: no per-line spacing"
        );
    }
}

#[test]
fn browser_fitting_tables_stay_in_the_prose_measure_and_wide_tables_keep_room() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).unwrap();
    for profile in [
        kobo_ui::CLARA_BW_METRICS,
        DisplayMetrics {
            width: 1264,
            height: 1680,
            pixels_per_inch: 300,
            ..kobo_ui::CLARA_BW_METRICS
        },
        DisplayMetrics {
            width: 1404,
            height: 1872,
            pixels_per_inch: 227,
            ..kobo_ui::CLARA_BW_METRICS
        },
    ] {
        for scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
            let metrics = DisplayMetrics {
                text_scale: scale,
                ..profile
            };
            let rows = vec![
                TableRow {
                    header: true,
                    cells: vec!["Mode".into(), "Speed".into()],
                    spans:Vec::new(),
                },
                TableRow {
                    header: false,
                    cells: vec!["Full".into(), "Slow".into()],
                    spans:Vec::new(),
                },
            ];
            let pieces = [
                Piece::Prose(vec![Run {
                    text: "Prose".into(),
                    strong: false,
                    emphasis: false,
                    link: None,
                    ..Run::default()
                }]),
                Piece::Table {
                    rows,
                    links: Vec::new(),
                },
            ];
            let screen =
                page_screen_with_metrics("Tables", &pieces, 0, Some(1), None, &metrics).build();
            let layout = screen.layout_with(&metrics, &Chrome::measuring(true));
            let text = layout
                .nodes
                .iter()
                .find(|node| node.text_lines == ["Prose"])
                .unwrap();
            let table = layout
                .nodes
                .iter()
                .find(|node| matches!(node.kind, LayoutKind::TableHeaderCell))
                .unwrap();
            assert!(
                (table.rect.x - text.rect.x).abs() <= 2,
                "{} {scale:?}: {} vs {}",
                metrics.width,
                table.rect.x,
                text.rect.x
            );
            assert!(fits(&screen, &metrics));
            let rows = vec![TableRow {
                header: false,
                cells: vec![
                    "A long unbroken sentence taking more than the prose measure".repeat(3),
                    "Second cell".into(),
                ],
                spans:Vec::new(),
            }];
            let screen = page_screen_with_metrics(
                "Wide",
                &[Piece::Table {
                    rows,
                    links: Vec::new(),
                }],
                0,
                Some(1),
                None,
                &metrics,
            )
            .build();
            assert!(matches!(
                screen.nodes.first(),
                Some(kobo_ui::Node::Table { .. })
            ));
        }
    }
}

#[test]
fn a_single_word_wider_than_the_page_is_cut_inside_the_word_without_losing_a_letter() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let word = "x".repeat(3000);
    let document = html(&format!("<p>{word}</p>"));
    let metrics = kobo_ui::CLARA_BW_METRICS;
    let layout = paginate_for(&document, "Long", &metrics);
    assert!(
        layout.pages.len() > 1,
        "a 3000 letter word needs several pages"
    );
    let kept: usize = layout
        .pages
        .iter()
        .flatten()
        .filter_map(text_of)
        .map(|text| text.chars().filter(|c| *c == 'x').count())
        .sum();
    assert_eq!(kept, 3000, "no letter is dropped between pages");
}

#[test]
fn splitting_preformatted_text_keeps_every_line_break() {
    for text in ["a\n\nz", "a\n\n\nz", "ab\ncd\n\nef\n", "\n\nx"] {
        let piece = Piece::Preformatted(text.to_owned());
        for at in cut_points(&piece) {
            let (head, tail) = split_at(&piece, at);
            let (Piece::Preformatted(head), Piece::Preformatted(tail)) = (head, tail) else {
                panic!("a preformatted piece splits into preformatted pieces");
            };
            assert_eq!(format!("{head}\n{tail}"), text, "{text:?} cut at {at}");
        }
    }
}

// Pins the reset after a soft break in the shared wrapper, with the reviewed
// fonts. The kobo-ui built-in test metrics were tried as a negative control:
// at widths 20-199 for these three spaces they showed no difference between
// the candidate and the always-hard mutant, so they cannot pin this (measured,
// not a universal property).
// A word wider than the line is cut at a grapheme boundary, which can leave a
// no-break space at the start of the next line. The wrap began that line, not
// the text, so it drops the space. With every line treated as a hard start the
// continuation would begin with the space. This is the inherited rule that
// drops leading soft whitespace on a wrapped line: it is NOT full CSS
// `white-space: pre-wrap`, which would keep it.
#[test]
fn a_forced_cut_drops_the_unicode_space_it_leaves_at_the_next_line() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).unwrap();
    for space in ['\u{00A0}', '\u{2007}', '\u{202F}'] {
        let text = format!("aaaa{space}bbbbbbbbbbbb");
        let lines = kobo_ui::wrap_text(&text, 78, kobo_ui::FontSize::Body);
        assert_eq!(lines, vec!["aaaa", "bbb", "bbb", "bbb", "bbb"], "{space:?}");
    }
}

#[test]
fn quote_colour_spans_follow_utf8_splits_and_link_identity() {
    let piece=Piece::Quote{depth:2,text:"é red blue".into(),links:vec![7],spans:vec![RichTextSpan{start:0,end:6,presentation:TextPresentation{foreground:Some([200,0,0]),..Default::default()}},RichTextSpan{start:6,end:11,presentation:TextPresentation{background:Some([0,0,200]),..Default::default()}}]};
    let (left,right)=split_at(&piece,7);
    let Piece::Quote{text,spans,links,..}=left else{panic!()};
    assert_eq!(text,"é red");assert_eq!(links,vec![7]);assert_eq!(spans[0].end,text.len());assert_eq!(spans.len(),1);
    let Piece::Quote{text,spans,links,..}=right else{panic!()};
    assert_eq!(text,"blue");assert_eq!(links,vec![7]);assert_eq!(spans[0].start,0);assert_eq!(spans[0].end,4);
}

#[test]
fn long_coloured_quote_pagination_keeps_rgb_and_words() {
    let html=format!("<blockquote><p style='color:red'>{}</p></blockquote>","é quoted words ".repeat(160));
    let doc=kobo_web_document::parse_coloured_document(html.as_bytes(),&Url::parse("https://fixtures.example/").unwrap(),&Limits::DEFAULT);
    let flat=pieces(&doc).0;let pages=paginate(&doc,budget(220));
    assert!(pages.pages.len()>3);assert_eq!(words(&flat),words(&pages.pages.iter().flatten().cloned().collect::<Vec<_>>()));
    for piece in pages.pages.iter().flatten(){if let Piece::Quote{text,spans,..}=piece {
        assert!(!spans.is_empty());assert_eq!(spans[0].start,0);assert_eq!(spans.last().unwrap().end,text.len());
        assert!(spans.iter().all(|s|s.presentation.foreground==Some([255,0,0])));
    }}
}

#[test]
fn pre_colour_keeps_raw_spaces_blank_lines_and_split_ranges() {
    let html=b"<pre>\n<span style='color:red'>  red  x\n\n</span><span style='color:blue'>    blue y</span>  \n</pre>";
    let url=Url::parse("https://fixtures.example/").unwrap();
    let plain=parse_document(html,&url,&Limits::DEFAULT);
    let colour=kobo_web_document::parse_coloured_document(html,&url,&Limits::DEFAULT);
    assert_eq!(plain.visible_text(),colour.visible_text());
    let flat=pieces(&colour).0;
    let Piece::ColouredPreformatted{text,spans}=&flat[0] else {panic!()};
    assert_eq!(text,"  red  x\n\n    blue y");assert_eq!(spans.len(),2);
    let (head,tail)=split_at(&flat[0],10);
    let Piece::ColouredPreformatted{text:head,spans:hs}=head else{panic!()};
    let Piece::ColouredPreformatted{text:tail,spans:ts}=tail else{panic!()};
    assert_eq!(format!("{head}\n{tail}"),*text);
    assert_eq!(hs.last().unwrap().end,head.len());assert_eq!(ts[0].start,0);
    assert!(ts[0].presentation.foreground==Some([0,0,255]));
}

#[test]
fn coloured_table_repeats_header_spans_across_pages(){
    let mut html=String::from("<table><tr><th style='color:red'>Name</th><th>Value</th></tr>");
    for _ in 0..30{html.push_str("<tr><td style='color:blue'>Item</td><td>42</td></tr>");}html.push_str("</table>");
    let doc=kobo_web_document::parse_coloured_document(html.as_bytes(),&Url::parse("https://fixtures.example/").unwrap(),&Limits::DEFAULT);
    let pages=paginate(&doc,|pieces|pieces.iter().map(|p|match p{Piece::Table{rows,..}=>rows.len(),_=>1}).sum::<usize>()<=5);assert!(pages.pages.len()>2);
    for piece in pages.pages.iter().flatten(){if let Piece::Table{rows,..}=piece{
        assert!(rows[0].header);assert_eq!(rows[0].spans[0][0].presentation.foreground,Some([255,0,0]));
        for row in &rows[1..]{assert_eq!(row.spans[0][0].presentation.foreground,Some([0,0,255]));}
    }}
}

#[test]
fn excessive_colour_runs_keep_text_and_have_bounded_payloads(){
    let inner=(0..600).map(|i|format!("<span style='color:{}'>w{i} </span>",if i%2==0{"red"}else{"blue"})).collect::<String>();
    for tag in ["p","h1","pre","blockquote","th"]{
        let html=if tag=="th"{format!("<table><tr><th>{inner}</th></tr></table>")}else{format!("<{tag}>{inner}</{tag}>")};
        let doc=kobo_web_document::parse_coloured_document(html.as_bytes(),&Url::parse("https://fixtures.example/").unwrap(),&Limits::DEFAULT);
        let (flat,_)=pieces(&doc);let all=flat.iter().map(|p|match p{Piece::Table{rows,..}=>rows.iter().flat_map(|r|r.cells.iter().map(String::as_str)).collect::<String>(),_=>text_of_piece(p)}).collect::<String>();assert!(all.contains("w599"),"{tag}");
        for piece in &flat{match piece{
            Piece::Prose(_)=>{let screen=append(ScreenBuilder::new("bounded"),std::slice::from_ref(piece)).build();assert!(screen.nodes.iter().all(|n|!matches!(n,kobo_ui::Node::RichText{spans,..} if spans.len()>kobo_ui::MAX_RICH_TEXT_SPANS)));},
            Piece::Heading{spans,..}|Piece::Quote{spans,..}|Piece::ColouredPreformatted{spans,..}=>assert!(spans.len()<=kobo_ui::MAX_RICH_TEXT_SPANS),
            Piece::Table{rows,..}=>assert!(rows.iter().flat_map(|r|&r.spans).all(|s|s.len()<=kobo_ui::MAX_RICH_TEXT_SPANS)),_=>{}
        }}
    }
}

#[test]
fn mono_unbroken_tokens_wrap_inside_the_same_measured_page() {
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).unwrap();
    for (metrics,name) in code_panels(){
        let text="i".repeat(160);
        let screen=page_screen_with_metrics("Code",&[Piece::Preformatted(text.clone())],0,Some(1),None,&metrics).build();
        let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
        let runs:Vec<_>=layout.nodes.iter().filter(|n|matches!(n.kind,kobo_ui::LayoutKind::MonoRichText(_))&&!n.text_lines.is_empty()).collect();
        assert!(runs.len()>1,"{name}");
        let boundary=layout.nodes.iter().find(|n|matches!(n.kind,kobo_ui::LayoutKind::MonoRichText(_))&&n.text_lines.is_empty()).unwrap();
        assert!(runs.iter().all(|n|n.rect.x+n.rect.width<=boundary.rect.x+boundary.rect.width),"{name}: forced wrap escaped mono measure");
        assert_eq!(runs.iter().map(|n|n.text_lines[0].as_str()).collect::<String>(),text,"{name}");
    }
}

#[test]
fn inline_code_keeps_mono_face_rgb_and_link_geometry_when_wrapped(){
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).unwrap();
    let url=Url::parse("https://fixtures.example/code").unwrap();
    let html=format!("<p>before <a href='/next'><code>{}</code></a> after</p>","i".repeat(160));
    let doc=parse_document(html.as_bytes(),&url,&Limits::DEFAULT);
    for (metrics,name) in code_panels(){
        let items=pieces(&doc).0;
        assert!(items.iter().any(|p|matches!(p,Piece::Prose(runs) if runs.iter().any(|r|r.code))));
        let screen=page_screen_with_metrics("Code",&items,0,Some(1),None,&metrics).build();
        let layout=screen.layout_with(&metrics,&Chrome::measuring(true));
        let runs:Vec<_>=layout.nodes.iter().filter(|n|matches!(n.kind,kobo_ui::LayoutKind::MonoRichText(_))&&!n.text_lines.is_empty()).collect();
        assert!(runs.len()>1,"{name}");
        let bound=layout.nodes.iter().find(|n|n.id==runs[0].id&&n.text_lines.is_empty()&&matches!(n.kind,kobo_ui::LayoutKind::RichText(_))).unwrap();
        assert!(runs.iter().all(|n|n.rect.x+n.rect.width<=bound.rect.x+bound.rect.width),"{name}");
        assert_eq!(runs.iter().map(|n|n.text_lines[0].as_str()).collect::<String>(),"i".repeat(160));
        for run in &runs {
            let width=kobo_ui::with_reading_scale(screen.text_scale.unwrap_or(metrics.text_scale),||kobo_ui::measure_text_in(&run.text_lines[0],kobo_ui::FontSize::Body,kobo_ui::Face::ReadingMono).0);
            assert_eq!(run.rect.width,width,"{name}");
            assert!(layout.nodes.iter().any(|n|matches!(n.kind,kobo_ui::LayoutKind::InlineLink(_))&&n.rect==run.rect),"{name}: link must cover actual mono run");
        }
        assert!(fits(&screen,&metrics),"{name}");
    }
}

#[test]
fn uncoloured_block_spans_keep_authored_emphasis_without_inventing_rgb(){
    let doc=parse_document(b"<h2><strong>Heading</strong></h2><blockquote><p><em>Quote</em></p></blockquote><table><tr><th>Key</th><th>Value</th></tr><tr><td><strong><em>Cell</em></strong></td><td>42</td></tr></table>",&Url::parse("https://fixtures.example/styles").unwrap(),&Limits::DEFAULT);
    let (items,_)=pieces(&doc);
    let mut styles=Vec::new();
    for p in items {match p {
        Piece::Heading{spans,..}|Piece::Quote{spans,..}=>styles.extend(spans.iter().map(|s|s.presentation)),
        Piece::Table{rows,..}=>for r in rows{styles.extend(r.spans.into_iter().flatten().map(|s|s.presentation));},_=>{}
    }}
    assert_eq!(styles.len(),3,"{styles:?}");
    assert!(styles[0].strong);assert!(styles[1].emphasis);assert!(styles[2].strong&&styles[2].emphasis);
    assert!(styles.iter().all(|s|s.foreground.is_none()&&s.background.is_none()));
}

#[test]
fn too_many_uncoloured_code_spans_show_a_simplification_note(){
    let html=format!("<p>{}</p>","<code>i</code> x ".repeat(600));
    let doc=parse_document(html.as_bytes(),&Url::parse("https://fixtures.example/styles").unwrap(),&Limits::DEFAULT);
    let (items,_)=pieces(&doc);
    assert!(items.iter().any(|p|matches!(p,Piece::Note(s) if s=="Some text colours simplified.")));
    assert!(items.iter().filter_map(|p|if let Piece::Prose(runs)=p{Some(runs)}else{None}).flatten().filter(|r|r.code).count()>256);
}
