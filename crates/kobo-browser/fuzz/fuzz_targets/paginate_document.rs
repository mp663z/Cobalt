//! Any page, paginated with a budget that stands in for the panel: every
//! page must hold something and pagination must stop.
#![no_main]

use kobo_web_document::{parse_document, Limits, Url};
use kobo_web_layout::{paginate, Piece};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&budget, page)) = data.split_first() else {
        return;
    };
    let Ok(base) = Url::parse("https://example.com/") else {
        return;
    };
    let document = parse_document(page, &base, &Limits::DEFAULT);
    let budget = usize::from(budget % 12) + 1;
    let layout = paginate(&document, |pieces: &[Piece]| pieces.len() <= budget);
    assert!(layout.pages.iter().all(|page| !page.is_empty()));
});
