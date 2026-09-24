//! Browse: a reader's web browser for Cobalt.
//!
//! Pages are turned, not scrolled. This first build reads only the sample
//! pages it ships with; loading from the network comes next.

use std::process::ExitCode;

use kobo_browser_core::{Go, History};
use kobo_sdk::{action_id, ActionId, Context, DisplayMetrics, Glyph, KoboApp, ScreenBuilder};
use kobo_web_document::{parse_document, Document, Limits, Url};
use kobo_web_layout::{link_action, page_screen, Paginator, Piece};

/// Where the sample pages live. `.invalid` can never be a real host, so a
/// sample address can never be confused with a page on the web.
const SAMPLES: &str = "https://samples.browse.invalid/";

const PAGES: &[(&str, &str)] = &[
    ("index.html", include_str!("../pages/index.html")),
    ("article.html", include_str!("../pages/article.html")),
    ("links.html", include_str!("../pages/links.html")),
];

fn sample(url: &Url) -> Option<&'static str> {
    let home = Url::parse(SAMPLES).ok()?;
    if url.origin() != home.origin() {
        return None;
    }
    let name = url.path().trim_start_matches('/');
    let name = if name.is_empty() { "index.html" } else { name };
    PAGES
        .iter()
        .find(|(page, _)| *page == name)
        .map(|(_, html)| *html)
}

/// A document on screen.
struct Loaded {
    url: Url,
    title: String,
    document: Document,
    paginator: Paginator,
    page: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum View {
    Page,
    /// The Links list, at this screen of it.
    Links(usize),
    /// Something the reader asked for that this build cannot do yet.
    Unavailable(Url),
}

struct Browser {
    history: History,
    loaded: Option<Loaded>,
    view: View,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            history: History::new(),
            loaded: None,
            view: View::Page,
        }
    }
}

impl Browser {
    fn open(&mut self, context: &mut Context, url: &Url) {
        let same = self
            .loaded
            .as_ref()
            .is_some_and(|loaded| loaded.url.same_document(url));
        if !same && sample(url).is_none() {
            self.view = View::Unavailable(url.clone());
            self.show(context);
            return;
        }
        let go = self.history.navigate(url.clone());
        self.carry_out(context, go);
    }

    fn carry_out(&mut self, context: &mut Context, go: Go) {
        match go {
            Go::Load(url) => self.load(context, &url),
            Go::Fragment(fragment) => {
                let page = fragment
                    .and_then(|fragment| self.fragment_page(context, &fragment))
                    .unwrap_or(0);
                self.turn_to(context, page);
            }
            Go::Page(page) => self.turn_to(context, page),
            Go::Stay => {}
        }
        self.view = View::Page;
        self.show(context);
    }

    fn load(&mut self, context: &mut Context, url: &Url) {
        let Some(html) = sample(url) else {
            self.view = View::Unavailable(url.clone());
            return;
        };
        let document = parse_document(html.as_bytes(), url, &Limits::DEFAULT);
        let title = document
            .title
            .clone()
            .unwrap_or_else(|| url.host().to_owned());
        let paginator = Paginator::for_document(&document);
        let restore = self.history.current().map_or(0, |entry| entry.page);
        self.loaded = Some(Loaded {
            url: url.clone(),
            title,
            document,
            paginator,
            page: 0,
        });
        let page = match url.fragment() {
            Some(fragment) if restore == 0 => self.fragment_page(context, fragment).unwrap_or(0),
            _ => restore,
        };
        self.turn_to(context, page);
    }

    /// Makes pages until `page` and the one after it exist, or the document
    /// runs out.
    fn make_pages(&mut self, context: &Context, page: usize) {
        let metrics = context.metrics();
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        let title = loaded.title.clone();
        let mut fits = |pieces: &[Piece]| {
            kobo_web_layout::fits(&page_screen(&title, pieces, 998, 999).build(), &metrics)
        };
        while loaded.paginator.pages().len() <= page.saturating_add(1)
            && loaded.paginator.next_page(&mut fits)
        {}
    }

