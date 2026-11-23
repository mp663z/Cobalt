//! FL57 measurement-only: paint a page through the REAL prepare_page_masked with LocalFace on caller-supplied font bytes.
//! Writes raw frame pixels and prints dimensions, byte length, bounds, and sha256. No production path.
use cobalt_text_preview::{prepare_page_masked, LocalFace};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1])?;
    let face = LocalFace::from_bytes(&bytes)?;
    let html = std::fs::read(&a[2])?;
    let (w, h): (u32, u32) = (a[4].parse()?, a[5].parse()?);
    let (frame, bounds) = prepare_page_masked(&html, &[], w, h, &face, None).map_err(|e| format!("refused: {e}"))?;
    std::fs::write(&a[3], frame.pixels())?;
    println!("len={} w={w} h={h} sha256={}", frame.pixels().len(), kobo_net::sha256::hex_digest(frame.pixels()));
    for b in &bounds { println!("bound {b:?}"); }
    Ok(())
}
