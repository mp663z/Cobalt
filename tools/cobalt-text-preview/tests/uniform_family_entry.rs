//! FL54: library entry over the accepted uniform-family path, SYNTHETIC catalog/provider (no real font bytes).
use cobalt_text_preview::{prepare_page_masked, prepare_page_uniform_family_masked};
use kobo_web_document::{
    css_text_page::FontProvider,
    family_page::FamilyFaces,
    font_family::{FaceCatalog, FaceCoverage, FamilyId, Generic, Lookup},
    inline_lines::GlyphBitmap,
};
struct Syn;
impl FontProvider for Syn {
    fn face_sha256_hex(&self) -> Option<String> { Some("r".into()) }
    fn bold_face_sha256_hex(&self) -> Option<String> { Some("b".into()) }
    fn has_glyph(&self, c: char, _: bool) -> Option<bool> { Some(c.is_ascii()) }
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(5) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(20) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(15) }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap { left: 0, top: -1, width: 2, height: 2, coverage: vec![255; 4] })
    }
}
struct Cat(Syn);
impl FaceCatalog for Cat {
    fn lookup(&self, n: &str) -> Lookup {
        match n { "arial" => Lookup::Available(FamilyId(1)), "gds transport" => Lookup::Absent, _ => Lookup::Unknown }
    }
    fn generic(&self, g: Generic) -> Option<FamilyId> { (g == Generic::SansSerif).then_some(FamilyId(1)) }
}
impl FaceCoverage for Cat {
    fn covers(&self, f: FamilyId, c: char, b: bool) -> Option<bool> { (f.0 == 1).then(|| self.0.has_glyph(c, b)).flatten() }
}
impl FamilyFaces for Cat {
    fn face(&self, f: FamilyId) -> Option<&dyn FontProvider> { (f.0 == 1).then_some(&self.0 as &dyn FontProvider) }
    fn face_hashes(&self, f: FamilyId) -> Option<(String, String)> { (f.0 == 1).then(|| ("r".into(), "b".into())) }
}
fn doc(fam: &str, body: &str) -> Vec<u8> {
    format!("<!doctype html><style>html,body,p,section{{margin:0}}p{{line-height:20px;font-size:16px}}{fam}</style>{body}").into_bytes()
}
const BODY: &str = "<section><p>Hello world</p><p style='font-weight:700'>Bold</p></section>";

#[test]
fn entry_equals_ordinary_masked_render_of_the_same_page_without_the_family() {
    for fam in ["section{font-family:sans-serif}", "section{font-family:\"GDS Transport\", arial, sans-serif}"] {
        let got = prepare_page_uniform_family_masked(&doc(fam, BODY), &[], 200, 80, &Cat(Syn), None).unwrap();
        let want = prepare_page_masked(&doc("", BODY), &[], 200, 80, &Syn, None).unwrap();
        assert_eq!(got.0, want.0, "{fam}");
        assert_eq!(got.1, want.1, "{fam}");
        assert!(!got.1.is_empty());
    }
}

#[test]
fn entry_refuses_what_the_parts_refuse() {
    let c = Cat(Syn);
    let ok = "section{font-family:sans-serif}";
    let r = |html: Vec<u8>, w: u32, h: u32| prepare_page_uniform_family_masked(&html, &[], w, h, &c, None).err();
    assert_eq!(r(doc("", BODY), 200, 80), Some("unsupported complete page")); // no family -> MissingKey
    assert_eq!(r(doc("section{font-family:verdana, sans-serif}", BODY), 200, 80), Some("unsupported complete page")); // unpinned list: intake
    assert_eq!(r(doc(ok, "<section><p>caf\u{e9}</p></section>"), 200, 80), Some("unsupported complete page"));
    assert_eq!(r(doc(&format!("{ok}p{{line-height:21px}}"), BODY), 200, 80), Some("unsupported complete page")); // odd leading, no gap proof
    assert_eq!(r(doc(ok, "<section><p><a href=x>link</a></p></section>"), 200, 80), Some("interactive page needs semantic fallback"));
    assert_eq!(r(doc(ok, BODY), 200, 30), Some("page needs scrolling or pagination"));
    assert_eq!(r(doc(ok, BODY), 0, 80), Some("invalid room budget"));
    assert_eq!(r(vec![b'a'; 3 * 1024 * 1024], 200, 80), Some("input budget"));
}

#[test]
fn catalog_mismatch_and_mixed_families_refuse_through_the_entry() {
    struct Bad(Cat);
    impl FaceCatalog for Bad {
        fn lookup(&self, n: &str) -> Lookup { self.0.lookup(n) }
        fn generic(&self, g: Generic) -> Option<FamilyId> { self.0.generic(g) }
    }
    impl FaceCoverage for Bad { fn covers(&self, f: FamilyId, c: char, b: bool) -> Option<bool> { self.0.covers(f, c, b) } }
    impl FamilyFaces for Bad {
        fn face(&self, f: FamilyId) -> Option<&dyn FontProvider> { self.0.face(f) }
        fn face_hashes(&self, _: FamilyId) -> Option<(String, String)> { Some(("wrong".into(), "b".into())) }
    }
    let ok = "section{font-family:sans-serif}";
    assert_eq!(prepare_page_uniform_family_masked(&doc(ok, BODY), &[], 200, 80, &Bad(Cat(Syn)), None).err(), Some("unsupported complete page"));
}

#[test]
fn media_width_is_the_layout_width() {
    let c = Cat(Syn);
    let css = "@media (min-width:150px){section{font-family:sans-serif}}";
    assert!(prepare_page_uniform_family_masked(&doc(css, BODY), &[], 200, 80, &c, None).is_ok());
    assert!(prepare_page_uniform_family_masked(&doc(css, BODY), &[], 100, 80, &c, None).is_err()); // rule skipped at 100px -> no family
}
