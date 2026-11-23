//! PRIVATE, unadopted host measurement (FL53): end-to-end uniform-family path with REAL faces.
//! Real Liberation2 Sans R/B bytes (host files, NOT bundled) behind an explicit catalog whose hashes are
//! computed from the bytes it loaded and then checked against the separately measured pinned profile.
//! Usage: family_uniform page.html out.rgba width height   (compares against the family-bound painter)
use fontdue::{Font, FontSettings};
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks_family_bound, FamilyProfile, FontProvider},
    display_list::Rgb,
    family_page::{paint_uniform_family_page, FamilyFaces},
    font_family::{FaceCatalog, FaceCoverage, FamilyId, Generic, Lookup},
    inline_lines::GlyphBitmap,
    parse_style_tree_with_pinned_font_family, Limits, SupplyBudget,
};
use std::fs;
struct Pair { reg: Font, bold: Font, reg_sha: String, bold_sha: String }
impl Pair { fn f(&self, bold: bool) -> &Font { if bold { &self.bold } else { &self.reg } } }
impl FontProvider for Pair {
    fn face_sha256_hex(&self) -> Option<String> { Some(self.reg_sha.clone()) }
    fn bold_face_sha256_hex(&self) -> Option<String> { Some(self.bold_sha.clone()) }
    fn has_glyph(&self, c: char, bold: bool) -> Option<bool> { Some(self.f(bold).lookup_glyph_index(c) != 0) }
    fn advance(&self, c: char, size: u32, bold: bool) -> Option<u32> {
        let f = self.f(bold);
        if f.lookup_glyph_index(c) == 0 { return None; }
        Some(f.metrics(c, size as f32).advance_width.round().max(1.0) as u32)
    }
    fn line_height(&self, size: u32, bold: bool) -> Option<u32> {
        let m = self.f(bold).horizontal_line_metrics(size as f32)?;
        Some((m.ascent.round() + (-m.descent).round() + m.line_gap.round()) as u32)
    }
    fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32> {
        Some(self.f(bold).horizontal_line_metrics(size as f32)?.ascent.round() as i32)
    }
    fn raster(&self, c: char, size: u32, bold: bool) -> Option<GlyphBitmap> {
        let f = self.f(bold);
        if f.lookup_glyph_index(c) == 0 { return None; }
        let (m, cov) = f.rasterize(c, size as f32);
        Some(GlyphBitmap { left: m.xmin, top: -(m.ymin + m.height as i32), width: m.width as u32, height: m.height as u32, coverage: cov })
    }
}
/// One explicit family (id 1). Hashes are those of the bytes it was built from. HOST-ONLY claims, not CSS or device
/// rules: `arial` is Available(id 1) as a measured fontconfig alias (fc-match arial -> LiberationSans-Regular.ttf;
/// Chrome 156 renders arial identically to Liberation Sans, ab84); `gds transport` is Absent per the host font
/// inventory (no matching family in fc-list, ab84/inventory.txt), also rendering like an invented name. Every other
/// name is Unknown (refuse). `sans-serif` maps to this face; no other generic is mapped.
struct Catalog { face: Pair, reg_sha: String, bold_sha: String }
impl FaceCatalog for Catalog {
    fn lookup(&self, n: &str) -> Lookup {
        match n { "arial" => Lookup::Available(FamilyId(1)), "gds transport" => Lookup::Absent, _ => Lookup::Unknown }
    }
    fn generic(&self, g: Generic) -> Option<FamilyId> { (g == Generic::SansSerif).then_some(FamilyId(1)) }
}
impl FaceCoverage for Catalog {
    fn covers(&self, f: FamilyId, c: char, bold: bool) -> Option<bool> { (f.0 == 1).then(|| self.face.has_glyph(c, bold)).flatten() }
}
impl FamilyFaces for Catalog {
    fn face(&self, f: FamilyId) -> Option<&dyn FontProvider> { (f.0 == 1).then_some(&self.face as &dyn FontProvider) }
    fn face_hashes(&self, f: FamilyId) -> Option<(String, String)> { (f.0 == 1).then(|| (self.reg_sha.clone(), self.bold_sha.clone())) }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let base = "/usr/share/fonts/truetype/liberation2/";
    let rb = fs::read(format!("{base}LiberationSans-Regular.ttf"))?;
    let bb = fs::read(format!("{base}LiberationSans-Bold.ttf"))?;
    let (rs, bs) = (kobo_net::sha256::hex_digest(&rb), kobo_net::sha256::hex_digest(&bb));
    let pr = FamilyProfile::Chrome156HostLiberation2Sans;
    // Grounding: bytes loaded here must equal the SEPARATELY measured profile hashes, else stop.
    if rs != pr.regular_sha256_hex() || bs != pr.bold_sha256_hex() { return Err("loaded bytes differ from measured profile hashes".into()); }
    let mk = || -> Result<Pair, Box<dyn std::error::Error>> { Ok(Pair {
        reg: Font::from_bytes(rb.as_slice(), FontSettings::default())?, bold: Font::from_bytes(bb.as_slice(), FontSettings::default())?,
        reg_sha: rs.clone(), bold_sha: bs.clone() }) };
    let cat = Catalog { face: mk()?, reg_sha: rs.clone(), bold_sha: bs.clone() };
    let reference = mk()?;
    let html = fs::read(&a[1])?;
    let (w, h): (u32, u32) = (a[3].parse()?, a[4].parse()?);
    let s = parse_style_tree_with_pinned_font_family(&html, &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if s.unsupported { return Err(format!("intake refused: {:?}", s.diagnostics).into()); }
    let t = BoxTree::from_style(&s);
    let uni = paint_uniform_family_page(&t, s.families.as_ref().unwrap(), w, h, &cat).map_err(|e| format!("uniform refused: {e:?}"))?;
    let fb = paint_direct_text_blocks_family_bound(&t, w, h, &reference, pr).map_err(|e| format!("family-bound refused: {e:?}"))?;
    let (ru, rf) = (uni.rasterize(w, h, Rgb(255, 255, 255)).map_err(|e| format!("{e:?}"))?, fb.rasterize(w, h, Rgb(255, 255, 255)).map_err(|e| format!("{e:?}"))?);
    fs::write(&a[2], &ru)?;
    println!("uniform==family_bound raster: {} (cmds {} vs {}); reg {rs} bold {bs}", ru == rf, uni.commands().len(), fb.commands().len());
    Ok(())
}
