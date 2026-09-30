use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, paint_single_text_page, FontProvider, PageError},
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
        assert!(paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err());
        assert!(
            paint_single_text_page(&boxes, 100, 100, &TestFace).is_err(),
            "defect {defect}"
        );
    }
}

#[test]
fn multiple_direct_text_blocks_stack_and_paint_in_document_order() {
    let boxes = tree("<html style='height:100px;background-color:#123456'><body><p style='background-color:#abcdef;color:#010203'>ab</p><section style='padding-top:2px'><p style='background-color:#fedcba;color:#040506'>cd</p></section></body></html>");
    assert!(paint_single_text_page(&boxes, 100, 100, &TestFace).is_err());
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    assert_eq!(list.commands().len(), 7);
    assert!(matches!(
        list.commands()[1],
        Command::Fill {
            color: Rgb(171, 205, 239),
            ..
        }
    ));
    assert!(matches!(
        list.commands()[2],
        Command::Fill {
            color: Rgb(254, 220, 186),
            ..
        }
    ));
    assert!(matches!(
        list.commands()[3],
        Command::GlyphRun {
            color: Rgb(1, 2, 3),
            ..
        }
    ));
    assert!(matches!(
        list.commands()[5],
        Command::GlyphRun {
            color: Rgb(4, 5, 6),
            ..
        }
    ));
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |y: usize| &pixels[y * 100 * 4..][..3];
    assert_eq!(at(4), &[1, 2, 3]);
    assert_eq!(at(10), &[18, 52, 86]);
    assert_eq!(at(12), &[254, 220, 186]);
    assert_eq!(at(16), &[4, 5, 6]);
}

#[test]
fn multiple_text_blocks_refuse_unsupported_sibling_without_partial_page() {
    for tail in [
        "<p>emoji🙂</p>",
        "<p>hi <em>there</em></p>",
        "<p style='border:1px solid red'>hi</p>",
    ] {
        let boxes = tree(&format!("<html style='height:100px;background-color:#123456'><body><p>ab</p>{tail}</body></html>"));
        assert!(paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err());
    }
}

#[test]
fn block_indentation_and_trailing_newline_do_not_create_inline_boxes() {
    let compact = tree("<html style='height:100px;background-color:#123456'><body><p>ab</p><p>cd</p></body></html>");
    let spaced = tree("<html style='height:100px;background-color:#123456'>\n<body> \t<p>ab</p>\r\n<p>cd</p>\n</body></html>\n");
    let first = paint_direct_text_blocks(&compact, 100, 100, &TestFace).unwrap();
    let second = paint_direct_text_blocks(&spaced, 100, 100, &TestFace).unwrap();
    assert_eq!(
        first.rasterize(100, 100, Rgb(255, 255, 255)).unwrap(),
        second.rasterize(100, 100, Rgb(255, 255, 255)).unwrap()
    );
    assert_eq!(compact.boxes.len(), spaced.boxes.len());
    for ink in ["&nbsp;", "words", "<em>inline</em>", "&#12;"] {
        let boxes = tree(&format!(
            "<html style='height:100px'><body><p>ab</p>{ink}<p>cd</p></body></html>"
        ));
        assert!(
            paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err(),
            "{ink}"
        );
    }
    let pre = tree("<html style='height:100px'><body style='white-space:pre'><p>ab</p>\n<p>cd</p></body></html>");
    assert!(paint_direct_text_blocks(&pre, 100, 100, &TestFace).is_err());
}

#[test]
fn horizontal_margins_position_background_and_text_together() {
    for (style, x, width) in [
        ("width:20px;margin-left:auto;margin-right:auto", 40, 20),
        ("width:20px;margin-left:7px", 7, 20),
        ("margin-left:10px;margin-right:15px", 10, 75),
        ("width:20px;margin-left:10%", 10, 20),
    ] {
        let boxes = tree(&format!("<html style='height:100px;background-color:#123456'><body><p style='{style};background-color:#abcdef;color:#010203'>ab</p></body></html>"));
        let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
        assert!(
            matches!(list.commands()[1], Command::Fill { rect, .. } if rect.x == x && rect.width == width)
        );
        assert!(matches!(list.commands()[2], Command::GlyphRun { bounds, .. } if bounds.x == x));
        let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
        let offset = (4 * 100 + usize::try_from(x).unwrap()) * 4;
        assert_eq!(&pixels[offset..][..3], &[1, 2, 3]);
    }
}

