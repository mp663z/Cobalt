//! Page text never drops a block solely because of its script or coverage.
use kobo_web_document::{
    basic_page::paint_basic_page,
    box_tree::BoxTree,
    css_text_page::FontProvider,
    display_list::{Command, Rgb},
    inline_lines::GlyphBitmap,
    Limits, SupplyBudget,
};
struct Face;
impl FontProvider for Face {
    fn has_glyph(&self, c: char, _: bool) -> Option<bool> {
        Some(c.is_ascii())
    }
    fn advance(&self, _: char, s: u32, _: bool) -> Option<u32> {
        Some(s / 2)
    }
    fn line_height(&self, s: u32, _: bool) -> Option<u32> {
        Some(s + 4)
    }
    fn baseline_offset(&self, s: u32, _: bool) -> Option<i32> {
        i32::try_from(s).ok()
    }
    fn raster(&self, c: char, s: u32, _: bool) -> Option<GlyphBitmap> {
        c.is_ascii().then(|| GlyphBitmap {
            left: 0,
            top: -(s as i32),
            width: s / 2,
            height: s,
            coverage: vec![255; (s * s / 2) as usize],
        })
    }
}
fn paint(
    body: &str,
) -> (
    Vec<Command>,
    kobo_web_document::basic_page::PartialStats,
    i64,
) {
    let html = format!(
        "<!doctype html><style>body{{margin:0;font-size:16px}}{}</style>{body}",
        "p{margin:0}"
    );
    let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    paint_basic_page(&BoxTree::from_style(&styled), 300, 500, &Face).unwrap()
}
#[test]
fn three_scripts_and_symbols_keep_all_text_blocks() {
    let (commands, stats, _) = paint("<p>हिन्दी</p><p>Русский</p><p>日本語</p><p>© € café</p>");
    assert!(stats.skips.is_empty(), "{:?}", stats.skips);
    assert_eq!(stats.text_blocks_painted, 4);
    assert!(commands
        .iter()
        .any(|c| matches!(c,Command::GlyphRun{coverage,..} if coverage.contains(&255))));
    assert!(stats.approximations["missing-glyph-placeholder"] > 0);
}
#[test]
fn indented_link_uses_its_own_color_from_first_glyph() {
    let (commands, stats, _) = paint("<p>black                         <a href='/'>Skip</a></p>");
    assert!(stats.skips.is_empty());
    let colors: Vec<_> = commands
        .iter()
        .filter_map(|c| {
            if let Command::GlyphRun { color, .. } = c {
                Some(*color)
            } else {
                None
            }
        })
        .collect();
    assert!(colors.contains(&Rgb(0, 0, 238)), "{colors:?}");
    let link = commands
        .iter()
        .find_map(|c| {
            if let Command::GlyphRun { color, bounds, .. } = c {
                (*color == Rgb(0, 0, 238)).then_some(bounds)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        link.width, 32,
        "all four link letters belong to the blue run"
    );
}
#[test]
fn mixed_sizes_share_a_baseline_and_preserve_ink() {
    let (commands, _, _) = paint("<p>small<span style='font-size:32px'>BIG</span>end</p>");
    let glyphs: Vec<_> = commands
        .iter()
        .filter_map(|c| {
            if let Command::GlyphRun { bounds, .. } = c {
                Some(bounds)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(glyphs.len(), 3);
    assert_eq!(
        glyphs[0].y + i32::try_from(glyphs[0].height).unwrap(),
        glyphs[1].y + i32::try_from(glyphs[1].height).unwrap()
    );
    assert_eq!(glyphs[1].height, 32);
}
#[test]
fn semantic_pre_keeps_indentation_and_blank_lines() {
    let (commands, stats, _) = paint("<pre>  a\n\nb    c</pre>");
    assert!(stats.skips.is_empty());
    let ys: Vec<_> = commands
        .iter()
        .filter_map(|c| {
            if let Command::GlyphRun { bounds, .. } = c {
                Some(bounds.y)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(ys.len(), 2);
    assert!(ys[1] - ys[0] >= 40);
}
#[test]
fn pre_css_and_ordered_list_markers_survive_intake() {
    let html=b"<!doctype html><style>div{white-space:pre}</style><div> x\n y</div><ol><li>one</li><li>two</li></ol>";
    let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        html,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    let tree = BoxTree::from_style(&styled);
    assert!(tree.boxes.iter().any(|b| b.style.white_space_pre));
    assert_eq!(
        tree.page_list_markers.values().cloned().collect::<Vec<_>>(),
        ["1. ", "2. "]
    );
    assert!(styled.unsupported, "exact painter still refuses pre");
}

#[test]
fn centered_underlines_follow_the_glyph_run() {
    let (commands, _, _) = paint("<p style='text-align:center'><u>abc</u></p>");
    let glyph = commands
        .iter()
        .find_map(|c| {
            if let Command::GlyphRun { bounds, .. } = c {
                Some(bounds)
            } else {
                None
            }
        })
        .unwrap();
    let underline = commands
        .iter()
        .find_map(|c| {
            if let Command::Fill { rect, .. } = c {
                Some(rect)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(glyph.x, underline.x);
    assert_eq!(glyph.width, underline.width);
}

#[test]
fn css_bidi_modes_are_retained_without_enabling_exact_paint() {
    for value in [
        "normal",
        "embed",
        "bidi-override",
        "isolate",
        "isolate-override",
        "plaintext",
    ] {
        let html = format!(
            "<!doctype html><span style='direction:rtl;unicode-bidi:{value}'>abc 123</span>"
        );
        let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
        );
        let box_tree = BoxTree::from_style(&styled);
        if value != "normal" {
            assert!(styled.unsupported);
            assert!(box_tree
                .boxes
                .iter()
                .any(|b| b.style.unicode_bidi
                    != kobo_web_document::computed_style::UnicodeBidi::Normal));
        }
    }
}
