//! The pictures the browser meets, good and bad, through the same path a
//! fetched picture takes: decode, fit to its room, reduce to panel greys.
//!
//! The files are in `tests/images`. The bad ones are what the web actually
//! sends: a download cut short, an error page served as a `.jpg`, a format
//! the decoder does not read, a header that claims more pixels than the
//! reader has memory for.

use crate::pictures::{prepare, prepare_for};

const ROOM: (u32, u32) = (300, 225);

fn image(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/images/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// Pictures that must show, and the size they decode to.
const GOOD: [(&str, u32, u32); 13] = [
    ("baseline.jpg", 320, 240),
    ("progressive.jpg", 320, 240),
    ("grey.jpg", 320, 240),
    ("cmyk.jpg", 160, 120),
    // Stored landscape, tagged to be turned a quarter clockwise.
    ("rotated-exif.jpg", 240, 320),
    ("grey8.png", 160, 120),
    ("grey16.png", 160, 120),
    ("palette.png", 160, 120),
    ("interlaced.png", 160, 120),
    ("transparent-logo.png", 200, 120),
    ("one-pixel.png", 1, 1),
    ("wide-strip.png", 2000, 10),
    ("truncated.jpg", 320, 240),
];

/// Pictures that must be refused, leaving the description in their place.
const BAD: [&str; 5] = [
    "truncated.png",
    "error-page.jpg",
    "not-supported.gif",
    "not-supported.webp",
    "pixel-bomb.png",
];

fn assert_fits_the_panel(name: &str, (width, height, grey): &(u32, u32, Vec<u8>)) {
    assert!(*width >= 1 && *height >= 1, "{name}: {width}x{height}");
    assert!(
        *width <= ROOM.0 && *height <= ROOM.1,
        "{name}: {width}x{height} overflows {ROOM:?}"
    );
    assert_eq!(grey.len(), (*width * *height) as usize, "{name}");
    let mut levels: Vec<u8> = grey.clone();
    levels.sort_unstable();
    levels.dedup();
    assert!(
        levels.len() <= usize::from(kobo_image::PANEL_GREYS),
        "{name}: {} greys",
        levels.len()
    );
}

#[test]
fn every_good_picture_decodes_at_its_true_size_and_fits_its_room() {
    for (name, width, height) in GOOD {
        let bytes = image(name);
        let decoded = kobo_image::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            (decoded.width(), decoded.height()),
            (width, height),
            "{name}"
        );
        let prepared = prepare(&bytes, ROOM.0, ROOM.1).unwrap_or_else(|| panic!("{name}"));
        assert_fits_the_panel(name, &prepared);
    }
}

#[test]
fn a_turned_photo_keeps_its_turned_shape_in_the_room() {
    let (width, height, _) = prepare(&image("rotated-exif.jpg"), ROOM.0, ROOM.1).expect("shown");
    assert!(height > width, "{width}x{height}");
}

#[test]
fn transparency_is_drawn_on_white_paper() {
    let (width, height, grey) =
        prepare(&image("transparent-logo.png"), ROOM.0, ROOM.1).expect("shown");
    let corners = [
        0,
        width as usize - 1,
        (height as usize - 1) * width as usize,
        grey.len() - 1,
    ];
    for at in corners {
        assert_eq!(grey[at], 255, "corner {at} of {width}x{height}");
    }
    let middle = (height as usize / 2) * width as usize + width as usize / 2;
    assert!(grey[middle] < 64, "the black disc: {}", grey[middle]);
}

#[test]
fn every_bad_picture_is_refused_without_a_panic() {
    for name in BAD {
        assert!(prepare(&image(name), ROOM.0, ROOM.1).is_none(), "{name}");
    }
}

#[test]
fn a_header_claiming_a_huge_picture_is_refused_before_decoding() {
    let error = kobo_image::decode(&image("pixel-bomb.png")).expect_err("refused");
    assert!(
        matches!(error, kobo_image::ImageError::TooManyPixels { .. }),
        "{error:?}"
    );
}

/// A small generator so the damage is the same on every run.
struct Damage(u64);

impl Damage {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound.max(1)).unwrap_or(u64::MAX);
        usize::try_from(self.next() % bound).unwrap_or(0)
    }

    /// One way a transfer or a server goes wrong.
    fn apply(&mut self, bytes: &mut Vec<u8>) {
        match self.below(5) {
            0 => {
                let at = self.below(bytes.len());
                bytes[at] ^= 1 << self.below(8);
            }
            1 => {
                let at = self.below(bytes.len());
                bytes[at] = [0x00, 0xff, 0x7f, 0x80][self.below(4)];
            }
            2 => {
                let keep = self.below(bytes.len());
                bytes.truncate(keep.max(1));
            }
            3 => {
                let from = self.below(bytes.len());
                let to = (from + 1 + self.below(64)).min(bytes.len());
                let copy = bytes[from..to].to_vec();
                let at = self.below(bytes.len());
                bytes.splice(at..at, copy);
            }
            _ => {
                // Headers hold the sizes: aim at the first 64 bytes.
                let at = self.below(bytes.len().min(64));
                bytes[at] = self.next().to_le_bytes()[0];
            }
        }
    }
}

#[test]
fn damaged_pictures_never_panic_and_never_overflow_their_room() {
    let rounds = std::env::var("BROWSER_DAMAGE_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(16);
    let mut damage = Damage(0x9e37_79b9_7f4a_7c15);
    let mut shown = 0;
    let mut refused = 0;
    for (name, _, _) in GOOD {
        let original = image(name);
        for _ in 0..rounds {
            let mut bytes = original.clone();
            for _ in 0..=damage.below(3) {
                damage.apply(&mut bytes);
            }
            match prepare(&bytes, ROOM.0, ROOM.1) {
                Some(prepared) => {
                    assert_fits_the_panel(name, &prepared);
                    shown += 1;
                }
                None => refused += 1,
            }
        }
    }
    // Both outcomes happen, or the damage is not reaching the decoder.
    assert!(shown > 0 && refused > 0, "{shown} shown, {refused} refused");
}

#[test]
fn a_colour_panel_gets_colour_for_colour_pictures_and_grey_for_grey_ones() {
    for (name, _, _) in GOOD {
        let picture =
            prepare_for(&image(name), ROOM.0, ROOM.1, true).unwrap_or_else(|| panic!("{name}"));
        assert!(
            picture.width <= ROOM.0 && picture.height <= ROOM.1,
            "{name}"
        );
        let per_pixel = if picture.colour { 3 } else { 1 };
        assert_eq!(
            picture.pixels.len(),
            (picture.width * picture.height) as usize * per_pixel,
            "{name}"
        );
    }
    let photo = prepare_for(&image("baseline.jpg"), ROOM.0, ROOM.1, true).expect("shown");
    assert!(photo.colour, "a colour photo stays in colour");
    let red_or_blue = photo
        .pixels
        .chunks_exact(3)
        .any(|rgb| rgb[0].abs_diff(rgb[1]) > 40 || rgb[2].abs_diff(rgb[1]) > 40);
    assert!(red_or_blue, "and has colour in it");
    let plain = prepare_for(&image("baseline.jpg"), ROOM.0, ROOM.1, false).expect("shown");
    assert!(!plain.colour, "a grey panel never gets colour");
}
