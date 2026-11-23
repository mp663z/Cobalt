//! FL58 measurement-only: does fontdue's public kern-table lookup reproduce Chrome's kerning on the pinned face?
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&a[1])?;
    let font = fontdue::Font::from_bytes(bytes.as_slice(), fontdue::FontSettings::default())?;
    let cs: Vec<char> = a[2].chars().collect();
    let (mut sum, mut n) = (0f32, 0);
    for w in cs.windows(2) {
        if let Some(k) = font.horizontal_kern(w[0], w[1], 16.0) { if k != 0.0 { n += 1; sum += k; println!("pair {:?}{:?} kern {k:.4}", w[0], w[1]); } }
    }
    println!("nonzero pairs {n} sum {sum:.4}");
    Ok(())
}
