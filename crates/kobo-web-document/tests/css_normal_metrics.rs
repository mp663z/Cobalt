//! Paired normal metrics must not leak into explicit CSS line-height.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, FontProvider, NormalLineMetrics},
    display_list::Rgb,
    inline_lines::GlyphBitmap,
    parse_style_tree, Limits,
};
use std::cell::Cell;
struct Face {
    paired: bool,
    calls: Cell<usize>,
    bad_bold: bool,
    invalid: bool,
}
impl FontProvider for Face {
    fn normal_line_metrics(&self, _: u32, bold: bool) -> Option<NormalLineMetrics> {
        self.calls.set(self.calls.get() + 1);
        self.paired.then_some(NormalLineMetrics {
            height: if self.invalid { 0 } else { 14 },
            baseline: if bold && self.bad_bold { 9 } else { 10 },
        })
    }
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> {
        Some(3)
    }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> {
        Some(18)
    }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> {
        Some(13)
    }
    fn raster(&self, c: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(if c == ' ' {
            GlyphBitmap {
                left: 0,
                top: 0,
                width: 0,
                height: 0,
                coverage: vec![],
            }
        } else {
            GlyphBitmap {
                left: 0,
                top: -1,
                width: 1,
                height: 1,
                coverage: vec![255],
            }
        })
    }
}
fn paint(style: &str, face: &Face) -> Vec<u8> {
    let t=BoxTree::from_style(&parse_style_tree(format!("<!doctype html><style>html,body{{margin:0}}p{{margin:0;{style}}}div{{height:10px;width:10px;background:blue}}</style><p>ab</p><div></div>").as_bytes(),&[],&Limits::DEFAULT));
    paint_direct_text_blocks(&t, 100, 100, face)
        .unwrap()
        .rasterize(100, 100, Rgb(255, 255, 255))
        .unwrap()
}
#[test]
fn normal_absent_and_keyword_use_pair_for_height_and_baseline() {
    let f = Face {
        paired: true,
        calls: Cell::new(0),
        bad_bold: false,
        invalid: false,
    };
    let p = paint("", &f);
    assert_eq!(p, paint("line-height:normal", &f));
    assert!(f.calls.get() > 0);
    assert_eq!(&p[(9 * 100) * 4..][..3], [0, 0, 0]);
    assert_eq!(&p[(14 * 100) * 4..][..3], [0, 0, 255]);
}
#[test]
fn explicit_px_percent_number_never_consult_new_method_and_stay_identical() {
    for style in [
        "line-height:20px",
        "line-height:125%",
        "line-height:1.25",
        "line-height:1.5em",
    ] {
        let f = Face {
            paired: true,
            calls: Cell::new(0),
            bad_bold: false,
            invalid: false,
        };
        let l = Face {
            paired: false,
            calls: Cell::new(0),
            bad_bold: false,
            invalid: false,
        };
        assert_eq!(paint(style, &f), paint(style, &l), "{style}");
        assert_eq!(f.calls.get(), 0, "{style}");
        assert_eq!(l.calls.get(), 0);
    }
}
#[test]
fn mismatched_bold_normal_pair_refuses_whole() {
    let f = Face {
        paired: true,
        calls: Cell::new(0),
        bad_bold: true,
        invalid: false,
    };
    let t = BoxTree::from_style(&parse_style_tree(
        b"<!doctype html><p>a <b>b</b></p>",
        &[],
        &Limits::DEFAULT,
    ));
    assert!(paint_direct_text_blocks(&t, 100, 100, &f).is_err());
}

#[test]
fn invalid_normal_pair_refuses_without_partial_paint() {
    let f = Face {
        paired: true,
        calls: Cell::new(0),
        bad_bold: false,
        invalid: true,
    };
    let t = BoxTree::from_style(&parse_style_tree(
        b"<!doctype html><p>ab</p>",
        &[],
        &Limits::DEFAULT,
    ));
    assert!(paint_direct_text_blocks(&t, 100, 100, &f).is_err());
}

#[test]
fn inherited_explicit_line_height_never_calls_new_method() {
    for value in ["24px", "150%", "1.5"] {
        let html = format!("<!doctype html><body style='line-height:{value}'><p>ab</p></body>");
        let tree = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
        let f = Face {
            paired: true,
            calls: Cell::new(0),
            bad_bold: false,
            invalid: false,
        };
        let l = Face {
            paired: false,
            calls: Cell::new(0),
            bad_bold: false,
            invalid: false,
        };
        assert_eq!(
            paint_direct_text_blocks(&tree, 100, 100, &f)
                .unwrap()
                .rasterize(100, 100, Rgb(255, 255, 255))
                .unwrap(),
            paint_direct_text_blocks(&tree, 100, 100, &l)
                .unwrap()
                .rasterize(100, 100, Rgb(255, 255, 255))
                .unwrap()
        );
        assert_eq!(f.calls.get(), 0, "{value}");
    }
}
