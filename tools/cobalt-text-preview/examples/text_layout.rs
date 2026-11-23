//! FL58 measurement-only: compose one line from LocalFace glyph rasters at alternative ORIGIN policies, black on white,
//! to compare with Chrome screenshots. mode: int (engine: sum of rounded advances), round (round(sum of fractional advances)),
//! floor (floor(sum of fractional)). Writes 1 byte/px gray frame. No production path; does not touch the painter.
use cobalt_text_preview::LocalFace;
use kobo_web_document::css_text_page::FontProvider;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1])?;
    let face = LocalFace::from_bytes(&bytes)?;
    let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())?;
    let (text, mode, out) = (&a[2], a[3].as_str(), &a[4]);
    let (w, h, size, margin) = (720usize, 40usize, 16u32, 8i32);
    let base = margin + i32::try_from(face.baseline_offset(size, false).ok_or("baseline")?)?;
    let mut px = vec![255u8; w * h];
    let (mut ix, mut fx) = (0i32, 0f32);
    let kern = mode.starts_with('k');
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if kern { if let Some(p) = prev { fx += font.horizontal_kern(p, c, size as f32).unwrap_or(0.0); } }
        prev = Some(c);
        let x0 = match mode { "int" => ix, "round" => fx.round() as i32, "floor" => fx.floor() as i32, "kround" | "kfloor" => if mode == "kfloor" { fx.floor() as i32 } else { fx.round() as i32 }, _ => return Err("mode".into()) };
        if c != ' ' {
            let r = face.raster(c, size, false).ok_or("raster")?;
            for yy in 0..r.height as i32 {
                for xx in 0..r.width as i32 {
                    let cov = u32::from(r.coverage[(yy as usize) * (r.width as usize) + xx as usize]);
                    let (x, y) = (margin + x0 + r.left + xx, base + r.top + yy);
                    if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                        let i = y as usize * w + x as usize;
                        px[i] = (u32::from(px[i]) * (255 - cov) / 255) as u8;
                    }
                }
            }
        }
        ix += i32::try_from(face.advance(c, size, false).ok_or("adv")?)?;
        fx += font.metrics(c, size as f32).advance_width;
    }
    std::fs::write(out, &px)?;
    Ok(())
}
