//! `:root` selects the document element with pseudo-class specificity and
//! paints exactly like the equivalent `html` rule; other pseudo-classes refuse.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::{DisplayList, Rgb},
    parse_style_tree, Limits,
};
fn paint(css: &str) -> Option<Vec<u8>> {
    let html = format!(
        "<!doctype html><style>html,body{{margin:0}}{css}</style><html><body><div style='width:10px;height:10px'></div></body></html>"
    );
    let arena = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    if arena.unsupported {
        return None;
    }
    let list: DisplayList = paint_backgrounds(&BoxTree::from_style(&arena), 40, 40).ok()?;
    list.rasterize(40, 40, Rgb(255, 255, 255)).ok()
}
fn at(p: &[u8], x: usize, y: usize) -> [u8; 3] {
    p[(y * 40 + x) * 4..][..3].try_into().unwrap()
}
#[test]
fn root_background_equals_html_background_pixels() {
    let a = paint(":root{background:rgb(250,0,0)}").expect("painted");
    let b = paint("html{background:rgb(250,0,0)}").expect("painted");
    assert_eq!(a, b);
    assert_eq!(at(&a, 0, 0), [250, 0, 0]);
    assert_eq!(at(&a, 39, 39), [250, 0, 0]);
}
#[test]
fn root_specificity_beats_html_in_either_order_in_pixels() {
    for css in [
        "html{background:blue}:root{background:rgb(250,0,0)}",
        ":root{background:rgb(250,0,0)}html{background:blue}",
    ] {
        let p = paint(css).expect("painted");
        assert_eq!(at(&p, 20, 20), [250, 0, 0], "{css}");
    }
    // A body rule is not the root: the body background propagates only when
    // the root has none, and :root here has one.
    let p = paint(":root{background:rgb(250,0,0)}body{background:blue}").expect("painted");
    assert_eq!(at(&p, 20, 20), [250, 0, 0]);
}
#[test]
fn non_root_element_never_matches_root() {
    let p = paint("body:root{background:rgb(250,0,0)}").expect("painted");
    assert_eq!(at(&p, 20, 20), [255, 255, 255]);
    let p = paint("div:root{background:rgb(250,0,0)}").expect("painted");
    assert_eq!(at(&p, 5, 5), [255, 255, 255]);
}
#[test]
fn unsupported_pseudo_classes_refuse_before_paint() {
    for sel in [
        ":hover",
        ":root:hover",
        ":root:root",
        ":not(p)",
        "::before",
        ":root::before",
    ] {
        assert!(
            paint(&format!("{sel}{{background:red}}")).is_none(),
            "{sel}"
        );
    }
}
