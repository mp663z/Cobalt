//! FL69 v2 (dumps per-glyph layout rects as `G x y w h bounds_x bounds_y` lines on stdout; otherwise identical to family_bound.rs). PRIVATE, unadopted host measurement: real Liberation2 Sans R/B bytes through fontdue as a
//! FontProvider with hashes and cmap coverage (lookup_glyph_index != 0), painted by the
//! family-bound painter. Usage: family_bound page.html out.rgba width height
use fontdue::{Font, FontSettings};
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks_family_bound, paint_direct_text_blocks_family_bound_shaped, FamilyProfile, FontProvider},
    display_list::Rgb,
    inline_lines::{FitRule, GlyphBitmap, RunShaper, ShapedCluster, TextShaping, SHAPED_UNIT},
    parse_style_tree_with_pinned_font_family, Limits, SupplyBudget,
};
use std::fs;
struct Pair { reg: Font, bold: Font, reg_sha: String, bold_sha: String }
impl Pair {
    fn f(&self, bold: bool) -> &Font { if bold { &self.bold } else { &self.reg } }
}
impl FontProvider for Pair {
    fn face_sha256_hex(&self) -> Option<String> { Some(self.reg_sha.clone()) }
    fn bold_face_sha256_hex(&self) -> Option<String> { Some(self.bold_sha.clone()) }
    fn has_glyph(&self, c: char, bold: bool) -> Option<bool> { Some(self.f(bold).lookup_glyph_index(c) != 0) }
    fn advance(&self, c: char, size: u32, bold: bool) -> Option<u32> {
        let f = self.f(bold);
        if f.lookup_glyph_index(c) == 0 { return None; }
        let w = f.metrics(c, size as f32).advance_width.round().max(1.0);
        Some(w as u32)
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
/// Host-only shaper from the SAME hash-checked bytes: unrounded hmtx advance (fontdue
/// advance_width, f32) plus the legacy `kern` table pair adjustment (fontdue horizontal_kern),
/// ASCII only, 1:1 clusters. Measured equal to GPOS kern pairs for ASCII in both faces
/// (kernpairs.py); NOT a general shaper (no GSUB, scripts, direction, spacing features).
struct KernRun<'a>(&'a Pair);
impl RunShaper for KernRun<'_> {
    fn shape_run(&self, text: &str, size: u32, bold: bool) -> Option<Vec<ShapedCluster>> {
        let f = self.0.f(bold);
        let cs: Vec<char> = text.chars().collect();
        let mut out = Vec::with_capacity(cs.len());
        for (i, &c) in cs.iter().enumerate() {
            if !('\u{20}'..='\u{7e}').contains(&c) { return None; }
            let mut px = f.metrics(c, size as f32).advance_width;
            if let Some(&n) = cs.get(i + 1) { px += f.horizontal_kern(c, n, size as f32).unwrap_or(0.0); }
            let u = (px * SHAPED_UNIT as f32).round();
            if !(u >= 0.0 && u < 4.0e9) { return None; }
            out.push(ShapedCluster { first_char: i as u32, char_len: 1, units: u as u32 });
        }
        Some(out)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let base = "/usr/share/fonts/truetype/liberation2/";
    let rb = fs::read(format!("{base}LiberationSans-Regular.ttf"))?;
    let bb = fs::read(format!("{base}LiberationSans-Bold.ttf"))?;
    let p = Pair {
        reg: Font::from_bytes(rb.as_slice(), FontSettings::default())?,
        bold: Font::from_bytes(bb.as_slice(), FontSettings::default())?,
        reg_sha: kobo_net::sha256::hex_digest(&rb),
        bold_sha: kobo_net::sha256::hex_digest(&bb),
    };
    let pr = FamilyProfile::Chrome156HostLiberation2Sans;
    eprintln!("reg_ok={} bold_ok={} H={:?} space={:?} nbsp={:?}", p.reg_sha == pr.regular_sha256_hex(), p.bold_sha == pr.bold_sha256_hex(), p.has_glyph('H', false), p.has_glyph(' ', false), p.has_glyph('\u{a0}', false));
    let html = fs::read(&a[1])?;
    let (w, h): (u32, u32) = (a[3].parse()?, a[4].parse()?);
    if a.get(5).map(String::as_str) == Some("plain") {
        let st = kobo_web_document::parse_style_tree(&html, &[], &Limits::DEFAULT);
        let t = BoxTree::from_style(&st);
        eprintln!("plain: unsupported={} ordinary painter: {:?}", st.unsupported, kobo_web_document::css_text_page::paint_direct_text_blocks(&t, w, h, &p).map(|d| d.commands().len()));
        return Ok(());
    }
    let s = parse_style_tree_with_pinned_font_family(&html, &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    if s.unsupported { return Err(format!("intake refused: {:?}", s.diagnostics).into()); }
    let t = BoxTree::from_style(&s);
    let list = if a.get(5).map(String::as_str) == Some("shaped") {
        let k = KernRun(&p);
        let sh = TextShaping { shaper: &k, fit: FitRule { unit: SHAPED_UNIT, slack: 0 } };
        paint_direct_text_blocks_family_bound_shaped(&t, w, h, &p, FamilyProfile::Chrome156HostLiberation2Sans, &sh)
    } else {
        paint_direct_text_blocks_family_bound(&t, w, h, &p, FamilyProfile::Chrome156HostLiberation2Sans)
    }
    .map_err(|e| format!("gate/painter refused: {e:?}"))?;
    for c in list.commands() { if let kobo_web_document::display_list::Command::GlyphRun { layout, bounds, .. } = c { println!("G {} {} {} {} {} {}", layout.x, layout.y, layout.width, layout.height, bounds.x, bounds.y); } }
    let rgba = list.rasterize(w, h, Rgb(255, 255, 255)).map_err(|e| format!("raster {e:?}"))?;
    fs::write(&a[2], rgba)?;
    println!("painted ok; regular sha {} bold sha {}", p.reg_sha, p.bold_sha);
    Ok(())
}
