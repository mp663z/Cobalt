//! Pages kept on the reader, for reading again without a network.
//!
//! Every page that arrives is written to the shelf and recorded in
//! [`kobo_browser_core::cache::Index`]. When a later fetch of the same address
//! fails because there is no way to the site, the kept copy is read back and
//! shown, labelled with when it was fetched so nobody mistakes it for today's.

use kobo_browser_core::cache::{Index, MAX_BYTES};
use kobo_sdk::clock::{Clock, Snapshot, SystemClock};
use kobo_sdk::{Context, ShelfDownload, ShelfProgress, ShelfUpload, StoreResult};

/// The store key the index lives under.
pub const INDEX_KEY: &str = "page-cache-index";

/// What a store answer meant for the saved pages.
#[derive(Debug, Eq, PartialEq)]
pub enum Heard {
    /// Not ours.
    Elsewhere,
    /// Ours, and nothing for the app to do.
    Handled,
    /// A kept copy was read back: its address, bytes and fetch time.
    Read {
        url: String,
        bytes: Vec<u8>,
        fetched: u64,
    },
    /// A kept copy could not be read back.
    Unreadable { url: String },
}

pub struct Saved {
    /// `None` until the stored index has been read.
    index: Option<Index>,
    /// Blob names on the shelf, held until the index arrives to compare.
    shelf: Option<Vec<String>>,
    uploads: Vec<ShelfUpload>,
    download: Option<(ShelfDownload, String, u64)>,
    clock: Box<dyn Clock>,
}

impl std::fmt::Debug for Saved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Saved")
            .field("index", &self.index)
            .field("uploads", &self.uploads.len())
            .field("reading", &self.download.as_ref().map(|(_, url, _)| url))
            .finish_non_exhaustive()
    }
}

impl Default for Saved {
    fn default() -> Self {
        Self::with_clock(reader_clock())
    }
}

/// The reader's clock at the offset the runtime was started with, or none.
fn reader_clock() -> Box<dyn Clock> {
    let minutes = std::env::var("KOBO_UTC_OFFSET_MINUTES")
        .ok()
        .and_then(|value| value.parse::<i16>().ok())
        .unwrap_or(0);
    match SystemClock::new(minutes).or_else(|_| SystemClock::new(0)) {
        Ok(clock) => Box::new(clock),
        Err(_) => Box::new(NoClock),
    }
}

struct NoClock;

impl Clock for NoClock {
    fn now(&self) -> Result<Snapshot, kobo_sdk::clock::Error> {
        Err(kobo_sdk::clock::Error::Unavailable)
    }
}

impl Saved {
    #[must_use]
    pub fn with_clock(clock: Box<dyn Clock>) -> Self {
        Self {
            index: None,
            shelf: None,
            uploads: Vec::new(),
            download: None,
            clock,
        }
    }

    /// Reads the index and the shelf's list, to start.
    pub fn start(context: &mut Context) {
        context.store().load(INDEX_KEY);
        context.shelf().list();
    }

    fn now_seconds(&self) -> u64 {
        self.clock.now().map_or(0, |now| now.unix_millis / 1000)
    }

    /// Keeps a page that just arrived.
    pub fn keep(&mut self, context: &mut Context, url: &str, bytes: &[u8]) {
        let fetched = self.now_seconds();
        let Some(index) = self.index.as_mut() else {
            return;
        };
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if size > MAX_BYTES {
            return;
        }
        let Some((name, evicted)) = index.insert(url, size, fetched) else {
            return;
        };
        for old in evicted {
            self.uploads.retain(|upload| upload.name() != old);
            context.shelf().remove(old);
        }
        self.uploads.retain(|upload| upload.name() != name);
        let mut upload = ShelfUpload::new(name, bytes.to_vec());
        upload.start(context);
        self.uploads.push(upload);
        self.save_index(context);
    }

    /// Records that `url` was read, so its copy is kept longer.
    pub fn touch(&mut self, context: &mut Context, url: &str) {
        if let Some(index) = self.index.as_mut() {
            if index.find(url).is_some() {
                index.touch(url);
                self.save_index(context);
            }
        }
    }

    /// Starts reading the kept copy of `url`. Returns false when there is
    /// none to read.
    pub fn read(&mut self, context: &mut Context, url: &str) -> bool {
        let Some(entry) = self.index.as_ref().and_then(|index| index.find(url)) else {
            return false;
        };
        let limit = usize::try_from(entry.bytes).unwrap_or(usize::MAX);
        let mut download = ShelfDownload::new(entry.name.clone()).at_most(limit);
        download.start(context);
        self.download = Some((download, url.to_owned(), entry.fetched));
        true
    }

    /// Stops a read that is no longer wanted.
    pub fn stop_reading(&mut self) {
        self.download = None;
    }

