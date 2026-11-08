//! PRIVATE candidate (ext entry only): `white-space: nowrap|normal`. nowrap is admitted only when the nowrap context's
//! text (whitespace collapsed, trimmed) has no ASCII whitespace, i.e. no break opportunity, so it is paint- and
//! geometry-equivalent to normal wrapping. Chrome 156 (private evidence nw1/out.json): one word nowrap/normal both
//! 20px; "aa bb" in a 30px box: nowrap 20 vs normal 40 (differ -> refused here); a long word: both 20.
//! Stub-font text painter; no real-glyph claim.
use kobo_web_document::box_tree::BoxTree;
use kobo_web_document::css_text_page::{paint_direct_text_blocks, FontProvider};
use kobo_web_document::display_list::Rgb;
use kobo_web_document::inline_lines::GlyphBitmap;
use kobo_web_document::{parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
struct Face;
impl FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(5) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(16) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(12) }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> { Some(GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] }) }
}
fn ext(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    let h = format!("<!doctype html><style>body{{margin:0}}.t{{font-size:16px;line-height:16px;background:#eee}}{css}</style>{body}");
    parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn pixels(css: &str, body: &str) -> Option<Vec<u8>> {
    let s = ext(css, body);
    if s.unsupported { return None; }
    let bt = BoxTree::from_style(&s);
    if bt.unsupported { return None; }
    let l = paint_direct_text_blocks(&bt, 100, 60, &Face).ok()?;
    l.rasterize(100, 60, Rgb(255, 255, 255)).ok()
}
fn admitted(css: &str, body: &str) -> bool {
    let s = ext(css, body);
    !s.unsupported && !BoxTree::from_style(&s).unsupported
}
#[test]
fn single_word_nowrap_paints_exactly_like_normal() {
    let bare = pixels("", "<div class=t>Hello</div>").expect("bare");
    for css in [".t{white-space:nowrap}", ".t{white-space:normal}", "body{white-space:nowrap}"] {
        assert_eq!(pixels(css, "<div class=t>Hello</div>").as_deref(), Some(bare.as_slice()), "{css}");
    }
    // surrounding source whitespace is trimmed, not a break opportunity
    let ws = "<div class=t>\n  Hello \n</div>";
    let ws_bare = pixels("", ws).expect("anchor: bare whitespace-padded page admitted");
    assert_ne!(Some(&ws_bare), pixels("", "<div class=t></div>").as_ref(), "anchor: text paints");
    assert_eq!(pixels(".t{white-space:nowrap}", ws), Some(ws_bare));
    // inline pieces without whitespace between them
    let b = "<div class=t>He<b>llo</b></div>";
    let pieces_bare = pixels("", b).expect("anchor: bare inline-pieces page admitted");
    assert_ne!(Some(&pieces_bare), pixels("", "<div class=t></div>").as_ref(), "anchor: pieces paint");
    assert_eq!(pixels(".t{white-space:nowrap}", b), Some(pieces_bare));
    // a too-wide word is refused identically with or without nowrap (engine fails rather than guess)
    let long = "<div class=t style='width:30px'>Helloooooooo</div>";
    assert_eq!(pixels(".t{white-space:nowrap}", long), pixels("", long));
    // anchors so equality cannot be None == None: the word fits a wide box and is admitted in both.
    let wide = "<div class=t style='width:100px'>Helloooooooo</div>";
    let w = pixels("", wide).expect("normal admitted");
    assert_eq!(pixels(".t{white-space:nowrap}", wide), Some(w));
    assert!(pixels("", long).is_none(), "too-wide word is refused by the placer (anchor for the equality above)");
}
#[test]
fn break_opportunities_refuse_because_chrome_differs() {
    for body in [
        "<div class=t style='width:30px'>aa bb</div>",
        "<div class=t>a b</div>",
        "<div class=t>a<b> b</b></div>",
        "<div class=t><b>a</b> <i>b</i></div>",
        "<div class=t>a\tb</div>",
        "<div class=t>a\nb</div>",
        "<div class=t>x<span class=r>a b</span></div>",
        "<div class=t>x<span style='display:inline-block'>y</span></div>",
    ] {
        assert!(!admitted(".t{white-space:nowrap}", body), "{body}");
    }
    // NW1 reviewer counterexamples (Chrome 20px high nowrap vs 40 normal: a nowrap span edge is still a break
    // opportunity in the normal parent). Only the span is nowrap; the container is normal.
    for body in [
        "<div class=t style='width:25px'><span class=n>a </span>b</div>",
        "<div class=t style='width:25px'>a<span class=n> b</span></div>",
        "<div class=t style='width:25px'><span class=n>a </span><span class=n>b</span></div>",
        "<div class=t><span class=n>a</span> b</div>",
        "<div class=t>a <span class=n>b</span></div>",
    ] {
        assert!(!admitted(".n{white-space:nowrap}", body), "{body}");
    }
    // A nowrap span in a context with no break opportunity at all is still fine.
    assert!(admitted(".n{white-space:nowrap}", "<div class=t>a<span class=n>b</span>c</div>"));
    assert!(admitted(".n{white-space:nowrap}", "<div class=t>\n a<span class=n>b</span> \n</div>"));
    // normal / reset: whitespace is fine again
    assert!(admitted(".t{white-space:normal}", "<div class=t>a b</div>"));
    assert!(admitted(".t{white-space:nowrap}.r{white-space:normal}", "<div class=t><span class=r>a b</span></div>"));
    // inherited nowrap reaches the inner multiword text
    assert!(!admitted("body{white-space:nowrap}", "<div class=t>a b</div>"));
    assert!(!admitted(".t{white-space:nowrap}", "<div class=t><span>a b</span></div>"));
    // inherit keyword follows the parent
    assert!(!admitted(".t{white-space:nowrap}.r{white-space:inherit}", "<div class=t><span class=r>a b</span></div>"));
}
#[test]
fn other_values_and_default_entry_refuse() {
    for v in ["pre", "pre-wrap", "pre-line", "break-spaces", "nowrap normal", "wrap", "NOWRAP x", ""] {
        assert!(ext(&format!(".t{{white-space:{v}}}"), "<div class=t>a</div>").unsupported, "{v}");
    }
    let h = "<!doctype html><style>.t{white-space:nowrap}</style><div class=t>Hello</div>";
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
}

/// Reviewer's exact Chrome 156 fixture (font 16 / line 16 / width 10, `.n{white-space:nowrap}`): Chrome heights
/// a=16 (normal 32 for b), c=16 (normal 32 for d), e=16. Each nowrap-span page must refuse here; the same pages
/// without nowrap (b, d) are the normal controls and are not affected by this candidate.
#[test]
fn reviewer_fixture_nowrap_span_edge_pages_refuse_and_normal_controls_do_not_depend_on_nowrap() {
    let css = ".t{width:10px}.n{white-space:nowrap}";
    for body in [
        "<div id=a class=t><span class=n>a </span>b</div>",
        "<div id=c class=t>a<span class=n> b</span></div>",
        "<div id=e class=t><span class=n>a </span><span class=n>b</span></div>",
    ] {
        assert!(!admitted(css, body), "{body}");
    }
    // Same text with the span not nowrap is a normal page: nowrap code is not involved at all (control).
    for body in ["<div id=b class=t><span>a </span>b</div>", "<div id=d class=t>a<span> b</span></div>"] {
        let s = ext(css, body);
        assert!(!s.nodes.iter().any(|n| n.style.white_space_nowrap), "{body}");
    }
}
