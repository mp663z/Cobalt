//! Pictures for the page being read.
//!
//! A page's layout already holds room for each picture whose size the page
//! declared, so fetching one never moves a word. What is left is deciding
//! which pictures to fetch and turning the bytes into panel greys.
//!
//! Only pictures on the screen being read are asked for, and only so many.
//! A picture is a separate request over a slow radio, and a long article can
//! carry dozens; fetching the ones nobody turns to would spend the battery on
//! nothing.

use std::collections::BTreeMap;

use kobo_sdk::{Context, Header, Task, TaskId, TaskOutcome};
use kobo_web_layout::picture_handle;

/// Pictures asked for per screen. A screen holds two or three at most at
/// [`kobo_web_layout::PICTURE_MAX_MM`]; the cap is for pages that declare
/// many tiny-but-not-too-tiny images in a row.
pub const MAX_PER_SCREEN: usize = 6;

/// Pictures asked for per page, however far the reader turns.
pub const MAX_PER_PAGE: usize = 48;

/// What the decoder reads. Asking for these only keeps a server that can
/// convert (Wikipedia's can) from sending WebP or AVIF, which it cannot.
const ACCEPT: &str = "image/jpeg,image/png;q=0.9";

/// Endings that name formats the decoder does not read, so they are not
/// fetched at all.
const UNREADABLE: [&str; 5] = [".svg", ".gif", ".webp", ".avif", ".ico"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Fetching(TaskId),
    /// Coming off the shelf, fitted and reduced when it was first shown.
    Reading,
    Shown,
    /// Refused, unreachable or undecodable. The room stays empty and the
    /// description under it says what it was.
    Failed,
}

#[derive(Debug, Default)]
pub struct Pictures {
    /// Whether the panel shows colour. Until the reader says, pictures are
    /// grey, which every panel can show.
    colour: bool,
    states: BTreeMap<usize, State>,
    /// Shelf keys being read, and the image each is for.
    reading: BTreeMap<String, usize>,
}

/// The shelf key a fitted picture is kept under. The room is part of it:
/// the same picture at another text size is fitted afresh.
#[must_use]
pub fn shelf_key(url: &str, width: u32, height: u32, colour: bool) -> String {
    let kind = if colour { "rgb" } else { "grey" };
    format!("picture:{kind}:{width}x{height}:{url}")
}

impl Pictures {
    /// Draws pictures in colour from now on, or not. Pictures already
    /// shown stay as they are until their page is opened again.
    pub fn set_colour(&mut self, colour: bool) {
        self.colour = colour;
    }

    #[must_use]
    pub const fn colour(&self) -> bool {
        self.colour
    }

    /// Forgets every picture: cancels what is on its way and releases what
    /// the runtime is holding. For a new page.
    /// Returns the shelf keys that were being read, to stop.
    pub fn clear(&mut self, context: &mut Context) -> Vec<String> {
        for (image, state) in std::mem::take(&mut self.states) {
            match state {
                State::Fetching(task) => context.cancel(task),
                State::Shown => context.drop_picture(picture_handle(image)),
                State::Reading | State::Failed => {}
            }
        }
        std::mem::take(&mut self.reading).into_keys().collect()
    }

    #[cfg(test)]
    #[must_use]
    pub fn state(&self, image: usize) -> Option<State> {
        self.states.get(&image).copied()
    }

    /// Asks for the pictures `wanted` names that have not been asked for,
    /// within the caps. `source` gives an image's address. `kept` is tried
    /// first: it starts reading a kept copy and returns its key, or `None`
    /// when there is none.
    pub fn want(
        &mut self,
        context: &mut Context,
        wanted: &[usize],
        source: impl Fn(usize) -> Option<String>,
        mut kept: impl FnMut(&mut Context, usize, &str) -> Option<String>,
    ) {
        for &image in wanted.iter().take(MAX_PER_SCREEN) {
            if self.states.len() >= MAX_PER_PAGE {
                return;
            }
            if self.states.contains_key(&image) {
                continue;
            }
            let Some(url) = source(image) else {
                continue;
            };
            let path = url
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if UNREADABLE.iter().any(|ending| path.ends_with(ending)) {
                self.states.insert(image, State::Failed);
                continue;
            }
            if let Some(key) = kept(context, image, &url) {
                self.reading.insert(key, image);
                self.states.insert(image, State::Reading);
                continue;
            }
            let task = context.spawn(Task::Fetch {
                url,
                offset: 0,
                max_bytes: u32::try_from(kobo_image::MAX_SOURCE_BYTES).unwrap_or(u32::MAX),
                credential: None,
                headers: vec![Header::new("Accept", ACCEPT)],
            });
            let state = task.map_or(State::Failed, State::Fetching);
            self.states.insert(image, state);
        }
    }

    /// Forgets one picture, so it is asked for again next time it is wanted.
    pub fn forget(&mut self, image: usize) {
        self.states.remove(&image);
        self.reading.retain(|_, reading| *reading != image);
    }

    /// Whether `key` is a shelf read of ours.
    #[must_use]
    pub fn reads(&self, key: &str) -> bool {
        self.reading.contains_key(key)
    }

