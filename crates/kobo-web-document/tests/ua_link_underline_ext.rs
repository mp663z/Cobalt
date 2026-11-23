//! PRIVATE candidate (ext entry only): UA presentation of links. Chrome 156 draws `a[href]` rgb(0,0,238) with an
//! underline and `u`/`ins` underlined; the base modelled neither (an unstyled link was admitted and painted black,
//! no underline). Under ext the UA values are declared: raw links refuse (underline gate) unless the author sets
//! `text-decoration:none`, and the UA colour then applies (author colour wins). Stub-font text painter: layer/colour
//! checks only, no glyph or underline pixel claim. Default entry is UNCHANGED (still admits a bare link).
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
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> { Some(GlyphBitmap { left: 0, top: -1, width: 3, height: 3, coverage: vec![255; 9] }) }
}
fn paint(ext: bool, css: &str, body: &str) -> Option<Vec<u8>> {
    let h = format!("<!doctype html><style>body{{margin:0}}div{{font-size:16px;line-height:16px;width:100px}}{css}</style>{body}");
    let s = if ext { parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL) } else { parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT) };
    if s.unsupported { return None; }
    let bt = BoxTree::from_style(&s);
    if bt.unsupported { return None; }
    paint_direct_text_blocks(&bt, 100, 40, &Face).ok()?.rasterize(100, 40, Rgb(255, 255, 255)).ok()
}
#[test]
fn raw_links_and_underline_elements_refuse_under_ext_but_not_without_href() {
    // Blue container so the link's own UA colour is not itself a refusal reason: the refusal must come from the underline.
    for b in ["<div style='color:#0000ee'>x <a href=u>t</a></div>", "<div style='color:#0000ee'><a href=''>t</a></div>", "<div>x <u>t</u></div>", "<div>x <ins>t</ins></div>"] {
        assert!(paint(true, "", b).is_none(), "ext must refuse {b}");
        assert!(paint(false, "", b).is_some(), "anchor: default entry admits (unfixed base gap) {b}");
    }
    // anchors: the same pages without the UA underline are admitted
    let plain = paint(true, "", "<div>x <a>t</a></div>").expect("a without href has no UA style");
    assert_eq!(Some(&plain), paint(true, "", "<div>x <span>t</span></div>").as_ref());
    assert!(paint(true, "", "<div>x <b>t</b></div>").is_some());
}
#[test]
fn text_decoration_none_turns_off_the_underline_and_ua_colour_applies() {
    // The text painter refuses a colour change inside a line, so colour is checked by putting the container in the
    // colour the link should have: UA blue on the anchor equals the same text in a blue container.
    let blue_div = |inner: &str| format!("<div style='color:#0000ee'>x {inner}</div>");
    let span_blue = paint(true, "", &blue_div("<span>t</span>")).expect("blue container, span");
    let span_black = paint(true, "", "<div>x <span>t</span></div>").expect("black span");
    assert_ne!(span_blue, span_black, "anchor: colour is visible in the raster");
    assert_eq!(paint(true, "a{text-decoration:none}", &blue_div("<a href=u>t</a>")).as_ref(), Some(&span_blue));
    assert_eq!(paint(true, "", &blue_div("<a href=u style='text-decoration:none'>t</a>")).as_ref(), Some(&span_blue));
    // SUPERSEDED v2 (delta10 per-run colour): the UA colour really is blue - in a black container the link now
    // PAINTS blue next to black text (pre-delta10 the painter refused a colour change inside a line; the refusal
    // was a missing-support limitation, noted here as "not an admission", and lives in git history at 3e6cb22).
    // Stronger assertion than the old refusal: the mixed-colour line paints exactly like the same text with an
    // explicitly blue span.
    let link_in_black = paint(true, "a{text-decoration:none}", "<div>x <a href=u>t</a></div>").expect("delta10: UA-blue link paints in a black container");
    let explicit_blue_span = paint(true, "", "<div>x <span style='color:#0000ee'>t</span></div>").expect("delta10: blue span paints in a black container");
    assert_eq!(&link_in_black, &explicit_blue_span, "link paints its own UA blue beside black text");
    assert_ne!(&link_in_black, &span_black, "link is visibly blue, not black");
    // author colour wins over the UA colour
    let red_div = "<div style='color:#ff0000'>x <span>t</span></div>";
    let span_red = paint(true, "", red_div).expect("red");
    assert_eq!(paint(true, "a{color:#ff0000;text-decoration:none}", "<div style='color:#ff0000'>x <a href=u>t</a></div>").as_ref(), Some(&span_red));
    // UA colour is inherited by the link's descendants
    let b_blue = paint(true, "", &blue_div("<b>t</b>")).expect("b blue");
    assert_eq!(paint(true, "a{text-decoration:none}", &blue_div("<a href=u><b>t</b></a>")).as_ref(), Some(&b_blue));
    // u / ins turned off are plain text
    assert_eq!(paint(true, "u,ins{text-decoration:none}", "<div>x <u>t</u></div>").as_ref(), Some(&span_black));
    assert_eq!(paint(true, "u,ins{text-decoration:none}", "<div>x <ins>t</ins></div>").as_ref(), Some(&span_black));
    // an author underline is still refused (only none is admitted)
    assert!(paint(true, "a{text-decoration:underline}", &blue_div("<a href=u>t</a>")).is_none());
    // none on a non-link is a no-op
    assert_eq!(paint(true, "span{text-decoration:none}", "<div>x <span>t</span></div>").as_ref(), Some(&span_black));
}
#[test]
fn other_decoration_forms_refuse_and_default_entry_is_unchanged() {
    // standalone (no UA underline involved): invalid forms refuse on a plain span too
    for d in ["text-decoration:none underline", "text-decoration:underline", "text-decoration:inherit", "text-decoration:none !x", "text-decoration:", "text-decoration:line-through"] {
        assert!(paint(true, &format!("span{{{d}}}"), "<div>x <span>t</span></div>").is_none(), "standalone {d}");
    }
    for d in ["text-decoration:none underline", "text-decoration:underline", "text-decoration:none !x", "text-decoration:inherit", "text-decoration:", "text-decoration-line:none", "text-decoration:line-through"] {
        assert!(paint(true, &format!("a{{{d}}}"), "<div>x <a href=u>t</a></div>").is_none(), "{d}");
    }
    // default entry: text-decoration none still refused; a bare link is still admitted there (unmodelled UA style, disclosed)
    assert!(paint(false, "a{text-decoration:none}", "<div>x <a href=u>t</a></div>").is_none());
    assert!(paint(false, "", "<div>x <a href=u>t</a></div>").is_some());
}
#[test]
fn line_through_elements_refuse_unless_author_none() {
    for b in ["<div>x <s>t</s></div>", "<div>x <strike>t</strike></div>", "<div>x <del>t</del></div>", "<div>x <abbr title=a>t</abbr></div>", "<div>x <acronym title=a>t</acronym></div>"] {
        assert!(paint(true, "", b).is_none(), "ext must refuse {b}");
        assert!(paint(true, "s,strike,del,abbr,acronym{text-decoration:none}", b).is_some(), "author none admits {b}");
        assert!(paint(true, "s,strike,del,abbr,acronym{text-decoration:underline}", b).is_none(), "{b}");
    }
    let plain = paint(true, "", "<div>x <span>t</span></div>").expect("span");
    assert_eq!(paint(true, "del{text-decoration:none}", "<div>x <del>t</del></div>").as_ref(), Some(&plain));
    // default entry unchanged: a bare <del> is still admitted there (pre-existing gap)
    assert!(paint(false, "", "<div>x <del>t</del></div>").is_some());
}
#[test]
fn abbr_without_title_is_plain() {
    let plain = paint(true, "", "<div>x <span>t</span></div>").expect("span");
    assert_eq!(paint(true, "", "<div>x <abbr>t</abbr></div>").as_ref(), Some(&plain));
}
#[test]
fn mark_gets_ua_yellow_background_and_black_text_or_refuses() {
    // Measured: the painter refuses an in-line background/colour change, so a bare <mark> refuses (never plain text).
    let plain = paint(true, "", "<div>x <span>t</span></div>").expect("span");
    let bare = paint(true, "", "<div>x <mark>t</mark></div>");
    assert!(bare.is_none(), "bare mark refuses");
    // author overrides: transparent background + explicit black text (the container is black by default) is the plain-text page
    assert_eq!(paint(true, "mark{background-color:transparent;color:#000000}", "<div>x <mark>t</mark></div>").as_ref(), Some(&plain));
    // default entry unchanged (pre-existing gap)
    assert!(paint(false, "", "<div>x <mark>t</mark></div>").is_some());
}
#[test]
fn mark_computed_style_is_exact_ua_yellow_and_black_even_under_a_red_parent() {
    let st = |css: &str, body: &str| {
        let h = format!("<!doctype html><style>{css}</style>{body}");
        parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
    };
    let find = |t: &kobo_web_document::style_tree::StyleTree, tag: &str| t.nodes.iter().find(|n| n.tag == tag).map(|n| (n.style.background_color, n.style.color));
    let red = st("", "<div style='color:#ff0000'>x <mark>t</mark></div>");
    assert_eq!(find(&red, "mark"), Some((Some(0xff_ff_00), 0x00_00_00)), "UA colour beats the inherited red");
    let plain = st("", "<div style='color:#ff0000'>x <span>t</span></div>");
    assert_eq!(find(&plain, "span"), Some((None, 0xff_00_00)), "anchor: span inherits red");
    // author wins over UA
    let a = st("mark{background-color:#00ff00;color:#0000ff}", "<div>x <mark>t</mark></div>");
    assert_eq!(find(&a, "mark"), Some((Some(0x00_ff_00), 0x00_00_ff)));
    // a block mark with the UA colours can paint (not every mark refuses)
    let blk = |css: &str| paint(true, &format!("mark{{display:block;height:16px;{css}}}"), "<div><mark>t</mark></div>");
    let ua = blk("").expect("block mark paints (UA colours)");
    assert_eq!(Some(&ua), blk("background-color:#ffff00;color:#000000").as_ref(), "UA == explicit yellow/black");
    assert_ne!(Some(&ua), blk("background-color:#fffe00").as_ref(), "off-by-one yellow differs");
    assert_ne!(Some(&ua), blk("background-color:transparent").as_ref(), "background is visible");
    assert_ne!(Some(&ua), blk("color:#000001").as_ref(), "off-by-one text colour differs");
}
