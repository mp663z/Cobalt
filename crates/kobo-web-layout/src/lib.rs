//! A web document, cut into pages that fit the panel they are drawn on.
//!
//! Nothing here guesses at line heights. A page is full when the toolkit's
//! own layout of it says something was pushed off the bottom, measured with
//! the same frame (top bar, page turns, action bar) the page is drawn in, so
//! a page that fits here fits on the panel at every profile and text size.
//! The caller supplies that check as a closure, which is also what lets the
//! packing rules be tested without fonts.
//!
//! The document is first flattened into [`Piece`]s, one per thing the toolkit
//! can draw, and pieces are packed greedily. A piece that does not fit is cut:
//! prose at a word, preformatted text at a line, a table between rows with its
//! header repeated. A heading is never left alone at the foot of a page.

use std::collections::BTreeMap;

use kobo_sdk::{DisplayMetrics, Screen, ScreenBuilder};
use kobo_ui::{
    Chrome, LayoutIssueKind, ParagraphPresentation, RichTextSpan, TableRow, TextPresentation,
};
use kobo_web_document::{Block, Document, Field, Form, Inline, Table, Warning};

/// Pages past this are not made. A 2 MiB page of prose is well under it at
/// the largest text size; the cap exists so that no input can make the
/// packing loop run without end.
pub const MAX_PAGES: usize = 2_000;

/// Inline links one piece of prose may carry. The toolkit draws at most this
/// many in a paragraph, so a paragraph with more is carried as several pieces
/// rather than having its later links silently lose their targets.
pub const MAX_LINKS_PER_PIECE: usize = kobo_ui::MAX_TEXT_LINKS;

/// Rows one table piece may carry, the toolkit's own ceiling.
pub const MAX_ROWS_PER_PIECE: usize = kobo_ui::MAX_TABLE_ROWS;

/// The action name a link is raised under.
#[must_use]
pub fn link_action(index: usize) -> String {
    format!("link-{index}")
}

/// The link index an action name was made from by [`link_action`].
#[must_use]
pub fn link_of(action: &str) -> Option<usize> {
    action.strip_prefix("link-")?.parse().ok()
}

/// A stretch of prose in one style.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Run {
    pub text: String,
    pub strong: bool,
    pub emphasis: bool,
    /// Index into [`Document::links`].
    pub link: Option<usize>,
}

/// One thing the toolkit draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Piece {
    /// `links` are the links the heading's words carry. The toolkit draws a
    /// heading without inline targets, so they are offered on the Links
    /// screen instead.
    Heading {
        level: u8,
        text: String,
        links: Vec<usize>,
    },
    Prose(Vec<Run>),
    /// Quoted prose, with its links offered as a heading's are.
    Quote {
        depth: u8,
        text: String,
        links: Vec<usize>,
    },
    Preformatted(String),
    /// `links` holds, for each row, the links its cells carry.
    Table {
        rows: Vec<TableRow>,
        links: Vec<Vec<usize>>,
    },
    /// Something in the page that is described rather than shown: an image
    /// before images are fetched, a form before forms are supported.
    Note(String),
    Rule,
}

impl Piece {
    fn is_heading(&self) -> bool {
        matches!(self, Self::Heading { .. })
    }

    /// Links this piece makes tappable in the text, in reading order.
    #[must_use]
    pub fn links(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        if let Self::Prose(runs) = self {
            for link in runs.iter().filter_map(|run| run.link) {
                if out.last() != Some(&link) {
                    out.push(link);
                }
            }
        }
        out
    }

    /// Every link on this piece, tappable in the text or not, in reading
    /// order: what the Links screen offers.
    #[must_use]
    pub fn all_links(&self) -> Vec<usize> {
        match self {
            Self::Heading { links, .. } | Self::Quote { links, .. } => links.clone(),
            Self::Table { links, .. } => links.iter().flatten().copied().collect(),
            _ => self.links(),
        }
    }
}

/// A document cut into pages.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Layout {
    pub pages: Vec<Vec<Piece>>,
    /// Page each fragment target starts on.
    pub anchors: BTreeMap<String, usize>,
    /// Whether [`MAX_PAGES`] was reached and the rest left out.
    pub truncated: bool,
}

impl Layout {
    /// The page a fragment points at, if the document has it.
    #[must_use]
    pub fn page_of(&self, fragment: &str) -> Option<usize> {
        self.anchors.get(fragment).copied()
    }
}

