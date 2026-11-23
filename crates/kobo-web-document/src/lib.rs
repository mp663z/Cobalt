//! Untrusted HTML into a small, bounded document model.
//!
//! The model ([`Document`]) is the boundary between the web and the panel.
//! Parser output is not trusted, layout never sees a DOM, anything the model
//! cannot say disappears or degrades to text, and every list in it has a
//! ceiling set by [`Limits`]. A page that reaches a ceiling is still shown,
//! marked with a [`Warning`] saying what was left out.

/// Device pixels per CSS pixel assumed by every layout, paint and media-query
/// path in this crate. There is no device-scale-factor model: viewport
/// width/height are `u32` CSS px that are rastered 1:1
/// (`DisplayList::rasterize(width, height)`), widths/heights/margins are
/// integer px, and media queries compare the same unscaled viewport.
/// Code that is only correct at this ratio (for example the fractional
/// border-width snap, which floors to whole device pixels) must reference this
/// constant so a future scale model fails to compile there instead of
/// silently keeping a 1x rule.
pub const DEVICE_PX_PER_CSS_PX: u32 = 1;

pub mod basic_page;
pub mod basic_text;
mod basic_text_paint;
pub mod box_tree;
pub mod computed_style;
mod convert;
mod css;
mod custom_props;
pub mod css_background;
mod css_border;
pub mod css_paint_bridge;
pub mod css_text_page;
pub mod data_stylesheet;
pub mod display_list;
pub mod flex_units;
pub mod font_family;
pub mod family_page;
pub mod flex_units_v2;
pub mod flex_units_v3;
mod dom;
pub use dom::measure;
mod host;
pub mod inline_lines;
mod media_query;
pub mod source_facts;
pub mod style_diagnostics;
mod style_syntax;
#[doc(hidden)]
pub mod refusal_classify;
pub mod style_tree;
pub mod stylesheet_bytes;
#[cfg(test)]
mod tree_tests;
pub mod url;

use html5ever::tendril::TendrilSink;

pub use css::{FocusState, HistoryState, InputState, SnapshotState, TargetState};
pub use media_query::MediaContext;
pub use style_tree::SupplyBudget;
pub use url::{Scheme, Url, UrlError};

/// Hard ceilings for one document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Bytes of markup given to the parser. The rest is not read.
    pub max_input_bytes: usize,
    /// Nodes the parser may create before input stops being fed to it.
    pub max_nodes: usize,
    /// Element nesting read into the model. Deeper content is kept as text of
    /// its shallowest dropped ancestor.
    pub max_depth: usize,
    /// Bytes of text kept across the whole document.
    pub max_text_bytes: usize,
    pub max_blocks: usize,
    pub max_links: usize,
    pub max_images: usize,
    pub max_table_rows: usize,
    pub max_table_columns: usize,
    pub max_forms: usize,
}

