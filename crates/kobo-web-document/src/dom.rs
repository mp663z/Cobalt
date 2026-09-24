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

pub struct Sink {
    nodes: RefCell<Vec<Node>>,
    text_bytes: Cell<usize>,
    max_text_bytes: usize,
    pub(crate) errors: Cell<usize>,
    pub(crate) dropped_attributes: Cell<usize>,
    pub(crate) dropped_text: Cell<bool>,
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
            max_text_bytes,
            errors: Cell::new(0),
            dropped_attributes: Cell::new(0),
            dropped_text: Cell::new(false),
        }
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
        for attribute in attrs {
            if kept.len() >= MAX_ATTRIBUTES || attribute.value.len() > MAX_ATTRIBUTE_LEN {
                self.dropped_attributes
                    .set(self.dropped_attributes.get() + 1);
                continue;
            }
            kept.push((attribute.name.local, attribute.value.to_string()));
        }
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

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

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
            for attribute in attrs {
                if existing.len() >= MAX_ATTRIBUTES {
                    break;
                }
                if !existing
                    .iter()
                    .any(|(name, _)| *name == attribute.name.local)
                    && attribute.value.len() <= MAX_ATTRIBUTE_LEN
                {
                    existing.push((attribute.name.local, attribute.value.to_string()));
                }
            }
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
