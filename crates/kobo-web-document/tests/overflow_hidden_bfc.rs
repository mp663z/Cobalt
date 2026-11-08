//! PRIVATE candidate FL22 (ext entry only): `overflow:hidden` admitted ONLY as a block formatting context whose clip can never
//! matter (text-free, float-free, static block descendants proven inside the padding box on USED edges). No clip is painted.
//! Expected rects are Chrome 156 values (private-evidence ul1/ov2.json o2/o4/o10 and reviewer ov3 findings); the engine side is
//! Command::Fill rects from paint_backgrounds (800x200), not literal rasterization, no Chrome/stored-pixel comparison.
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Command, parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
type R = (i32, i32, u32, u32);
fn fills_with(css: &str, section_attr: &str, inner: &str, outer_extra: &str) -> Option<Vec<R>> {
    let h = format!("<!doctype html><style>body{{margin:0}}nav{{height:9px;background:#111}}aside{{height:7px;background:#222}}section{{background:#333}}article{{background:#444}}{css}</style><body>{outer_extra}<nav></nav><section {section_attr}>{inner}</section><aside></aside>");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported { return None; }
    let bt = BoxTree::from_style(&t);
    if bt.unsupported { return None; }
    let l = paint_backgrounds(&bt, 800, 200).ok()?;
    Some(l.commands().iter().filter_map(|c| if let Command::Fill { rect, .. } = c { Some((rect.x, rect.y, rect.width, rect.height)) } else { None }).collect())
}
fn fills(section_style: &str, inner: &str) -> Option<Vec<R>> { fills_with("", &format!("style='{section_style}'"), inner, "") }
const MT10: &str = "<article style='height:20px;margin-top:10px'></article>";
const MB10: &str = "<article style='height:20px;margin-bottom:10px'></article>";
const HIDDEN_MT: [R; 4] = [(0, 0, 800, 9), (0, 9, 800, 30), (0, 19, 800, 20), (0, 39, 800, 7)];
const VISIBLE_MT: [R; 4] = [(0, 0, 800, 9), (0, 19, 800, 20), (0, 19, 800, 20), (0, 39, 800, 7)];
#[test]
fn hidden_contains_child_margins_like_chrome() {
    assert_eq!(fills("overflow:hidden", MT10), Some(HIDDEN_MT.to_vec())); // ov2 o2: section @9+30
    assert_eq!(fills("overflow:hidden", MB10), Some(vec![(0, 0, 800, 9), (0, 9, 800, 30), (0, 9, 800, 20), (0, 39, 800, 7)])); // o4
    assert_eq!(fills("overflow:hidden;padding:3px", "<article style='height:20px;margin:4px'></article>"),
        Some(vec![(0, 0, 800, 9), (0, 9, 800, 34), (7, 16, 786, 20), (0, 43, 800, 7)])); // o10: box 34, article @16 w786, aside @43
    // plain (visible) for contrast: the margin escapes (o1)
    assert_eq!(fills("", MT10), Some(VISIBLE_MT.to_vec()));
}
#[test]
fn cascade_visible_reset_order_specificity_important() {
    assert_eq!(fills("overflow:hidden;overflow:visible", MT10), Some(VISIBLE_MT.to_vec()));
    assert_eq!(fills("overflow:visible;overflow:hidden", MT10), Some(HIDDEN_MT.to_vec()));
    assert_eq!(fills_with(".a{overflow:hidden}.a{overflow:visible}", "class=a", MT10, ""), Some(VISIBLE_MT.to_vec()));
    assert_eq!(fills_with("#s{overflow:hidden}.a{overflow:visible}", "id=s class=a", MT10, ""), Some(HIDDEN_MT.to_vec()));
    assert_eq!(fills_with(".a{overflow:hidden !important}", "class=a style='overflow:visible'", MT10, ""), Some(HIDDEN_MT.to_vec()));
    assert_eq!(fills_with(".a{overflow:visible !important}", "class=a style='overflow:hidden'", MT10, ""), Some(VISIBLE_MT.to_vec()));
    assert_eq!(fills_with("section{overflow:hidden}.a{--o:visible;overflow:var(--o)}", "class=a", MT10, ""), Some(VISIBLE_MT.to_vec()));
}
#[test]
fn unproven_or_unmodelled_shapes_refuse() {
    let hid = "overflow:hidden";
    // explicit/two-value/axis forms and other keywords
    // FL33 TEST-EXPECTATION CHANGE (disclosed): `overflow-x:hidden`, `overflow-y:hidden` and `overflow:auto` were refused here and
    // are now typed onto the same state as `overflow:hidden` (see tests/overflow_axes.rs, Chrome-verified); they are asserted
    // equal to the hidden result below.
    for d in ["overflow-x:hidden", "overflow-y:hidden", "overflow:auto"] {
        assert_eq!(fills(d, MT10), fills(hid, MT10), "{d}");
        assert!(fills(d, MT10).is_some(), "{d}");
    }
    for d in ["overflow:hidden hidden", "overflow:hidden visible", "overflow:clip", "overflow:scroll", "overflow-x:visible", "overflow-y:scroll"] {
        assert_eq!(fills(d, MT10), None, "{d}");
    }
    // reviewer hole 2: 780px child with margin-left:50px passes width/margin-sign checks but its used right edge is 830 > 800
    assert_eq!(fills(hid, "<article style='height:20px;width:780px;margin-left:50px'></article>"), None);
    assert_eq!(fills(hid, "<article style='height:20px;width:900px'></article>"), None); // o7
    // fits exactly on used edges: 750 + 50 = 800 is contained (no Chrome number claimed beyond containment)
    assert!(fills(hid, "<article style='height:20px;width:750px;margin-left:50px'></article>").is_some());
    // o8 / explicit height, min-height, empty block (o6: no collapse-through), border-box
    assert_eq!(fills("overflow:hidden;height:15px", "<article style='height:40px'></article>"), None);
    assert_eq!(fills("overflow:hidden;min-height:30px", MT10), None);
    assert_eq!(fills(hid, ""), None);
    assert_eq!(fills("overflow:hidden;box-sizing:border-box", MT10), None);
    // descendants: explicit height with children, percentage height, negative margin, positioned, text, float context
    assert_eq!(fills(hid, "<article style='height:20px'><div style='height:40px'></div></article>"), None);
    assert_eq!(fills(hid, "<article style='height:50%'></article>"), None);
    assert_eq!(fills(hid, "<article style='height:20px;margin-top:-4px'></article>"), None);
    assert_eq!(fills(hid, "<article style='height:20px;position:relative'></article>"), None);
    assert_eq!(fills(hid, "<article style='height:20px;position:absolute'></article>"), None);
    assert_eq!(fills(hid, "text"), None);
    assert_eq!(fills(hid, "<article style='height:20px'>t</article>"), None);
    // reviewer hole 1: float context anywhere on the page refuses (Chrome narrows/shifts the hidden box beside a float)
    assert_eq!(fills_with("", &format!("style='{hid}'"), MT10, "<div style='float:left;width:200px;height:20px'></div>"), None);
    // flex item: refused (parent flex)
    assert_eq!(fills_with("", "", "", "<div style='display:flex'><div style='overflow:hidden'><article style='height:5px'></article></div></div>"), None);
    // default entry unchanged
    let h = format!("<!doctype html><style>body{{margin:0}}</style><section style='overflow:hidden'><article style='height:20px'></article></section>");
    assert!(parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT).unsupported);
}
#[test]
fn inherit_keyword_follows_the_typed_value_and_is_gated() {
    // inherit from a visible parent behaves like visible; inherit under a hidden parent is hidden and gated like it.
    assert_eq!(fills("overflow:inherit", MT10), Some(VISIBLE_MT.to_vec()));
    let hid_parent = "<div style='overflow:hidden'><section style='overflow:inherit'><article style='height:20px'></article></section></div>";
    let h = format!("<!doctype html><style>body{{margin:0}}article{{background:#444}}</style>{hid_parent}");
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    assert!(!t.unsupported);
    assert!(!BoxTree::from_style(&t).unsupported); // text-free, contained: admitted
}
#[test]
fn clip_edge_is_the_padding_box_not_the_border_box_fl23() {
    // Reviewer ov7 (Chrome 156): 800px container, hidden section with border-right:10px (content 790, padding edge 790, border
    // edge 800); a 795px child is clipped at 790 in Chrome (hit-test at 793 returns the section) => must REFUSE, not paint it
    // into the border band.
    let c = |sec: &str, child: &str| fills(sec, &format!("<article style='height:20px;{child}'></article>"));
    assert_eq!(c("overflow:hidden;border-right:10px solid #000", "width:795px"), None);
    assert_eq!(c("overflow:hidden;border-right:10px dashed #000", "width:795px"), None);
    assert_eq!(c("overflow:hidden;border-left:10px solid #000", "width:795px"), None);
    // inside the padding edge: admitted (790 = content width), with the section border painted by the existing path
    assert!(c("overflow:hidden;border-right:10px solid #000", "width:790px").is_some());
    assert!(c("overflow:hidden;border-right:10px solid #000", "").is_some());
    // padding (not border) keeps its old behaviour: child inside the padding box is fine, beyond content+padding refuses
    assert!(c("overflow:hidden;padding-right:10px", "width:790px").is_some());
    assert_eq!(c("overflow:hidden;padding-right:10px", "width:801px"), None);
    // the descendant's own border is no longer counted twice: 780 + 2*5 border = 790 fits a 790 padding box
    assert!(c("overflow:hidden;border-right:10px solid #000", "width:780px;border:5px solid #00f").is_some());
    assert_eq!(c("overflow:hidden;border-right:10px solid #000", "width:781px;border:5px solid #00f"), None);
}
#[test]
fn rtl_border_band_variant_refuses_fl23() {
    // Reviewer note: over-constrained RTL could open the same window on the LEFT border band. Here the 795px child in a hidden
    // RTL box with a 10px LEFT border (padding box 10..800) sits at 5..800 and crosses the padding edge: REFUSED by the
    // containment check (the engine's own RTL placement; not Chrome-measured by me).
    assert_eq!(fills("overflow:hidden;direction:rtl;border-left:10px solid #000", "<article style='height:20px;width:795px'></article>"), None);
    // plain hidden RTL with no border: the child is fully inside the box, admitted by the same check; hidden adds nothing
    // there beyond the existing RTL placement (RTL itself is outside my Chrome oracle here, so no equality claim).
    assert!(fills("overflow:hidden;direction:rtl", "<article style='height:20px;width:795px'></article>").is_some());
}
