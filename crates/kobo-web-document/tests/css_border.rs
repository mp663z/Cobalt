use kobo_web_document::{
    box_tree::BoxTree,
    css_background::paint_backgrounds,
    display_list::{DisplayList, Rgb},
    parse_style_tree, Limits,
};
fn paint(
    styles: &str,
    html: &str,
) -> Result<DisplayList, kobo_web_document::css_paint_bridge::BridgeError> {
    let html =
        format!("<!doctype html><style>html,body{{margin:0}}p{{margin:0}}{styles}</style>{html}");
    let arena = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    paint_backgrounds(&BoxTree::from_style(&arena), 80, 80)
}
fn rgb(list: &DisplayList, x: usize, y: usize) -> [u8; 3] {
    let p = list.rasterize(80, 80, Rgb(255, 255, 255)).unwrap();
    p[(y * 80 + x) * 4..][..3].try_into().unwrap()
}
#[test]
fn currentcolor_unequal_width_ring_and_clipped_background() {
    let list=paint("div{color:#123456;width:20px;height:10px;border:solid;border-width:2px 4px 6px 8px;padding:3px;background:red;background-clip:content-box}","<html><body><div></div></body></html>").unwrap();
    for (x, y) in [(0, 0), (37, 0), (0, 23), (37, 23), (7, 10), (34, 10)] {
        assert_eq!(rgb(&list, x, y), [0x12, 0x34, 0x56], "{x},{y}");
    }
    assert_eq!(rgb(&list, 8, 2), [255, 255, 255]);
    assert_eq!(rgb(&list, 11, 5), [255, 0, 0]);
    assert_eq!(rgb(&list, 38, 24), [255, 255, 255]);
}
#[test]
fn border_only_root_and_propagated_body_still_paint() {
    for selector in ["html", "body"] {
        let list = paint(
            &format!("{selector}{{height:20px;border:2px solid blue;background:red}}"),
            "<html><body style='height:20px'></body></html>",
        )
        .unwrap();
        assert_eq!(rgb(&list, 0, 0), [0, 0, 255]);
        assert_eq!(rgb(&list, 20, 10), [255, 0, 0]);
        assert_eq!(rgb(&list, 40, 50), [255, 0, 0]);
    }
    let list = paint(
        "div{width:20px;height:10px;border:2px solid blue}",
        "<html><body><div></div></body></html>",
    )
    .unwrap();
    assert_eq!(rgb(&list, 0, 0), [0, 0, 255]);
    assert_eq!(rgb(&list, 3, 3), [255, 255, 255]);
}
#[test]
fn currentcolor_resolves_at_element_but_explicit_rgb_inherits_as_rgb() {
    for (color, expected) in [("currentColor", [0, 0, 255]), ("red", [255, 0, 0])] {
        let list=paint(&format!("body{{color:red;border-color:{color}}}div{{width:20px;height:10px;color:blue;border:2px solid;border-color:inherit}}"),"<html><body><div></div></body></html>").unwrap();
        assert_eq!(rgb(&list, 0, 0), expected);
    }
}
#[test]
fn descendant_background_covers_parent_border_where_negative_margin_overlaps() {
    let list=paint("main{width:20px;height:20px;border:4px solid blue}div{width:20px;height:10px;background:red;margin-left:-4px;margin-top:-4px}","<html><body><main><div></div></main></body></html>").unwrap();
    assert_eq!(rgb(&list, 0, 0), [255, 0, 0]);
    assert_eq!(rgb(&list, 27, 0), [0, 0, 255]);
}
#[test]
fn differing_side_colours_alpha_and_styles_refuse() {
    for extra in [
        "border-left-color:red",
        "border-left-color:transparent",
        "border-color:rgba(0,0,0,0.5)",
        "border-style:dashed",
        "border-style:double",
    ] {
        assert!(
            paint(
                &format!("div{{width:20px;height:10px;border:2px solid blue;{extra}}}"),
                "<html><body><div></div></body></html>"
            )
            .is_err(),
            "{extra}"
        );
    }
}
#[test]
fn transparent_none_hidden_and_zero_sides_do_not_add_paint() {
    for border in [
        "2px solid transparent",
        "2px none red",
        "2px hidden blue",
        "0 solid red",
    ] {
        let list = paint(
            &format!("div{{width:20px;height:10px;border:{border}}}"),
            "<html><body><div></div></body></html>",
        )
        .unwrap();
        assert!(list.commands().is_empty(), "{border}");
    }
    let list=paint("div{width:20px;height:10px;border:2px solid blue;border-top-width:0px;border-bottom-width:0px}","<html><body><div></div></body></html>").unwrap();
    assert_eq!(rgb(&list, 0, 0), [0, 0, 255]);
    assert_eq!(rgb(&list, 3, 0), [255, 255, 255]);
    assert_eq!(rgb(&list, 23, 9), [0, 0, 255]);
}
#[test]
fn longhand_colour_cascade_and_shorthand_reset() {
    for (decl, expected) in [
        ("border:2px solid red;border-color:blue", [0, 0, 255]),
        ("border-color:red;border:2px solid;color:blue", [0, 0, 255]),
        (
            "border:2px solid red;border-color:initial;color:blue",
            [0, 0, 255],
        ),
    ] {
        let list = paint(
            &format!("div{{width:20px;height:10px;{decl}}}"),
            "<html><body><div></div></body></html>",
        )
        .unwrap();
        assert_eq!(rgb(&list, 0, 0), expected);
    }
}

