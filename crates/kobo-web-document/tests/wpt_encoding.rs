//! How a page that is not UTF-8 is decoded.
//!
//! A page that is valid UTF-8 is read as UTF-8; anything else is read as
//! Windows-1252, the fallback browsers use for Western pages, and the
//! document warns that it was. Other legacy encodings are not decoded (see
//! `tests/wpt/include.txt`). The mapping is checked against the Encoding
//! Standard's own index, `tests/wpt/encoding/index-windows-1252.txt`.

use kobo_web_document::{parse_document, Limits, Url, Warning};

fn read(bytes: &[u8]) -> kobo_web_document::Document {
    let url = Url::parse("https://encoding.example/").expect("url");
    parse_document(bytes, &url, &Limits::DEFAULT)
}

#[test]
fn every_high_byte_decodes_as_the_windows_1252_index_says() {
    let path = format!(
        "{}/tests/wpt/encoding/index-windows-1252.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let index = std::fs::read_to_string(&path).expect("index");
    let mut seen = 0;
    for line in index.lines().filter(|line| !line.starts_with('#')) {
        let mut fields = line.split_whitespace();
        let (Some(pointer), Some(code)) = (fields.next(), fields.next()) else {
            continue;
        };
        let pointer: u8 = pointer.parse().expect("pointer");
        let code = u32::from_str_radix(code.trim_start_matches("0x"), 16).expect("code");
        let expected = char::from_u32(code).expect("char");
        let byte = 0x80 + pointer;
        let mut page = b"<pre>[".to_vec();
        page.push(byte);
        page.extend_from_slice(b"]</pre>");
        let document = read(&page);
        assert!(document.warnings.contains(&Warning::NotUtf8), "{byte:#x}");
        assert_eq!(
            document.visible_text(),
            format!("[{expected}]\n"),
            "byte {byte:#x}"
        );
        seen += 1;
    }
    assert_eq!(seen, 128);
}

#[test]
fn utf8_with_or_without_a_byte_order_mark_is_read_as_utf8() {
    for page in [
        "<pre>[caf\u{e9} \u{2014} \u{1F600}]</pre>"
            .as_bytes()
            .to_vec(),
        [
            b"\xEF\xBB\xBF".as_slice(),
            "<pre>[caf\u{e9} \u{2014} \u{1F600}]</pre>".as_bytes(),
        ]
        .concat(),
    ] {
        let document = read(&page);
        assert!(!document.warnings.contains(&Warning::NotUtf8));
        assert_eq!(document.visible_text(), "[caf\u{e9} \u{2014} \u{1F600}]\n");
    }
}
