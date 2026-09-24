//! Which pages are kept on the reader, and which go when room runs out.
//!
//! A page is kept as a shelf blob holding the bytes that arrived. This is the
//! index of those blobs: what each holds, when it was fetched and when it was
//! last read. The index is small enough to live under one store key, and it is
//! the only place the cache's size is counted, so eviction never has to list
//! the shelf.
//!
//! Least recently read goes first. A page read this morning is more likely
//! to be read again offline than one fetched last week and never reopened.

/// Bytes of pages kept, all together.
///
/// A tenth of what the runtime allows an app on its shelf, which is room for
/// a few hundred ordinary articles and leaves the rest for pictures later.
pub const MAX_BYTES: u64 = 24 * 1024 * 1024;

/// Pages kept, whatever their size. Bounds the index as well as the shelf.
pub const MAX_ENTRIES: usize = 200;

/// Addresses longer than this are not kept. With [`MAX_ENTRIES`] it keeps the
/// index under the store's 256 KiB per value, and a page with an address this
/// long is almost always a search result or a tracking link, not something to
/// read again.
pub const MAX_URL: usize = 1_000;

const MAGIC: &str = "kobo-browse-cache 1";

/// One kept page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The shelf blob holding the page.
    pub name: String,
    /// The address it was fetched from, without a fragment.
    pub url: String,
    pub bytes: u64,
    /// When it was fetched, in Unix seconds. Zero when the clock was unknown.
    pub fetched: u64,
    /// When it was last read, as a count of reads across the whole cache,
    /// so the order survives a clock that is wrong or jumps.
    pub used: u64,
}

/// The index of kept pages.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Index {
    entries: Vec<Entry>,
    reads: u64,
}

/// The blob name a page is kept under: a hash of its address, so the name is
/// short, safe for the shelf, and the same every time.
#[must_use]
pub fn blob_name(url: &str) -> String {
    // FNV-1a. Not for security: a collision costs one page being replaced by
    // another, which the address stored beside it catches on the way out.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("page-{hash:016x}")
}

