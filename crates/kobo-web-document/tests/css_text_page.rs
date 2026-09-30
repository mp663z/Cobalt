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
    BoxTree::from_style(&parse_style_tree(
        format!("<!doctype html>{html}").as_bytes(),
        &[],
        &Limits::DEFAULT,
    ))
}

#[test]
fn single_text_paint_refuses_quirks() {
    let boxes = BoxTree::from_style(&parse_style_tree(
        b"<html style='height:100px'><body style='height:80px'><p style='height:20px'>ab</p></body></html>",
        &[],
        &Limits::DEFAULT,
    ));
    assert!(boxes.quirks);
    assert_eq!(
        paint_single_text_page(&boxes, 100, 100, &TestFace).err(),
        Some(PageError::Unsupported)
    );
}

#[test]
fn whole_single_text_page_paints_background_before_measured_glyph() {
    let boxes = tree("<html style='height:100px'><body style='padding-top:2px'><p style='padding-left:5px;padding-top:3px;background-color:#abcdef'>ab</p></body></html>");
    let list = paint_single_text_page(&boxes, 100, 100, &TestFace).unwrap();
    assert_eq!(list.commands().len(), 3);
    assert!(matches!(
        &list.commands()[0],
        Command::Fill {
            color: Rgb(0xab, 0xcd, 0xef),
            ..
        }
    ));
    assert!(matches!(&list.commands()[1], Command::GlyphRun { .. }));
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 100 + x) * 4..(y * 100 + x) * 4 + 4];
    assert_eq!(at(5, 9), &[0, 0, 0, 255]);
    assert_eq!(at(6, 9), &[0xab, 0xcd, 0xef, 255]);
    assert_eq!(at(5, 4), &[0xab, 0xcd, 0xef, 255]);
    assert_eq!(at(0, 0), &[255, 255, 255, 255]);
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
        "<html style='height:100px;background-color:#123456'><body><p style='border:1px solid red'>hi</p></body></html>",
        "<html style='height:100px'><body><p style='margin-top:10px'>hi</p></body></html>",
    ] {
        assert_eq!(
            paint_single_text_page(&tree(html), 100, 100, &face).err(),
            Some(PageError::Unsupported),
            "{html}"
        );
    }
    assert_eq!(
        paint_single_text_page(
            &tree(
                "<html style='height:100px;background-color:#123456'><body><p>hi</p></body></html>"
            ),
            100,
            100,
            &Missing
        )
        .err(),
        Some(PageError::Unsupported)
    );
}

#[test]
fn direct_text_canvas_background_propagation_and_paint_order() {
    for (root_style, body_style, canvas, body_fill) in [
        ("background-color:#123456", "", Rgb(18, 52, 86), false),
        ("", "background-color:#abcdef", Rgb(171, 205, 239), false),
        (
            "background-color:#123456",
            "background-color:#abcdef",
            Rgb(18, 52, 86),
            true,
        ),
    ] {
        let boxes = tree(&format!("<html style='height:100px;{root_style}'><body style='{body_style}'><p style='background-color:#fedcba;color:#010203'>ab</p></body></html>"));
        let list = paint_single_text_page(&boxes, 100, 100, &TestFace).unwrap();
        assert!(
            matches!(list.commands()[0], Command::Fill { color, rect, .. }
            if color == canvas && rect.width == 100 && rect.height == 100)
        );
        assert_eq!(list.commands().len(), if body_fill { 5 } else { 4 });
        let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
        assert_eq!(
            &pixels[(99 * 100 + 99) * 4..][..3],
            &[canvas.0, canvas.1, canvas.2]
        );
        assert_eq!(&pixels[0..3], &[254, 220, 186]);
        // TestFace glyph begins four pixels below the content origin.
        assert_eq!(&pixels[(4 * 100) * 4..][..3], &[1, 2, 3]);
    }
}

#[test]
fn malformed_arena_is_refused_before_geometry_passes() {
    for defect in 0..7 {
        let mut boxes = tree("<html style='height:100px'><body><p>hi</p></body></html>");
        match defect {
            0 => boxes.boxes[0].children = vec![usize::MAX],
            1 => boxes.boxes[1].parent = Some(usize::MAX),
            2 => boxes.boxes[1].children.push(0),
            3 => {
                let child = boxes.boxes[0].children[0];
                boxes.boxes[0].children.push(child);
            }
            4 => boxes.boxes[0].children.clear(),
            5 => boxes.boxes[0].source = None,
            _ => boxes.roots[0] = usize::MAX,
        }
        assert!(
            paint_single_text_page(&boxes, 100, 100, &TestFace).is_err(),
            "defect {defect}"
        );
    }
}
