//! A small retained, style-bearing view of the repaired HTML tree.
//!
//! The reader's semantic `Document` intentionally discards geometry. This
//! arena keeps DOM ancestry and computed values for a separate box renderer.
//! It is not painted yet. Unsupported CSS does not become a guessed layout.

use crate::computed_style::{Computed, Declaration, Display, Length, Origin, Property, Value};
use crate::css;
use crate::dom::{Data, Node, DOCUMENT};
use crate::style_syntax;

const MAX_DECLARATIONS_PER_ELEMENT: usize = 256;
const MAX_RULES: usize = 4096;
const MAX_MATCH_WORK: usize = 4_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyledNode {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub tag: String,
    /// Text content for a retained #text node.
    pub text: Option<String>,
    pub style: Computed,
}

#[derive(Default)]
pub struct StyleTree {
    pub nodes: Vec<StyledNode>,
    pub truncated: bool,
    pub unsupported: bool,
}

struct MatchedRule {
    selector: css::Selector,
    property: Property,
    value: Value,
    important: bool,
    source_order: u32,
}

impl StyleTree {
    /// Compute a bounded style arena from the browser's HTML5 tree. This is a
    /// separate path: current reader conversion stays unchanged.
    #[allow(clippy::too_many_lines)] // One bounded tree walk retains parentage and cascade.
    pub(crate) fn from_dom(nodes: &[Node], sheets: &[Vec<u8>]) -> Self {
        let mut tree = Self::default();
        let (rules, incomplete) = matched_rules(nodes, sheets);
        tree.unsupported = incomplete;
        let mut stack = vec![(DOCUMENT, None::<usize>, Computed::INITIAL)];
        let mut match_work = 0_usize;
        while let Some((handle, parent, inherited)) = stack.pop() {
            if let Data::Text(text) = &nodes[handle].data {
                if tree.nodes.len() >= nodes.len() {
                    tree.truncated = true;
                    break;
                }
                let index = tree.nodes.len();
                tree.nodes.push(StyledNode {
                    parent,
                    children: Vec::new(),
                    tag: "#text".to_owned(),
                    text: Some(text.clone()),
                    style: inherited,
                });
                if let Some(parent) = parent {
                    tree.nodes[parent].children.push(index);
                }
                continue;
            }
            let Data::Element { name, .. } = &nodes[handle].data else {
                for &child in nodes[handle].children.iter().rev() {
                    stack.push((child, parent, inherited));
                }
                continue;
            };
            if tree.nodes.len() >= nodes.len() || match_work >= MAX_MATCH_WORK {
                tree.truncated = true;
                break;
            }
            let mut declarations = Vec::new();
            for rule in &rules {
                match_work += 1;
                if match_work > MAX_MATCH_WORK {
                    tree.truncated = true;
                    break;
                }
                if rule.selector.matches(nodes, handle) {
                    declarations.push(Declaration {
                        property: rule.property,
                        value: rule.value,
                        origin: Origin::Author,
                        important: rule.important,
                        specificity: rule.selector.specificity,
                        source_order: rule.source_order,
                    });
                    if declarations.len() == MAX_DECLARATIONS_PER_ELEMENT {
                        tree.truncated = true;
                        break;
                    }
                }
            }
            if tree.truncated {
                break;
            }
            if let Some(inline) = css::attribute(nodes, handle, "style") {
                let parsed = properties(inline);
                if declarations.len() + parsed.len() > MAX_DECLARATIONS_PER_ELEMENT {
                    tree.truncated = true;
                    break;
                }
                declarations.extend(parsed.into_iter().enumerate().map(
                    |(offset, (property, value, important))| Declaration {
                        property,
                        value,
                        origin: Origin::Author,
                        important,
                        specificity: (1, 0, 0),
                        source_order: u32::try_from(offset).unwrap_or(u32::MAX),
                    },
                ));
            }
            declarations.push(Declaration {
                property: Property::Display,
                value: Value::Display(ua_display(&name.local)),
                origin: Origin::UserAgent,
                important: false,
                specificity: (0, 0, 0),
                source_order: 0,
            });
            let style = Computed::cascade(Some(inherited), &declarations);
            let index = tree.nodes.len();
            tree.nodes.push(StyledNode {
                parent,
                children: Vec::new(),
                tag: name.local.to_string(),
                text: None,
                style,
            });
            if let Some(parent) = parent {
                tree.nodes[parent].children.push(index);
            }
            for &child in nodes[handle].children.iter().rev() {
                stack.push((child, Some(index), style));
            }
        }
        tree
    }
}

