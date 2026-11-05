//! Nested bare phrases share one sourced IFC; rectangle-only paths still refuse.
use kobo_web_document::{
    box_tree::{BoxTree, VerticalPass, WidthPass},
    css_text_page::{paint_direct_text_blocks, FontProvider},
    display_list::{Command, Rgb},
    inline_lines::GlyphBitmap,
    parse_style_tree, Limits,
};
use std::cell::Cell;
struct Face {
    calls: Cell<usize>,
}
impl FontProvider for Face {
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> {
        self.calls.set(self.calls.get() + 1);
        Some(5)
    }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> {
        self.calls.set(self.calls.get() + 1);
        Some(20)
    }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> {
        self.calls.set(self.calls.get() + 1);
        Some(15)
    }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        self.calls.set(self.calls.get() + 1);
        Some(GlyphBitmap {
            left: 0,
            top: -1,
            width: 1,
            height: 1,
            coverage: vec![255],
        })
    }
}
fn tree(body: &str) -> BoxTree {
    BoxTree::from_style(&parse_style_tree(format!("<!doctype html><style>html,body,p{{margin:0}}p{{width:100px;line-height:20px}}</style><p>{body}</p><div style='width:10px;height:10px;background:blue'></div>").as_bytes(),&[],&Limits::DEFAULT))
}
#[test]
fn complete_page_nested_paint_and_rectangle_only_refusals_are_atomic() {
    let nested =
        tree("aa<span><strong>bb<span style='font-weight:400'>cc</span></strong></span> dd");
    let flat = tree("aa<strong>bb</strong>cc dd");
    let f = Face {
        calls: Cell::new(0),
    };
    let actual = paint_direct_text_blocks(&nested, 100, 100, &f).unwrap();
    let expected = paint_direct_text_blocks(&flat, 100, 100, &f).unwrap();
    assert_eq!(
        actual.rasterize(100, 100, Rgb(255, 255, 255)),
        expected.rasterize(100, 100, Rgb(255, 255, 255))
    );
    assert!(
        actual
            .commands()
            .iter()
            .filter_map(|c| if let Command::GlyphRun { source, .. } = c {
                source.node
            } else {
                None
            })
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 4
    );
    assert!(kobo_web_document::css_background::paint_backgrounds(&nested, 100, 100).is_err());
    let widths = WidthPass::from_boxes(&nested, 100);
    let vertical = VerticalPass::from_boxes(&nested, 100, 100);
    assert!(
        kobo_web_document::css_paint_bridge::paint_content_rectangles(
            &nested,
            &widths,
            &vertical,
            100,
            100,
            &[]
        )
        .is_err()
    );
}
#[test]
fn complete_bad_subtree_and_page_cap_fail_before_metrics() {
    for html in [
        "aa<span><span style='padding:1px'></span></span>bb",
        "aa<span><img></span>bb",
        "aa<span><br></span>bb",
    ] {
        let face = Face {
            calls: Cell::new(0),
        };
        assert!(paint_direct_text_blocks(&tree(html), 100, 100, &face).is_err());
        assert_eq!(face.calls.get(), 0);
    }
    let huge = tree(&"<span>a</span>".repeat(4097));
    let face = Face {
        calls: Cell::new(0),
    };
    assert!(paint_direct_text_blocks(&huge, 100, 100, &face).is_err());
    assert_eq!(face.calls.get(), 0);
}
#[test]
fn anonymous_block_and_nested_replaced_remain_refused() {
    let face = Face {
        calls: Cell::new(0),
    };
    for html in [
        "aa<div>bb</div>cc",
        "aa<span><svg></svg></span>bb",
        "aa<span style='display:inline-block'>bb</span>cc",
    ] {
        assert!(paint_direct_text_blocks(&tree(html), 100, 100, &face).is_err());
    }
}

#[test]
fn phrase_marker_origin_attributes_and_typography_fail_closed() {
    let face = Face {
        calls: Cell::new(0),
    };
    for html in [
        "aa<span onclick='void(0)'><span>bb</span></span>",
        "aa<span contenteditable><span>bb</span></span>",
        "aa<unknown><span>bb</span></unknown>",
        "aa<em><span>bb</span></em>",
        "aa<i><span>bb</span></i>",
    ] {
        assert!(
            paint_direct_text_blocks(&tree(html), 100, 100, &face).is_err(),
            "{html}"
        );
    }
    let mut forged = tree("aa<span><span>bb</span></span>");
    let inline = forged.boxes.iter().position(|n| n.inline_phrase).unwrap();
    forged.boxes[inline].inline_phrase = false;
    assert!(paint_direct_text_blocks(&forged, 100, 100, &face).is_err());
}

#[test]
fn whole_page_depth_and_cap_precedence_before_callbacks() {
    for depth in [96, 97] {
        let nested = format!("{}a{}", "<span>".repeat(depth), "</span>".repeat(depth));
        let face = Face {
            calls: Cell::new(0),
        };
        let result = paint_direct_text_blocks(&tree(&nested), 100, 100, &face);
        assert_eq!(result.is_ok(), depth == 96);
        if depth == 97 {
            assert_eq!(face.calls.get(), 0);
        }
    }
    // Page arena cap wins before collector cap when both apply.
    let huge = tree(&"<span>a</span>".repeat(4097));
    let face = Face {
        calls: Cell::new(0),
    };
    assert!(matches!(
        paint_direct_text_blocks(&huge, 100, 100, &face),
        Err(kobo_web_document::css_text_page::PageError::Unsupported)
    ));
    assert_eq!(face.calls.get(), 0);
}

#[test]
fn legacy_direct_single_text_tags_keep_pixels_but_new_shapes_stay_closed() {
    let face = Face {
        calls: Cell::new(0),
    };
    let baseline = paint_direct_text_blocks(&tree("aa<span>bb</span>cc"), 100, 100, &face)
        .unwrap()
        .rasterize(100, 100, Rgb(255, 255, 255))
        .unwrap();
    for tag in ["u", "abbr", "unknown"] {
        let html = format!("aa<{tag} data-x='1' aria-label='term' lang='en' role='note' unknown-meta='x'>bb</{tag}>cc");
        let actual = paint_direct_text_blocks(&tree(&html), 100, 100, &face)
            .unwrap()
            .rasterize(100, 100, Rgb(255, 255, 255))
            .unwrap();
        assert_eq!(actual, baseline, "{tag}");
        for html in [
            format!("aa<{tag}><span>bb</span></{tag}>cc"),
            format!("aa<span><{tag}>bb</{tag}></span>cc"),
            format!("aa<{tag}></{tag}>cc"),
        ] {
            assert!(
                paint_direct_text_blocks(&tree(&html), 100, 100, &face).is_err(),
                "{html}"
            );
        }
    }
    for html in [
        "aa<img>bb",
        "aa<br>bb",
        "aa<span onclick='void(0)'>bb</span>cc",
        "aa<unknown onclick='void(0)'>bb</unknown>cc",
    ] {
        let before = face.calls.get();
        assert!(
            paint_direct_text_blocks(&tree(html), 100, 100, &face).is_err(),
            "{html}"
        );
        assert_eq!(face.calls.get(), before);
    }
}