impl Index {
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Bytes kept, all together.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.entries.iter().map(|entry| entry.bytes).sum()
    }

    /// The kept copy of `url`, if there is one.
    #[must_use]
    pub fn find(&self, url: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.url == url)
    }

    /// Records that `url` was read, so it is kept longer.
    pub fn touch(&mut self, url: &str) {
        self.reads = self.reads.saturating_add(1);
        let reads = self.reads;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.url == url) {
            entry.used = reads;
        }
    }

    /// Records a page kept under [`blob_name`] and returns the blobs to
    /// remove to make room, oldest read first. Returns `None`, and changes
    /// nothing, for a page that is not worth keeping: too large on its own or
    /// with an address past [`MAX_URL`].
    pub fn insert(&mut self, url: &str, bytes: u64, fetched: u64) -> Option<(String, Vec<String>)> {
        if url.len() > MAX_URL || bytes > MAX_BYTES || url.contains(['\t', '\n']) {
            return None;
        }
        let name = blob_name(url);
        // Another address with the same hash, or this address again: either
        // way the blob is about to be overwritten.
        self.entries.retain(|entry| entry.name != name);
        self.reads = self.reads.saturating_add(1);
        self.entries.push(Entry {
            name: name.clone(),
            url: url.to_owned(),
            bytes,
            fetched,
            used: self.reads,
        });
        let mut evicted = Vec::new();
        while self.entries.len() > MAX_ENTRIES || self.bytes() > MAX_BYTES {
            let Some(oldest) = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.name != name)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(at, _)| at)
            else {
                break;
            };
            evicted.push(self.entries.remove(oldest).name);
        }
        Some((name, evicted))
    }

    /// Forgets a blob, for one that turned out to be missing or unreadable.
    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|entry| entry.name != name);
    }

    /// The index as stored.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = format!("{MAGIC}\n{}\n", self.reads);
        for entry in &self.entries {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\n",
                entry.name, entry.bytes, entry.fetched, entry.used, entry.url
            ));
        }
        out.into_bytes()
    }

    /// Reads a stored index. Anything unreadable gives an empty index rather
    /// than an error: the worst a lost index costs is pages fetched again,
    /// and the orphaned blobs are found by [`Index::orphans`].
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Self {
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Self::default();
        };
        let mut lines = text.lines();
        if lines.next() != Some(MAGIC) {
            return Self::default();
        }
        let reads = lines.next().and_then(|line| line.parse().ok()).unwrap_or(0);
        let mut index = Self {
            entries: Vec::new(),
            reads,
        };
        for line in lines.take(MAX_ENTRIES) {
            let mut fields = line.splitn(5, '\t');
            let (Some(name), Some(bytes), Some(fetched), Some(used), Some(url)) = (
                fields.next(),
                fields.next().and_then(|f| f.parse().ok()),
                fields.next().and_then(|f| f.parse().ok()),
                fields.next().and_then(|f| f.parse().ok()),
                fields.next(),
            ) else {
                continue;
            };
            if name != blob_name(url) || url.len() > MAX_URL {
                continue;
            }
            index.entries.push(Entry {
                name: name.to_owned(),
                url: url.to_owned(),
                bytes,
                fetched,
                used,
            });
        }
        index
    }

    /// Page blobs on the shelf that the index does not know, to remove.
    #[must_use]
    pub fn orphans<'a>(&self, on_shelf: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        on_shelf
            .into_iter()
            .filter(|name| {
                name.starts_with("page-") && !self.entries.iter().any(|e| e.name == *name)
            })
            .map(str::to_owned)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_stable_short_and_shelf_safe() {
        let name = blob_name("https://example.com/");
        assert_eq!(name, blob_name("https://example.com/"));
        assert_ne!(name, blob_name("https://example.com/a"));
        assert_eq!(name.len(), 21);
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn the_least_recently_read_page_goes_first() {
        let mut index = Index::default();
        let big = MAX_BYTES / 3;
        index.insert("https://a.example/", big, 1).unwrap();
        index.insert("https://b.example/", big, 2).unwrap();
        index.insert("https://c.example/", big, 3).unwrap();
        index.touch("https://a.example/");
        let (_, evicted) = index.insert("https://d.example/", big, 4).unwrap();
        assert_eq!(evicted, vec![blob_name("https://b.example/")]);
        assert!(index.find("https://a.example/").is_some());
        assert!(index.bytes() <= MAX_BYTES);
    }

    #[test]
    fn the_entry_count_is_bounded_too() {
        let mut index = Index::default();
        for n in 0..MAX_ENTRIES + 10 {
            index
                .insert(&format!("https://example.com/{n}"), 10, 0)
                .unwrap();
        }
        assert_eq!(index.entries().len(), MAX_ENTRIES);
        assert!(index.find("https://example.com/0").is_none());
    }

    #[test]
    fn keeping_a_page_again_replaces_it() {
        let mut index = Index::default();
        index.insert("https://a.example/", 10, 1).unwrap();
        index.insert("https://a.example/", 20, 2).unwrap();
        assert_eq!(index.entries().len(), 1);
        assert_eq!(index.find("https://a.example/").unwrap().bytes, 20);
    }

    #[test]
    fn pages_not_worth_keeping_are_refused() {
        let mut index = Index::default();
        assert!(index.insert(&"x".repeat(MAX_URL + 1), 10, 0).is_none());
        assert!(index
            .insert("https://a.example/", MAX_BYTES + 1, 0)
            .is_none());
        assert!(index.insert("https://a.example/\tx", 1, 0).is_none());
        assert!(index.entries().is_empty());
    }

    #[test]
    fn the_index_survives_being_stored_and_a_full_one_fits_a_store_value() {
        let mut index = Index::default();
        for n in 0..MAX_ENTRIES {
            let url = format!("https://example.com/{n}/{}", "a".repeat(MAX_URL - 40));
            index.insert(&url, 1, 1_700_000_000).unwrap();
        }
        let stored = index.encode();
        assert!(stored.len() <= 256 * 1024, "{} bytes", stored.len());
        assert_eq!(Index::decode(&stored), index);
    }

    #[test]
    fn a_damaged_index_reads_as_what_survives() {
        assert_eq!(Index::decode(b"\xff\xfe"), Index::default());
        assert_eq!(Index::decode(b"something else\n"), Index::default());
        let good = format!(
            "{MAGIC}\n7\n{}\t5\t1\t2\thttps://a.example/\nbroken line\npage-0000000000000000\t1\t1\t1\thttps://b.example/\n",
            blob_name("https://a.example/")
        );
        let index = Index::decode(good.as_bytes());
        assert_eq!(
            index.entries().len(),
            1,
            "the line whose name does not match its address is dropped"
        );
        assert_eq!(index.find("https://a.example/").unwrap().bytes, 5);
    }

    #[test]
    fn blobs_the_index_does_not_know_are_orphans() {
        let mut index = Index::default();
        let (kept, _) = index.insert("https://a.example/", 1, 0).unwrap();
        let orphans = index.orphans([kept.as_str(), "page-dead", "other-blob"]);
        assert_eq!(orphans, vec!["page-dead".to_owned()]);
    }
}
