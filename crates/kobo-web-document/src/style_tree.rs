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
    /// The HTML parser selected full or limited quirks mode.
    pub quirks: bool,
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
                tree.unsupported |= unknown || !style_syntax::declarations_complete(inline);
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
            tree.unsupported |= style.line_height == Some(0);
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum MediaFit {
    Screen,
    Skip,
    Unknown,
}

/// Only unconditional screen/all and a definite print-only sheet are known.
/// Other media lists or queries might match the viewport, so paint refuses.
fn stylesheet_media(value: Option<&str>) -> MediaFit {
    match value.map(str::trim) {
        None => MediaFit::Screen,
        Some(value)
            if value.eq_ignore_ascii_case("all") || value.eq_ignore_ascii_case("screen") =>
        {
            MediaFit::Screen
        }
        Some(value) if value.eq_ignore_ascii_case("print") => MediaFit::Skip,
        _ => MediaFit::Unknown,
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
            "link" => {
                let rel = css::attribute(nodes, handle, "rel").unwrap_or("");
                if !rel
                    .split_whitespace()
                    .any(|part| part.eq_ignore_ascii_case("stylesheet"))
                    || css::attribute(nodes, handle, "disabled").is_some()
                {
                    continue;
                }
                match stylesheet_media(css::attribute(nodes, handle, "media")) {
                    MediaFit::Screen => {}
                    MediaFit::Skip => continue,
                    MediaFit::Unknown => {
                        unsupported = true;
                        continue;
                    }
                }
                // A linked sheet that the host did not supply cannot be
                // treated as an empty stylesheet. The two-sheet resource
                // ceiling also must not turn a later link into a silent skip.
                let Some(bytes) = sheets.get(linked).filter(|_| linked < 2) else {
                    unsupported = true;
                    continue;
                };
                linked += 1;
                // Replacement characters can change selectors, tokens and
                // colors. Until CSS @charset/HTTP encoding are resolved by
                // the host, only literal UTF-8 bytes can prove full paint.
                let Ok(value) = String::from_utf8(bytes.clone()) else {
                    unsupported = true;
                    continue;
                };
                value
            }
            "style" => {
                // HTML style elements with a non-CSS type do not participate
                // in the screen cascade. The exact CSS MIME token is enough
                // for this path; unknown or parameterized types remain inert.
                if css::attribute(nodes, handle, "type")
                    .is_some_and(|value| !value.trim().eq_ignore_ascii_case("text/css"))
                {
                    continue;
                }
                match stylesheet_media(css::attribute(nodes, handle, "media")) {
                    MediaFit::Screen => {}
                    MediaFit::Skip => continue,
                    MediaFit::Unknown => {
                        unsupported = true;
                        continue;
                    }
                }
                nodes[handle]
                    .children
                    .iter()
                    .filter_map(|&child| match &nodes[child].data {
                        Data::Text(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect()
            }
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
            let mut selectors = rule.prelude.split(',');
            let (properties, unknown) = properties(rule.declarations);
            unsupported |= unknown || !style_syntax::declarations_complete(rule.declarations);
            for selector in selectors.by_ref().take(16) {
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
            // A selector group beyond our bound may match a node we would
            // otherwise paint without its rule. Never present that as complete.
            unsupported |= selectors.next().is_some();
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
        // Every accepted background has no image. An explicit `none` makes
        // that invariant clear without manufacturing a color declaration or
        // changing its cascade order. Other image values remain unsupported.
        if name == "background-image" && value == "none" {
            continue;
        }
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
            "line-height" => parse_keyword(value)
                .or_else(|| {
                    if value == "normal" {
                        Some(Value::LineHeight(None))
                    } else if let Some(Length::Px(px)) = parse_width(value) {
                        (px > 0).then_some(Value::LineHeight(Some(px)))
                    } else if let Some(Length::Percent(percent)) = parse_width(value) {
                        (percent > 0).then_some(Value::LineHeightPercent(percent))
                    } else if let Some(number) = value.strip_suffix("em") {
                        // An em length computes at this element's font size,
                        // unlike an inherited unitless multiplier.
                        match parse_width(&format!("{number}%")) {
                            Some(Length::Percent(number)) if number > 0 => {
                                number.checked_mul(100).map(Value::LineHeightPercent)
                            }
                            _ => None,
                        }
                    } else if value.bytes().all(|c| c.is_ascii_digit() || c == b'.') {
                        match parse_width(&format!("{value}%")) {
                            Some(Length::Percent(number)) if number > 0 => {
                                Some(Value::LineHeightNumber(number))
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                })
                .map(|value| (Property::LineHeight, value)),
            "font-size" => parse_keyword(value)
                .or_else(|| match parse_width(value) {
                    Some(Length::Px(px)) => Some(Value::FontSize(px)),
                    _ => None, // relative sizes require parent-dependent computed values
                })
                .map(|value| (Property::FontSize, value)),
            // A single opaque color (or transparent/none) is the only
            // background shorthand this painter can account for. All image,
            // position, size, repeat, attachment, origin and clip forms remain
            // unsupported rather than silently dropping their paint effects.
            "background" => parse_keyword(value)
                .or_else(|| match value {
                    "transparent" | "none" => Some(Value::BackgroundColor(None)),
                    "currentcolor" => Some(Value::BackgroundCurrentColor),
                    _ => opaque_color(value).map(|color| Value::BackgroundColor(Some(color))),
                })
                .map(|value| (Property::BackgroundColor, value)),
            "background-color" => parse_keyword(value)
                .or_else(|| match value {
                    "transparent" => Some(Value::BackgroundColor(None)),
                    "currentcolor" => Some(Value::BackgroundCurrentColor),
                    "white" => Some(Value::BackgroundColor(Some(0xff_ff_ff))),
                    "black" => Some(Value::BackgroundColor(Some(0))),
                    _ => opaque_color(value).map(|color| Value::BackgroundColor(Some(color))),
                })
                .map(|value| (Property::BackgroundColor, value)),
            // On `color` itself currentColor resolves to the inherited color.
            // On background it remains a keyword until this element paints.
            "color" => parse_keyword(value)
                .or_else(|| (value == "currentcolor").then_some(Value::Inherit))
                .or_else(|| opaque_color(value).map(Value::Color))
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

/// Exact opaque CSS sRGB colors supported by the integer-pixel painter.
/// Alpha, system colors, relative colors and space-separated functional
/// syntax remain unsupported until compositing and Color 4 are implemented.
fn opaque_color(value: &str) -> Option<u32> {
    let named = match value {
        "black" => Some(0x00_00_00),
        "silver" => Some(0xc0_c0_c0),
        "gray" | "grey" => Some(0x80_80_80),
        "white" => Some(0xff_ff_ff),
        "maroon" => Some(0x80_00_00),
        "red" => Some(0xff_00_00),
        "purple" => Some(0x80_00_80),
        "fuchsia" => Some(0xff_00_ff),
        "green" => Some(0x00_80_00),
        "lime" => Some(0x00_ff_00),
        "olive" => Some(0x80_80_00),
        "yellow" => Some(0xff_ff_00),
        "navy" => Some(0x00_00_80),
        "blue" => Some(0x00_00_ff),
        "teal" => Some(0x00_80_80),
        "aqua" => Some(0x00_ff_ff),
        _ => None,
    };
    named.or_else(|| hex_color(value)).or_else(|| {
        let channels = value.strip_prefix("rgb(")?.strip_suffix(')')?;
        let mut parts = channels.split(',').map(str::trim);
        let component = |part: &str| {
            if part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            part.parse::<u8>().ok()
        };
        let r = component(parts.next()?)?;
        let g = component(parts.next()?)?;
        let b = component(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        Some((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
    })
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
    fn single_color_background_shorthand_cascades_and_refuses_other_components() {
        let styled = tree("<style>p{background:#123456;background-color:lime;background:green}</style><p>Text</p>", &[]);
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|node| node.tag == "p")
                .unwrap()
                .style
                .background_color,
            Some(0x00_80_00)
        );
        assert!(!styled.unsupported);
        let cleared = tree("<p style='background:red;background:none'>Text</p>", &[]);
        assert_eq!(
            cleared
                .nodes
                .iter()
                .find(|node| node.tag == "p")
                .unwrap()
                .style
                .background_color,
            None
        );
        assert!(!cleared.unsupported);
        for value in [
            "url(x)",
            "red no-repeat",
            "linear-gradient(red, blue)",
            "red padding-box",
        ] {
            assert!(tree(&format!("<p style='background:{value}'>Text</p>"), &[]).unsupported);
        }
    }

    #[test]
    fn exact_opaque_css_colors_share_foreground_and_background_parsing() {
        let styled = tree(
            "<p style='color:RED;background-color:rgb(10, 20, 255)'>Text</p>",
            &[],
        );
        let p = styled.nodes.iter().find(|node| node.tag == "p").unwrap();
        assert_eq!(p.style.color, 0xff_00_00);
        assert_eq!(p.style.background_color, Some(0x0a_14_ff));
        assert!(!styled.unsupported);
        assert_eq!(opaque_color("green"), Some(0x00_80_00));
        assert_eq!(opaque_color("lime"), Some(0x00_ff_00));
        for value in [
            "rgb(256,0,0)",
            "rgb(2.5,0,0)",
            "rgb(1,2,3,4)",
            "rgb(1 2 3)",
            "rgba(1,2,3,.5)",
        ] {
            assert_eq!(opaque_color(value), None);
            assert!(
                tree(
                    &format!("<p style='background-color:{value}'>Text</p>"),
                    &[]
                )
                .unsupported
            );
        }
    }

    #[test]
    fn oversized_selector_list_refuses_partial_style() {
        let sixteen = (0..16)
            .map(|n| format!(".c{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let within = tree(
            &format!("<style>{sixteen}{{color:#123456}}</style><p class='c15'>Text</p>"),
            &[],
        );
        assert!(!within.unsupported);
        assert_eq!(
            within
                .nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .color,
            0x12_34_56
        );
        let beyond = tree(
            &format!("<style>{sixteen},p{{color:#123456}}</style><p>Text</p>"),
            &[],
        );
        assert!(beyond.unsupported);
        assert_eq!(
            beyond
                .nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .color,
            0
        );
    }

    #[test]
    fn comment_only_declaration_segments_are_valid_css() {
        let styled = tree(
            "<style>p{/* comment */; background-color:red; /*tail*/}</style><p>Text</p>",
            &[],
        );
        assert!(!styled.unsupported);
        assert_eq!(
            styled
                .nodes
                .iter()
                .find(|node| node.tag == "p")
                .unwrap()
                .style
                .background_color,
            Some(0xff_00_00)
        );
    }

    #[test]
    fn incomplete_declaration_blocks_refuse_visual_output() {
        for html in [
            "<style>p{color:#123456;broken}</style><p>Text</p>",
            "<p style='color:#123456;broken'>Text</p>",
            "<p style='color:#123456; broken:'>Text</p>",
        ] {
            let styled = tree(html, &[]);
            assert!(styled.unsupported, "{html}");
        }
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
    fn typed_style_element_applies_only_css() {
        let html = "<style type='text/plain'>p{border:1px solid red}</style><p>Text</p>";
        let plain = tree(html, &[]);
        assert!(!plain.unsupported);
        let css = tree(
            "<style type='TEXT/CSS'>p{color:green}</style><p>Text</p>",
            &[],
        );
        assert!(!css.unsupported);
        assert_eq!(
            css.nodes
                .iter()
                .find(|node| node.tag == "p")
                .unwrap()
                .style
                .color,
            0x00_80_00
        );
        let unknown = tree(
            "<style type='text/css; charset=utf-8'>p{border:1px solid red}</style><p>Text</p>",
            &[],
        );
        assert!(!unknown.unsupported);
    }

    #[test]
    fn stylesheet_media_refuses_unknown_queries_instead_of_dropping_rules() {
        for html in [
            "<style media='screen and (min-width: 1px)'>p{color:red}</style><p>Text</p>",
            "<link rel=stylesheet media='screen, print' href=/x><p>Text</p>",
            "<link rel=stylesheet media='(min-width: 1px)' href=/x><p>Text</p>",
        ] {
            assert!(tree(html, &[]).unsupported, "{html}");
        }
        let print_only = tree(
            "<style media='print'>p{border:1px solid red}</style><p>Text</p>",
            &[],
        );
        assert!(!print_only.unsupported);
        let print_link = tree("<link rel=stylesheet media=print href=/x><p>Text</p>", &[]);
        assert!(!print_link.unsupported);
    }

    #[test]
    fn linked_stylesheet_invalid_utf8_does_not_become_lossy_css() {
        let html = "<link rel=stylesheet href=/x><p>Text</p>";
        let malformed = tree(html, &[b"p{color:#123456}\xff".to_vec()]);
        assert!(malformed.unsupported);
        let valid = tree(html, &[b"p{color:#123456}".to_vec()]);
        assert!(!valid.unsupported);
        assert_eq!(
            valid
                .nodes
                .iter()
                .find(|node| node.tag == "p")
                .unwrap()
                .style
                .color,
            0x12_34_56
        );
    }

    #[test]
    fn missing_or_excess_linked_sheets_refuse_a_partial_visual_page() {
        let missing = tree(
            "<link rel=stylesheet href=/x><p style='background-color:red'>Text</p>",
            &[],
        );
        assert!(missing.unsupported);
        let two = [b"p{color:red}".to_vec(), b"p{color:blue}".to_vec()];
        let html = "<link rel=stylesheet href=/a><link rel=stylesheet href=/b><link rel=stylesheet href=/c><p>Text</p>";
        let excess = tree(html, &two);
        assert!(excess.unsupported);
        // Non-stylesheet links are not an implicit style dependency.
        let unrelated = tree("<link rel=author href=/person><p>Text</p>", &[]);
        assert!(!unrelated.unsupported);
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

#[cfg(test)]
mod background_image_none_tests {
    use crate::{parse_style_tree, Limits};

    #[test]
    fn explicit_no_image_does_not_reset_background_color() {
        for style in [
            "background:red;background-image:none",
            "background-image:none;background-color:red",
        ] {
            let html = format!("<!doctype html><p style='{style}'>ab</p>");
            let tree = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
            assert!(!tree.unsupported);
            assert_eq!(
                tree.nodes
                    .iter()
                    .find(|n| n.tag == "p")
                    .unwrap()
                    .style
                    .background_color,
                Some(0xff_00_00)
            );
        }
        for image in [
            "url(x)",
            "linear-gradient(red,blue)",
            "none,url(x)",
            "inherit",
            "var(--image)",
        ] {
            let html = format!("<!doctype html><p style='background-image:{image}'>ab</p>");
            assert!(
                parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported,
                "{image}"
            );
        }
    }
}

#[cfg(test)]
mod currentcolor_tests {
    use crate::{parse_style_tree, Limits};

    #[test]
    fn currentcolor_inherits_as_keyword_and_uses_own_foreground() {
        let tree = parse_style_tree(b"<!doctype html><section style='color:red;background:currentcolor'><p style='color:blue;background:inherit'>ab</p><div style='color:green;background:currentcolor'>cd</div><article style='color:white;background:unset'>ef</article></section>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        let node = |tag: &str| tree.nodes.iter().find(|n| n.tag == tag).unwrap();
        assert_eq!(
            node("section").style.used_background_color(),
            Some(0xff_00_00)
        );
        assert!(node("p").style.background_current_color);
        assert_eq!(node("p").style.used_background_color(), Some(0x00_00_ff));
        assert_eq!(node("div").style.used_background_color(), Some(0x00_80_00));
        assert_eq!(node("article").style.used_background_color(), None);
    }

    #[test]
    fn currentcolor_follows_cascade_and_not_declaration_order() {
        let tree = parse_style_tree(b"<!doctype html><style>p{color:blue !important;background:currentColor}</style><p style='color:green'>ab</p><div style='background-color:currentcolor;color:#123456'>cd</div>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        let node = |tag: &str| tree.nodes.iter().find(|n| n.tag == tag).unwrap();
        assert_eq!(node("p").style.used_background_color(), Some(0x00_00_ff));
        assert_eq!(node("div").style.used_background_color(), Some(0x12_34_56));
    }
}

#[cfg(test)]
mod foreground_currentcolor_tests {
    use crate::{parse_style_tree, Limits};

    #[test]
    fn foreground_currentcolor_uses_parent_not_earlier_declaration() {
        let tree = parse_style_tree(b"<!doctype html><section style='color:red'><p style='color:blue;color:currentcolor;background:currentcolor'>ab</p><div style='color:currentcolor'>cd</div></section>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        for tag in ["p", "div"] {
            let node = tree.nodes.iter().find(|n| n.tag == tag).unwrap();
            assert_eq!(node.style.color, 0xff_00_00);
        }
        assert_eq!(
            tree.nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .used_background_color(),
            Some(0xff_00_00)
        );
    }

    #[test]
    fn foreground_currentcolor_obeys_important_and_root_initial_color() {
        let tree = parse_style_tree(b"<!doctype html><html style='color:currentcolor'><style>p{color:blue !important}</style><body style='color:red'><p style='color:currentcolor'>ab</p></body></html>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        assert_eq!(
            tree.nodes
                .iter()
                .find(|n| n.tag == "html")
                .unwrap()
                .style
                .color,
            0
        );
        assert_eq!(
            tree.nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .color,
            0x00_00_ff
        );
    }
}

#[cfg(test)]
mod pixel_line_height_tests {
    use crate::{parse_style_tree, Limits};
    #[test]
    fn pixel_line_height_inherits_and_normal_resets() {
        let tree = parse_style_tree(b"<!doctype html><section style='line-height:24px'><p style='font-size:12px'>ab</p><div style='line-height:normal'>cd</div></section>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        assert_eq!(
            tree.nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .line_height,
            Some(24)
        );
        assert_eq!(
            tree.nodes
                .iter()
                .find(|n| n.tag == "div")
                .unwrap()
                .style
                .line_height,
            None
        );
        for value in ["0", "-2px", "10.5px"] {
            let html = format!("<!doctype html><p style='line-height:{value}'>ab</p>");
            assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported);
        }
    }
}

#[cfg(test)]
mod number_line_height_tests {
    use crate::{parse_style_tree, Limits};
    #[test]
    fn unitless_line_height_is_inherited_before_font_size_multiplication() {
        let tree = parse_style_tree(b"<!doctype html><section style='font-size:20px;line-height:1.5'><p style='font-size:12px'>ab</p><div style='line-height:inherit;font-size:16px'>cd</div><article style='line-height:normal'>ef</article></section>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        let node = |tag: &str| tree.nodes.iter().find(|n| n.tag == tag).unwrap();
        assert_eq!(node("section").style.used_line_height(10), Some(30));
        assert_eq!(node("p").style.used_line_height(10), Some(18));
        assert_eq!(node("div").style.used_line_height(10), Some(24));
        assert_eq!(node("article").style.used_line_height(10), Some(10));
        for value in ["1.234", "1..5", "-1", ".", "0"] {
            let html = format!("<!doctype html><p style='line-height:{value}'>ab</p>");
            assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported);
        }
    }
}

#[cfg(test)]
mod percentage_line_height_tests {
    use crate::{parse_style_tree, Limits};
    #[test]
    fn percent_line_height_computes_at_owner_size_before_inheritance() {
        let tree = parse_style_tree(b"<!doctype html><section style='font-size:20px;line-height:150%'><p style='font-size:12px'>ab</p><div style='font-size:16px;line-height:125%'>cd</div></section>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        let node = |tag: &str| tree.nodes.iter().find(|n| n.tag == tag).unwrap();
        assert_eq!(node("section").style.line_height, Some(30));
        assert_eq!(node("p").style.used_line_height(10), Some(30));
        assert_eq!(node("div").style.used_line_height(10), Some(20));
        for style in [
            "font-size:13px;line-height:150%",
            "font-size:20px;line-height:0%",
            "font-size:4294967295px;line-height:200%",
        ] {
            let html = format!("<!doctype html><p style='{style}'>ab</p>");
            assert!(
                parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported,
                "{style}"
            );
        }
    }
}

#[cfg(test)]
mod em_line_height_tests {
    use crate::{parse_style_tree, Limits};
    #[test]
    fn em_line_height_computes_to_length_before_inheritance() {
        let tree = parse_style_tree(b"<!doctype html><section style='font-size:20px;line-height:1.5em'><p style='font-size:12px'>ab</p><div style='font-size:16px;line-height:1.25em'>cd</div></section>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        let node = |tag: &str| tree.nodes.iter().find(|n| n.tag == tag).unwrap();
        assert_eq!(node("section").style.line_height, Some(30));
        assert_eq!(node("p").style.used_line_height(10), Some(30));
        assert_eq!(node("div").style.used_line_height(10), Some(20));
        for value in ["0em", "-1em", "1.234em", "1rem", "1ex", "42949672em"] {
            let html = format!("<!doctype html><p style='line-height:{value}'>ab</p>");
            assert!(
                parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported,
                "{value}"
            );
        }
    }
}
