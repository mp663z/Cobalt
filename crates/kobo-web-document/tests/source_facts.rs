use std::fmt::Write;

use kobo_web_document::{
    parse_document, parse_styled_document,
    source_facts::{ParsedQuirks, ParsedSourceFacts, SourceProfile, PASSIVE_HTML_ALLOWLIST},
    Document, Limits, Url, Warning,
};

fn parse(html: &[u8], limits: &Limits) -> Document {
    parse_document(
        html,
        &Url::parse("https://example.test/page").unwrap(),
        limits,
    )
}
fn facts(html: &str) -> ParsedSourceFacts {
    parse(html.as_bytes(), &Limits::DEFAULT)
        .source_facts
        .unwrap()
}
fn total(p: &SourceProfile) -> usize {
    p.scripts
        + p.forms
        + p.links
        + p.details_summary
        + p.image_maps
        + p.frames
        + p.dialogs
        + p.images
        + p.media
        + p.svg_math
        + p.foreign_other
        + p.unknown_custom
        + p.head_affecting
        + p.template
        + p.interaction_attrs
}

#[test]
fn every_profile_tag_in_retained_contexts_refuses_passive_classification() {
    // 35 tags x 7 contexts. HTML tree repair/raw-text rules may discard or
    // reinterpret a token; that is not a raw-tag counter guarantee. Each
    // context checks repeated-parse stability and independent refusal evidence;
    // exact class counts are tested separately, not by regex over source.
    let tags = [
        "script", "form", "input", "button", "select", "textarea", "datalist", "a", "area",
        "details", "summary", "map", "iframe", "frame", "frameset", "object", "embed", "applet",
        "noembed", "noframes", "dialog", "img", "picture", "source", "video", "audio", "track",
        "canvas", "svg", "math", "x-custom", "link", "meta", "style", "template",
    ];
    let wrappers = [
        ("<div>", "</div>"),
        ("<span>", "</span>"),
        ("<head>", "</head>"),
        ("<template>", "</template>"),
        ("<form>", "</form>"),
        ("<div hidden>", "</div>"),
        ("<object>", "</object>"),
    ];
    for tag in tags {
        for (open, close) in wrappers {
            let html = format!("<!doctype html>{open}<{tag}></{tag}>{close}");
            let a = facts(&html);
            let b = facts(&html);
            assert_eq!(a, b, "{tag} {open}");
            assert!(a.scan_completed);
            // Even discarded frames must not be promoted by inventory alone:
            // parse repair errors remain separate admission evidence.
            assert!(total(&a.profile) > 0 || a.parse_errors > 0, "{html}");
        }
    }
}

#[test]
fn exact_profile_classes_and_nested_counts() {
    let f = facts("<!doctype html><details><summary>x</summary><input><a></a><a href='bad:'></a><map><area></map><script></script><dialog></dialog></details>");
    assert_eq!(f.profile.details_summary, 2);
    assert_eq!(f.profile.forms, 1);
    assert_eq!(f.profile.links, 3);
    assert_eq!(f.profile.image_maps, 1);
    assert_eq!(f.profile.scripts, 1);
    assert_eq!(f.profile.dialogs, 1);
    assert_eq!(f.profile.unknown_custom, 0);
    let f = facts("<!doctype html><template><script></script><img><template><input></template></template><div hidden><script></script><a></a></div><object><script></script></object>");
    assert_eq!(f.profile.scripts, 3);
    assert_eq!(f.profile.template, 2);
    assert_eq!(f.profile.images, 1);
    assert_eq!(f.profile.forms, 1);
    assert_eq!(f.profile.links, 1);
}

#[test]
fn interaction_attributes_are_per_element_not_per_attribute() {
    let f = facts("<!doctype html><div hidden tabindex='0' onclick='' contenteditable><span onfocus=''></span></div>");
    assert_eq!(f.profile.interaction_attrs, 2);
    assert_eq!(
        facts("<!doctype html><a hidden></a><a href=''></a><a href='bad:'></a>")
            .profile
            .links,
        3
    );
}

#[test]
fn foreign_and_unknown_names_are_not_passive() {
    let f = facts("<!doctype html><svg xml:lang='en' xmlns:xlink='http://www.w3.org/1999/xlink'><a xlink:href='x'/></svg><math><mi>x</mi></math><custom-thing></custom-thing>");
    assert_eq!(f.profile.svg_math, 4);
    assert_eq!(f.profile.unknown_custom, 1);
    assert!(f.attr_namespace_discarded);
    assert!(f.source_loss_free); // Namespace safety is a separate conjunct.
}

