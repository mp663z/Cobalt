//! A bounded arena tree for html5ever to build into.
//!
//! html5ever decides where every node goes, which is the part of HTML parsing
//! that is hard to get right. This sink only stores what it is told, in one
//! vector indexed by handle, so there are no reference cycles to leak and no
//! recursion to overflow. It keeps nothing the document model does not read:
//! comments, processing instructions and the doctype are not stored.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use html5ever::interface::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::StrTendril;
use html5ever::{Attribute, LocalName, QualName};

pub type Handle = usize;

pub const DOCUMENT: Handle = 0;

#[derive(Debug)]
pub enum Data {
    Document,
    Element {
        name: QualName,
        attrs: Vec<(LocalName, String)>,
        template: Option<Handle>,
    },
    Text(String),
    /// A comment, doctype or instruction: kept as a node so html5ever can
    /// address it, but empty.
    Other,
}

#[derive(Debug)]
pub struct Node {
    pub data: Data,
    pub parent: Option<Handle>,
    pub children: Vec<Handle>,
}

/// Per-element attribute ceiling. The rest are dropped.
pub const MAX_ATTRIBUTES: usize = 32;
/// Longest attribute value kept, in bytes. A longer one is dropped whole.
pub const MAX_ATTRIBUTE_LEN: usize = 2048;

/// MEASUREMENT-ONLY scratch (isolated copy, not for adoption): drop log and a
/// per-(element,attribute) length exemption. Defaults are off and identical to head7.
pub mod measure {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    pub static LOG: Mutex<Vec<(String, String, usize, bool)>> = Mutex::new(Vec::new());
    /// 0 = off. Otherwise link[href], img[src], source[srcset] may be up to this many bytes.
    pub static EXEMPT_CAP: AtomicUsize = AtomicUsize::new(0);
    /// 0 = off. NARROW candidate: only link[href] starting with data:text/css (ASCII case-insensitive)
    /// may exceed the 2048-byte cap, each up to this many bytes...
    pub static SHEET_HREF_CAP: AtomicUsize = AtomicUsize::new(0);
    /// ...and at most this many such bytes in total per document (beyond: normal drop + loss).
    pub static SHEET_AGG_CAP: AtomicUsize = AtomicUsize::new(0);
    thread_local! { pub static TAG: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) }; }
}

pub struct Sink {
    nodes: RefCell<Vec<Node>>,
    text_bytes: Cell<usize>,
    exempt_bytes: Cell<usize>,
    /// Opt-in per-parse retention of oversize `link[href^=data:text/css]` values:
    /// (per-href cap, per-document aggregate cap). (0, 0) is off and byte-identical
    /// to the default behavior. Unlike `measure::*` this is not process-global.
    inline_sheet_href: Cell<(usize, usize)>,
    max_text_bytes: usize,
    pub(crate) errors: Cell<usize>,
    pub(crate) dropped_attributes: Cell<usize>,
    pub(crate) dropped_text: Cell<bool>,
    pub(crate) merged_attributes_dropped: Cell<usize>,
    pub(crate) attr_namespace_discarded: Cell<bool>,
    pub(crate) quirks_mode: Cell<QuirksMode>,
}

impl Sink {
    pub fn new(max_text_bytes: usize) -> Self {
        Self {
            nodes: RefCell::new(vec![Node {
                data: Data::Document,
                parent: None,
                children: Vec::new(),
            }]),
            text_bytes: Cell::new(0),
            exempt_bytes: Cell::new(0),
            inline_sheet_href: Cell::new((0, 0)),
            max_text_bytes,
            errors: Cell::new(0),
            dropped_attributes: Cell::new(0),
            dropped_text: Cell::new(false),
            merged_attributes_dropped: Cell::new(0),
            attr_namespace_discarded: Cell::new(false),
            quirks_mode: Cell::new(QuirksMode::Quirks),
        }
    }

    /// Opt in to keeping (whole, uncounted as dropped) `link[href]` values that start
    /// with `data:text/css` up to `per_href` bytes each and `aggregate` bytes per
    /// document. Every other over-cap attribute still counts as dropped.
    pub fn set_inline_sheet_href_caps(&self, per_href: usize, aggregate: usize) {
        self.inline_sheet_href.set((per_href, aggregate));
    }