impl Limits {
    /// The limits the browser ships with. Chosen in the measurement spike:
    /// a 2 MiB page parses in tens of milliseconds on the host and stays
    /// well under the memory budget, and no real article comes near them.
    pub const DEFAULT: Self = Self {
        max_input_bytes: 2 * 1024 * 1024,
        max_nodes: 120_000,
        max_depth: 96,
        max_text_bytes: 1024 * 1024,
        max_blocks: 8_000,
        max_links: 4_000,
        max_images: 200,
        max_table_rows: 200,
        max_table_columns: 12,
        max_forms: 16,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A block of the document, in reading order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        content: Vec<Inline>,
        anchor: Option<String>,
    },
    Paragraph {
        content: Vec<Inline>,
        anchor: Option<String>,
    },
    List {
        ordered: bool,
        start: u32,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Preformatted(String),
    Table(Table),
    Image(ImageRef),
    Form(Form),
    Rule,
}

/// Text inside a block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Code(String),
    /// `index` is the link's position in [`Document::links`].
    Link {
        index: usize,
        label: Vec<Inline>,
    },
    /// A link whose target cannot be followed, kept as text.
    Inert(Vec<Inline>),
    LineBreak,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Table {
    pub caption: Option<String>,
    /// The first row is the header when `header` is true.
    pub rows: Vec<Vec<Vec<Inline>>>,
    pub header: bool,
    /// Whether rows or columns were cut to the limits.
    pub clipped: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageRef {
    pub src: Url,
    pub alt: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// The link the image sits inside, if any.
    pub link: Option<usize>,
}

/// A form reduced to what the browser can submit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Form {
    pub action: Url,
    pub method: Method,
    pub fields: Vec<Field>,
    /// True when this form was declared as multipart or plain text and
    /// therefore cannot be sent as URL-encoded data by this browser.
    pub urlencoded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Field {
    Text {
        name: String,
        value: String,
        label: String,
        search: bool,
    },
    Hidden {
        name: String,
        value: String,
    },
    /// A single selection, including a radio group. The chosen index is
    /// absent for a radio group with nothing checked. `radio` distinguishes
    /// an unselected group from a select with a default first option.
    Select {
        name: String,
        label: String,
        options: Vec<OptionValue>,
        chosen: Option<usize>,
        radio: bool,
    },
    Checkbox {
        name: String,
        value: String,
        label: String,
        checked: bool,
    },
    Submit {
        name: Option<String>,
        value: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionValue {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Link {
    pub target: Url,
    /// The words of the link, flattened, for the link list.
    pub text: String,
}

/// Something the reader should be told was left out or changed.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Warning {
    InputTruncated,
    TooManyNodes,
    TooDeep,
    TextTruncated,
    TooManyBlocks,
    TooManyLinks,
    TooManyImages,
    TableClipped,
    AttributesDropped,
    /// `(count)` scripts, frames, plugins and similar dropped.
    ActiveContentRemoved(usize),
    UnsupportedLinks(usize),
    NotUtf8,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Document {
    pub title: Option<String>,
    /// Where relative references were resolved from: `<base href>` if the
    /// page gave a usable one, otherwise the page's own address.
    pub base: Option<Url>,
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
    pub warnings: Vec<Warning>,
    /// Parse errors html5ever recovered from. A count, for diagnostics.
    pub parse_errors: usize,
    /// Full semantic-parse provenance, not byte identity or CSS admission.
    /// Absent on documents not produced by the parser.
    pub source_facts: Option<source_facts::ParsedSourceFacts>,
    /// The anchor where the page's own content starts, past its site
    /// navigation: the first heading or paragraph in `<main>` or
    /// `role="main"`, else the target of a "skip to content" link.
    pub main: Option<String>,
    /// Positions in `blocks` of the page's own content, with the site's
    /// navigation, sidebars and page furniture left out. Empty when the page
    /// gives no sign of where its content is.
    pub reader: Vec<usize>,
}

/// The anchor given to the start of the main content when its first block
/// has no id of its own. An id cannot hold a space, so no page can name it.
pub const MAIN_ANCHOR: &str = " main";

impl Document {
    /// Whether [`Document::reader_view`] would leave anything out.
    #[must_use]
    pub fn has_reader_view(&self) -> bool {
        !self.reader.is_empty() && self.reader.len() < self.blocks.len()
    }

    /// The page with only its own content, for reading: the blocks named by
    /// `reader`, starting at the top. `None` when the page gives no sign of
    /// where its content is, or when nothing would be left out.
    #[must_use]
    pub fn reader_view(&self) -> Option<Document> {
        if !self.has_reader_view() {
            return None;
        }
        Some(Document {
            title: self.title.clone(),
            base: self.base.clone(),
            blocks: self
                .reader
                .iter()
                .map(|&i| self.blocks[i].clone())
                .collect(),
            links: self.links.clone(),
            warnings: self.warnings.clone(),
            parse_errors: self.parse_errors,
            source_facts: self.source_facts.clone(),
            main: None,
            reader: Vec::new(),
        })
    }

    /// Every image in reading order.
    #[must_use]
    pub fn images(&self) -> Vec<&ImageRef> {
        fn walk<'a>(blocks: &'a [Block], out: &mut Vec<&'a ImageRef>) {
            for block in blocks {
                match block {
                    Block::Image(image) => out.push(image),
                    Block::List { items, .. } => items.iter().for_each(|item| walk(item, out)),
                    Block::Quote(inner) => walk(inner, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.blocks, &mut out);
        out
    }

    /// The visible text, one block per line, for tests and search.
    #[must_use]
    pub fn visible_text(&self) -> String {
        let mut out = String::new();
        convert::visible_text(&self.blocks, &mut out);
        out
    }
}

/// Parses a page fetched from `url`.
#[must_use]
pub fn parse_document(bytes: &[u8], url: &Url, limits: &Limits) -> Document {
    parse_with_styles(bytes, url, limits, false)
}

/// Parse inline author stylesheets for the bounded CSS visibility subset.
/// The unstyled entry point remains stable for reader and corpus expectations.
#[must_use]
pub fn parse_styled_document(bytes: &[u8], url: &Url, limits: &Limits) -> Document {
    parse_with_styles(bytes, url, limits, true)
}

fn parse_with_styles(bytes: &[u8], url: &Url, limits: &Limits, styled: bool) -> Document {
    let mut warnings = Vec::new();
    let input = if bytes.len() > limits.max_input_bytes {
        warnings.push(Warning::InputTruncated);
        &bytes[..limits.max_input_bytes]
    } else {
        bytes
    };
    let text = decode(input, &mut warnings);

    let sink = dom::Sink::new(limits.max_text_bytes);
    // Scripting is off, so `<noscript>` is read as the page it is for a
    // browser that runs none.
    let options = html5ever::ParseOpts {
        tree_builder: html5ever::tree_builder::TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parser = html5ever::parse_document(sink, options);
    let mut rest: &str = &text;
    while !rest.is_empty() {
        let mut end = rest.len().min(16 * 1024);
        while !rest.is_char_boundary(end) {
            end += 1;
        }
        parser.process(rest[..end].into());
        rest = &rest[end..];
        if parser.tokenizer.sink.sink.len() > limits.max_nodes {
            warnings.push(Warning::TooManyNodes);
            break;
        }
    }
    let sink = parser.finish();
    if sink.dropped_text.get() {
        warnings.push(Warning::TextTruncated);
    }
    if sink.dropped_attributes.get() > 0 || sink.merged_attributes_dropped.get() > 0 {
        warnings.push(Warning::AttributesDropped);
    }
    let parse_errors = sink.errors.get();
    let mut facts = source_facts::ParsedSourceFacts::from_sink(
        &sink,
        bytes.len(),
        bytes.len() > limits.max_input_bytes,
        warnings.contains(&Warning::TooManyNodes),
        std::str::from_utf8(input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input)).is_err(),
    );
    let nodes = sink.into_nodes();
    facts.scan(&nodes);
    let author_styles = if styled {
        css::Styles::from_dom(&nodes)
    } else {
        css::Styles::default()
    };
    let mut document = convert::convert(&nodes, url, limits, warnings, author_styles);
    document.parse_errors = parse_errors;
    document.source_facts = Some(facts);
    document
}

/// Parse HTML into a retained style arena, separate from reader conversion.
/// This does not paint or fetch resources. Callers must handle `unsupported`
/// and `truncated` rather than claim a complete rendered page.
#[must_use]
pub fn parse_style_tree(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes,
        sheets,
        limits,
        None,
        SupplyBudget::CANONICAL,
        false,
        false,
        false,
        false,
        None,
    )
}

/// Like [`parse_style_tree`] but evaluates supported `@media` width queries
/// against `media`. The caller must lay out at the same width; another width
/// needs a new parse. Without a context every width query is refused.
#[must_use]
pub fn parse_style_tree_with_media(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
    media: MediaContext,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes,
        sheets,
        limits,
        Some(media),
        SupplyBudget::CANONICAL,
        false,
        false,
        false,
        false,
        None,
    )
}

/// Like [`parse_style_tree_with_media`] with an explicit [`SupplyBudget`].
/// `SupplyBudget::CANONICAL` is byte-identical to the other entries; any other
/// value is a named candidate profile the caller must record. Input bytes
/// remain governed by `limits` alone.
#[must_use]
pub fn parse_style_tree_with_supply(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
    media: Option<MediaContext>,
    budget: SupplyBudget,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes, sheets, limits, media, budget, false, false, false, false, None,
    )
}

/// CANDIDATE, opt-in, unadopted: like [`parse_style_tree_with_supply`], but an
/// oversize `link[href]` whose value starts with `data:text/css` is retained whole
/// (not counted as a dropped attribute) up to the encoded size implied by
/// `budget.max_sheet_bytes`, and `budget.max_linked_sheets` such values per
/// document, so the engine's own strict decoder admits or refuses it. Every
/// other dropped attribute is still counted and refused.
#[must_use]
pub fn parse_style_tree_with_inline_sheets(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
    media: Option<MediaContext>,
    budget: SupplyBudget,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes, sheets, limits, media, budget, true, false, false, false, None,
    )
}

/// CANDIDATE, opt-in, unadopted: [`parse_style_tree_with_inline_sheets`] plus an
/// extended selector grammar: attribute selectors (`[a]`, `=`, `~=`, `|=`, `^=`,
/// `$=`, `*=`, no case flags), `*`, `:first-child`, `:last-child`, `:only-child`,
/// and the `+` and `~` combinators, plus a bounded `:nth-child(An+B)` (argument
/// <= 32 chars, |A| <= 1024, |B| <= 65535, attached signs only; comments,
/// escaped `n` and the `of S` form refused as an unsupported subset), matched
/// with a backtracking, fuel-bounded matcher. Other state and functional
/// pseudo-classes, pseudo-elements, namespaces and escapes are still refused
/// as `CssSelector`; fuel exhaustion refuses as `MatchWork` and truncates the
/// tree, while an oversize sibling list (> 4096) refuses only that rule for
/// that element: one `MatchWork` diagnostic per refused rule-element pair,
/// every other rule and element still evaluated, and the DELIVERED tree is
/// flagged truncated so the all-or-nothing painter refuses the page - a
/// dropped potential match is never presented as complete. The default
/// entries are unchanged.
///
/// The same entry also accepts `font-size: <n>em` (EXPERIMENTAL, UNREVIEWED
/// candidate): the exact parent-relative multiplier the percentage path already
/// uses, up to four decimals, tried after `rem`. A result that is not a whole
/// pixel still refuses as `ComputedValue`; nothing is rounded. This is bundled
/// with the selector grammar, not an em-only opt-in, and it is not adopted:
/// every other entry, including the default, keeps `em` unrecognized there.
#[must_use]
pub fn parse_style_tree_with_inline_sheets_and_selectors(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
    media: Option<MediaContext>,
    budget: SupplyBudget,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes, sheets, limits, media, budget, true, true, false, false, None,
    )
}

/// CANDIDATE, opt-in, unadopted (EXPERIMENTAL, private research): like
/// [`parse_style_tree_with_inline_sheets_and_selectors`], plus the state
/// pseudo-classes `:link`, `:hover` and `:active` under explicit caller
/// [`SnapshotState`] guarantees. A guarantee is MANDATORY per state family
/// (there is no absent-state path) and the two families are independently
/// scoped: `HistoryState::FreshNoHistory` (the caller's written assertion of
/// a fresh profile / no prior navigation) enrolls only `:link`;
/// `InputState::QuiescentCapture` (the caller's written assertion that no
/// pointer is over the viewport, no key is held or activating, and no touch
/// contact exists) enrolls only `:hover`/`:active`. Under FreshNoHistory,
/// `:link` matches an HTML-namespace `a`/`area` that has an `href` attribute
/// (any value, including empty); SVG `a` is narrowed out in v1 because the
/// DOM flattens `xlink:href` to the local name `href`. Under
/// QuiescentCapture, `:hover` and `:active` never match - the correct CSS
/// semantic for a quiescent capture. `FocusState::UnfocusedInitial` (the
/// caller's written assertion of a fragment-free capture URL, fresh profile,
/// no history navigation or bfcache restore, no input dispatch and no
/// programmatic focus, with the engine additionally refusing the guarantee
/// when the DOM carries `autofocus`) enrolls only the focus family, which
/// then never matches - the correct CSS semantic for an unfocused initial
/// capture. `:visited` and every other pseudo-class stay refused, as does any state pseudo-class at every
/// other entry (fail-closed selector refusal). Default entries are
/// byte-unchanged.
#[must_use]
pub fn parse_style_tree_with_state(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
    media: Option<MediaContext>,
    budget: SupplyBudget,
    snapshot: &SnapshotState,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes,
        sheets,
        limits,
        media,
        budget,
        true,
        true,
        false,
        false,
        Some(snapshot),
    )
}

/// CANDIDATE, opt-in, unadopted (EXPERIMENTAL): [`parse_style_tree`] plus acceptance of
/// `text-decoration: none|underline` (nothing else, no longhands). Every other entry,
/// including [`parse_style_tree`], keeps `text-decoration` unrecognized exactly as in the
/// reviewed r1 base; painting still needs `BoxTree::from_style_with_text_decoration`.
#[must_use]
pub fn parse_style_tree_with_text_decoration(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes,
        sheets,
        limits,
        None,
        SupplyBudget::CANONICAL,
        false,
        false,
        true,
        false,
        None,
    )
}

