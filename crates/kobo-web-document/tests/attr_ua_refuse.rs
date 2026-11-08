//! PRIVATE candidate FL46 (soundness fix, refusal only): the `hidden` (except until-found), `popover` and `dir` (except ltr) HTML
//! attributes change used style through the UA sheet; this build models none of them, so any element carrying one refuses the page.
//! Chrome oracle: private-evidence ul1/at65 (w01 hidden, w08 popover, w15 dir=rtl painted differently from the engine before the fix;
//! engine Fill rects vs my own rasteriser, constructed, synthetic, ext entry, MediaContext default). This test holds no Chrome/pixels.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors,
    parse_style_tree_with_media, parse_style_tree_with_pinned_font_family, Limits, MediaContext, SupplyBudget,
};
fn ext_ok(h: &str) -> bool {
    let t = parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if t.unsupported {
        return false;
    }
    let bt = BoxTree::from_style(&t);
    !bt.unsupported && paint_backgrounds(&bt, 800, 600).is_ok()
}
fn pg(attrs: &str) -> String {
    format!("<!doctype html><style>.c{{background:#cde}}.d{{background:#fda}}</style><body style='margin:0'><div class=c {attrs} style='height:20px;width:100px'><div class=d style='height:10px;width:50px'></div></div>")
}
#[test]
fn anchors_inert_and_listed_attributes_still_admit() {
    for a in ["", "title=x lang=en data-x=1 role=banner aria-label=x tabindex=0", "hidden=until-found", "hidden=UNTIL-FOUND", "dir=ltr", "dir=LTR", "dir=' ltr '", "draggable=true", "width=300 height=300 border=5 hspace=9 vspace=9 bgcolor=red nowrap valign=bottom"] {
        assert!(ext_ok(&pg(a)), "anchor refused: {a}");
    }
}
#[test]
fn hidden_popover_and_non_ltr_dir_refuse_anywhere() {
    for a in ["hidden", "hidden=''", "hidden=hidden", "hidden=true", "hidden=until-found-not", "popover", "popover=auto", "popover=manual", "dir=rtl", "dir=RTL", "dir=auto", "dir=''", "dir=bogus", "align=center", "align=right", "align=left", "align=justify", "align=middle", "align=bogus", "align=''"] {
        assert!(!ext_ok(&pg(a)), "admitted: {a}");
    }
    // a nested element, the body and the root carry the attribute
    assert!(!ext_ok("<!doctype html><style>.c{background:#cde}</style><body style='margin:0'><div class=c style='height:20px'><div hidden style='height:5px'></div></div>"));
    assert!(!ext_ok("<!doctype html><style>.c{background:#cde}</style><body dir=rtl style='margin:0'><div class=c style='height:20px'></div>"));
    assert!(!ext_ok("<!doctype html><html dir=rtl><style>.c{background:#cde}</style><body style='margin:0'><div class=c style='height:20px'></div>"));
    assert!(!ext_ok("<!doctype html><style>.c{background:#cde}</style><body style='margin:0'><div class=c style='height:20px;display:none' popover></div>"), "even a display:none box refuses");
}
#[test]
fn every_entry_refuses() {
    let h = pg("hidden");
    for tree in [parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT), parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL), parse_style_tree_with_media(h.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(785).unwrap())] {
        assert!(tree.unsupported);
    }
}

// ---- FL47 (additive): presentational hint attributes. Chrome oracle ul1/bg66; refusal only.
fn body_pg(attrs: &str) -> String {
    format!("<!doctype html><style>.d{{background:#fda}}</style><body {attrs}><div class=d style='height:10px;width:100px'></div>")
}
#[test]
fn body_legacy_attributes_refuse_and_inert_attributes_stay_admitted() {
    assert!(ext_ok(&body_pg("style='margin:0'")), "anchor");
    for a in ["bgcolor=red", "bgcolor=#00ff00", "text=red", "link=blue", "vlink=green", "alink=red", "background=x.png", "leftmargin=0", "rightmargin=0", "topmargin=0", "bottommargin=0", "marginwidth=0", "marginheight=0", "bgcolor=red style='margin:0'", "topmargin=20 style='margin:0'"] {
        assert!(!ext_ok(&body_pg(a)), "admitted: {a}");
    }
    // the same names on a non-body element are inert in Chrome (a6/b6 0 diff) and stay admitted
    for a in ["bgcolor=red", "text=red", "link=blue", "background=x.png", "leftmargin=0", "marginwidth=0"] {
        assert!(ext_ok(&pg(a)), "anchor refused: {a}");
    }
    // html bgcolor was 0 diff in Chrome (a9) and is not refused
    assert!(ext_ok("<!doctype html><html bgcolor=red><style>.d{background:#fda}</style><body style='margin:0'><div class=d style='height:10px;width:100px'></div></html>"));
    // align on a nested element and on the body
    assert!(!ext_ok("<!doctype html><style>.d{background:#fda}</style><body style='margin:0' align=center><div class=d style='height:10px;width:100px'></div>"));
    assert!(!ext_ok("<!doctype html><style>.d{background:#fda}</style><body style='margin:0'><section align=center class=d style='height:10px;width:100px'></section>"));
}
