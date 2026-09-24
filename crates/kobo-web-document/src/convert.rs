//! From the parsed tree to the document model.
//!
//! One pass over the tree in document order. Block elements end the paragraph
//! being gathered and start their own; inline elements nest inside it. What
//! the model has no word for is either dropped with its contents (scripts,
//! frames, plugins, anything hidden) or unwrapped so its text is kept.

use html5ever::{local_name, ns};

use crate::dom::{Data, Handle, Node, DOCUMENT};
use crate::{
    Block, Field, Form, ImageRef, Inline, Limits, Link, Method, Table, Url, UrlError, Warning,
};

/// Dropped with everything inside them.
const DROPPED: &[&str] = &[
    "script", "style", "template", "iframe", "frame", "frameset", "object", "embed", "applet",
    "canvas", "video", "audio", "source", "track", "svg", "math", "map", "select", "button",
    "textarea", "datalist", "noembed", "noframes", "dialog", "head", "link", "meta",
];

/// End the paragraph being gathered and hold their own content.
const CONTAINERS: &[&str] = &[
    "div",
    "section",
    "article",
    "main",
    "header",
    "footer",
    "aside",
    "nav",
    "figure",
    "figcaption",
    "dl",
    "dt",
    "dd",
    "center",
    "address",
    "details",
    "summary",
    "fieldset",
    "legend",
    "hgroup",
    "body",
    "html",
    "li",
    "caption",
    "noscript",
    "search",
    "menu",
];

struct Converter<'a> {
    nodes: &'a [Node],
    base: Url,
    limits: &'a Limits,
    links: Vec<Link>,
    blocks_made: usize,
    images: usize,
    active_removed: usize,
    unsupported_links: usize,
    warnings: Vec<Warning>,
    pending_anchor: Option<String>,
    title: Option<String>,
}

/// Paragraph content being gathered at one block level.
#[derive(Default)]
struct Gather {
    inlines: Vec<Inline>,
    /// Images met inside the paragraph, placed after it.
    images: Vec<ImageRef>,
}

pub fn convert(
    nodes: &[Node],
    url: &Url,
    limits: &Limits,
    warnings: Vec<Warning>,
) -> crate::Document {
    let mut converter = Converter {
        nodes,
        base: url.clone(),
        limits,
        links: Vec::new(),
        blocks_made: 0,
        images: 0,
        active_removed: 0,
        unsupported_links: 0,
        warnings,
        pending_anchor: None,
        title: None,
    };
    converter.read_head(DOCUMENT, 0);
    let mut blocks = Vec::new();
    let mut gather = Gather::default();
    converter.blocks_in(DOCUMENT, 0, &mut blocks, &mut gather);
    converter.flush(&mut blocks, &mut gather);
    if converter.active_removed > 0 {
        converter
            .warnings
            .push(Warning::ActiveContentRemoved(converter.active_removed));
    }
    if converter.unsupported_links > 0 {
        converter
            .warnings
            .push(Warning::UnsupportedLinks(converter.unsupported_links));
    }
    converter.warnings.sort();
    converter.warnings.dedup();
    crate::Document {
        title: converter.title,
        base: Some(converter.base),
        blocks,
        links: converter.links,
        warnings: converter.warnings,
        parse_errors: 0,
    }
}

