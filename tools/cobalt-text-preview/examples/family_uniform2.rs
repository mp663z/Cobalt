//! PRIVATE, unadopted host measurement (FL55): REAL faces through the actual library entry
//! `prepare_page_uniform_family_masked`, with name/alias/absence/generic mappings validated AT RUNTIME by a headless
//! Chrome probe against the loaded bytes (pinned @font-face). A mapping that cannot be validated becomes Unknown
//! (refuse); nothing is assumed from the host. Real Liberation2 Sans R/B host files are loaded, not bundled, and
//! must equal the separately measured pinned profile hashes.
//! Usage: family_uniform2 page.html out.bin width height
use cobalt_text_preview::{prepare_page_masked, prepare_page_uniform_family_masked};
use fontdue::{Font, FontSettings};
use kobo_web_document::{
    css_text_page::{FamilyProfile, FontProvider},
    family_page::FamilyFaces,
    font_family::{FaceCatalog, FaceCoverage, FamilyId, Generic, Lookup},
    inline_lines::GlyphBitmap,
};
use std::{fs, process::Command};
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
/// Runtime-validated catalog: each claim is true only if the Chrome probe below proved it for THIS run.
struct Catalog { face: Pair, reg_sha: String, bold_sha: String, generic_ok: bool, arial_ok: bool, gds_absent: bool }
impl FaceCatalog for Catalog {
    fn lookup(&self, n: &str) -> Lookup {
        match n {
            "arial" if self.arial_ok => Lookup::Available(FamilyId(1)),
            "gds transport" if self.gds_absent => Lookup::Absent,
            _ => Lookup::Unknown,
        }
    }
    fn generic(&self, g: Generic) -> Option<FamilyId> { (g == Generic::SansSerif && self.generic_ok).then_some(FamilyId(1)) }
}
impl FaceCoverage for Catalog {
    fn covers(&self, f: FamilyId, c: char, bold: bool) -> Option<bool> { (f.0 == 1).then(|| self.face.has_glyph(c, bold)).flatten() }
}
impl FamilyFaces for Catalog {
    fn face(&self, f: FamilyId) -> Option<&dyn FontProvider> { (f.0 == 1).then_some(&self.face as &dyn FontProvider) }
    fn face_hashes(&self, f: FamilyId) -> Option<(String, String)> { (f.0 == 1).then(|| (self.reg_sha.clone(), self.bold_sha.clone())) }
}
/// Render one probe page in headless Chrome 156 and return the sha256 of the screenshot bytes.
fn probe(dir: &std::path::Path, name: &str, style: &str, font_src: Option<&str>) -> Option<String> {
    let face = font_src.map(|p| format!("@font-face{{font-family:PinX;src:url({p})}}")).unwrap_or_default();
    let html = format!("<!doctype html><meta charset=utf-8><style>{face}</style><body style='margin:0;font-size:20px;background:#fff'><p style=\"margin:0;font-family:{style}\">Hamburgefonstiv Quick</p><script>document.fonts.load('20px PinX').catch(()=>0).then(()=>document.fonts.ready).then(()=>document.title='ok')</script>");
    let page = dir.join(format!("{name}.html"));
    let png = dir.join(format!("{name}.png"));
    fs::write(&page, html).ok()?;
    let st = Command::new("/usr/bin/google-chrome").args(["--headless=new", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files", "--force-device-scale-factor=1", "--window-size=300,40", "--virtual-time-budget=4000"])
        .arg(format!("--screenshot={}", png.display())).arg(format!("file://{}", page.display())).output().ok()?;
    if !st.status.success() { return None; }
    Some(kobo_net::sha256::hex_digest(&fs::read(&png).ok()?))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let base = "/usr/share/fonts/truetype/liberation2/";
    let rb = fs::read(format!("{base}LiberationSans-Regular.ttf"))?;
    let bb = fs::read(format!("{base}LiberationSans-Bold.ttf"))?;
    let (rs, bs) = (kobo_net::sha256::hex_digest(&rb), kobo_net::sha256::hex_digest(&bb));
    let pr = FamilyProfile::Chrome156HostLiberation2Sans;
    if rs != pr.regular_sha256_hex() || bs != pr.bold_sha256_hex() { return Err("loaded bytes differ from measured profile hashes".into()); }
    let dir = std::path::PathBuf::from(format!("{}/probe", std::env::current_dir()?.display()));
    fs::create_dir_all(&dir)?;
    let pin = dir.join("pin.ttf");
    // Negative control: COBALT_FU_NEGATIVE=1 pins DIFFERENT bytes (DejaVu) so every mapping check must fail closed.
    if std::env::var_os("COBALT_FU_NEGATIVE").is_some() { fs::write(&pin, fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")?)?; } else { fs::write(&pin, &rb)?; }
    // Runtime mapping validation against the LOADED bytes (regular face; bold mapping is not validated here).
    let pinned = probe(&dir, "pinned", "PinX", Some("pin.ttf"));
    let generic = probe(&dir, "generic", "sans-serif", None);
    let arial = probe(&dir, "arial", "arial", None);
    let gds = probe(&dir, "gds", "'GDS Transport'", None);
    let invented = probe(&dir, "invented", "'zzNoSuchFamily'", None);
    let dflt = probe(&dir, "dflt", "initial", None);
    let generic_ok = pinned.is_some() && generic == pinned;
    let arial_ok = pinned.is_some() && arial == pinned;
    // Absent only if rendering equals an invented name AND the host default AND differs from the pinned face.
    let gds_absent = pinned.is_some() && gds.is_some() && gds == invented && gds == dflt && gds != pinned;
    eprintln!("runtime validation: generic_ok={generic_ok} arial_ok={arial_ok} gds_absent={gds_absent} pinned={:?}", pinned.as_deref().map(|h| &h[..12]));
    let mk = || -> Result<Pair, Box<dyn std::error::Error>> { Ok(Pair {
        reg: Font::from_bytes(rb.as_slice(), FontSettings::default())?, bold: Font::from_bytes(bb.as_slice(), FontSettings::default())?,
        reg_sha: rs.clone(), bold_sha: bs.clone() }) };
    let cat = Catalog { face: mk()?, reg_sha: rs.clone(), bold_sha: bs.clone(), generic_ok, arial_ok, gds_absent };
    let html = fs::read(&a[1])?;
    let (w, h): (u32, u32) = (a[3].parse()?, a[4].parse()?);
    let (frame, bounds) = prepare_page_uniform_family_masked(&html, &[], w, h, &cat, None).map_err(|e| format!("entry refused: {e}"))?;
    // Reference: the ordinary masked entry on the same page with every font-family declaration removed, same bytes.
    let plain: String = String::from_utf8(html.clone())?.replace("font-family:sans-serif", "").replace("font-family:\"GDS Transport\", arial, sans-serif", "");
    let reference = prepare_page_masked(plain.as_bytes(), &[], w, h, &mk()?, None).map_err(|e| format!("reference refused: {e}"))?;
    fs::write(&a[2], frame.pixels())?;
    println!("entry==ordinary-masked-reference frame: {} bounds: {} ({} runs); reg {rs} bold {bs}; frame sha256 {}", frame == reference.0, bounds == reference.1, bounds.len(), kobo_net::sha256::hex_digest(frame.pixels()));
    Ok(())
}
