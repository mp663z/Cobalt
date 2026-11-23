use kobo_web_document::{
    box_tree::BoxTree,
    css_background::{paint_backgrounds, paint_backgrounds_float_kernel},
    display_list::{Command, DisplayList, Rgb},
    parse_style_tree, Limits,
};

fn tree(css: &str, body: &str) -> BoxTree {
    let html = format!("<!doctype html><style>body{{margin:0}}{css}</style>{body}");
    BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
}

const BODY: &str = "<div class=p><div class=f></div><div class=c></div></div><div class=n></div>";
const COLORS: &str = ".f{background:#f00}.c{background:#0f0}.n{background:#00f}";
const BASE: &str = ".p{width:400px}.f{float:left;width:100px;height:50px}.c{clear:both;height:10px}.n{width:400px;height:7px}";

/// Colour at (x, y): the last fill command covering it wins (paint order).
fn pixel(list: &DisplayList, x: i32, y: i32) -> Option<Rgb> {
    let mut out = None;
    for c in list.commands() {
        if let Command::Fill { rect, color, .. } = c {
            if x >= rect.x
                && y >= rect.y
                && x < rect.x + i32::try_from(rect.width).unwrap()
                && y < rect.y + i32::try_from(rect.height).unwrap()
            {
                out = Some(*color);
            }
        }
    }
    out
}

/// Review blocker 4:29 (colored overlap): parent height 10, float 100x50 red,
/// clear 10 green, next block 400x7 blue at y10. Chrome 156 paints the float above
/// the blue block (pixel 10,12 red). Default painters refuse; only the explicit float-kernel painter draws the float last.
#[test]
fn default_painters_still_refuse_float_layout() {
    for extra in ["", ".p{height:10px}", ".p{height:59px}", ".p{height:60px}", ".p{height:300px}"] {
        let t = tree(&format!("{BASE}{COLORS}{extra}"), BODY);
        assert!(paint_backgrounds(&t, 800, 600).is_err(), "default painter drew: {extra}");
    }
}

/// Recorded Chrome 156 reference (private floatkernel/v2/chrome-v2-with-y.txt,
/// b1: next block y=10): the float is red at (10,12) even though the blue block
/// background covers it in tree order. Pixel colours of the overflow case and the
/// fitting case below are the spec-order result: block backgrounds first, then the float.
#[test]
fn explicit_float_kernel_painter_draws_float_above_overflowed_sibling_background() {
    let t = tree(&format!("{BASE}{COLORS}.p{{height:10px}}"), BODY);
    let list = paint_backgrounds_float_kernel(&t, 800, 600).expect("kernel painter");
    assert_eq!(pixel(&list, 10, 12), Some(Rgb(255, 0, 0)), "float above blue block");
    assert_eq!(pixel(&list, 150, 12), Some(Rgb(0, 0, 255)), "blue visible beside float");
    assert_eq!(pixel(&list, 10, 45), Some(Rgb(255, 0, 0)), "float below the parent box");
    assert_eq!(pixel(&list, 10, 55), Some(Rgb(0, 255, 0)), "cleared block starts at y50");
    assert_eq!(pixel(&list, 150, 12), Some(Rgb(0, 0, 255)));
}

#[test]
fn explicit_painter_fitting_parent_and_other_shapes() {
    let t = tree(&format!("{BASE}{COLORS}"), BODY);
    let list = paint_backgrounds_float_kernel(&t, 800, 600).expect("fits");
    assert_eq!(pixel(&list, 10, 10), Some(Rgb(255, 0, 0)));
    assert_eq!(pixel(&list, 150, 52), Some(Rgb(0, 255, 0)));
    assert_eq!(pixel(&list, 150, 62), Some(Rgb(0, 0, 255)));
    // the explicit entry does not admit anything outside the kernel
    for (css, body) in [
        (format!("{BASE}{COLORS}.f{{float:inline-start}}"), BODY),
        (format!("{BASE}{COLORS}"), "<div class=p><div class=f></div></div><div class=n></div>"),
    ] {
        assert!(paint_backgrounds_float_kernel(&tree(&css, body), 800, 600).is_err(), "{css}");
    }
    // plain page paints the same through both entries
    let plain = tree(
        ".p{width:400px}.f{width:100px;height:50px}.c{height:10px}.n{width:400px;height:7px}.f{background:#f00}",
        BODY,
    );
    assert_eq!(
        paint_backgrounds(&plain, 800, 600).unwrap().commands(),
        paint_backgrounds_float_kernel(&plain, 800, 600).unwrap().commands()
    );
}

