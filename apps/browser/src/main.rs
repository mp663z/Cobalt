//! Browse: a reader's web browser for Cobalt.
//!
//! Pages are turned, not scrolled. This first build reads only the sample
//! pages it ships with; loading from the network comes next.

use std::collections::BTreeMap;
use std::process::ExitCode;

use kobo_browser_core::address::{self, DEFAULT_SEARCH};
use kobo_browser_core::fetch::{self, Failure, Kind};
use kobo_browser_core::{Go, History};
use kobo_sdk::keyboard::{TextEntry, Typing};
use kobo_sdk::{
    action_id, ActionId, Context, DisplayMetrics, Glyph, Header, Heartbeat, KoboApp, ScreenBuilder,
    StoreResult, Task, TaskError, TaskId, TaskOutcome,
};
use kobo_web_document::{parse_document, Document, Field, Limits, Url};
use kobo_web_layout::{link_action, page_screen_with, picture_handle, pieces, Paginator, Piece};

mod forms;
mod pictures;
mod saved;
use pictures::Pictures;
use saved::{Heard, Saved};

/// Where the sample pages live. `.invalid` can never be a real host, so a
/// sample address can never be confused with a page on the web.
const SAMPLES: &str = "https://samples.browse.invalid/";

/// Pages made per background step. A page of a long article took about 10 ms
/// in a release build on a desktop and will be several times that on the
/// reader, so two keeps a tap from waiting long behind a step.
const PAGES_PER_STEP: usize = 2;

const PAGES: &[(&str, &str)] = &[
    ("index.html", include_str!("../pages/index.html")),
    ("article.html", include_str!("../pages/article.html")),
    ("links.html", include_str!("../pages/links.html")),
    ("forms.html", include_str!("../pages/forms.html")),
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
    /// Pieces put before the document's own: the saved-copy note.
    lead: usize,
    note: Option<String>,
    /// The whole page, set aside while `document` is its reader view.
    full: Option<Document>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum View {
    Page,
    /// The Links list, at this screen of it.
    Links(usize),
    /// The heading list, at this screen of it.
    Sections(usize),
    /// The navigation choices where the reader switch uses a bar slot.
    Navigate,
    /// One form, at this screen of its controls.
    Form(usize, usize),
    /// Choices for one select or radio field.
    Options(usize, usize, usize),
    /// A page on its way.
    Loading(Url),
    /// A page that did not arrive.
    Failed(Url, Failure),
    /// A response that is not a page, named.
    Unsupported(Url, &'static str),
    /// A form cannot be sent with the browser's current network support.
    FormUnavailable(String),
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
    form_entry: TextEntry,
    editing: Option<(usize, usize)>,
    pending: Option<Pending>,
    /// Where history was before a Back or Forward whose page is being loaded.
    stepped_from: Option<usize>,
    /// The last page that failed, for Retry.
    retry: Option<Url>,
    /// A zero-second nap that brings control back to paginate some more of
    /// the page being read, so a long page shows its first screen at once.
    pager: Option<TaskId>,
    /// Counts the wait on the Loading screen, so a slow site looks slow
    /// rather than stuck.
    clock: Heartbeat,
    /// Pictures fetched for the page being read.
    pictures: Pictures,
    /// Pages kept on the reader for reading without a network.
    saved: Saved,
    /// A kept copy being read back in place of a page that did not come:
    /// its address, why the fetch failed, and the history step it came from.
    reading: Option<(Url, Failure, Option<usize>)>,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            history: History::new(),
            loaded: None,
            view: View::Page,
            address: TextEntry::new().opened_by("address"),
            form_entry: TextEntry::new(),
            editing: None,
            pending: None,
            stepped_from: None,
            retry: None,
            pager: None,
            clock: Heartbeat::default(),
            pictures: Pictures::default(),
            saved: Saved::default(),
            reading: None,
        }
    }
}

fn lower_first(words: &str) -> String {
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}

/// The address a page is kept under: fragments name places in one page.
fn cache_key(url: &Url) -> String {
    url.without_fragment().to_string()
}

