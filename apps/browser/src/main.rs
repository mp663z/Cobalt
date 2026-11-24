//! Browse: a reader's web browser for Cobalt.
//!
//! Pages are turned, not scrolled. This first build reads only the sample
//! pages it ships with; loading from the network comes next.

use std::collections::BTreeMap;
use std::process::ExitCode;

use kobo_browser_core::address::{self, DEFAULT_SEARCH};
use kobo_browser_core::fetch::{self, Failure, Kind};
use kobo_browser_core::library::{Library, Place};
use kobo_browser_core::{Go, History};
use kobo_sdk::keyboard::{TextEntry, Typing};
use kobo_sdk::{
    action_id, ActionId, Context, DisplayMetrics, Glyph, Header, Heartbeat, KoboApp, ScreenBuilder,
    StoreResult, Task, TaskError, TaskId, TaskOutcome,
};
use kobo_web_document::{Document, Field, Limits, Url};
use kobo_web_layout::{
    link_action, page_screen_with_address, picture_handle, pieces, Paginator, Piece,
};

#[cfg(test)]
mod arrival_observation;
mod arrival_prep;
#[cfg(test)]
mod arrival_prep_tests;
#[cfg(test)]
mod arrival_timing;
mod browser_chrome;
mod bundled_pair_adapter;
mod css_stage;
mod forms;
mod native_font_adapter;
mod page_mode;
#[allow(dead_code)] // Page view compiles ahead of the view switch that installs it.
mod page_view;
mod page_images;
mod pictures;
#[cfg(test)]
mod s0_native;
mod saved;
mod sheet_supply;
use css_stage::CssStage;
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
    ("images.html", include_str!("../pages/images.html")),
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
    page_source: Option<Vec<u8>>,
    painted: Option<page_mode::PageMode>,
    paint_refused: bool,
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
    Start,
    /// The Links list, at this screen of it.
    Links(usize),
    /// The heading list, at this screen of it.
    Sections(usize),
    /// The navigation choices where the reader switch uses a bar slot.
    Navigate,
    Bookmarks(usize),
    Recent(usize),
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
    /// POST confirmation: still does not send until a supported runtime path exists.
    PostConfirm(usize),
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
    library: Library,
    library_loaded: bool,
    browser_menu: bool,
    zoom_panel:bool,
    home_page: Option<Url>,
    zoom: u16,
    scroll_places:std::collections::BTreeMap<String,(u16,u32)>,
    loaded: Option<Loaded>,
    view: View,
    /// The address field. Its inline keyboard leaves the page visible.
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
    /// CSS page staging: eligibility and frame ownership for a styled
    /// presentation that production keeps disabled. See [`css_stage`].
    css: CssStage,
    sheets: sheet_supply::SheetSupply,
    /// TEST-ONLY (116): drop-order log for producer-mode frames.
    #[cfg(test)]
    frame_drops: std::rc::Rc<std::cell::RefCell<Vec<u64>>>,
    /// TEST-ONLY: the most recent arrival observation, plus how many
    /// arrivals have been observed this session. Bounded on purpose --
    /// one record and one counter, no history retention -- so the
    /// experiment's own memory cost is fixed. See [`arrival_observation`].
    #[cfg(test)]
    last_observation: Option<arrival_observation::ArrivalObservation>,
    #[cfg(test)]
    observation_count: u64,
    /// TEST-ONLY: the display path's classification of the response the
    /// next arrival carries (wrapper identity is caller knowledge, not
    /// byte knowledge).
    #[cfg(test)]
    observation_input: Option<arrival_observation::ObservationInput>,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            history: History::new(),
            library: Library::default(),
            library_loaded: false,
            browser_menu: false,
            zoom_panel:false,
            home_page: None,
            zoom: 100,
            scroll_places:Default::default(),
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
            css: CssStage::production(),
            sheets: sheet_supply::SheetSupply::default(),
            #[cfg(test)]
            frame_drops: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
            #[cfg(test)]
            last_observation: None,
            #[cfg(test)]
            observation_count: 0,
            #[cfg(test)]
            observation_input: None,
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
        self.zoom_panel=false;
        self.browser_menu=false;
        if self.pending.is_some()||self.reading.is_some(){self.stop(context);}
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
                // Fragment offsets belong to the native paginator, not painted screens.
                if let Some(loaded) = self.loaded.as_mut() {
                    loaded.painted = None;
                }
                context.drop_picture(page_mode::HANDLE);
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
        // Navigation retires the frame when it starts, not when the next
        // page lands: until then the old page's frame matches nothing.
        self.css.clear();
        self.sheets.clear(context);
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
        self.css.clear();
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
                    #[cfg(test)]
                    {
                        self.observation_input =
                            Some(arrival_observation::ObservationInput::PlainWrapped {
                                origin_len: body.len(),
                                origin_utf8_valid: std::str::from_utf8(body).is_ok(),
                                wrapped_len: html.len(),
                            });
                    }
                    self.show_document_noted(context, &url, html.as_bytes(), note);
                } else {
                    #[cfg(test)]
                    {
                        self.observation_input =
                            Some(arrival_observation::ObservationInput::Markup { len: body.len() });
                    }
                    self.show_document_noted(context, &url, body, note);
                }
                self.view = View::Page;
                true
            }
        }
    }

    fn stored(&mut self, context: &mut Context, from: Option<&str>, result: &StoreResult) {
        self.validate_css(context);
        if let StoreResult::Loaded { key, value } = result {
            if key == "browser-home" {
                if self.home_page.is_none() {
                    self.home_page = value
                        .as_deref()
                        .and_then(|v| std::str::from_utf8(v).ok())
                        .and_then(|v| Url::parse(v).ok());
                }
                return;
            }
            if key == "browser-library" && !self.library_loaded {
                let mut library = value.as_deref().map(Library::decode).unwrap_or_default();
                // The sample can arrive before the startup store load. Keep
                // visits made in this process ahead of older stored visits.
                let pending_changes =
                    !self.library.recent.is_empty() || !self.library.bookmarks.is_empty();
                for place in self.library.bookmarks.iter().rev() {
                    if !library.contains(&place.url) {
                        library.toggle(place.url.clone(), &place.title);
                    }
                }
                for place in self.library.recent.iter().rev() {
                    library.visit(place.url.clone(), &place.title);
                }
                self.library = library;
                self.library_loaded = true;
                if pending_changes {
                    self.save_library(context);
                }
                self.show(context);
                return;
            }
        }
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
        #[cfg(test)]
        {
            self.observation_input =
                Some(arrival_observation::ObservationInput::Markup { len: bytes.len() });
        }
        self.show_document_noted(context, url, bytes, None);
    }

    fn show_document_noted(
        &mut self,
        context: &mut Context,
        url: &Url,
        bytes: &[u8],
        note: Option<String>,
    ) {
        // Each arriving native document starts at actual size. Painted zoom is
        // an explicit page operation, never a label carried onto native prose.
        self.zoom = 100;
        for key in self.pictures.clear(context) {
            self.saved.stop_reading(&key);
        }
        if let Some(task) = self.pager.take() {
            context.cancel(task);
        }
        context.drop_picture(page_mode::HANDLE);
        // A new document replaces any staged frame before it is parsed.
        self.css.clear();
        self.sheets.clear(context);
        // The arrival's parse. Enabled stage only: it owns a copy (<= the input ceiling) that the parse and
        // preparation both read. Disabled keeps the exact existing call and allocates nothing.
        #[cfg(not(test))]
        let parsed = css_stage::ParsedArrival::parse(bytes, url, self.css.is_enabled(), self.pictures.colour());
        #[cfg(test)]
        let mut parsed = {
            let input = self
                .observation_input
                .take()
                .unwrap_or(arrival_observation::ObservationInput::Markup { len: bytes.len() });
            css_stage::ParsedArrival::parse(
                bytes,
                url,
                self.css.is_enabled(),
                input,
                self.loaded.is_some(),
                self.pictures.colour(),
            )
        };
        #[cfg(test)]
        {
            self.last_observation = Some(parsed.take_observation());
            self.observation_count += 1;
        }
        let document = parsed.document();
        let mut title = document
            .title
            .clone()
            .unwrap_or_else(|| url.host().to_owned());
        if note.is_some() {
            // The note is on the first screen only; the bar is on every one,
            // so a copy reopened further in still says what it is.
            title = format!("Saved: {title}");
        }
        let (paginator, lead) = paginator_for(document, note.as_deref());
        // Admission consumes the arrival's own parse (document + the source it was parsed from) and hands the
        // document back, so the retained source is dropped here, before the reader is built: production records
        // eligibility and stops, never publishing.
        let metrics = context.metrics();
        let colour = self.pictures.colour();
        #[cfg(not(test))]
        let document = self.css.admit_parsed(parsed, &metrics, colour);
        // TEST-ONLY (116 v3): the SAME admission through the ONE private
        // local producer entry. The non-escaping closure borrows the SAME
        // arriving `bytes` only as the no-source fallback; with a source the
        // preparer reads the Admission's captured buffer and the parse's
        // consumed record decides the binding.
        #[cfg(test)]
        let document = {
            let drops = std::rc::Rc::clone(&self.frame_drops);
            let produce = move |admission: &css_stage::Admission| {
                arrival_prep::prepare_real_frame_consumed(
                    bytes,
                    admission,
                    admission.consumed(),
                    &drops,
                )
            };
            self.css
                .admit_test_parsed(parsed, &metrics, colour, produce)
        };
        let (restore, place) = self
            .history
            .current()
            .map_or((0, None), |entry| (entry.page, entry.place));
        let mut paginator = paginator;
        if let Some(place) = place.filter(|_| restore > 0 && note.is_some()) {
            paginator.break_before_place(place + lead);
        }
        self.editing = None;
        self.form_entry.close();
        self.loaded = Some(Loaded {
            page_source: page_mode::retain_source(bytes),
            painted: None,
            paint_refused: false,
            url: url.clone(),
            title,
            document,
            paginator,
            page: 0,
            lead,
            note,
            full: None,
        });
        // Linked sheets are fetched only when the CSS stage is enabled AND admission left a candidate (a refused
        // page never costs a fetch); production is never enabled, so it starts no extra task. The result is held for the future pipeline and changes nothing on screen.
        if self.css.is_enabled() && self.css.candidate_held() {
            let media = u32::try_from(metrics.width)
                .ok()
                .and_then(kobo_web_document::MediaContext::new);
            let census =
                kobo_web_document::host_stylesheet_links(bytes, url, &Limits::DEFAULT, media);
            self.sheets.begin(context, &census);
        }
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
        // Colour panels should not silently discard authored text/image RGB.
        // Keep native forms, fragment/history positions and saved-copy notices
        // until those semantics have a matching painted presentation.
        if self.pictures.colour()
            && restore == 0
            && url.fragment().is_none()
            && self
                .loaded
                .as_ref()
                .is_some_and(|loaded| loaded.note.is_none())
        {
            if let Some(loaded) = self.loaded.as_mut() {
                if forms::forms(&loaded.document.blocks).is_empty() {
                    loaded.painted = loaded.page_source.as_deref().and_then(|source| {
                        page_mode::PageMode::build(source, &loaded.title, &metrics)
                    });
                    loaded.paint_refused = loaded.painted.is_none();
                }
            }
        }
        if let Some((zoom,top))=self.scroll_places.get(&url.to_string()).copied(){
            if url.fragment().is_none(){if let Some(loaded)=self.loaded.as_mut(){if loaded.note.is_none() && forms::forms(&loaded.document.blocks).is_empty(){
                loaded.painted=loaded.page_source.as_deref().and_then(|bytes|page_mode::PageMode::build_zoom(bytes,&loaded.title,&metrics,zoom));
                if let Some(painted)=loaded.painted.as_mut(){painted.restore_scroll(top);self.zoom=zoom;}
            }}}
        }
        if let Some(loaded) = &self.loaded {
            let title = loaded
                .document
                .title
                .as_deref()
                .unwrap_or_else(|| loaded.url.host());
            self.library.visit(loaded.url.without_fragment(), title);
            self.save_library(context);
        }
    }

    fn save_library(&self, context: &mut Context) {
        if self.library_loaded {
            context
                .store()
                .save("browser-library", self.library.encode());
        }
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
            let builder = page_screen_with_address(
                &title,
                pieces,
                998,
                Some(999),
                Some(false),
                &metrics,
                Some(&loaded.url.to_string()),
            );
            let screen=browser_chrome::toolbar(builder,self.zoom,false).build();
            browser_chrome::page_fits(&screen,pieces,&metrics,|piece|browser_chrome::toolbar(page_screen_with_address(
                &title,std::slice::from_ref(piece),998,Some(999),Some(false),&metrics,Some(&loaded.url.to_string())
            ),self.zoom,false).build())
        };
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
        loaded.painted = None;
        context.drop_picture(page_mode::HANDLE);
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
        // The reader swap changes what the page is; any staged frame dies.
        self.css.clear();
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
        if let Some(painted) = self
            .loaded
            .as_mut()
            .and_then(|loaded| loaded.painted.as_mut())
        {
            painted.turn_to(page);
            return;
        }
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
    /// Rebuild native text colour without changing edited controls or the
    /// semantic place. A changed semantic parse is refused, never guessed.
    fn refresh_native_colour(&mut self, context: &mut Context) {
        let Some(loaded)=self.loaded.as_ref() else {return};
        if loaded.painted.is_some(){return;}
        let Some(source)=loaded.page_source.as_deref() else {return};
        let colour=self.pictures.colour();
        let mut document=if colour {
            kobo_web_document::parse_coloured_document(source,&loaded.url,&Limits::DEFAULT)
        } else {kobo_web_document::parse_document(source,&loaded.url,&Limits::DEFAULT)};
        let original=loaded.full.as_ref().unwrap_or(&loaded.document);
        if document.visible_text()!=original.visible_text() || document.links!=original.links
            || document.images()!=original.images() {return;}
        let old_forms:Vec<_>=forms::forms(&original.blocks).into_iter().cloned().collect();
        let new_forms=forms::forms(&document.blocks);
        if old_forms.len()!=new_forms.len() || old_forms.iter().zip(&new_forms)
            .any(|(old,new)|old.action!=new.action||old.method!=new.method||old.fields.len()!=new.fields.len()) {return;}
        for (index,form) in old_forms.into_iter().enumerate() {
            let Some(target)=forms::form_mut(&mut document.blocks,index) else {return};
            let colours=std::mem::take(&mut target.field_colours);
            *target=form;
            target.field_colours=colours;
        }
        let reader=loaded.full.is_some();
        let (document,full)=if reader {
            let Some(view)=document.reader_view() else {return};
            (view,Some(document))
        } else {(document,None)};
        let place=loaded.paginator.place_of(loaded.page).unwrap_or(0);
        let (paginator,lead)=paginator_for(&document,if reader {None}else{loaded.note.as_deref()});
        if let Some(task)=self.pager.take(){context.cancel(task);}
        let loaded=self.loaded.as_mut().unwrap();
        loaded.document=document;loaded.full=full;loaded.paginator=paginator;
        loaded.lead=lead;loaded.page=0;
        let page=self.place_page(context,place).unwrap_or(0);
        self.turn_to(context,page);
    }

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
        if let Some(loaded) = &self.loaded {
            if let Some(painted) = &loaded.painted {
                return painted
                    .visible_links()
                    .into_iter()
                    .filter_map(|index| {
                        let target = loaded.url.join(&painted.view.links()[index]).ok()?;
                        loaded
                            .document
                            .links
                            .iter()
                            .position(|link| link.target == target)
                    })
                    .collect();
            }
        }
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
        let View::Form(index, page) = self.view else {
            return false;
        };
        let Some(loaded) = self.loaded.as_ref() else {
            return false;
        };
        let form = forms::forms(&loaded.document.blocks).get(index).copied();
        let selected = form.and_then(|form| {
            let pages = forms::form_pages(loaded, index, &context.metrics());
            pages
                .get(page.min(pages.len().saturating_sub(1)))?
                .iter()
                .copied()
                .find(|&field| {
                    matches!(form.fields.get(field), Some(Field::Submit { .. }))
                        && action == action_id(&format!("submit-field-{field}"))
                })
        });
        if action != action_id("submit-form") && selected.is_none() {
            return false;
        }
        if form.is_some_and(|f| f.method == kobo_web_document::Method::Post && f.urlencoded) {
            self.view = View::PostConfirm(index);
            self.show(context);
            return true;
        }
        let to = form.and_then(|form| {
            forms::get_url(form, selected.or_else(|| forms::default_submit(form)))
        });
        if let Some(to) = to {
            self.open(context, &to);
        } else {
            self.view =
                View::FormUnavailable("This form cannot be sent. Nothing was sent.".to_owned());
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

    fn page_mode_action(&mut self, context: &mut Context, action: ActionId) -> bool {
        if action == action_id("page-view") {
            if let Some(loaded)=self.loaded.as_ref(){self.scroll_places.remove(&loaded.url.to_string());}
            if self
                .loaded
                .as_ref()
                .is_some_and(|loaded| loaded.full.is_some())
            {
                self.toggle_reader(context);
            }
            if let Some(loaded) = self.loaded.as_mut() {
                // Forms need the native controls until the protocol supports painted hit regions.
                let forms = !forms::forms(&loaded.document.blocks).is_empty();
                loaded.painted = (!forms)
                    .then(|| {
                        loaded.page_source.as_deref().and_then(|bytes| {
                            page_mode::PageMode::build_zoom(
                                bytes,
                                &loaded.title,
                                &context.metrics(),
                                self.zoom,
                            )
                        })
                    })
                    .flatten();
                loaded.paint_refused = loaded.painted.is_none();
            }
        } else if action == action_id("native-reader") {
            if let Some(loaded)=self.loaded.as_ref(){self.scroll_places.remove(&loaded.url.to_string());}
            if let Some(loaded) = self.loaded.as_mut() {
                loaded.painted = None;
            }
            context.drop_picture(page_mode::HANDLE);
        } else {
            return false;
        }
        self.view = View::Page;
        true
    }

    fn show(&self, context: &mut Context) {
        if matches!(self.view, View::Page) {
            if let Some(loaded) = &self.loaded {
                if let Some(painted) = &loaded.painted {
                    if !painted.publish(
                        context,
                        self.pictures.colour(),
                        self.address.is_open().then(|| self.address.keyboard()),
                    ) {
                        context.set_screen(
                            page_screen_with_address(
                                &loaded.title,
                                self.current_pieces(),
                                loaded.page,
                                None,
                                Some(false),
                                &context.metrics(),
                                Some(&loaded.url.to_string()),
                            )
                            .build(),
                        );
                        return;
                    }
                }
            }
        }
        context.set_screen(self.screen(&context.metrics()).build());
    }

    /// Size, rotation, scale or panel changes arrive without a callback:
    /// the first event after one retires a frame measured for before it.
    fn validate_css(&mut self, context: &Context) {
        let metrics = context.metrics();
        let colour = self.pictures.colour();
        self.css.validate(&metrics, colour);
        if let Some(loaded) = self.loaded.as_mut() {
            if loaded
                .painted
                .as_ref()
                .is_some_and(|painted| painted.metrics != metrics)
            {
                loaded.painted = loaded
                    .page_source
                    .as_deref()
                    .and_then(|bytes| page_mode::PageMode::build(bytes, &loaded.title, &metrics));
                loaded.paint_refused = loaded.painted.is_none();
            }
        }
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

    fn browser_toolbar_action(&mut self, context: &mut Context, action: ActionId) -> bool {
        let slider = action
            .0
            .checked_sub(browser_chrome::SLIDER_BASE.0)
            .filter(|value| *value <= 100);
        if action == action_id("browser-zoom") {
            self.zoom_panel=!self.zoom_panel;self.browser_menu=false;
        } else if action==action_id("zoom-done") || (self.zoom_panel && (action==action_id("back")||action==ActionId::BACK)){
            self.zoom_panel=false;
        } else if action == action_id("browser-more") {
            self.zoom_panel=false;
            self.browser_menu = !self.browser_menu;
        } else if self.browser_menu && (action == action_id("back")||action==ActionId::BACK) {
            self.browser_menu = false;
        } else if action == action_id("go-home") {
            if self.pending.is_some()||self.reading.is_some(){self.stop(context);}
            self.zoom_panel=false;
            self.browser_menu = false;
            if let Some(url) = self.home_page.clone() {
                self.open(context, &url);
            } else {
                self.view = View::Start;
                self.show(context);
            }
            return true;
        } else if action == action_id("set-home") {
            if let Some(loaded) = &self.loaded {
                self.home_page = Some(loaded.url.clone());
                context
                    .store()
                    .save("browser-home", loaded.url.to_string().into_bytes());
            }
            self.browser_menu = false;
        } else if action == action_id("add-favourite") {
            if let Some(loaded) = &self.loaded {
                let url = loaded.url.without_fragment();
                if !self.library.contains(&url) {
                    self.library.toggle(url, &loaded.title);
                    self.save_library(context);
                }
            }
            self.browser_menu = false;
        } else if action == action_id("reload") {
            if let View::Failed(url,_)|View::Unsupported(url,_)=&self.view{
                let url=url.clone();self.fetch(context,&url,None);self.show(context);return true;
            }
            if let Some(loaded)=self.loaded.as_ref(){self.scroll_places.remove(&loaded.url.to_string());}
            // The old raster is retired before the new document arrives.
            // Reset the label before publishing that intermediate native frame.
            self.zoom = 100;
            if let Some(url) = self.loaded.as_ref().map(|loaded| loaded.url.clone()) {
                // Stop referencing the old raster before retiring its cache entry.
                if let Some(loaded) = self.loaded.as_mut() {
                    loaded.painted = None;
                }
                self.show(context);
                self.load(context, &url);
            }
            return true;
        } else if slider.is_some()
            || action == action_id("zoom-less")
            || action == action_id("zoom-more")
            || action == action_id("zoom-reset")
        {
            // A new zoom rebuild starts at its first viewport. Retire the
            // old session offset rather than resurrecting it on Back.
            if let Some(loaded)=self.loaded.as_ref(){self.scroll_places.remove(&loaded.url.to_string());}
            self.zoom = if action==action_id("zoom-reset"){100} else if let Some(value) = slider {
                75 + (value * 125 / 100) as u16
            } else if action == action_id("zoom-less") {
                self.zoom.saturating_sub(25).max(75)
            } else {
                (self.zoom + 25).min(200)
            };
            if let Some(loaded) = self.loaded.as_mut() {
                loaded.painted = loaded
                    .page_source
                    .as_deref()
                    .filter(|_| forms::forms(&loaded.document.blocks).is_empty())
                    .and_then(|bytes| {
                        page_mode::PageMode::build_zoom(
                            bytes,
                            &loaded.title,
                            &context.metrics(),
                            self.zoom,
                        )
                    });
                loaded.paint_refused = loaded.painted.is_none();
                if loaded.painted.is_none() {
                    self.zoom = 100;
                }
            }
        } else {
            if action == action_id("bookmarks") {
                self.browser_menu = false;
            }
            return false;
        }
        self.show(context);
        true
    }

    fn handle_address(&mut self, context: &mut Context, action: ActionId) -> bool {
        if action == action_id("edit-address") && !self.address.is_open() {
            self.browser_menu = false;
            self.zoom_panel=false;
            let value=if matches!(self.view,View::Start){String::new()}else{self.displayed_address()};
            self.address.open_with(value);
            self.show(context);
            return true;
        }
        if action == action_id("address") {
            self.browser_menu = false;
        }
        if self.address.is_open() {
            for (name, value) in [
                ("url-slash", "/"),
                ("url-dot", "."),
                ("url-com", ".com"),
                ("url-colon", ":"),
            ] {
                if action == action_id(name) {
                    self.address.insert(value);
                    self.show(context);
                    return true;
                }
            }
        }
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
                if action == action_id("back") || action==ActionId::BACK {
                    self.address.close();
                    self.show(context);
                }
                true
            }
            None => false,
        }
    }

    fn post_screen(form: Option<&kobo_web_document::Form>) -> ScreenBuilder {
        let Some(form) = form else {
            return ScreenBuilder::new("browser-post")
                .top_bar("POST form")
                .text("Form no longer available.");
        };
        ScreenBuilder::new("browser-post")
            .top_bar("POST form")
            .heading("Review submission")
            .text(format!("Send form data to {}?", form.action.origin()))
            .secondary("POST may change data on this site. This browser cannot send POST yet.")
            .button("cancel-post", "Cancel")
    }

    fn form_unavailable_screen(reason: &str) -> ScreenBuilder {
        ScreenBuilder::new("browser-form-unavailable")
            .top_bar("Form")
            .heading("Not sent")
            .text(reason)
            .button("return-to-form", "Back to the page")
    }

    fn navigate_screen(&self) -> ScreenBuilder {
        let marked = self
            .loaded
            .as_ref()
            .is_some_and(|loaded| self.library.contains(&loaded.url.without_fragment()));
        let heading = if self
            .loaded
            .as_ref()
            .is_some_and(|loaded| loaded.paint_refused)
        {
            "Reader kept: page paint was incomplete"
        } else {
            "Navigate"
        };
        ScreenBuilder::new("browser-navigate")
            .top_bar(heading)
            .top_bar_action("return", "Done")
            .top_bar_action("native-reader", "Reader")
            .rows([
                (
                    "page-view",
                    "Page view",
                    "Paint this page, or keep Reader if unsupported",
                    Glyph::Note,
                ),
                ("sections", "Sections", "Jump to a heading", Glyph::Bookmark),
                ("links", "Links", "Links on this page", Glyph::Bookmark),
                (
                    "mark",
                    if marked {
                        "Remove bookmark"
                    } else {
                        "Bookmark this page"
                    },
                    "Save or remove this address",
                    Glyph::Bookmark,
                ),
                (
                    "bookmarks",
                    "Bookmarks",
                    "Pages you marked",
                    Glyph::Bookmark,
                ),
                (
                    "recent",
                    "History",
                    "Recently visited pages",
                    Glyph::Bookmark,
                ),
            ])
    }

    fn handle_form_return(&mut self, context: &mut Context, action: ActionId) -> bool {
        if let View::PostConfirm(index) = self.view {
            if action == action_id("cancel-post") || action == action_id("back") {
                self.view = View::Form(index, 0);
                self.show(context);
                return true;
            }
        }
        if matches!(self.view, View::FormUnavailable(_)) && action == action_id("return-to-form") {
            self.view = View::Page;
            self.show(context);
            return true;
        }
        false
    }

    fn library_action(&mut self, context: &mut Context, action: ActionId) -> bool {
        if action == action_id("back") || action == action_id("return") {
            return false;
        }
        if action == action_id("mark") && matches!(self.view, View::Navigate) {
            if let Some(loaded) = &self.loaded {
                let title = loaded
                    .document
                    .title
                    .as_deref()
                    .unwrap_or_else(|| loaded.url.host());
                self.library.toggle(loaded.url.without_fragment(), title);
                self.save_library(context);
            }
        } else if action == action_id("bookmarks") {
            if self.pending.is_some()||self.reading.is_some(){self.stop(context);}
            self.view = View::Bookmarks(0);
        } else if action == action_id("recent") {
            if self.pending.is_some()||self.reading.is_some(){self.stop(context);}
            self.view = View::Recent(0);
        } else if action == action_id("places-next") || action == action_id("places-previous") {
            let next = action == action_id("places-next");
            self.view = match self.view {
                View::Bookmarks(page) => View::Bookmarks(if next {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                }),
                View::Recent(page) => View::Recent(if next {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                }),
                _ => self.view.clone(),
            };
        } else if matches!(self.view, View::Bookmarks(_) | View::Recent(_)) {
            let recent = matches!(self.view, View::Recent(_));
            let places = if recent {
                &self.library.recent
            } else {
                &self.library.bookmarks
            };
            let pages = fit_pages(places, &context.metrics(), |entries| {
                place_screen(entries, places.len(), 998, 999, recent)
            });
            let page = match self.view {
                View::Bookmarks(page) | View::Recent(page) => page,
                _ => 0,
            }
            .min(pages.len().saturating_sub(1));
            let first = pages.iter().take(page).map(Vec::len).sum::<usize>();
            let selected = pages
                .get(page)
                .and_then(|rows| {
                    rows.iter()
                        .enumerate()
                        .find(|(i, _)| action == action_id(&format!("place-{i}")))
                })
                .map(|(i, _)| first + i);
            if let Some(url) = selected
                .and_then(|index| places.get(index))
                .map(|place| place.url.clone())
            {
                self.open(context, &url);
                return true;
            }
        } else {
            return false;
        }
        self.show(context);
        true
    }

    fn library_screen(&self, recent: bool, page: usize, metrics: &DisplayMetrics) -> ScreenBuilder {
        let places = if recent {
            &self.library.recent
        } else {
            &self.library.bookmarks
        };
        let pages = fit_pages(places, metrics, |entries| {
            place_screen(entries, places.len(), 998, 999, recent)
        });
        let page = page.min(pages.len().saturating_sub(1));
        place_screen(
            pages.get(page).map_or(&[], Vec::as_slice),
            places.len(),
            page,
            pages.len(),
            recent,
        )
    }

    fn start_screen() -> ScreenBuilder {
        browser_chrome::toolbar(
            ScreenBuilder::new("browser-start")
            .top_bar("Browse")
            .top_bar_glyph("browser-close", "Close browser", Glyph::Close)
            .field("address", "", "Search or enter address")
            .heading("Search the web")
            .secondary("Your bookmarks and recent pages, one tap away.")
            .divider()
            .rows([
                ("bookmarks", "Bookmarks", "Saved for later", Glyph::Bookmark),
                ("recent", "History", "Pick up where you left off", Glyph::Clock),
            ])
            .fill()
            .divider()
            .rows([("samples", "Sample pages", "Explore offline", Glyph::Note)]),
            100,
            false,
        )
    }

    fn screen(&self, metrics: &DisplayMetrics) -> ScreenBuilder {
        let owns=self.address.is_open()||self.form_entry.is_open()||self.zoom_panel||self.browser_menu
            || (!matches!(self.view,View::Start|View::Page)) || self.history.position()>0;
        self.browser_screen(metrics).owns_back(owns)
    }

    fn browser_screen(&self, metrics: &DisplayMetrics) -> ScreenBuilder {
        let mut builder = self.browser_content(metrics);
        if self.address.is_open() {
            builder = builder.top_bar(
                self.loaded
                    .as_ref()
                    .filter(|_| !matches!(self.view, View::Start))
                    .map_or("Browse", |loaded| loaded.title.as_str()),
            );
            browser_chrome::address_keys(
                browser_chrome::toolbar(builder, self.zoom, true).top_bar_glyph(
                    "kb.cancel",
                    "Close keyboard",
                    Glyph::Close,
                ),
            )
            .keyboard(self.address.keyboard(), "Go")
        } else if matches!(self.view, View::Page|View::Loading(_)|View::Failed(..)|View::Unsupported(..)) {
            let builder = builder
                .top_bar(self.loaded.as_ref().map_or_else(
                    || "Browse".to_owned(),
                    |loaded| {
                        loaded.painted.as_ref().map_or_else(
                            || loaded.title.clone(),
                            |painted| {
                                format!(
                                    "{}/{} {}",
                                    painted.page + 1,
                                    painted.view.pages(),
                                    loaded.title
                                )
                            },
                        )
                    },
                ))
                .top_bar_glyph("browser-close", "Close browser", Glyph::Close)
                ;
            let builder = if matches!(self.view,View::Loading(_)){builder.loading_browser_bar()}else if matches!(self.view,View::Failed(..)|View::Unsupported(..)){builder.recovery_browser_bar()}else{browser_chrome::toolbar(builder, self.zoom, false)};
            if self.zoom_panel {
                builder.popover("browser-zoom",|panel|panel
                    .section(format!("Zoom {}%",self.zoom))
                    .slider(browser_chrome::SLIDER_BASE,((self.zoom.saturating_sub(75))*100/125).min(100) as u8)
                    .grid(3,false,[("zoom-less","-"),("zoom-reset","100%"),("zoom-more","+")])
                    .button("zoom-done","Done"))
            } else if self.browser_menu {
                builder.popover("browser-more", |menu| {
                    menu.rows([
                        ("navigate", "Navigate", "Links, sections and page modes", Glyph::Note),
                        ("edit-address", "Keyboard", "", Glyph::Keyboard),
                        ("go-home", "Go Home", "", Glyph::Home),
                        ("bookmarks", "Favourites", "", Glyph::Star),
                        ("add-favourite", "Add to Favourites", "", Glyph::Star),
                        ("set-home", "Set Page as Home", "", Glyph::Home),
                    ])
                })
            } else {
                builder
            }
        } else {
            builder
        }
    }

    fn keyboard_pieces(&self, metrics: &DisplayMetrics) -> Vec<Piece> {
        let pieces = self.current_pieces();
        if !self.address.is_open() {
            return pieces.to_vec();
        }
        let Some(loaded) = &self.loaded else {
            return Vec::new();
        };
        let mut paginator = Paginator::new(pieces.to_vec(), Vec::new());
        paginator.next_page(&mut |candidate| {
            let builder = page_screen_with_address(
                &loaded.title,
                candidate,
                loaded.page,
                loaded.paginator.done().then(|| loaded.paginator.pages().len()),
                if loaded.full.is_some() {
                    Some(true)
                } else {
                    loaded.document.has_reader_view().then_some(false)
                },
                metrics,
                Some(&self.displayed_address()),
            )
            .top_bar("Browse")
            .top_bar_glyph("kb.cancel", "Close keyboard", Glyph::Close);
            browser_chrome::fits(
                browser_chrome::address_keys(browser_chrome::toolbar(builder, self.zoom, true))
                    .keyboard(self.address.keyboard(), "Go"),
                metrics,
                true,
            )
        });
        paginator.pages().first().cloned().unwrap_or_default()
    }

    fn displayed_address(&self) -> String {
        if self.address.is_open() {
            self.address.keyboard().caret_text()
        } else if let View::Loading(url)|View::Failed(url,_)|View::Unsupported(url,_)=&self.view{
            url.to_string()
        } else {
            self.loaded
                .as_ref()
                .map_or_else(String::new, |loaded| loaded.url.to_string())
        }
    }

    fn browser_content(&self, metrics: &DisplayMetrics) -> ScreenBuilder {
        if self.form_entry.is_open() {
            return self.form_entry_screen();
        }
        match (&self.view, &self.loaded) {
            (View::Start, _) if self.address.is_open() => {
                ScreenBuilder::new("browser-start-typing")
                    .top_bar("Browse")
                    .field(
                        "edit-address",
                        self.displayed_address(),
                        "Search or enter address",
                    )
                    .secondary("Search the web or enter a website address")
            }
            (View::Start, _) => Self::start_screen(),
            (View::Loading(url), _) => ScreenBuilder::new("browser-loading")
                .top_bar(url.host())
                .field(
                    "edit-address",
                    if self.address.is_open() {
                        self.address.keyboard().caret_text()
                    } else {
                        url.to_string()
                    },
                    "Search or enter address",
                )
                .activity("Loading the page", None)
                .secondary(self.clock.waited_words())
                .secondary(url.to_string()),
            (View::Failed(url, failure), loaded) => {
                let (heading, sentence) = failure.explain();
                let mut builder = ScreenBuilder::new("browser-failed")
                    .top_bar(url.host())
                    .field("edit-address",self.displayed_address(),"Search or enter address")
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
            (View::PostConfirm(index), Some(loaded)) => {
                Self::post_screen(forms::forms(&loaded.document.blocks).get(*index).copied())
            }
            (View::FormUnavailable(reason), _) => Self::form_unavailable_screen(reason),
            (View::Unsupported(url, what), loaded) => {
                let builder = ScreenBuilder::new("browser-unsupported")
                    .top_bar(url.host())
                    .field("edit-address",self.displayed_address(),"Search or enter address")
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
            (View::Navigate, Some(_)) => self.navigate_screen(),
            (View::Bookmarks(page), _) => self.library_screen(false, *page, metrics),
            (View::Recent(page), _) => self.library_screen(true, *page, metrics),
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
            (View::Page, Some(loaded)) if loaded.painted.is_some() => {
                loaded.painted.as_ref().expect("painted").screen(
                    &loaded.title,
                    &self.displayed_address(),
                    self.address.is_open().then(|| self.address.keyboard()),
                    |index|{
                        let target=loaded.url.join(loaded.painted.as_ref()?.view.links().get(index)?).ok()?;
                        loaded.document.links.iter().position(|link|link.target==target).map(|i|action_id(&link_action(i)))
                    },
                )
            }
            (_, Some(loaded)) => page_screen_with_address(
                &if loaded.paint_refused {
                    format!("Reader: {}", loaded.title)
                } else {
                    loaded.title.clone()
                },
                &self.keyboard_pieces(metrics),
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
                metrics,
                Some(&self.displayed_address()),
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
            View::Page if loaded.painted.is_some() => self.page_links(),
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
        context.store().load("browser-library");
        context.store().load("browser-home");
        context.device().read_identity();
        self.view = View::Start;
        self.show(context);
    }

    fn on_action(&mut self, context: &mut Context, action: ActionId) {
        self.validate_css(context);
        // The shell offers its reserved Back only on screens that declare an
        // internal destination. Match the app's named Back after that boundary.
        if action==ActionId::BACK && self.form_entry.is_open(){
            self.form_entry.close();self.editing=None;self.show(context);return;
        }
        let action=if action==ActionId::BACK{action_id("back")}else{action};
        if ["navigate","links","sections","bookmarks","recent"].into_iter().any(|name|action==action_id(name)){
            self.browser_menu=false;self.zoom_panel=false;
        }
        if let Some(value)=action.0.checked_sub(page_mode::SCROLL_BASE.0).filter(|value|*value<=200){
            if matches!(self.view,View::Page)&&!self.address.is_open()&&!self.browser_menu&&!self.zoom_panel{
                if let Some(loaded)=self.loaded.as_mut(){if let Some(painted)=loaded.painted.as_mut(){
                    painted.scroll(value as i32-100);
                    let key=loaded.url.to_string();let place=(self.zoom,painted.scroll_position());
                    if self.scroll_places.len()>=64&&!self.scroll_places.contains_key(&key){if let Some(first)=self.scroll_places.keys().next().cloned(){self.scroll_places.remove(&first);}}
                    self.scroll_places.insert(key,place);self.show(context);
                }}
            }
            return;
        }
        if action == action_id("browser-close") {
            context.exit();
            return;
        }
        if action == action_id("home") {
            if self.pending.is_some()||self.reading.is_some(){self.stop(context);}
            self.zoom_panel=false;self.browser_menu=false;
            self.view = View::Start;
            self.address.close();
            self.show(context);
            return;
        }
        if action == action_id("samples") {
            if let Ok(url) = Url::parse(SAMPLES) {
                self.open(context, &url);
            }
            return;
        }
        if self.form_entry.is_open() {
            self.handle_form_entry(context, action);
            return;
        }
        if self.address.is_open()
            && (action == action_id("zoom-less")
                || action == action_id("zoom-more")
                || action
                    .0
                    .checked_sub(browser_chrome::SLIDER_BASE.0)
                    .is_some_and(|value| value <= 100))
        {
            self.browser_toolbar_action(context, action);
            return;
        }
        if self.handle_address(context, action) {
            return;
        }
        if self.browser_toolbar_action(context, action) {
            return;
        }
        if self.library_action(context, action) {
            return;
        }
        if self.submit_form(context, action) {
            return;
        }
        if self.handle_form_return(context, action) {
            return;
        }
        if self.form_action(context, action) {
            self.show(context);
            return;
        }
        if action == action_id("next-page") || action == action_id("previous-page") {
            if let Some(loaded)=self.loaded.as_ref(){self.scroll_places.remove(&loaded.url.to_string());}
            if let Some(page) = self.loaded.as_ref().map(|loaded| {
                loaded
                    .painted
                    .as_ref()
                    .map_or(loaded.page, |painted| painted.page)
            }) {
                let to = if action == action_id("next-page") {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                };
                self.turn_to(context, to);
            }
        } else if matches!(self.view, View::Loading(_))
            && (action == action_id("cancel-load") || action == action_id("back") || action==ActionId::BACK)
        {
            self.stop(context);
        } else if action == action_id("retry") {
            if let Some(url) = self.retry.take() {
                self.fetch(context, &url, None);
            }
        } else if action == action_id("back")
            && matches!(
                self.view,
                View::Links(_)
                    | View::Sections(_)
                    | View::Navigate
                    | View::Bookmarks(_)
                    | View::Recent(_)
            )
        {
            self.view = if self.loaded.is_some() {
                View::Page
            } else {
                View::Start
            };
        } else if action == action_id("back") {
            self.stepped_from = Some(self.history.position());
            let go = self.history.back();
            self.carry_out(context, go);
            return;
        } else if action == action_id("forward") {
            if self.pending.is_some()||self.reading.is_some(){self.stop(context);}
            self.zoom_panel=false;self.browser_menu=false;
            self.stepped_from = Some(self.history.position());
            let go = self.history.forward();
            self.carry_out(context, go);
            return;
        } else if action == action_id("navigate") {
            self.browser_menu=false;self.zoom_panel=false;
            self.view = View::Navigate;
        } else if action == action_id("links") {
            self.view = View::Links(0);
        } else if action == action_id("sections") {
            if let Some(loaded) = self.loaded.as_mut() {
                loaded.painted = None;
            }
            context.drop_picture(page_mode::HANDLE);
            self.view = View::Sections(0);
        } else if self.page_mode_action(context, action) {
            // The view switch owns its cursor and retains the native reader.
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
            self.view = if self.loaded.is_some() {
                View::Page
            } else {
                View::Start
            };
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
        self.validate_css(context);
        if self.pager == Some(task) {
            self.pager = None;
            if !matches!(outcome, TaskOutcome::Cancelled) {
                self.page_on(context);
            }
            return;
        }
        if self.sheets.on_task(context, task, &outcome) {
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
        context: &mut Context,
        request: kobo_sdk::DeviceRequest,
        result: kobo_sdk::DeviceResult,
    ) {
        let mut colour_changed = false;
        if request == kobo_sdk::DeviceRequest::ReadIdentity {
            if let kobo_sdk::DeviceResult::Identity(identity) = result {
                colour_changed = self.pictures.colour() != identity.colour_panel();
                self.pictures.set_colour(identity.colour_panel());
            }
        }
        // A panel identity answer is exactly the moment a frame measured
        // for another panel must be retired.
        self.validate_css(context);
        if colour_changed {
            // Pending shelf reads and shown native pictures were prepared for
            // the old panel format. Retire them before requesting the visible
            // images again under the new format's distinct cache keys.
            for key in self.pictures.clear(context) {
                self.saved.stop_reading(&key);
            }
            self.refresh_native_colour(context);
            // An already-visible Page raster also needs publication in the
            // confirmed format. Native Reader keeps its position and controls.
            self.fetch_pictures(context);
            self.show(context);
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

    fn on_background(&mut self, _context: &mut Context) {
        // Leaving the foreground ends the moment a frame was measured for,
        // just as suspend and exit do; re-admission needs a fresh arrival.
        self.css.clear();
    }

    fn on_suspend(&mut self, context: &mut Context) {
        // Sleep may outlive the moment a frame was measured for.
        self.css.clear();
        self.on_background(context);
    }

    fn on_exit(&mut self, _context: &mut Context) {
        self.css.clear();
    }

    fn on_page_turn(&mut self, context: &mut Context, forward: bool) {
        self.validate_css(context);
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
            (View::Bookmarks(_) | View::Recent(_), true) => "places-next",
            (View::Bookmarks(_) | View::Recent(_), false) => "places-previous",
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

fn place_screen(
    entries: &[Place],
    total: usize,
    page: usize,
    of: usize,
    recent: bool,
) -> ScreenBuilder {
    let name = if recent { "History" } else { "Bookmarks" };
    let builder = ScreenBuilder::new("browser-places")
        .top_bar(name)
        .top_bar_action("return", "Done")
        .secondary(format!(
            "{total} {}",
            if total == 1 { "page" } else { "pages" }
        ))
        .page_turns("places-previous", "places-next")
        .page_position(
            u16::try_from(page + 1).unwrap_or(u16::MAX),
            u16::try_from(of.max(1)).unwrap_or(u16::MAX),
        );
    if total == 0 {
        return builder.text(if recent {
            "No pages visited yet."
        } else {
            "No bookmarks yet."
        });
    }
    builder.rows(entries.iter().enumerate().map(|(i, place)| {
        (
            format!("place-{i}"),
            place.title.clone(),
            place.url.to_string(),
            Glyph::Bookmark,
        )
    }))
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
#[cfg(test)]
mod css_stage_tests;
#[cfg(all(test, debug_assertions))]
mod fixture_tests;
#[cfg(test)]
mod image_tests;
#[cfg(test)]
mod sheet_supply_tests;

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
