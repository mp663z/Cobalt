//! art82: rasterize engine output for the first 12 ACCEPTED min/max rows of art80/cases_engine.tsv. Writes /tmp/art82_N.rgba (w+20 x 30, white).
use kobo_web_document::{box_tree::BoxTree, css_background::paint_backgrounds, display_list::Rgb, parse_style_tree, Limits};
#[test]
fn raster() {
    let mut n = 0;
    for l in std::fs::read_to_string(&fixture("art80")).unwrap().lines() {
        let mut p = l.splitn(3, '\t');
        let (c, res, html) = (p.next().unwrap(), p.next().unwrap(), p.next().unwrap());
        if res.starts_with("REFUSE") || !(html.contains("min-width") || html.contains("max-width")) { continue; }
        let w: u32 = html.split("width:").nth(1).unwrap().split("px").next().unwrap().parse().unwrap();
        let t = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
        let px = paint_backgrounds(&t, 1000, 200).unwrap().rasterize(w + 20, 30, Rgb(255, 255, 255)).unwrap();
        std::fs::write(format!("/tmp/art82_{n}.rgba"), px).unwrap();
        std::fs::write(format!("/tmp/art82_{n}.html"), html).unwrap();
        std::fs::write(format!("/tmp/art82_{n}.case"), c).unwrap();
        n += 1;
        if n == 12 { break; }
    }
}

/// Fixture contract: tests/fixtures/<name>/cases_engine.tsv inside this crate; override the
/// directory with COBALT_FIXTURES. (Recovered from release assets; see PROVENANCE.)
fn fixture(name: &str) -> String {
    let base = std::env::var("COBALT_FIXTURES")
        .unwrap_or_else(|_| format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR")));
    format!("{base}/{name}/cases_engine.tsv")
}