    fn finish_pages(&mut self, context: &Context) {
        self.make_pages(context, usize::MAX - 1);
    }

    fn fragment_page(&mut self, context: &Context, fragment: &str) -> Option<usize> {
        let loaded = self.loaded.as_ref()?;
        if !loaded.paginator.has_fragment(fragment) {
            return None;
        }
        loop {
            let loaded = self.loaded.as_ref()?;
            if let Some(page) = loaded.paginator.page_of(fragment) {
                return Some(page);
            }
            let made = loaded.paginator.pages().len();
            self.make_pages(context, made);
            if self.loaded.as_ref()?.paginator.pages().len() == made {
                return None;
            }
        }
    }

    fn turn_to(&mut self, context: &Context, page: usize) {
        self.make_pages(context, page);
        // The count in the bar is only honest once every page exists.
        self.finish_pages(context);
        if let Some(loaded) = self.loaded.as_mut() {
            let last = loaded.paginator.pages().len().saturating_sub(1);
            loaded.page = page.min(last);
            self.history.set_page(loaded.page);
        }
    }

    fn current_pieces(&self) -> &[Piece] {
        self.loaded
            .as_ref()
            .and_then(|loaded| loaded.paginator.pages().get(loaded.page))
            .map_or(&[], Vec::as_slice)
    }

    /// Links on the page being read, in reading order: those drawn in the
    /// text and those in headings, quotations and tables, which are not.
    fn page_links(&self) -> Vec<usize> {
        let mut links: Vec<usize> = Vec::new();
        for link in self.current_pieces().iter().flat_map(Piece::all_links) {
            if !links.contains(&link) {
                links.push(link);
            }
        }
        links
    }

    fn show(&self, context: &mut Context) {
        context.set_screen(self.screen(&context.metrics()).build());
    }

    fn screen(&self, metrics: &DisplayMetrics) -> ScreenBuilder {
        match (&self.view, &self.loaded) {
            (View::Unavailable(url), _) => ScreenBuilder::new("browser-unavailable")
                .top_bar("Not in this build")
                .heading("Web pages come next")
                .text("This build of Browse reads only the sample pages it ships with. Loading pages from the network is the next step.")
                .secondary(url.to_string())
                .button("return", "Back to the page"),
            (View::Links(page), Some(loaded)) => {
                let here = self.page_links();
                let pages = links_pages(loaded, &here, metrics);
                let page = (*page).min(pages.len().saturating_sub(1));
                links_screen(
                    loaded,
                    pages.get(page).map_or(&[], Vec::as_slice),
                    here.len(),
                    page,
                    pages.len(),
                )
            }
            (_, Some(loaded)) => page_screen(
                &loaded.title,
                self.current_pieces(),
                loaded.page,
                loaded.paginator.pages().len(),
            ),
            (_, None) => ScreenBuilder::new("browser-empty")
                .top_bar("Browse")
                .text("Nothing to show."),
        }
    }

    fn link_named(&self, action: ActionId) -> Option<usize> {
        let loaded = self.loaded.as_ref()?;
        let candidates: Vec<usize> = match self.view {
            View::Links(_) => (0..loaded.document.links.len()).collect(),
            _ => self
                .current_pieces()
                .iter()
                .flat_map(Piece::links)
                .collect(),
        };
        candidates.into_iter().find(|&index| {
            index < loaded.document.links.len() && action_id(&link_action(index)) == action
        })
    }
}

impl KoboApp for Browser {
    fn on_start(&mut self, context: &mut Context) {
        if let Ok(home) = Url::parse(SAMPLES) {
            if let Ok(index) = home.join("index.html") {
                self.open(context, &index);
            }
        }
    }

