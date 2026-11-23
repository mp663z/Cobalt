//! FL71 constructed painter x provider isolation (synthetic pages; measurement-only; no admission change).
//! Rows = cases; columns = painter/provider combos: A single_text_page+Atkinson, B single_text_page+Liberation, C direct_text_blocks+Atkinson,
//! D direct_text_blocks+Liberation (all via cobalt_text_preview::LocalFace, parse_style_tree default entry), E family_bound(+strict shaped absent)+Liberation
//! (parse_style_tree_with_pinned_font_family, section{font-family:sans-serif}). Provider face bytes: Atkinson atk/pin.ttf, Liberation2 Sans Regular only
//! (LocalFace is regular-only here, so bold cases test the painter's refusal, not bold metrics).
use cobalt_text_preview::LocalFace;
use kobo_web_document::{box_tree::BoxTree, css_text_page::*, parse_style_tree, parse_style_tree_with_pinned_font_family, Limits, SupplyBudget};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let atk = LocalFace::from_bytes(&std::fs::read(&a[1]).unwrap()).unwrap();
    let lib_bytes = std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf").unwrap();
    let lib = LocalFace::from_bytes(&lib_bytes).unwrap();
    let lib_b = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSans-Bold.ttf").unwrap()).unwrap();
    let _ = &lib_b;
    let long = "The quick brown fox jumps over the lazy dog and keeps running through the long grass until evening falls.";
    let cases: Vec<(&str, String)> = vec![
        ("plain", format!("<p>{long}</p>")),
        ("plain_w280_lh20", format!("<p style='width:280px;line-height:20px'>{long}</p>")),
        ("bold_span", "<p>Hello <span style='font-weight:700'>bold</span> text</p>".into()),
        ("b_tag", "<p>Hello <b>bold</b> text</p>".into()),
        ("em_tag", "<p>Hello <em>it</em> text</p>".into()),
        ("two_p", "<p>One line</p><p>Two line</p>".into()),
        ("right", "<p style='text-align:right'>Hello world text</p>".into()),
        ("center", "<p style='text-align:center'>Hello world text</p>".into()),
        ("h1", "<h1>Title text</h1>".into()),
        ("longword", "<p style='width:120px'>Short supercalifragilisticexpialidocious end</p>".into()),
    ];
    let fmt = |r: Result<kobo_web_document::display_list::DisplayList, PageError>| match r { Ok(l) => format!("OK({})", l.commands().len()), Err(e) => format!("{e:?}") };
    println!("{:<18} {:<14} {:<14} {:<14} {:<14} {:<14}", "case", "A single+Atk", "B single+Lib", "C direct+Atk", "D direct+Lib", "E famBound+Lib");
    for (n, body) in &cases {
        let html = format!("<!DOCTYPE html><html><head><title>t</title></head><body>{body}</body></html>");
        let st = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        let t = BoxTree::from_style(&st);
        let ra = fmt(paint_single_text_page(&t, 600, 800, &atk));
        let rb = fmt(paint_single_text_page(&t, 600, 800, &lib));
        let rc = fmt(paint_direct_text_blocks(&t, 600, 800, &atk));
        let rd = fmt(paint_direct_text_blocks(&t, 600, 800, &lib));
        let html2 = format!("<!DOCTYPE html><html><head><title>t</title><style>section{{font-family:sans-serif}}</style></head><body><section>{body}</section></body></html>");
        let st2 = parse_style_tree_with_pinned_font_family(html2.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        let re = if st2.unsupported { format!("parse:{:?}", st2.diagnostics.first_reason()) } else {
            let t2 = BoxTree::from_style(&st2);
            let p = PairProv { r: &lib, sha: kobo_net::sha256::hex_digest(&lib_bytes) };
            fmt(paint_direct_text_blocks_family_bound(&t2, 600, 800, &p, FamilyProfile::Chrome156HostLiberation2Sans))
        };
        println!("{:<18} {:<14} {:<14} {:<14} {:<14} {:<14}", n, ra, rb, rc, rd, re);
    }
}
struct PairProv<'a> { r: &'a LocalFace, sha: String }
impl FontProvider for PairProv<'_> {
    fn face_sha256_hex(&self) -> Option<String> { Some(self.sha.clone()) }
    fn bold_face_sha256_hex(&self) -> Option<String> { Some(FamilyProfile::Chrome156HostLiberation2Sans.bold_sha256_hex().to_string()) }
    fn has_glyph(&self, c: char, bold: bool) -> Option<bool> { self.r.has_glyph(c, bold) }
    fn advance(&self, c: char, s: u32, b: bool) -> Option<u32> { self.r.advance(c, s, b) }
    fn line_height(&self, s: u32, b: bool) -> Option<u32> { self.r.line_height(s, b) }
    fn baseline_offset(&self, s: u32, b: bool) -> Option<i32> { self.r.baseline_offset(s, b) }
    fn raster(&self, c: char, s: u32, b: bool) -> Option<kobo_web_document::inline_lines::GlyphBitmap> { self.r.raster(c, s, b) }
}
