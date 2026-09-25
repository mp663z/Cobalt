//! A small, private list of places the browser has visited or marked.
//!
//! The active Back/Forward chain is separate: this is a durable list for
//! returning to a page after the app restarts. Values are length-prefixed so
//! an address or title containing a newline cannot change the record shape.

use kobo_web_document::Url;

pub const MAX_BOOKMARKS: usize = 100;
pub const MAX_RECENT: usize = 100;
const MAGIC: &[u8] = b"kobo-browse-library 1\n";
const MAX_VALUE: usize = 256 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Place {
    pub url: Url,
    pub title: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Library {
    pub bookmarks: Vec<Place>,
    pub recent: Vec<Place>,
}

impl Library {
    pub fn visit(&mut self, url: Url, title: &str) {
        remember(&mut self.recent, url, title, MAX_RECENT);
    }

    pub fn toggle(&mut self, url: Url, title: &str) -> bool {
        if let Some(at) = self.bookmarks.iter().position(|place| place.url == url) {
            self.bookmarks.remove(at);
            false
        } else {
            remember(&mut self.bookmarks, url, title, MAX_BOOKMARKS);
            true
        }
    }

    #[must_use]
    pub fn contains(&self, url: &Url) -> bool {
        self.bookmarks.iter().any(|place| &place.url == url)
    }

    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for (kind, places) in [(b'B', &self.bookmarks), (b'R', &self.recent)] {
            for place in places {
                let url = place.url.to_string();
                if url.len() > 1000 || place.title.len() > 200 {
                    continue;
                }
                out.push(kind);
                out.extend_from_slice(&u32::try_from(url.len()).unwrap_or_default().to_le_bytes());
                out.extend_from_slice(
                    &u32::try_from(place.title.len())
                        .unwrap_or_default()
                        .to_le_bytes(),
                );
                out.extend_from_slice(url.as_bytes());
                out.extend_from_slice(place.title.as_bytes());
            }
        }
        out
    }

    /// Invalid or partial data yields an empty library instead of a partial list.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Self {
        let Some(mut rest) = bytes.strip_prefix(MAGIC) else {
            return Self::default();
        };
        if rest.len() > MAX_VALUE {
            return Self::default();
        }
        let mut library = Self::default();
        while !rest.is_empty() {
            if rest.len() < 9 {
                return Self::default();
            }
            let kind = rest[0];
            let url_len = u32::from_le_bytes([rest[1], rest[2], rest[3], rest[4]]) as usize;
            let title_len = u32::from_le_bytes([rest[5], rest[6], rest[7], rest[8]]) as usize;
            rest = &rest[9..];
            if url_len > 1000 || title_len > 200 || rest.len() < url_len + title_len {
                return Self::default();
            }
            let url = std::str::from_utf8(&rest[..url_len])
                .ok()
                .and_then(|s| Url::parse(s).ok());
            let title = std::str::from_utf8(&rest[url_len..url_len + title_len]).ok();
            rest = &rest[url_len + title_len..];
            if let (Some(url), Some(title)) = (url, title) {
                match kind {
                    b'B' if library.bookmarks.len() < MAX_BOOKMARKS => {
                        library.bookmarks.push(Place {
                            url,
                            title: title.to_owned(),
                        });
                    }
                    b'R' if library.recent.len() < MAX_RECENT => {
                        library.recent.push(Place {
                            url,
                            title: title.to_owned(),
                        });
                    }
                    b'B' | b'R' => {}
                    _ => return Self::default(),
                }
            }
        }
        library
    }
}

fn remember(places: &mut Vec<Place>, url: Url, title: &str, limit: usize) {
    if url.to_string().len() > 1000 {
        return;
    }
    let mut title = title.to_owned();
    while title.len() > 200 {
        title.pop();
    }
    places.retain(|place| place.url != url);
    places.insert(0, Place { url, title });
    places.truncate(limit);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }
    #[test]
    fn round_trip_and_revisit() {
        let mut library = Library::default();
        library.visit(url("https://example.com/a"), "A\nfirst");
        library.visit(url("https://example.com/b"), "B");
        library.visit(url("https://example.com/a"), "A again");
        assert_eq!(library.recent.len(), 2);
        assert_eq!(library.recent[0].title, "A again");
        assert!(library.toggle(url("https://example.com/a"), "A again"));
        assert_eq!(Library::decode(&library.encode()), library);
        assert!(!library.toggle(url("https://example.com/a"), "A again"));
        assert!(library.bookmarks.is_empty());
    }
    #[test]
    fn caps_and_corruption() {
        let mut library = Library::default();
        for i in 0..150 {
            let url = url(&format!("https://example.com/{i}"));
            library.visit(url.clone(), "Page");
            library.toggle(url, "Page");
        }
        assert_eq!(library.recent.len(), MAX_RECENT);
        assert_eq!(library.bookmarks.len(), MAX_BOOKMARKS);
        let bytes = library.encode();
        assert_eq!(Library::decode(&bytes), library);
        assert_eq!(
            Library::decode(&bytes[..bytes.len() - 1]),
            Library::default()
        );
        assert_eq!(Library::decode(b"garbage"), Library::default());
    }
}
