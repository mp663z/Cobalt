//! Any bytes as a picture, through what the browser does with one: decode,
//! fit, reduce to panel greys. Refusals are fine; panics and oversize
//! results are not.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(picture) = kobo_image::decode(data) else {
        return;
    };
    let Ok(mut fitted) = picture.fit_enlarging(300, 225) else {
        return;
    };
    fitted.dither(kobo_image::PANEL_GREYS);
    assert!(fitted.width() <= 300 && fitted.height() <= 225);
    assert_eq!(
        fitted.grey().len(),
        fitted.width() as usize * fitted.height() as usize
    );
});
