//! Browse: a reader's web browser for Cobalt.
//!
//! Pages are turned, not scrolled. This first build reads only the sample
//! pages it ships with; loading from the network comes next.

use std::process::ExitCode;

use kobo_browser_core::address::{self, DEFAULT_SEARCH};
use kobo_browser_core::fetch::{self, Failure, Kind};
use kobo_browser_core::{Go, History};
use kobo_sdk::keyboard::{TextEntry, Typing};
use kobo_sdk::{
    action_id, ActionId, Context, DisplayMetrics, Glyph, Header, KoboApp, ScreenBuilder, Task,
    TaskError, TaskId, TaskOutcome,
};
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
    /// A page on its way.
    Loading(Url),
    /// A page that did not arrive.
    Failed(Url, Failure),
    /// A response that is not a page, named.
    Unsupported(Url, &'static str),
}

/// A fetch in flight, and what to do with history when it lands.
struct Pending {
    task: TaskId,
    url: Url,
    /// For Back and Forward, which have already moved history, where it
    /// was before; `None` for a link or an address, recorded only once the
    /// page arrives.
    stepped_from: Option<usize>,
}

struct Browser {
    history: History,
    loaded: Option<Loaded>,
    view: View,
    /// The address field. While it is open it covers the page.
    address: TextEntry,
    pending: Option<Pending>,
    /// Where history was before a Back or Forward whose page is being loaded.
    stepped_from: Option<usize>,
    /// The last page that failed, for Retry.
    retry: Option<Url>,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            history: History::new(),
            loaded: None,
            view: View::Page,
            address: TextEntry::new().opened_by("address"),
            pending: None,
            stepped_from: None,
            retry: None,
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
            self.fetch(context, url, None);
            self.show(context);
            return;
        }
        let go = self.history.navigate(url.clone());
        self.carry_out(context, go);
    }

    fn carry_out(&mut self, context: &mut Context, go: Go) {
        self.view = View::Page;
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
        self.stepped_from = None;
        self.show(context);
    }

    /// Stops a page on its way and goes back to the one on screen.
    fn stop(&mut self, context: &mut Context) {
        if let Some(pending) = self.pending.take() {
            context.cancel(pending.task);
            if let Some(position) = pending.stepped_from {
                self.history.return_to(position);
            }
        }
        self.view = View::Page;
    }

    fn load(&mut self, context: &mut Context, url: &Url) {
        if let Some(html) = sample(url) {
            self.show_document(context, url, html.as_bytes());
        } else {
            let from = self.stepped_from.take();
            self.fetch(context, url, from);
        }
    }

    /// Asks the runtime for a page. One at a time: a new request replaces
    /// the one in flight.
    fn fetch(&mut self, context: &mut Context, url: &Url, stepped_from: Option<usize>) {
        if let Some(pending) = self.pending.take() {
            context.cancel(pending.task);
        }
        let task = context.spawn(Task::Fetch {
            url: url.without_fragment().to_string(),
            offset: 0,
            max_bytes: fetch::MAX_PAGE_BYTES,
            credential: None,
            headers: vec![Header::new("Accept", fetch::ACCEPT)],
        });
        match task {
            Some(task) => {
                self.pending = Some(Pending {
                    task,
                    url: url.clone(),
                    stepped_from,
                });
                self.view = View::Loading(url.clone());
            }
            None => self.failed(url, Failure::Unreachable, stepped_from),
        }
    }

    fn failed(&mut self, url: &Url, failure: Failure, stepped_from: Option<usize>) {
        if let Some(position) = stepped_from {
            // Back or Forward moved history onto a page that did not come;
            // the page still on screen is the one history should name.
            self.history.return_to(position);
        }
        self.retry = Some(url.clone());
        self.view = View::Failed(url.clone(), failure);
    }

    fn arrived(&mut self, context: &mut Context, pending: Pending, body: &[u8]) {
        let url = pending.url;
        match fetch::sniff(body) {
            Kind::Unsupported(what) => {
                if let Some(position) = pending.stepped_from {
                    self.history.return_to(position);
                }
                self.view = View::Unsupported(url, what);
            }
            kind => {
                if pending.stepped_from.is_none() {
                    let _ = self.history.navigate(url.clone());
                }
                if kind == Kind::Plain {
                    let html = plain_page(body);
                    self.show_document(context, &url, html.as_bytes());
                } else {
                    self.show_document(context, &url, body);
                }
                self.view = View::Page;
            }
        }
    }

    fn show_document(&mut self, context: &mut Context, url: &Url, bytes: &[u8]) {
        let document = parse_document(bytes, url, &Limits::DEFAULT);
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
        if self.address.is_open() {
            return ScreenBuilder::new("browser-address")
                .top_bar("Go to")
                .text_entry(&self.address, "An address, or words to search for", "Go");
        }
        match (&self.view, &self.loaded) {
            (View::Loading(url), _) => ScreenBuilder::new("browser-loading")
                .top_bar(url.host())
                .activity("Loading the page", None)
                .secondary(url.to_string())
                .button("cancel-load", "Cancel"),
            (View::Failed(url, failure), loaded) => {
                let (heading, sentence) = failure.explain();
                let mut builder = ScreenBuilder::new("browser-failed")
                    .top_bar(url.host())
                    .heading(heading)
                    .text(sentence)
                    .secondary(url.to_string());
                if failure.worth_retrying() {
                    builder = builder.button("retry", "Try again");
                }
                if loaded.is_some() {
                    builder = builder.button("return", "Back to the page");
                }
                builder
            }
            (View::Unsupported(url, what), loaded) => {
                let builder = ScreenBuilder::new("browser-unsupported")
                    .top_bar(url.host())
                    .heading("Not a web page")
                    .text(format!(
                        "This address leads to {what}. Browse shows web pages and plain text."
                    ))
                    .secondary(url.to_string());
                if loaded.is_some() {
                    builder.button("return", "Back to the page")
                } else {
                    builder
                }
            }
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
        match self.address.handle(action) {
            Some(Typing::Submitted(typed)) => {
                if let Some(to) = address::resolve(&typed, DEFAULT_SEARCH) {
                    self.open(context, to.url());
                    return;
                }
                self.show(context);
                return;
            }
            Some(Typing::Changed | Typing::Cancelled) => {
                self.show(context);
                return;
            }
            // The top bar's Back closes the field; it is not a step back
            // through history from behind a keyboard.
            None if self.address.is_open() => {
                if action == action_id("back") {
                    self.address.close();
                    self.show(context);
                }
                return;
            }
            None => {}
        }
        if action == action_id("next-page") || action == action_id("previous-page") {
            if let Some(page) = self.loaded.as_ref().map(|loaded| loaded.page) {
                let to = if action == action_id("next-page") {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                };
                self.turn_to(context, to);
            }
        } else if matches!(self.view, View::Loading(_))
            && (action == action_id("cancel-load") || action == action_id("back"))
        {
            self.stop(context);
        } else if action == action_id("retry") {
            if let Some(url) = self.retry.take() {
                self.fetch(context, &url, None);
            }
        } else if action == action_id("back") {
            self.stepped_from = Some(self.history.position());
            let go = self.history.back();
            self.carry_out(context, go);
            return;
        } else if action == action_id("forward") {
            self.stepped_from = Some(self.history.position());
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

    fn on_task(&mut self, context: &mut Context, task: TaskId, outcome: TaskOutcome) {
        let Some(pending) = self.pending.take_if(|pending| pending.task == task) else {
            // An answer to a request that was cancelled or replaced.
            return;
        };
        match outcome {
            TaskOutcome::Completed(body) => self.arrived(context, pending, &body),
            TaskOutcome::Failed(error) => {
                self.failed(&pending.url, failure(error), pending.stepped_from);
            }
            TaskOutcome::Cancelled => {
                if let Some(position) = pending.stepped_from {
                    self.history.return_to(position);
                }
                self.view = View::Page;
            }
        }
        self.show(context);
    }

    fn on_page_turn(&mut self, context: &mut Context, forward: bool) {
        if self.address.is_open() {
            return;
        }
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

/// What the reader is told for each way the runtime reports a failed fetch.
fn failure(error: TaskError) -> Failure {
    match error {
        TaskError::Denied => Failure::Denied,
        TaskError::NoCredential | TaskError::Unauthorized => Failure::Refused,
        TaskError::Offline => Failure::Offline,
        TaskError::Unreachable => Failure::Unreachable,
        TaskError::TooLarge => Failure::TooLarge,
        TaskError::TimedOut => Failure::TimedOut,
        TaskError::NotFound => Failure::NotFound,
        TaskError::RateLimited(seconds) => Failure::Busy {
            retry_after_seconds: seconds,
        },
    }
}

/// Plain text, as a page: one preformatted block, so its lines stay lines.
fn plain_page(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let mut html = String::with_capacity(text.len() + 32);
    html.push_str("<!doctype html><pre>");
    for character in text.chars() {
        match character {
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '&' => html.push_str("&amp;"),
            other => html.push(other),
        }
    }
    html.push_str("</pre>");
    html
}

#[cfg(all(test, debug_assertions))]
mod fixture_tests;