#[test]
fn direction_and_visible_list_markers_are_not_silently_omitted() {
    for html in [
        "<html style='height:100px'><body><p style='direction:rtl'>ab</p></body></html>",
        "<html style='height:100px;direction:rtl'><body><p>ab</p></body></html>",
        "<html style='height:100px'><body><ul><li>ab</li></ul></body></html>",
        "<html style='height:100px'><body><p>ab</p><li style='height:10px'></li></body></html>",
    ] {
        let boxes = tree(html);
        assert!(
            paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err(),
            "{html}"
        );
    }
    // A child explicitly resetting to LTR has left-origin line placement.
    let boxes = tree("<html style='height:100px;direction:rtl'><body><p style='direction:ltr'>ab</p></body></html>");
    assert!(paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_ok());
}

#[test]
fn currentcolor_reaches_canvas_and_local_background_pixels() {
    let boxes = tree("<html style='height:100px;background:currentcolor;color:#123456'><body><p style='color:#abcdef;background:currentcolor'>ab</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&pixels[0..3], &[171, 205, 239]);
    assert_eq!(&pixels[99 * 100 * 4..][..3], &[18, 52, 86]);
}

#[test]
fn foreground_currentcolor_reaches_glyph_pixels() {
    let boxes = tree("<html style='height:100px;color:#123456'><body><p style='color:red;color:currentcolor;background:white'>ab</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&pixels[4 * 100 * 4..][..3], &[18, 52, 86]);
}

#[test]
fn single_line_punctuation_paints_but_unsupported_wrap_refuses_whole_page() {
    let boxes =
        tree("<html style='height:100px'><body><p style='color:#123456'>Hi, ab.</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    assert_eq!(list.commands().len(), 6);
    let narrow =
        tree("<html style='height:100px'><body><p style='width:10px'>Hi, ab.</p></body></html>");
    assert!(paint_direct_text_blocks(&narrow, 100, 100, &TestFace).is_err());
}

#[test]
fn explicit_pixel_line_height_sets_geometry_and_half_leading() {
    let boxes = tree("<html style='height:100px'><body style='line-height:14px'><p style='width:6px'>ab cd</p><p>ef</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    for y in [6, 20, 34] {
        assert_eq!(&pixels[y * 100 * 4..][..3], &[0, 0, 0]);
    }
    for value in ["9px", "11px"] {
        let boxes = tree(&format!(
            "<html style='height:100px'><body><p style='line-height:{value}'>ab</p></body></html>"
        ));
        assert!(paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err());
    }
}

#[test]
fn unitless_line_height_paints_and_refuses_fractional_used_values() {
    let boxes = tree("<html style='height:100px'><body style='line-height:1.5'><p style='font-size:12px'>ab</p><p style='font-size:16px'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    for y in [8, 29] {
        assert_eq!(&pixels[y * 100 * 4..][..3], &[0, 0, 0]);
    }
    let boxes = tree("<html style='height:100px'><body><p style='font-size:13px;line-height:1.5'>ab</p></body></html>");
    assert!(paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err());
}

#[test]
fn percent_line_height_inherits_as_length_not_multiplier() {
    let boxes = tree("<html style='height:100px'><body style='font-size:20px;line-height:150%'><p style='font-size:12px'>ab</p><p style='font-size:16px'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    for y in [14, 44] {
        assert_eq!(&pixels[y * 100 * 4..][..3], &[0, 0, 0]);
    }
}

#[test]
fn em_and_percent_line_height_produce_same_complete_raster() {
    let page = |height: &str| {
        tree(&format!("<html style='height:100px'><body style='font-size:20px;line-height:{height}'><p style='font-size:12px'>ab</p><p style='font-size:16px'>cd</p></body></html>"))
    };
    let em = paint_direct_text_blocks(&page("1.5em"), 100, 100, &TestFace).unwrap();
    let percent = paint_direct_text_blocks(&page("150%"), 100, 100, &TestFace).unwrap();
    assert_eq!(
        em.rasterize(100, 100, Rgb(255, 255, 255)).unwrap(),
        percent.rasterize(100, 100, Rgb(255, 255, 255)).unwrap()
    );
}

#[test]
fn percentage_font_size_and_explicit_pixels_paint_the_same_page() {
    let page = |size: &str| {
        tree(&format!("<html style='height:100px'><body style='font-size:20px'><p style='font-size:{size};line-height:120%'>ab</p></body></html>"))
    };
    let percent = paint_direct_text_blocks(&page("150%"), 100, 100, &TestFace).unwrap();
    let pixels = paint_direct_text_blocks(&page("30px"), 100, 100, &TestFace).unwrap();
    assert_eq!(
        percent.rasterize(100, 100, Rgb(255, 255, 255)).unwrap(),
        pixels.rasterize(100, 100, Rgb(255, 255, 255)).unwrap()
    );
}

#[test]
fn shorter_even_line_height_uses_negative_half_leading_when_ink_fits() {
    let boxes = tree("<html style='height:100px'><body style='line-height:8px'><p style='width:6px'>ab cd</p><p>ef</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    for y in [3, 11, 19] {
        assert_eq!(&pixels[y * 100 * 4..][..3], &[0, 0, 0]);
    }
    // Odd half-leading and too-short boxes remain honest refusals, even
    // though CSS in a general browser allows glyph overflow outside lines.
    for value in ["9px", "2px"] {
        let boxes = tree(&format!(
            "<html style='height:100px'><body><p style='line-height:{value}'>ab</p></body></html>"
        ));
        assert!(paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err());
    }
}

#[test]
fn definite_text_block_height_keeps_extra_room_before_the_next_sibling() {
    let boxes = tree("<html style='height:100px'><body><p style='height:30px;background-color:#abcdef'>ab</p><p style='background-color:#fedcba'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 100 + x) * 4..][..3];
    assert_eq!(at(0, 4), &[0, 0, 0]);
    assert_eq!(at(0, 29), &[0xab, 0xcd, 0xef]);
    assert_eq!(at(0, 30), &[0xfe, 0xdc, 0xba]);
    assert_eq!(at(0, 34), &[0, 0, 0]);
}

#[test]
fn zero_and_indefinite_text_heights_remain_refused() {
    for style in ["height:0px", "height:50%"] {
        let boxes = tree(&format!(
            "<html style='height:100px'><body><p style='{style}'>ab cd</p></body></html>"
        ));
        assert!(
            paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err(),
            "{style}"
        );
    }
}

#[test]
fn positive_adjoining_sibling_margins_collapse_to_the_larger_gap() {
    let boxes = tree("<html style='height:100px;background-color:#123456'><body><p style='margin-bottom:7px;background-color:#abcdef'>ab</p><p style='margin-top:4px;background-color:#fedcba'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |y: usize| &pixels[(y * 100 + 1) * 4..][..3];
    assert_eq!(at(9), &[0xab, 0xcd, 0xef]);
    for y in 10..17 {
        assert_eq!(at(y), &[0x12, 0x34, 0x56]);
    }
    assert_eq!(at(17), &[0xfe, 0xdc, 0xba]);
    assert_eq!(&pixels[(21 * 100) * 4..][..3], &[0, 0, 0]);
}

#[test]
fn parent_edge_vertical_margins_remain_refused() {
    for html in [
        "<html style='height:100px'><body><p style='margin-top:4px'>ab</p></body></html>",
        "<html style='height:100px'><body><p style='margin-bottom:4px'>ab</p></body></html>",
        "<html style='height:100px'><body><p style='margin-top:-4px'>ab</p></body></html>",
        "<html style='height:100px'><body><p style='margin-bottom:-4px'>ab</p></body></html>",
    ] {
        assert!(paint_direct_text_blocks(&tree(html), 100, 100, &TestFace).is_err());
    }
}

#[test]
fn negative_sibling_margins_overlap_backgrounds_but_keep_text_on_top() {
    // First block paints rows 0..10; the -6px gap pulls the second block's
    // background up to row 4, covering the first block's lower background.
    // CSS paints inline content above later block backgrounds, so the first
    // block's ink at rows 4..6 stays visible inside the overlap.
    let boxes = tree("<html style='height:100px;background-color:#123456'><body><p style='margin-bottom:-6px;background-color:#abcdef'>ab</p><p style='background-color:#fedcba'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let commands = list.commands();
    let last_fill = commands
        .iter()
        .rposition(|command| matches!(command, Command::Fill { .. }))
        .unwrap();
    let first_glyph = commands
        .iter()
        .position(|command| matches!(command, Command::GlyphRun { .. }))
        .unwrap();
    assert!(last_fill < first_glyph);
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 100 + x) * 4..][..3];
    assert_eq!(at(1, 2), &[0xab, 0xcd, 0xef]);
    assert_eq!(at(0, 4), &[0, 0, 0]);
    assert_eq!(at(0, 5), &[0, 0, 0]);
    assert_eq!(at(1, 4), &[0xfe, 0xdc, 0xba]);
    assert_eq!(at(1, 12), &[0xfe, 0xdc, 0xba]);
    assert_eq!(at(1, 14), &[0x12, 0x34, 0x56]);
    assert_eq!(at(0, 8), &[0, 0, 0]);
}

#[test]
fn large_negative_sibling_margin_clips_above_the_canvas() {
    // A -12px gap pulls the second block's background up to row -2, partly
    // above the canvas; the rasterizer clips it and painting still succeeds.
    let boxes = tree("<html style='height:100px;background-color:#123456'><body style='height:100px'><p style='background-color:#abcdef'>ab</p><p style='margin-top:-12px;background-color:#fedcba'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 100 + x) * 4..][..3];
    assert_eq!(at(1, 0), &[0xfe, 0xdc, 0xba]);
    assert_eq!(at(1, 7), &[0xfe, 0xdc, 0xba]);
    assert_eq!(at(1, 8), &[0xab, 0xcd, 0xef]);
    assert_eq!(at(0, 4), &[0, 0, 0]);
}

#[test]
fn sibling_percentage_gaps_use_containing_width_not_height_or_child_width() {
    let boxes = tree("<html style='height:100px;background-color:#123456'><body style='width:80px'><p style='width:20px;margin-bottom:10%;background-color:#abcdef'>ab</p><p style='margin-top:5%;background-color:#fedcba'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    for y in 10..18 {
        assert_eq!(&pixels[(y * 100 + 1) * 4..][..3], &[0x12, 0x34, 0x56]);
    }
    assert_eq!(&pixels[(18 * 100 + 1) * 4..][..3], &[0xfe, 0xdc, 0xba]);
    // An exact negative percentage gap is supported: -10% of the 80px body
    // width pulls the second block's background up 8px over the first.
    let boxes = tree("<html style='height:100px;background-color:#123456'><body style='width:80px'><p style='margin-bottom:-10%;background-color:#abcdef'>ab</p><p style='background-color:#fedcba'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&pixels[(2 * 100 + 1) * 4..][..3], &[0xfe, 0xdc, 0xba]);
    assert_eq!(&pixels[(11 * 100 + 1) * 4..][..3], &[0xfe, 0xdc, 0xba]);
    assert_eq!(&pixels[(12 * 100 + 1) * 4..][..3], &[0x12, 0x34, 0x56]);
    for value in ["3%", "-3%", "auto"] {
        let boxes = tree(&format!("<html style='height:100px'><body style='width:80px'><p style='margin-bottom:{value}'>ab</p><p>cd</p></body></html>"));
        assert!(
            paint_direct_text_blocks(&boxes, 100, 100, &TestFace).is_err(),
            "{value}"
        );
    }
}

struct OverhangFace;
impl FontProvider for OverhangFace {
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
                left: -1,
                top: -3,
                width: 5,
                height: 2,
                coverage: vec![255; 10],
            }
        })
    }
}

#[test]
fn horizontal_ink_overflows_content_without_changing_wrapped_geometry() {
    let boxes = tree("<html><body style='padding-left:4px'><p style='width:3px;background-color:#abcdef'>T T</p><p style='width:3px;background-color:#fedcba'>T</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 20, 40, &OverhangFace).unwrap();
    let glyph_bounds: Vec<_> = list
        .commands()
        .iter()
        .filter_map(|cmd| match cmd {
            Command::GlyphRun { bounds, .. } => Some((bounds.x, bounds.y, bounds.width)),
            _ => None,
        })
        .collect();
    // Advances fit the width and create two lines. Ink overhangs both edges;
    // it does not expand the line box or move the following block down.
    assert_eq!(glyph_bounds, [(3, 4, 5), (3, 14, 5), (3, 24, 5)]);
    let pixels = list.rasterize(20, 40, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4];
    assert_eq!(at(3, 4), [0, 0, 0, 255]);
    assert_eq!(at(7, 4), [0, 0, 0, 255]);
    assert_eq!(at(8, 4), [255, 255, 255, 255]);
    assert_eq!(at(4, 20), [254, 220, 186, 255]);
}

#[test]
fn horizontal_ink_at_viewport_edge_clips_only_when_rasterized() {
    let boxes = tree("<html><body><p style='width:3px'>T</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 3, 20, &OverhangFace).unwrap();
    assert!(
        matches!(&list.commands()[0], Command::GlyphRun { bounds, .. } if bounds.x == -1 && bounds.width == 5)
    );
    let pixels = list.rasterize(3, 20, Rgb(255, 255, 255)).unwrap();
    assert_eq!(&pixels[4 * 3 * 4..5 * 3 * 4], &[0, 0, 0, 255].repeat(3));
}

#[test]
fn horizontal_overhang_does_not_permit_unbreakable_words_or_hidden_overflow() {
    for html in [
        "<html><body><p style='width:3px'>TT</p></body></html>",
        "<html><body><p style='width:3px;overflow:hidden'>T</p></body></html>",
    ] {
        assert_eq!(
            paint_direct_text_blocks(&tree(html), 20, 40, &OverhangFace).err(),
            Some(PageError::Unsupported)
        );
    }
}

#[test]
fn short_definite_height_keeps_background_and_following_sibling_in_flow() {
    let boxes = tree("<html><body><p style='height:3px;background-color:#abcdef'>ab</p><p style='background-color:#fedcba'>cd</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 100 + x) * 4..][..3];
    assert_eq!(at(1, 2), &[0xab, 0xcd, 0xef]);
    assert_eq!(at(1, 3), &[0xfe, 0xdc, 0xba]);
    // Earlier text overflows its short box and stays above later backgrounds.
    assert_eq!(at(0, 4), &[0, 0, 0]);
    assert_eq!(at(0, 7), &[0, 0, 0]);
    assert_eq!(at(1, 13), &[255, 255, 255]);
}

#[test]
fn wrapped_text_overflows_definite_height_without_inflating_ancestor_height() {
    let boxes = tree("<html><body style='background-color:#123456'><div style='width:6px;background-color:#abcdef'><p style='height:5px'>ab cd</p></div><p style='background-color:#fedcba'>ef</p></body></html>");
    let list = paint_direct_text_blocks(&boxes, 100, 100, &TestFace).unwrap();
    let glyph_y: Vec<_> = list
        .commands()
        .iter()
        .filter_map(|cmd| match cmd {
            Command::GlyphRun { bounds, .. } => Some(bounds.y),
            _ => None,
        })
        .collect();
    assert_eq!(glyph_y, [4, 4, 14, 14, 9, 9]);
    let pixels = list.rasterize(100, 100, Rgb(255, 255, 255)).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 100 + x) * 4..][..3];
    assert_eq!(at(1, 4), &[0xab, 0xcd, 0xef]);
    assert_eq!(at(1, 5), &[0xfe, 0xdc, 0xba]);
    assert_eq!(at(0, 14), &[0, 0, 0]);
    assert_eq!(at(1, 15), &[0x12, 0x34, 0x56]);
}

#[test]
fn definite_height_overflow_does_not_enable_unproven_clipping() {
    for overflow in ["hidden", "scroll", "auto"] {
        let boxes = tree(&format!(
            "<html><body><p style='height:3px;overflow:{overflow}'>ab</p></body></html>"
        ));
        assert_eq!(
            paint_direct_text_blocks(&boxes, 100, 100, &TestFace).err(),
            Some(PageError::Unsupported)
        );
    }
}
