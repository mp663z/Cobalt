//! Restricted non-solid border proof: one side, complete integer patterns only.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree,
    Limits,
};
fn paint(css: &str) -> Result<Vec<u8>, kobo_web_document::css_paint_bridge::BridgeError> {
    let html=format!("<!doctype html><style>html,body{{margin:0}}div{{width:30px;height:30px;{css}}}</style><div></div>");
    let tree = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
    Ok(paint_backgrounds(&tree, 80, 80)?
        .rasterize(80, 80, Rgb(255, 255, 255))
        .unwrap())
}
#[test]
fn complete_single_side_patterns_have_exact_ink_and_gaps() {
    for side in ["top", "right", "bottom", "left"] {
        for (style, w, length, dash, gap) in [
            ("dotted", 1, 11, 1, 1),
            ("dotted", 2, 18, 2, 2),
            ("dotted", 3, 27, 3, 3),
            ("dashed", 1, 18, 3, 2),
            ("dashed", 2, 36, 6, 4),
            ("dashed", 3, 24, 6, 3),
            ("dashed", 4, 44, 8, 4),
        ] {
            let p = paint(&format!(
                "width:{length}px;height:{length}px;border-{side}:{w}px {style} blue"
            ))
            .unwrap();
            for y in 0_usize..80 {
                for x in 0_usize..80 {
                    let (along, across) = match side {
                        "top" => (x, y),
                        "left" => (y, x),
                        "bottom" => (x, y.saturating_sub(length)),
                        _ => (y, x.saturating_sub(length)),
                    };
                    let in_band = match side {
                        "bottom" => y >= length && y < length + w,
                        "right" => x >= length && x < length + w,
                        _ => across < w,
                    };
                    let expected = if in_band && along < length && along % (dash + gap) < dash {
                        [0, 0, 255]
                    } else {
                        [255, 255, 255]
                    };
                    assert_eq!(
                        &p[(y * 80 + x) * 4..][..3],
                        expected,
                        "{style}/{w}/{length}/{side} at{x},{y}"
                    );
                }
            }
        }
    }
}
#[test]
fn unsupported_fits_circles_and_joins_refuse_whole() {
    for css in [
        "border-top:4px dotted blue",
        "border-top:3px dashed blue;width:25px",
        "border-top:2px dotted blue;width:17px",
        "border:3px dashed blue",
        "border-top:3px dashed blue;border-left:3px solid blue",
        "border-top:1.5px dotted blue",
        "border-top:3px double blue",
        "border-top:3px dotted rgba(0,0,255,.5)",
    ] {
        assert!(paint(css).is_err(), "{css}");
    }
}
#[test]
fn style_cascade_keeps_kind_and_shorthand_reset() {
    for decl in [
        "border-top:2px dotted blue",
        "border-top:2px solid blue;border-top-style:dotted",
        "border-top:2px dotted blue;border-top-style:initial;border-top-style:dotted",
        "border-top:2px dotted blue;border-top-style:unset;border-top-style:dotted",
        "border-top:2px dotted blue!important;border-top-style:solid",
    ] {
        assert_eq!(
            paint(&format!("width:18px;{decl}")).unwrap(),
            paint("width:18px;border-top:2px dotted blue").unwrap()
        );
    }
    let h=b"<!doctype html><style>html,body{margin:0}body{border-top-style:dotted;border-top-width:0}div{width:18px;height:20px;border-top:2px blue;border-top-style:inherit}</style><div></div>";
    let t = BoxTree::from_style(&parse_style_tree(h, &[], &Limits::DEFAULT));
    assert!(paint_backgrounds(&t, 80, 80).is_ok());
}
#[test]
fn pattern_commands_obey_existing_whole_page_budget() {
    let h=b"<!doctype html><style>html,body{margin:0}div{width:9999px;height:1px;border-top:1px dotted blue}</style><div></div>";
    let t = BoxTree::from_style(&parse_style_tree(h, &[], &Limits::DEFAULT));
    assert!(paint_backgrounds(&t, 80, 80).is_err());
}

#[test]
fn currentcolor_and_no_ink_styles_preserve_used_widths() {
    assert_eq!(
        paint("width:18px;color:blue;border-top:2px dotted currentColor").unwrap(),
        paint("width:18px;border-top:2px dotted blue").unwrap()
    );
    for css in [
        "border-top:2px dotted transparent",
        "border:2px none blue",
        "border:2px hidden blue",
        "border:0 dotted blue",
    ] {
        let p = paint(css).unwrap();
        assert!(p.chunks_exact(4).all(|pixel| pixel[..3] == [255, 255, 255]));
    }
}

#[test]
fn visual_lab_source_keeps_rendering() {
    let html = include_bytes!("fixtures/single-side-pattern-lab.html");
    let tree = BoxTree::from_style(&parse_style_tree(html, &[], &Limits::DEFAULT));
    assert!(paint_backgrounds(&tree, 400, 320).is_ok());
}