impl Converter<'_> {
    fn tag(&self, handle: Handle) -> Option<&str> {
        match &self.nodes[handle].data {
            Data::Element { name, .. } if name.ns == ns!(html) => Some(&name.local),
            Data::Element { name, .. }
                if name.ns == ns!(svg) && name.local == local_name!("svg") =>
            {
                Some("svg")
            }
            Data::Element { name, .. }
                if name.ns == ns!(mathml) && name.local == local_name!("math") =>
            {
                Some("math")
            }
            _ => None,
        }
    }

    fn attr(&self, handle: Handle, wanted: &str) -> Option<&str> {
        match &self.nodes[handle].data {
            Data::Element { attrs, .. } => attrs
                .iter()
                .find(|(name, _)| &**name == wanted)
                .map(|(_, value)| value.as_str()),
            _ => None,
        }
    }

    /// Reads `<title>` and `<base href>` wherever the parser put them.
    fn read_head(&mut self, handle: Handle, depth: usize) {
        if depth > 8 {
            return;
        }
        for &child in &self.nodes[handle].children {
            match self.tag(child) {
                Some("title") if self.title.is_none() => {
                    let text = collapse(&self.text_of(child));
                    if !text.is_empty() {
                        self.title = Some(text);
                    }
                }
                Some("base") => {
                    if let Some(href) = self.attr(child, "href") {
                        if let Ok(base) = self.base.join(href) {
                            self.base = base;
                        }
                    }
                }
                Some("head" | "html") => self.read_head(child, depth + 1),
                _ => {}
            }
        }
    }

    fn hidden(&self, handle: Handle) -> bool {
        if self.attr(handle, "hidden").is_some() || self.attr(handle, "aria-hidden") == Some("true")
        {
            return true;
        }
        self.attr(handle, "style").is_some_and(|style| {
            let style: String = style
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .to_ascii_lowercase();
            style.contains("display:none") || style.contains("visibility:hidden")
        })
    }

    /// All text under a node, without structure. Iterative, so depth is free.
    fn text_of(&self, handle: Handle) -> String {
        let mut out = String::new();
        let mut stack = vec![handle];
        while let Some(node) = stack.pop() {
            match &self.nodes[node].data {
                Data::Text(text) => out.push_str(text),
                Data::Element { .. } => {
                    if self.tag(node).is_some_and(|tag| DROPPED.contains(&tag)) {
                        continue;
                    }
                    if self.tag(node) == Some("br") {
                        out.push('\n');
                    }
                    stack.extend(self.nodes[node].children.iter().rev());
                }
                _ => {}
            }
        }
        out
    }

    fn take_anchor(&mut self, handle: Handle) -> Option<String> {
        self.attr(handle, "id")
            .or_else(|| self.attr(handle, "name"))
            .map(str::to_owned)
            .or_else(|| self.pending_anchor.take())
    }

    fn push_block(&mut self, blocks: &mut Vec<Block>, block: Block) {
        if self.blocks_made >= self.limits.max_blocks {
            self.warnings.push(Warning::TooManyBlocks);
            return;
        }
        self.blocks_made += 1;
        blocks.push(block);
    }

    /// Ends the paragraph being gathered.
    fn flush(&mut self, blocks: &mut Vec<Block>, gather: &mut Gather) {
        let content = trim_inlines(std::mem::take(&mut gather.inlines));
        if !content.is_empty() {
            let anchor = self.pending_anchor.take();
            self.push_block(blocks, Block::Paragraph { content, anchor });
        }
        for image in std::mem::take(&mut gather.images) {
            self.push_block(blocks, Block::Image(image));
        }
    }

    /// Reads the children of a block-level node.
    fn blocks_in(
        &mut self,
        handle: Handle,
        depth: usize,
        blocks: &mut Vec<Block>,
        gather: &mut Gather,
    ) {
        for index in 0..self.nodes[handle].children.len() {
            let child = self.nodes[handle].children[index];
            self.block(child, depth + 1, blocks, gather);
        }
    }

    /// One match over every block element the model knows, kept whole so
    /// the mapping from tag to block reads in one place.
    #[allow(clippy::too_many_lines)]
    fn block(
        &mut self,
        handle: Handle,
        depth: usize,
        blocks: &mut Vec<Block>,
        gather: &mut Gather,
    ) {
        if let Data::Text(text) = &self.nodes[handle].data {
            push_text(&mut gather.inlines, text);
            return;
        }
        let Some(tag) = self.tag(handle) else { return };
        let tag = tag.to_owned();
        if DROPPED.contains(&tag.as_str()) {
            if matches!(
                tag.as_str(),
                "script" | "iframe" | "object" | "embed" | "applet" | "frame"
            ) {
                self.active_removed += 1;
            }
            return;
        }
        if self.hidden(handle) {
            return;
        }
        if depth > self.limits.max_depth {
            self.warnings.push(Warning::TooDeep);
            push_text(&mut gather.inlines, &self.text_of(handle));
            return;
        }
        match tag.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush(blocks, gather);
                let level = tag.as_bytes()[1] - b'0';
                let mut inner = Gather::default();
                self.inlines_in(handle, depth, &mut inner);
                let content = trim_inlines(inner.inlines);
                let anchor = self.take_anchor(handle);
                if !content.is_empty() {
                    self.push_block(
                        blocks,
                        Block::Heading {
                            level,
                            content,
                            anchor,
                        },
                    );
                }
                for image in inner.images {
                    self.push_block(blocks, Block::Image(image));
                }
            }
            "p" => {
                self.flush(blocks, gather);
                if let Some(anchor) = self.attr(handle, "id") {
                    self.pending_anchor = Some(anchor.to_owned());
                }
                self.blocks_in(handle, depth, blocks, gather);
                self.flush(blocks, gather);
            }
            "ul" | "ol" => {
                self.flush(blocks, gather);
                let ordered = tag == "ol";
                let start = self
                    .attr(handle, "start")
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(1);
                let mut items = Vec::new();
                for index in 0..self.nodes[handle].children.len() {
                    let child = self.nodes[handle].children[index];
                    if matches!(self.nodes[child].data, Data::Text(_)) || self.hidden(child) {
                        continue;
                    }
                    let mut item = Vec::new();
                    let mut inner = Gather::default();
                    if self.tag(child) == Some("li") {
                        self.blocks_in(child, depth + 1, &mut item, &mut inner);
                    } else {
                        self.block(child, depth + 1, &mut item, &mut inner);
                    }
                    self.flush(&mut item, &mut inner);
                    if !item.is_empty() {
                        items.push(item);
                    }
                }
                if !items.is_empty() {
                    self.push_block(
                        blocks,
                        Block::List {
                            ordered,
                            start,
                            items,
                        },
                    );
                }
            }
            "blockquote" => {
                self.flush(blocks, gather);
                let mut inner = Vec::new();
                let mut inner_gather = Gather::default();
                self.blocks_in(handle, depth, &mut inner, &mut inner_gather);
                self.flush(&mut inner, &mut inner_gather);
                if !inner.is_empty() {
                    self.push_block(blocks, Block::Quote(inner));
                }
            }
            "pre" | "listing" | "xmp" | "plaintext" => {
                self.flush(blocks, gather);
                let text = self.text_of(handle);
                let text = text
                    .strip_prefix('\n')
                    .unwrap_or(&text)
                    .trim_end()
                    .to_owned();
                if !text.is_empty() {
                    self.push_block(blocks, Block::Preformatted(text));
                }
            }
            "hr" => {
                self.flush(blocks, gather);
                self.push_block(blocks, Block::Rule);
            }
            "img" => {
                if let Some(image) = self.image(handle, None) {
                    gather.images.push(image);
                } else if let Some(alt) = self.attr(handle, "alt") {
                    push_text(&mut gather.inlines, alt);
                }
            }
            "table" => {
                self.flush(blocks, gather);
                if self.is_data_table(handle) {
                    let table = self.table(handle, depth);
                    if !table.rows.is_empty() {
                        self.push_block(blocks, Block::Table(table));
                    }
                } else {
                    self.layout_table(handle, depth, blocks, gather);
                }
            }
            "form" => {
                self.flush(blocks, gather);
                self.blocks_in(handle, depth, blocks, gather);
                self.flush(blocks, gather);
                if let Some(form) = self.form(handle) {
                    self.push_block(blocks, Block::Form(form));
                }
            }
            "br" => gather.inlines.push(Inline::LineBreak),
            tag if CONTAINERS.contains(&tag) => {
                self.flush(blocks, gather);
                if let Some(anchor) = self.attr(handle, "id") {
                    self.pending_anchor = Some(anchor.to_owned());
                }
                self.blocks_in(handle, depth, blocks, gather);
                self.flush(blocks, gather);
            }
            _ => self.inline(handle, depth, gather),
        }
    }

    fn inlines_in(&mut self, handle: Handle, depth: usize, gather: &mut Gather) {
        for index in 0..self.nodes[handle].children.len() {
            let child = self.nodes[handle].children[index];
            match &self.nodes[child].data {
                Data::Text(text) => push_text(&mut gather.inlines, text),
                Data::Element { .. } => self.inline(child, depth + 1, gather),
                _ => {}
            }
        }
    }

    /// One inline element. Block elements met here are unwrapped.
    fn inline(&mut self, handle: Handle, depth: usize, gather: &mut Gather) {
        let Some(tag) = self.tag(handle) else { return };
        let tag = tag.to_owned();
        if DROPPED.contains(&tag.as_str()) {
            if matches!(
                tag.as_str(),
                "script" | "iframe" | "object" | "embed" | "applet" | "frame"
            ) {
                self.active_removed += 1;
            }
            return;
        }
        if self.hidden(handle) {
            return;
        }
        if depth > self.limits.max_depth {
            self.warnings.push(Warning::TooDeep);
            push_text(&mut gather.inlines, &self.text_of(handle));
            return;
        }
        match tag.as_str() {
            "em" | "i" | "cite" | "dfn" | "var" => {
                let inner = self.nested(handle, depth, gather);
                if !inner.is_empty() {
                    gather.inlines.push(Inline::Emphasis(inner));
                }
            }
            "strong" | "b" => {
                let inner = self.nested(handle, depth, gather);
                if !inner.is_empty() {
                    gather.inlines.push(Inline::Strong(inner));
                }
            }
            "code" | "kbd" | "samp" | "tt" => {
                let text = collapse_keep_edges(&self.text_of(handle));
                if !text.trim().is_empty() {
                    gather.inlines.push(Inline::Code(text.trim().to_owned()));
                }
            }
            "br" => gather.inlines.push(Inline::LineBreak),
            "img" => {
                let link = None;
                if let Some(image) = self.image(handle, link) {
                    gather.images.push(image);
                } else if let Some(alt) = self.attr(handle, "alt") {
                    push_text(&mut gather.inlines, alt);
                }
            }
            "a" => self.link(handle, depth, gather),
            "input" => {
                if let Some(value) = self.attr(handle, "value") {
                    if matches!(self.attr(handle, "type"), Some("submit" | "button")) {
                        push_text(&mut gather.inlines, value);
                    }
                }
            }
            "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote"
            | "section" => {
                push_text(&mut gather.inlines, " ");
                self.inlines_in(handle, depth, gather);
                push_text(&mut gather.inlines, " ");
            }
            "td" | "th" => {
                self.inlines_in(handle, depth, gather);
                push_text(&mut gather.inlines, " ");
            }
            _ => self.inlines_in(handle, depth, gather),
        }
    }

    /// Children of an inline element, as their own list.
    fn nested(&mut self, handle: Handle, depth: usize, gather: &mut Gather) -> Vec<Inline> {
        let mut inner = Gather::default();
        self.inlines_in(handle, depth, &mut inner);
        gather.images.extend(inner.images);
        inner.inlines
    }

    fn link(&mut self, handle: Handle, depth: usize, gather: &mut Gather) {
        if let Some(anchor) = self
            .attr(handle, "id")
            .or_else(|| self.attr(handle, "name"))
        {
            if self.attr(handle, "href").is_none() && self.pending_anchor.is_none() {
                self.pending_anchor = Some(anchor.to_owned());
            }
        }
        let Some(href) = self.attr(handle, "href").map(str::to_owned) else {
            self.inlines_in(handle, depth, gather);
            return;
        };
        let target = self.base.join(&href);
        let mut inner = Gather::default();
        self.inlines_in(handle, depth, &mut inner);
        let label = trim_inlines(inner.inlines);
        match target {
            Ok(target) if self.links.len() < self.limits.max_links => {
                let index = self.links.len();
                let mut text = String::new();
                flatten(&label, &mut text);
                let mut text = collapse(&text);
                if text.is_empty() {
                    text = inner
                        .images
                        .first()
                        .map(|image| image.alt.clone())
                        .unwrap_or_default();
                }
                self.links.push(Link { target, text });
                for mut image in inner.images {
                    image.link = Some(index);
                    gather.images.push(image);
                }
                if !label.is_empty() {
                    gather.inlines.push(Inline::Link { index, label });
                }
            }
            Ok(_) => {
                self.warnings.push(Warning::TooManyLinks);
                gather.images.extend(inner.images);
                if !label.is_empty() {
                    gather.inlines.push(Inline::Inert(label));
                }
            }
            Err(error) => {
                if error == UrlError::UnsupportedScheme {
                    self.unsupported_links += 1;
                }
                gather.images.extend(inner.images);
                if !label.is_empty() {
                    gather.inlines.push(Inline::Inert(label));
                }
            }
        }
    }

    fn image(&mut self, handle: Handle, link: Option<usize>) -> Option<ImageRef> {
        let src = self
            .attr(handle, "src")
            .filter(|s| !s.trim().is_empty() && !s.trim_start().starts_with("data:"))
            .or_else(|| self.attr(handle, "data-src"))?;
        let src = self.base.join(src).ok()?;
        let dimension = |value: Option<&str>| {
            value.and_then(|v| v.trim().trim_end_matches("px").parse::<u32>().ok())
        };
        let width = dimension(self.attr(handle, "width"));
        let height = dimension(self.attr(handle, "height"));
        // Tracking pixels and spacers are not pictures.
        if width.is_some_and(|w| w <= 2) || height.is_some_and(|h| h <= 2) {
            return None;
        }
        if self.images >= self.limits.max_images {
            self.warnings.push(Warning::TooManyImages);
            return None;
        }
        self.images += 1;
        Some(ImageRef {
            src,
            alt: collapse(self.attr(handle, "alt").unwrap_or("")),
            width,
            height,
            link,
        })
    }

    fn rows(&self, handle: Handle, out: &mut Vec<Handle>, depth: usize) {
        if depth > 3 {
            return;
        }
        for &child in &self.nodes[handle].children {
            match self.tag(child) {
                Some("tr") => out.push(child),
                Some("thead" | "tbody" | "tfoot") => self.rows(child, out, depth + 1),
                _ => {}
            }
        }
    }

    fn cells(&self, row: Handle) -> Vec<Handle> {
        self.nodes[row]
            .children
            .iter()
            .copied()
            .filter(|&c| matches!(self.tag(c), Some("td" | "th")))
            .collect()
    }

    /// A table holding data rather than page layout: it says so with header
    /// cells or a caption, and has no table inside it.
    fn is_data_table(&self, handle: Handle) -> bool {
        let mut stack = vec![handle];
        let mut header = false;
        let mut first = true;
        while let Some(node) = stack.pop() {
            match self.tag(node) {
                Some("table") if !first => return false,
                Some("th" | "caption" | "thead") => header = true,
                _ => {}
            }
            first = false;
            stack.extend(self.nodes[node].children.iter().copied());
        }
        header
    }

    fn table(&mut self, handle: Handle, depth: usize) -> Table {
        let mut row_handles = Vec::new();
        self.rows(handle, &mut row_handles, 0);
        let caption = self.nodes[handle]
            .children
            .iter()
            .find(|&&c| self.tag(c) == Some("caption"))
            .map(|&c| collapse(&self.text_of(c)))
            .filter(|c| !c.is_empty());
        let mut clipped = false;
        if row_handles.len() > self.limits.max_table_rows {
            row_handles.truncate(self.limits.max_table_rows);
            clipped = true;
        }
        let header = row_handles
            .first()
            .is_some_and(|&row| self.cells(row).iter().all(|&c| self.tag(c) == Some("th")));
        let mut rows = Vec::new();
        for row in row_handles {
            let mut cells = self.cells(row);
            if cells.len() > self.limits.max_table_columns {
                cells.truncate(self.limits.max_table_columns);
                clipped = true;
            }
            let mut out = Vec::new();
            for cell in cells {
                let mut gather = Gather::default();
                self.inlines_in(cell, depth + 2, &mut gather);
                out.push(trim_inlines(gather.inlines));
            }
            if out.iter().any(|cell| !cell.is_empty()) {
                rows.push(out);
            }
        }
        if clipped {
            self.warnings.push(Warning::TableClipped);
        }
        Table {
            caption,
            rows,
            header,
            clipped,
        }
    }

    /// A table used to lay a page out: its cells are read in order as if
    /// they were sections.
    fn layout_table(
        &mut self,
        handle: Handle,
        depth: usize,
        blocks: &mut Vec<Block>,
        gather: &mut Gather,
    ) {
        let mut rows = Vec::new();
        self.rows(handle, &mut rows, 0);
        for row in rows {
            if self.hidden(row) {
                continue;
            }
            for cell in self.cells(row) {
                if self.hidden(cell) {
                    continue;
                }
                self.blocks_in(cell, depth + 2, blocks, gather);
                // Cells in one row read as one line, as they are seen.
                push_text(&mut gather.inlines, " ");
            }
            self.flush(blocks, gather);
        }
    }

    fn form(&mut self, handle: Handle) -> Option<Form> {
        let action = self.attr(handle, "action").unwrap_or("");
        let action = if action.trim().is_empty() {
            self.base.without_fragment()
        } else {
            self.base.join(action).ok()?
        };
        let method = if self
            .attr(handle, "method")
            .is_some_and(|m| m.eq_ignore_ascii_case("post"))
        {
            Method::Post
        } else {
            Method::Get
        };
        let mut fields = Vec::new();
        let mut stack = vec![handle];
        while let Some(node) = stack.pop() {
            if self.tag(node) == Some("input") {
                let kind = self
                    .attr(node, "type")
                    .unwrap_or("text")
                    .to_ascii_lowercase();
                let name = self.attr(node, "name").map(str::to_owned);
                let value = self.attr(node, "value").unwrap_or("").to_owned();
                match (kind.as_str(), name) {
                    ("text" | "search" | "email" | "url" | "tel" | "number", Some(name)) => {
                        let label = self
                            .attr(node, "aria-label")
                            .or_else(|| self.attr(node, "placeholder"))
                            .or_else(|| self.attr(node, "title"))
                            .unwrap_or(&name)
                            .to_owned();
                        fields.push(Field::Text {
                            name,
                            value,
                            label,
                            search: kind == "search",
                        });
                    }
                    ("hidden", Some(name)) => fields.push(Field::Hidden { name, value }),
                    ("submit", name) => fields.push(Field::Submit {
                        name,
                        value: if value.is_empty() {
                            "Submit".to_owned()
                        } else {
                            value
                        },
                    }),
                    _ => {}
                }
            }
            stack.extend(self.nodes[node].children.iter().rev().copied());
        }
        let has_text = fields.iter().any(|f| matches!(f, Field::Text { .. }));
        has_text.then_some(Form {
            action,
            method,
            fields,
        })
    }
}

