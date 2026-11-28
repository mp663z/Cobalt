//! Pins the inline-chip skip counters: a painted border refuses the chip (counted once per
//! element) and a background on a further inline ancestor is dropped (counted once per element).
use kobo_web_document::{
    basic_page::paint_basic_page,
    box_tree::BoxTree,
    css_text_page::FontProvider,
    display_list::Command,
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
fn skips(body: &str) -> std::collections::BTreeMap<&'static str, usize> {
    paint(body).1.skips
}
fn chip_skips(body: &str) -> (usize, usize) {
    let s = skips(body);
    (
        s.get("inline-background-bordered").copied().unwrap_or(0),
        s.get("inline-background-nested").copied().unwrap_or(0),
    )
}
fn chip_fills(body: &str) -> usize {
    paint(body).0.iter().filter(|c| matches!(c, Command::Fill { .. })).count()
}

#[test]
fn plain_chip_fires_neither_counter_and_paints() {
    let body = "<p>a <span style=\"background:#ffff00\">x</span> b</p>";
    assert_eq!(chip_skips(body), (0, 0));
    assert!(chip_fills(body) >= 1, "the chip is painted");
}

#[test]
fn painted_border_refuses_the_chip_and_counts_bordered_once_per_element() {
    let one = "<p>a <span style=\"background:#ffff00;border:1px solid #000\">x</span> b</p>";
    assert_eq!(chip_skips(one), (1, 0));
    assert_eq!(chip_fills(one), 0, "refused chip paints no fill");
    // Two text leaves under the same bordered element (an inner <b> splits the run) count once.
    let split = "<p>a <span style=\"background:#ffff00;border:1px solid #000\">x <b>y</b> z</span> b</p>";
    assert_eq!(chip_skips(split), (1, 0));
    // Two separate bordered elements count twice.
    let two = "<p><span style=\"background:#ffff00;border:1px solid #000\">x</span> \
               <span style=\"background:#00ffff;border:1px solid #000\">y</span></p>";
    assert_eq!(chip_skips(two), (2, 0));
}

#[test]
fn border_without_background_is_not_a_chip_and_counts_nothing() {
    assert_eq!(
        chip_skips("<p>a <span style=\"border:1px solid #000\">x</span> b</p>"),
        (0, 0)
    );
}

#[test]
fn background_on_a_further_inline_ancestor_counts_nested_once_per_element() {
    let one = "<p><span style=\"background:#ffff00\">a <span style=\"background:#00ffff\">b</span></span></p>";
    assert_eq!(chip_skips(one), (0, 1));
    assert!(chip_fills(one) >= 1, "the nearest chip still paints");
    // Two leaves under the nested inner chip and the same outer ancestor count the outer once.
    let many = "<p><span style=\"background:#ffff00\">a <span style=\"background:#00ffff\">b <b>c</b></span></span></p>";
    assert_eq!(chip_skips(many), (0, 1));
    // An ancestor without a background does not count.
    let none = "<p><span>a <span style=\"background:#00ffff\">b</span></span></p>";
    assert_eq!(chip_skips(none), (0, 0));
}

#[test]
fn bordered_chip_refuses_and_the_outer_background_is_not_counted_nested() {
    let body = "<p><span style=\"background:#ffff00\">a <span style=\"background:#00ffff;border:1px solid #000\">b</span></span></p>";
    assert_eq!(chip_skips(body), (1, 0));
}