/// Whether a built page fits the panel: nothing pushed off it or clipped.
///
/// Only overflow counts. A character the face lacks is drawn as a
/// replacement and a touch target is the frame's business, not the text's.
#[must_use]
pub fn fits(screen: &Screen, metrics: &DisplayMetrics) -> bool {
    !screen
        .diagnostics(metrics, &Chrome::measuring(true))
        .issues
        .iter()
        .any(|issue| {
            matches!(
                issue.kind,
                LayoutIssueKind::ContentOverflow { .. }
                    | LayoutIssueKind::Clipped
                    | LayoutIssueKind::TextOverflow
                    | LayoutIssueKind::InteractiveOffscreen
            )
        })
}

/// The browser's page: the frame every page is drawn in and measured in.
///
/// `page` is zero-based. Pagination measures with the frame it will be drawn
/// in, so the frame lives here beside the packing rather than in the app,
/// where a change to one could quietly stop matching the other.
#[must_use]
pub fn page_screen(title: &str, pieces: &[Piece], page: usize, of: usize) -> ScreenBuilder {
    let builder = ScreenBuilder::new("browser-page")
        .top_bar(title)
        .top_bar_action("links", "Links")
        .reading(true)
        .page_turns("previous-page", "next-page")
        .page_position(
            u16::try_from(page.saturating_add(1)).unwrap_or(u16::MAX),
            u16::try_from(of.max(1)).unwrap_or(u16::MAX),
        )
        .action_bar([
            ("back", "Back"),
            ("address", "Go to"),
            ("forward", "Forward"),
        ]);
    append(builder, pieces)
}

/// Pages a document for one panel, measured in [`page_screen`].
#[must_use]
pub fn paginate_for(document: &Document, title: &str, metrics: &DisplayMetrics) -> Layout {
    paginate(document, |pieces| {
        fits(&page_screen(title, pieces, 998, 999).build(), metrics)
    })
}

/// Adds a page's pieces to a screen.
#[must_use]
pub fn append(mut builder: ScreenBuilder, pieces: &[Piece]) -> ScreenBuilder {
    for piece in pieces {
        builder = match piece {
            Piece::Heading { level, text, .. } => builder.heading_at_level(*level, text),
            Piece::Prose(runs) => prose(builder, runs),
            Piece::Quote { depth, text, .. } => builder.quote(*depth, text),
            Piece::Preformatted(text) => builder.text(text),
            Piece::Table { rows, .. } => builder.table(rows.clone(), Vec::new()),
            Piece::Note(text) => builder.secondary(text),
            Piece::Rule => builder.divider(),
        };
    }
    builder
}

fn prose(builder: ScreenBuilder, runs: &[Run]) -> ScreenBuilder {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut links: Vec<(String, usize, usize)> = Vec::new();
    for run in runs {
        let start = text.len();
        text.push_str(&run.text);
        let end = text.len();
        if start == end {
            continue;
        }
        if run.strong || run.emphasis {
            spans.push(RichTextSpan {
                start,
                end,
                presentation: TextPresentation {
                    strong: run.strong,
                    emphasis: run.emphasis,
                    ..TextPresentation::default()
                },
            });
        }
        if let Some(link) = run.link {
            match links.last_mut() {
                Some((name, _, last_end)) if *last_end == start && *name == link_action(link) => {
                    *last_end = end;
                }
                _ => links.push((link_action(link), start, end)),
            }
        }
    }
    if spans.is_empty() && links.is_empty() {
        builder.text(text)
    } else if spans.is_empty() {
        builder.text_linking(text, links)
    } else {
        builder.rich_text_linking(text, spans, ParagraphPresentation::default(), links)
    }
}

/// Flattens a document into pieces, with each anchor's piece index.
#[must_use]
pub fn pieces(document: &Document) -> (Vec<Piece>, Vec<(String, usize)>) {
    let mut flat = Flattener::default();
    if let Some(note) = warnings_note(&document.warnings) {
        flat.out.push(Piece::Note(note));
    }
    flat.blocks(&document.blocks, 0);
    (flat.out, flat.anchors)
}

