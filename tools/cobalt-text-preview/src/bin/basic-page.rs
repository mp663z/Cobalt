// Render one local page. No network requests or host-specific font discovery.
use cobalt_text_preview::basic_preview::{prepare, Faces};
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("basic-page: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut page = None;
    let mut sheets = None;
    let mut output = None;
    let mut width = 800;
    let mut fonts = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/kobo-text/fonts");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!("basic-page --page PAGE --output PREFIX [--stylesheet-dir DIR] [--fonts DIR] [--width 800]\nWrites PREFIX.rgba, PREFIX.census and PREFIX.imguris. Stylesheets are sheet-0.css, sheet-1.css, ... in document link order. Fonts default to crates/kobo-text/fonts. Use render_basic.py for the final composited PNG.");
            return Ok(());
        }
        let value = args.next().ok_or("option requires a value")?;
        match arg.as_str() {
            "--page" => page = Some(PathBuf::from(value)),
            "--stylesheet-dir" => sheets = Some(PathBuf::from(value)),
            "--output" => output = Some(PathBuf::from(value)),
            "--fonts" => fonts = PathBuf::from(value),
            "--width" => width = value.parse()?,
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    let page = page.ok_or("--page is required")?;
    let output = output.ok_or("--output is required")?;
    let mut styles = Vec::new();
    if let Some(directory) = sheets {
        // Reject gaps instead of silently changing which linked sheet receives bytes.
        let mut files: Vec<_> = std::fs::read_dir(&directory)?.collect::<Result<_, _>>()?;
        files.retain(|entry| entry.file_name().to_string_lossy().ends_with(".css"));
        if files.len() > 48 {
            return Err("at most 48 stylesheets may be supplied".into());
        }
        for index in 0..files.len() {
            styles.push(std::fs::read(directory.join(format!("sheet-{index}.css")))?);
        }
    }
    let faces = Faces::load(&fonts)?;
    let result = prepare(&std::fs::read(page)?, &styles, width, &faces)?;
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut rgba = std::io::BufWriter::new(std::fs::File::create(output.with_extension("rgba"))?);
    result.write_rgba(&mut rgba)?;
    std::io::Write::flush(&mut rgba)?;
    std::fs::write(output.with_extension("census"), result.census())?;
    std::fs::write(output.with_extension("imguris"), result.images.join(&b'\n'))?;
    print!("{}", result.census());
    Ok(())
}
