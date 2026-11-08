//! Golden document models for the fixture pages.
//!
//! Each `fixtures/NAME.html` has a `fixtures/NAME.ir` beside it holding the
//! model's debug form. A change to parsing shows up as a diff of that file.
//! Regenerate with `UPDATE_GOLDEN=1 cargo test -p kobo-web-document`, then
//! read the diff before committing it.

use std::fmt::Write as _;
use std::path::Path;

use kobo_web_document::{parse_document, Limits, Url};

const BASE: &str = "https://fixtures.example/page.html";

fn render(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let bytes = std::fs::read(path.join(format!("{name}.html"))).expect("fixture");
    let document = parse_document(&bytes, &Url::parse(BASE).expect("url"), &Limits::DEFAULT);
    let mut out = String::new();
    writeln!(out, "title: {:?}", document.title).unwrap();
    writeln!(
        out,
        "base: {:?}",
        document.base.as_ref().map(ToString::to_string)
    )
    .unwrap();
    writeln!(out, "warnings: {:?}", document.warnings).unwrap();
    writeln!(out, "links:").unwrap();
    for (index, link) in document.links.iter().enumerate() {
        writeln!(out, "  {index}: {} {:?}", link.target, link.text).unwrap();
    }
    writeln!(out, "blocks:").unwrap();
    writeln!(out, "{:#?}", document.blocks).unwrap();
    writeln!(out, "text:").unwrap();
    out.push_str(&document.visible_text());
    out
}

fn check(name: &str) {
    let actual = render(name);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.ir"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} missing; run with UPDATE_GOLDEN=1", path.display()));
    assert!(
        actual == expected,
        "{name}: model changed from {}. Rerun with UPDATE_GOLDEN=1 and review the diff.\n{actual}",
        path.display()
    );
}

#[test]
fn basic() {
    check("basic");
}

#[test]
fn malformed() {
    check("malformed");
}

#[test]
fn sanitize() {
    check("sanitize");
}

#[test]
fn latin1() {
    check("latin1");
}

#[test]
fn links() {
    check("links");
}

fn parse(html: &str) -> kobo_web_document::Document {
    parse_document(
        html.as_bytes(),
        &Url::parse(BASE).expect("url"),
        &Limits::DEFAULT,
    )
}

#[test]
fn deep_nesting_stops_at_the_depth_limit_and_keeps_the_text() {
    let depth = 5_000;
    let html = format!(
        "{}deep words{}",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    );
    let document = parse(&html);
    assert!(document
        .warnings
        .contains(&kobo_web_document::Warning::TooDeep));
    assert!(document.visible_text().contains("deep words"));
}

#[test]
fn oversized_input_is_cut_and_says_so() {
    let paragraph = "<p>".to_owned() + &"word ".repeat(200) + "</p>";
    let html = paragraph.repeat(4 * 1024 * 1024 / paragraph.len());
    let document = parse(&html);
    assert!(document
        .warnings
        .contains(&kobo_web_document::Warning::InputTruncated));
    assert!(document.visible_text().len() <= Limits::DEFAULT.max_text_bytes + 1024);
}

#[test]
fn a_flood_of_links_is_capped() {
    let mut html = String::new();
    for n in 0..10_000 {
        write!(html, "<a href=\"/l/{n}\">l{n}</a> ").unwrap();
    }
    let document = parse(&html);
    assert_eq!(document.links.len(), Limits::DEFAULT.max_links);
    assert!(document
        .warnings
        .contains(&kobo_web_document::Warning::TooManyLinks));
}

#[test]
fn every_link_in_the_model_is_referenced_once_in_reading_order() {
    fn walk(blocks: &[kobo_web_document::Block], out: &mut Vec<usize>) {
        use kobo_web_document::{Block, Inline};
        fn inlines(content: &[Inline], out: &mut Vec<usize>) {
            for inline in content {
                match inline {
                    Inline::Link { index, label } => {
                        out.push(*index);
                        inlines(label, out);
                    }
                    Inline::Emphasis(inner) | Inline::Strong(inner) | Inline::Inert(inner) => {
                        inlines(inner, out);
                    }
                    _ => {}
                }
            }
        }
        for block in blocks {
            match block {
                Block::Heading { content, .. } | Block::Paragraph { content, .. } => {
                    inlines(content, out);
                }
                Block::List { items, .. } => items.iter().for_each(|item| walk(item, out)),
                Block::Quote(inner) => walk(inner, out),
                Block::Table(table) => {
                    for cell in table.rows.iter().flatten() {
                        inlines(cell, out);
                    }
                }
                _ => {}
            }
        }
    }
    for name in ["basic", "malformed", "sanitize", "links"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let bytes = std::fs::read(path.join(format!("{name}.html"))).expect("fixture");
        let document = parse_document(&bytes, &Url::parse(BASE).expect("url"), &Limits::DEFAULT);
        let mut seen = Vec::new();
        walk(&document.blocks, &mut seen);
        assert_eq!(
            seen,
            (0..document.links.len()).collect::<Vec<_>>(),
            "{name}"
        );
    }
}

