//! Bounded, fail-closed CSS visibility subset for static article rendering.
//! No URLs, imports, pseudo selectors, font downloads or script are evaluated.
use crate::dom::{Data, Handle, Node};

const MAX_SHEETS: usize = 8;
const MAX_BYTES: usize = 64 * 1024;
const MAX_RULES: usize = 1024;
const MAX_SELECTORS: usize = 16;
const MAX_STEPS: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Simple {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    /// `:root`: the document element (the parent is the Document node).
    root: bool,
    /// Extended grammar only: type selectors compare ASCII case-insensitively
    /// (Chrome matches `linearGradient` and `lineargradient` on SVG elements) and
    /// class tokens split on ASCII whitespace only.
    ext: bool,
    /// Extended grammar only: attribute selectors and structural pseudo-classes.
    attrs: Vec<AttrSel>,
    /// Bit 1 `:first-child`, bit 2 `:last-child`, bit 4 `:only-child`.
    structural: u8,
    /// Occurrences of structural pseudo-classes (each counts toward specificity,
    /// so `:first-child:first-child` is two).
    structural_n: u8,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct AttrSel {
    name: String,
    op: AttrOp,
    value: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AttrOp {
    Exists,
    Equals,
    Word,
    Dash,
    Prefix,
    Suffix,
    Substring,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Selector {
    // Rightmost first. Relation says how the next step to the left connects.
    steps: Vec<(Simple, Relation)>,
    pub(crate) specificity: (u16, u16, u16),
    /// Extended (opt-in) grammar: backtracking matcher, bounded by fuel.
    ext: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Relation {
    Descendant,
    Child,
    NextSibling,
    LaterSibling,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Visibility {
    Hidden,
    Visible,
}
#[derive(Clone, Debug)]
struct Rule {
    selector: Selector,
    visibility: Visibility,
    important: bool,
    order: usize,
}

type RankedVisibility = (bool, (u16, u16, u16), usize, Visibility);

#[derive(Default)]
pub(super) struct Styles {
    rules: Vec<Rule>,
}
impl Styles {
    pub fn from_dom(nodes: &[Node]) -> Self {
        let mut styles = Self::default();
        let mut bytes = 0;
        let mut sheets = 0;
        for node in nodes {
            let Data::Element { name, .. } = &node.data else {
                continue;
            };
            if &*name.local != "style" || sheets >= MAX_SHEETS || bytes >= MAX_BYTES {
                continue;
            }
            sheets += 1;
            let mut text = String::new();
            for &child in &node.children {
                if let Data::Text(content) = &nodes[child].data {
                    text.push_str(content);
                }
            }
            let kept = text.len().min(MAX_BYTES - bytes);
            let kept = (0..=kept)
                .rev()
                .find(|&n| text.is_char_boundary(n))
                .unwrap_or(0);
            bytes += kept;
            styles.add_sheet(&text[..kept]);
        }
        styles
    }
    fn add_sheet(&mut self, text: &str) {
        let clean = strip_comments(text);
        let input = clean.as_bytes();
        let mut cursor = 0;
        while cursor < input.len() && self.rules.len() < MAX_RULES {
            let begin = cursor;
            while cursor < input.len() && input[cursor] != b'{' {
                cursor += 1;
            }
            if cursor == input.len() {
                break;
            }
            let header = clean[begin..cursor].trim();
            cursor += 1;
            let body_begin = cursor;
            let mut depth = 1usize;
            while cursor < input.len() && depth != 0 {
                match input[cursor] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    _ => {}
                }
                cursor += 1;
            }
            if depth != 0 {
                break;
            }
            // Skip whole @media/@supports/@font-face blocks. Their inner rules
            // cannot leak into the unconditional cascade.
            if header.starts_with('@') || clean[body_begin..cursor - 1].contains(['{', '}']) {
                continue;
            }
            let body = &clean[body_begin..cursor - 1];
            let Some((visibility, important)) = display(body) else {
                continue;
            };
            for item in header.split(',').take(MAX_SELECTORS) {
                if self.rules.len() >= MAX_RULES {
                    break;
                }
                if let Some(selector) = selector(item.trim()) {
                    self.rules.push(Rule {
                        selector,
                        visibility,
                        important,
                        order: self.rules.len(),
                    });
                }
            }
        }
    }
    pub fn hidden(&self, nodes: &[Node], handle: Handle) -> bool {
        let inline = attribute(nodes, handle, "style").and_then(display);
        let mut chosen: Option<RankedVisibility> = None;
        for rule in &self.rules {
            if !rule.selector.matches(nodes, handle) {
                continue;
            }
            let rank = (rule.important, rule.selector.specificity, rule.order);
            if chosen.is_none_or(|(important, specificity, order, _)| {
                rank >= (important, specificity, order)
            }) {
                chosen = Some((rank.0, rank.1, rank.2, rule.visibility));
            }
        }
        if let Some((value, important)) = inline {
            let rank = (important, (1, 0, 0), usize::MAX);
            if chosen.is_none_or(|(a, b, c, _)| rank >= (a, b, c)) {
                chosen = Some((rank.0, rank.1, rank.2, value));
            }
        }
        matches!(chosen, Some((_, _, _, Visibility::Hidden)))
    }
}
pub(crate) fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start + 2..].find("*/") else {
            return out;
        };
        rest = &rest[start + 2 + end + 2..];
    }
    out.push_str(rest);
    out
}
fn display(input: &str) -> Option<(Visibility, bool)> {
    let mut value = None;
    for declaration in input.split(';').take(128) {
        let Some((name, rhs)) = declaration.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("display") {
            continue;
        }
        let lower = rhs.trim().to_ascii_lowercase();
        let important = lower.ends_with("!important");
        let body = lower.strip_suffix("!important").unwrap_or(&lower).trim();
        let parsed = match body {
            "none" => Some(Visibility::Hidden),
            "block" | "inline" | "inline-block" | "list-item" | "flex" | "grid" | "table" => {
                Some(Visibility::Visible)
            }
            _ => None,
        };
        if let Some(parsed) = parsed {
            value = Some((parsed, important));
        }
    }
    value
}
pub(crate) fn selector(input: &str) -> Option<Selector> {
    if input.is_empty() || input.len() > 256 || input.contains(['[', ']', '+', '~', '*', '|']) {
        return None;
    }
    let spaced = input.replace('>', " > ");
    let parts: Vec<_> = spaced.split_whitespace().collect();
    if parts.is_empty() || parts.len() > MAX_STEPS * 2 {
        return None;
    }
    let mut parsed: Vec<(Simple, Relation)> = Vec::new();
    let mut expecting_step = true;
    let mut child = false;
    for part in parts {
        if part == ">" {
            if expecting_step {
                return None;
            }
            expecting_step = true;
            child = true;
            continue;
        }
        parsed.push((
            simple(part)?,
            if child {
                Relation::Child
            } else {
                Relation::Descendant
            },
        ));
        if parsed.len() > MAX_STEPS {
            return None;
        }
        expecting_step = false;
        child = false;
    }
    if expecting_step {
        return None;
    }
    let specificity =
        parsed
            .iter()
            .fold((0_u16, 0_u16, 0_u16), |(ids, classes, types), (part, _)| {
                (
                    ids + u16::from(part.id.is_some()),
                    // A pseudo-class counts as a class for specificity.
                    classes + u16::try_from(part.classes.len()).unwrap_or(8) + u16::from(part.root),
                    types + u16::from(part.tag.is_some()),
                )
            });
    parsed.reverse();
    Some(Selector {
        steps: parsed,
        specificity,
        ext: false,
    })
}
fn simple(part: &str) -> Option<Simple> {
    let mut tag = None;
    let mut id = None;
    let mut classes = Vec::new();
    let mut root = false;
    let mut offset = 0;
    while offset < part.len() {
        let marker = part.as_bytes()[offset];
        let skip = usize::from(matches!(marker, b'#' | b'.' | b':'));
        let end = part[offset + skip..]
            .find(['#', '.', ':'])
            .map_or(part.len(), |at| offset + skip + at);
        let token = &part[offset + skip..end];
        if token.is_empty()
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return None;
        }
        match marker {
            b'#' if id.is_none() => id = Some(token.to_owned()),
            b'.' if classes.len() < 8 => classes.push(token.to_owned()),
            // Only `:root` is a supported pseudo-class. Every other colon form
            // (`:hover`, `:not(`, `::before`) fails closed above or below.
            b':' if !root && token.eq_ignore_ascii_case("root") => root = true,
            b if b.is_ascii_alphabetic() && offset == 0 && tag.is_none() => {
                tag = Some(token.to_ascii_lowercase());
            }
            _ => return None,
        }
        offset = end;
    }
    Some(Simple {
        tag,
        id,
        classes,
        root,
        ext: false,
        attrs: Vec::new(),
        structural: 0,
        structural_n: 0,
    })
}

