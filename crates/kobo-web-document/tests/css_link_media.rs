//! Link and `<style>` media attributes evaluated by the shared bounded media
//! evaluator, plus the exact host-supply count handshake on the fully parsed
//! path. Real engine, no mocks. Isolated diagnostic branch only.
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_media, style_diagnostics::RefusalReason,
    style_tree::StyleTree, Limits, MediaContext,
};

fn run(html: &str, sheets: &[&str], width: Option<u32>) -> StyleTree {
    let sheets: Vec<Vec<u8>> = sheets.iter().map(|s| s.as_bytes().to_vec()).collect();
    match width {
        Some(w) => parse_style_tree_with_media(
            html.as_bytes(),
            &sheets,
            &Limits::DEFAULT,
            MediaContext::new(w).unwrap(),
        ),
        None => parse_style_tree(html.as_bytes(), &sheets, &Limits::DEFAULT),
    }
}
fn p_size(t: &StyleTree) -> u32 {
    t.nodes
        .iter()
        .find(|n| n.tag == "p")
        .unwrap()
        .style
        .font_size
}
fn style_page(media: &str) -> String {
    format!("<!doctype html><style media=\"{media}\">p{{font-size:20px}}</style><p>x</p>")
}
fn media_refused(t: &StyleTree) -> usize {
    t.diagnostics.count(RefusalReason::StylesheetMedia)
}

#[test]
fn type_lists_true_false_and_unknown_permutations() {
    // (media, applies at 1072, refused)
    let cases = [
        ("screen", true, false),
        ("all", true, false),
        ("screen, print", true, false),
        ("print, screen", true, false),
        ("print", false, false),
        ("print, print", false, false),
        ("only screen", true, false),
        ("speech", false, true),
        ("screen, speech", false, true),
        ("speech, screen", false, true),
        ("print, speech", false, true),
        ("tv", false, true),
    ];
    for (media, applies, refused) in cases {
        for w in [Some(1072), None] {
            let t = run(&style_page(media), &[], w);
            assert_eq!(media_refused(&t), usize::from(refused), "{media} {w:?}");
            assert_eq!(t.unsupported, refused, "{media} {w:?}");
            if !refused {
                assert_eq!(p_size(&t) == 20, applies, "{media} {w:?}");
            }
        }
    }
}

#[test]
fn width_features_follow_the_viewport_exactly() {
    let m = "screen and (min-width:768px)";
    assert_eq!(p_size(&run(&style_page(m), &[], Some(768))), 20);
    assert_eq!(p_size(&run(&style_page(m), &[], Some(1072))), 20);
    let below = run(&style_page(m), &[], Some(767));
    assert!(!below.unsupported, "{:?}", below.diagnostics);
    assert_ne!(p_size(&below), 20);
    let m = "(max-width:600px)";
    assert_ne!(p_size(&run(&style_page(m), &[], Some(1072))), 20);
    assert_eq!(p_size(&run(&style_page(m), &[], Some(600))), 20);
    // em uses the 16px initial font size, exactly: 40.0625em = 641px.
    let m = "(min-width: 40.0625em)";
    assert_eq!(p_size(&run(&style_page(m), &[], Some(641))), 20);
    assert_ne!(p_size(&run(&style_page(m), &[], Some(640))), 20);
}

#[test]
fn unknown_queries_empty_values_and_missing_context_fail_closed() {
    for m in [
        "screen, print and (min-width:600px)",
        "print, (min-width:600px)",
        "(prefers-color-scheme: dark)",
        "screen and (prefers-color-scheme: dark)",
        "not screen",
        "screen and (min-width:1px) /* c */",
        "(min-width:1px) and (min-width:2px) and (min-width:3px) and (min-width:4px) and (min-width:5px) and (min-width:6px) and (min-width:7px) and (min-width:8px) and (min-width:9px)",
        "",
        "   ",
    ] {
        let t = run(&style_page(m), &[], Some(1072));
        assert_eq!(media_refused(&t), 1, "{m:?}");
        assert!(t.unsupported, "{m:?}");
    }
    // No MediaContext: any width feature refuses, even a satisfiable one.
    let t = run(&style_page("(min-width:1px)"), &[], None);
    assert_eq!(media_refused(&t), 1);
    // Absent attribute stays Screen.
    let t = run(
        "<!doctype html><style>p{font-size:20px}</style><p>x</p>",
        &[],
        None,
    );
    assert!(!t.unsupported);
}

#[test]
fn link_media_known_false_consumes_no_slot_or_host_index() {
    // Three links; the first is width-false at 700 and must consume nothing.
    let html = "<!doctype html>\
        <link rel=stylesheet media='(min-width:768px)' href=/a>\
        <link rel=stylesheet href=/b>\
        <link rel=stylesheet media='screen, print' href=/c><p>x</p>";
    let t = run(html, &["p{font-size:11px}", "p{font-size:12px}"], Some(700));
    assert!(!t.unsupported, "{:?}", t.diagnostics);
    assert_eq!(t.host_sheets_claimed, Some(2));
    assert_eq!(p_size(&t), 12, "later sheet wins in document order");
    // At 1072 all three are Screen-fit: the two-slot ceiling now refuses.
    let t = run(
        html,
        &[
            "p{font-size:10px}",
            "p{font-size:11px}",
            "p{font-size:12px}",
        ],
        Some(1072),
    );
    assert!(t.unsupported);
    assert_eq!(t.diagnostics.count(RefusalReason::StylesheetCount), 1);
    assert_eq!(t.host_sheets_claimed, None);
}

