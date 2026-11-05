//! Restricted single-direct-text preview with an explicit local TrueType face.
//! Not a general browser renderer, and not substituted for the frozen
//! background-only benchmark entry point.
use cobalt_text_preview::{prepare_page_masked, prepare_page_static, LocalFace, PanelFrame};
use kobo_web_document::{
    box_tree::BoxTree, css_text_page::paint_single_text_page, display_list::Rgb, parse_style_tree,
    Limits,
};
use std::{env, fmt::Write as FmtWrite, fs, io::Write as IoWrite};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let html_path = args
        .next()
        .ok_or("usage: cobalt-text-preview input.html output.ppm width height font.ttf")?;
    let output_path = args.next().ok_or("missing output path")?;
    let width: u32 = args.next().ok_or("missing width")?.parse()?;
    let height: u32 = args.next().ok_or("missing height")?.parse()?;
    let font_path = args.next().ok_or("missing font file")?;
    let mut mask_path = None;
    let mut bold_path = None;
    let mut blocks = false;
    let mut static_capture = false;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--blocks" if !blocks => blocks = true,
            "--static-capture" if !static_capture => static_capture = true,
            "--mask-json" if mask_path.is_none() => {
                mask_path = Some(args.next().ok_or("missing mask output path")?);
            }
            "--font-bold" if bold_path.is_none() => {
                bold_path = Some(args.next().ok_or("missing bold font file")?);
            }
            _ => return Err("unknown or repeated option".into()),
        }
    }
    if mask_path.is_some() && !blocks {
        return Err("--mask-json requires --blocks".into());
    }
    if static_capture {
        if !blocks {
            return Err("--static-capture requires --blocks".into());
        }
        eprintln!(
            "static capture: interactive content rendered inert; measurement only, not a shipping surface"
        );
    }
    // The optional bold face loads only when a bold glyph actually needs it.
    let face = match bold_path {
        None => LocalFace::from_bytes(&fs::read(font_path)?)?,
        Some(path) => LocalFace::with_bold_loader(
            &fs::read(font_path)?,
            Box::new(move || {
                let bytes = std::fs::read(&path).ok()?;
                if bytes.is_empty() || bytes.len() > Limits::DEFAULT.max_input_bytes {
                    return None;
                }
                fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
            }),
        )?,
    };
    let html = fs::read(html_path)?;
    let frame = if blocks {
        let (frame, bounds) = if static_capture {
            prepare_page_static(&html, &[], width, height, &face, Some(true))?
        } else {
            prepare_page_masked(&html, &[], width, height, &face, Some(true))?
        };
        if let Some(path) = mask_path {
            let mut json = String::from("[");
            for (index, rect) in bounds.iter().enumerate() {
                if index > 0 {
                    json.push(',');
                }
                write!(
                    json,
                    "{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
                    rect.x, rect.y, rect.width, rect.height
                )
                .map_err(|_| "mask formatting failed")?;
            }
            json.push(']');
            fs::write(path, json)?;
        }
        frame
    } else {
        let styled = parse_style_tree(&html, &[], &Limits::DEFAULT);
        let tree = BoxTree::from_style(&styled);
        let list = paint_single_text_page(&tree, width, height, &face)
            .map_err(|error| format!("unsupported direct-text geometry/font: {error:?}"))?;
        let rgba = list
            .rasterize(width, height, Rgb(255, 255, 255))
            .map_err(|error| format!("raster error: {error:?}"))?;
        PanelFrame::from_rgba_in_place(width, height, rgba, Some(true))?
    };
    let mut output = fs::File::create(output_path)?;
    write!(output, "P6\n{width} {height}\n255\n")?;
    output.write_all(frame.pixels())?;
    Ok(())
}