fn ident_ok(token: &str) -> bool {
    let b = token.as_bytes();
    !b.is_empty()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_')
        && !b[0].is_ascii_digit()
        && !(b[0] == b'-' && b.get(1).is_some_and(u8::is_ascii_digit))
        && token != "-"
}
fn take_ident(s: &str, at: usize) -> (&str, usize) {
    let end = s.as_bytes()[at..]
        .iter()
        .position(|c| !(c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_'))
        .map_or(s.len(), |n| at + n);
    (&s[at..end], end)
}
fn skip_ws(b: &[u8], mut at: usize) -> usize {
    while at < b.len() && matches!(b[at], b' ' | b'\t' | b'\n' | b'\r' | 0x0C) {
        at += 1;
    }
    at
}
fn attr_selector(s: &str, mut at: usize) -> Option<(AttrSel, usize)> {
    let b = s.as_bytes();
    at = skip_ws(b, at + 1);
    let (name, next) = take_ident(s, at);
    if !ident_ok(name) {
        return None;
    }
    at = skip_ws(b, next);
    let (op, value) = match *b.get(at)? {
        b']' => return Some((AttrSel { name: name.to_owned(), op: AttrOp::Exists, value: String::new() }, at + 1)),
        b'=' => {
            at += 1;
            (AttrOp::Equals, ())
        }
        c @ (b'~' | b'|' | b'^' | b'$' | b'*') if b.get(at + 1) == Some(&b'=') => {
            at += 2;
            (
                match c {
                    b'~' => AttrOp::Word,
                    b'|' => AttrOp::Dash,
                    b'^' => AttrOp::Prefix,
                    b'$' => AttrOp::Suffix,
                    _ => AttrOp::Substring,
                },
                (),
            )
        }
        _ => return None,
    };
    let _ = value;
    at = skip_ws(b, at);
    let text = match *b.get(at)? {
        q @ (b'"' | b'\'') => {
            let close = b[at + 1..].iter().position(|c| *c == q)? + at + 1;
            let inner = &s[at + 1..close];
            if inner.contains(['\\', '\n', '\r', '\u{c}']) {
                return None;
            }
            at = close + 1;
            inner
        }
        _ => {
            let (word, next) = take_ident(s, at);
            if !ident_ok(word) {
                return None;
            }
            at = next;
            word
        }
    };
    at = skip_ws(b, at);
    // `i` / `s` case flags are not supported.
    if *b.get(at)? != b']' {
        return None;
    }
    Some((AttrSel { name: name.to_owned(), op, value: text.to_owned() }, at + 1))
}
fn compound_ext(s: &str, mut at: usize) -> Option<(Simple, usize)> {
    let b = s.as_bytes();
    let mut simple = Simple {
        tag: None,
        id: None,
        classes: Vec::new(),
        root: false,
        ext: true,
        attrs: Vec::new(),
        structural: 0,
        structural_n: 0,
    };
    let start = at;
    if b.get(at) == Some(&b'*') {
        at += 1;
    } else if b.get(at).is_some_and(|c| c.is_ascii_alphabetic()) {
        let (tag, next) = take_ident(s, at);
        if !ident_ok(tag) {
            return None;
        }
        simple.tag = Some(tag.to_ascii_lowercase());
        at = next;
    }
    while at < b.len() {
        match b[at] {
            b'#' => {
                let (token, next) = take_ident(s, at + 1);
                if simple.id.is_some() || !ident_ok(token) {
                    return None;
                }
                simple.id = Some(token.to_owned());
                at = next;
            }
            b'.' => {
                let (token, next) = take_ident(s, at + 1);
                if simple.classes.len() >= 8 || !ident_ok(token) {
                    return None;
                }
                simple.classes.push(token.to_owned());
                at = next;
            }
            b'[' => {
                let (attr, next) = attr_selector(s, at)?;
                if simple.attrs.len() >= 8 {
                    return None;
                }
                simple.attrs.push(attr);
                at = next;
            }
            b':' => {
                let (token, next) = take_ident(s, at + 1);
                match token.to_ascii_lowercase().as_str() {
                    "root" if !simple.root => simple.root = true,
                    "first-child" => simple.structural |= 1,
                    "last-child" => simple.structural |= 2,
                    "only-child" => simple.structural |= 4,
                    _ => return None,
                }
                if token.to_ascii_lowercase() != "root" {
                    if simple.structural_n >= 8 {
                        return None;
                    }
                    simple.structural_n += 1;
                }
                at = next;
            }
            _ => break,
        }
    }
    (at > start).then_some((simple, at))
}
/// Opt-in grammar: compounds of type/`*`/`#id`/`.class`/`[attr op value]`/
/// `:root`/`:first-child`/`:last-child`/`:only-child`, joined by descendant,
/// `>`, `+` and `~`. State pseudo-classes, functional pseudo-classes,
/// pseudo-elements, namespaces, escapes and case flags are refused.
pub(crate) fn selector_ext(input: &str) -> Option<Selector> {
    if input.is_empty() || input.len() > 256 {
        return None;
    }
    let b = input.as_bytes();
    let mut steps: Vec<(Simple, Relation)> = Vec::new();
    let mut relation = Relation::Descendant;
    let mut at = skip_ws(b, 0);
    loop {
        let (simple, next) = compound_ext(input, at)?;
        steps.push((simple, relation));
        if steps.len() > MAX_STEPS {
            return None;
        }
        let after = skip_ws(b, next);
        if after >= b.len() {
            break;
        }
        let spaced = after > next;
        match b[after] {
            c @ (b'>' | b'+' | b'~') => {
                relation = match c {
                    b'>' => Relation::Child,
                    b'+' => Relation::NextSibling,
                    _ => Relation::LaterSibling,
                };
                at = skip_ws(b, after + 1);
            }
            _ if spaced => {
                relation = Relation::Descendant;
                at = after;
            }
            _ => return None,
        }
    }
    let specificity = steps.iter().fold((0_u16, 0_u16, 0_u16), |(ids, classes, types), (part, _)| {
        (
            ids + u16::from(part.id.is_some()),
            classes
                + u16::try_from(part.classes.len() + part.attrs.len()).unwrap_or(16)
                + u16::from(part.root)
                + u16::from(part.structural_n),
            types + u16::from(part.tag.is_some()),
        )
    });
    steps.reverse();
    Some(Selector { steps, specificity, ext: true })
}
const EXT_FUEL: u32 = 4096;
const MAX_SIBLINGS: usize = 4096;
fn is_element(nodes: &[Node], handle: Handle) -> bool {
    matches!(nodes[handle].data, Data::Element { .. })
}
/// HTML attributes whose selector values compare ASCII case-insensitively.
const CI_VALUE_ATTRS: &[&str] = &[
    "accept", "accept-charset", "align", "alink", "axis", "bgcolor", "charset", "checked",
    "clear", "codetype", "color", "compact", "declare", "defer", "dir", "direction", "disabled",
    "enctype", "face", "frame", "hreflang", "http-equiv", "lang", "language", "link", "media",
    "method", "multiple", "nohref", "noresize", "noshade", "nowrap", "readonly", "rel", "rev",
    "rules", "scope", "scrolling", "selected", "shape", "target", "text", "type", "valign",
    "valuetype", "vlink",
];
impl AttrSel {
    fn matches(&self, nodes: &[Node], handle: Handle) -> bool {
        let Data::Element { name, attrs, .. } = &nodes[handle].data else {
            return false;
        };
        let html = name.ns == html5ever::ns!(html);
        // Chrome compares attribute names ASCII case-insensitively in an HTML
        // document, also on SVG elements (`[viewbox]` matches `viewBox`).
        let found = attrs
            .iter()
            .find(|(key, _)| (&**key).eq_ignore_ascii_case(&self.name));
        let Some((_, actual)) = found else { return false };
        if self.op == AttrOp::Exists {
            return true;
        }
        let fold = html && CI_VALUE_ATTRS.contains(&self.name.to_ascii_lowercase().as_str());
        let (actual, want) = if fold {
            (actual.to_ascii_lowercase(), self.value.to_ascii_lowercase())
        } else {
            (actual.clone(), self.value.clone())
        };
        match self.op {
            AttrOp::Exists => true,
            AttrOp::Equals => actual == want,
            AttrOp::Word => {
                // CSS whitespace is ASCII only (space, tab, LF, FF, CR); U+00A0 and
                // other Unicode spaces are ordinary characters.
                let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\u{c}' | '\r');
                !want.is_empty()
                    && !want.contains(is_space)
                    && actual.split(is_space).any(|word| word == want)
            }
            AttrOp::Dash => actual == want || actual.starts_with(&format!("{want}-")),
            AttrOp::Prefix => !want.is_empty() && actual.starts_with(&want),
            AttrOp::Suffix => !want.is_empty() && actual.ends_with(&want),
            AttrOp::Substring => !want.is_empty() && actual.contains(&want),
        }
    }
}
fn element_siblings(nodes: &[Node], handle: Handle) -> Option<&[Handle]> {
    let parent = nodes[handle].parent?;
    Some(&nodes[parent].children)
}
fn structural_ok(nodes: &[Node], handle: Handle, bits: u8) -> bool {
    let Some(siblings) = element_siblings(nodes, handle) else {
        // A detached node has no siblings: it is its own first and last element.
        return true;
    };
    let first = siblings.iter().copied().find(|&n| is_element(nodes, n));
    let last = siblings.iter().rev().copied().find(|&n| is_element(nodes, n));
    (bits & 1 == 0 || first == Some(handle))
        && (bits & 2 == 0 || last == Some(handle))
        && (bits & 4 == 0 || (first == Some(handle) && last == Some(handle)))
}
impl Selector {
    /// `None` means the bounded matcher ran out of fuel or met an oversize
    /// sibling list: the caller must refuse, never treat it as a mismatch.
    pub(crate) fn matches_checked(&self, nodes: &[Node], handle: Handle) -> Option<bool> {
        if !self.ext {
            return Some(self.matches(nodes, handle));
        }
        let mut fuel = EXT_FUEL;
        self.ext_match(nodes, handle, 0, &mut fuel)
    }
    fn ext_match(&self, nodes: &[Node], node: Handle, index: usize, fuel: &mut u32) -> Option<bool> {
        if *fuel == 0 {
            return None;
        }
        *fuel -= 1;
        let (simple, relation) = &self.steps[index];
        if !simple.matches(nodes, node) {
            return Some(false);
        }
        if index + 1 == self.steps.len() {
            return Some(true);
        }
        match relation {
            Relation::Child => match nodes[node].parent {
                Some(parent) => self.ext_match(nodes, parent, index + 1, fuel),
                None => Some(false),
            },
            Relation::Descendant => {
                let mut ancestor = nodes[node].parent;
                while let Some(candidate) = ancestor {
                    if self.ext_match(nodes, candidate, index + 1, fuel)? {
                        return Some(true);
                    }
                    ancestor = nodes[candidate].parent;
                }
                Some(false)
            }
            Relation::NextSibling | Relation::LaterSibling => {
                let Some(siblings) = element_siblings(nodes, node) else {
                    return Some(false);
                };
                if siblings.len() > MAX_SIBLINGS {
                    return None;
                }
                let at = siblings.iter().position(|&n| n == node)?;
                for &earlier in siblings[..at].iter().rev() {
                    if !is_element(nodes, earlier) {
                        continue;
                    }
                    if self.ext_match(nodes, earlier, index + 1, fuel)? {
                        return Some(true);
                    }
                    if *relation == Relation::NextSibling {
                        break;
                    }
                }
                Some(false)
            }
        }
    }
}
pub(crate) fn attribute<'a>(nodes: &'a [Node], handle: Handle, name: &str) -> Option<&'a str> {
    let Data::Element { attrs, .. } = &nodes[handle].data else {
        return None;
    };
    attrs
        .iter()
        .find(|(key, _)| &**key == name)
        .map(|(_, value)| value.as_str())
}
impl Selector {
    pub(crate) fn matches(&self, nodes: &[Node], handle: Handle) -> bool {
        let mut current = handle;
        for (index, (simple, _)) in self.steps.iter().enumerate() {
            if index == 0 {
                if !simple.matches(nodes, current) {
                    return false;
                }
            } else {
                let mut ancestor = nodes[current].parent;
                let mut found = None;
                while let Some(node) = ancestor {
                    if simple.matches(nodes, node) {
                        found = Some(node);
                        break;
                    }
                    if self.steps[index - 1].1 == Relation::Child {
                        break;
                    }
                    ancestor = nodes[node].parent;
                }
                let Some(node) = found else { return false };
                current = node;
            }
        }
        true
    }
}
impl Simple {
    fn matches(&self, nodes: &[Node], handle: Handle) -> bool {
        let Data::Element { name, .. } = &nodes[handle].data else {
            return false;
        };
        if self.root
            && !nodes[handle]
                .parent
                .is_some_and(|parent| matches!(nodes[parent].data, Data::Document))
        {
            return false;
        }
        if self.tag.as_ref().is_some_and(|tag| {
            if self.ext {
                !name.local.as_ref().eq_ignore_ascii_case(tag)
            } else {
                name.local.as_ref() != tag
            }
        }) {
            return false;
        }
        if self
            .id
            .as_deref()
            .is_some_and(|id| attribute(nodes, handle, "id") != Some(id))
        {
            return false;
        }
        self.classes.iter().all(|class| {
            attribute(nodes, handle, "class").is_some_and(|value| {
                if self.ext {
                    // Extended grammar: class tokens split on ASCII whitespace only.
                    value
                        .split(|c: char| matches!(c, ' ' | '\t' | '\n' | '\u{c}' | '\r'))
                        .any(|word| word == class)
                } else {
                    value.split_whitespace().any(|word| word == class)
                }
            })
        }) && self.attrs.iter().all(|attr| attr.matches(nodes, handle))
            && (self.structural == 0 || structural_ok(nodes, handle, self.structural))
    }
}