#[test]
fn form_controls_keep_names_values_labels_and_defaults() {
    use kobo_web_document::{Block, Field, Method};
    let page = parse(
        r#"<form action="/find" method="get">
        <label for="q">Search terms</label><input id="q" type="search" name="q" value="ink">
        <input type="hidden" name="source" value="reader">
        <label for="size">Size</label><select id="size" name="size"><option value="s">Small</option><option value="l" selected>Large</option></select>
        <label><input type="checkbox" name="available" checked>Available only</label>
        <label><input type="radio" name="sort" value="new">Newest</label>
        <label><input type="radio" name="sort" value="old" checked>Oldest</label>
        <input type="submit" name="go" value="Find">
    </form>"#,
    );
    let form = page
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Form(form) => Some(form),
            _ => None,
        })
        .expect("form");
    assert_eq!(form.action.to_string(), "https://fixtures.example/find");
    assert_eq!(form.method, Method::Get);
    assert!(
        matches!(&form.fields[0], Field::Text { name, value, label, search: true } if name == "q" && value == "ink" && label == "Search terms")
    );
    assert!(
        matches!(&form.fields[1], Field::Hidden { name, value } if name == "source" && value == "reader")
    );
    assert!(
        matches!(&form.fields[2], Field::Select { name, options, chosen: Some(1), .. } if name == "size" && options[1].value == "l" && options[1].label == "Large")
    );
    assert!(
        matches!(&form.fields[3], Field::Checkbox { name, checked: true, label, .. } if name == "available" && label.contains("Available only"))
    );
    assert!(
        matches!(&form.fields[4], Field::Select { name, options, chosen: Some(1), .. } if name == "sort" && options[0].value == "new" && options[1].label.contains("Oldest"))
    );
    assert!(
        matches!(&form.fields[5], Field::Submit { name: Some(name), value } if name == "go" && value == "Find")
    );
}

#[test]
fn unsupported_and_disabled_form_controls_are_not_submitted() {
    use kobo_web_document::{Block, Field};
    let page = parse(
        r#"<form method="post"><input name="a" disabled>
        <select name="multi" multiple><option>A</option></select>
        <input type="radio" name="choice" value="a"><input type="radio" name="choice" value="b">
        <input type="checkbox" name="yes"><input type="password" name="password" value="secret">
    </form>"#,
    );
    let form = page
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Form(form) => Some(form),
            _ => None,
        })
        .expect("form");
    assert!(matches!(form.method, kobo_web_document::Method::Post));
    assert_eq!(form.fields.len(), 2);
    assert!(matches!(
        &form.fields[0],
        Field::Select { chosen: None, .. }
    ));
    assert!(matches!(
        &form.fields[1],
        Field::Checkbox { checked: false, .. }
    ));
}

#[test]
fn form_count_and_field_count_are_bounded() {
    use kobo_web_document::Block;
    let mut html = String::new();
    for _ in 0..(Limits::DEFAULT.max_forms + 10) {
        html.push_str("<form><input name=q><input name=submit type=submit>");
        html.push_str(&"<input name=x>".repeat(100));
        html.push_str("</form>");
    }
    let document = parse(&html);
    let forms: Vec<_> = document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Form(form) => Some(form),
            _ => None,
        })
        .collect();
    assert_eq!(forms.len(), Limits::DEFAULT.max_forms);
    assert!(forms.iter().all(|form| form.fields.len() <= 64));
}

#[test]
fn radio_and_select_with_the_same_name_remain_separate_controls() {
    use kobo_web_document::{Block, Field};
    let page = parse(
        r"<form><select name=option><option value=s>Small</option></select>
        <input type=radio name=option value=a checked><input type=radio name=option value=b>
    </form>",
    );
    let form = page
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Form(form) => Some(form),
            _ => None,
        })
        .unwrap();
    assert_eq!(form.fields.len(), 2);
    assert!(
        matches!(&form.fields[0], Field::Select { radio: false, options, .. } if options.len() == 1)
    );
    assert!(
        matches!(&form.fields[1], Field::Select { radio: true, options, chosen: Some(0), .. } if options.len() == 2)
    );
}

#[test]
fn multipart_form_is_modelled_but_marked_as_not_urlencoded() {
    use kobo_web_document::Block;
    let page = parse(r#"<form enctype="multipart/form-data"><input name=q></form>"#);
    let form = page
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Form(form) => Some(form),
            _ => None,
        })
        .unwrap();
    assert!(!form.urlencoded);
}
