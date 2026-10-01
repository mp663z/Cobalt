//! Restricted single-direct-text preview with an explicit local TrueType face.
//! Not a general browser renderer, and not substituted for the frozen
//! background-only benchmark entry point.
use cobalt_text_preview::{prepare_page, LocalFace, PanelFrame};
use kobo_web_document::{
    box_tree::BoxTree, css_text_page::paint_single_text_page, display_list::Rgb, parse_style_tree,
    Limits,
};
use std::{env, fs, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let html_path = args
        .next()
        .ok_or("usage: cobalt-text-preview input.html output.ppm width height font.ttf")?;
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
    let face = LocalFace::from_bytes(&fs::read(font_path)?)?;
    let html = fs::read(html_path)?;
    let frame = if blocks {
        prepare_page(&html, &[], width, height, &face, Some(true))?
    } else {
        let styled = parse_style_tree(&html, &[], &Limits::DEFAULT);
        let tree = BoxTree::from_style(&styled);
        let list = paint_single_text_page(&tree, width, height, &face)
            .map_err(|error| format!("unsupported direct-text geometry/font: {error:?}"))?;
        let rgba = list
            .rasterize(width, height, Rgb(255, 255, 255))
            .map_err(|error| format!("raster error: {error:?}"))?;
        PanelFrame::from_rgba(width, height, &rgba, Some(true))?
    };
    let mut output = fs::File::create(output_path)?;
    write!(output, "P6\n{width} {height}\n255\n")?;
    output.write_all(frame.pixels())?;
    Ok(())
}