#[cfg(test)]
mod tests {
    use crate::{parse_styled_document, Limits, Url};
    fn visible(html: &str) -> String {
        let url = Url::parse("https://example.org/article").unwrap();
        parse_styled_document(html.as_bytes(), &url, &Limits::DEFAULT).visible_text()
    }
    #[test]
    fn cascade_selector_specificity_important_and_inline() {
        let html = r#"<style>.nav{display:none} article > .nav{display:block} #menu{display:none!important} .avoid{display:none}</style>
        <article><p class="nav">Shown child</p><div><p class="nav">Hidden descendant</p></div></article>
        <p id="menu" style="display:block">Hidden important</p>
        <p class="avoid" style="display:block">Shown inline</p><p>Always shown</p>"#;
        let text = visible(html);
        assert!(text.contains("Shown child"));
        assert!(text.contains("Shown inline"));
        assert!(text.contains("Always shown"));
        assert!(!text.contains("Hidden descendant"));
        assert!(!text.contains("Hidden important"));
    }
    #[test]
    fn grouped_selectors_comments_and_unsupported_syntax_fail_closed() {
        let text = visible("<style>/*hide*/ .ad, #toc { display : none } [data-x] { display:none } @import url(https://evil.test/x);</style><p class=ad>Ad</p><p id=toc>TOC</p><p data-x=1>Visible</p>");
        assert!(!text.contains("Ad"));
        assert!(!text.contains("TOC"));
        assert!(text.contains("Visible"));
    }
    #[test]
    fn at_rule_blocks_do_not_leak_declarations_outside_their_condition() {
        let text = visible("<style>@media print { .article {display:none} } @supports (display:grid) { .article {display:none} } .other {display:none}</style><p class=article>Article</p><p class=other>Other</p>");
        assert!(text.contains("Article"));
        assert!(!text.contains("Other"));
    }
    #[test]
    fn wikipedia_fixture_hides_matching_navigation_but_keeps_article() {
        let html = r#"<style>#mw-panel, .vector-toc {display:none} .mw-parser-output {display:block}</style><nav id="mw-panel">Navigation</nav><div class="vector-toc">Contents</div><main><h1>E-reader</h1><div class="mw-parser-output"><p>Electronic ink</p></div></main>"#;
        let text = visible(html);
        assert!(!text.contains("Navigation"));
        assert!(!text.contains("Contents"));
        assert!(text.contains("E-reader"));
        assert!(text.contains("Electronic ink"));
    }
}