    /// Bytes of oversize `link[href]` values kept under an opt-in or measurement cap.
    pub(crate) fn retained_oversize_href_bytes(&self) -> usize {
        self.exempt_bytes.get()
    }

    pub fn len(&self) -> usize {
        self.nodes.borrow().len()
    }

    fn push(&self, data: Data) -> Handle {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(Node {
            data,
            parent: None,
            children: Vec::new(),
        });
        nodes.len() - 1
    }

    fn detach(nodes: &mut [Node], child: Handle) {
        if let Some(parent) = nodes[child].parent.take() {
            nodes[parent].children.retain(|&c| c != child);
        }
    }

    /// Appends text, merging with a text node already in that position.
    fn text_at(&self, parent: Handle, before: Option<Handle>, text: &str) {
        let used = self.text_bytes.get();
        if used >= self.max_text_bytes {
            self.dropped_text.set(true);
            return;
        }
        let room = self.max_text_bytes - used;
        let text = if text.len() > room {
            self.dropped_text.set(true);
            let mut end = room;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            &text[..end]
        } else {
            text
        };
        self.text_bytes.set(used + text.len());
        let mut nodes = self.nodes.borrow_mut();
        let position = match before {
            Some(sibling) => nodes[parent].children.iter().position(|&c| c == sibling),
            None => Some(nodes[parent].children.len()),
        };
        let Some(position) = position else { return };
        if position > 0 {
            let previous = nodes[parent].children[position - 1];
            if let Data::Text(existing) = &mut nodes[previous].data {
                existing.push_str(text);
                return;
            }
        }
        nodes.push(Node {
            data: Data::Text(text.to_owned()),
            parent: Some(parent),
            children: Vec::new(),
        });
        let handle = nodes.len() - 1;
        nodes[parent].children.insert(position, handle);
    }

    fn node_at(&self, parent: Handle, before: Option<Handle>, child: Handle) {
        let mut nodes = self.nodes.borrow_mut();
        Self::detach(&mut nodes, child);
        let position = match before {
            Some(sibling) => nodes[parent].children.iter().position(|&c| c == sibling),
            None => Some(nodes[parent].children.len()),
        };
        let Some(position) = position else { return };
        nodes[child].parent = Some(parent);
        nodes[parent].children.insert(position, child);
    }

    fn keep_attrs(
        &self,
        existing: &mut Vec<(LocalName, String)>,
        attrs: Vec<Attribute>,
        losses: &Cell<usize>,
        tag: &str,
    ) {
        for attribute in attrs {
            // QualName namespace and prefix are independent. Observe both before
            // dedupe or limits can discard this evidence.
            if !attribute.name.ns.is_empty() || attribute.name.prefix.is_some() {
                self.attr_namespace_discarded.set(true);
            }
            if existing
                .iter()
                .any(|(name, _)| *name == attribute.name.local)
            {
                continue;
            }
            let exempt_cap = measure::EXEMPT_CAP.load(std::sync::atomic::Ordering::Relaxed);
            let (own_cap, own_agg) = self.inline_sheet_href.get();
            let (sheet_cap, sheet_agg) = if own_cap > 0 {
                (own_cap, own_agg)
            } else {
                (
                    measure::SHEET_HREF_CAP.load(std::sync::atomic::Ordering::Relaxed),
                    measure::SHEET_AGG_CAP.load(std::sync::atomic::Ordering::Relaxed),
                )
            };
            let narrow = sheet_cap > 0
                && tag == "link"
                && &*attribute.name.local == "href"
                && attribute
                    .value
                    .as_bytes()
                    .get(..13)
                    .is_some_and(|p| p.eq_ignore_ascii_case(b"data:text/css"))
                && attribute.value.len() <= sheet_cap
                && self.exempt_bytes.get().saturating_add(attribute.value.len()) <= sheet_agg;
            let exempt = narrow
                || (exempt_cap > 0
                    && matches!(
                        (tag, &*attribute.name.local),
                        ("link", "href") | ("img", "src") | ("source", "srcset")
                    )
                    && attribute.value.len() <= exempt_cap);
            if narrow && attribute.value.len() > MAX_ATTRIBUTE_LEN && existing.len() < MAX_ATTRIBUTES {
                self.exempt_bytes.set(self.exempt_bytes.get().saturating_add(attribute.value.len()));
            }
            if existing.len() >= MAX_ATTRIBUTES
                || (attribute.value.len() > MAX_ATTRIBUTE_LEN && !exempt)
            {
                if attribute.value.len() > MAX_ATTRIBUTE_LEN || existing.len() >= MAX_ATTRIBUTES {
                    measure::LOG.lock().unwrap().push((
                        tag.to_owned(),
                        attribute.name.local.to_string(),
                        attribute.value.len(),
                        existing.len() >= MAX_ATTRIBUTES,
                    ));
                }
                losses.set(losses.get().saturating_add(1));
                continue;
            }
            existing.push((attribute.name.local, attribute.value.to_string()));
        }
    }

