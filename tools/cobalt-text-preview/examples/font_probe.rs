use cobalt_text_preview::LocalFace;
use kobo_web_document::css_text_page::FontProvider;
fn main() {
    let face = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf").unwrap()).unwrap();
    for size in [16u32, 18, 36] {
        println!("size={size}: advance(A)={:?} advance( )={:?} lh={:?} baseline={:?} metrics={:?}",
            face.advance('A', size, false), face.advance(' ', size, false), face.line_height(size, false), face.baseline_offset(size, false), face.normal_line_metrics(size, false));
        let bm = face.raster('A', size, false).unwrap();
        let nonzero = bm.coverage.iter().filter(|&&b| b > 0).count();
        println!("  raster(A): left={} top={} w={} h={} cov_len={} nonzero={}", bm.left, bm.top, bm.width, bm.height, bm.coverage.len(), nonzero);
    }
}