#[test]
fn parsed_merge_losses_and_dedupe_are_distinct() {
    let attrs = (0..32).fold(String::new(), |mut attrs, n| {
        write!(attrs, " a{n}='v'").unwrap();
        attrs
    });
    let full = format!(
        "<!doctype html><html{attrs}><body><html a0='{}' fresh='v' another='v'><p>x",
        "x".repeat(2049)
    );
    let f = facts(&full);
    assert_eq!(f.merged_attributes_dropped, 2);
    assert!(!f.source_loss_free);
    let f = facts(&format!(
        "<!doctype html><html{attrs}><body><html a0='{}'><p>x",
        "x".repeat(2049)
    ));
    assert_eq!(f.merged_attributes_dropped, 0);
    assert!(f.source_loss_free);
    let f = facts(&format!(
        "<!doctype html><body><body fresh='{}'>",
        "x".repeat(2049)
    ));
    assert_eq!(f.merged_attributes_dropped, 1);
    let f = facts(&format!("<!doctype html><div{attrs} fresh='v'></div>"));
    assert_eq!(f.dropped_attributes, 1);
    assert!(!f.source_loss_free);
}

#[test]
fn input_node_text_and_attribute_limits_are_distinct_loss_evidence() {
    let html = b"<!doctype html><p>abcdef</p>";
    for limits in [
        Limits {
            max_input_bytes: 12,
            ..Limits::DEFAULT
        },
        Limits {
            max_nodes: 1,
            ..Limits::DEFAULT
        },
        Limits {
            max_text_bytes: 2,
            ..Limits::DEFAULT
        },
    ] {
        let f = parse(html, &limits).source_facts.unwrap();
        assert!(f.scan_completed);
        assert!(!f.source_loss_free);
        assert_eq!(f.input_original_bytes, html.len());
    }
    let f = facts(&format!(
        "<!doctype html><p data-long='{}'>x",
        "a".repeat(2049)
    ));
    assert_eq!(f.dropped_attributes, 1);
    assert!(!f.source_loss_free);
}

#[test]
fn utf8_cut_bom_recovery_and_quirks_are_separate() {
    for bytes in [
        b"<!doctype html><p>\xff".as_slice(),
        b"<!doctype html><p>\xe2\x82".as_slice(),
    ] {
        let f = parse(bytes, &Limits::DEFAULT).source_facts.unwrap();
        assert!(f.not_utf8);
        assert!(!f.source_loss_free);
    }
    let f = parse(b"\xef\xbb\xbf<!doctype html><p>x</p>", &Limits::DEFAULT)
        .source_facts
        .unwrap();
    assert!(!f.not_utf8);
    assert!(f.source_loss_free);
    assert_eq!(f.quirks, ParsedQuirks::NoQuirks);
    let f = facts("<p>x");
    assert_eq!(f.quirks, ParsedQuirks::Quirks);
    assert!(f.source_loss_free);
    assert!(f.parse_errors > 0);
}

#[test]
fn conversion_depth_and_block_caps_do_not_impersonate_scan_loss() {
    for limits in [
        Limits {
            max_depth: 1,
            ..Limits::DEFAULT
        },
        Limits {
            max_blocks: 1,
            ..Limits::DEFAULT
        },
    ] {
        let d = parse(
            b"<!doctype html><div><div><p>one</p><p>two</p></div></div>",
            &limits,
        );
        let f = d.source_facts.unwrap();
        assert!(f.scan_completed && f.source_loss_free);
        assert!(
            d.warnings.contains(&Warning::TooDeep) || d.warnings.contains(&Warning::TooManyBlocks)
        );
    }
}

#[test]
fn styled_parse_and_reader_view_preserve_full_source_provenance() {
    let html = b"<!doctype html><style>.gone {display:none}</style><nav>menu</nav><main><h1>Title</h1><p>body</p><a class='gone'>link</a></main><footer>end</footer>";
    let url = Url::parse("https://example.test/page").unwrap();
    let plain = parse_document(html, &url, &Limits::DEFAULT);
    let styled = parse_styled_document(html, &url, &Limits::DEFAULT);
    assert_eq!(plain.source_facts, styled.source_facts);
    let reader = plain
        .reader_view()
        .expect("fixture must filter reader blocks");
    assert_eq!(reader.source_facts, plain.source_facts);
    assert!(reader.blocks.len() < plain.blocks.len());
    assert!(Document::default().source_facts.is_none());
}

#[test]
fn repair_and_finish_counts_are_retained_nodes_not_raw_tags() {
    let f = facts("<!doctype html><table><input><tr><td><a>x</table><b><i>x</b>y</i>");
    assert_eq!(f.profile.forms, 1);
    assert_eq!(f.profile.links, 1);
    assert!(f.scan_completed);
    let f = facts("<!doctype html>");
    assert_eq!(f.actual_arena_nodes, 4); // document + synthesized html/head/body
    let f = facts("<!doctype html><input");
    assert_eq!(f.profile.forms, 0); // unfinished token is not an arena node
    assert!(f.parse_errors > 0);
    let d = parse(
        b"<!doctype html><p>x</p>",
        &Limits {
            max_nodes: 1,
            ..Limits::DEFAULT
        },
    );
    let f = d.source_facts.unwrap();
    assert!(f.node_stop);
    assert!(f.actual_arena_nodes > 1); // feed ceiling is not allocation ceiling
}