    pub fn into_nodes(self) -> Vec<Node> {
        self.nodes.into_inner()
    }
}

pub struct Name {
    name: QualName,
}

impl std::fmt::Debug for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.name)
    }
}

impl html5ever::interface::ElemName for Name {
    fn ns(&self) -> &html5ever::Namespace {
        &self.name.ns
    }
    fn local_name(&self) -> &LocalName {
        &self.name.local
    }
}

impl TreeSink for Sink {
    type Handle = Handle;
    type Output = Self;
    type ElemName<'a> = Name;

    fn finish(self) -> Self {
        self
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {
        self.errors.set(self.errors.get().saturating_add(1));
    }

    fn get_document(&self) -> Handle {
        DOCUMENT
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> Name {
        match &self.nodes.borrow()[*target].data {
            Data::Element { name, .. } => Name { name: name.clone() },
            // html5ever only asks for the names of elements it created.
            _ => Name {
                name: QualName::new(None, html5ever::ns!(html), html5ever::local_name!("")),
            },
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        let mut kept = Vec::with_capacity(attrs.len().min(MAX_ATTRIBUTES));
        measure::TAG.with(|t| *t.borrow_mut() = name.local.to_string());
        self.keep_attrs(&mut kept, attrs, &self.dropped_attributes, &name.local);
        let template = flags.template.then(|| self.push(Data::Other));
        self.push(Data::Element {
            name,
            attrs: kept,
            template,
        })
    }

    fn create_comment(&self, _text: StrTendril) -> Handle {
        self.push(Data::Other)
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Handle {
        self.push(Data::Other)
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        match child {
            NodeOrText::AppendNode(node) => self.node_at(*parent, None, node),
            NodeOrText::AppendText(text) => self.text_at(*parent, None, &text),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev_element: &Handle,
        child: NodeOrText<Handle>,
    ) {
        let has_parent = self.nodes.borrow()[*element].parent.is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
    }

    fn get_template_contents(&self, target: &Handle) -> Handle {
        match &self.nodes.borrow()[*target].data {
            Data::Element {
                template: Some(contents),
                ..
            } => *contents,
            _ => *target,
        }
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks_mode.set(mode);
    }

    fn append_before_sibling(&self, sibling: &Handle, new_node: NodeOrText<Handle>) {
        let Some(parent) = self.nodes.borrow()[*sibling].parent else {
            return;
        };
        match new_node {
            NodeOrText::AppendNode(node) => self.node_at(parent, Some(*sibling), node),
            NodeOrText::AppendText(text) => self.text_at(parent, Some(*sibling), &text),
        }
    }

    fn add_attrs_if_missing(&self, target: &Handle, attrs: Vec<Attribute>) {
        let mut nodes = self.nodes.borrow_mut();
        if let Data::Element {
            attrs: existing, ..
        } = &mut nodes[*target].data
        {
            measure::TAG.with(|t| *t.borrow_mut() = String::from("(merge)"));
            self.keep_attrs(existing, attrs, &self.merged_attributes_dropped, "(merge)");
        }
    }

    fn remove_from_parent(&self, target: &Handle) {
        Self::detach(&mut self.nodes.borrow_mut(), *target);
    }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        let mut nodes = self.nodes.borrow_mut();
        let children = std::mem::take(&mut nodes[*node].children);
        for &child in &children {
            nodes[child].parent = Some(*new_parent);
        }
        nodes[*new_parent].children.extend(children);
    }
}

#[cfg(test)]
mod source_fact_tests {
    use super::*;
    use crate::source_facts::ParsedSourceFacts;

    fn attr(local: &str, value: &str) -> Attribute {
        Attribute {
            name: QualName::new(None, html5ever::ns!(), local.into()),
            value: value.into(),
        }
    }

    fn element(sink: &Sink, attrs: Vec<Attribute>) -> Handle {
        sink.create_element(
            QualName::new(None, html5ever::ns!(html), "div".into()),
            attrs,
            ElementFlags::default(),
        )
    }

    #[test]
    fn both_attribute_paths_classify_all_rejections_after_duplicates() {
        for merge in [false, true] {
            let sink = Sink::new(100);
            let full: Vec<_> = (0..MAX_ATTRIBUTES)
                .map(|n| attr(&format!("a{n}"), "v"))
                .collect();
            let mut incoming = vec![attr("a0", &"x".repeat(MAX_ATTRIBUTE_LEN + 1))];
            incoming.extend([attr("new1", "v"), attr("a1", "v"), attr("new2", "v")]);
            let handle = if merge {
                let handle = element(&sink, full);
                sink.add_attrs_if_missing(&handle, incoming);
                handle
            } else {
                element(&sink, full.into_iter().chain(incoming).collect())
            };
            assert_eq!(sink.dropped_attributes.get(), if merge { 0 } else { 2 });
            assert_eq!(
                sink.merged_attributes_dropped.get(),
                if merge { 2 } else { 0 }
            );
            let nodes = sink.into_nodes();
            let Data::Element { attrs, .. } = &nodes[handle].data else {
                panic!()
            };
            assert_eq!(attrs.len(), MAX_ATTRIBUTES);
        }
    }

    #[test]
    fn both_paths_reject_oversized_new_but_not_oversized_existing() {
        for merge in [false, true] {
            let sink = Sink::new(100);
            let incoming = vec![
                attr("same", &"x".repeat(MAX_ATTRIBUTE_LEN + 1)),
                attr("new", &"x".repeat(MAX_ATTRIBUTE_LEN + 1)),
            ];
            if merge {
                let handle = element(&sink, vec![attr("same", "first")]);
                sink.add_attrs_if_missing(&handle, incoming);
            } else {
                element(
                    &sink,
                    vec![attr("same", "first")]
                        .into_iter()
                        .chain(incoming)
                        .collect(),
                );
            }
            assert_eq!(
                sink.dropped_attributes.get() + sink.merged_attributes_dropped.get(),
                1
            );
        }
    }

    #[test]
    fn namespace_uri_or_prefix_observed_before_any_filter_on_both_paths() {
        for merge in [false, true] {
            for namespace in [false, true] {
                for prefix in [false, true] {
                    let sink = Sink::new(100);
                    let mut incoming = attr("a0", &"x".repeat(MAX_ATTRIBUTE_LEN + 1));
                    incoming.name.ns = if namespace {
                        "urn:test".into()
                    } else {
                        html5ever::ns!()
                    };
                    incoming.name.prefix = prefix.then(|| "p".into());
                    let full: Vec<_> = (0..MAX_ATTRIBUTES)
                        .map(|n| attr(&format!("a{n}"), "v"))
                        .collect();
                    if merge {
                        let handle = element(&sink, full);
                        sink.add_attrs_if_missing(&handle, vec![incoming]);
                    } else {
                        element(&sink, full.into_iter().chain([incoming]).collect());
                    }
                    assert_eq!(sink.attr_namespace_discarded.get(), namespace || prefix);
                    assert_eq!(
                        sink.dropped_attributes.get() + sink.merged_attributes_dropped.get(),
                        0
                    );
                }
            }
        }
    }

    #[test]
    fn loss_accounting_saturates_on_both_paths() {
        let sink = Sink::new(100);
        sink.dropped_attributes.set(usize::MAX);
        sink.merged_attributes_dropped.set(usize::MAX);
        let handle = element(
            &sink,
            vec![attr("too_long", &"x".repeat(MAX_ATTRIBUTE_LEN + 1))],
        );
        sink.add_attrs_if_missing(
            &handle,
            vec![attr("other", &"x".repeat(MAX_ATTRIBUTE_LEN + 1))],
        );
        assert_eq!(sink.dropped_attributes.get(), usize::MAX);
        assert_eq!(sink.merged_attributes_dropped.get(), usize::MAX);
    }

    #[test]
    fn arena_scan_includes_detached_template_and_other_namespaces_once() {
        let sink = Sink::new(100);
        let parent = element(&sink, vec![]);
        let script = sink.create_element(
            QualName::new(None, html5ever::ns!(html), "script".into()),
            vec![],
            ElementFlags::default(),
        );
        sink.append(&parent, NodeOrText::AppendNode(script));
        sink.remove_from_parent(&script);
        sink.create_element(
            QualName::new(None, "urn:other".into(), "div".into()),
            vec![],
            ElementFlags::default(),
        );
        let mut template_flags = ElementFlags::default();
        template_flags.template = true;
        let template = sink.create_element(
            QualName::new(None, html5ever::ns!(html), "template".into()),
            vec![],
            template_flags,
        );
        let contents = sink.get_template_contents(&template);
        let input = sink.create_element(
            QualName::new(None, html5ever::ns!(html), "input".into()),
            vec![],
            ElementFlags::default(),
        );
        sink.append(&contents, NodeOrText::AppendNode(input));
        let mut facts = ParsedSourceFacts::from_sink(&sink, 0, false, false, false);
        facts.scan(&sink.into_nodes());
        assert!(facts.scan_completed);
        assert_eq!(facts.profile.scripts, 1);
        assert_eq!(facts.profile.forms, 1);
        assert_eq!(facts.profile.template, 1);
        assert_eq!(facts.profile.foreign_other, 1);
    }
}

#[cfg(test)]
mod profile_saturation_test {
    use super::*;
    use crate::source_facts::ParsedSourceFacts;

    #[test]
    fn saturated_profile_counts_cannot_wrap_to_zero() {
        let sink = Sink::new(100);
        sink.create_element(
            QualName::new(None, html5ever::ns!(html), "script".into()),
            vec![Attribute {
                name: QualName::new(None, html5ever::ns!(), "onclick".into()),
                value: "".into(),
            }],
            ElementFlags::default(),
        );
        let mut facts = ParsedSourceFacts::from_sink(&sink, 0, false, false, false);
        facts.profile.scripts = usize::MAX;
        facts.profile.interaction_attrs = usize::MAX;
        facts.scan(&sink.into_nodes());
        assert_eq!(facts.profile.scripts, usize::MAX);
        assert_eq!(facts.profile.interaction_attrs, usize::MAX);
    }
}

#[cfg(test)]
mod profile_mapping_tests {
    use super::*;
    use crate::source_facts::{ParsedSourceFacts, SourceProfile};

    #[test]
    fn every_refusing_html_tag_has_an_explicit_exact_profile_mapping() {
        type Mapping = (&'static [&'static str], fn(&SourceProfile) -> usize);
        let groups: &[Mapping] = &[
            (&["script"], |p| p.scripts),
            (
                &["form", "input", "button", "select", "textarea", "datalist"],
                |p| p.forms,
            ),
            (&["a", "area"], |p| p.links),
            (&["details", "summary"], |p| p.details_summary),
            (&["map"], |p| p.image_maps),
            (
                &[
                    "iframe", "frame", "frameset", "object", "embed", "applet", "noembed",
                    "noframes",
                ],
                |p| p.frames,
            ),
            (&["dialog"], |p| p.dialogs),
            (&["img", "picture", "source"], |p| p.images),
            (&["video", "audio", "track", "canvas"], |p| p.media),
            (&["link", "meta", "style"], |p| p.head_affecting),
            (&["template"], |p| p.template),
            (&["unknown", "custom-name", "noscript", "base"], |p| {
                p.unknown_custom
            }),
        ];
        for (tags, count) in groups {
            for tag in *tags {
                let sink = Sink::new(0);
                sink.create_element(
                    QualName::new(None, html5ever::ns!(html), (*tag).into()),
                    vec![],
                    ElementFlags::default(),
                );
                let mut f = ParsedSourceFacts::from_sink(&sink, 0, false, false, false);
                f.scan(&sink.into_nodes());
                assert_eq!(count(&f.profile), 1, "{tag}");
            }
        }
    }
}
