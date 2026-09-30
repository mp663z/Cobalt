//! Restricted single-direct-text preview with an explicit local TrueType face.
//! Not a general browser renderer, and not substituted for the frozen
//! background-only benchmark entry point.
use fontdue::{Font, FontSettings};
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, paint_single_text_page, FontProvider},
    display_list::Rgb,
    inline_lines::GlyphBitmap,
    parse_style_tree, Limits,
};
use std::{env, fs, io::Write};

struct LocalFace(Font);
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
impl LocalFace {
    fn pixels(size: u32) -> Option<f32> {
        (1..=256).contains(&size).then_some(size as f32) // bounded exact integer -> f32
    }
    fn bounded_u32(value: f32) -> Option<u32> {
        if !value.is_finite() || value < 1.0 || value > f32::from(u16::MAX) {
            return None;
        }
        Some(value as u32) // checked finite positive range above
    }
}
impl FontProvider for LocalFace {
    fn advance(&self, character: char, size: u32) -> Option<u32> {
        if self.0.lookup_glyph_index(character) == 0 {
            return None;
        }
        let width = self.0.metrics(character, Self::pixels(size)?).advance_width;
        Self::bounded_u32(width.round().max(1.0))
    }
    fn line_height(&self, size: u32) -> Option<u32> {
        let metrics = self.0.horizontal_line_metrics(Self::pixels(size)?)?;
        Self::bounded_u32((metrics.ascent - metrics.descent + metrics.line_gap).ceil())
    }
    fn baseline_offset(&self, size: u32) -> Option<i32> {
        let ascent = self
            .0
            .horizontal_line_metrics(Self::pixels(size)?)?
            .ascent
            .ceil();
        i32::try_from(Self::bounded_u32(ascent)?).ok()
    }
    fn raster(&self, character: char, size: u32) -> Option<GlyphBitmap> {
        if self.0.lookup_glyph_index(character) == 0 {
            return None;
        }
        let (metrics, coverage) = self.0.rasterize(character, Self::pixels(size)?);
        Some(GlyphBitmap {
            left: metrics.xmin,
            top: -(metrics.ymin + i32::try_from(metrics.height).ok()?),
            width: u32::try_from(metrics.width).ok()?,
            height: u32::try_from(metrics.height).ok()?,
            coverage,
        })
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let html_path = args
        .next()
        .ok_or("usage: css_direct_text_preview input.html output.ppm width height font.ttf")?;
    let output_path = args.next().ok_or("missing output path")?;
    let width: u32 = args.next().ok_or("missing width")?.parse()?;
    let height: u32 = args.next().ok_or("missing height")?.parse()?;
    let font_path = args.next().ok_or("missing font file")?;
    let blocks = match args.next().as_deref() {
        None => false,
        Some("--blocks") => true,
        Some(_) => return Err("expected optional --blocks".into()),
    };
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    let face = LocalFace(Font::from_bytes(
        fs::read(font_path)?,
        FontSettings::default(),
    )?);
    let html = fs::read(html_path)?;
    let styled = parse_style_tree(&html, &[], &Limits::DEFAULT);
    let tree = BoxTree::from_style(&styled);
    let list = if blocks {
        paint_direct_text_blocks(&tree, width, height, &face)
    } else {
        paint_single_text_page(&tree, width, height, &face)
    }
    .map_err(|error| format!("unsupported direct-text geometry/font: {error:?}"))?;
    let rgba = list
        .rasterize(width, height, Rgb(255, 255, 255))
        .map_err(|error| format!("raster error: {error:?}"))?;
    let mut output = fs::File::create(output_path)?;
    write!(output, "P6\n{width} {height}\n255\n")?;
    for pixel in rgba.chunks_exact(4) {
        output.write_all(&pixel[..3])?;
    }
    Ok(())
}
