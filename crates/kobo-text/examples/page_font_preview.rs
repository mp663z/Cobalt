//! Render the best-effort font session without an app or installed fonts.
use kobo_text::page_font_session::PageFontSession;
use std::io::Write;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("output PPM path required")?;
    let session = PageFontSession::bundled()?;
    let width = 760_usize;
    let height = 300_usize;
    let mut pixels = vec![255_u8; width * height * 3];
    for (row, text) in [
        "Page font session: Unicode is retained",
        "French: café déjà vu. Symbols: © € £ × °",
        "Russian: Русский текст",
        "Hindi: हिन्दी पाठ",
        "Japanese: 日本語の文章",
        "Missing glyphs are boxes, not discarded paragraphs.",
    ]
    .iter()
    .enumerate()
    {
        let mut x = 16_i32;
        let baseline = 34 + i32::try_from(row)? * 44;
        for ch in text.chars() {
            let glyph = session
                .raster(ch, 23, row == 0, false)
                .ok_or("glyph error")?;
            for gy in 0..glyph.height {
                for gx in 0..glyph.width {
                    let px = x + glyph.left + i32::try_from(gx)?;
                    let py = baseline + glyph.top + i32::try_from(gy)?;
                    if px < 0 || py < 0 {
                        continue;
                    }
                    let (px, py) = (usize::try_from(px)?, usize::try_from(py)?);
                    if px >= width || py >= height {
                        continue;
                    }
                    let level = 255 - glyph.coverage[usize::try_from(gy * glyph.width + gx)?];
                    for channel in 0..3 {
                        pixels[(py * width + px) * 3 + channel] = level;
                    }
                }
            }
            x += i32::try_from(glyph.advance)?;
        }
    }
    let mut output = std::fs::File::create(path)?;
    write!(output, "P6\n{width} {height}\n255\n")?;
    output.write_all(&pixels)?;
    Ok(())
}
