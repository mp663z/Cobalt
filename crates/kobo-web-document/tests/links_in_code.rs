//! Links inside `<code>`, the way API documentation writes types.

use kobo_web_document::{parse_document, Block, Inline, Limits, Url};

#[test]
fn links_inside_code_are_kept_and_the_words_stay_code() {
    let html = r#"<p>the box, <code><a href="enum.Option.html">Option</a>&lt;<a href="struct.Box.html">Box&lt;T&gt;</a>&gt;</code>.</p>"#;
    let url = Url::parse("https://doc.example/std/option/index.html").expect("url");
    let document = parse_document(html.as_bytes(), &url, &Limits::DEFAULT);
    let targets: Vec<String> = document.links.iter().map(|l| l.target.to_string()).collect();
    assert_eq!(
        targets,
        [
            "https://doc.example/std/option/enum.Option.html",
            "https://doc.example/std/option/struct.Box.html"
        ]
    );
    assert_eq!(document.visible_text(), "the box, Option<Box<T>>.\n");
    let Block::Paragraph { content, .. } = &document.blocks[0] else {
        panic!("{:?}", document.blocks[0]);
    };
    assert!(content.contains(&Inline::Code("<".to_owned())), "{content:?}");
    assert!(content.iter().any(|inline| matches!(
        inline,
        Inline::Link { label, .. } if label == &[Inline::Code("Option".to_owned())]
    )));
}
