#[test]
fn malformed_progressive_fuzz_input_is_refused_in_both_public_paths() {
    let bytes = include_bytes!("fixtures/progressive-invalid-marker.jpg");
    assert!(matches!(kobo_image::decode(bytes), Err(kobo_image::ImageError::Undecodable(_))));
    assert!(matches!(kobo_image::decode_colour(bytes), Err(kobo_image::ImageError::Undecodable(_))));
}
#[test]
fn ordinary_progressive_jpeg_still_decodes() {
    let bytes = include_bytes!("fixtures/progressive-valid.jpg");
    let grey = kobo_image::decode(bytes).expect("progressive grey");
    let colour = kobo_image::decode_colour(bytes).expect("progressive colour");
    assert_eq!((grey.width(), grey.height()), (32, 24));
    assert_eq!((colour.width(), colour.height()), (32, 24));
}
#[test]
fn marker_scan_budget_and_truncation_are_refused_without_entropy_decode() {
    let mut stream = vec![0xff, 0xd8, 0xff, 0xc2, 0, 2];
    for _ in 0..65 { stream.extend_from_slice(&[0xff, 0xda, 0, 2]); }
    stream.extend_from_slice(&[0xff, 0xd9]);
    assert!(kobo_image::decode(&stream).is_err());
    for bytes in [vec![0xff, 0xd8, 0xff], vec![0xff, 0xd8, 0xff, 0xc2, 0, 9],
        vec![0xff, 0xd8, 0xff, 0x93, 0, 2], vec![0xff, 0xd8, 0xff, 0xd8]] {
        assert!(kobo_image::decode_colour(&bytes).is_err());
    }
}

#[test]
fn existing_browser_best_effort_entropy_eof_remains_decodable() {
    let bytes = include_bytes!("../../../apps/browser/tests/images/truncated.jpg");
    assert!(kobo_image::decode(bytes).is_ok());
    assert!(kobo_image::decode_colour(bytes).is_ok());
}