fn warnings_note(warnings: &[Warning]) -> Option<String> {
    let mut left_out = Vec::new();
    for warning in warnings {
        let what = match warning {
            Warning::InputTruncated | Warning::TooManyNodes | Warning::TextTruncated => {
                "the end of a very long page"
            }
            Warning::TooDeep => "deeply nested content",
            Warning::TooManyBlocks => "blocks past the page limit",
            Warning::TooManyLinks => "links past the link limit",
            Warning::TooManyImages => "images past the image limit",
            Warning::TableClipped => "rows or columns of a large table",
            Warning::ActiveContentRemoved(_)
            | Warning::AttributesDropped
            | Warning::UnsupportedLinks(_)
            | Warning::NotUtf8 => continue,
        };
        if !left_out.contains(&what) {
            left_out.push(what);
        }
    }
    (!left_out.is_empty()).then(|| format!("Left out: {}.", left_out.join(", ")))
}

#[derive(Default)]
struct Flattener {
    out: Vec<Piece>,
    anchors: Vec<(String, usize)>,
}

#[derive(Clone, Copy, Default)]
struct Style {
    strong: bool,
    emphasis: bool,
    link: Option<usize>,
}

impl Flattener {
    fn anchor(&mut self, anchor: Option<&String>) {
        if let Some(anchor) = anchor {
            self.anchors.push((anchor.clone(), self.out.len()));
        }
    }

    fn blocks(&mut self, blocks: &[Block], quote: u8) {
        for block in blocks {
            self.block(block, quote, None);
        }
    }

    fn block(&mut self, block: &Block, quote: u8, marker: Option<String>) {
        match block {
            Block::Heading {
                level,
                content,
                anchor,
            } => {
                self.anchor(anchor.as_ref());
                let text = plain(content);
                if !text.trim().is_empty() {
                    self.out.push(Piece::Heading {
                        level: (*level).clamp(1, 6),
                        text: text.trim().to_owned(),
                        links: links_in(content),
                    });
                }
            }
            Block::Paragraph { content, anchor } => {
                self.anchor(anchor.as_ref());
                self.paragraph(content, quote, marker);
            }
            Block::List {
                ordered,
                start,
                items,
            } => {
                let mut number = *start;
                for item in items {
                    let mark = if *ordered {
                        let mark = format!("{number}. ");
                        number = number.saturating_add(1);
                        mark
                    } else {
                        "\u{2022} ".to_owned()
                    };
                    let mut mark = Some(mark);
                    for inner in item {
                        self.block(inner, quote, mark.take());
                    }
                    if let Some(mark) = mark {
                        self.out.push(Piece::Prose(vec![Run {
                            text: mark.trim_end().to_owned(),
                            ..Run::default()
                        }]));
                    }
                }
            }
            Block::Quote(inner) => {
                for block in inner {
                    self.block(block, quote.saturating_add(1).min(3), None);
                }
            }
            Block::Preformatted(text) => {
                let text = text.trim_end_matches('\n');
                if !text.is_empty() {
                    self.out.push(Piece::Preformatted(drawable(text)));
                }
            }
            Block::Table(table) => self.table(table),
            Block::Image(image) => {
                let alt = drawable(image.alt.trim());
                let label = if alt.is_empty() {
                    "Image".to_owned()
                } else {
                    format!("Image: {alt}")
                };
                // An image inside a link is often the only way to follow it,
                // so its description is the link.
                self.out.push(match image.link {
                    Some(link) => Piece::Prose(vec![Run {
                        text: label,
                        link: Some(link),
                        ..Run::default()
                    }]),
                    None => Piece::Note(label),
                });
            }
            Block::Form(form) => self.out.push(Piece::Note(form_note(form))),
            Block::Rule => self.out.push(Piece::Rule),
        }
    }

    fn paragraph(&mut self, content: &[Inline], quote: u8, marker: Option<String>) {
        let mut runs = Vec::new();
        if let Some(marker) = marker {
            runs.push(Run {
                text: marker,
                ..Run::default()
            });
        }
        inline_runs(content, Style::default(), &mut runs);
        let runs = tidy(runs);
        if runs.iter().all(|run| run.text.trim().is_empty()) {
            return;
        }
        if quote > 0 {
            let text: String = runs.iter().map(|run| run.text.as_str()).collect();
            let mut links: Vec<usize> = Vec::new();
            for link in runs.iter().filter_map(|run| run.link) {
                if !links.contains(&link) {
                    links.push(link);
                }
            }
            self.out.push(Piece::Quote {
                depth: quote,
                text,
                links,
            });
            return;
        }
        for chunk in split_links(runs) {
            self.out.push(Piece::Prose(chunk));
        }
    }

