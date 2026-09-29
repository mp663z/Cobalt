//! A small retained, style-bearing view of the repaired HTML tree.
//!
//! The reader's semantic `Document` intentionally discards geometry. This
//! arena keeps DOM ancestry and computed values for a separate box renderer.
//! It is not painted yet. Unsupported CSS does not become a guessed layout.

use crate::computed_style::{
    BoxSizing, Computed, Declaration, Direction, Display, Length, Margin, Origin, Property, Value,
};
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
                let (parsed, unknown) = properties(inline);
                tree.unsupported |= unknown;
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
        unsupported |= parsed.truncated || parsed.malformed || parsed.unsupported_at_rule;
        for rule in parsed.rules {
            if rules.len() >= MAX_RULES {
                return (rules, true);
            }
            if !rule.conditions.is_empty() {
                unsupported = true;
                continue;
            }
            let selectors = rule.prelude.split(',').take(16);
            let (properties, unknown) = properties(rule.declarations);
            unsupported |= unknown;
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

#[allow(clippy::too_many_lines)] // Explicit supported-property parsing rejects all unknown CSS.
fn properties(body: &str) -> (Vec<(Property, Value, bool)>, bool) {
    let mut result = Vec::new();
    let mut unsupported = false;
    for (name, raw) in style_syntax::declarations(body) {
        let name = css::strip_comments(name);
        let cleaned = css::strip_comments(raw);
        let value = cleaned.trim().to_ascii_lowercase();
        let important = value.ends_with("!important");
        let value = value.strip_suffix("!important").unwrap_or(&value).trim();
        let name = name.trim().to_ascii_lowercase();
        if name == "padding" {
            if let Some(keyword) = parse_keyword(value) {
                for property in [
                    Property::PaddingTop,
                    Property::PaddingRight,
                    Property::PaddingBottom,
                    Property::PaddingLeft,
                ] {
                    result.push((property, keyword, important));
                }
                continue;
            }
            let sides: Vec<_> = value.split_ascii_whitespace().collect();
            let pixel = |value: &str| match parse_width(value) {
                Some(Length::Px(px)) => Some(px),
                _ => None,
            };
            let Some(padding) = (match sides.as_slice() {
                [top] => pixel(top).map(|top| [top; 4]),
                [top, right] => pixel(top)
                    .zip(pixel(right))
                    .map(|(top, right)| [top, right, top, right]),
                [top, right, bottom] => pixel(top)
                    .zip(pixel(right))
                    .zip(pixel(bottom))
                    .map(|((top, right), bottom)| [top, right, bottom, right]),
                [top, right, bottom, left] => pixel(top)
                    .zip(pixel(right))
                    .zip(pixel(bottom))
                    .zip(pixel(left))
                    .map(|(((top, right), bottom), left)| [top, right, bottom, left]),
                _ => None,
            }) else {
                unsupported = true;
                continue;
            };
            for (property, pixels) in [
                Property::PaddingTop,
                Property::PaddingRight,
                Property::PaddingBottom,
                Property::PaddingLeft,
            ]
            .into_iter()
            .zip(padding)
            {
                result.push((property, Value::Padding(pixels), important));
            }
            continue;
        }
        if name == "margin" {
            if let Some(keyword) = parse_keyword(value) {
                for property in [
                    Property::MarginTop,
                    Property::MarginRight,
                    Property::MarginBottom,
                    Property::MarginLeft,
                ] {
                    result.push((property, keyword, important));
                }
                continue;
            }
            // Expand the four physical sides at the declaration's position.
            // Cascade metadata (origin, importance, specificity and source
            // order) is added by the caller to each expanded property.
            let sides: Vec<_> = value.split_ascii_whitespace().collect();
            let Some(margins) = (match sides.as_slice() {
                [top] => parse_margin(top).map(|top| [top; 4]),
                [top, right] => parse_margin(top)
                    .zip(parse_margin(right))
                    .map(|(top, right)| [top, right, top, right]),
                [top, right, bottom] => parse_margin(top)
                    .zip(parse_margin(right))
                    .zip(parse_margin(bottom))
                    .map(|((top, right), bottom)| [top, right, bottom, right]),
                [top, right, bottom, left] => parse_margin(top)
                    .zip(parse_margin(right))
                    .zip(parse_margin(bottom))
                    .zip(parse_margin(left))
                    .map(|(((top, right), bottom), left)| [top, right, bottom, left]),
                _ => None,
            }) else {
                unsupported = true;
                continue;
            };
            for (property, margin) in [
                Property::MarginTop,
                Property::MarginRight,
                Property::MarginBottom,
                Property::MarginLeft,
            ]
            .into_iter()
            .zip(margins)
            {
                result.push((property, Value::Margin(margin), important));
            }
            continue;
        }
        let parsed = match name.as_str() {
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
            "font-size" => parse_keyword(value)
                .or_else(|| match parse_width(value) {
                    Some(Length::Px(px)) => Some(Value::FontSize(px)),
                    _ => None, // relative sizes require parent-dependent computed values
                })
                .map(|value| (Property::FontSize, value)),
            "background-color" => parse_keyword(value)
                .or_else(|| match value {
                    "transparent" => Some(Value::BackgroundColor(None)),
                    "white" => Some(Value::BackgroundColor(Some(0xff_ff_ff))),
                    "black" => Some(Value::BackgroundColor(Some(0))),
                    _ => hex_color(value).map(|color| Value::BackgroundColor(Some(color))),
                })
                .map(|value| (Property::BackgroundColor, value)),
            "color" => parse_keyword(value)
                .or_else(|| hex_color(value).map(Value::Color))
                .map(|value| (Property::Color, value)),
            "width" => parse_keyword(value)
                .or_else(|| parse_width(value).map(Value::Width))
                .map(|value| (Property::Width, value)),
            "height" => parse_keyword(value)
                .or_else(|| parse_width(value).map(Value::Height))
                .map(|value| (Property::Height, value)),
            "box-sizing" => parse_keyword(value)
                .or(match value {
                    "content-box" => Some(Value::BoxSizing(BoxSizing::ContentBox)),
                    "border-box" => Some(Value::BoxSizing(BoxSizing::BorderBox)),
                    _ => None,
                })
                .map(|value| (Property::BoxSizing, value)),
            "padding-top" => parse_keyword(value)
                .or_else(|| {
                    parse_width(value).and_then(|length| match length {
                        Length::Px(px) => Some(Value::Padding(px)),
                        _ => None,
                    })
                })
                .map(|value| (Property::PaddingTop, value)),
            "padding-bottom" => parse_keyword(value)
                .or_else(|| {
                    parse_width(value).and_then(|length| match length {
                        Length::Px(px) => Some(Value::Padding(px)),
                        _ => None,
                    })
                })
                .map(|value| (Property::PaddingBottom, value)),
            "padding-left" => parse_keyword(value)
                .or_else(|| {
                    parse_width(value).and_then(|length| match length {
                        Length::Px(px) => Some(Value::Padding(px)),
                        _ => None,
                    })
                })
                .map(|value| (Property::PaddingLeft, value)),
            "padding-right" => parse_keyword(value)
                .or_else(|| {
                    parse_width(value).and_then(|length| match length {
                        Length::Px(px) => Some(Value::Padding(px)),
                        _ => None,
                    })
                })
                .map(|value| (Property::PaddingRight, value)),
            "margin-left" => parse_keyword(value)
                .or_else(|| parse_margin(value).map(Value::Margin))
                .map(|value| (Property::MarginLeft, value)),
            "margin-right" => parse_keyword(value)
                .or_else(|| parse_margin(value).map(Value::Margin))
                .map(|value| (Property::MarginRight, value)),
            "margin-top" => parse_keyword(value)
                .or_else(|| parse_margin(value).map(Value::Margin))
                .map(|value| (Property::MarginTop, value)),
            "margin-bottom" => parse_keyword(value)
                .or_else(|| parse_margin(value).map(Value::Margin))
                .map(|value| (Property::MarginBottom, value)),
            "direction" => parse_keyword(value)
                .or(match value {
                    "ltr" => Some(Value::Direction(Direction::Ltr)),
                    "rtl" => Some(Value::Direction(Direction::Rtl)),
                    _ => None,
                })
                .map(|value| (Property::Direction, value)),
            _ => None,
        };
        if let Some((property, value)) = parsed {
            result.push((property, value, important));
        } else {
            unsupported = true;
        }
    }
    (result, unsupported)
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

fn parse_margin(value: &str) -> Option<Margin> {
    if value == "auto" {
        return Some(Margin::Auto);
    }
    let (negative, value) = value
        .strip_prefix('-')
        .map_or((false, value), |rest| (true, rest));
    let length = parse_width(value)?;
    let sign = if negative { -1 } else { 1 };
    match length {
        Length::Auto => None,
        Length::Px(px) => Some(Margin::Px(i32::try_from(px).ok()?.checked_mul(sign)?)),
        Length::Percent(percent) => Some(Margin::Percent(
            i32::try_from(percent).ok()?.checked_mul(sign)?,
        )),
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
    fn unknown_at_rules_fail_closed_without_leaking_following_rules() {
        for html in [
            "<style>@import url('x.css'); p{background-color:#123456}</style><p>Text</p>",
            "<style>@font-face{font-family:x;src:url('x.woff')} p{background-color:#123456}</style><p>Text</p>",
        ] {
            let styled = tree(html, &[]);
            assert!(styled.unsupported, "{html}");
            assert_eq!(styled.nodes.iter().find(|n| n.tag == "p").unwrap().style.background_color, Some(0x12_34_56));
        }
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
    fn padding_shorthand_expands_in_cascade_order_and_rejects_percent() {
        let styled = tree("<style>p{padding:1px 2px 3px 4px;padding-left:7px}</style><p style='padding:5px 6px'>Text</p>", &[]);
        let p = styled.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!(
            (
                p.style.padding_top,
                p.style.padding_right,
                p.style.padding_bottom,
                p.style.padding_left
            ),
            (5, 6, 5, 6)
        );
        assert!(!styled.unsupported);
        let styled = tree(
            "<div style='padding:7px'><p style='padding:inherit;padding-left:9px'>Text</p></div>",
            &[],
        );
        let p = styled.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!(
            (
                p.style.padding_top,
                p.style.padding_right,
                p.style.padding_bottom,
                p.style.padding_left
            ),
            (7, 7, 7, 9)
        );
        assert!(!styled.unsupported);
        assert!(tree("<p style='padding:10%'>Text</p>", &[]).unsupported);
        assert!(tree("<p style='padding:1px 2px 3px 4px 5px'>Text</p>", &[]).unsupported);
    }

    #[test]
    fn margin_shorthand_expands_in_cascade_order_and_refuses_unknown_values() {
        let styled = tree("<style>p{margin:1px 2px 3px 4px;margin-left:7px}</style><p style='margin:5px 6px'>Text</p>", &[]);
        let p = styled.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!(
            (
                p.style.margin_top,
                p.style.margin_right,
                p.style.margin_bottom,
                p.style.margin_left
            ),
            (Margin::Px(5), Margin::Px(6), Margin::Px(5), Margin::Px(6))
        );
        assert!(!styled.unsupported);
        let styled = tree("<p style='margin:0;margin-left:9px'>Text</p>", &[]);
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .margin_left,
            Margin::Px(9)
        );
        assert!(!styled.unsupported);
        let styled = tree(
            "<div style='margin:7px'><p style='margin:inherit'>Text</p></div>",
            &[],
        );
        let p = styled.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!(
            [
                p.style.margin_top,
                p.style.margin_right,
                p.style.margin_bottom,
                p.style.margin_left
            ],
            [Margin::Px(7); 4]
        );
        assert!(!styled.unsupported);
        assert!(tree("<p style='margin:1em'>Text</p>", &[]).unsupported);
        assert!(tree("<p style='margin:1px 2px 3px 4px 5px'>Text</p>", &[]).unsupported);
    }

    #[test]
    fn vertical_margins_are_signed_and_noninherited() {
        let styled = tree("<div style='margin-top:10%;margin-bottom:-6px'><p style='margin-top:auto'>child</p></div>", &[]);
        let div = styled.nodes.iter().find(|n| n.tag == "div").unwrap();
        let p = styled.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!(div.style.margin_top, Margin::Percent(1000));
        assert_eq!(div.style.margin_bottom, Margin::Px(-6));
        assert_eq!(p.style.margin_top, Margin::Auto);
        assert_eq!(p.style.margin_bottom, Margin::Px(0));
    }

    #[test]
    fn horizontal_margins_and_direction_have_distinct_inheritance() {
        let styled = tree("<div style='direction:rtl;margin-left:-25%;margin-right:auto'><section style='margin-left:7px'>child</section></div>", &[]);
        let div = styled.nodes.iter().find(|n| n.tag == "div").unwrap();
        let section = styled.nodes.iter().find(|n| n.tag == "section").unwrap();
        assert_eq!(div.style.direction, Direction::Rtl);
        assert_eq!(div.style.margin_left, Margin::Percent(-2500));
        assert_eq!(div.style.margin_right, Margin::Auto);
        assert_eq!(section.style.direction, Direction::Rtl);
        assert_eq!(section.style.margin_left, Margin::Px(7));
        assert_eq!(section.style.margin_right, Margin::Px(0));
    }

    #[test]
    fn font_size_is_retained_for_future_text_layout() {
        let styled = tree("<html style='font-size:18px'><body><p style='font-size:22px'>Words</p><div style='font-size:1.5em'>Unsupported</div></body></html>", &[]);
        let node = |tag| styled.nodes.iter().find(|n| n.tag == tag).unwrap();
        assert_eq!(node("html").style.font_size, 18);
        assert_eq!(node("body").style.font_size, 18);
        assert_eq!(node("p").style.font_size, 22);
        assert_eq!(node("div").style.font_size, 18);
        assert!(styled.unsupported); // em is not misread as px
    }

    #[test]
    fn horizontal_padding_is_noninherited_and_bounded_to_integer_px() {
        let styled = tree("<div style='padding-left:12px;padding-right:5px'><p style='padding-left:25%'>child</p></div>", &[]);
        assert!(styled.unsupported); // percentage padding needs containing width resolution
        let div = styled.nodes.iter().find(|n| n.tag == "div").unwrap();
        let child = styled.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!((div.style.padding_left, div.style.padding_right), (12, 5));
        assert_eq!(
            (child.style.padding_left, child.style.padding_right),
            (0, 0)
        );
    }

    #[test]
    fn vertical_padding_is_noninherited_and_rejects_percentage() {
        let styled = tree("<div style='padding-top:12px;padding-bottom:5px'><p style='padding-top:25%'>child</p></div>", &[]);
        assert!(styled.unsupported);
        let div = styled.nodes.iter().find(|n| n.tag == "div").unwrap();
        let child = styled.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!((div.style.padding_top, div.style.padding_bottom), (12, 5));
        assert_eq!(
            (child.style.padding_top, child.style.padding_bottom),
            (0, 0)
        );
    }

    #[test]
    fn background_color_is_noninherited_and_transparent_by_default() {
        let styled = tree("<div style='background-color:#123456'><p style='background-color:transparent'>A</p><section style='background-color:white'>B</section></div>", &[]);
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|n| n.tag == "div")
                .unwrap()
                .style
                .background_color,
            Some(0x12_34_56)
        );
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .background_color,
            None
        );
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|n| n.tag == "section")
                .unwrap()
                .style
                .background_color,
            Some(0xff_ff_ff)
        );
    }

    #[test]
    fn box_sizing_cascade_is_noninherited() {
        let styled = tree("<style>div{box-sizing:border-box!important}</style><div style='box-sizing:content-box'><section>child</section></div>", &[]);
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|n| n.tag == "div")
                .unwrap()
                .style
                .box_sizing,
            BoxSizing::BorderBox
        );
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|n| n.tag == "section")
                .unwrap()
                .style
                .box_sizing,
            BoxSizing::ContentBox
        );
    }

    #[test]
    fn height_cascade_keeps_percentage_until_layout() {
        let styled = tree("<style>p{height:50%;height:12px!important}</style><p style='height:75px'>A</p><p style='height:25% !important'>B</p><div style='height:1.5px'>C</div>", &[]);
        let paragraphs: Vec<_> = styled.nodes.iter().filter(|n| n.tag == "p").collect();
        assert_eq!(paragraphs[0].style.height, Length::Px(12));
        assert_eq!(paragraphs[1].style.height, Length::Percent(2500));
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|n| n.tag == "div")
                .unwrap()
                .style
                .height,
            Length::Auto
        );
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