/// Appends text with HTML whitespace collapsed to single spaces.
fn push_text(inlines: &mut Vec<Inline>, text: &str) {
    let collapsed = collapse_keep_edges(text);
    if collapsed.is_empty() {
        return;
    }
    if let Some(Inline::Text(existing)) = inlines.last_mut() {
        if existing.ends_with(' ') && collapsed.starts_with(' ') {
            existing.push_str(&collapsed[1..]);
        } else {
            existing.push_str(&collapsed);
        }
    } else {
        inlines.push(Inline::Text(collapsed));
    }
}

fn collapse_keep_edges(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() && c != '\u{a0}' {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// Collapsed and trimmed.
fn collapse(text: &str) -> String {
    collapse_keep_edges(text).trim().to_owned()
}

/// Drops leading and trailing whitespace and line breaks from a run.
fn trim_inlines(mut inlines: Vec<Inline>) -> Vec<Inline> {
    while matches!(inlines.first(), Some(Inline::LineBreak)) {
        inlines.remove(0);
    }
    while matches!(inlines.last(), Some(Inline::LineBreak)) {
        inlines.pop();
    }
    if let Some(Inline::Text(text)) = inlines.first_mut() {
        *text = text.trim_start().to_owned();
    }
    if let Some(Inline::Text(text)) = inlines.last_mut() {
        *text = text.trim_end().to_owned();
    }
    inlines.retain(|inline| !matches!(inline, Inline::Text(text) if text.is_empty()));
    inlines
}

/// Flattens inlines to plain text.
pub fn flatten(inlines: &[Inline], out: &mut String) {
    for inline in inlines {
        match inline {
            Inline::Text(text) | Inline::Code(text) => out.push_str(text),
            Inline::Emphasis(inner) | Inline::Strong(inner) | Inline::Inert(inner) => {
                flatten(inner, out);
            }
            Inline::Link { label, .. } => flatten(label, out),
            Inline::LineBreak => out.push('\n'),
        }
    }
}

pub fn visible_text(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph { content, .. } => {
                flatten(content, out);
                out.push('\n');
            }
            Block::List { items, .. } => {
                for item in items {
                    visible_text(item, out);
                }
            }
            Block::Quote(inner) => visible_text(inner, out),
            Block::Preformatted(text) => {
                out.push_str(text);
                out.push('\n');
            }
            Block::Table(table) => {
                for row in &table.rows {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|cell| {
                            let mut text = String::new();
                            flatten(cell, &mut text);
                            text
                        })
                        .collect();
                    out.push_str(&cells.join(" | "));
                    out.push('\n');
                }
            }
            Block::Image(image) => {
                if !image.alt.is_empty() {
                    out.push_str(&format!("[{}]\n", image.alt));
                }
            }
            Block::Form(_) | Block::Rule => {}
        }
    }
}
