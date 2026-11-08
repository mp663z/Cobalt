//! Content-free inventory of the retained semantic-parse arena.
//!
//! Includes detached, repaired, synthetic and template nodes. This is neither
//! raw-token inventory nor source identity. No field authorizes CSS publication.
use crate::dom::{Data, Node, Sink};
use html5ever::interface::QuirksMode;

/// Explicit passive HTML admission classification. All other HTML names either
/// have a refusing profile class below or count as unknown, even if HTML knows
/// their name. Inclusion here does not prove style or conversion completeness.
pub const PASSIVE_HTML_ALLOWLIST: &[&str] = &[
    "html",
    "head",
    "body",
    "title",
    "div",
    "span",
    "p",
    "main",
    "article",
    "section",
    "header",
    "footer",
    "nav",
    "aside",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hgroup",
    "address",
    "blockquote",
    "pre",
    "hr",
    "br",
    "wbr",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "figure",
    "figcaption",
    "table",
    "caption",
    "colgroup",
    "col",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "b",
    "i",
    "em",
    "strong",
    "small",
    "s",
    "u",
    "mark",
    "sub",
    "sup",
    "code",
    "kbd",
    "samp",
    "var",
    "abbr",
    "cite",
    "q",
    "dfn",
    "time",
    "data",
    "bdi",
    "bdo",
    "ruby",
    "rt",
    "rp",
    "ins",
    "del",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParsedQuirks {
    NoQuirks,
    LimitedQuirks,
    Quirks,
}

/// Saturating element counts; interaction attributes count once per element.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceProfile {
    pub scripts: usize,
    pub forms: usize,
    pub links: usize,
    pub details_summary: usize,
    pub image_maps: usize,
    pub frames: usize,
    pub dialogs: usize,
    pub images: usize,
    pub media: usize,
    pub svg_math: usize,
    pub foreign_other: usize,
    pub unknown_custom: usize,
    pub head_affecting: usize,
    pub template: usize,
    pub interaction_attrs: usize,
}

// Independent evidence flags are intentionally not one combined admission state.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedSourceFacts {
    pub scan_completed: bool,
    /// Input/retention loss only. Does NOT subsume namespace, quirks, parser
    /// recovery, conversion warnings, CSS proof or source binding.
    pub source_loss_free: bool,
    pub input_original_bytes: usize,
    pub actual_arena_nodes: usize,
    pub input_clipped: bool,
    pub node_stop: bool,
    pub dropped_text: bool,
    pub dropped_attributes: usize,
    pub merged_attributes_dropped: usize,
    pub attr_namespace_discarded: bool,
    pub not_utf8: bool,
    pub parse_errors: usize,
    pub quirks: ParsedQuirks,
    pub profile: SourceProfile,
}

impl ParsedSourceFacts {
    pub(crate) fn from_sink(
        sink: &Sink,
        original: usize,
        clipped: bool,
        node_stop: bool,
        not_utf8: bool,
    ) -> Self {
        let dropped_attributes = sink.dropped_attributes.get();
        let merged_attributes_dropped = sink.merged_attributes_dropped.get();
        let dropped_text = sink.dropped_text.get();
        Self {
            scan_completed: false,
            source_loss_free: !clipped
                && !node_stop
                && !not_utf8
                && !dropped_text
                && dropped_attributes == 0
                && merged_attributes_dropped == 0,
            input_original_bytes: original,
            actual_arena_nodes: 0,
            input_clipped: clipped,
            node_stop,
            dropped_text,
            dropped_attributes,
            merged_attributes_dropped,
            attr_namespace_discarded: sink.attr_namespace_discarded.get(),
            not_utf8,
            parse_errors: sink.errors.get(),
            quirks: match sink.quirks_mode.get() {
                QuirksMode::NoQuirks => ParsedQuirks::NoQuirks,
                QuirksMode::LimitedQuirks => ParsedQuirks::LimitedQuirks,
                QuirksMode::Quirks => ParsedQuirks::Quirks,
            },
            profile: SourceProfile::default(),
        }
    }

    pub(crate) fn scan(&mut self, nodes: &[Node]) {
        self.actual_arena_nodes = nodes.len();
        for node in nodes {
            let Data::Element { name, attrs, .. } = &node.data else {
                continue;
            };
            let p = &mut self.profile;
            if attrs.iter().any(|(name, _)| {
                let name: &str = name;
                matches!(name, "contenteditable" | "tabindex") || name.starts_with("on")
            }) {
                p.interaction_attrs = p.interaction_attrs.saturating_add(1);
            }
            let counter = if name.ns == html5ever::ns!(svg) || name.ns == html5ever::ns!(mathml) {
                &mut p.svg_math
            } else if name.ns != html5ever::ns!(html) {
                &mut p.foreign_other
            } else {
                match name.local.as_ref() {
                    "script" => &mut p.scripts,
                    "form" | "input" | "button" | "select" | "textarea" | "datalist" => {
                        &mut p.forms
                    }
                    "a" | "area" => &mut p.links,
                    "details" | "summary" => &mut p.details_summary,
                    "map" => &mut p.image_maps,
                    "iframe" | "frame" | "frameset" | "object" | "embed" | "applet" | "noembed"
                    | "noframes" => &mut p.frames,
                    "dialog" => &mut p.dialogs,
                    "img" | "picture" | "source" => &mut p.images,
                    "video" | "audio" | "track" | "canvas" => &mut p.media,
                    "link" | "meta" | "style" => &mut p.head_affecting,
                    "template" => &mut p.template,
                    local if PASSIVE_HTML_ALLOWLIST.contains(&local) => continue,
                    _ => &mut p.unknown_custom,
                }
            };
            *counter = counter.saturating_add(1);
        }
        self.scan_completed = true;
    }
}