fn ua_display(tag: &str) -> Display {
    match tag {
        "html" | "body" | "main" | "article" | "section" | "header" | "footer" | "aside"
        | "nav" | "div" | "p" | "blockquote" | "pre" | "figure" | "figcaption" | "form"
        | "fieldset" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Display::Block,
        "li" => Display::ListItem,
        "head" | "title" | "meta" | "link" | "style" | "script" | "template" => Display::None,
        _ => Display::Inline,
    }
}

fn matched_rules(nodes: &[Node], sheets: &[Vec<u8>]) -> (Vec<MatchedRule>, bool) {
    let mut rules = Vec::new();
    let mut unsupported = false;
    let mut linked = 0;
    let mut stack = vec![DOCUMENT];
    while let Some(handle) = stack.pop() {
        stack.extend(nodes[handle].children.iter().rev());
        let Data::Element { name, .. } = &nodes[handle].data else {
            continue;
        };
        let body = match &*name.local {
            "link" if linked < sheets.len() && linked < 2 => {
                let rel = css::attribute(nodes, handle, "rel").unwrap_or("");
                if !rel
                    .split_whitespace()
                    .any(|part| part.eq_ignore_ascii_case("stylesheet"))
                    || css::attribute(nodes, handle, "disabled").is_some()
                    || !matches!(
                        css::attribute(nodes, handle, "media"),
                        None | Some("all" | "screen")
                    )
                {
                    continue;
                }
                let value = String::from_utf8_lossy(&sheets[linked]).into_owned();
                linked += 1;
                value
            }
            "style" => nodes[handle]
                .children
                .iter()
                .filter_map(|&child| match &nodes[child].data {
                    Data::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect(),
            _ => continue,
        };
        let parsed = style_syntax::scan(&body);
        unsupported |= parsed.truncated || parsed.malformed;
        for rule in parsed.rules {
            if rules.len() >= MAX_RULES {
                return (rules, true);
            }
            if !rule.conditions.is_empty() {
                unsupported = true;
                continue;
            }
            let selectors = rule.prelude.split(',').take(16);
            let properties = properties(rule.declarations);
            for selector in selectors {
                let Some(selector) = css::selector(selector.trim()) else {
                    unsupported = true;
                    continue;
                };
                for &(property, value, important) in &properties {
                    rules.push(MatchedRule {
                        selector: selector.clone(),
                        property,
                        value,
                        important,
                        source_order: u32::try_from(rules.len()).unwrap_or(u32::MAX),
                    });
                    if rules.len() >= MAX_RULES {
                        return (rules, true);
                    }
                }
            }
        }
    }
    (rules, unsupported)
}

fn properties(body: &str) -> Vec<(Property, Value, bool)> {
    let mut result = Vec::new();
    for (name, raw) in style_syntax::declarations(body) {
        let name = css::strip_comments(name);
        let cleaned = css::strip_comments(raw);
        let value = cleaned.trim().to_ascii_lowercase();
        let important = value.ends_with("!important");
        let value = value.strip_suffix("!important").unwrap_or(&value).trim();
        let parsed = match name.trim().to_ascii_lowercase().as_str() {
            "display" => parse_keyword(value)
                .or(match value {
                    "none" => Some(Value::Display(Display::None)),
                    "block" => Some(Value::Display(Display::Block)),
                    "inline" => Some(Value::Display(Display::Inline)),
                    "inline-block" => Some(Value::Display(Display::InlineBlock)),
                    "list-item" => Some(Value::Display(Display::ListItem)),
                    _ => None,
                })
                .map(|value| (Property::Display, value)),
            "color" => parse_keyword(value)
                .or_else(|| hex_color(value).map(Value::Color))
                .map(|value| (Property::Color, value)),
            "width" => parse_keyword(value)
                .or_else(|| parse_width(value).map(Value::Width))
                .map(|value| (Property::Width, value)),
            _ => None,
        };
        if let Some((property, value)) = parsed {
            result.push((property, value, important));
        }
    }
    result
}

fn parse_keyword(value: &str) -> Option<Value> {
    match value {
        "inherit" => Some(Value::Inherit),
        "initial" => Some(Value::Initial),
        "unset" => Some(Value::Unset),
        "revert" => Some(Value::Revert),
        _ => None,
    }
}

fn parse_width(value: &str) -> Option<Length> {
    if value == "auto" {
        return Some(Length::Auto);
    }
    let (number, percent) = if let Some(n) = value.strip_suffix("px") {
        (n, false)
    } else if let Some(n) = value.strip_suffix('%') {
        (n, true)
    } else {
        return (value == "0").then_some(Length::Px(0));
    };
    let (whole, fractional) = number.split_once('.').unwrap_or((number, ""));
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || fractional.len() > 2
        || !fractional.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let base = whole.parse::<u32>().ok()?;
    let mut value = base.checked_mul(100)?;
    if !fractional.is_empty() {
        value = value.checked_add(
            fractional.parse::<u32>().ok()? * if fractional.len() == 1 { 10 } else { 1 },
        )?;
    }
    if percent {
        Some(Length::Percent(value))
    } else if value % 100 == 0 {
        Some(Length::Px(value / 100))
    } else {
        // Fractional px needs fixed-point support in used values.
        None
    }
}

fn hex_color(value: &str) -> Option<u32> {
    let digits = value.strip_prefix('#')?;
    match digits.len() {
        3 => {
            let mut value = 0;
            for ch in digits.chars() {
                let n = ch.to_digit(16)?;
                value = (value << 8) | (n << 4) | n;
            }
            Some(value)
        }
        6 => u32::from_str_radix(digits, 16).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dom, Limits};
    use html5ever::tendril::TendrilSink;

    fn tree(html: &str, sheets: &[Vec<u8>]) -> StyleTree {
        let parser = html5ever::parse_document(
            dom::Sink::new(Limits::DEFAULT.max_text_bytes),
            html5ever::ParseOpts::default(),
        );
        let nodes = parser.one(html).into_nodes();
        StyleTree::from_dom(&nodes, sheets)
    }

    #[test]
    fn parentage_inheritance_and_display_are_retained() {
        let styled = tree("<style>section {display:block;color:#123456} section > p {display:none}</style><section><p>Words</p></section>", &[]);
        let section = styled
            .nodes
            .iter()
            .position(|node| node.tag == "section")
            .unwrap();
        let paragraph = styled
            .nodes
            .iter()
            .position(|node| node.tag == "p")
            .unwrap();
        assert_eq!(styled.nodes[paragraph].parent, Some(section));
        assert_eq!(styled.nodes[paragraph].style.color, 0x12_34_56);
        assert_eq!(styled.nodes[paragraph].style.display, Display::None);
        assert!(!styled.truncated);
    }

    #[test]
    fn external_and_inline_order_share_one_cascade() {
        let css = [b".item{display:none}".to_vec()];
        let visible = tree("<link rel=stylesheet href=/x><style>.item{display:block}</style><p class=item>Words</p>", &css);
        let hidden = tree("<style>.item{display:block}</style><link rel=stylesheet href=/x><p class=item>Words</p>", &css);
        assert_eq!(
            visible
                .nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .display,
            Display::Block
        );
        assert_eq!(
            hidden
                .nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .display,
            Display::None
        );
    }
}
