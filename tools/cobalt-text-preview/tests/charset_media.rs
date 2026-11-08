use cobalt_text_preview::{prepare_page, LocalFace};
#[test]
fn leading_charset_screen_media_matches_flat_preview() {
    let face = LocalFace::bundled_reader_face().unwrap();
    let conditional = b"<!doctype html><style>@charset \"utf-8\";@media only screen{p{color:red}}@media print{p{display:grid}}</style><p>Screen CSS</p>";
    let flat = b"<!doctype html><style>p{color:red}</style><p>Screen CSS</p>";
    let actual = prepare_page(conditional, &[], 240, 100, &face, Some(true)).unwrap();
    assert_eq!(
        actual,
        prepare_page(flat, &[], 240, 100, &face, Some(true)).unwrap()
    );
}
#[test]
fn unsupported_conditions_and_interaction_still_fail_whole_page() {
    let face = LocalFace::bundled_reader_face().unwrap();
    for html in [
        "<!doctype html><style>@media print{@supports(x:y){}}</style><p>x</p>",
        "<!doctype html><style>@charset \"utf-8\";@media screen{p{color:red}}</style><p><a href='/x'>x</a></p>",
    ] {
        assert!(prepare_page(html.as_bytes(), &[], 240, 100, &face, Some(true)).is_err());
    }
}
