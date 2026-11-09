use std::fmt::Write as _;
fn main() {
    let mut args = std::env::args().skip(1);
    let mode = args.next().expect("mode: default|state");
    let path = args.next().expect("html path");
    let bytes = std::fs::read(path).expect("read");
    let limits = kobo_web_document::Limits::DEFAULT;
    let tree = match mode.as_str() {
        "default" => kobo_web_document::parse_style_tree(&bytes, &[], &limits),
        other => panic!("bad mode {other}"),
    };
    let mut out = String::new();
    writeln!(
        out,
        "unsupported={} truncated={} quirks={} first={:?}",
        tree.unsupported,
        tree.truncated,
        tree.quirks,
        tree.diagnostics.first_reason()
    )
    .unwrap();
    for (i, n) in tree.nodes.iter().enumerate() {
        writeln!(
            out,
            "{i} tag={} text={:?} display={:?} font_size={} color={:#x}",
            n.tag, n.text, n.style.display, n.style.font_size, n.style.color
        )
        .unwrap();
    }
    print!("{out}");
}
