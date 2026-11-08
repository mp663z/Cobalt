//! PRIVATE host measurement: per-glyph fontdue raster geometry for Liberation2 Sans R/B.
//! Usage: glyph_dump cells.txt   (lines: codepoint size weight) -> lines: cp size weight xmin ymin w h coverage_sum ascent
use fontdue::{Font, FontSettings};
use std::fs;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let base = "/usr/share/fonts/truetype/liberation2/";
    let rb = fs::read(format!("{base}LiberationSans-Regular.ttf")).unwrap();
    let bb = fs::read(format!("{base}LiberationSans-Bold.ttf")).unwrap();
    let r = Font::from_bytes(rb.as_slice(), FontSettings::default()).unwrap();
    let b = Font::from_bytes(bb.as_slice(), FontSettings::default()).unwrap();
    for line in fs::read_to_string(&a[1]).unwrap().lines() {
        let v: Vec<u32> = line.split_whitespace().map(|x| x.parse().unwrap()).collect();
        let (cp, size, w) = (v[0], v[1], v[2]);
        let ch = char::from_u32(cp).unwrap();
        let f = if w == 700 { &b } else { &r };
        let (m, cov) = f.rasterize(ch, size as f32);
        if std::env::var("BITMAPS").is_ok() { println!("BM {cp} {size} {w} {} {} {} {} {}", m.xmin, m.ymin, m.width, m.height, cov.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")); continue; }
        let sum: u32 = cov.iter().map(|&x| u32::from(x)).sum();
        let asc = f.horizontal_line_metrics(size as f32).unwrap().ascent;
        // Actual nonzero-coverage bbox (outline bounds can include all-zero edge rows/cols).
        let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0usize, usize::MAX, 0usize);
        for yy in 0..m.height { for xx in 0..m.width { if cov[yy * m.width + xx] > 0 { x0 = x0.min(xx); x1 = x1.max(xx); y0 = y0.min(yy); y1 = y1.max(yy); } } }
        if x0 == usize::MAX { println!("{cp} {size} {w} 0 0 0 0 0 {asc:.4}"); continue; }
        // xmin of first ink col; ymin = distance of ink bottom row below(-)/above(+) baseline: outline ymin + rows below last ink row
        let nx = m.xmin + x0 as i32;
        let ny = m.ymin + (m.height - 1 - y1) as i32;
        let rm: Vec<String> = (y0..=y1).map(|yy| (0..m.width).map(|xx| cov[yy * m.width + xx]).max().unwrap_or(0).to_string()).collect();
        println!("{cp} {size} {w} {nx} {ny} {} {} {:.4} {:.4} {}", x1 - x0 + 1, y1 - y0 + 1, f64::from(sum) / 255.0, asc, rm.join(","));
    }
}
