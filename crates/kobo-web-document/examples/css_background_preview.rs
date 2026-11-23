//! Save the restricted CSS background pass as a portable pixmap for local
//! comparison. This is not the browser UI or evidence of general CSS rendering.
use kobo_web_document::{
    box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree,
    Limits,
};
use std::{env, fs, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let html_path = args
        .next()
        .ok_or("usage: css_background_preview input.html output.ppm width height")?;
    let output_path = args.next().ok_or("missing output path")?;
    let width: u32 = args.next().ok_or("missing width")?.parse()?;
    let height: u32 = args.next().ok_or("missing height")?.parse()?;
    let html = fs::read(html_path)?;
    let styled = parse_style_tree(&html, &[], &Limits::DEFAULT);
    let tree = BoxTree::from_style(&styled);
    let list = paint_backgrounds(&tree, width, height)
        .map_err(|err| format!("unsupported CSS/background geometry: {err:?}"))?;
    let rgba = list
        .rasterize(width, height, Rgb(255, 255, 255))
        .map_err(|err| format!("raster error: {err:?}"))?;
    let mut output = fs::File::create(output_path)?;
    write!(output, "P6\n{width} {height}\n255\n")?;
    for pixel in rgba.chunks_exact(4) {
        output.write_all(&pixel[..3])?;
    }
    Ok(())
}