    fn table(&mut self, table: &Table) {
        if let Some(caption) = &table.caption {
            self.out.push(Piece::Note(drawable(caption)));
        }
        let rows: Vec<TableRow> = table
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| TableRow {
                header: table.header && index == 0,
                cells: row
                    .iter()
                    .map(|cell| plain(cell).trim().to_owned())
                    .collect(),
            })
            .collect();
        if rows.is_empty() {
            return;
        }
        let row_links: Vec<Vec<usize>> = table
            .rows
            .iter()
            .map(|row| row.iter().flat_map(|cell| links_in(cell)).collect())
            .collect();
        let header = rows.first().filter(|row| row.header).cloned();
        let body_start = usize::from(header.is_some());
        let per = MAX_ROWS_PER_PIECE - body_start;
        if rows.len() == body_start {
            self.out.push(Piece::Table {
                rows,
                links: row_links,
            });
            return;
        }
        let header_links: Vec<Vec<usize>> = row_links[..body_start].to_vec();
        for (chunk, chunk_links) in rows[body_start..]
            .chunks(per)
            .zip(row_links[body_start..].chunks(per))
        {
            let mut rows: Vec<TableRow> = header.iter().cloned().collect();
            rows.extend(chunk.iter().cloned());
            let mut links = header_links.clone();
            links.extend(chunk_links.iter().cloned());
            self.out.push(Piece::Table { rows, links });
        }
        if table.clipped {
            self.out.push(Piece::Note("Table cut short.".to_owned()));
        }
    }
}

fn form_note(form: &Form) -> String {
    let label = form.fields.iter().find_map(|field| match field {
        Field::Text { label, search, .. } if *search || !label.is_empty() => {
            Some(if label.is_empty() {
                "Search".to_owned()
            } else {
                label.clone()
            })
        }
        _ => None,
    });
    format!(
        "Form: {} (forms are not supported yet)",
        label.unwrap_or_else(|| "fields".to_owned())
    )
}

/// Text with every character the panel's faces cannot draw replaced by one
/// they can. A page with a character the type lacks would otherwise draw a
/// gap, and an emoji in a comment is not a reason to lose the comment.
fn drawable(text: &str) -> String {
    let faces = [kobo_ui::Face::Reading, kobo_ui::Face::Text];
    if faces
        .iter()
        .all(|&face| kobo_ui::undrawable_in(text, face).is_none())
    {
        return text.to_owned();
    }
    let mut buffer = [0_u8; 4];
    let stand_in = if faces
        .iter()
        .all(|&face| kobo_ui::undrawable_in(REPLACEMENT, face).is_none())
    {
        REPLACEMENT
    } else {
        "?"
    };
    text.chars()
        .map(|character| {
            let one: &str = character.encode_utf8(&mut buffer);
            if faces
                .iter()
                .all(|&face| kobo_ui::undrawable_in(one, face).is_none())
            {
                one.to_owned()
            } else {
                stand_in.to_owned()
            }
        })
        .collect()
}

const REPLACEMENT: &str = "\u{fffd}";

fn inline_runs(content: &[Inline], style: Style, out: &mut Vec<Run>) {
    for inline in content {
        match inline {
            Inline::Text(text) | Inline::Code(text) => out.push(Run {
                text: drawable(text),
                strong: style.strong,
                emphasis: style.emphasis,
                link: style.link,
            }),
            Inline::Emphasis(inner) => inline_runs(
                inner,
                Style {
                    emphasis: true,
                    ..style
                },
                out,
            ),
            Inline::Strong(inner) => inline_runs(
                inner,
                Style {
                    strong: true,
                    ..style
                },
                out,
            ),
            Inline::Link { index, label } => inline_runs(
                label,
                Style {
                    link: Some(*index),
                    ..style
                },
                out,
            ),
            Inline::Inert(inner) => inline_runs(
                inner,
                Style {
                    link: None,
                    ..style
                },
                out,
            ),
            Inline::LineBreak => out.push(Run {
                text: "\n".to_owned(),
                ..Run::default()
            }),
        }
    }
}

