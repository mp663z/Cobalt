//! FL60 S0 measurement-only: record TODAY's integer-width behaviour (bounds list + frame sha) of the real prepare_page_masked
//! on a caller-supplied font, for synthetic pages. No production path. Output is evidence only, never an expectation.
use cobalt_text_preview::{prepare_page_masked, LocalFace};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let face = LocalFace::from_bytes(&std::fs::read(&a[1])?)?;
    let (w, h): (u32, u32) = (a[3].parse()?, a[4].parse()?);
    match prepare_page_masked(&std::fs::read(&a[2])?, &[], w, h, &face, None) {
        Ok((frame, bounds)) => {
            std::fs::write(&a[5], frame.pixels())?;
            println!("OK sha256={} runs={}", kobo_net::sha256::hex_digest(frame.pixels()), bounds.len());
            for b in &bounds { println!("rect x={} y={} w={} h={}", b.x, b.y, b.width, b.height); }
        }
        Err(e) => println!("REFUSED {e}"),
    }
    Ok(())
}
