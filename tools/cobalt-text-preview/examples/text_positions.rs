//! FL57 measurement-only example: engine per-glyph advances (LocalFace::advance == round(fontdue advance_width))
//! next to the unrounded fontdue advance, on caller-supplied font bytes. Prints sha256 of the bytes. No production path.
use cobalt_text_preview::LocalFace;
use kobo_web_document::css_text_page::FontProvider;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1])?;
    let face = LocalFace::from_bytes(&bytes)?;
    println!("face_sha256 {}", face.face_sha256_hex().unwrap_or_default());
    let size: u32 = a[3].parse()?;
    let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())?;
    let (mut x, mut fx) = (0u32, 0f32);
    for (i, c) in a[2].chars().enumerate() {
        let adv = face.advance(c, size, false).ok_or("no advance")?;
        let m = font.metrics(c, size as f32);
        let r = face.raster(c, size, false).ok_or("no raster")?;
        println!("{i} {c:?} x={x} adv={adv} frac_adv={:.4} fx={fx:.4} bmp_left={} bmp_top={} w={} h={}", m.advance_width, r.left, r.top, r.width, r.height);
        x += adv;
        fx += m.advance_width;
    }
    println!("end int={x} frac={fx:.4} ascent={:?} baseline={:?}", face.line_height(size, false), face.baseline_offset(size, false));
    Ok(())
}