/// Merges neighbouring runs of one style and trims the paragraph's ends.
fn tidy(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last)
                if last.strong == run.strong
                    && last.emphasis == run.emphasis
                    && last.link == run.link =>
            {
                last.text.push_str(&run.text);
            }
            _ => out.push(run),
        }
    }
    if let Some(first) = out.first_mut() {
        first.text = first.text.trim_start().to_owned();
    }
    if let Some(last) = out.last_mut() {
        last.text = last.text.trim_end().to_owned();
    }
    out.retain(|run| !run.text.is_empty());
    out
}

/// Cuts a paragraph so no piece carries more links than the toolkit draws.
fn split_links(runs: Vec<Run>) -> Vec<Vec<Run>> {
    let mut chunks = Vec::new();
    let mut chunk: Vec<Run> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for run in runs {
        if let Some(link) = run.link {
            if seen.last() != Some(&link) {
                if seen.len() == MAX_LINKS_PER_PIECE {
                    chunks.push(tidy(std::mem::take(&mut chunk)));
                    seen.clear();
                }
                seen.push(link);
            }
        }
        chunk.push(run);
    }
    if !chunk.is_empty() {
        chunks.push(tidy(chunk));
    }
    chunks.retain(|chunk| !chunk.is_empty());
    chunks
}

/// Links carried anywhere in some inline content, in order, once each.
fn links_in(content: &[Inline]) -> Vec<usize> {
    let mut runs = Vec::new();
    inline_runs(content, Style::default(), &mut runs);
    let mut links: Vec<usize> = Vec::new();
    for link in runs.iter().filter_map(|run| run.link) {
        if !links.contains(&link) {
            links.push(link);
        }
    }
    links
}

fn plain(content: &[Inline]) -> String {
    let mut runs = Vec::new();
    inline_runs(content, Style::default(), &mut runs);
    let mut text = String::new();
    for run in runs {
        text.push_str(&run.text);
    }
    text
}

/// Cuts a document into pages.
///
/// `fits` answers whether a page made of these pieces fits the panel. It is
/// asked about candidate pages many times, so it must be deterministic: the
/// same pieces always get the same answer.
pub fn paginate(document: &Document, fits: impl FnMut(&[Piece]) -> bool) -> Layout {
    let (pieces, anchors) = pieces(document);
    paginate_pieces(pieces, &anchors, fits)
}

/// [`paginate`], from pieces already flattened.
pub fn paginate_pieces(
    pieces: Vec<Piece>,
    anchors: &[(String, usize)],
    mut fits: impl FnMut(&[Piece]) -> bool,
) -> Layout {
    let mut paginator = Paginator::new(pieces, anchors.to_vec());
    while paginator.next_page(&mut fits) {}
    paginator.into_layout()
}

/// Pagination a page at a time.
///
/// A long page costs hundreds of milliseconds to cut up even on the host, so
/// the browser makes the page being read and the one after it first, and the
/// rest while nobody is waiting. Pages come out exactly as [`paginate`] makes
/// them; only when they are made changes.
#[derive(Clone, Debug)]
pub struct Paginator {
    queue: std::collections::VecDeque<(usize, Piece)>,
    anchors: Vec<(String, usize)>,
    /// Page each original piece first appears on.
    starts: BTreeMap<usize, usize>,
    pages: Vec<Vec<Piece>>,
    truncated: bool,
}

impl Paginator {
    #[must_use]
    pub fn new(pieces: Vec<Piece>, anchors: Vec<(String, usize)>) -> Self {
        Self {
            queue: pieces.into_iter().enumerate().collect(),
            anchors,
            starts: BTreeMap::new(),
            pages: Vec::new(),
            truncated: false,
        }
    }

    /// Flattens a document and starts paginating it.
    #[must_use]
    pub fn for_document(document: &Document) -> Self {
        let (pieces, anchors) = pieces(document);
        Self::new(pieces, anchors)
    }

    /// Pages made so far.
    #[must_use]
    pub fn pages(&self) -> &[Vec<Piece>] {
        &self.pages
    }

    /// Whether every page has been made.
    #[must_use]
    pub fn done(&self) -> bool {
        self.queue.is_empty() || self.truncated
    }

