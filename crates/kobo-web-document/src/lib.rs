//! Untrusted HTML into a small, bounded document model.
//!
//! The model ([`Document`]) is the boundary between the web and the panel.
//! Parser output is not trusted, layout never sees a DOM, anything the model
//! cannot say disappears or degrades to text, and every list in it has a
//! ceiling set by [`Limits`]. A page that reaches a ceiling is still shown,
//! marked with a [`Warning`] saying what was left out.

mod convert;
mod dom;
mod host;
pub mod url;

use html5ever::tendril::TendrilSink;

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
    Submit {
        name: Option<String>,
        value: String,
    },
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
}

impl Document {
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
    if sink.dropped_attributes.get() > 0 {
        warnings.push(Warning::AttributesDropped);
    }
    let parse_errors = sink.errors.get();
    let nodes = sink.into_nodes();
    let mut document = convert::convert(&nodes, url, limits, warnings);
    document.parse_errors = parse_errors;
    document
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
