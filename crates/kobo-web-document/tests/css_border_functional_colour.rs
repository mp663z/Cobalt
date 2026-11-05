//! Spaced `rgb(r, g, b)` in border declarations must give the same computed
//! decoration and the same rasterized pixels as the tight form, and
//! unsupported forms must still refuse before painting.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::{DisplayList, Rgb},
    parse_style_tree, Limits,
};
fn paint(decl: &str) -> Option<DisplayList> {
    let html = format!(
        "<!doctype html><style>html,body{{margin:0}}div{{width:20px;height:10px;padding:2px;margin:0}}</style><html><body><div style='{decl}'></div></body></html>"
    );
    let arena = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    if arena.unsupported {
        return None;
    }
    paint_backgrounds(&BoxTree::from_style(&arena), 80, 80).ok()
}
fn pixels(list: &DisplayList) -> Vec<u8> {
    list.rasterize(80, 80, Rgb(255, 255, 255)).unwrap()
}
fn at(p: &[u8], x: usize, y: usize) -> [u8; 3] {
    p[(y * 80 + x) * 4..][..3].try_into().unwrap()
}
#[test]
fn spaced_and_tight_rgb_rasterize_identically() {
    let cases = [
        (
            "border:3px solid rgb(10, 20, 30);background:rgb(250, 0, 0)",
            "border:3px solid rgb(10,20,30);background:rgb(250,0,0)",
        ),
        (
            "border:rgb(10, 20, 30) 3px solid",
            "border:rgb(10,20,30) 3px solid",
        ),
        (
            "border-style:solid;border-width:2px 4px;border-color:rgb(10, 20, 30)",
            "border-style:solid;border-width:2px 4px;border-color:rgb(10,20,30)",
        ),
        (
            "border-width:3px;border-style:solid dashed;border-color:rgb( 1 , 2 , 3 )",
            "border-width:3px;border-style:solid dashed;border-color:rgb(1,2,3)",
        ),
    ];
    for (spaced, tight) in cases {
        let a = paint(spaced);
        let b = paint(tight);
        assert_eq!(a.is_some(), b.is_some(), "{spaced}");
        if let (Some(a), Some(b)) = (a, b) {
            assert_eq!(pixels(&a), pixels(&b), "{spaced}");
        }
    }
}
#[test]
fn spaced_rgb_ring_has_exact_pixels() {
    let list =
        paint("border:3px solid rgb(10, 20, 30);background:rgb(250, 0, 0)").expect("painted");
    let p = pixels(&list);
    // Outer box is 20+2*2+2*3 = 30 wide, 10+4+6 = 20 high at (0,0).
    for (x, y) in [
        (0, 0),
        (29, 0),
        (0, 19),
        (29, 19),
        (2, 10),
        (15, 2),
        (15, 17),
    ] {
        assert_eq!(at(&p, x, y), [10, 20, 30], "{x},{y}");
    }
    assert_eq!(at(&p, 3, 3), [250, 0, 0]);
    assert_eq!(at(&p, 26, 16), [250, 0, 0]);
    assert_eq!(at(&p, 30, 5), [255, 255, 255]);
    assert_eq!(at(&p, 5, 20), [255, 255, 255]);
    if let Ok(dir) = std::env::var("COBALT_DIAG_DUMP") {
        std::fs::write(format!("{dir}/spaced_rgb_ring.rgba"), &p).unwrap();
    }
}
#[test]
fn unsupported_forms_still_refuse_before_paint() {
    for bad in [
        "border:3px solid rgb(10, 20, 30",
        "border:3px solid rgb(10, 20, 30))",
        "border:3px solid rgba(10, 20, 30, 0.5)",
        "border:3px solid var(--x, red)",
        "border:3px solid rgb(10 20 30)",
        "border-color:rgb(1, 2, 3) rgb(4, 5, 6) rgb(7, 8, 9) rgb(1, 2, 3)",
    ] {
        // The last case is accepted syntax (four colours) but differing side
        // colours need diagonal joins, so painting must refuse it.
        assert!(
            paint(&format!("border-style:solid;border-width:3px;{bad}")).is_none()
                || bad.starts_with("border-color"),
            "{bad}"
        );
    }
    assert!(paint("border-style:solid;border-width:3px;border-color:rgb(1, 2, 3) rgb(4, 5, 6) rgb(7, 8, 9) rgb(1, 2, 3)").is_none());
}