    /// Makes the next page. Returns false once there is nothing left to make.
    pub fn next_page(&mut self, fits: &mut impl FnMut(&[Piece]) -> bool) -> bool {
        if self.done() {
            return false;
        }
        if self.pages.len() >= MAX_PAGES {
            self.truncated = true;
            return false;
        }
        let mut page: Vec<Piece> = Vec::new();
        let mut origins: Vec<usize> = Vec::new();
        'fill: {
            // Take as many whole pieces as fit, found by galloping rather than
            // one at a time: every question lays out the whole candidate page.
            let whole = gallop(&page, &self.queue, fits);
            for _ in 0..whole {
                if let Some((origin, piece)) = self.queue.pop_front() {
                    page.push(piece);
                    origins.push(origin);
                }
            }
            let Some((origin, piece)) = self.queue.pop_front() else {
                break 'fill;
            };
            // Does not fit whole. Take as much of it as fits here.
            if let Some((head, tail)) = cut(&page, &piece, fits) {
                page.push(head);
                origins.push(origin);
                self.queue.push_front((origin, tail));
                break 'fill;
            }
            if page.is_empty() {
                // Nothing of it fits even on an empty page. Draw it alone and
                // let the toolkit clip it: a page that shows too little is
                // better than a loop that never ends or a piece that
                // disappears.
                page.push(piece);
                origins.push(origin);
                break 'fill;
            }
            // Keep a heading with what follows it, unless the heading is all
            // the page has.
            self.queue.push_front((origin, piece));
            while page.len() > 1 && page.last().is_some_and(Piece::is_heading) {
                if let (Some(heading), Some(from)) = (page.pop(), origins.pop()) {
                    self.queue.push_front((from, heading));
                }
            }
        }
        let index = self.pages.len();
        for origin in origins {
            self.starts.entry(origin).or_insert(index);
        }
        self.pages.push(page);
        true
    }

    /// The page a fragment starts on, if that page has been made.
    #[must_use]
    pub fn page_of(&self, fragment: &str) -> Option<usize> {
        let (_, index) = self.anchors.iter().find(|(name, _)| name == fragment)?;
        match self.starts.range(*index..).next() {
            Some((_, &page)) => Some(page),
            None if self.done() => Some(self.pages.len().saturating_sub(1)),
            None => None,
        }
    }

    /// Whether the document has this fragment at all.
    #[must_use]
    pub fn has_fragment(&self, fragment: &str) -> bool {
        self.anchors.iter().any(|(name, _)| name == fragment)
    }

    #[must_use]
    pub fn into_layout(self) -> Layout {
        let mut layout = Layout {
            anchors: BTreeMap::new(),
            truncated: self.truncated,
            pages: Vec::new(),
        };
        for (anchor, _) in &self.anchors {
            if let Some(page) = self.page_of(anchor) {
                layout.anchors.entry(anchor.clone()).or_insert(page);
            }
        }
        layout.pages = self.pages;
        layout
    }
}

/// How many of the queue's leading pieces fit after `page`, all together.
fn gallop(
    page: &[Piece],
    queue: &std::collections::VecDeque<(usize, Piece)>,
    fits: &mut impl FnMut(&[Piece]) -> bool,
) -> usize {
    let mut try_count = |count: usize| {
        let mut candidate = page.to_vec();
        candidate.extend(queue.iter().take(count).map(|(_, piece)| piece.clone()));
        fits(&candidate)
    };
    // Largest count known to fit, and smallest known not to.
    let (mut good, mut bad) = (0_usize, None::<usize>);
    let mut step = 1_usize;
    while good < queue.len() {
        let next = (good + step).min(queue.len());
        if try_count(next) {
            good = next;
            step = step.saturating_mul(2);
        } else {
            bad = Some(next);
            break;
        }
    }
    let Some(mut bad) = bad else {
        return good;
    };
    while bad - good > 1 {
        let middle = good + (bad - good) / 2;
        if try_count(middle) {
            good = middle;
        } else {
            bad = middle;
        }
    }
    good
}