    /// A kept picture came off the shelf, or `None` when it could not be
    /// read. Returns whether there is now something new to draw.
    pub fn off_shelf(&mut self, context: &mut Context, key: &str, blob: Option<&[u8]>) -> bool {
        let Some(image) = self.reading.remove(key) else {
            return false;
        };
        let shown = blob.and_then(unpack).and_then(|picture| {
            put(
                context,
                image,
                picture.width,
                picture.height,
                picture.colour,
                picture.pixels.to_vec(),
            )
        });
        if shown.is_some() {
            self.states.insert(image, State::Shown);
            true
        } else {
            // Asked for from the site next time it is wanted.
            self.states.remove(&image);
            false
        }
    }

    /// The image a task was fetching, if it was one of these.
    #[must_use]
    pub fn fetching(&self, task: TaskId) -> Option<usize> {
        self.states
            .iter()
            .find(|(_, state)| **state == State::Fetching(task))
            .map(|(image, _)| *image)
    }

    /// Takes a fetched picture, fits it to the `room` the layout gave it and
    /// hands it to the runtime. Returns the fitted picture, packed for the
    /// shelf, when there is now something new to draw.
    pub fn arrived(
        &mut self,
        context: &mut Context,
        image: usize,
        outcome: TaskOutcome,
        room: Option<(u32, u32)>,
    ) -> Option<Vec<u8>> {
        let packed = match (outcome, room) {
            (TaskOutcome::Completed(bytes), Some((width, height))) => {
                prepare_for(&bytes, width, height, self.colour).and_then(|picture| {
                    let packed = pack(&picture);
                    put(
                        context,
                        image,
                        picture.width,
                        picture.height,
                        picture.colour,
                        picture.pixels,
                    )
                    .map(|_| packed)
                })
            }
            _ => None,
        };
        let state = if packed.is_some() {
            State::Shown
        } else {
            State::Failed
        };
        self.states.insert(image, state);
        packed
    }
}

fn put(
    context: &mut Context,
    image: usize,
    width: u32,
    height: u32,
    colour: bool,
    pixels: Vec<u8>,
) -> Option<kobo_sdk::TilePicture> {
    if colour {
        context.put_colour_picture(picture_handle(image), width, height, pixels)
    } else {
        context.put_picture(picture_handle(image), width, height, pixels)
    }
}

/// A picture fitted to its room, ready for the panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prepared<P = Vec<u8>> {
    pub width: u32,
    pub height: u32,
    /// Three bytes a pixel, red, green and blue, rather than one grey.
    pub colour: bool,
    pub pixels: P,
}

/// Decodes, fits and reduces a picture to the panel's greys.
#[cfg(test)]
pub(crate) fn prepare(bytes: &[u8], width: u32, height: u32) -> Option<(u32, u32, Vec<u8>)> {
    prepare_for(bytes, width, height, false)
        .map(|picture| (picture.width, picture.height, picture.pixels))
}

/// Decodes and fits a picture for a panel with or without colour. A colour
/// panel still gets grey for a picture that has no colour in it, at a
/// third of the bytes.
pub(crate) fn prepare_for(bytes: &[u8], width: u32, height: u32, colour: bool) -> Option<Prepared> {
    let decoded = if colour {
        kobo_image::decode_colour(bytes)
    } else {
        kobo_image::decode(bytes)
    };
    let mut picture = decoded.ok()?.fit_enlarging(width, height).ok()?;
    let (width, height) = (picture.width(), picture.height());
    if colour && picture.colour().is_some() {
        // The controller quantises colour against its own filter; dithering
        // the grey first would put the grain in twice.
        let rgb = picture.into_colour()?;
        return Some(Prepared {
            width,
            height,
            colour: true,
            pixels: rgb,
        });
    }
    picture.dither(kobo_image::PANEL_GREYS);
    Some(Prepared {
        width,
        height,
        colour: false,
        pixels: picture.into_grey(),
    })
}

const PACKED_GREY: &[u8; 4] = b"KGR1";
const PACKED_RGB: &[u8; 4] = b"KRG1";

/// A fitted picture as kept on the shelf: a tag naming grey or colour, the
/// size, the pixels.
#[must_use]
pub fn pack(picture: &Prepared) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + picture.pixels.len());
    out.extend_from_slice(if picture.colour {
        PACKED_RGB
    } else {
        PACKED_GREY
    });
    out.extend_from_slice(&picture.width.to_le_bytes());
    out.extend_from_slice(&picture.height.to_le_bytes());
    out.extend_from_slice(&picture.pixels);
    out
}

/// Reads a kept picture back, refusing one whose size does not match its
/// pixels: a torn write must not reach the panel as a skewed picture.
#[must_use]
pub fn unpack(blob: &[u8]) -> Option<Prepared<&[u8]>> {
    let (colour, rest) = if let Some(rest) = blob.strip_prefix(PACKED_GREY) {
        (false, rest)
    } else {
        (true, blob.strip_prefix(PACKED_RGB)?)
    };
    let (width, rest) = rest.split_first_chunk::<4>()?;
    let (height, pixels) = rest.split_first_chunk::<4>()?;
    let width = u32::from_le_bytes(*width);
    let height = u32::from_le_bytes(*height);
    let area = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(if colour { 3 } else { 1 })?;
    (width > 0 && height > 0 && pixels.len() == area).then_some(Prepared {
        width,
        height,
        colour,
        pixels,
    })
}