/// CANDIDATE, opt-in, unadopted (EXPERIMENTAL): [`parse_style_tree_with_supply`] plus the
/// `text-decoration: none|underline` acceptance of [`parse_style_tree_with_text_decoration`].
/// The caller supplies the media context and supply budget exactly as for
/// `parse_style_tree_with_supply`; no limit or default budget changes.
#[must_use]
pub fn parse_style_tree_with_supply_and_text_decoration(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
    media: Option<MediaContext>,
    budget: SupplyBudget,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes, sheets, limits, media, budget, false, false, true, false, None,
    )
}

/// CANDIDATE, opt-in, unadopted (EXPERIMENTAL, private research): like
/// [`parse_style_tree_with_supply`] but retains exactly `font-family: sans-serif` and
/// `font-family: "GDS Transport", arial, sans-serif` (see `pinned_family_list`) as the explicit
/// generic SansSerif instead of refusing the whole render. Every other family value still
/// refuses. Painting a tree from this entry needs
/// [`css_text_page::paint_direct_text_blocks_family_bound`]. The other TEXT painters refuse any
/// text or run that carries a family (`collect_inline_runs`); trees whose family sits only on
/// boxes without text (for example a family-only div) are NOT refused by them, and background
/// painters never inspect family.
#[must_use]
pub fn parse_style_tree_with_pinned_font_family(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
    media: Option<MediaContext>,
    budget: SupplyBudget,
) -> style_tree::StyleTree {
    parse_style_tree_in(
        bytes, sheets, limits, media, budget, false, false, false, true, None,
    )
}