/// The largest leading part of `piece` that fits after `page`, and the rest.
fn cut(
    page: &[Piece],
    piece: &Piece,
    fits: &mut impl FnMut(&[Piece]) -> bool,
) -> Option<(Piece, Piece)> {
    let points = cut_points(piece);
    if points.is_empty() {
        return None;
    }
    let try_at = |at: usize, fits: &mut dyn FnMut(&[Piece]) -> bool| {
        let (head, _) = split_at(piece, at);
        let mut candidate = page.to_vec();
        candidate.push(head);
        fits(&candidate)
    };
    // Binary search for the last cut point whose head fits.
    let (mut low, mut high) = (0_usize, points.len());
    while low < high {
        let middle = low + (high - low) / 2;
        if try_at(points[middle], fits) {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if low == 0 {
        if !page.is_empty() {
            return None;
        }
        // Not even the first word fits a page of its own: a word wider than
        // the panel. Cut inside it, at characters.
        let chars = char_points(piece);
        let (mut low, mut high) = (0_usize, chars.len());
        while low < high {
            let middle = low + (high - low) / 2;
            if try_at(chars[middle], fits) {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low == 0 {
            return None;
        }
        return Some(split_at(piece, chars[low - 1]));
    }
    Some(split_at(piece, points[low - 1]))
}

fn text_of(piece: &Piece) -> Option<String> {
    match piece {
        Piece::Prose(runs) => Some(runs.iter().map(|run| run.text.as_str()).collect()),
        Piece::Quote { text, .. } | Piece::Preformatted(text) => Some(text.clone()),
        _ => None,
    }
}

/// Places a piece may be cut, as offsets that leave both halves non-empty.
/// For tables, an offset is a row count.
fn cut_points(piece: &Piece) -> Vec<usize> {
    match piece {
        Piece::Table { rows, .. } => {
            let header = usize::from(rows.first().is_some_and(|row| row.header));
            (header + 1..rows.len()).collect()
        }
        Piece::Preformatted(text) => text
            .match_indices('\n')
            .map(|(at, _)| at + 1)
            .filter(|&at| at < text.len())
            .collect(),
        Piece::Prose(_) | Piece::Quote { .. } => {
            let text = text_of(piece).unwrap_or_default();
            let mut points = Vec::new();
            let mut previous_space = false;
            for (at, character) in text.char_indices() {
                let space = character.is_whitespace();
                if previous_space && !space && at > 0 {
                    points.push(at);
                }
                previous_space = space;
            }
            points
        }
        _ => Vec::new(),
    }
}

fn char_points(piece: &Piece) -> Vec<usize> {
    text_of(piece)
        .map(|text| {
            text.char_indices()
                .map(|(at, _)| at)
                .filter(|&at| at > 0)
                .collect()
        })
        .unwrap_or_default()
}

fn split_at(piece: &Piece, at: usize) -> (Piece, Piece) {
    match piece {
        Piece::Table { rows, links } => {
            let header = usize::from(rows.first().is_some_and(|row| row.header));
            let mut tail = rows[..header].to_vec();
            tail.extend(rows[at..].iter().cloned());
            let mut tail_links = links[..header].to_vec();
            tail_links.extend(links[at..].iter().cloned());
            (
                Piece::Table {
                    rows: rows[..at].to_vec(),
                    links: links[..at].to_vec(),
                },
                Piece::Table {
                    rows: tail,
                    links: tail_links,
                },
            )
        }
        Piece::Preformatted(text) => (
            Piece::Preformatted(text[..at].trim_end_matches('\n').to_owned()),
            Piece::Preformatted(text[at..].to_owned()),
        ),
        // Both halves offer the quote's links: which half a link's words fell
        // in is not tracked, and a link offered twice is better than one lost.
        Piece::Quote { depth, text, links } => (
            Piece::Quote {
                depth: *depth,
                text: text[..at].trim_end().to_owned(),
                links: links.clone(),
            },
            Piece::Quote {
                depth: *depth,
                text: text[at..].to_owned(),
                links: links.clone(),
            },
        ),
        Piece::Prose(runs) => {
            let mut head = Vec::new();
            let mut tail = Vec::new();
            let mut offset = 0;
            for run in runs {
                let end = offset + run.text.len();
                if end <= at {
                    head.push(run.clone());
                } else if offset >= at {
                    tail.push(run.clone());
                } else {
                    let split = at - offset;
                    head.push(Run {
                        text: run.text[..split].to_owned(),
                        ..run.clone()
                    });
                    tail.push(Run {
                        text: run.text[split..].to_owned(),
                        ..run.clone()
                    });
                }
                offset = end;
            }
            (Piece::Prose(tidy(head)), Piece::Prose(tidy(tail)))
        }
        other => (other.clone(), other.clone()),
    }
}

#[cfg(test)]
mod tests;
