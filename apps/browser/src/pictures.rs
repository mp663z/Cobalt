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
    Shown,
    /// Refused, unreachable or undecodable. The room stays empty and the
    /// description under it says what it was.
    Failed,
}

#[derive(Debug, Default)]
pub struct Pictures {
    states: BTreeMap<usize, State>,
}

impl Pictures {
    /// Forgets every picture: cancels what is on its way and releases what
    /// the runtime is holding. For a new page.
    pub fn clear(&mut self, context: &mut Context) {
        for (image, state) in std::mem::take(&mut self.states) {
            match state {
                State::Fetching(task) => context.cancel(task),
                State::Shown => context.drop_picture(picture_handle(image)),
                State::Failed => {}
            }
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn state(&self, image: usize) -> Option<State> {
        self.states.get(&image).copied()
    }

    /// Asks for the pictures `wanted` names that have not been asked for,
    /// within the caps. `source` gives an image's address.
    pub fn want(
        &mut self,
        context: &mut Context,
        wanted: &[usize],
        source: impl Fn(usize) -> Option<String>,
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
    /// hands it to the runtime. Returns whether there is now something new to
    /// draw.
    pub fn arrived(
        &mut self,
        context: &mut Context,
        image: usize,
        outcome: TaskOutcome,
        room: Option<(u32, u32)>,
    ) -> bool {
        let shown = match (outcome, room) {
            (TaskOutcome::Completed(bytes), Some((width, height))) => {
                prepare(&bytes, width, height).and_then(|(width, height, grey)| {
                    context.put_picture(picture_handle(image), width, height, grey)
                })
            }
            _ => None,
        };
        let state = if shown.is_some() {
            State::Shown
        } else {
            State::Failed
        };
        self.states.insert(image, state);
        shown.is_some()
    }
}

/// Decodes, fits and reduces a picture to the panel's greys.
fn prepare(bytes: &[u8], width: u32, height: u32) -> Option<(u32, u32, Vec<u8>)> {
    let mut picture = kobo_image::decode(bytes)
        .ok()?
        .fit_enlarging(width, height)
        .ok()?;
    picture.dither(kobo_image::PANEL_GREYS);
    Some((picture.width(), picture.height(), picture.into_grey()))
}
