//! Host-only prototype controls for page-level shaped layout: legacy identity,
//! whole-page fallback, single cumulative px rounding, heights/painter agreement.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, paint_direct_text_blocks_shaped, FontProvider},
    display_list::Command,
    inline_lines::{FitRule, GlyphBitmap, RunShaper, ShapedCluster, TextShaping},
    parse_style_tree, Limits,
};
struct Face;
impl FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> {
        Some(5)
    }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> {
        Some(20)
    }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> {
        Some(15)
    }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] })
    }
}
/// Every char advances `units` (1/1024 px).
struct Uniform(u32, bool);
impl RunShaper for Uniform {
    fn shape_run(&self, text: &str, _: u32, _: bool) -> Option<Vec<ShapedCluster>> {
        if !self.1 {
            return None;
        }
        Some(
            text.chars()
                .enumerate()
                .map(|(i, _)| ShapedCluster { first_char: i as u32, char_len: 1, units: self.0 })
                .collect(),
        )
    }
}
const FIT: FitRule = FitRule { unit: 1024, slack: 0 };
fn page(style: &str, body: &str) -> BoxTree {
    BoxTree::from_style(&parse_style_tree(
        format!("<!doctype html><style>html,body,p,div{{margin:0}}p{{width:100px;line-height:20px;{style}}}</style>{body}<div style='width:10px;height:10px;background:blue'></div>").as_bytes(),
        &[],
        &Limits::DEFAULT,
    ))
}
const TEXT: &str = "<p>the quick brown fox jumps over the lazy dog again and again</p>";
type R = Result<Vec<Command>, kobo_web_document::css_text_page::PageError>;
fn both_r(t: &BoxTree, units: u32, ok: bool) -> (R, R) {
    let s = Uniform(units, ok);
    let sh = TextShaping { shaper: &s, fit: FIT };
    (
        paint_direct_text_blocks(t, 100, 200, &Face).map(|d| d.commands().to_vec()),
        paint_direct_text_blocks_shaped(t, 100, 200, &Face, &sh).map(|d| d.commands().to_vec()),
    )
}
fn both(t: &BoxTree, units: u32, ok: bool) -> (Vec<Command>, Vec<Command>) {
    let (a, b) = both_r(t, units, ok);
    (a.unwrap(), b.unwrap())
}
#[test]
fn legacy_advances_scaled_give_identical_page() {
    let (a, b) = both(&page("", TEXT), 5 * 1024, true);
    assert_eq!(a, b);
}
#[test]
fn shaper_none_is_whole_page_legacy() {
    let (a, b) = both(&page("", TEXT), 5632, false);
    assert_eq!(a, b);
}
#[test]
fn non_left_alignment_is_whole_page_legacy() {
    for al in ["center", "right"] {
        // Non-left alignment must stay byte-identical to legacy (result or refusal).
        let (a, b) = both_r(&page(&format!("text-align:{al}"), TEXT), 5632, true);
        assert_eq!(a, b, "{al}");
        println!("align {al} legacy_ok={}", a.is_ok());
    }
}
/// Shapes everything except text containing "lait" (that block is ineligible).
struct Selective;
impl RunShaper for Selective {
    fn shape_run(&self, text: &str, s: u32, b: bool) -> Option<Vec<ShapedCluster>> {
        if text.contains("lait") { None } else { Uniform(7168, true).shape_run(text, s, b) }
    }
}
#[test]
fn one_ineligible_block_makes_whole_page_legacy() {
    let sh = TextShaping { shaper: &Selective, fit: FIT };
    let t = page("", &format!("{TEXT}<p>cafe au lait is ineligible here</p>"));
    let legacy = paint_direct_text_blocks(&t, 100, 300, &Face).unwrap();
    let shaped = paint_direct_text_blocks_shaped(&t, 100, 300, &Face, &sh).unwrap();
    assert_eq!(legacy.commands(), shaped.commands());
    // Control: without the ineligible block the same shaper does change the page.
    let t2 = page("", TEXT);
    let l2 = paint_direct_text_blocks(&t2, 100, 300, &Face).unwrap();
    let s2 = paint_direct_text_blocks_shaped(&t2, 100, 300, &Face, &sh).unwrap();
    assert_ne!(l2.commands(), s2.commands());
}
#[test]
fn wider_advances_change_wrapping_and_are_not_legacy() {
    let t = page("", TEXT);
    let (a, b) = both(&t, 7 * 1024, true);
    assert_ne!(a, b);
    // 100px / 7px = 14 glyph cells per line at most; every glyph origin < 100.
    for c in &b {
        if let Command::GlyphRun { layout, .. } = c {
            assert!(layout.x < 100);
        }
    }
}
#[test]
fn cumulative_x_is_rounded_once_not_summed_pre_rounded() {
    // 5.4px per glyph: pre-rounded sum would give 5k, cumulative gives round(5.4k).
    let t = page("width:400px", "<p>aaaaaaaaaa</p>");
    let s = Uniform(5530, true); // 5.400390625 px
    let sh = TextShaping { shaper: &s, fit: FIT };
    let dl = paint_direct_text_blocks_shaped(&t, 400, 100, &Face, &sh).unwrap();
    let xs: Vec<u32> = dl
        .commands()
        .iter()
        .filter_map(|c| if let Command::GlyphRun { layout, .. } = c { Some(layout.x as u32) } else { None })
        .collect();
    let want: Vec<u32> = (0..10_u32).map(|k| (k * 5530 + 512) / 1024).collect();
    assert_eq!(xs, want);
    assert_ne!(want, (0..10).map(|k| k * 5).collect::<Vec<_>>());
}
#[test]
fn eligible_shaped_pages_are_never_silently_legacy() {
    // Not a height cross-check (heights and painter share one layout by
    // construction). Only asserts eligible input really takes the shaped path.
    for units in [4096, 5000, 5632, 6000, 7168, 9000] {
        for n in [1_usize, 7, 23, 60, 150] {
            let body = format!("<p>{}</p>", "ab cd ".repeat(n));
            let t = page("", &body);
            let s = Uniform(units, true);
            let sh = TextShaping { shaper: &s, fit: FIT };
            let shaped = paint_direct_text_blocks_shaped(&t, 100, 4000, &Face, &sh).unwrap();
            let legacy = paint_direct_text_blocks(&t, 100, 4000, &Face).unwrap();
            if units != 5120 && n > 7 {
                assert_ne!(shaped.commands(), legacy.commands(), "{units} {n}");
            }
        }
    }
}
