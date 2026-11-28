//! Progressive frames are refused before entropy decoding until work can be bounded.
use kobo_image::{decode, decode_colour, size, ImageError};

const TIMEOUT: &[u8] = include_bytes!("fixtures/progressive-timeout.jpg");
const VALID: &[u8] = include_bytes!("fixtures/valid-progressive.jpg");

#[test]
fn progressive_jpeg_refused_at_every_shared_entry() {
    for bytes in [TIMEOUT, VALID] {
        for result in [
            size(bytes).map(|_| ()),
            decode(bytes).map(|_| ()),
            decode_colour(bytes).map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(ImageError::Undecodable(ref why)) if why == "non-sequential JPEG is refused without a decoder work budget")
            );
        }
    }
}

#[test]
fn progressive_timeout_refusal_repeats_without_entropy_work() {
    let start = std::time::Instant::now();
    for _ in 0..1000 {
        assert!(size(TIMEOUT).is_err());
        assert!(decode(TIMEOUT).is_err());
        assert!(decode_colour(TIMEOUT).is_err());
    }
    eprintln!("3000 preflight refusals: {:?}", start.elapsed());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}
