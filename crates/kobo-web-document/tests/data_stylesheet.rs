use kobo_web_document::data_stylesheet::{decode, AdmissionError as E, MAX_DECODED_BYTES};
fn uri(payload: &str) -> String {
    format!("data:text/css;base64,{payload}")
}
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for part in bytes.chunks(3) {
        let a = part[0];
        let b = *part.get(1).unwrap_or(&0);
        let c = *part.get(2).unwrap_or(&0);
        out.push(char::from(ALPHABET[usize::from(a >> 2)]));
        out.push(char::from(ALPHABET[usize::from(((a & 3) << 4) | (b >> 4))]));
        out.push(if part.len() > 1 {
            char::from(ALPHABET[usize::from(((b & 15) << 2) | (c >> 6))])
        } else {
            '='
        });
        out.push(if part.len() > 2 {
            char::from(ALPHABET[usize::from(c & 63)])
        } else {
            '='
        });
    }
    out
}
#[test]
fn canonical_vectors_and_exact_css_bytes() {
    for (encoded, plain) in [
        ("", ""),
        ("Zg==", "f"),
        ("Zm8=", "fo"),
        ("Zm9v", "foo"),
        ("Zm9vYg==", "foob"),
        ("Zm9vYmE=", "fooba"),
        ("Zm9vYmFy", "foobar"),
    ] {
        let sheet = decode(&uri(encoded)).unwrap();
        assert_eq!(sheet.css(), plain);
        assert_eq!(sheet.admission().encoded_bytes, encoded.len());
        assert_eq!(sheet.admission().decoded_bytes, plain.len());
    }
    for css in [
        "p{color:red}",
        "\u{feff}p{color:blue}",
        "/* café */ p{color:red}",
    ] {
        assert_eq!(decode(&uri(&base64(css.as_bytes()))).unwrap().css(), css);
    }
}
#[test]
fn header_tokens_casefold_without_changing_payload() {
    for prefix in [
        "DATA:TEXT/CSS;BASE64,",
        "data:text/css;charset=utf-8;base64,",
        "DaTa:TeXt/CsS;CHARSET=UTF-8;BaSe64,",
    ] {
        assert_eq!(decode(&(prefix.to_owned() + "Zg==")).unwrap().css(), "f");
    }
}
#[test]
fn unsupported_profiles_never_fall_through_to_network() {
    for (input, error) in [
        ("https://example.invalid/a.css", E::UnsupportedScheme),
        ("file:///a.css", E::UnsupportedScheme),
        ("", E::UnsupportedScheme),
        ("data:;base64,Zg==", E::UnsupportedMedia),
        ("data:text/html;base64,Zg==", E::UnsupportedMedia),
        ("data:text/css,Zg==", E::UnsupportedRepresentation),
        (
            "data:text/css;charset=latin1;base64,Zg==",
            E::UnsupportedRepresentation,
        ),
        (
            "data:text/css;charset=utf-8;charset=utf-8;base64,Zg==",
            E::UnsupportedRepresentation,
        ),
        (
            "data:text/css;base64;charset=utf-8,Zg==",
            E::UnsupportedRepresentation,
        ),
        (
            "data:text/css;foo=bar;base64,Zg==",
            E::UnsupportedRepresentation,
        ),
    ] {
        assert_eq!(decode(input).err(), Some(error), "{input}");
    }
}
#[test]
fn illegal_alphabet_padding_and_tail_bits_refuse() {
    for payload in [
        "Zg",
        "Zg=",
        "Z===",
        "====",
        "=m9v",
        "Zm=v",
        "Zm9v====",
        "Zg==AAAA",
        "Zh==",
        "Zm9=",
        "Zg==\n",
        "Z g==",
        "Zg==#fragment",
        "Zg%3D%3D",
        "Zg--",
        "Zg__",
        "Zg==\0",
        "éé",
    ] {
        assert_eq!(
            decode(&uri(payload)).err(),
            Some(E::InvalidBase64),
            "{payload:?}"
        );
    }
}
#[test]
fn invalid_utf8_and_css_nul_refuse_but_bom_is_preserved() {
    assert_eq!(decode(&uri("/w==")).err(), Some(E::InvalidUtf8));
    assert_eq!(decode(&uri("AA==")).err(), Some(E::InvalidCssByte));
    assert_eq!(decode(&uri("77u/TA==")).unwrap().css(), "\u{feff}L");
}
#[test]
fn exact_limit_and_both_encoded_decoded_budget_failures() {
    let exact = vec![b'a'; MAX_DECODED_BYTES];
    let encoded = base64(&exact);
    let sheet = decode(&uri(&encoded)).unwrap();
    assert_eq!(sheet.css().len(), MAX_DECODED_BYTES);
    let one_more = base64(&vec![b'a'; MAX_DECODED_BYTES + 1]);
    assert_eq!(one_more.len(), encoded.len());
    assert_eq!(decode(&uri(&one_more)).err(), Some(E::DecodedBudget));
    let larger = base64(&vec![b'a'; MAX_DECODED_BYTES + 3]);
    assert_eq!(decode(&uri(&larger)).err(), Some(E::EncodedBudget));
    assert_eq!(
        decode(&format!("data:text/css;{},Zg==", "x".repeat(100))).err(),
        Some(E::HeaderBudget)
    );
}
#[test]
fn wired_decoder_changes_link_contract_deterministically() {
    use kobo_web_document::{parse_style_tree, style_diagnostics::RefusalReason, Limits};
    let html=b"<!doctype html><link rel=stylesheet href='data:text/css;base64,cHtjb2xvcjpyZWR9'><p>x</p>";
    let before = parse_style_tree(html, &[], &Limits::DEFAULT);
    let sheet = decode("data:text/css;base64,cHtjb2xvcjpyZWR9").unwrap();
    assert_eq!(sheet.css(), "p{color:red}");
    let after = parse_style_tree(html, &[], &Limits::DEFAULT);
    // Parsing is deterministic, and a retained data: sheet is now admitted
    // instead of reported missing. The unwired MissingStylesheet contract is
    // intentionally replaced by this wiring proposal.
    assert_eq!(before.diagnostics, after.diagnostics);
    assert_eq!(after.diagnostics.count(RefusalReason::MissingStylesheet), 0);
    assert!(!after.unsupported);
}

#[test]
fn canonical_ascii_roundtrips_across_every_padding_boundary() {
    for size in 0..257 {
        let plain: String = (0..size)
            .map(|i| char::from(b' ' + u8::try_from(i % 95).unwrap()))
            .collect();
        let encoded = base64(plain.as_bytes());
        let result = decode(&uri(&encoded)).unwrap();
        assert_eq!(result.css(), plain);
        assert_eq!(result.admission().decoded_bytes, size);
    }
}