/// Host-side census of the external stylesheets the parse will ask the caller to supply through `sheets`, resolved
/// against `page` (and the first `<base href>`). Additive: it parses with the same DOM sink and byte ceiling as
/// [`parse_style_tree`] but runs no cascade, and changes no existing entry. `complete` is false when the input or DOM
/// was cut short or attributes were dropped (then the list is not an inventory). The caller must supply exactly
/// `links.len()` sheets, in this order, or the parse refuses; fetching them is the caller's job.
#[must_use]
pub fn host_stylesheet_links(
    bytes: &[u8],
    page: &Url,
    limits: &Limits,
    media: Option<MediaContext>,
) -> style_tree::HostSheetLinks {
    let input = &bytes[..bytes.len().min(limits.max_input_bytes)];
    let mut warnings = Vec::new();
    let text = decode(input, &mut warnings);
    let sink = dom::Sink::new(limits.max_text_bytes);
    let options = html5ever::ParseOpts {
        tree_builder: html5ever::tree_builder::TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parser = html5ever::parse_document(sink, options);
    let mut rest: &str = &text;
    let mut cut = bytes.len() > limits.max_input_bytes;
    while !rest.is_empty() {
        let mut end = rest.len().min(16 * 1024);
        while !rest.is_char_boundary(end) {
            end += 1;
        }
        parser.process(rest[..end].into());
        rest = &rest[end..];
        if parser.tokenizer.sink.sink.len() > limits.max_nodes {
            cut = true;
            break;
        }
    }
    let sink = parser.finish();
    let dropped = sink
        .dropped_attributes
        .get()
        .saturating_add(sink.merged_attributes_dropped.get())
        > 0;
    let nodes = sink.into_nodes();
    style_tree::host_sheet_links_in(&nodes, media, page, !cut && !dropped)
}

/// Per-href and per-document retention caps, derived from the sheet budget and sheet count.
fn inline_href_caps(budget: &SupplyBudget) -> Option<(usize, usize)> {
    let per = data_stylesheet::max_href_len(budget.max_sheet_bytes)?;
    Some((per, per.checked_mul(budget.max_linked_sheets)?))
}

fn parse_style_tree_in(
    bytes: &[u8],
    sheets: &[Vec<u8>],
    limits: &Limits,
    media: Option<MediaContext>,
    budget: SupplyBudget,
    inline_sheets: bool,
    ext_selectors: bool,
    text_deco: bool,
    family_bind: bool,
    snapshot: Option<&SnapshotState>,
) -> style_tree::StyleTree {
    let input = &bytes[..bytes.len().min(limits.max_input_bytes)];
    let mut warnings = Vec::new();
    let text = decode(input, &mut warnings);
    let sink = dom::Sink::new(limits.max_text_bytes);
    if inline_sheets {
        if let Some((per, aggregate)) = inline_href_caps(&budget) {
            sink.set_inline_sheet_href_caps(per, aggregate);
        }
        // Barrier-B: ext parse entries opt in to the resource-reference
        // retention budget; the default entry stays (0, 0) byte-identical.
        sink.set_resource_attr_caps(dom::RESOURCE_ATTR_PER_VALUE, dom::RESOURCE_ATTR_AGGREGATE);
    }
    let options = html5ever::ParseOpts {
        tree_builder: html5ever::tree_builder::TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut parser = html5ever::parse_document(sink, options);
    let mut rest: &str = &text;
    let mut input_truncated = bytes.len() > limits.max_input_bytes;
    let mut diagnostics = style_diagnostics::StyleDiagnostics::default();
    diagnostics.input_bytes = bytes.len();
    if input_truncated {
        diagnostics.record(style_diagnostics::RefusalReason::InputBytes);
    }
    while !rest.is_empty() {
        let mut end = rest.len().min(16 * 1024);
        while !rest.is_char_boundary(end) {
            end += 1;
        }
        parser.process(rest[..end].into());
        rest = &rest[end..];
        if parser.tokenizer.sink.sink.len() > limits.max_nodes {
            input_truncated = true;
            diagnostics.record(style_diagnostics::RefusalReason::DomNodes);
            break;
        }
    }
    let sink = parser.finish();
    diagnostics.dropped_attributes = sink
        .dropped_attributes
        .get()
        .saturating_add(sink.merged_attributes_dropped.get());
    diagnostics.dropped_text = sink.dropped_text.get();
    diagnostics.retained_inline_sheet_href_bytes = sink.retained_oversize_href_bytes();
    diagnostics.retained_resource_attr_bytes = sink.retained_resource_attr_bytes();
    if diagnostics.dropped_attributes > 0 {
        diagnostics.record(style_diagnostics::RefusalReason::DroppedAttributes);
    }
    if diagnostics.dropped_text {
        diagnostics.record(style_diagnostics::RefusalReason::DroppedText);
    }
    let dropped = sink.dropped_text.get() || diagnostics.dropped_attributes > 0;
    let quirks = sink.quirks_mode.get();
    let nodes = sink.into_nodes();
    diagnostics.dom_nodes = nodes.len();
    if warnings.contains(&Warning::NotUtf8)
        || std::str::from_utf8(input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input)).is_err()
    {
        diagnostics.record(style_diagnostics::RefusalReason::Encoding);
    }
    if quirks != html5ever::interface::QuirksMode::NoQuirks {
        diagnostics.record(style_diagnostics::RefusalReason::Quirks);
    }
    let mut tree = style_tree::StyleTree::from_dom_with(
        &nodes,
        sheets,
        media,
        budget,
        ext_selectors,
        text_deco,
        family_bind,
        snapshot,
    );
    // Exact host-supply handshake, only on the fully parsed and fully claimed
    // path: the input was not cut short (a truncated DOM is a partial link
    // census, not an inventory) and the engine's own link walk met no
    // refusal. An unclaimed extra supplied sheet must not be silently
    // accepted. Missing sheets already refuse in the walk itself.
    if !input_truncated
        && tree
            .host_sheets_claimed
            .is_some_and(|claimed| sheets.len() != claimed)
    {
        tree.diagnostics
            .record(style_diagnostics::RefusalReason::StylesheetCount);
        tree.unsupported = true;
    }
    tree.diagnostics.prepend(&diagnostics);
    tree.truncated |= input_truncated || dropped;
    tree.quirks = quirks != html5ever::interface::QuirksMode::NoQuirks;
    // Reader mode can show legacy Windows-1252 text and a cut UTF-8 tail,
    // but the visual path cannot claim exact CSS selectors/values after an
    // unverified decode. Keep its retained tree for diagnostics and refuse
    // painting until character-encoding handling is explicit.
    tree.unsupported |= warnings.contains(&Warning::NotUtf8)
        || std::str::from_utf8(input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input)).is_err();
    tree
}

