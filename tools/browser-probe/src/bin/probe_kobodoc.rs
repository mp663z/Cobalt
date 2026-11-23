fn main() {
    let Some(bytes) = browser_probe::input() else {
        browser_probe::run_baseline();
        return;
    };
    let text = String::from_utf8_lossy(&bytes);
    let start = std::time::Instant::now();
    let document = kobo_doc::html::parse(&text);
    let elapsed = start.elapsed();
    println!("kobo-doc blocks={} links={} truncated={} parse_us={}", document.blocks.len(), document.links.len(), document.truncated, elapsed.as_micros());
}
