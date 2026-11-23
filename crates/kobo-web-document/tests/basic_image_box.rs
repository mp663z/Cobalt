//! `<img>` is a real box kind (BoxKind::Image), not an intake byte rewrite.
//! The box carries marker geometry (padding-top = scaled intrinsic height,
//! sentinel background the harness swaps for decoded pixels) and its alt
//! text as the pre-blit paint; `images_in_order` feeds the blit step.
use kobo_web_document::{
    basic_page::{normalize_for_basic, paint_basic_page},
    box_tree::{BoxKind, BoxTree},
    css_text_page::FontProvider,
    display_list::{Command, Rgb},
    inline_lines::GlyphBitmap,
    Limits, SupplyBudget,
};

struct Face;
impl FontProvider for Face {
    fn has_glyph(&self, c: char, _: bool) -> Option<bool> {
        Some(c.is_ascii())
    }
    fn advance(&self, _: char, s: u32, _: bool) -> Option<u32> {
        Some(s / 2)
    }
    fn line_height(&self, s: u32, _: bool) -> Option<u32> {
        Some(s + 4)
    }
    fn baseline_offset(&self, s: u32, _: bool) -> Option<i32> {
        i32::try_from(s).ok()
    }
    fn raster(&self, c: char, s: u32, _: bool) -> Option<GlyphBitmap> {
        c.is_ascii().then(|| GlyphBitmap {
            left: 0,
            top: -(s as i32),
            width: s / 2,
            height: s,
            coverage: vec![255; (s * s / 2) as usize],
        })
    }
}

const PNG_2X4: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAAEAAAAAA==";
const PNG_1600X800: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAABkAAAAMgAAAAAA==";

fn tree(body: &str) -> BoxTree {
    let html = format!("<!doctype html><style>body{{margin:0}}</style>{body}");
    let bytes = normalize_for_basic(html.as_bytes());
    let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        &bytes,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    BoxTree::from_style(&styled)
}

#[test]
fn img_is_an_image_box_with_marker_geometry() {
    let t = tree(&format!("<div><img src=\"{PNG_2X4}\" alt=\"A\"></div>"));
    let img = t
        .boxes
        .iter()
        .find(|b| b.kind == BoxKind::Image)
        .expect("image box");
    // 2x4 intrinsic, narrower than the 800px content width: unscaled.
    assert_eq!(img.style.padding_top, 4);
    // Sentinel for document-order image 0 is rgb(1,0,0).
    assert_eq!(img.style.used_background_color(), Some(0x0001_0000));
    // The alt text rides along as the pre-blit paint.
    assert_eq!(img.children.len(), 1);
    assert_eq!(t.boxes[img.children[0]].text.as_deref(), Some("A"));
    assert_eq!(t.images_in_order(), vec![PNG_2X4.as_bytes().to_vec()]);
}

#[test]
fn wide_images_scale_to_the_content_width() {
    let t = tree(&format!("<div><img src=\"{PNG_1600X800}\"></div>"));
    let img = t
        .boxes
        .iter()
        .find(|b| b.kind == BoxKind::Image)
        .expect("image box");
    // 1600x800 scaled to 800 wide: height 400.
    assert_eq!(img.style.padding_top, 400);
    // No alt: no pre-blit text child.
    assert!(img.children.is_empty());
}

#[test]
fn img_without_data_uri_falls_back_to_alt_text() {
    let t =
        tree("<div><p>x<img src=\"https://example.invalid/a.png\" alt=\"alt text\">y</p></div>");
    assert!(t.boxes.iter().all(|b| b.kind != BoxKind::Image));
    assert!(t.images_in_order().is_empty());
    assert!(t
        .boxes
        .iter()
        .any(|b| b.kind == BoxKind::Text && b.text.as_deref() == Some("alt text")));
}

#[test]
fn picture_with_qualifying_img_is_replaced_by_the_image_box() {
    let t = tree(&format!(
        "<div><a href=\"#x\"><picture><img src=\"{PNG_2X4}\"></picture></a></div>"
    ));
    assert_eq!(
        t.boxes.iter().filter(|b| b.kind == BoxKind::Image).count(),
        1
    );
    assert_eq!(t.images_in_order().len(), 1);
}

#[test]
fn image_box_paints_the_sentinel_fill_for_the_blit_step() {
    let html = format!(
        "<!doctype html><style>body{{margin:0;font-size:16px}}</style><div><img src=\"{PNG_2X4}\" alt=\"A\"></div>"
    );
    let bytes = normalize_for_basic(html.as_bytes());
    let styled = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        &bytes,
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    let (commands, _stats, _height) =
        paint_basic_page(&BoxTree::from_style(&styled), 300, 500, &Face).unwrap();
    assert!(commands.iter().any(|c| matches!(
        c,
        Command::Fill {
            color: Rgb(1, 0, 0),
            ..
        }
    )));
}


#[test]
fn extreme_image_attributes_are_bounded_without_overflow() {
    for (width, height, expected) in [(801_u32, u32::MAX, 4096_u32), (u32::MAX, u32::MAX, 800)] {
        let t = tree(&format!(
            "<img src='{PNG_2X4}' width='{width}' height='{height}'>"
        ));
        let img = t.boxes.iter().find(|b| b.kind == BoxKind::Image).unwrap();
        assert_eq!(img.style.padding_top, expected);
    }
}
