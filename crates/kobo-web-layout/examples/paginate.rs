//! Times pagination of a saved page on the Clara BW: `paginate FILE URL`.
use std::time::Instant;

use kobo_ui::TextScale;
use kobo_web_document::{parse_document, Limits, Url};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(path), Some(url)) = (args.next(), args.next()) else {
        eprintln!("usage: paginate FILE URL");
        std::process::exit(2);
    };
    kobo_text::install(kobo_ui::CLARA_BW_METRICS).expect("fonts");
    let bytes = std::fs::read(path).expect("read");
    let started = Instant::now();
    let document = parse_document(&bytes, &Url::parse(&url).expect("url"), &Limits::DEFAULT);
    println!("parse: {:?}", started.elapsed());
    let title = document.title.clone().unwrap_or_default();
    for text_scale in [TextScale::Default, TextScale::Large, TextScale::ExtraLarge] {
        let metrics = kobo_ui::DisplayMetrics {
            text_scale,
            ..kobo_ui::CLARA_BW_METRICS
        };
        let started = Instant::now();
        let mut paginator = kobo_web_layout::Paginator::for_document(&document);
        let mut fits = |pieces: &[kobo_web_layout::Piece]| {
            kobo_web_layout::fits(
                &kobo_web_layout::page_screen(&title, pieces, 998, 999).build(),
                &metrics,
            )
        };
        paginator.next_page(&mut fits);
        let first = started.elapsed();
        let started = Instant::now();
        let layout = kobo_web_layout::paginate_for(&document, &title, &metrics);
        println!(
            "{text_scale:?}: first page in {first:?}, {} pages in {:?}{}",
            layout.pages.len(),
            started.elapsed(),
            if layout.truncated { " (truncated)" } else { "" }
        );
    }
}