/// Bytes to text. UTF-8 (with or without a byte-order mark) is read as is.
/// Anything else is read as Windows-1252, which is what browsers fall back to
/// for Western pages and which cannot fail.
fn decode(bytes: &[u8], warnings: &mut Vec<Warning>) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(error) if error.error_len().is_none() && bytes.len() - error.valid_up_to() < 4 => {
            // A multi-byte character cut by a byte ceiling.
            String::from_utf8_lossy(&bytes[..error.valid_up_to()]).into_owned()
        }
        Err(_) => {
            warnings.push(Warning::NotUtf8);
            bytes.iter().map(|&b| windows_1252(b)).collect()
        }
    }
}

fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}',
        '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}',
        '\u{2014}', '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}',
        '\u{178}',
    ];
    match byte {
        0x80..=0x9F => HIGH[usize::from(byte - 0x80)],
        other => char::from(other),
    }
}

pub use box_tree::shadow_counts;

#[cfg(test)]
mod pinned_family_generated {
    //! Candidate-private: generated `::after` boxes under the pinned family. The pinned public entry
    //! has no extended selectors, so this crate-internal combination (extended selectors + pinned
    //! family) is reachable only from here; it adds no public API.
    use super::*;
    use crate::box_tree::{BoxKind, BoxTree};
    use crate::css_text_page::{paint_direct_text_blocks_family_bound, FamilyProfile, FontProvider};
    use crate::inline_lines::GlyphBitmap;
    struct Face;
    impl FontProvider for Face {
        fn face_sha256_hex(&self) -> Option<String> { Some(FamilyProfile::Chrome156HostLiberation2Sans.regular_sha256_hex().into()) }
        fn bold_face_sha256_hex(&self) -> Option<String> { Some(FamilyProfile::Chrome156HostLiberation2Sans.bold_sha256_hex().into()) }
        fn has_glyph(&self, _: char, _: bool) -> Option<bool> { Some(true) }
        fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(5) }
        fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(20) }
        fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(15) }
        fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> {
            Some(GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] })
        }
    }
    fn run(css: &str, body: &str) -> (bool, Option<bool>, usize) {
        let html = format!("<!doctype html><style>html,body,div,p{{margin:0}}div{{line-height:20px}}{css}</style>{body}");
        let s = parse_style_tree_in(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
            true,
            true,
            false,
            true,
            None,
        );
        if s.unsupported {
            return (true, None, 0);
        }
        let t = BoxTree::from_style(&s);
        let generated_text = t.boxes.iter().filter(|b| b.kind == BoxKind::Text && b.text.as_deref() == Some("x")).count();
        (false, Some(paint_direct_text_blocks_family_bound(&t, 200, 100, &Face, FamilyProfile::Chrome156HostLiberation2Sans).is_ok()), generated_text)
    }
    const F: &str = "\"GDS Transport\", arial, sans-serif";
    #[test]
    fn generated_after_text_follows_the_same_gate() {
        // Inherited family on the generated box (originating div has the pinned family).
        let (refused, painted, _) = run(&format!("div{{font-family:{F}}} div::after{{content:\"x\"}}"), "<div>ab</div>");
        println!("inherit+content x: refused_at_intake={refused} painted={painted:?}");
        // Generated text with a family only on the ::after rule, originating text unbound.
        let (refused2, painted2, _) = run(&format!("div::after{{content:\"x\";font-family:{F}}}"), "<div>ab</div>");
        println!("after-only family: refused_at_intake={refused2} painted={painted2:?}");
        // Non-ASCII generated text.
        let (refused3, painted3, _) = run(&format!("div{{font-family:{F}}} div::after{{content:\"\u{e9}\"}}"), "<div>ab</div>");
        println!("non-ascii content: refused_at_intake={refused3} painted={painted3:?}");
        // Empty content (GOV shape) with the pinned family: no text leaf.
        let (refused4, painted4, _) = run(&format!("div{{font-family:{F}}} div::after{{content:\"\"}}"), "<div>ab</div>");
        println!("empty content: refused_at_intake={refused4} painted={painted4:?}");
        // Controls: the same generated text is refused at intake with NO family at all, so the
        // intake refusal of content:"x" is independent of the family binding (generated TEXT is
        // unreachable here); and the empty-content page's bound refusal is compared to the
        // family-less twin through the ordinary painter.
        let html = "<!doctype html><style>html,body,div,p{margin:0}div{line-height:20px} div::after{content:\"x\"}</style><div>ab</div>";
        let s = parse_style_tree_in(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
            true,
            true,
            false,
            false,
            None,
        );
        println!(
            "content x, no family, family_bind=false: unsupported={}",
            s.unsupported
        );
        assert!(
            s.unsupported,
            "generated text content refuses independent of the family"
        );
        let html = "<!doctype html><style>html,body,div,p{margin:0}div{line-height:20px} div::after{content:\"\"}</style><div>ab</div>";
        let s = parse_style_tree_in(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
            true,
            true,
            false,
            false,
            None,
        );
        let ordinary = crate::css_text_page::paint_direct_text_blocks(
            &BoxTree::from_style(&s),
            200,
            100,
            &Face,
        )
        .is_ok();
        println!("empty content, family-less twin, ordinary painter ok={ordinary}");
        assert_eq!(painted4 == Some(true), ordinary, "family binding must not change the empty-content outcome");
        assert!(refused && refused2 && refused3, "generated text refuses at intake");
        assert_ne!(painted3, Some(true), "non-ASCII generated text must not paint");
        assert_ne!(painted2, Some(true), "unbound originating text must not paint");
    }
}
