//! Browse: a reader's web browser for Cobalt.
//!
//! Pages are turned, not scrolled. This first build reads only the sample
//! pages it ships with; loading from the network comes next.

use std::process::ExitCode;

use kobo_browser_core::{Go, History};
use kobo_sdk::{action_id, ActionId, Context, Glyph, KoboApp, ScreenBuilder};
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
    Links,
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

    /// Links drawn on the page being read, in reading order.
    fn page_links(&self) -> Vec<usize> {
        let mut links: Vec<usize> = Vec::new();
        for link in self.current_pieces().iter().flat_map(Piece::links) {
            if !links.contains(&link) {
                links.push(link);
            }
        }
        links
    }

    fn show(&self, context: &mut Context) {
        context.set_screen(self.screen().build());
    }

    fn screen(&self) -> ScreenBuilder {
        match (&self.view, &self.loaded) {
            (View::Unavailable(url), _) => ScreenBuilder::new("browser-unavailable")
                .top_bar("Not in this build")
                .heading("Web pages come next")
                .text("This build of Browse reads only the sample pages it ships with. Loading pages from the network is the next step.")
                .secondary(url.to_string())
                .button("return", "Back to the page"),
            (View::Links, Some(loaded)) => {
                let links = self.page_links();
                let builder = ScreenBuilder::new("browser-links")
                    .top_bar("Links on this page")
                    .top_bar_action("return", "Done");
                if links.is_empty() {
                    builder.text("There are no links on this page.")
                } else {
                    builder.rows(links.into_iter().filter_map(|index| {
                        let link = loaded.document.links.get(index)?;
                        let title = if link.text.trim().is_empty() {
                            link.target.to_string()
                        } else {
                            link.text.clone()
                        };
                        Some((link_action(index), title, link.target.to_string(), Glyph::Bookmark))
                    }))
                }
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
            View::Links => self.page_links(),
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
            self.view = View::Links;
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
        let name = if forward {
            "next-page"
        } else {
            "previous-page"
        };
        self.on_action(context, action_id(name));
    }
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