#[test]
fn border_command_budget_fails_whole_page() {
    let html = format!("<html><body>{}</body></html>", "<div></div>".repeat(1100));
    let err = paint("div{width:20px;height:1px;border:1px solid blue}", &html).err();
    assert_eq!(
        err,
        Some(kobo_web_document::css_paint_bridge::BridgeError::TooManyCommands)
    );
}

#[test]
fn visual_lab_sources_keep_rendering_without_replacing_wpt() {
    for html in [
        include_bytes!("fixtures/solid-border-lab.html").as_slice(),
        include_bytes!("fixtures/solid-border-inherit-lab.html").as_slice(),
    ] {
        let arena = parse_style_tree(html, &[], &Limits::DEFAULT);
        assert!(paint_backgrounds(&BoxTree::from_style(&arena), 400, 320).is_ok());
    }
}

// Experimental: Chrome (headless, 1x) computes border widths 1.5px->1px, 0.5px->1px,
// 2.99px->2px, 0.01px->1px, 1.99px->1px, 49.75px->49px.
#[test]
fn fractional_border_widths_snap_like_blink_floor_min_one() {
    for (decl, w) in [("1.5px", 1u32), ("0.5px", 1), ("2.99px", 2), ("0.01px", 1), ("1.99px", 1)] {
        let list = paint(
            &format!("div{{width:20px;height:10px;border:{decl} solid blue}}"),
            "<html><body><div></div></body></html>",
        )
        .unwrap();
        // The snapped ring is w px thick: pixel at (w-1, 0) is border, (w, w) is content.
        assert_eq!(rgb(&list, w as usize - 1, 0), [0, 0, 255], "{decl}");
        assert_eq!(rgb(&list, w as usize, w as usize), [255, 255, 255], "{decl}");
    }
}
#[test]
fn fractional_lengths_other_than_border_width_still_refuse() {
    for extra in ["width:20.5px", "height:10.5px", "padding:1.5px", "margin:1.5px"] {
        assert!(
            paint(
                &format!("div{{width:20px;height:10px;border:2px solid blue;{extra}}}"),
                "<html><body><div></div></body></html>"
            )
            .is_err(),
            "{extra}"
        );
    }
}

// Fail-closed controls for the experimental snap: only plain `N.NN px` with N.NN > 0 is accepted.
// Chrome gives answers for some of these (>2 decimals floor, `.5px` -> 1px, `1e1px` -> 10px,
// quirks-mode unitless), but this patch deliberately refuses them instead of guessing.
#[test]
fn unsupported_fractional_border_spellings_still_refuse() {
    for decl in ["1.555px", ".5px", "1e1px", "-1.5px", "-1px", "1.5", "1.5em", "1..5px"] {
        assert!(
            paint(
                &format!("div{{width:20px;height:10px;border:{decl} solid blue}}"),
                "<html><body><div></div></body></html>"
            )
            .is_err(),
            "{decl}"
        );
    }
}
#[test]
fn zero_fractional_border_is_zero_not_one() {
    for decl in ["0.0px", "0.00px", "0px"] {
        let list = paint(
            &format!("div{{width:20px;height:10px;border:{decl} solid blue}}"),
            "<html><body><div></div></body></html>",
        )
        .unwrap();
        assert!(list.commands().is_empty(), "{decl}");
    }
}
#[test]
fn fractional_border_snap_applies_in_longhand_and_shorthand_forms() {
    for style in [
        "border-width:1.5px;border-style:solid;border-color:blue",
        "border-top-width:1.5px;border-right-width:1.5px;border-bottom-width:1.5px;border-left-width:1.5px;border-style:solid;border-color:blue",
        "border:1.5px solid blue",
    ] {
        let list = paint(
            &format!("div{{width:20px;height:10px;{style}}}"),
            "<html><body><div></div></body></html>",
        )
        .unwrap();
        assert_eq!(rgb(&list, 0, 0), [0, 0, 255], "{style}");
        assert_eq!(rgb(&list, 1, 1), [255, 255, 255], "{style}");
    }
}