#[test]
fn allowlist_is_unique_and_excludes_every_refusing_class() {
    let mut list = PASSIVE_HTML_ALLOWLIST.to_vec();
    list.sort_unstable();
    list.dedup();
    assert_eq!(list.len(), PASSIVE_HTML_ALLOWLIST.len());
    for tag in [
        "script", "input", "form", "a", "area", "map", "details", "summary", "dialog", "iframe",
        "img", "canvas", "meta", "style", "template", "noscript", "x-custom",
    ] {
        assert!(!PASSIVE_HTML_ALLOWLIST.contains(&tag), "{tag}");
    }
}

#[test]
fn head_furniture_and_unknown_known_html_are_explicitly_classified() {
    let f = facts("<!doctype html><head><meta charset='utf-8'><link rel='stylesheet' href='x'><style>p{}</style></head><body><base href='/'><noscript>x</noscript><progress>1</progress>");
    assert_eq!(f.profile.head_affecting, 3);
    assert_eq!(f.profile.unknown_custom, 3); // base/noscript/progress not passive
    assert_eq!(f.profile.template, 0);
}

#[test]
fn frames_and_images_and_media_have_distinct_exact_counts() {
    let f = facts("<!doctype html><iframe></iframe><object></object><embed><applet></applet><noembed></noembed><noframes></noframes><img><picture><source></picture><video><track></video><audio></audio><canvas></canvas>");
    assert_eq!(f.profile.frames, 6);
    assert_eq!(f.profile.images, 3);
    assert_eq!(f.profile.media, 4);
    let f = facts("<!doctype html><frameset><frame></frameset>");
    assert_eq!(f.profile.frames, 2);
}

#[test]
fn final_feed_ceiling_and_finish_created_nodes_are_reported_honestly() {
    let f = parse(
        b"<!doctype html>",
        &Limits {
            max_nodes: 1,
            ..Limits::DEFAULT
        },
    )
    .source_facts
    .unwrap();
    assert!(!f.node_stop); // only document exists when feed returns
    assert_eq!(f.actual_arena_nodes, 4); // finish synthesizes nodes
    assert!(f.source_loss_free); // not a hard allocation budget assertion
    let html = format!("<!doctype html><p>{}</p><input>", "x".repeat(17_000));
    let f = parse(
        html.as_bytes(),
        &Limits {
            max_nodes: 6,
            ..Limits::DEFAULT
        },
    )
    .source_facts
    .unwrap();
    assert!(f.node_stop);
    assert!(!f.source_loss_free);
    assert_eq!(f.profile.forms, 1);
}

#[test]
fn repaired_attribute_merges_preserve_facts_when_no_storage_loss() {
    let a = facts("<!doctype html><html><body tabindex='0'><p>x</p>");
    let b = facts("<!doctype html><html><body><body tabindex='0'><p>x</p>");
    assert_eq!(a.profile, b.profile);
    assert!(a.source_loss_free && b.source_loss_free);
    assert_eq!(a.profile.interaction_attrs, 1);
    assert_eq!(b.merged_attributes_dropped, 0);
    let d = parse(
        format!("<!doctype html><body><body fresh='{}'>", "x".repeat(2049)).as_bytes(),
        &Limits::DEFAULT,
    );
    assert!(d.warnings.contains(&Warning::AttributesDropped));
}

#[test]
fn both_attribute_paths_feed_existing_style_loss_diagnostics() {
    use kobo_web_document::parse_style_tree;
    use kobo_web_document::style_diagnostics::RefusalReason;
    for html in [
        format!("<!doctype html><div fresh='{}'>x</div>", "x".repeat(2049)),
        format!("<!doctype html><body><body fresh='{}'>x", "x".repeat(2049)),
    ] {
        let tree = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        assert_eq!(tree.diagnostics.dropped_attributes, 1);
        assert!(tree.truncated);
        assert_eq!(tree.diagnostics.count(RefusalReason::DroppedAttributes), 1);
        let d = parse(html.as_bytes(), &Limits::DEFAULT);
        assert!(d.warnings.contains(&Warning::AttributesDropped));
    }
    let html = format!(
        "<!doctype html><div fresh='{}'>x</div><body other='{}'>",
        "x".repeat(2049),
        "x".repeat(2049)
    );
    let tree = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    assert_eq!(tree.diagnostics.dropped_attributes, 2);
}

#[test]
fn all_zero_profile_is_not_missing_or_complete_proof_by_itself() {
    let clean = facts("<!doctype html><p>text</p>");
    assert_eq!(total(&clean.profile), 0);
    assert!(clean.scan_completed && clean.source_loss_free);
    assert_eq!(clean.parse_errors, 0);
    assert_eq!(clean.quirks, ParsedQuirks::NoQuirks);
    let lost = parse(
        b"<!doctype html><p>text</p>",
        &Limits {
            max_text_bytes: 0,
            ..Limits::DEFAULT
        },
    )
    .source_facts
    .unwrap();
    assert_eq!(total(&lost.profile), 0);
    assert!(lost.scan_completed);
    assert!(!lost.source_loss_free);
    assert!(lost.dropped_text);
    let repaired = facts("<!doctype html><p></p></bogus>");
    assert_eq!(total(&repaired.profile), 0);
    assert!(repaired.source_loss_free);
    assert!(repaired.parse_errors > 0);
    assert!(Document::default().source_facts.is_none());
}