    fn save_index(&self, context: &mut Context) {
        if let Some(index) = &self.index {
            context.store().save(INDEX_KEY, index.encode());
        }
    }

    /// Feeds one store answer through the saved pages. `from` is the key or
    /// blob name the answer is for, when the runtime says: a refusal carries
    /// no name of its own, and must not fail a transfer it was not about.
    pub fn heard(
        &mut self,
        context: &mut Context,
        from: Option<&str>,
        result: &StoreResult,
    ) -> Heard {
        let refused_elsewhere = |name: &str| {
            matches!(result, StoreResult::Denied(_)) && from.is_some_and(|from| from != name)
        };
        match result {
            StoreResult::Denied(_) if from == Some(INDEX_KEY) => return Heard::Handled,
            StoreResult::Loaded { key, value } if key == INDEX_KEY => {
                self.index = Some(value.as_deref().map(Index::decode).unwrap_or_default());
                self.sweep(context);
                return Heard::Handled;
            }
            StoreResult::Saved { key } if key == INDEX_KEY => return Heard::Handled,
            StoreResult::Shelf(blobs) => {
                self.shelf = Some(blobs.iter().map(|(name, _)| name.clone()).collect());
                self.sweep(context);
                return Heard::Handled;
            }
            StoreResult::ShelfRemoved { name } if name.starts_with("page-") => {
                return Heard::Handled;
            }
            _ => {}
        }
        if let Some((download, url, fetched)) = self
            .download
            .as_mut()
            .filter(|(download, _, _)| !refused_elsewhere(download.name()))
        {
            match download.advance(context, result) {
                ShelfProgress::Elsewhere => {}
                ShelfProgress::Moving { .. } => return Heard::Handled,
                ShelfProgress::Done => {
                    let heard = Heard::Read {
                        url: url.clone(),
                        bytes: download.bytes().to_vec(),
                        fetched: *fetched,
                    };
                    self.download = None;
                    return heard;
                }
                ShelfProgress::Failed(_) => {
                    let url = url.clone();
                    self.download = None;
                    self.forget_url(context, &url);
                    return Heard::Unreadable { url };
                }
            }
        }
        for at in 0..self.uploads.len() {
            if refused_elsewhere(self.uploads[at].name()) {
                continue;
            }
            match self.uploads[at].advance(context, result) {
                ShelfProgress::Elsewhere => continue,
                ShelfProgress::Moving { .. } => return Heard::Handled,
                ShelfProgress::Done => {
                    self.uploads.remove(at);
                    return Heard::Handled;
                }
                ShelfProgress::Failed(_) => {
                    // Storage full or refused: the page simply is not kept.
                    let name = self.uploads.remove(at).name().to_owned();
                    if let Some(index) = self.index.as_mut() {
                        index.remove(&name);
                    }
                    self.save_index(context);
                    return Heard::Handled;
                }
            }
        }
        Heard::Elsewhere
    }

    fn forget_url(&mut self, context: &mut Context, url: &str) {
        let Some(name) = self
            .index
            .as_ref()
            .and_then(|index| index.find(url))
            .map(|entry| entry.name.clone())
        else {
            return;
        };
        if let Some(index) = self.index.as_mut() {
            index.remove(&name);
        }
        context.shelf().remove(name);
        self.save_index(context);
    }

    /// Once both the index and the shelf's list are in, removes page blobs
    /// the index does not know: left by an index that was lost or a write
    /// that never finished.
    fn sweep(&mut self, context: &mut Context) {
        let (Some(index), Some(names)) = (&self.index, self.shelf.take()) else {
            return;
        };
        for orphan in index.orphans(names.iter().map(String::as_str)) {
            context.shelf().remove(orphan);
        }
    }

    /// When a kept copy was fetched, as a reader would say it, or `None`
    /// when the time is unknown.
    #[must_use]
    pub fn when(&self, fetched: u64) -> Option<String> {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        if fetched == 0 {
            return None;
        }
        let offset = self.clock.now().map_or(0, |now| now.utc_offset_minutes);
        let then = Snapshot {
            unix_millis: fetched.saturating_mul(1000),
            monotonic_millis: 0,
            utc_offset_minutes: offset,
        };
        let date = then.date()?;
        let (hour, minute) = then.hour_minute()?;
        let today = self.clock.now().ok().and_then(Snapshot::date);
        let month = MONTHS.get(usize::from(date.month.saturating_sub(1)))?;
        Some(if today == Some(date) {
            format!("today at {hour:02}:{minute:02}")
        } else {
            format!(
                "{} {month} {} at {hour:02}:{minute:02}",
                date.day, date.year
            )
        })
    }

    #[cfg(test)]
    pub fn index(&self) -> Option<&Index> {
        self.index.as_ref()
    }
}
