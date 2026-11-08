//! S0 characterisation (evidence only, never an expectation): the test-producer
//! app path (NativeFontAdapter, AtkinsonHyperlegible-Regular, paint_single_text_page,
//! no sheets). Writes glyph layout rects to $S0_OUT when set. Asserts nothing about layout.
use crate::native_font_adapter::NativeFontAdapter;
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::css_text_page::{paint_single_text_page, FontProvider};
use kobo_web_document::display_list::Command;
use kobo_web_document::{parse_style_tree, Limits};

const LINE: &str = "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs.";

#[test]
fn s0_native_default_characterise() {
    let Ok(dir) = std::env::var("S0_OUT") else { return };
    let adapter = NativeFontAdapter::new().expect("session");
    let mut adv = String::new();
    for c in LINE.chars() {
        adv.push_str(&format!("{},{:?}\n", c as u32, adapter.advance(c, 16, false)));
    }
    std::fs::write(format!("{dir}/advances16.txt"), adv).unwrap();
    for (w, tag) in [(600u32, "600"), (320u32, "320")] {
        let html = format!("<!DOCTYPE html><html><head><title>t</title></head><body><p>{LINE}</p></body></html>");
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        let tree = BoxTree::from_style(&styled);
        let res = paint_single_text_page(&tree, w, 800, &adapter);
        let mut out = String::new();
        match res {
            Ok(list) => {
                for cmd in list.commands() {
                    if let Command::GlyphRun { layout, bounds, .. } = cmd {
                        out.push_str(&format!("{},{},{},{},{},{}\n", layout.x, layout.y, layout.width, layout.height, bounds.x, bounds.width));
                    }
                }
            }
            Err(e) => out.push_str(&format!("REFUSED {e:?}\n")),
        }
        std::fs::write(format!("{dir}/layout_{tag}.txt"), out).unwrap();
    }
}

/// Refusal/acceptance map for variants through the SAME entry (evidence only).
#[test]
fn s0_native_variants_characterise() {
    let Ok(dir) = std::env::var("S0_OUT") else { return };
    let adapter = NativeFontAdapter::new().expect("session");
    let variants: [(&str, &str); 7] = [
        ("plain", "<p>Hello world text</p>"),
        ("bold_b", "<p>Hello <b>bold</b> text</p>"),
        ("two_runs_em", "<p>Hello <em>it</em> text</p>"),
        ("two_paragraphs", "<p>One line</p><p>Two line</p>"),
        ("align_right", "<p style='text-align:right'>Hello world text</p>"),
        ("align_center", "<p style='text-align:center'>Hello world text</p>"),
        ("heading", "<h1>Title text</h1>"),
    ];
    let mut out = String::new();
    for (name, body) in variants {
        let html = format!("<!DOCTYPE html><html><head><title>t</title></head><body>{body}</body></html>");
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        let tree = BoxTree::from_style(&styled);
        match paint_single_text_page(&tree, 600, 800, &adapter) {
            Ok(list) => {
                let runs: Vec<_> = list.commands().iter().filter_map(|c| if let Command::GlyphRun { layout, .. } = c { Some((layout.x, layout.y, layout.width, layout.height)) } else { None }).collect();
                let first = runs.first().copied();
                let last = runs.last().copied();
                out.push_str(&format!("{name}: ACCEPTED glyphs={} first={first:?} last={last:?}\n", runs.len()));
            }
            Err(e) => out.push_str(&format!("{name}: REFUSED {e:?}\n")),
        }
    }
    std::fs::write(format!("{dir}/variants.txt"), out).unwrap();
}
