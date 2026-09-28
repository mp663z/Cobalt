use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_single_text_page, FontProvider, PageError},
    display_list::{Command, Rgb},
    inline_lines::GlyphBitmap,
    parse_style_tree, Limits,
};

struct TestFace;
impl FontProvider for TestFace {
    fn advance(&self, _: char, _: u32) -> Option<u32> {
        Some(3)
    }
    fn line_height(&self, _: u32) -> Option<u32> {
        Some(10)
    }
    fn baseline_offset(&self, _: u32) -> Option<i32> {
        Some(7)
    }
    fn raster(&self, character: char, _: u32) -> Option<GlyphBitmap> {
        Some(if character == ' ' {
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
                top: -3,
                width: 1,
                height: 2,
                coverage: vec![255, 255],
            }
        })
    }
}
fn tree(html: &str) -> BoxTree {
    BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
}

#[test]
fn whole_single_text_page_paints_background_before_measured_glyph() {
    let boxes = tree("<html style='height:100px'><body style='padding-top:2px;background-color:#123456'><p style='padding-left:5px;padding-top:3px;background-color:#abcdef'>ab</p></body></html>");
    let list = paint_single_text_page(&boxes, 100, 100, &TestFace).unwrap();
    assert_eq!(list.commands().len(), 4);
    assert!(matches!(
        &list.commands()[0],
        Command::Fill {
            color: Rgb(0x12, 0x34, 0x56),
            ..
        }
    ));
    assert!(matches!(
        &list.commands()[1],
        Command::Fill {
            color: Rgb(0xab, 0xcd, 0xef),
            ..
        }
    ));
    assert!(matches!(&list.commands()[2], Command::GlyphRun { .. }));
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
    assert_eq!(at(5, 9), &[0, 0, 0, 255]);
    assert_eq!(at(6, 9), &[0xab, 0xcd, 0xef, 255]);
    assert_eq!(at(5, 4), &[0xab, 0xcd, 0xef, 255]);
    assert_eq!(at(0, 0), &[0x12, 0x34, 0x56, 255]);
}

struct Missing;
impl FontProvider for Missing {
    fn advance(&self, _: char, _: u32) -> Option<u32> {
        None
    }
    fn line_height(&self, _: u32) -> Option<u32> {
        Some(10)
    }
    fn baseline_offset(&self, _: u32) -> Option<i32> {
        Some(7)
    }
    fn raster(&self, _: char, _: u32) -> Option<GlyphBitmap> {
        None
    }
}

#[test]
fn refuses_branches_unknown_css_and_provider_failure_without_partial_page() {
    let face = TestFace;
    for html in [
        "<html style='height:100px'><body><p>first</p><p>second</p></body></html>",
        "<html style='height:100px'><body><p>hi <em>there</em></p></body></html>",
        "<html style='height:100px'><body><p style='border:1px solid red'>hi</p></body></html>",
        "<html style='height:100px'><body><p>emoji🙂</p></body></html>",
    ] {
        assert_eq!(
            paint_single_text_page(&tree(html), 100, 100, &face).err(),
            Some(PageError::Unsupported),
            "{html}"
        );
    }
    assert_eq!(
        paint_single_text_page(
            &tree("<html style='height:100px'><body><p>hi</p></body></html>"),
            100,
            100,
            &Missing
        )
        .err(),
        Some(PageError::Unsupported)
    );
}
