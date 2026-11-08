use cobalt_text_preview::{prepare_page, LocalFace};
#[test]
fn normalized_structure_does_not_admit_anonymous_text_or_lists_or_interaction() {
    let face = LocalFace::bundled_reader_face().unwrap();
    for html in [
        "<!doctype html><div>A<p>B</p>C</div>",
        "<!doctype html><ul><li>A<p>B</p>C</li></ul>",
        "<!doctype html><div><a href='/x'>A</a><p>B</p>C</div>",
    ] {
        assert!(
            prepare_page(html.as_bytes(), &[], 300, 200, &face, Some(true)).is_err(),
            "{html}"
        );
    }
}