    fn on_action(&mut self, context: &mut Context, action: ActionId) {
        if action == action_id("next-page") || action == action_id("previous-page") {
            if let Some(page) = self.loaded.as_ref().map(|loaded| loaded.page) {
                let to = if action == action_id("next-page") {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                };
                self.turn_to(context, to);
            }
        } else if action == action_id("back") {
            let go = self.history.back();
            self.carry_out(context, go);
            return;
        } else if action == action_id("forward") {
            let go = self.history.forward();
            self.carry_out(context, go);
            return;
        } else if action == action_id("links") {
            self.view = View::Links(0);
        } else if action == action_id("links-next") || action == action_id("links-previous") {
            if let View::Links(page) = self.view {
                self.view = View::Links(if action == action_id("links-next") {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                });
            }
        } else if action == action_id("return") {
            self.view = View::Page;
        } else if let Some(index) = self.link_named(action) {
            if let Some(target) = self
                .loaded
                .as_ref()
                .and_then(|loaded| loaded.document.links.get(index))
                .map(|link| link.target.clone())
            {
                self.open(context, &target);
                return;
            }
        }
        self.show(context);
    }

    fn on_page_turn(&mut self, context: &mut Context, forward: bool) {
        let name = match (&self.view, forward) {
            (View::Links(_), true) => "links-next",
            (View::Links(_), false) => "links-previous",
            (_, true) => "next-page",
            (_, false) => "previous-page",
        };
        self.on_action(context, action_id(name));
    }
}

/// One screen of the Links list: `entries` are link indices, `here` how
/// many of the document's links are on the page being read.
///
/// The page's own links come first. A page of prose often has none, and a
/// Links screen that only said so would send the reader paging to find one.
fn links_screen(
    loaded: &Loaded,
    entries: &[usize],
    here: usize,
    page: usize,
    of: usize,
) -> ScreenBuilder {
    let total = loaded.document.links.len();
    let summary = match here {
        0 => format!("None on this page, {total} in the document."),
        _ => format!("{here} on this page, {total} in the document. These come first."),
    };
    let builder = ScreenBuilder::new("browser-links")
        .top_bar("Links")
        .top_bar_action("return", "Done")
        .secondary(summary)
        .page_turns("links-previous", "links-next")
        .page_position(
            u16::try_from(page + 1).unwrap_or(u16::MAX),
            u16::try_from(of.max(1)).unwrap_or(u16::MAX),
        );
    if total == 0 {
        return builder.text("This page has no links.");
    }
    builder.rows(entries.iter().filter_map(|&index| {
        let link = loaded.document.links.get(index)?;
        let title = if link.text.trim().is_empty() {
            link.target.to_string()
        } else {
            link.text.clone()
        };
        Some((
            link_action(index),
            title,
            link.target.to_string(),
            Glyph::Bookmark,
        ))
    }))
}

/// Cuts the Links list into screens that fit, the page's own links first.
fn links_pages(loaded: &Loaded, here: &[usize], metrics: &DisplayMetrics) -> Vec<Vec<usize>> {
    let mut entries: Vec<usize> = here.to_vec();
    entries.extend((0..loaded.document.links.len()).filter(|index| !here.contains(index)));
    let mut pages = Vec::new();
    let mut rest = entries.as_slice();
    while !rest.is_empty() {
        let fits = |count: usize| {
            count <= kobo_sdk::MAX_ROWS
                && kobo_web_layout::fits(
                    &links_screen(loaded, &rest[..count], here.len(), 998, 999).build(),
                    metrics,
                )
        };
        // Always at least one row, so a list too tall for one row still ends.
        let (mut good, mut bad) = (1, rest.len() + 1);
        while bad - good > 1 {
            let middle = good + (bad - good) / 2;
            if fits(middle) {
                good = middle;
            } else {
                bad = middle;
            }
        }
        pages.push(rest[..good].to_vec());
        rest = &rest[good..];
    }
    pages
}

fn main() -> ExitCode {
    match kobo_sdk::run("browser", Browser::default()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("browser: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
