//! Prints the document model for a saved page: `dump FILE URL`.
fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(path), Some(url)) = (args.next(), args.next()) else {
        eprintln!("usage: dump FILE URL");
        std::process::exit(2);
    };
    let bytes = std::fs::read(path).expect("read");
    let url = kobo_web_document::Url::parse(&url).expect("url");
    let document =
        kobo_web_document::parse_document(&bytes, &url, &kobo_web_document::Limits::DEFAULT);
    println!("title: {:?}", document.title);
    println!(
        "blocks: {} links: {} images: {} warnings: {:?}",
        document.blocks.len(),
        document.links.len(),
        document.images().len(),
        document.warnings
    );
    print!("{}", document.visible_text());
}