impl Browser {
    /// Drops a read of a kept copy that a newer request replaces.
    fn forget_reading(&mut self) {
        if let Some((url, _, _)) = self.reading.take() {
            self.saved.stop_reading(&cache_key(&url));
        }
    }

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
        self.clock.stop(context);
        if let Some((url, _, stepped_from)) = self.reading.take() {
            self.saved.stop_reading(&cache_key(&url));
            if let Some(position) = stepped_from {
                self.history.return_to(position);
            }
        }
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
        self.forget_reading();
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
                // A new request is a new wait.
                self.clock.stop(context);
                self.clock.start(context);
            }
            None => self.failed(context, url, Failure::Unreachable, stepped_from),
        }
    }

    fn failed(
        &mut self,
        context: &mut Context,
        url: &Url,
        failure: Failure,
        stepped_from: Option<usize>,
    ) {
        if matches!(failure, Failure::Offline | Failure::Unreachable)
            && self.saved.read(context, &cache_key(url))
        {
            // The Loading screen stays up while the copy comes off the shelf.
            self.reading = Some((url.clone(), failure, stepped_from));
            return;
        }
        self.show_failure(url, failure, stepped_from);
    }

    fn show_failure(&mut self, url: &Url, failure: Failure, stepped_from: Option<usize>) {
        if let Some(position) = stepped_from {
            // Back or Forward moved history onto a page that did not come;
            // the page still on screen is the one history should name.
            self.history.return_to(position);
        }
        self.retry = Some(url.clone());
        self.view = View::Failed(url.clone(), failure);
    }

    fn arrived(&mut self, context: &mut Context, pending: &Pending, body: &[u8]) {
        if self.display(
            context,
            pending.url.clone(),
            pending.stepped_from,
            body,
            None,
        ) {
            self.saved.keep(context, &cache_key(&pending.url), body);
        }
    }

    /// Shows a response as the page for `url`, with an optional line above
    /// it. Returns whether it was a page.
    fn display(
        &mut self,
        context: &mut Context,
        url: Url,
        stepped_from: Option<usize>,
        body: &[u8],
        note: Option<String>,
    ) -> bool {
        match fetch::sniff(body) {
            Kind::Unsupported(what) => {
                if let Some(position) = stepped_from {
                    self.history.return_to(position);
                }
                self.view = View::Unsupported(url, what);
                false
            }
            kind => {
                if stepped_from.is_none() {
                    let _ = self.history.navigate(url.clone());
                }
                // Before the page is laid out, so the pictures on its first
                // screen are measured on the page and not the Loading screen.
                self.view = View::Page;
                if kind == Kind::Plain {
                    let html = plain_page(body);
                    self.show_document_noted(context, &url, html.as_bytes(), note);
                } else {
                    self.show_document_noted(context, &url, body, note);
                }
                self.view = View::Page;
                true
            }
        }
    }

    fn stored(&mut self, context: &mut Context, from: Option<&str>, result: &StoreResult) {
        match self.saved.heard(context, from, result) {
            Heard::Elsewhere | Heard::Handled => {}
            Heard::Read {
                url: key, bytes, ..
            } if self.pictures.reads(&key) => {
                self.saved.touch(context, &key);
                if self.pictures.off_shelf(context, &key, Some(&bytes)) {
                    self.show(context);
                } else {
                    self.saved.forget_url(context, &key);
                    self.fetch_pictures(context);
                }
            }
            Heard::Unreadable { url: key } if self.pictures.reads(&key) => {
                self.pictures.off_shelf(context, &key, None);
                self.fetch_pictures(context);
            }
            heard => {
                if self.reading.is_some() {
                    self.read_back(context, heard);
                    self.show(context);
                }
            }
        }
    }

    /// A kept copy came off the shelf, or could not.
    fn read_back(&mut self, context: &mut Context, heard: Heard) {
        let Some((url, failure, stepped_from)) = self.reading.take() else {
            return;
        };
        match heard {
            Heard::Read {
                url: key,
                bytes,
                fetched,
            } if key == cache_key(&url) => {
                let why = lower_first(failure.explain().0);
                let note = match self.saved.when(fetched) {
                    Some(when) => {
                        format!("Saved copy from {when}: {why}, so it may not be the latest.")
                    }
                    None => format!("Saved copy: {why}, so it may not be the latest."),
                };
                self.saved.touch(context, &key);
                self.display(context, url, stepped_from, &bytes, Some(note));
            }
            _ => self.show_failure(&url, failure, stepped_from),
        }
    }

    fn show_document(&mut self, context: &mut Context, url: &Url, bytes: &[u8]) {
        self.show_document_noted(context, url, bytes, None);
    }

    fn show_document_noted(
        &mut self,
        context: &mut Context,
        url: &Url,
        bytes: &[u8],
        note: Option<String>,
    ) {
        for key in self.pictures.clear(context) {
            self.saved.stop_reading(&key);
        }
        if let Some(task) = self.pager.take() {
            context.cancel(task);
        }
        let document = parse_document(bytes, url, &Limits::DEFAULT);
        let mut title = document
            .title
            .clone()
            .unwrap_or_else(|| url.host().to_owned());
        if note.is_some() {
            // The note is on the first screen only; the bar is on every one,
            // so a copy reopened further in still says what it is.
            title = format!("Saved: {title}");
        }
        let (paginator, lead) = paginator_for(&document, note.as_deref());
        let (restore, place) = self
            .history
            .current()
            .map_or((0, None), |entry| (entry.page, entry.place));
        self.editing = None;
        self.form_entry.close();
        self.loaded = Some(Loaded {
            url: url.clone(),
            title,
            document,
            paginator,
            page: 0,
            lead,
            note,
            full: None,
        });
        let page = match (url.fragment(), place) {
            (_, Some(place)) if restore > 0 => {
                self.place_page(context, place + lead).unwrap_or(restore)
            }
            (Some(fragment), _) if restore == 0 => {
                self.fragment_page(context, fragment).unwrap_or(0)
            }
            // A new page opens where its own content starts, past the site's
            // menus; they are a page turn back. A saved copy still opens on
            // its first screen, which says when it was saved.
            (None, _) if restore == 0 && lead == 0 => self.main_page(context).unwrap_or(0),
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
        let mut fits = |pieces: &[Piece]| kobo_web_layout::page_fits(&title, pieces, &metrics);
        while loaded.paginator.pages().len() <= page.saturating_add(1)
            && loaded.paginator.next_page(&mut fits)
        {}
    }

    /// Carries on paginating in the background until the page count is known.
    ///
    /// Every page is a layout pass for each candidate break, and a long
    /// article is dozens of pages: doing them all before the first screen
    /// kept a reader on "Loading" for seconds after the page had arrived.
    /// So the first screens are made at once and the rest a few at a time,
    /// between naps that let a tap or a page turn in first.
    fn keep_paging(&mut self, context: &mut Context) {
        if let Some(task) = self.pager.take() {
            context.cancel(task);
        }
        if self
            .loaded
            .as_ref()
            .is_some_and(|loaded| !loaded.paginator.done())
        {
            self.pager = context.spawn(Task::Sleep { seconds: 0 });
        }
    }

    /// One background step: a few more pages, then either another nap or,
    /// once the last page exists, a repaint so the count appears.
    fn page_on(&mut self, context: &mut Context) {
        let made = self
            .loaded
            .as_ref()
            .map_or(0, |loaded| loaded.paginator.pages().len());
        self.make_pages(context, made + PAGES_PER_STEP - 2);
        if self
            .loaded
            .as_ref()
            .is_some_and(|loaded| loaded.paginator.done())
        {
            if matches!(self.view, View::Page) && !self.address.is_open() {
                self.show(context);
            }
        } else {
            self.pager = context.spawn(Task::Sleep { seconds: 0 });
        }
    }

    /// The page holding piece `place`, making pages until it exists.
    fn place_page(&mut self, context: &Context, place: usize) -> Option<usize> {
        loop {
            let loaded = self.loaded.as_ref()?;
            if let Some(page) = loaded.paginator.page_of_place(place) {
                return Some(page);
            }
            let made = loaded.paginator.pages().len();
            self.make_pages(context, made);
            if self.loaded.as_ref()?.paginator.pages().len() == made {
                return None;
            }
        }
    }

    /// The page the main content starts on, when the page marks one.
    fn main_page(&mut self, context: &Context) -> Option<usize> {
        let main = self.loaded.as_ref()?.document.main.clone()?;
        self.fragment_page(context, &main)
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

    /// Swaps between the whole page and its reader view. The reader view
    /// starts at its top; leaving it returns to where the whole page was.
    fn toggle_reader(&mut self, context: &mut Context) {
        let Some(loaded) = self.loaded.as_mut() else {
            return;
        };
        let (document, full, page) = if let Some(full) = loaded.full.take() {
            let page = self.history.current().map_or(0, |entry| entry.page);
            (full, None, page)
        } else {
            let Some(view) = loaded.document.reader_view() else {
                return;
            };
            (view, Some(std::mem::take(&mut loaded.document)), 0)
        };
        let note = if full.is_some() {
            None
        } else {
            loaded.note.as_deref()
        };
        let (paginator, lead) = paginator_for(&document, note);
        loaded.document = document;
        loaded.full = full;
        loaded.paginator = paginator;
        loaded.lead = lead;
        loaded.page = 0;
        if let Some(task) = self.pager.take() {
            context.cancel(task);
        }
        for key in self.pictures.clear(context) {
            self.saved.stop_reading(&key);
        }
        self.turn_to(context, page);
    }

    fn turn_to(&mut self, context: &mut Context, page: usize) {
        self.make_pages(context, page);
        if self.pager.is_none() {
            self.keep_paging(context);
        }
        if let Some(loaded) = self.loaded.as_mut() {
            let last = loaded.paginator.pages().len().saturating_sub(1);
            loaded.page = page.min(last);
            // History keeps the place in the whole page, which is what
            // reopens; a reader view is always entered from there.
            if loaded.full.is_none() {
                let place = loaded
                    .paginator
                    .place_of(loaded.page)
                    .map(|place| place.saturating_sub(loaded.lead));
                self.history.set_position(loaded.page, place);
            }
        }
        self.fetch_pictures(context);
    }

    /// Asks for the pictures on the screen being read. The sample pages are
    /// built in and have none to fetch.
    fn fetch_pictures(&mut self, context: &mut Context) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        if sample(&loaded.url).is_some() {
            return;
        }
        let wanted: Vec<usize> = self
            .current_pieces()
            .iter()
            .filter_map(|piece| match piece {
                Piece::Picture { image, .. } => Some(*image),
                _ => None,
            })
            .collect();
        let metrics = context.metrics();
        let rooms: BTreeMap<usize, (u32, u32)> = wanted
            .iter()
            .filter_map(|&image| Some((image, self.picture_room(&metrics, image)?)))
            .collect();
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let images = loaded.document.images();
        let saved = &mut self.saved;
        let colour = self.pictures.colour();
        self.pictures.want(
            context,
            &wanted,
            |image| images.get(image).map(|found| found.src.to_string()),
            |context, image, url| {
                let (width, height) = rooms.get(&image)?;
                let key = pictures::shelf_key(url, *width, *height, colour);
                saved.read(context, &key).then_some(key)
            },
        );
    }

    /// Where `image` is drawn on the screen being read, if it is on it.
    fn image_source(&self, image: usize) -> Option<String> {
        let loaded = self.loaded.as_ref()?;
        loaded
            .document
            .images()
            .get(image)
            .map(|found| found.src.to_string())
    }

    fn picture_room(&self, metrics: &DisplayMetrics, image: usize) -> Option<(u32, u32)> {
        let screen = self.screen(metrics).build();
        kobo_web_layout::picture_box(&screen, metrics, picture_handle(image))
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

    fn form_action(&mut self, context: &Context, action: ActionId) -> bool {
        match self.view {
            View::Page => {
                let selected = self.current_pieces().iter().find_map(|piece| {
                    if let Piece::Form { index, .. } = piece {
                        (action == action_id(&format!("form-{index}"))).then_some(*index)
                    } else {
                        None
                    }
                });
                if let Some(index) = selected {
                    self.view = View::Form(index, 0);
                    return true;
                }
            }
            View::Form(index, page) => {
                if action == action_id("return") || action == action_id("back") {
                    self.view = View::Page;
                    return true;
                }
                if action == action_id("form-next") || action == action_id("form-previous") {
                    self.view = View::Form(
                        index,
                        if action == action_id("form-next") {
                            page.saturating_add(1)
                        } else {
                            page.saturating_sub(1)
                        },
                    );
                    return true;
                }
                let selected = self.loaded.as_ref().and_then(|loaded| {
                    let pages = forms::form_pages(loaded, index, &context.metrics());
                    let allowed = pages.get(page.min(pages.len().saturating_sub(1)))?;
                    let form = forms::forms(&loaded.document.blocks).get(index).copied()?;
                    forms::field_action(form, action).filter(|field| allowed.contains(field))
                });
                if let Some(field) = selected {
                    if let Some(loaded) = self.loaded.as_mut() {
                        match forms::form_mut(&mut loaded.document.blocks, index)
                            .and_then(|form| form.fields.get_mut(field))
                        {
                            Some(Field::Text { value, .. }) => {
                                self.editing = Some((index, field));
                                self.form_entry.open_with(value.clone());
                            }
                            Some(Field::Checkbox { checked, .. }) => *checked = !*checked,
                            Some(Field::Select { .. }) => {
                                self.view = View::Options(index, field, 0);
                            }
                            _ => {}
                        }
                    }
                    return true;
                }
                if action == action_id("submit-form") {
                    // Submission is handled by the caller with a Context.
                    return false;
                }
            }
            View::Options(index, field, page) => {
                if action == action_id("return-to-form") || action == action_id("back") {
                    self.view = View::Form(index, 0);
                    return true;
                }
                if action == action_id("option-next") || action == action_id("option-previous") {
                    self.view = View::Options(
                        index,
                        field,
                        if action == action_id("option-next") {
                            page.saturating_add(1)
                        } else {
                            page.saturating_sub(1)
                        },
                    );
                    return true;
                }
                return self.choose_option(context, index, field, page, action);
            }
            _ => {}
        }
        false
    }

    fn choose_option(
        &mut self,
        context: &Context,
        index: usize,
        field: usize,
        page: usize,
        action: ActionId,
    ) -> bool {
        let allowed = self
            .loaded
            .as_ref()
            .and_then(|loaded| {
                let form = forms::forms(&loaded.document.blocks).get(index).copied()?;
                let pages = forms::option_pages(form, field, &context.metrics());
                pages.get(page.min(pages.len().saturating_sub(1))).cloned()
            })
            .unwrap_or_default();
        if let Some(Field::Select {
            chosen, options, ..
        }) = self
            .loaded
            .as_mut()
            .and_then(|loaded| forms::form_mut(&mut loaded.document.blocks, index))
            .and_then(|form| form.fields.get_mut(field))
        {
            if let Some(option) = allowed
                .into_iter()
                .find(|i| *i < options.len() && action_id(&format!("option-{i}")) == action)
            {
                *chosen = Some(option);
                self.view = View::Form(index, 0);
                return true;
            }
        }
        false
    }

    fn submit_form(&mut self, context: &mut Context, action: ActionId) -> bool {
        let View::Form(index, _) = self.view else {
            return false;
        };
        if action != action_id("submit-form") {
            return false;
        }
        let Some(loaded) = self.loaded.as_ref() else {
            return false;
        };
        let form = forms::forms(&loaded.document.blocks).get(index).copied();
        let to = form.and_then(|form| forms::get_url(form, None));
        if let Some(to) = to {
            self.open(context, &to);
        } else {
            self.view = View::FormUnavailable(
                if form.is_some_and(|f| f.method == kobo_web_document::Method::Post) {
                    "POST forms are not available yet. Nothing was sent.".to_owned()
                } else {
                    "This form cannot be sent. Nothing was sent.".to_owned()
                },
            );
        }
        self.show(context);
        true
    }

    fn select_section(&mut self, context: &mut Context, action: ActionId) {
        if action == action_id("return") {
            self.view = View::Page;
            return;
        }
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let selected = headings(loaded)
            .into_iter()
            .find(|(place, _, _)| action_id(&section_action(*place)) == action);
        if let Some((place, _, _)) = selected {
            if let Some(page) = self.place_page(context, place) {
                self.view = View::Page;
                self.turn_to(context, page);
            }
        }
    }

    fn show(&self, context: &mut Context) {
        context.set_screen(self.screen(&context.metrics()).build());
    }

    fn form_entry_screen(&self) -> ScreenBuilder {
        let label = self
            .editing
            .and_then(|(index, field)| {
                self.loaded
                    .as_ref()
                    .and_then(|loaded| forms::forms(&loaded.document.blocks).get(index).copied())
                    .and_then(|form| form.fields.get(field))
                    .and_then(|field| match field {
                        Field::Text { label, .. } => Some(label.as_str()),
                        _ => None,
                    })
            })
            .unwrap_or("Text");
        ScreenBuilder::new("browser-form-entry")
            .top_bar(label)
            .text_entry(&self.form_entry, label, "Save")
    }

    fn handle_form_entry(&mut self, context: &mut Context, action: ActionId) {
        match self.form_entry.handle(action) {
            Some(Typing::Submitted(value)) => {
                if let (Some((index, field)), Some(loaded)) =
                    (self.editing.take(), self.loaded.as_mut())
                {
                    if let Some(Field::Text { value: current, .. }) =
                        forms::form_mut(&mut loaded.document.blocks, index)
                            .and_then(|form| form.fields.get_mut(field))
                    {
                        *current = value;
                    }
                }
            }
            Some(Typing::Cancelled) => {
                self.editing = None;
            }
            Some(Typing::Changed) | None => {}
        }
        self.show(context);
    }

    fn options_screen(
        loaded: &Loaded,
        index: usize,
        field: usize,
        page: usize,
        metrics: &DisplayMetrics,
    ) -> ScreenBuilder {
        forms::forms(&loaded.document.blocks)
            .get(index)
            .map_or_else(
                || {
                    ScreenBuilder::new("browser-options")
                        .top_bar("Choose")
                        .text("Form no longer available.")
                },
                |form| forms::option_screen(form, field, page, metrics),
            )
    }

    fn handle_address(&mut self, context: &mut Context, action: ActionId) -> bool {
        match self.address.handle(action) {
            Some(Typing::Submitted(typed)) => {
                if let Some(to) = address::resolve(&typed, DEFAULT_SEARCH) {
                    self.open(context, to.url());
                } else {
                    self.show(context);
                }
                true
            }
            Some(Typing::Changed | Typing::Cancelled) => {
                self.show(context);
                true
            }
            None if self.address.is_open() => {
                if action == action_id("back") {
                    self.address.close();
                    self.show(context);
                }
                true
            }
            None => false,
        }
    }

    fn form_unavailable_screen(reason: &str) -> ScreenBuilder {
        ScreenBuilder::new("browser-form-unavailable")
            .top_bar("Form")
            .heading("Not sent")
            .text(reason)
            .button("return-to-form", "Back to the page")
    }

    fn screen(&self, metrics: &DisplayMetrics) -> ScreenBuilder {
        if self.form_entry.is_open() {
            return self.form_entry_screen();
        }
        if self.address.is_open() {
            return ScreenBuilder::new("browser-address")
                .top_bar("Go to")
                .text_entry(&self.address, "An address, or words to search for", "Go");
        }
        match (&self.view, &self.loaded) {
            (View::Loading(url), _) => ScreenBuilder::new("browser-loading")
                .top_bar(url.host())
                .activity("Loading the page", None)
                .secondary(self.clock.waited_words())
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
            (View::FormUnavailable(reason), _) => Self::form_unavailable_screen(reason),
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
            (View::Form(index, page), Some(loaded)) => {
                forms::form_screen(loaded, *index, *page, metrics)
            }
            (View::Options(index, field, page), Some(loaded)) => {
                Self::options_screen(loaded, *index, *field, *page, metrics)
            }
            (View::Links(page), Some(loaded)) => {
                let links = self.page_links();
                let pages = links_pages(loaded, &links, metrics);
                let page = (*page).min(pages.len().saturating_sub(1));
                links_screen(
                    loaded,
                    pages.get(page).map_or(&[], Vec::as_slice),
                    links.len(),
                    page,
                    pages.len(),
                )
            }
            (View::Navigate, Some(_)) => ScreenBuilder::new("browser-navigate")
                .top_bar("Navigate")
                .top_bar_action("return", "Done")
                .rows([
                    ("sections", "Sections", "Jump to a heading", Glyph::Bookmark),
                    ("links", "Links", "Links on this page", Glyph::Bookmark),
                ]),
            (View::Sections(page), Some(loaded)) => {
                let headings = headings(loaded);
                let pages = sections_pages(&headings, metrics);
                let page = (*page).min(pages.len().saturating_sub(1));
                sections_screen(
                    pages.get(page).map_or(&[], Vec::as_slice),
                    headings.len(),
                    page,
                    pages.len(),
                )
            }
            (_, Some(loaded)) => page_screen_with(
                &loaded.title,
                self.current_pieces(),
                loaded.page,
                // The count in the bar is only honest once every page exists.
                loaded
                    .paginator
                    .done()
                    .then(|| loaded.paginator.pages().len()),
                if loaded.full.is_some() {
                    Some(true)
                } else {
                    loaded.document.has_reader_view().then_some(false)
                },
            ),
            (_, None) => ScreenBuilder::new("browser-empty")
                .top_bar("Browse")
                .text("Nothing to show."),
        }
    }

    fn link_named(&self, action: ActionId) -> Option<usize> {
        let loaded = self.loaded.as_ref()?;
        let candidates: Vec<usize> = match self.view {
            View::Links(_) => self.page_links(),
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
        Saved::start(context);
        context.device().read_identity();
        if let Ok(home) = Url::parse(SAMPLES) {
            if let Ok(index) = home.join("index.html") {
                self.open(context, &index);
            }
        }
    }

    fn on_action(&mut self, context: &mut Context, action: ActionId) {
        if self.form_entry.is_open() {
            self.handle_form_entry(context, action);
            return;
        }
        if self.handle_address(context, action) {
            return;
        }
        if self.submit_form(context, action) {
            return;
        }
        if matches!(self.view, View::FormUnavailable(_)) && action == action_id("return-to-form") {
            self.view = View::Page;
            self.show(context);
            return;
        }
        if self.form_action(context, action) {
            self.show(context);
            return;
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
        } else if action == action_id("back")
            && matches!(
                self.view,
                View::Links(_) | View::Sections(_) | View::Navigate
            )
        {
            self.view = View::Page;
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
        } else if action == action_id("navigate") {
            self.view = View::Navigate;
        } else if action == action_id("links") {
            self.view = View::Links(0);
        } else if action == action_id("sections") {
            self.view = View::Sections(0);
        } else if action == action_id("reader") {
            self.toggle_reader(context);
        } else if action == action_id("links-next") || action == action_id("links-previous") {
            if let View::Links(page) = self.view {
                self.view = View::Links(if action == action_id("links-next") {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                });
            }
        } else if action == action_id("sections-next") || action == action_id("sections-previous") {
            if let View::Sections(page) = self.view {
                self.view = View::Sections(if action == action_id("sections-next") {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                });
            }
        } else if matches!(self.view, View::Sections(_)) {
            self.select_section(context, action);
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
        if self.pager == Some(task) {
            self.pager = None;
            if !matches!(outcome, TaskOutcome::Cancelled) {
                self.page_on(context);
            }
            return;
        }
        if let Some(image) = self.pictures.fetching(task) {
            match self.picture_room(&context.metrics(), image) {
                Some(room) => {
                    if let Some(packed) = self.pictures.arrived(context, image, outcome, Some(room))
                    {
                        if let Some(url) = self.image_source(image) {
                            let key =
                                pictures::shelf_key(&url, room.0, room.1, self.pictures.colour());
                            self.saved.keep(context, &key, &packed);
                        }
                        self.show(context);
                    }
                }
                // Turned away from before it came: asked for again on return.
                None => self.pictures.forget(image),
            }
            return;
        }
        if self.clock.on_task(context, task, &outcome) {
            if matches!(self.view, View::Loading(_)) && !self.address.is_open() {
                self.show(context);
            }
            return;
        }
        let Some(pending) = self.pending.take_if(|pending| pending.task == task) else {
            // An answer to a request that was cancelled or replaced.
            return;
        };
        self.clock.stop(context);
        match outcome {
            TaskOutcome::Completed(body) => self.arrived(context, &pending, &body),
            TaskOutcome::Failed(error) => {
                self.failed(context, &pending.url, failure(error), pending.stepped_from);
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

    fn on_device_result(
        &mut self,
        _context: &mut Context,
        request: kobo_sdk::DeviceRequest,
        result: kobo_sdk::DeviceResult,
    ) {
        if request == kobo_sdk::DeviceRequest::ReadIdentity {
            if let kobo_sdk::DeviceResult::Identity(identity) = result {
                self.pictures.set_colour(identity.colour_panel());
            }
        }
    }

    fn on_load(&mut self, context: &mut Context, key: &str, result: StoreResult) {
        self.stored(context, Some(key), &result);
    }

    fn on_save(&mut self, context: &mut Context, key: &str, result: StoreResult) {
        self.stored(context, Some(key), &result);
    }

    fn on_shelf(&mut self, context: &mut Context, name: &str, result: StoreResult) {
        self.stored(context, Some(name), &result);
    }

    fn on_store(&mut self, context: &mut Context, result: StoreResult) {
        self.stored(context, None, &result);
    }

    fn on_page_turn(&mut self, context: &mut Context, forward: bool) {
        if self.address.is_open() {
            return;
        }
        let name = match (&self.view, forward) {
            (View::Links(_), true) => "links-next",
            (View::Links(_), false) => "links-previous",
            (View::Form(_, _), true) => "form-next",
            (View::Form(_, _), false) => "form-previous",
            (View::Options(_, _, _), true) => "option-next",
            (View::Options(_, _, _), false) => "option-previous",
            (View::Sections(_), true) => "sections-next",
            (View::Sections(_), false) => "sections-previous",
            (_, true) => "next-page",
            (_, false) => "previous-page",
        };
        self.on_action(context, action_id(name));
    }
}

/// A heading's place in the paginator, depth, and visible title. Use the
/// paginator's own pieces rather than document block indices: lists, quotes
/// and the saved-copy note can change the positions of headings.
fn headings(loaded: &Loaded) -> Vec<(usize, u8, String)> {
    let (flat, _) = pieces(&loaded.document);
    flat.into_iter()
        .enumerate()
        .filter_map(|(place, piece)| {
            if let Piece::Heading { level, text, .. } = piece {
                Some((place + loaded.lead, level, text))
            } else {
                None
            }
        })
        .collect()
}

fn section_action(place: usize) -> String {
    format!("section-{place}")
}

fn sections_screen(
    entries: &[(usize, u8, String)],
    total: usize,
    page: usize,
    of: usize,
) -> ScreenBuilder {
    let builder = ScreenBuilder::new("browser-sections")
        .top_bar("Sections")
        .top_bar_action("return", "Done")
        .secondary(format!("{total} sections in this view."))
        .page_turns("sections-previous", "sections-next")
        .page_position(
            u16::try_from(page + 1).unwrap_or(u16::MAX),
            u16::try_from(of.max(1)).unwrap_or(u16::MAX),
        );
    if total == 0 {
        return builder.text("This page has no headings.");
    }
    builder.rows(entries.iter().map(|(place, level, title)| {
        (
            section_action(*place),
            title.clone(),
            format!("Level {level}"),
            Glyph::Bookmark,
        )
    }))
}

/// Only the links on the screen being read, in reading order.
fn links_screen(
    loaded: &Loaded,
    entries: &[usize],
    total: usize,
    page: usize,
    of: usize,
) -> ScreenBuilder {
    let builder = ScreenBuilder::new("browser-links")
        .top_bar("Links")
        .top_bar_action("return", "Done")
        .secondary(format!("{total} links on this page."))
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

fn fit_pages<T: Clone>(
    entries: &[T],
    metrics: &DisplayMetrics,
    screen: impl Fn(&[T]) -> ScreenBuilder,
) -> Vec<Vec<T>> {
    let mut pages = Vec::new();
    let mut rest = entries;
    while !rest.is_empty() {
        let fits = |count: usize| {
            count <= kobo_sdk::MAX_ROWS
                && kobo_web_layout::fits(&screen(&rest[..count]).build(), metrics)
        };
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

fn links_pages(loaded: &Loaded, here: &[usize], metrics: &DisplayMetrics) -> Vec<Vec<usize>> {
    fit_pages(here, metrics, |entries| {
        links_screen(loaded, entries, here.len(), 998, 999)
    })
}

fn sections_pages(
    headings: &[(usize, u8, String)],
    metrics: &DisplayMetrics,
) -> Vec<Vec<(usize, u8, String)>> {
    fit_pages(headings, metrics, |entries| {
        sections_screen(entries, headings.len(), 998, 999)
    })
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

#[cfg(test)]
mod corpus_tests;
#[cfg(all(test, debug_assertions))]
mod fixture_tests;
#[cfg(test)]
mod image_tests;

/// Pages `document`, with the saved-copy note first when there is one.
/// Returns the paginator and how many pieces come before the document's own.
fn paginator_for(document: &Document, note: Option<&str>) -> (Paginator, usize) {
    let (mut pieces, mut anchors) = pieces(document);
    let lead = usize::from(note.is_some());
    if let Some(note) = note {
        pieces.insert(0, Piece::Note(note.to_owned()));
        for (_, at) in &mut anchors {
            *at += 1;
        }
    }
    (Paginator::new(pieces, anchors), lead)
}
