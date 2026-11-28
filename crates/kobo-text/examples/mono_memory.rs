//! Reproducible resident-memory probe for exact mono geometry sharing.
use kobo_text::{Typeface, MONO_FONT};
use kobo_text::page_font_session::PageFontSession;
fn rss() -> String { std::fs::read_to_string("/proc/self/status").unwrap().lines().filter(|l| l.starts_with("VmRSS:")).collect() }
fn main() {
    let ui = Typeface::from_bytes(MONO_FONT, "mono", kobo_ui::DisplayMetrics::default()).unwrap();
    println!("ui {}",rss());
    let page = PageFontSession::from_bytes(MONO_FONT, include_bytes!("../fonts/DejaVuSansMono-Bold.ttf")).unwrap();
    println!("page {}",rss());
    std::hint::black_box((&ui, &page));
}