#[cfg(test)]
mod selector_control {
    //! Control harness (ignored by default): prints, per selector, the preorder
    //! element indices the extended matcher selects, for comparison with Chrome's
    //! `querySelectorAll` on the same document. Env: SEL_HTML, SEL_LIST, SEL_OUT.
    use super::*;
    use html5ever::tendril::TendrilSink;
    fn preorder(nodes: &[Node], at: Handle, out: &mut Vec<Handle>) {
        if matches!(nodes[at].data, Data::Element { .. }) {
            out.push(at);
        }
        for &child in &nodes[at].children {
            preorder(nodes, child, out);
        }
    }
    #[test]
    #[ignore]
    fn control_dump() {
        let html = std::fs::read(std::env::var("SEL_HTML").unwrap()).unwrap();
        let text = String::from_utf8_lossy(&html).into_owned();
        let sink = crate::dom::Sink::new(64 * 1024 * 1024);
        let mut parser = html5ever::parse_document(
            sink,
            html5ever::ParseOpts {
                tree_builder: html5ever::tree_builder::TreeBuilderOpts {
                    scripting_enabled: false,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        parser.process(text.into());
        let nodes = parser.finish().into_nodes();
        let mut order = Vec::new();
        preorder(&nodes, crate::dom::DOCUMENT, &mut order);
        let list = std::fs::read_to_string(std::env::var("SEL_LIST").unwrap()).unwrap();
        let mut out = String::new();
        for line in list.lines() {
            let line = line.trim_end_matches('\r');
            match selector_ext(line) {
                None => out.push_str(&format!("{line}\tREFUSED\n")),
                Some(sel) => {
                    let mut hits = Vec::new();
                    let mut unresolved = false;
                    for (index, &handle) in order.iter().enumerate() {
                        match sel.matches_checked(&nodes, handle) {
                            Some(true) => hits.push(index.to_string()),
                            Some(false) => {}
                            None => unresolved = true,
                        }
                    }
                    if unresolved {
                        out.push_str(&format!("{line}\tUNRESOLVED\n"));
                    } else {
                        out.push_str(&format!("{line}\t{}\n", hits.join(",")));
                    }
                }
            }
        }
        std::fs::write(std::env::var("SEL_OUT").unwrap(), out).unwrap();
    }
}

#[cfg(test)]
mod ext_selector_tests {
    use crate::{
        parse_style_tree_with_inline_sheets, parse_style_tree_with_inline_sheets_and_selectors,
        style_tree::StyleTree, Limits, SupplyBudget,
    };
    const RED: Option<u32> = Some(0xff_00_00);
    fn run(ext: bool, html: &str) -> StyleTree {
        let budget = SupplyBudget::CANONICAL;
        if ext {
            parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(), &[], &Limits::DEFAULT, None, budget)
        } else {
            parse_style_tree_with_inline_sheets(html.as_bytes(), &[], &Limits::DEFAULT, None, budget)
        }
    }
    /// Background of every element with this tag, in document order.
    fn reds(tree: &StyleTree, tag: &str) -> Vec<bool> {
        tree.nodes
            .iter()
            .filter(|n| n.tag == tag)
            .map(|n| n.style.background_color == RED)
            .collect()
    }
    #[test]
    fn default_entry_still_refuses_attribute_selectors_and_ext_entry_applies_them() {
        let html = "<style>p[data-x]{background-color:#ff0000}</style><p data-x=1>a</p><p>b</p>";
        let default = run(false, html);
        assert!(default.unsupported);
        assert_eq!(reds(&default, "p"), [false, false]);
        let ext = run(true, html);
        assert!(!ext.unsupported, "{:?}", ext.diagnostics);
        assert_eq!(reds(&ext, "p"), [true, false]);
    }
    #[test]
    fn attribute_operators_follow_selectors_semantics() {
        let cases: &[(&str, &str, bool)] = &[
            (r#"[a=b]"#, r#"a=b"#, true),
            (r#"[a=B]"#, r#"a=b"#, false),
            (r#"[a~=b]"#, r#"a="x b y""#, true),
            (r#"[a~=""]"#, r#"a="x b y""#, false),
            (r#"[a~="x b"]"#, r#"a="x b y""#, false),
            (r#"[a|=en]"#, r#"a="en-GB""#, true),
            (r#"[a|=en]"#, r#"a="eng""#, false),
            (r#"[a^=x]"#, r#"a="xy""#, true),
            (r#"[a^=""]"#, r#"a="xy""#, false),
            (r#"[a$=y]"#, r#"a="xy""#, true),
            (r#"[a*="x y"]"#, r#"a="x y z""#, true),
            (r#"[a*=""]"#, r#"a="x""#, false),
            // `type` compares ASCII case-insensitively on HTML elements.
            (r#"[type=text]"#, r#"type="TEXT""#, true),
            (r#"[data-t=text]"#, r#"data-t="TEXT""#, false),
            (r#"[A]"#, r#"a="x""#, true),
        ];
        for (selector, attributes, expected) in cases {
            let html = format!("<style>p{selector}{{background-color:#ff0000}}</style><p {attributes}>x</p>");
            let tree = run(true, &html);
            assert!(!tree.unsupported, "{selector}");
            assert_eq!(reds(&tree, "p"), [*expected], "{selector} on {attributes}");
        }
    }
    #[test]
    fn structural_pseudo_classes_use_element_siblings_not_text_or_comments() {
        let html = "<style>i:first-child{background-color:#ff0000} b:last-child{background-color:#ff0000} u:only-child{background-color:#ff0000}</style>\
            <div>text<!--c--><i>1</i>text<i>2</i><!--c-->tail</div><div><b>1</b><b>2</b>t</div><div>t<u>o</u>t</div><div><u>1</u><u>2</u></div>";
        let tree = run(true, html);
        assert!(!tree.unsupported);
        assert_eq!(reds(&tree, "i"), [true, false]);
        assert_eq!(reds(&tree, "b"), [false, true]);
        assert_eq!(reds(&tree, "u"), [true, false, false]);
    }
    #[test]
    fn sibling_combinators_and_backtracking_descendant_chains() {
        let html = "<style>i + i{background-color:#ff0000} em ~ s{background-color:#ff0000} div > section span{background-color:#ff0000}</style>\
            <section><i>1</i>t<i>2</i><b></b><i>3</i><em></em><u></u><s>a</s><s>b</s></section>\
            <div><section><b><section><span>deep</span></section></b></section></div><article><section><span>no</span></section></article>";
        let tree = run(true, html);
        assert!(!tree.unsupported);
        assert_eq!(reds(&tree, "i"), [false, true, false]);
        assert_eq!(reds(&tree, "s"), [true, true]);
        // The outer `section` is a child of `div`; the nearest `section` is not.
        assert_eq!(reds(&tree, "span"), [true, false]);
    }
    #[test]
    fn default_entry_greedy_descendant_chain_gap_is_unchanged() {
        // Pre-existing default behavior: the nearest ancestor `section` is chosen and
        // never revisited, so this nested case is missed. Kept as is (default
        // byte-identical); the opt-in matcher above handles it.
        let html = "<style>div > section span{background-color:#ff0000}</style><div><section><b><section><span>deep</span></section></b></section></div>";
        assert_eq!(reds(&run(false, html), "span"), [false]);
    }
    #[test]
    fn unsupported_selector_forms_still_refuse() {
        for selector in [
            "p:hover", "p:focus", "p:focus-within", "p:visited", "p:link", "p::before", "p:before",
            "p:not(.a)", "p:is(.a)", "p:nth-child(2)", "p:first-of-type", "p:empty", "p[a=b i]",
            "p[a=b s]", "p[a|b=c]", "p[*|a]", r"p.a\:b", "svg|p", "p,", "> p", "p >", "p[a=1]",
            "p[a=b c]", "p[a=\"b\\\"c\"]", "p#a#b", ".1a", "p[a=b", "p[a",
        ] {
            let html = format!("<style>{selector}{{background-color:#ff0000}}</style><p a=b>x</p>");
            let tree = run(true, &html);
            assert!(tree.unsupported, "{selector} must refuse");
            // `p,` keeps its valid first half but is flagged unsupported, which
            // refuses the whole tree; every other form must not apply at all.
            if selector != "p," {
                assert_eq!(reds(&tree, "p"), [false], "{selector}");
            }
        }
    }
    #[test]
    fn oversize_sibling_list_with_sibling_combinator_refuses_not_mismatches() {
        let mut html = String::from("<style>i + i{background-color:#ff0000}</style><div>");
        for _ in 0..4100 {
            html.push_str("<i></i>");
        }
        html.push_str("</div>");
        let tree = run(true, &html);
        assert!(tree.truncated || tree.unsupported);
    }
    #[test]
    fn repeated_structural_pseudo_classes_count_each_occurrence_for_specificity() {
        // Chrome: two pseudo-classes beat one attribute selector.
        let html = "<style>p:first-child:first-child{color:#ff0000} p[a]{color:#0000ff}</style><div><p a=x>t</p></div>";
        let tree = run(true, html);
        assert!(!tree.unsupported);
        assert_eq!(tree.nodes.iter().find(|n| n.tag == "p").unwrap().style.color, 0xff_00_00);
        for pseudo in ["last-child", "only-child"] {
            let html = format!("<style>p:{pseudo}:{pseudo}{{color:#ff0000}} p[a]{{color:#0000ff}}</style><div><p a=x>t</p></div>");
            let tree = run(true, &html);
            assert_eq!(tree.nodes.iter().find(|n| n.tag == "p").unwrap().style.color, 0xff_00_00, "{pseudo}");
        }
        // Mixed kinds add up too: first+last+only (3) beats .a.b (2).
        let html = "<style>p:first-child:last-child:only-child{color:#ff0000} p.a.b{color:#0000ff}</style><div><p class='a b'>t</p></div>";
        assert_eq!(run(true, html).nodes.iter().find(|n| n.tag == "p").unwrap().style.color, 0xff_00_00);
    }
    #[test]
    fn word_and_class_matching_use_ascii_css_whitespace_only() {
        let cases: &[(&str, &str, bool)] = &[
            ("[data-w~=b]", "data-w=\"a\u{a0}b\"", false),
            ("[data-w~=b]", "data-w=\"a\u{b}b\"", false),
            ("[data-w~=b]", "data-w=\"a\u{2003}b\"", false),
            ("[data-w~=b]", "data-w=\"a\u{c}b\"", true),
            ("[data-w~=b]", "data-w=\"a\tb\"", true),
            ("[data-w~=b]", "data-w=\"a\nb\"", true),
            ("[data-w~=b]", "data-w=\"a\rb\"", true),
            ("[data-w~=\"a\u{a0}b\"]", "data-w=\"a\u{a0}b\"", true),
            ("[data-w~=\"a\tb\"]", "data-w=\"a\tb\"", false),
            ("[class~=b]", "class=\"a\u{a0}b\"", false),
            (".b", "class=\"a\u{a0}b\"", false),
            (".b", "class=\"a\u{b}b\"", false),
            (".b", "class=\"a\u{c}b\"", true),
            (".b", "class=\"a b\"", true),
        ];
        for (selector, attribute, expected) in cases {
            let html = format!("<style>p{selector}{{background-color:#ff0000}}</style><p {attribute}>x</p>");
            let tree = run(true, &html);
            assert!(!tree.unsupported, "{selector}");
            assert_eq!(reds(&tree, "p"), [*expected], "{selector} on {attribute:?}");
        }
    }
    #[test]
    fn svg_type_selectors_match_camel_case_elements_case_insensitively() {
        let html = "<style>lineargradient{background-color:#ff0000} foreignObject{background-color:#ff0000} CLIPPATH{background-color:#ff0000}</style>\
            <svg><defs><linearGradient></linearGradient><clipPath></clipPath></defs><foreignObject></foreignObject></svg>";
        let tree = run(true, html);
        assert!(!tree.unsupported);
        for tag in ["lineargradient", "foreignobject", "clippath"] {
            // html5ever keeps the adjusted camel-case local name; find ignoring case.
            let hit: Vec<bool> = tree.nodes.iter().filter(|n| n.tag.eq_ignore_ascii_case(tag)).map(|n| n.style.background_color == RED).collect();
            assert_eq!(hit, [true], "{tag}");
        }
    }
    #[test]
    fn unicode_whitespace_around_selectors_does_not_trim_or_split() {
        for selector in ["p\u{a0}", "\u{a0}p", "div\u{a0}span", "div\u{a0}> span", "p\u{2003}"] {
            let html = format!("<style>{selector}{{background-color:#ff0000}}</style><div><span>x</span></div><p>y</p>");
            let tree = run(true, &html);
            assert!(tree.unsupported, "{selector:?} must refuse");
            assert!(reds(&tree, "p").iter().chain(reds(&tree, "span").iter()).all(|r| !r), "{selector:?}");
        }
        for selector in ["div\u{c}>\u{c}span", "\tdiv\t", "div\n> span"] {
            let html = format!("<style>{selector}{{background-color:#ff0000}}</style><div><span>x</span></div>");
            assert!(!run(true, &html).unsupported, "{selector:?}");
        }
    }
    #[test]
    fn specificity_counts_attributes_and_structural_pseudos_as_classes() {
        let html = "<style>p.c{background-color:#00ff00} p[a]{background-color:#ff0000} p:first-child.c{background-color:#0000ff} span{background-color:#00ff00} span:first-child{background-color:#ff0000}</style>\
            <div><p class=c a=1>x</p></div><div><span>y</span></div>";
        let tree = run(true, html);
        assert!(!tree.unsupported);
        // p[a] ties p.c on specificity and wins on source order; the third rule
        // (two classes) beats both.
        assert_eq!(tree.nodes.iter().find(|n| n.tag == "p").unwrap().style.background_color, Some(0x00_00_ff));
        assert_eq!(reds(&tree, "span"), [true]);
    }
}
