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
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Selector {
    // Rightmost first. Relation says how the next step to the left connects.
    steps: Vec<(Simple, Relation)>,
    pub(crate) specificity: (u16, u16, u16),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Relation {
    Descendant,
    Child,
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
    if input.is_empty() || input.len() > 256 || input.contains([':', '[', ']', '+', '~', '*', '|'])
    {
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
                    classes + u16::try_from(part.classes.len()).unwrap_or(8),
                    types + u16::from(part.tag.is_some()),
                )
            });
    parsed.reverse();
    Some(Selector {
        steps: parsed,
        specificity,
    })
}
fn simple(part: &str) -> Option<Simple> {
    let mut tag = None;
    let mut id = None;
    let mut classes = Vec::new();
    let mut offset = 0;
    while offset < part.len() {
        let marker = part.as_bytes()[offset];
        let end = part[offset + usize::from(marker == b'#' || marker == b'.')..]
            .find(['#', '.'])
            .map_or(part.len(), |at| {
                offset + usize::from(marker == b'#' || marker == b'.') + at
            });
        let token = &part[offset + usize::from(marker == b'#' || marker == b'.')..end];
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
            b if b.is_ascii_alphabetic() && offset == 0 && tag.is_none() => {
                tag = Some(token.to_ascii_lowercase());
            }
            _ => return None,
        }
        offset = end;
    }
    Some(Simple { tag, id, classes })
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
        if self
            .tag
            .as_ref()
            .is_some_and(|tag| name.local.as_ref() != tag)
        {
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
            attribute(nodes, handle, "class")
                .is_some_and(|value| value.split_whitespace().any(|word| word == class))
        })
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