/// Review blocker 4:51: two kernels in one tree. The explicit painter must refuse.
#[test]
fn explicit_painter_refuses_two_kernels_in_one_tree() {
    let css = ".p1{width:400px;height:10px}.p2{width:400px}.f1{float:left;width:100px;height:50px;background:#f00}.c1{clear:both;height:10px;background:#0f0}.f2{float:left;width:100px;height:5px;background:#00f}.c2{clear:both;height:5px;background:#ff0}";
    let two = "<div class=p1><div class=f1></div><div class=c1></div></div><div class=p2><div class=f2></div><div class=c2></div></div>";
    assert!(paint_backgrounds_float_kernel(&tree(css, two), 800, 600).is_err());
    let one = "<div class=p1><div class=f1></div><div class=c1></div></div>";
    assert!(paint_backgrounds_float_kernel(&tree(css, one), 800, 600).is_ok());
}

const BASE_B: &str = ".p{width:400px}.f{float:left;width:100px;height:50px;background:#f00}.n{height:7px;background:#00f}.m{width:400px;height:20px;background:#880}";
const BODY_B: &str = "<div class=p><div class=f></div><div class=n></div></div><div class=m></div>";

/// Variant B pixels (Chrome 156 whole-image compared in floatkernel-b/ctx, 0 diffs):
/// float red above the blue normal block and above the later block, which starts at y7.
#[test]
fn variant_b_explicit_painter_pixels() {
    let t = tree(BASE_B, BODY_B);
    let list = paint_backgrounds_float_kernel(&t, 785, 600).expect("variant B");
    assert_eq!(pixel(&list, 10, 3), Some(Rgb(255, 0, 0)), "float above n");
    assert_eq!(pixel(&list, 150, 3), Some(Rgb(0, 0, 255)), "n beside float");
    assert_eq!(pixel(&list, 10, 20), Some(Rgb(255, 0, 0)), "float above m");
    assert_eq!(pixel(&list, 150, 20), Some(Rgb(136, 136, 0)), "m starts at y7");
    assert_eq!(pixel(&list, 150, 40), None, "nothing below m");
    assert!(paint_backgrounds(&t, 785, 600).is_err(), "default painter stays closed");
}

/// Variant C: float:right (Chrome 156, floatkernel-c/ctx r1-r7, whole-image 0 diffs).
#[test]
fn variant_c_right_float_explicit_painter_pixels() {
    let t = tree(&BASE_B.replace("float:left", "float:right"), BODY_B);
    let list = paint_backgrounds_float_kernel(&t, 785, 600).expect("variant C");
    assert_eq!(pixel(&list, 350, 3), Some(Rgb(255, 0, 0)), "right float above n");
    assert_eq!(pixel(&list, 299, 3), Some(Rgb(0, 0, 255)), "n left of float");
    assert_eq!(pixel(&list, 10, 3), Some(Rgb(0, 0, 255)), "left edge is n, not float");
    assert_eq!(pixel(&list, 350, 20), Some(Rgb(255, 0, 0)), "float above m");
    assert_eq!(pixel(&list, 10, 20), Some(Rgb(136, 136, 0)), "m under left edge");
    assert_eq!(pixel(&list, 400, 3), None, "nothing right of the parent");
    assert!(paint_backgrounds(&t, 785, 600).is_err(), "default painter stays closed");
    // A margin on a right float is refused by the explicit painter too.
    let m = tree(&format!("{}.f{{margin-left:5px}}", BASE_B.replace("float:left", "float:right")), BODY_B);
    assert!(paint_backgrounds_float_kernel(&m, 785, 600).is_err());
}

fn right_pixels_ok(css: &str) {
    let t = tree(css, BODY_B);
    let list = paint_backgrounds_float_kernel(&t, 785, 600).expect("right float");
    assert_eq!(pixel(&list, 350, 3), Some(Rgb(255, 0, 0)), "float at the right edge");
    assert_eq!(pixel(&list, 10, 3), Some(Rgb(0, 0, 255)), "n at the left edge");
    assert!(paint_backgrounds(&t, 785, 600).is_err(), "default painter stays closed");
}

