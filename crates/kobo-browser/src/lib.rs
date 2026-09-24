//! Where the browser is and how it got there.
//!
//! Pure state, no I/O: the app asks what a navigation means, fetches if told
//! to, and reports back. Keeping it here means Back and Forward can be tested
//! as transitions rather than as taps in a simulator.

use kobo_web_document::Url;

/// Entries kept. Older ones fall off the back.
pub const MAX_HISTORY: usize = 100;

/// One place in the history: an address and the page the reader was on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub url: Url,
    /// Zero-based page within the document, kept so that Back returns to the
    /// page that was being read rather than the top.
    pub page: usize,
}

/// What the app has to do to carry out a navigation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Go {
    /// Load this address; the document is not the one showing.
    Load(Url),
    /// Same document, move to the page holding this fragment, or the first
    /// page when there is none.
    Fragment(Option<String>),
    /// Same document, restore this page.
    Page(usize),
    /// Nothing to go to.
    Stay,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct History {
    entries: Vec<Entry>,
    /// Index of the current entry. Meaningless while `entries` is empty.
    current: usize,
}

impl History {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.current)
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    #[must_use]
    pub fn can_go_back(&self) -> bool {
        !self.entries.is_empty() && self.current > 0
    }

    #[must_use]
    pub fn can_go_forward(&self) -> bool {
        self.current + 1 < self.entries.len()
    }

    /// Records the page the reader is on now.
    pub fn set_page(&mut self, page: usize) {
        if let Some(entry) = self.entries.get_mut(self.current) {
            entry.page = page;
        }
    }

    /// Follows a link or an address typed in. Anything forward of here is
    /// dropped, as in every browser.
    pub fn navigate(&mut self, url: Url) -> Go {
        let go = match self.current() {
            Some(entry) if entry.url == url => return Go::Stay,
            Some(entry) if entry.url.same_document(&url) => {
                Go::Fragment(url.fragment().map(str::to_owned))
            }
            _ => Go::Load(url.clone()),
        };
        if !self.entries.is_empty() {
            self.entries.truncate(self.current + 1);
        }
        self.entries.push(Entry { url, page: 0 });
        if self.entries.len() > MAX_HISTORY {
            let excess = self.entries.len() - MAX_HISTORY;
            self.entries.drain(..excess);
        }
        self.current = self.entries.len() - 1;
        go
    }

    /// Steps back one entry.
    pub fn back(&mut self) -> Go {
        if !self.can_go_back() {
            return Go::Stay;
        }
        self.step(self.current - 1)
    }

    /// Steps forward one entry.
    pub fn forward(&mut self) -> Go {
        if !self.can_go_forward() {
            return Go::Stay;
        }
        self.step(self.current + 1)
    }

    fn step(&mut self, to: usize) -> Go {
        let from = self.entries[self.current].url.clone();
        self.current = to;
        let entry = &self.entries[to];
        if entry.url.same_document(&from) {
            Go::Page(entry.page)
        } else {
            Go::Load(entry.url.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).expect("url")
    }

    #[test]
    fn back_and_forward_walk_the_entries_and_restore_pages() {
        let mut history = History::new();
        assert_eq!(history.back(), Go::Stay);
        assert_eq!(
            history.navigate(url("https://a.example/")),
            Go::Load(url("https://a.example/"))
        );
        history.set_page(4);
        assert_eq!(
            history.navigate(url("https://b.example/")),
            Go::Load(url("https://b.example/"))
        );
        history.set_page(2);
        assert_eq!(history.back(), Go::Load(url("https://a.example/")));
        assert_eq!(history.current().map(|e| e.page), Some(4));
        assert_eq!(history.forward(), Go::Load(url("https://b.example/")));
        assert_eq!(history.current().map(|e| e.page), Some(2));
        assert_eq!(history.forward(), Go::Stay);
    }

    #[test]
    fn navigating_from_the_middle_drops_what_was_ahead() {
        let mut history = History::new();
        for page in [
            "https://a.example/",
            "https://b.example/",
            "https://c.example/",
        ] {
            let _ = history.navigate(url(page));
        }
        let _ = history.back();
        let _ = history.back();
        let _ = history.navigate(url("https://d.example/"));
        let urls: Vec<String> = history
            .entries()
            .iter()
            .map(|e| e.url.to_string())
            .collect();
        assert_eq!(urls, ["https://a.example/", "https://d.example/"]);
        assert!(!history.can_go_forward());
    }

    #[test]
    fn a_fragment_in_the_same_document_does_not_load() {
        let mut history = History::new();
        let _ = history.navigate(url("https://a.example/p"));
        assert_eq!(
            history.navigate(url("https://a.example/p#part")),
            Go::Fragment(Some("part".into()))
        );
        history.set_page(7);
        assert_eq!(history.back(), Go::Page(0));
        assert_eq!(history.forward(), Go::Page(7));
        assert_eq!(history.navigate(url("https://a.example/p#part")), Go::Stay);
    }

    #[test]
    fn history_is_bounded() {
        let mut history = History::new();
        for n in 0..(MAX_HISTORY + 25) {
            let _ = history.navigate(url(&format!("https://a.example/{n}")));
        }
        assert_eq!(history.entries().len(), MAX_HISTORY);
        assert_eq!(
            history.entries()[0].url.to_string(),
            format!("https://a.example/{}", 25)
        );
        assert!(history.can_go_back());
        let mut steps = 0;
        while history.back() != Go::Stay {
            steps += 1;
        }
        assert_eq!(steps, MAX_HISTORY - 1);
    }
}