#[test]
fn link_media_unknown_refuses_before_supply_and_known_true_maps_in_order() {
    let html = "<!doctype html>\
        <link rel=stylesheet media='screen and (min-width:768px)' href=/a>\
        <link rel=stylesheet media='(prefers-color-scheme: dark)' href=/b><p>x</p>";
    let t = run(
        html,
        &["p{font-size:11px}", "p{font-size:12px}"],
        Some(1072),
    );
    assert!(t.unsupported);
    assert_eq!(media_refused(&t), 1);
    // The refused link claims nothing, the other claims host index 0, and the
    // handshake does not fire on a refused walk (no invented mapping).
    assert_eq!(t.diagnostics.count(RefusalReason::StylesheetCount), 0);
    assert_eq!(t.host_sheets_claimed, None);
}

#[test]
fn handshake_exact_count_missing_extra_and_skipped_inputs() {
    let one = "<!doctype html><link rel=stylesheet href=/a><p>x</p>";
    let ok = run(one, &["p{font-size:11px}"], Some(1072));
    assert!(!ok.unsupported);
    assert_eq!(ok.host_sheets_claimed, Some(1));
    // Extra sheet no link claims: refused, was silently accepted before.
    let extra = run(one, &["p{font-size:11px}", "p{font-size:12px}"], Some(1072));
    assert!(extra.unsupported);
    assert_eq!(extra.diagnostics.count(RefusalReason::StylesheetCount), 1);
    // Missing sheet: refused by the walk (existing behavior), no extra record.
    let missing = run(one, &[], Some(1072));
    assert_eq!(
        missing.diagnostics.count(RefusalReason::MissingStylesheet),
        1
    );
    assert_eq!(missing.diagnostics.count(RefusalReason::StylesheetCount), 0);
    // Print, disabled and non-stylesheet links claim nothing.
    let skipped = "<!doctype html><link rel=stylesheet media=print href=/p>\
        <link rel=stylesheet disabled href=/d><link rel=icon href=/i><p>x</p>";
    assert!(!run(skipped, &[], Some(1072)).unsupported);
    let t = run(skipped, &["p{font-size:11px}"], Some(1072));
    assert!(t.unsupported);
    assert_eq!(t.diagnostics.count(RefusalReason::StylesheetCount), 1);
    // Duplicate links are not deduplicated: each needs its own entry.
    let dup = "<!doctype html><link rel=stylesheet href=/a><link rel=stylesheet href=/a><p>x</p>";
    assert!(!run(dup, &["p{font-size:11px}", "p{font-size:12px}"], Some(1072)).unsupported);
    assert_eq!(
        run(dup, &["p{font-size:11px}"], Some(1072))
            .diagnostics
            .count(RefusalReason::MissingStylesheet),
        1
    );
    // A retained data: link takes a linked slot but no host index.
    let data =
        "<!doctype html><link rel=stylesheet href='data:text/css;base64,cHtmb250LXNpemU6OXB4fQ=='>\
        <link rel=stylesheet href=/a><p>x</p>";
    let t = run(data, &["p{font-size:11px}"], Some(1072));
    assert!(!t.unsupported, "{:?}", t.diagnostics);
    assert_eq!(t.host_sheets_claimed, Some(1));
    assert!(
        run(
            data,
            &["p{font-size:11px}", "p{font-size:12px}"],
            Some(1072)
        )
        .unsupported
    );
}

#[test]
fn handshake_does_not_fire_on_truncated_or_partially_refused_inputs() {
    // Truncated input: partial link census, not an inventory.
    let html = "<!doctype html><link rel=stylesheet href=/a><p>x</p>";
    let limits = Limits {
        max_input_bytes: 40,
        ..Limits::DEFAULT
    };
    let sheets = vec![b"p{}".to_vec(), b"p{}".to_vec()];
    let t = parse_style_tree_with_media(
        html.as_bytes(),
        &sheets,
        &limits,
        MediaContext::new(1072).unwrap(),
    );
    assert_eq!(t.diagnostics.count(RefusalReason::InputBytes), 1);
    assert_eq!(t.diagnostics.count(RefusalReason::StylesheetCount), 0);
    // Existing partial refusal (unknown link media) plus extra supply keeps its
    // original diagnostics unchanged: no invented mapping, no new record.
    let partial = "<!doctype html><link rel=stylesheet media=speech href=/a><p>x</p>";
    let t = run(partial, &["p{}", "p{}"], Some(1072));
    assert_eq!(media_refused(&t), 1);
    assert_eq!(t.diagnostics.count(RefusalReason::StylesheetCount), 0);
    // A content-level refusal also leaves the handshake out.
    let content = "<!doctype html><link rel=stylesheet href=/a><p>x</p>";
    let t = run(content, &["p{unknown-prop:1}", "p{}"], Some(1072));
    assert!(t.unsupported);
    assert_eq!(t.diagnostics.count(RefusalReason::StylesheetCount), 0);
}

#[test]
fn style_elements_and_links_share_the_same_classifier() {
    for m in [
        "screen, print",
        "speech",
        "(prefers-color-scheme: dark)",
        "screen and (min-width:768px)",
    ] {
        let style = run(&style_page(m), &[], Some(1072));
        let link = run(
            &format!("<!doctype html><link rel=stylesheet media=\"{m}\" href=/a><p>x</p>"),
            &["p{font-size:20px}"],
            Some(1072),
        );
        assert_eq!(media_refused(&style), media_refused(&link), "{m}");
        assert_eq!(style.unsupported, link.unsupported, "{m}");
        if !style.unsupported {
            assert_eq!(p_size(&style), p_size(&link), "{m}");
        }
    }
}
