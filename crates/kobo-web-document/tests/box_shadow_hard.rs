//! PRIVATE candidate FL26 (ext entry only): ONE hard outer `box-shadow` (blur 0, no inset, spread >= 0, explicit opaque colour).
//! Provenance of the expected rects: for each fixture below the engine's Command::Fill list was rasterised in painter order over
//! white (private-evidence ul1/sh26/cmp.py) and compared with a Chrome 156 headless screenshot at 800x600, device scale 1:
//! 0 differing pixels in all 12 fixtures (constructed diagnostics, ext entry, MediaContext default, synthetic, text-free). This
//! test compares engine Fill rects with the recorded rects; it holds no stored pixels and no Chrome in the loop.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::{paint_backgrounds, paint_backgrounds_float_kernel},
    css_paint_bridge::{paint_content_rectangles, paint_padding_rectangles, FillSpec},
    display_list::{Command, Rgb},
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
};
type F = (i32, i32, u32, u32, u32);
fn tree(html: &str) -> Option<BoxTree> {
    let t = parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return None;
    }
    let bt = BoxTree::from_style(&t);
    (!bt.unsupported).then_some(bt)
}
fn fills(html: &str) -> Option<Vec<F>> {
    let l = paint_backgrounds(&tree(html)?, 800, 600).ok()?;
    Some(
        l.commands()
            .iter()
            .filter_map(|c| match c {
                Command::Fill { rect, color: Rgb(r, g, b), .. } => Some((rect.x, rect.y, rect.width, rect.height, (u32::from(*r) << 16) | (u32::from(*g) << 8) | u32::from(*b))),
                _ => None,
            })
            .collect(),
    )
}
const PRE: &str = "<!doctype html><style>body{margin:0}</style><body>";
fn one(style: &str) -> String {
    format!("{PRE}<div style='height:20px;width:80px;margin:20px 0 0 20px;background:#fff;{style}'></div>")
}
const CHROME_VERIFIED: &[(&str, &str, &[F])] = &[
    ("a1", r####"<!doctype html><style>body{margin:0}</style><body><div style="height:20px;background:#0a0"></div><div style="height:30px;margin:0 0 0 40px;width:100px;box-shadow:10px 12px 0 0 #000"></div><div style="height:25px;background:#00f;width:60px;margin-left:60px"></div><div style="height:15px;width:80px;background:#ccc;box-shadow:-20px -10px 0 4px #f00"></div>"####, &[(0, 0, 800, 20, 0x00aa00), (50, 50, 100, 12, 0x000000), (140, 32, 10, 18, 0x000000), (60, 50, 60, 25, 0x0000ff), (-24, 61, 88, 14, 0xff0000), (-24, 75, 24, 9, 0xff0000), (0, 75, 80, 15, 0xcccccc)]),
    ("a2", r####"<!doctype html><style>body{margin:0}</style><body><div style="height:40px;width:200px;background:#ddd;margin:20px 0 0 30px;box-shadow:0 0 0 6px #333"></div><div style="height:40px;width:200px;margin:20px 0 0 30px;box-shadow:8px 8px 0 0 #a0a"></div><div style="height:30px;width:100px;margin:10px 0 0 50px;background:#ffc"></div>"####, &[(24, 14, 212, 6, 0x333333), (24, 60, 212, 6, 0x333333), (24, 20, 6, 40, 0x333333), (230, 20, 6, 40, 0x333333), (30, 20, 200, 40, 0xdddddd), (38, 120, 200, 8, 0xaa00aa), (230, 88, 8, 32, 0xaa00aa), (50, 130, 100, 30, 0xffffcc)]),
    ("b1", r####"<!doctype html><style>body{margin:0}</style><body><div style="width:100px;height:40px;padding:10px 15px 5px 20px;border:3px solid #123;margin:30px 0 0 40px;box-shadow:12px 9px 0 2px #c60"></div><div style="width:50px;height:20px;padding:4px;border:2px solid #555;border-width:2px 6px 1px 3px;margin:30px 0 0 40px;background:#eee;box-shadow:-9px 7px 0 0 #090"></div>"####, &[(50, 91, 145, 11, 0xcc6600), (181, 37, 14, 54, 0xcc6600), (40, 30, 141, 3, 0x112233), (40, 88, 141, 3, 0x112233), (40, 33, 3, 55, 0x112233), (178, 33, 3, 55, 0x112233), (31, 152, 67, 7, 0x009900), (31, 128, 9, 24, 0x009900), (40, 121, 67, 31, 0xeeeeee), (40, 121, 67, 2, 0x555555), (40, 151, 67, 1, 0x555555), (40, 123, 3, 28, 0x555555), (101, 123, 6, 28, 0x555555)]),
    ("c4", r####"<!doctype html><style>body{margin:0}main{width:60px;height:40px;background:#0a0;margin:20px 0 0 20px}.p{position:absolute;top:30px;left:40px;width:30px;height:20px;background:#fff;box-shadow:20px 10px 0 0 #000}.q{position:absolute;top:45px;left:75px;width:30px;height:30px;background:#00f}.r{position:absolute;top:20px;left:200px;width:40px;height:30px;box-shadow:10px 10px 0 5px #800}section{height:30px;width:100px;margin-left:10px;background:#ccc}</style><main></main><div class=p></div><div class=q></div><div class=r></div><section></section>"####, &[(20, 20, 60, 40, 0x00aa00), (10, 60, 100, 30, 0xcccccc), (60, 50, 30, 10, 0x000000), (70, 40, 20, 10, 0x000000), (40, 30, 30, 20, 0xffffff), (75, 45, 30, 30, 0x0000ff), (205, 50, 50, 15, 0x880000), (240, 25, 15, 25, 0x880000)]),
    ("d1", r####"<!doctype html><style>body{margin:0}</style><body><div style="display:flex;margin:20px 0 0 20px"><div style="width:60px;height:40px;background:#0a0;box-shadow:14px 14px 0 0 #000"></div><div style="width:60px;height:40px;background:#ccc"></div><div style="width:60px;height:30px;box-shadow:0 8px 0 0 #00f"></div></div>"####, &[(34, 60, 60, 14, 0x000000), (80, 34, 14, 26, 0x000000), (20, 20, 60, 40, 0x00aa00), (80, 20, 60, 40, 0xcccccc), (140, 50, 60, 8, 0x0000ff)]),
    ("f1", r####"<!doctype html><style>body{margin:0}</style><body><div style="margin:5px 0 0 5px;width:50px;height:30px;background:#fff;box-shadow:-20px -20px 0 0 #000"></div><div style="margin:40px 0 0 700px;width:80px;height:30px;box-shadow:30px 590px 0 20px #444"></div>"####, &[(-15, -15, 50, 20, 0x000000), (-15, 5, 20, 10, 0x000000), (5, 5, 50, 30, 0xffffff), (710, 645, 120, 70, 0x444444)]),
    ("g1", r####"<!doctype html><style>body{margin:0}</style><body><div style="width:300px;padding:10px;background:#cde;margin:20px 0 0 20px"><div style="height:20px;background:#fff;box-shadow:6px 6px 0 0 #000"></div><div style="height:20px;margin-top:12px;background:#eee;box-shadow:-6px -6px 0 0 #f00"></div></div>"####, &[(20, 20, 320, 72, 0xccddee), (36, 50, 300, 6, 0x000000), (330, 36, 6, 14, 0x000000), (30, 30, 300, 20, 0xffffff), (24, 56, 300, 6, 0xff0000), (24, 62, 6, 14, 0xff0000), (30, 62, 300, 20, 0xeeeeee)]),
    ("h1", r####"<!doctype html><style>body{margin:0}</style><body><div style="height:30px;width:200px;margin:20px;background:#fff;box-shadow:0 0 0 0 #000"></div><div style="height:30px;width:200px;margin:20px;background:#fff;box-shadow:rgb(10,200,30) 4px 4px"></div><div style="height:30px;width:200px;margin:20px;background:#fff;box-shadow:5px 5px #123456"></div>"####, &[(20, 20, 200, 30, 0xffffff), (24, 100, 200, 4, 0x0ac81e), (220, 74, 4, 26, 0x0ac81e), (20, 70, 200, 30, 0xffffff), (25, 150, 200, 5, 0x123456), (220, 125, 5, 25, 0x123456), (20, 120, 200, 30, 0xffffff)]),
    ("i1", r####"<!doctype html><style>body{margin:0}.pa{width:100px;height:30px;margin:20px 0 0 20px;background:#fff;box-shadow:8px 8px 0 0 #000}.ch{height:10px;margin:6px;background:#eee;box-shadow:inherit}</style><div class=pa><div class=ch></div></div><div style="height:20px;margin:30px 0 0 20px;width:60px;box-shadow:inherit;background:#ddd"></div>"####, &[(28, 50, 100, 8, 0x000000), (120, 28, 8, 22, 0x000000), (20, 20, 100, 30, 0xffffff), (34, 30, 88, 8, 0x000000), (114, 28, 8, 2, 0x000000), (26, 20, 88, 10, 0xeeeeee), (20, 80, 60, 20, 0xdddddd)]),
    ("i2", r####"<!doctype html><style>body{margin:0}.a{height:20px;width:80px;margin:20px 0 0 20px;background:#fff}</style><div class=a style="box-shadow:5px 5px 0 0 #000;box-shadow:none"></div><div class=a style="box-shadow:none;box-shadow:5px 5px 0 0 #f00"></div><div class=a style="box-shadow:5px 5px 0 0 #00f;box-shadow:initial"></div><div class=a style="box-shadow:5px 5px 0 0 #0a0;box-shadow:unset"></div>"####, &[(20, 20, 80, 20, 0xffffff), (25, 80, 80, 5, 0xff0000), (100, 65, 5, 15, 0xff0000), (20, 60, 80, 20, 0xffffff), (20, 100, 80, 20, 0xffffff), (20, 140, 80, 20, 0xffffff)]),
    ("i3", r####"<!doctype html><style>body{margin:0}.a{height:20px;width:80px;margin:20px 0 0 20px;background:#fff}.x{box-shadow:6px 6px 0 0 #f00}#y{box-shadow:none}.z{box-shadow:7px 7px 0 0 #00f !important}</style><div class="a x"></div><div class="a x" id=y></div><div class="a z" style="box-shadow:none"></div><div class="a x" style="box-shadow:3px 3px 0 0 #0a0"></div>"####, &[(26, 40, 80, 6, 0xff0000), (100, 26, 6, 14, 0xff0000), (20, 20, 80, 20, 0xffffff), (20, 60, 80, 20, 0xffffff), (27, 120, 80, 7, 0x0000ff), (100, 107, 7, 13, 0x0000ff), (20, 100, 80, 20, 0xffffff), (23, 160, 80, 3, 0x00aa00), (100, 143, 3, 17, 0x00aa00), (20, 140, 80, 20, 0xffffff)]),
    ("i4", r####"<!doctype html><style>body{margin:0}.a{--s:6px 6px 0 0 #f00;height:20px;width:80px;margin:20px 0 0 20px;background:#fff}</style><div class=a style="box-shadow:var(--s)"></div><div class=a style="box-shadow:var(--missing,4px 4px 0 0 #00f)"></div><div class=a style="--t:none;box-shadow:var(--t)"></div>"####, &[(26, 40, 80, 6, 0xff0000), (100, 26, 6, 14, 0xff0000), (20, 20, 80, 20, 0xffffff), (24, 80, 80, 4, 0x0000ff), (100, 64, 4, 16, 0x0000ff), (20, 60, 80, 20, 0xffffff), (20, 100, 80, 20, 0xffffff)]),
];
#[test]
fn chrome_pixel_verified_fixtures_match_recorded_rects() {
    for (name, html, expected) in CHROME_VERIFIED {
        assert_eq!(fills(html).as_deref(), Some(*expected), "{name}");
    }
    // the shadow shapes are really present (anchors against an all-equal-empty comparison)
    assert!(CHROME_VERIFIED.iter().all(|(_, _, e)| e.len() >= 4));
}
#[test]
fn shadow_is_drawn_only_outside_the_border_box() {
    // transparent box, ring shadow (a2 first box): nothing painted inside the box, four outer bands
    let f = fills(&format!("{PRE}<div style='height:40px;width:200px;margin:20px 0 0 30px;box-shadow:0 0 0 6px #333'></div>")).unwrap();
    assert_eq!(f, vec![(24, 14, 212, 6, 0x333333), (24, 60, 212, 6, 0x333333), (24, 20, 6, 40, 0x333333), (230, 20, 6, 40, 0x333333)]);
    // an offset that leaves the shadow entirely disjoint from the box is one rect
    let f = fills(&one("box-shadow:200px 0 0 0 #123")).unwrap();
    assert_eq!(f[0], (220, 20, 80, 20, 0x112233));
}
#[test]
fn none_is_a_typed_reset_and_default_entry_is_unchanged() {
    assert_eq!(fills(&one("box-shadow:5px 5px 0 0 #000;box-shadow:none")).unwrap().len(), 1);
    assert_eq!(fills(&one("box-shadow:none;box-shadow:5px 5px 0 0 #000")).unwrap().len(), 3);
    // default entry (no ext opt-in): BOTH spellings are refused as an unknown property; the arm and the old none-skip are ext-only
    for v in ["5px 5px 0 0 #000", "none"] {
        let h = format!("<!doctype html><div style='height:10px;box-shadow:{v}'></div>");
        assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported, "{v}");
    }
    let plain = "<!doctype html><div style='height:10px'></div>";
    assert!(!parse_style_tree(plain.as_bytes(), &[], &Limits::DEFAULT).unsupported, "anchor: the default entry admits the same page without box-shadow");
}
#[test]
fn unmodelled_shapes_refuse_at_style() {
    for v in [
        "5px 5px 3px 0 #000",           // blur
        "5px 5px 0 -1px #000",          // negative spread
        "inset 5px 5px 0 0 #000",       // inset
        "5px 5px 0 0 #000 inset",
        "5px 5px 0 0 #000, 1px 1px 0 0 #fff", // list
        "5px 5px 0 0",                  // omitted colour (currentcolor)
        "5px 5px 0 0 currentcolor",
        "5px 5px 0 0 transparent",
        "5px 5px 0 0 rgba(0,0,0,0.5)",
        "5px 5px 0 0 #0008",
        "5px 5px 0 0 red blue",
        "5px 5px 0 #000 3px",           // colour between lengths
        "5em 5px 0 0 #000",
        "50% 5px 0 0 #000",
        "5.5px 5px 0 0 #000",
        "5px #000",                     // one length
        "5px 5px 0 0 0 #000",           // five lengths
        "+5px 5px 0 0 #000",
        "5px 5px 0 0 #000 !important x",
        "20000px 5px 0 0 #000",
        "calc(1px) 5px 0 0 #000",
    ] {
        assert_eq!(fills(&one(&format!("box-shadow:{v}"))), None, "{v}");
    }
}
#[test]
fn global_keyword_revert_is_no_shadow() {
    // `revert` is handled by the shared keyword path (UA value: none); it paints no shadow.
    assert_eq!(fills(&one("box-shadow:5px 5px 0 0 #000;box-shadow:revert")).unwrap().len(), 1);
}
#[test]
fn admitted_spellings() {
    let base = fills(&one("box-shadow:4px 6px 0 2px #123456")).unwrap();
    for v in ["#123456 4px 6px 0 2px", "4px 6px 0px 2px rgb(18,52,86)", "4px 6px 0 2px #123456", "4px 6px 0px 2px #123456", "4px 6px 0 2px rgb(18, 52, 86)"] {
        assert_eq!(fills(&one(&format!("box-shadow:{v}"))).as_ref(), Some(&base), "{v}");
    }
    assert_eq!(fills(&one("box-shadow:4px 6px #123456")), fills(&one("box-shadow:4px 6px 0 0 #123456")));
    assert_eq!(fills(&one("box-shadow:-0px 0 #123456")), fills(&one("box-shadow:0 0 #123456")));
}
#[test]
fn paint_refusals() {
    let sh = "box-shadow:5px 5px 0 0 #000";
    // root, body, below overflow:hidden, zero-size, text page, other painters
    assert_eq!(fills(&format!("<!doctype html><style>html{{{sh}}}body{{margin:0}}</style><div style='height:10px'></div>")), None);
    assert_eq!(fills(&format!("<!doctype html><style>body{{margin:0;{sh}}}</style><div style='height:10px'></div>")), None);
    let hid = format!("{PRE}<section style='overflow:hidden'><article style='height:20px;{sh}'></article></section>");
    assert_eq!(fills(&hid), None);
    let hid2 = format!("{PRE}<section style='overflow:hidden;{sh}'><article style='height:20px'></article></section>");
    assert!(fills(&hid2).is_some(), "the hidden box's OWN shadow is outside its clip, so it is not clipped");
    assert_eq!(fills(&format!("{PRE}<div style='height:0;width:50px;{sh}'></div>")), None);
    assert_eq!(fills(&format!("{PRE}<div style='height:10px;width:0;{sh}'></div>")), None);
    assert_eq!(fills(&format!("{PRE}<div style='height:10px;{sh}'>text</div>")), None);
    let t = tree(&one(sh)).unwrap();
    assert!(paint_backgrounds_float_kernel(&t, 800, 600).is_err());
    let spec = [FillSpec { box_index: 2, color: Rgb(1, 2, 3) }];
    let rect_painters = |t: &BoxTree| {
        let w = kobo_web_document::box_tree::WidthPass::from_boxes(t, 800);
        let v = kobo_web_document::box_tree::VerticalPass::from_boxes(t, 800, 600);
        (paint_padding_rectangles(t, &w, &v, 800, 600, &spec).is_ok(), paint_content_rectangles(t, &w, &v, 800, 600, &spec).is_ok())
    };
    // anchor: the same page without a shadow is painted by both rectangle primitives; with a shadow both refuse
    assert_eq!(rect_painters(&tree(&one("")).unwrap()), (true, true));
    assert_eq!(rect_painters(&t), (false, false));
}
struct Face;
impl kobo_web_document::css_text_page::FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(2) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(10) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(7) }
    fn raster(&self, ch: char, _: u32, _: bool) -> Option<kobo_web_document::inline_lines::GlyphBitmap> {
        Some(kobo_web_document::inline_lines::GlyphBitmap { left: 0, top: -3, width: u32::from(ch != ' '), height: if ch == ' ' { 0 } else { 2 }, coverage: if ch == ' ' { vec![] } else { vec![255; 2] } })
    }
}
#[test]
fn text_pages_refuse_a_shadowed_box_with_stub_font_anchors() {
    use kobo_web_document::css_text_page::{paint_direct_text_blocks, paint_single_text_page};
    let page = |style: &str| format!("<!doctype html><style>body{{margin:0}}div{{margin:0;padding:0}}</style><div><section style='{style}'>ab</section></div>");
    let (plain, shadowed) = (tree(&page("")).unwrap(), tree(&page("box-shadow:3px 3px 0 0 #000")).unwrap());
    assert!(paint_direct_text_blocks(&plain, 60, 100, &Face).is_ok(), "anchor: the unshadowed page paints");
    assert!(paint_direct_text_blocks(&shadowed, 60, 100, &Face).is_err());
    let single = |style: &str| tree(&format!("<!doctype html><style>body{{margin:0}}</style><div style='{style}'>ab</div>")).unwrap();
    assert!(paint_single_text_page(&single(""), 60, 100, &Face).is_ok(), "anchor: single text page paints");
    assert!(paint_single_text_page(&single("box-shadow:3px 3px 0 0 #000"), 60, 100, &Face).is_err());
}
#[test]
fn important_and_whitespace_spellings_equal_the_bare_value() {
    let base = fills(&one("box-shadow:4px 6px 0 2px #123456")).unwrap();
    assert_eq!(base.len(), 3, "anchor: box fill plus two outer bands");
    for v in ["box-shadow:4px 6px 0 2px #123456 !important", "box-shadow:4px 6px 0 2px #123456!important", "box-shadow:  4px   6px 0 2px   #123456  ", "box-shadow:4px 6px 0 2px #123456 ;", "box-shadow:\t4px\t6px 0 2px #123456", "BOX-SHADOW:4px 6px 0 2px #123456", "box-shadow:4PX 6px 0 2px #123456"] {
        assert_eq!(fills(&one(v)).as_ref(), Some(&base), "{v:?}");
    }
    let upper = fills(&one("box-shadow:4px 6px 0 2px #ABCDEF")).unwrap();
    assert_eq!(upper[1].4, 0xabcdef);
    // `! important` (valid CSS) is refused by the shared declaration reader: conservative over-refusal, not a Chrome claim
    assert_eq!(fills(&one("box-shadow:4px 6px 0 2px #123456 ! important")), None);
}
