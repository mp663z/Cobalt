//! The legacy provider (no proven line gap) keeps refusing odd explicit leading. The gap-proven
//! rule is covered by the synthetic crate test odd_leading_gap and the private Chrome lh78 grid.
use cobalt_text_preview::LocalFace;
use kobo_web_document::{box_tree::BoxTree, css_text_page::{paint_direct_text_blocks, FontProvider}, display_list::Rgb, Limits};

fn ink_top(face: &LocalFace, size: u32, line_height: &str) -> Option<usize> {
    let html = format!("<!doctype html><body style='margin:0'><p style='margin:0;font-size:{size}px;line-height:{line_height};color:#000'>HHHH</p>");
    let styled = kobo_web_document::parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
    let tree = BoxTree::from_style(&styled);
    let list = paint_direct_text_blocks(&tree, 200, 80, face).ok()?;
    let rgba = list.rasterize(200, 80, Rgb(255, 255, 255)).ok()?;
    (0..80).find(|&y| (0..200).any(|x| rgba[(y * 200 + x) * 4] < 128))
}

#[test]
fn legacy_provider_still_refuses_odd_leading_and_admits_even() {
    let face = LocalFace::bundled_reader_face().unwrap();
    for size in [13u32, 16, 24] {
        let natural = i64::from(face.line_height(size, false).unwrap());
        for lh in (natural - 4)..=(natural + 5) {
            let top = ink_top(&face, size, &format!("{lh}px"));
            assert_eq!(top.is_some(), (lh - natural) % 2 == 0, "size {size} lh {lh} natural {natural}");
        }
    }
}