/// Reviewer evidence 5:55 (Chrome 156 var-right/var-fallback-right, 0 diffs): var()
/// reaches the same kernel through the normal cascade. Right is a global INTAKE
/// expansion (incl. var()); only the explicit painter draws it, defaults stay closed.
#[test]
fn variant_c_var_right_and_fallback_reach_the_kernel() {
    let plain = BASE_B.replace("float:left;", "");
    right_pixels_ok(&format!("{plain}.f{{--side:right;float:var(--side)}}"));
    right_pixels_ok(&format!("{plain}.f{{float:var(--missing,right)}}"));
}

/// A later float:none resets the float: no kernel remains and the default painter is
/// not blocked by float geometry. An unparsed value does NOT reset: it is refused.
#[test]
fn variant_c_float_none_reset_and_unparsed_value_are_not_kernels() {
    let reset = tree(&format!("{}.f{{float:none}}", BASE_B.replace("float:left", "float:right")), BODY_B);
    assert!(paint_backgrounds(&reset, 785, 600).is_ok());
    let bad = tree(&format!("{}.f{{float:var(--x,inline-start)}}", BASE_B.replace("float:left;", "")), BODY_B);
    assert!(paint_backgrounds_float_kernel(&bad, 785, 600).is_err());
}

/// Variant D: kernel float width as a whole-px percentage of the px parent (Chrome
/// 156 floatkernel-c/ctxd d1-d7 + q4 33% = 132px, whole-image 0 diffs). Fractional
/// results (25.001%, 66.6667%), 0% and >100% stay refused.
#[test]
fn variant_d_whole_px_percent_float_width() {
    let base = BASE_B.replace("width:100px", "width:50%");
    let t = tree(&base, BODY_B);
    let list = paint_backgrounds_float_kernel(&t, 785, 600).expect("50% of 400 = 200");
    assert_eq!(pixel(&list, 199, 3), Some(Rgb(255, 0, 0)), "float covers 200px");
    assert_eq!(pixel(&list, 200, 3), Some(Rgb(0, 0, 255)), "n visible past 200px");
    assert!(paint_backgrounds(&t, 785, 600).is_err(), "default painter stays closed");
    let r = tree(&base.replace("float:left", "float:right").replace("50%", "75%"), BODY_B);
    let list = paint_backgrounds_float_kernel(&r, 785, 600).expect("75% right");
    assert_eq!(pixel(&list, 100, 3), Some(Rgb(255, 0, 0)), "right float starts at x100");
    assert_eq!(pixel(&list, 99, 3), Some(Rgb(0, 0, 255)));
    let zero = tree(&base.replace(".p{width:400px}", ".p{width:0px}"), BODY_B);
    assert!(paint_backgrounds_float_kernel(&zero, 785, 600).is_err(), "0px parent, 50% float");
    for bad in ["25.001%", "66.6667%", "101%", "0%"] {
        let t = tree(&BASE_B.replace("100px", bad), BODY_B);
        assert!(paint_backgrounds_float_kernel(&t, 785, 600).is_err(), "{bad}");
    }
}

/// Variant E1: explicit zero-height clear (clearfix shape). Chrome 156 g1-g7 0 diffs.
#[test]
fn variant_e1_zero_height_clear_pixels() {
    let css = ".p{width:400px;background:#ccc}.f{float:left;width:100px;height:50px;background:#f00}.c{clear:both}.m{width:400px;height:20px;background:#880}";
    let body = "<div class=p><div class=f></div><div class=c></div></div><div class=m></div>";
    let t = tree(css, body);
    let list = paint_backgrounds_float_kernel(&t, 785, 600).expect("E1");
    assert_eq!(pixel(&list, 10, 49), Some(Rgb(255, 0, 0)), "float rows");
    assert_eq!(pixel(&list, 300, 49), Some(Rgb(204, 204, 204)), "parent holds the float: 50 tall");
    assert_eq!(pixel(&list, 300, 50), Some(Rgb(136, 136, 0)), "next block starts at y50");
    assert!(paint_backgrounds(&t, 785, 600).is_err(), "default painter stays closed");
}
