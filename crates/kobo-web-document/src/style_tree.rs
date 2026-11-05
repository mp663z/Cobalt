//! A small retained, style-bearing view of the repaired HTML tree.
//!
//! The reader's semantic `Document` intentionally discards geometry. This
//! arena keeps DOM ancestry and computed values for a separate box renderer.
//! It is not painted yet. Unsupported CSS does not become a guessed layout.

use crate::computed_style::{
    AlignItems, BorderColor, BoxSizing, Clip, Computed, Declaration, Direction, Display, FontStyle, JustifyContent, Length,
    Margin, Origin, Position, Property, TextAlign, Value,
};
use crate::css;
use crate::dom::{Data, Node, DOCUMENT};
use crate::style_diagnostics::{RefusalReason, StyleDiagnostics};
use crate::style_syntax;

const MAX_DECLARATIONS_PER_ELEMENT: usize = 256;
const MAX_RULES: usize = 4096;
const MAX_MATCH_WORK: usize = 4_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyledNode {
    /// Original retained non-replaced phrase with no event/editing attributes.
    pub inline_phrase: bool,
    /// Retained attributes are safe for a legacy direct single-text inline.
    pub inline_attributes: bool,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub tag: String,
    /// Text content for a retained #text node.
    pub text: Option<String>,
    pub style: Computed,
}

#[derive(Default)]
pub struct StyleTree {
    pub diagnostics: StyleDiagnostics,
    pub nodes: Vec<StyledNode>,
    pub truncated: bool,
    pub unsupported: bool,
    /// The HTML parser selected full or limited quirks mode.
    pub quirks: bool,
    /// Host-path stylesheet links the engine claimed while walking the
    /// document with its live link classifier (media fit, `disabled`, retained
    /// data: hrefs and the two-sheet ceiling included). `Some` only when the
    /// link walk finished with no refusal anywhere, so the count is the full
    /// host-path inventory of the parsed document; `None` otherwise.
    pub host_sheets_claimed: Option<usize>,
}

struct MatchedRule {
    selector: css::Selector,
    property: Property,
    value: Value,
    important: bool,
    source_order: u32,
}

/// Stylesheet supply budget for the style-tree entries. `CANONICAL` equals the
/// literals this crate used before the budget existed. The aggregate gate is
/// NEW: its canonical value is DERIVED (sheets x per-sheet) and can never fire
/// under CANONICAL. Input bytes stay in `Limits`, one owner. Candidate
/// profiles are host-measure only, not a device budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupplyBudget {
    pub max_linked_sheets: usize,
    pub max_sheet_bytes: usize,
    pub max_aggregate_bytes: usize,
}

impl SupplyBudget {
    pub const CANONICAL: Self = Self {
        max_linked_sheets: 2,
        max_sheet_bytes: style_syntax::MAX_BYTES,
        max_aggregate_bytes: 2 * style_syntax::MAX_BYTES,
    };
}

impl StyleTree {
    /// Compute a bounded style arena from the browser's HTML5 tree. This is a
    /// separate path: current reader conversion stays unchanged.
    #[allow(clippy::too_many_lines)] // One bounded tree walk retains parentage and cascade.
    #[cfg(test)]
    pub(crate) fn from_dom(
        nodes: &[Node],
        sheets: &[Vec<u8>],
        media: Option<crate::media_query::MediaContext>,
    ) -> Self {
        Self::from_dom_with(nodes, sheets, media, SupplyBudget::CANONICAL)
    }

    #[allow(clippy::too_many_lines)] // One bounded tree walk retains parentage and cascade.
    pub(crate) fn from_dom_with(
        nodes: &[Node],
        sheets: &[Vec<u8>],
        media: Option<crate::media_query::MediaContext>,
        budget: SupplyBudget,
    ) -> Self {
        let mut tree = Self::default();
        let (rules, incomplete, claimed) =
            matched_rules(nodes, sheets, &mut tree.diagnostics, media, budget);
        tree.host_sheets_claimed = (!incomplete).then_some(claimed);
        tree.diagnostics.expanded_declarations = rules.len();
        tree.unsupported = incomplete;
        let mut stack = vec![(DOCUMENT, None::<usize>, Computed::INITIAL)];
        let mut match_work = 0_usize;
        let mut root_font_px: u32 = 16;
        while let Some((handle, parent, inherited)) = stack.pop() {
            if let Data::Text(text) = &nodes[handle].data {
                if tree.nodes.len() >= nodes.len() {
                    tree.truncated = true;
                    tree.diagnostics.record(RefusalReason::RetainedNodes);
                    break;
                }
                let index = tree.nodes.len();
                tree.nodes.push(StyledNode {
                    inline_phrase: false,
                    inline_attributes: false,
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
            let Data::Element { name, attrs, .. } = &nodes[handle].data else {
                for &child in nodes[handle].children.iter().rev() {
                    stack.push((child, parent, inherited));
                }
                continue;
            };
            if tree.nodes.len() >= nodes.len() || match_work >= MAX_MATCH_WORK {
                tree.diagnostics.record(if match_work >= MAX_MATCH_WORK {
                    RefusalReason::MatchWork
                } else {
                    RefusalReason::RetainedNodes
                });
                tree.truncated = true;
                break;
            }
            let mut declarations = Vec::new();
            for rule in &rules {
                match_work += 1;
                if match_work > MAX_MATCH_WORK {
                    tree.diagnostics.record(RefusalReason::MatchWork);
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
                        tree.diagnostics.record(RefusalReason::MatchedDeclarations);
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
                let refused = unknown || !style_syntax::declarations_complete(inline);
                if refused {
                    tree.diagnostics.record(RefusalReason::CssDeclaration);
                }
                tree.unsupported |= refused;
                if declarations.len() + parsed.len() > MAX_DECLARATIONS_PER_ELEMENT {
                    tree.diagnostics.record(RefusalReason::InlineDeclarations);
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
            if name.local.as_ref() == "p" {
                // The HTML user-agent sheet gives paragraphs 1em vertical
                // margins; the cascade resolves the em against each
                // paragraph's own computed font size.
                for property in [Property::MarginTop, Property::MarginBottom] {
                    declarations.push(Declaration {
                        property,
                        value: Value::Margin(Margin::Em(10_000)),
                        origin: Origin::UserAgent,
                        important: false,
                        specificity: (0, 0, 0),
                        source_order: 0,
                    });
                }
            }
            // The HTML user-agent sheet styles phrase elements: strong/b
            // are bold, em/i are italic. Only weight 400/700 has a real
            // face pair downstream; italic is computed, then refused by the
            // restricted painter rather than rendered upright.
            if matches!(name.local.as_ref(), "strong" | "b") {
                declarations.push(Declaration {
                    property: Property::FontWeight,
                    value: Value::FontWeight(700),
                    origin: Origin::UserAgent,
                    important: false,
                    specificity: (0, 0, 0),
                    source_order: 0,
                });
            }
            if matches!(name.local.as_ref(), "em" | "i") {
                declarations.push(Declaration {
                    property: Property::FontStyle,
                    value: Value::FontStyle(FontStyle::Italic),
                    origin: Origin::UserAgent,
                    important: false,
                    specificity: (0, 0, 0),
                    source_order: 0,
                });
            }
            if name.local.as_ref() == "body" {
                // The HTML user-agent sheet gives the body an 8px margin on
                // every side (Chrome, Firefox and WebKit all agree).
                for property in [
                    Property::MarginTop,
                    Property::MarginRight,
                    Property::MarginBottom,
                    Property::MarginLeft,
                ] {
                    declarations.push(Declaration {
                        property,
                        value: Value::Margin(Margin::Px(8)),
                        origin: Origin::UserAgent,
                        important: false,
                        specificity: (0, 0, 0),
                        source_order: 0,
                    });
                }
            }
            let mut rem_refused = false;
            for declaration in &mut declarations {
                // MEASUREMENT-ONLY rem lengths: margin/padding against the root computed font size.
                match declaration.value {
                    Value::MarginRem(tt) => {
                        let product = i64::from(tt) * i64::from(root_font_px);
                        match i32::try_from(product / 10_000) {
                            Ok(px) if product % 10_000 == 0 => {
                                declaration.value = Value::Margin(Margin::Px(px));
                            }
                            _ => rem_refused = true,
                        }
                    }
                    Value::Padding(v)
                        if v & crate::computed_style::PADDING_REM_FLAG != 0
                            && v & crate::computed_style::PADDING_EM_FLAG == 0
                            && matches!(
                                declaration.property,
                                Property::PaddingTop | Property::PaddingRight | Property::PaddingBottom | Property::PaddingLeft
                            ) =>
                    {
                        let product = u64::from(v & !crate::computed_style::PADDING_REM_FLAG) * u64::from(root_font_px);
                        declaration.value = Value::Padding(if product % 10_000 == 0 {
                            u32::try_from(product / 10_000)
                                .ok()
                                .filter(|px| *px < crate::computed_style::PADDING_REM_FLAG)
                                .unwrap_or(crate::computed_style::PADDING_UNREPRESENTABLE)
                        } else {
                            crate::computed_style::PADDING_UNREPRESENTABLE
                        });
                    }
                    _ => {}
                }
                if let Value::FontSizeRem(rem) = declaration.value {
                    // rem is the root computed font size (initial 16px on the root itself).
                    // Only a whole pixel result is representable; anything else computes
                    // to 0 and is refused as ComputedValue.
                    let product = rem.checked_mul(root_font_px);
                    declaration.value = Value::FontSize(
                        product.filter(|p| p % 10_000 == 0).map_or(0, |p| p / 10_000),
                    );
                }
            }
            let style = Computed::cascade(Some(inherited), &declarations);
            if parent.is_none() {
                root_font_px = style.font_size;
            }
            let refused = rem_refused
                || style.line_height == Some(0)
                || style.font_size == 0
                || [style.padding_top, style.padding_right, style.padding_bottom, style.padding_left]
                    .contains(&crate::computed_style::PADDING_UNREPRESENTABLE);
            if refused {
                tree.diagnostics.record(RefusalReason::ComputedValue);
            }
            tree.unsupported |= refused;
            let index = tree.nodes.len();
            tree.nodes.push(StyledNode {
                inline_phrase: matches!(
                    name.local.as_ref(),
                    "span" | "b" | "strong" | "em" | "i" | "code" | "a"
                ) && attrs.iter().all(|(attr, _)| {
                    matches!(attr.as_ref(), "id" | "class" | "style" | "title")
                        || (name.local.as_ref() == "a" && attr.as_ref() == "href")
                }),
                // Legacy direct single-text inline shapes ignored inert attrs.
                // Keep that contract except event/editing behavior deliberately
                // excluded from this picture-only proposal. New nested shapes
                // still require the strict inline_phrase allowlist above.
                inline_attributes: attrs.iter().all(|(attr, _)| {
                    !attr.as_ref().starts_with("on") && attr.as_ref() != "contenteditable"
                }),
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
        tree.diagnostics.retained_nodes = tree.nodes.len();
        tree.diagnostics.match_attempts = match_work;
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

/// Link and `<style>` media attribute, evaluated by the same bounded
/// evaluator as in-sheet `@media` (`media_query::evaluate`): no new grammar.
/// An absent attribute is Screen. An empty or whitespace-only value keeps its
/// previous fail-closed Unknown result (it is not reinterpreted as "all").
/// Pure type lists use `media_fit` (Unknown dominates a known true member);
/// width features are exact rational bounds against the supplied viewport;
/// any '(' without a `MediaContext` is Unknown (fail closed); anything else
/// (prefers-color-scheme, commas mixed with features, comments, more than
/// eight features) is Unknown and refuses. A known-false sheet is skipped and
/// consumes neither a linked slot nor a host index. Width-dependent results
/// mean a different viewport width needs a fresh parse.
fn stylesheet_media(
    value: Option<&str>,
    media: Option<crate::media_query::MediaContext>,
) -> MediaFit {
    let Some(value) = value else {
        return MediaFit::Screen;
    };
    if value
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .is_empty()
    {
        return MediaFit::Unknown;
    }
    match crate::media_query::evaluate(value, media) {
        style_syntax::MediaFit::Screen => MediaFit::Screen,
        style_syntax::MediaFit::Skip => MediaFit::Skip,
        style_syntax::MediaFit::Unknown => MediaFit::Unknown,
    }
}

/// Map a retained-data admission refusal onto an existing refusal reason.
/// No new diagnostics type: budgets read as the shared scanner budget, bad
/// payload bytes as sheet encoding, unsupported data: syntax as malformed CSS.
fn data_admission_reason(error: crate::data_stylesheet::AdmissionError) -> RefusalReason {
    use crate::data_stylesheet::AdmissionError as E;
    match error {
        E::EncodedBudget | E::DecodedBudget | E::Allocation => RefusalReason::CssScanBudget,
        E::InvalidUtf8 | E::InvalidCssByte => RefusalReason::StylesheetEncoding,
        E::UnsupportedScheme
        | E::UnsupportedMedia
        | E::UnsupportedRepresentation
        | E::HeaderBudget
        | E::InvalidBase64 => RefusalReason::CssMalformed,
    }
}

#[allow(clippy::too_many_lines)] // Source-order stylesheet processing with bounded refusal evidence.
fn matched_rules(
    nodes: &[Node],
    sheets: &[Vec<u8>],
    diagnostics: &mut StyleDiagnostics,
    media: Option<crate::media_query::MediaContext>,
    budget: SupplyBudget,
) -> (Vec<MatchedRule>, bool, usize) {
    let mut rules = Vec::new();
    let mut unsupported = false;
    let mut aggregate = 0_usize;
    let mut linked = 0;
    let mut supplied = 0;
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
                match stylesheet_media(css::attribute(nodes, handle, "media"), media) {
                    MediaFit::Screen => {}
                    MediaFit::Skip => continue,
                    MediaFit::Unknown => {
                        diagnostics.record(RefusalReason::StylesheetMedia);
                        unsupported = true;
                        continue;
                    }
                }
                // A retained data: href decodes in place under the same
                // two-sheet ceiling and the existing per-sheet budget; an
                // href dropped by DOM retention (value over 2048 bytes or
                // past the attribute count) keeps the host-supply path
                // unchanged. Admission refusals map onto existing reasons
                // only: budgets to CssScanBudget, payload bytes to
                // StylesheetEncoding, unsupported data: syntax to
                // CssMalformed. No new resource budget or refusal type.
                let decoded = match css::attribute(nodes, handle, "href").filter(|href| {
                    href.as_bytes()
                        .get(..5)
                        .is_some_and(|p| p.eq_ignore_ascii_case(b"data:"))
                }) {
                    Some(href) => {
                        if linked >= budget.max_linked_sheets {
                            diagnostics.record(RefusalReason::StylesheetCount);
                            unsupported = true;
                            continue;
                        }
                        match crate::data_stylesheet::decode_with_max(href, budget.max_sheet_bytes)
                        {
                            Ok(sheet) => {
                                linked += 1;
                                Some(sheet)
                            }
                            Err(error) => {
                                diagnostics.record(data_admission_reason(error));
                                unsupported = true;
                                continue;
                            }
                        }
                    }
                    None => None,
                };
                let text: &str = match &decoded {
                    // The decoder already proved UTF-8 and the shared budget.
                    Some(sheet) => sheet.css(),
                    None => {
                        // A linked sheet that the host did not supply cannot
                        // be treated as an empty stylesheet. The two-sheet
                        // resource ceiling also must not turn a later link
                        // into a silent skip. The host array is indexed by
                        // supplied external sheets only: an admitted data:
                        // sheet consumes a ceiling slot but no host slot.
                        let Some(bytes) = sheets
                            .get(supplied)
                            .filter(|_| linked < budget.max_linked_sheets)
                        else {
                            diagnostics.record(if linked >= budget.max_linked_sheets {
                                RefusalReason::StylesheetCount
                            } else {
                                RefusalReason::MissingStylesheet
                            });
                            unsupported = true;
                            continue;
                        };
                        linked += 1;
                        supplied += 1;
                        // Replacement characters can change selectors, tokens
                        // and colors. Until CSS @charset/HTTP encoding are
                        // resolved by the host, only literal UTF-8 bytes can
                        // prove full paint. Validation borrows, and the
                        // existing scanner budget is checked before any sheet
                        // allocation, so an oversized or invalid host supply
                        // refuses without materializing bytes. Refusal
                        // precedence is unchanged: encoding, then budget.
                        let Ok(text) = std::str::from_utf8(bytes) else {
                            diagnostics.record(RefusalReason::StylesheetEncoding);
                            unsupported = true;
                            continue;
                        };
                        if text.len() > budget.max_sheet_bytes {
                            diagnostics.record(RefusalReason::CssScanBudget);
                            unsupported = true;
                            continue;
                        }
                        text
                    }
                };
                // NEW aggregate gate, after the per-sheet checks, in document
                // order. Never fires under CANONICAL (derived bound).
                if aggregate.saturating_add(text.len()) > budget.max_aggregate_bytes {
                    diagnostics.record(RefusalReason::SupplyAggregate);
                    unsupported = true;
                    continue;
                }
                aggregate += text.len();
                text.to_owned()
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
                match stylesheet_media(css::attribute(nodes, handle, "media"), media) {
                    MediaFit::Screen => {}
                    MediaFit::Skip => continue,
                    MediaFit::Unknown => {
                        diagnostics.record(RefusalReason::StylesheetMedia);
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
        let parsed = style_syntax::scan_in_budget(&body, media, budget.max_sheet_bytes);
        if parsed.truncated {
            diagnostics.record(RefusalReason::CssScanBudget);
        }
        if parsed.malformed {
            diagnostics.record(RefusalReason::CssMalformed);
        }
        if parsed.unsupported_at_rule {
            diagnostics.record(RefusalReason::CssAtRule);
        }
        for _ in 0..parsed.unsupported_conditions {
            diagnostics.record(RefusalReason::CssCondition);
        }
        unsupported |= parsed.truncated
            || parsed.malformed
            || parsed.unsupported_at_rule
            || parsed.unsupported_conditions > 0;
        for rule in parsed.rules {
            if rules.len() >= MAX_RULES {
                diagnostics.record(RefusalReason::ExpandedRules);
                return (rules, true, supplied);
            }
            match style_syntax::conditions_fit(&rule.conditions, media) {
                style_syntax::MediaFit::Screen => {}
                style_syntax::MediaFit::Skip | style_syntax::MediaFit::Unknown => continue,
            }
            let mut selectors = rule.prelude.split(',');
            let (properties, unknown) = properties(rule.declarations);
            let refused = unknown || !style_syntax::declarations_complete(rule.declarations);
            if refused {
                diagnostics.record(RefusalReason::CssDeclaration);
            }
            unsupported |= refused;
            for selector in selectors.by_ref().take(16) {
                let Some(selector) = css::selector(selector.trim()) else {
                    diagnostics.record(RefusalReason::CssSelector);
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
                        diagnostics.record(RefusalReason::ExpandedRules);
                        return (rules, true, supplied);
                    }
                }
            }
            // A selector group beyond our bound may match a node we would
            // otherwise paint without its rule. Never present that as complete.
            let refused = selectors.next().is_some();
            if refused {
                diagnostics.record(RefusalReason::CssSelectorGroup);
            }
            unsupported |= refused;
        }
    }
    (rules, unsupported, supplied)
}

#[allow(clippy::too_many_lines)] // Explicit supported-property parsing rejects all unknown CSS.
pub(crate) fn properties(body: &str) -> (Vec<(Property, Value, bool)>, bool) {
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
            let pixel = |value: &str| parse_padding(value);
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
        match border_declarations(&name, value) {
            BorderParse::NotBorder => {}
            BorderParse::Unsupported => {
                unsupported = true;
                continue;
            }
            BorderParse::Declared(list) => {
                result.extend(
                    list.into_iter()
                        .map(|(property, v)| (property, v, important)),
                );
                continue;
            }
        }
        if name == "background-image" || name == "background-clip" {
            match background_layers(&name, value) {
                Some(list) => {
                    result.extend(
                        list.into_iter()
                            .map(|(property, v)| (property, v, important)),
                    );
                }
                None => unsupported = true,
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
                [top] => parse_margin_value(top).map(|top| [top; 4]),
                [top, right] => parse_margin_value(top)
                    .zip(parse_margin_value(right))
                    .map(|(top, right)| [top, right, top, right]),
                [top, right, bottom] => parse_margin_value(top)
                    .zip(parse_margin_value(right))
                    .zip(parse_margin_value(bottom))
                    .map(|((top, right), bottom)| [top, right, bottom, right]),
                [top, right, bottom, left] => parse_margin_value(top)
                    .zip(parse_margin_value(right))
                    .zip(parse_margin_value(bottom))
                    .zip(parse_margin_value(left))
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
                result.push((property, margin, important));
            }
            continue;
        }
        if name == "flex" {
            // Scratch: exact expansion of CSS Flexbox 1 sec 7.1.1 for the forms
            // `none | auto | <n> <n>? || <px|auto|0%>`. Fractions, negatives,
            // other percentages, content keywords and calc refuse.
            if let Some(keyword) = parse_keyword(value) {
                for property in [Property::FlexGrow, Property::FlexShrink, Property::FlexBasis] {
                    result.push((property, keyword, important));
                }
                continue;
            }
            match parse_flex_shorthand(value) {
                Some((grow, shrink, basis)) => {
                    result.push((Property::FlexGrow, Value::FlexGrow(grow), important));
                    result.push((Property::FlexShrink, Value::FlexShrink(shrink), important));
                    result.push((Property::FlexBasis, Value::FlexBasis(basis), important));
                }
                None => unsupported = true,
            }
            continue;
        }
        if name == "background" {
            // The shorthand resets every background longhand it omits.
            result.push((Property::BackgroundClip, Value::Initial, important));
            result.push((Property::BackgroundLayers, Value::Initial, important));
        }
        // Explicit non-admission for inline glyph/visibility changes, including
        // empty ancestors. Never classify these as harmless bare phrases.
        if matches!(
            name.as_str(),
            "text-transform" | "letter-spacing" | "word-spacing" | "visibility"
        ) {
            unsupported = true;
            continue;
        }
        let parsed = match name.as_str() {
            "position" => parse_keyword(value)
                .or(match value {
                    "static" => Some(Value::Position(Position::Static)),
                    "absolute" => Some(Value::Position(Position::Absolute)),
                    _ => None,
                })
                .map(|v| (Property::Position, v)),
            "top" | "right" | "bottom" | "left" => parse_keyword(value)
                .or_else(|| {
                    parse_margin(value)
                        .filter(|v| matches!(v, Margin::Auto | Margin::Px(_)))
                        .map(Value::Offset)
                })
                .map(|v| {
                    (
                        match name.as_str() {
                            "top" => Property::Top,
                            "right" => Property::Right,
                            "bottom" => Property::Bottom,
                            _ => Property::Left,
                        },
                        v,
                    )
                }),
            "display" => parse_keyword(value)
                .or(match value {
                    "none" => Some(Value::Display(Display::None)),
                    "block" => Some(Value::Display(Display::Block)),
                    "inline" => Some(Value::Display(Display::Inline)),
                    "inline-block" => Some(Value::Display(Display::InlineBlock)),
                    "list-item" => Some(Value::Display(Display::ListItem)),
                    "flex" => Some(Value::Display(Display::Flex)),
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
            "font-family" => {
                // Retention is not provider binding. Keep the existing whole
                // render refusal even for a recognized generic/global value.
                unsupported = true;
                parse_keyword(value)
                    .or(match value {
                        "serif" => Some(Value::FontFamily(Some(
                            crate::computed_style::GenericFamily::Serif,
                        ))),
                        "sans-serif" => Some(Value::FontFamily(Some(
                            crate::computed_style::GenericFamily::SansSerif,
                        ))),
                        "monospace" => Some(Value::FontFamily(Some(
                            crate::computed_style::GenericFamily::Monospace,
                        ))),
                        _ => None,
                    })
                    .map(|value| (Property::FontFamily, value))
            }
            "font-size" => parse_keyword(value)
                .or_else(|| match parse_width(value) {
                    Some(Length::Px(px)) if px > 0 => Some(Value::FontSize(px)),
                    Some(Length::Percent(percent)) if percent > 0 => {
                        Some(Value::FontSizePercent(percent))
                    }
                    _ => parse_rem(value).map(Value::FontSizeRem),
                    // zero/subpixel/other relative forms remain unsupported
                })
                .map(|value| (Property::FontSize, value)),
            "font-weight" => parse_keyword(value)
                .or_else(|| match value {
                    "normal" => Some(Value::FontWeight(400)),
                    "bold" => Some(Value::FontWeight(700)),
                    "bolder" => Some(Value::FontWeightRelative { bolder: true }),
                    "lighter" => Some(Value::FontWeightRelative { bolder: false }),
                    _ if value.bytes().all(|c| c.is_ascii_digit()) => value
                        .parse::<u16>()
                        .ok()
                        .filter(|w| (100..=900).contains(w) && w % 100 == 0)
                        .map(Value::FontWeight),
                    _ => None,
                })
                .map(|value| (Property::FontWeight, value)),
            "font-style" => parse_keyword(value)
                .or(match value {
                    "normal" => Some(Value::FontStyle(FontStyle::Normal)),
                    "italic" => Some(Value::FontStyle(FontStyle::Italic)),
                    "oblique" => Some(Value::FontStyle(FontStyle::Oblique)),
                    _ => None,
                })
                .map(|value| (Property::FontStyle, value)),
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
            "min-width" => parse_keyword(value)
                .or_else(|| match parse_width(value) { Some(l @ (Length::Auto | Length::Px(_))) => Some(Value::MinWidth(l)), _ => None })
                .map(|value| (Property::MinWidth, value)),
            "max-width" => parse_keyword(value)
                .or_else(|| match if value == "none" { Some(Length::Auto) } else { parse_width(value) } { Some(l @ (Length::Auto | Length::Px(_))) => Some(Value::MaxWidth(l)), _ => None })
                .map(|value| (Property::MaxWidth, value)),
            "height" => parse_keyword(value)
                .or_else(|| parse_width(value).map(Value::Height))
                .map(|value| (Property::Height, value)),
            "flex-grow" => parse_keyword(value)
                .or_else(|| {
                    // Scratch: plain unsigned decimal integer only; fractions, signs, exponents refuse.
                    (!value.is_empty()
                        && value.len() <= 4
                        && value.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| value.parse::<u32>().ok().map(Value::FlexGrow))
                    .flatten()
                })
                .map(|value| (Property::FlexGrow, value)),
            "flex-shrink" => parse_keyword(value)
                .or_else(|| {
                    (!value.is_empty()
                        && value.len() <= 4
                        && value.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| value.parse::<u32>().ok().map(Value::FlexShrink))
                    .flatten()
                })
                .map(|value| (Property::FlexShrink, value)),
            "flex-basis" => parse_keyword(value)
                .or_else(|| {
                    // auto or px lengths only; percentages, content, calc refuse.
                    match parse_width(value) {
                        Some(len @ (Length::Auto | Length::Px(_))) => Some(Value::FlexBasis(len)),
                        _ => None,
                    }
                })
                .map(|value| (Property::FlexBasis, value)),
            "justify-content" => parse_keyword(value)
                .or(match value {
                    "flex-start" => Some(Value::JustifyContent(JustifyContent::FlexStart)),
                    "flex-end" => Some(Value::JustifyContent(JustifyContent::FlexEnd)),
                    "center" => Some(Value::JustifyContent(JustifyContent::Center)),
                    "space-between" => Some(Value::JustifyContent(JustifyContent::SpaceBetween)),
                    "space-around" => Some(Value::JustifyContent(JustifyContent::SpaceAround)),
                    "space-evenly" => Some(Value::JustifyContent(JustifyContent::SpaceEvenly)),
                    _ => None,
                })
                .map(|value| (Property::JustifyContent, value)),
            "align-items" => parse_keyword(value)
                .or(match value {
                    "stretch" => Some(Value::AlignItems(AlignItems::Stretch)),
                    "flex-start" => Some(Value::AlignItems(AlignItems::FlexStart)),
                    "flex-end" => Some(Value::AlignItems(AlignItems::FlexEnd)),
                    "center" => Some(Value::AlignItems(AlignItems::Center)),
                    _ => None,
                })
                .map(|value| (Property::AlignItems, value)),
            "box-sizing" => parse_keyword(value)
                .or(match value {
                    "content-box" => Some(Value::BoxSizing(BoxSizing::ContentBox)),
                    "border-box" => Some(Value::BoxSizing(BoxSizing::BorderBox)),
                    _ => None,
                })
                .map(|value| (Property::BoxSizing, value)),
            "padding-top" => parse_keyword(value)
                .or_else(|| {
                    parse_padding(value).map(Value::Padding)
                })
                .map(|value| (Property::PaddingTop, value)),
            "padding-bottom" => parse_keyword(value)
                .or_else(|| {
                    parse_padding(value).map(Value::Padding)
                })
                .map(|value| (Property::PaddingBottom, value)),
            "padding-left" => parse_keyword(value)
                .or_else(|| {
                    parse_padding(value).map(Value::Padding)
                })
                .map(|value| (Property::PaddingLeft, value)),
            "padding-right" => parse_keyword(value)
                .or_else(|| {
                    parse_padding(value).map(Value::Padding)
                })
                .map(|value| (Property::PaddingRight, value)),
            "text-align" => parse_keyword(value)
                .or(match value {
                    "left" => Some(Value::TextAlign(TextAlign::Left)),
                    "right" => Some(Value::TextAlign(TextAlign::Right)),
                    "center" => Some(Value::TextAlign(TextAlign::Center)),
                    _ => None,
                })
                .map(|v| (Property::TextAlign, v)),
            "margin-left" => parse_keyword(value)
                .or_else(|| parse_margin_value(value))
                .map(|value| (Property::MarginLeft, value)),
            "margin-right" => parse_keyword(value)
                .or_else(|| parse_margin_value(value))
                .map(|value| (Property::MarginRight, value)),
            "margin-top" => parse_keyword(value)
                .or_else(|| parse_margin_value(value))
                .map(|value| (Property::MarginTop, value)),
            "margin-bottom" => parse_keyword(value)
                .or_else(|| parse_margin_value(value))
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

/// `<number>rem`, up to four decimals, exact in ten-thousandths. Zero is refused.
fn parse_rem(value: &str) -> Option<u32> {
    let number = value.strip_suffix("rem")?;
    if number.ends_with('.') {
        return None; // a dot requires fraction digits
    }
    let (whole, fractional) = number.split_once('.').unwrap_or((number, ""));
    if (whole.is_empty() && fractional.is_empty())
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || fractional.len() > 4
        || !fractional.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let mut v = if whole.is_empty() { 0 } else { whole.parse::<u32>().ok()?.checked_mul(10_000)? };
    if !fractional.is_empty() {
        let scale = 10u32.pow(4 - u32::try_from(fractional.len()).ok()?);
        v = v.checked_add(fractional.parse::<u32>().ok()?.checked_mul(scale)?)?;
    }
    (v > 0).then_some(v)
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
    let sign = if negative { -1 } else { 1 };
    if let Some(number) = value.strip_suffix("em") {
        // An em margin computes against the element's own font size. The
        // percent parser supplies exact fixed-point; the cascade resolves
        // whole pixels only.
        if let Some(Length::Percent(hundredths)) = parse_width(&format!("{number}%")) {
            return Some(Margin::Em(
                i32::try_from(hundredths)
                    .ok()?
                    .checked_mul(100)?
                    .checked_mul(sign)?,
            ));
        }
        return None;
    }
    let length = parse_width(value)?;
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
        let (channels, alpha) = if let Some(channels) = value.strip_prefix("rgb(") {
            (channels.strip_suffix(')')?, false)
        } else {
            (value.strip_prefix("rgba(")?.strip_suffix(')')?, true)
        };
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
        // Fully opaque legacy rgba is identical to rgb; no alpha blending
        // or channel clamping is silently approximated.
        if alpha && !matches!(parts.next()?, "1" | "1.0" | "1.00") {
            return None;
        }
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
        StyleTree::from_dom(&nodes, sheets, None)
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
        // Em margins resolve against the paragraph's own computed font size.
        for (html, px) in [
            ("<p style='margin:1em'>Text</p>", 16),
            ("<p style='font-size:20px;margin:1em'>Text</p>", 20),
        ] {
            let em = tree(html, &[]);
            let paragraph = em.nodes.iter().find(|n| n.tag == "p").unwrap();
            assert_eq!(
                [
                    paragraph.style.margin_top,
                    paragraph.style.margin_right,
                    paragraph.style.margin_bottom,
                    paragraph.style.margin_left
                ],
                [Margin::Px(px); 4],
                "{html}"
            );
            assert!(!em.unsupported, "{html}");
        }
        // Subpixel and out-of-range em margins still refuse to paint.
        assert!(
            tree("<p style='margin:1.234em'>Text</p>", &[]).unsupported
                || tree("<p style='margin:1.234em'>Text</p>", &[])
                    .nodes
                    .iter()
                    .any(|n| matches!(n.style.margin_top, Margin::Em(_)))
        );
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
        // The div's -6px does not inherit; the paragraph's bottom margin is
        // the UA sheet's 1em at the default 16px font size.
        assert_eq!(p.style.margin_bottom, Margin::Px(16));
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
    // Replaced regression history: `screen, print` on a link used to refuse as an
    // unknown media list. It is now Screen-fit (see tests/css_link_media.rs); the
    // link below still refuses here only because no sheet is supplied
    // (MissingStylesheet), and context-less width features still refuse.
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
    fn linked_stylesheet_scan_budget_checked_before_allocation() {
        let html = "<link rel=stylesheet href=/x><p>Text</p>";
        // Exactly the existing scanner budget is still admitted.
        let mut exact = b"p{color:red}/*".to_vec();
        exact.resize(crate::style_syntax::MAX_BYTES - 2, b'x');
        exact.extend_from_slice(b"*/");
        assert_eq!(exact.len(), crate::style_syntax::MAX_BYTES);
        let admitted = tree(html, &[exact]);
        assert!(!admitted.unsupported);
        assert_eq!(admitted.diagnostics.count(RefusalReason::CssScanBudget), 0);
        // One byte over refuses with the same reason the scanner truncation
        // reported, before the sheet String is materialized.
        let mut over = b"p{color:red}/*".to_vec();
        over.resize(crate::style_syntax::MAX_BYTES + 1, b'x');
        let refused = tree(html, &[over]);
        assert!(refused.unsupported);
        assert_eq!(refused.diagnostics.count(RefusalReason::CssScanBudget), 1);
        assert_eq!(refused.diagnostics.count(RefusalReason::CssMalformed), 0);
    }

    #[test]
    fn linked_stylesheet_oversized_invalid_utf8_keeps_encoding_precedence() {
        let html = "<link rel=stylesheet href=/x><p>Text</p>";
        let mut over = b"p{color:red}\xff".to_vec();
        over.resize(crate::style_syntax::MAX_BYTES + 1, b'x');
        let refused = tree(html, &[over]);
        assert!(refused.unsupported);
        assert_eq!(
            refused.diagnostics.count(RefusalReason::StylesheetEncoding),
            1
        );
        assert_eq!(refused.diagnostics.count(RefusalReason::CssScanBudget), 0);
    }

    #[test]
    fn retained_data_stylesheet_decodes_under_existing_ceilings() {
        // "p{color:#123456}" in canonical base64.
        let html =
            "<link rel=stylesheet href='data:text/css;base64,cHtjb2xvcjojMTIzNDU2fQ=='><p>Text</p>";
        let admitted = tree(html, &[]);
        assert!(!admitted.unsupported);
        let color = admitted
            .nodes
            .iter()
            .find(|node| node.tag == "p")
            .unwrap()
            .style
            .color;
        assert_eq!(color, 0x12_34_56);
        // The same link counts toward the existing two-sheet ceiling.
        let three = format!(
            "{0}{0}{0}<p>Text</p>",
            "<link rel=stylesheet href='data:text/css;base64,cHt9'>"
        );
        let refused = tree(&three, &[]);
        assert!(refused.unsupported);
        assert_eq!(refused.diagnostics.count(RefusalReason::StylesheetCount), 1);
    }

    #[test]
    fn retained_data_stylesheet_refusals_map_to_existing_reasons() {
        // Unsupported media type and invalid base64 are malformed input, not
        // a new refusal class; both refuse the sheet without a panic.
        let wrong_media = tree(
            "<link rel=stylesheet href='data:text/plain;base64,cHt9'><p>Text</p>",
            &[],
        );
        assert!(wrong_media.unsupported);
        assert_eq!(
            wrong_media.diagnostics.count(RefusalReason::CssMalformed),
            1
        );
        let bad_base64 = tree(
            "<link rel=stylesheet href='data:text/css;base64,cHt'><p>Text</p>",
            &[],
        );
        assert!(bad_base64.unsupported);
        assert_eq!(bad_base64.diagnostics.count(RefusalReason::CssMalformed), 1);
    }

    #[test]
    fn mixed_data_then_external_keeps_host_supply_index() {
        // A data: sheet consumes a two-sheet ceiling slot but no host slot:
        // the later external link must read the FIRST host sheet, not skip it.
        let html = "<link rel=stylesheet href='data:text/css;base64,cHtjb2xvcjojMTExMTExfQ=='>                    <link rel=stylesheet href=/x><p>Text</p>";
        let mixed = tree(html, &[b"p{color:#222222}".to_vec()]);
        assert!(!mixed.unsupported);
        let color = mixed
            .nodes
            .iter()
            .find(|node| node.tag == "p")
            .unwrap()
            .style
            .color;
        // Source order: the external sheet's declaration wins.
        assert_eq!(color, 0x22_22_22);
    }

    #[test]
    fn non_ascii_href_never_panics_and_stays_host_supply() {
        // Three 2-byte codepoints: byte index 5 is inside the third one.
        let html = "<link rel=stylesheet href='\u{e9}\u{e9}\u{e9}'><p>Text</p>";
        let refused = tree(html, &[]);
        assert!(refused.unsupported);
        assert_eq!(
            refused.diagnostics.count(RefusalReason::MissingStylesheet),
            1
        );
    }

    #[test]
    fn print_and_disabled_data_links_skip_before_decode() {
        for html in [
            "<link rel=stylesheet media=print href='data:text/css;base64,cHtjb2xvcjojMTExMTExfQ=='>",
            "<link rel=stylesheet disabled href='data:text/css;base64,cHtjb2xvcjojMTExMTExfQ=='>",
            "<link rel=stylesheet href='data:text/plain;base64,cHt9' media=print>",
        ] {
            let skipped = tree(&(html.to_owned() + "<p>Text</p>"), &[]);
            assert!(!skipped.unsupported, "{html}");
            assert_eq!(skipped.diagnostics.count(RefusalReason::CssMalformed), 0);
            assert_eq!(
                skipped.diagnostics.count(RefusalReason::MissingStylesheet),
                0
            );
        }
    }

    #[test]
    fn failed_data_decode_consumes_no_ceiling_slot() {
        // One malformed data: link (no slot), then two admitted external
        // sheets reach the ceiling exactly: the third external refuses.
        let head = "<link rel=stylesheet href='data:text/css;base64,cHt'>";
        let ext = "<link rel=stylesheet href=/x>";
        let html = format!("{head}{ext}{ext}{ext}<p>Text</p>");
        let tree = tree(
            &html,
            &[b"p{color:#111111}".to_vec(), b"p{color:#222222}".to_vec()],
        );
        assert!(tree.unsupported);
        assert_eq!(tree.diagnostics.count(RefusalReason::CssMalformed), 1);
        assert_eq!(tree.diagnostics.count(RefusalReason::StylesheetCount), 1);
        assert_eq!(tree.diagnostics.count(RefusalReason::MissingStylesheet), 0);
    }

    #[test]
    fn dropped_data_href_keeps_host_supply_path() {
        // A data: href value over the 2048-byte DOM attribute retention is
        // dropped before this arm, so the link keeps the existing missing
        // host-supply refusal instead of decoding or raising any cap.
        let long_href = format!(
            "<link rel=stylesheet href='data:text/css;base64,{}'>",
            "QUJD".repeat(513)
        );
        let refused = tree(&(long_href + "<p>Text</p>"), &[]);
        assert!(refused.unsupported);
        assert_eq!(
            refused.diagnostics.count(RefusalReason::MissingStylesheet),
            1
        );
        assert_eq!(refused.diagnostics.count(RefusalReason::CssMalformed), 0);
    }

    #[test]
    fn dropped_href_with_supplied_sheet_is_admitted_under_canonical() {
        // Canonical behavior pinned BEFORE the supply-budget work: an href
        // dropped by DOM retention takes the host-supply branch, so a host
        // sheet supplied at that position IS admitted today (HOST-CLAIMED,
        // engine-classified as an external link; never promoted to a verified
        // data: origin). Without a supplied sheet it refuses exactly once.
        let long_href = format!(
            "<link rel=stylesheet href='data:text/css;base64,{}'>",
            "QUJD".repeat(513)
        );
        let html = long_href + "<p>Text</p>";
        let refused = tree(&html, &[]);
        assert!(refused.unsupported);
        assert_eq!(
            refused.diagnostics.count(RefusalReason::MissingStylesheet),
            1
        );
        assert_eq!(refused.host_sheets_claimed, None);
        let admitted = tree(&html, &[b"p{color:#123456}".to_vec()]);
        assert!(!admitted.unsupported);
        assert_eq!(admitted.host_sheets_claimed, Some(1));
        assert_eq!(
            admitted.diagnostics.count(RefusalReason::MissingStylesheet),
            0
        );
        assert_eq!(
            admitted.diagnostics.count(RefusalReason::StylesheetCount),
            0
        );
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

#[cfg(test)]
mod percentage_font_size_tests {
    use crate::{parse_style_tree, Limits};
    #[test]
    fn rem_font_size_resolves_against_root_and_refuses_subpixel() {
        let t = |h: &str| parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT);
        let size = |tr: &crate::style_tree::StyleTree, tag: &str| tr.nodes.iter().find(|n| n.tag == tag).unwrap().style.font_size;
        let a = t("<!doctype html><p style='font-size:1.5rem'>a</p>");
        assert!(!a.unsupported);
        assert_eq!(size(&a, "p"), 24);
        let b = t("<!doctype html><html style='font-size:20px'><body><div style='font-size:2rem'><span style='font-size:50%'>a</span></div></body></html>");
        assert!(!b.unsupported);
        assert_eq!(size(&b, "div"), 40);
        assert_eq!(size(&b, "span"), 20);
        let c = t("<!doctype html><html style='font-size:2rem'><body><p>a</p></body></html>");
        assert!(!c.unsupported);
        assert_eq!(size(&c, "p"), 32); // root rem is the initial 16px
        let d = t("<!doctype html><p style='font-size:.5rem'>a</p>");
        assert!(!d.unsupported);
        assert_eq!(size(&d, "p"), 8);
        for bad in ["font-size:0rem", "font-size:1.rem", "font-size:.rem", "font-size:1e1rem", "font-size:+1rem", "font-size:1.4rem", "font-size:1.00001rem", "font-size:-1rem", "font-size:1REMx", "font-size:99999999rem"] {
            let h = format!("<!doctype html><p style='{bad}'>a</p>");
            assert!(t(&h).unsupported, "{bad}");
        }
    }

    #[test]
    fn percentage_font_size_resolves_parent_then_inherits_length() {
        let tree = parse_style_tree(b"<!doctype html><section style='font-size:20px'><p style='font-size:150%;line-height:120%'>ab<span style='font-size:80%'>cd</span></p><div style='font-size:125%'>ef</div></section>", &[], &Limits::DEFAULT);
        assert!(!tree.unsupported);
        let node = |tag: &str| tree.nodes.iter().find(|n| n.tag == tag).unwrap();
        assert_eq!(node("p").style.font_size, 30);
        assert_eq!(node("p").style.line_height, Some(36));
        assert_eq!(node("span").style.font_size, 24);
        assert_eq!(node("span").style.line_height, Some(36));
        assert_eq!(node("div").style.font_size, 25);
        for style in [
            "font-size:0",
            "font-size:0%",
            "font-size:33%",
            "font-size:4294967295%",
        ] {
            let html = format!("<!doctype html><p style='{style}'>ab</p>");
            assert!(
                parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported,
                "{style}"
            );
        }
    }
}

const SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

fn side_property(side: usize, kind: &str) -> Property {
    match (kind, side) {
        ("width", 0) => Property::BorderTopWidth,
        ("width", 1) => Property::BorderRightWidth,
        ("width", 2) => Property::BorderBottomWidth,
        ("width", _) => Property::BorderLeftWidth,
        ("style", 0) => Property::BorderTopStyle,
        ("style", 1) => Property::BorderRightStyle,
        ("style", 2) => Property::BorderBottomStyle,
        ("style", _) => Property::BorderLeftStyle,
        (_, 0) => Property::BorderTopColor,
        (_, 1) => Property::BorderRightColor,
        (_, 2) => Property::BorderBottomColor,
        (_, _) => Property::BorderLeftColor,
    }
}

fn border_width(token: &str) -> Option<u32> {
    match token {
        "thin" => Some(1),
        "medium" => Some(3),
        "thick" => Some(5),
        _ => match parse_width(token) {
            Some(Length::Px(px)) => Some(px),
            _ => fractional_border_px(token),
        },
    }
}

/// Experimental (head7 copy only): Blink snaps a positive fractional border
/// width to whole device pixels, flooring at 1x and never below 1px
/// (verified in Chrome: 49.75px computes to 49px). At most two decimals, px only.
fn fractional_border_px(token: &str) -> Option<u32> {
    // Blink's rule is floor(w * dsf) / dsf with a 1-device-px minimum; the
    // integer result below is only that rule at dsf == 1.
    const _: () = assert!(crate::DEVICE_PX_PER_CSS_PX == 1, "border snap is the dsf == 1 case only");
    let number = token.strip_suffix("px")?;
    let (whole, frac) = number.split_once('.')?;
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || frac.is_empty()
        || frac.len() > 2
        || !frac.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let whole = whole.parse::<u32>().ok()?;
    if whole == 0 && frac.bytes().any(|c| c != b'0') {
        Some(1)
    } else {
        Some(whole)
    }
}

/// Retain supported line styles; painting independently proves their geometry.
fn border_style(token: &str) -> Option<crate::computed_style::BorderStyle> {
    match token {
        "none" | "hidden" => Some(crate::computed_style::BorderStyle::None),
        "solid" => Some(crate::computed_style::BorderStyle::Solid),
        "dashed" => Some(crate::computed_style::BorderStyle::Dashed),
        "dotted" => Some(crate::computed_style::BorderStyle::Dotted),
        _ => None,
    }
}

/// Opaque colours or currentColor; partial alpha still refuses.
fn border_color(token: &str) -> Option<BorderColor> {
    match token {
        "transparent" => Some(BorderColor::Transparent),
        "currentcolor" => Some(BorderColor::CurrentColor),
        _ => opaque_color(token).map(BorderColor::Opaque),
    }
}

/// One to four values expanded to top, right, bottom, left.
fn four_sides<T: Copy>(tokens: &[&str], parse: impl Fn(&str) -> Option<T>) -> Option<[T; 4]> {
    let parsed: Option<Vec<T>> = tokens.iter().map(|token| parse(token)).collect();
    match parsed?.as_slice() {
        [a] => Some([*a; 4]),
        [a, b] => Some([*a, *b, *a, *b]),
        [a, b, c] => Some([*a, *b, *c, *b]),
        [a, b, c, d] => Some([*a, *b, *c, *d]),
        _ => None,
    }
}

type Declared = Vec<(Property, Value)>;

enum BorderParse {
    NotBorder,
    /// A border property whose value this engine cannot account for.
    Unsupported,
    Declared(Declared),
}

impl From<Option<Declared>> for BorderParse {
    fn from(parsed: Option<Declared>) -> Self {
        parsed.map_or(Self::Unsupported, Self::Declared)
    }
}

/// ASCII-whitespace split that keeps a parenthesised group in one token.
/// `None` when parentheses are unbalanced or nest beyond one level.
fn split_top_level(value: &str) -> Option<Vec<&str>> {
    let mut tokens = Vec::new();
    let (mut start, mut depth) = (None, 0_u8);
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'(' => {
                if depth == 1 {
                    return None;
                }
                depth += 1;
                start.get_or_insert(index);
            }
            b')' => {
                depth = depth.checked_sub(1)?;
                start.get_or_insert(index);
            }
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' if depth == 0 => {
                if let Some(begin) = start.take() {
                    tokens.push(&value[begin..index]);
                }
            }
            _ => {
                start.get_or_insert(index);
            }
        }
    }
    if depth != 0 {
        return None;
    }
    if let Some(begin) = start {
        tokens.push(&value[begin..]);
    }
    Some(tokens)
}

fn border_declarations(name: &str, value: &str) -> BorderParse {
    let Some(rest) = name.strip_prefix("border") else {
        return BorderParse::NotBorder;
    };
    // Functional colours such as `rgb(255, 0, 0)` contain spaces. Split only
    // at depth zero; unbalanced parentheses stay refused.
    let Some(tokens) = split_top_level(value) else {
        return BorderParse::Unsupported;
    };
    let global = parse_keyword(value);
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    let mut out: Declared = Vec::new();
    // border-width / border-style / border-color
    if matches!(rest, "width" | "style" | "color") {
        let sides: Option<[Value; 4]> = if let Some(keyword) = global {
            Some([keyword; 4])
        } else {
            match rest {
                "width" => four_sides(&tokens, border_width).map(|w| w.map(Value::Padding)),
                "style" => four_sides(&tokens, border_style).map(|v| v.map(Value::BorderStyle)),
                _ => four_sides(&tokens, border_color).map(|v| v.map(Value::BorderColor)),
            }
        };
        return sides
            .map(|sides| {
                for (side, v) in sides.into_iter().enumerate() {
                    out.push((side_property(side, rest), v));
                }
                out
            })
            .into();
    }
    // border: and border-<side>[-width|-style|-color]
    let (sides, kind): (Vec<usize>, &str) = if rest.is_empty() {
        ((0..4).collect(), "")
    } else {
        let mut parts = rest.splitn(2, '-');
        let side_name = parts.next();
        let Some(side) = SIDES
            .iter()
            .position(|candidate| Some(*candidate) == side_name)
        else {
            return BorderParse::NotBorder;
        };
        (vec![side], parts.next().unwrap_or(""))
    };
    if matches!(kind, "width" | "style" | "color") {
        let v = global.or_else(|| match kind {
            "width" => border_width(value).map(Value::Padding),
            "style" => border_style(value).map(Value::BorderStyle),
            _ => border_color(value).map(Value::BorderColor),
        });
        return v
            .map(|v| sides.iter().map(|&s| (side_property(s, kind), v)).collect())
            .into();
    }
    if !kind.is_empty() {
        return BorderParse::NotBorder;
    }
    // Shorthand: width, style and colour in any order; omitted ones reset.
    if let Some(keyword) = global {
        for &side in &sides {
            for k in ["width", "style", "color"] {
                out.push((side_property(side, k), keyword));
            }
        }
        return BorderParse::Declared(out);
    }
    let (mut width, mut style, mut color) = (
        3,
        crate::computed_style::BorderStyle::None,
        BorderColor::CurrentColor,
    );
    let (mut seen_width, mut seen_style, mut seen_colour) = (false, false, false);
    if tokens.is_empty() || tokens.len() > 3 {
        return BorderParse::Unsupported;
    }
    for token in tokens {
        if let (false, Some(w)) = (seen_width, border_width(token)) {
            width = w;
            seen_width = true;
        } else if let (false, Some(v)) = (seen_style, border_style(token)) {
            style = v;
            seen_style = true;
        } else if let (false, Some(c)) = (seen_colour, border_color(token)) {
            color = c;
            seen_colour = true;
        } else {
            return BorderParse::Unsupported;
        }
    }
    for &side in &sides {
        out.push((side_property(side, "width"), Value::Padding(width)));
        out.push((side_property(side, "style"), Value::BorderStyle(style)));
        out.push((side_property(side, "color"), Value::BorderColor(color)));
    }
    BorderParse::Declared(out)
}

/// `background-image` only counts layers (every one must be `none`), and
/// `background-clip` lists the clip of each layer.
fn background_layers(name: &str, value: &str) -> Option<Declared> {
    if let Some(keyword) = parse_keyword(value) {
        // An inherited image list is not something this engine tracks.
        return (name == "background-clip").then(|| vec![(Property::BackgroundClip, keyword)]);
    }
    let items: Vec<&str> = value.split(',').map(str::trim).collect();
    if items.is_empty() || items.len() > 4 {
        return None;
    }
    if name == "background-image" {
        if items.iter().any(|item| *item != "none") {
            return None;
        }
        return Some(vec![(
            Property::BackgroundLayers,
            Value::BackgroundLayers(u8::try_from(items.len()).ok()?),
        )]);
    }
    let mut clips = [Clip::Border; 4];
    for (slot, item) in clips.iter_mut().zip(&items) {
        *slot = match *item {
            "border-box" => Clip::Border,
            "padding-box" => Clip::Padding,
            "content-box" => Clip::Content,
            _ => return None,
        };
    }
    Some(vec![(
        Property::BackgroundClip,
        Value::BackgroundClips {
            clips,
            len: u8::try_from(items.len()).ok()?,
        },
    )])
}

#[cfg(test)]
mod phrase_tests {
    use super::*;
    fn tree(html: &str, sheets: &[Vec<u8>]) -> StyleTree {
        crate::parse_style_tree(html.as_bytes(), sheets, &crate::Limits::DEFAULT)
    }
    #[test]
    fn user_agent_styles_phrase_elements_and_font_weight_cascades() {
        let styled = tree("<html><body><p>one <strong>two</strong> <b>three</b> <em>four</em> <i>five</i></p></body></html>", &[]);
        let weight = |tag: &str| {
            styled
                .nodes
                .iter()
                .find(|node| node.tag == tag)
                .unwrap()
                .style
                .font_weight
        };
        assert_eq!(weight("strong"), 700);
        assert_eq!(weight("b"), 700);
        assert_eq!(weight("p"), 400);
        let style = |tag: &str| {
            styled
                .nodes
                .iter()
                .find(|node| node.tag == tag)
                .unwrap()
                .style
                .font_style
        };
        assert_eq!(style("em"), crate::computed_style::FontStyle::Italic);
        assert_eq!(style("i"), crate::computed_style::FontStyle::Italic);
        assert_eq!(style("p"), crate::computed_style::FontStyle::Normal);

        let styled = tree("<style>section{font-weight:600} .up{font-weight:bolder} .down{font-weight:lighter}</style><html><body><section><p class=up>a</p><p class=down>b</p></section></body></html>", &[]);
        let up = styled
            .nodes
            .iter()
            .find(|n| n.tag == "p" && n.style.font_weight == 900)
            .unwrap();
        let down = styled
            .nodes
            .iter()
            .find(|n| n.tag == "p" && n.style.font_weight == 400)
            .unwrap();
        assert_eq!(up.style.font_weight, 900); // bolder from 600
        assert_eq!(down.style.font_weight, 400); // lighter from 600
        let section = styled.nodes.iter().find(|n| n.tag == "section").unwrap();
        assert_eq!(section.style.font_weight, 600); // numeric weights compute...

        let styled = tree(
            "<style>p{font-weight:900}</style><html><body><p>a</p></body></html>",
            &[],
        );
        let p = styled.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!(p.style.font_weight, 900); // ...and are refused at paint
    }
}

#[cfg(test)]
mod border_functional_colour_tests {
    use crate::{parse_style_tree, Limits};
    fn refused(style: &str) -> bool {
        let html = format!("<!doctype html><p style='{style}'>x</p>");
        parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported
    }
    #[test]
    fn spaced_rgb_is_one_colour_token() {
        for ok in [
            "border:1px solid rgb(255, 0, 0)",
            "border:rgb(255, 0, 0) 1px solid",
            "border-color:rgb(0, 0, 0)",
            "border-color:rgb(0, 0, 0) rgb(1,1,1)",
            "border-color:rgb( 0 , 0 , 0 ) #fff red blue",
        ] {
            assert!(!refused(ok), "{ok}");
        }
    }
    #[test]
    fn malformed_or_unsupported_functions_stay_refused() {
        for bad in [
            "border:1px solid rgb(255, 0, 0",
            "border:1px solid rgb(255, 0, 0))",
            "border:1px solid rgb (255, 0, 0)",
            "border:1px solid rgb(255 0 0)",
            "border:1px solid rgba(255, 0, 0, 0.5)",
            "border:1px solid var(--x, red)",
            "border:1px solid calc(1px + 1px)",
            "border-color:rgb(0, 0, 0) rgb(1,1,1) red blue green",
            "border:1px solid rgb((1), 0, 0)",
        ] {
            assert!(refused(bad), "{bad}");
        }
    }
    #[test]
    fn spaced_rgb_cascades_like_unspaced() {
        let spaced = parse_style_tree(
            b"<!doctype html><p style='border:2px solid rgb(1, 2, 3)'>x</p>",
            &[],
            &Limits::DEFAULT,
        );
        let tight = parse_style_tree(
            b"<!doctype html><p style='border:2px solid rgb(1,2,3)'>x</p>",
            &[],
            &Limits::DEFAULT,
        );
        let node =
            |t: &crate::style_tree::StyleTree| t.nodes.iter().find(|n| n.tag == "p").unwrap().style;
        assert_eq!(node(&spaced).decor, node(&tight).decor);
    }
}

#[cfg(test)]
mod root_selector_tests {
    use crate::{parse_style_tree, style_diagnostics::RefusalReason, Limits};
    fn sizes(html: &str) -> (bool, Vec<(String, u32)>, usize) {
        let t = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        let sel = t.diagnostics.count(RefusalReason::CssSelector);
        let v = t
            .nodes
            .iter()
            .filter(|n| ["html", "body", "div", "p", "span"].contains(&n.tag.as_str()))
            .map(|n| (n.tag.clone(), n.style.font_size))
            .collect();
        (t.unsupported, v, sel)
    }
    fn page(css: &str, body: &str) -> String {
        format!("<!doctype html><style>{css}</style>{body}")
    }
    fn size_of(html: &str, tag: &str) -> u32 {
        let (unsupported, v, _) = sizes(html);
        assert!(!unsupported, "{html}");
        v.iter().find(|(t, _)| t == tag).unwrap().1
    }
    #[test]
    fn root_matches_only_the_document_element() {
        let body = "<div><p>x</p></div>";
        assert_eq!(size_of(&page(":root{font-size:20px}", body), "html"), 20);
        assert_eq!(size_of(&page(":root{font-size:20px}", body), "p"), 20);
        assert_eq!(
            size_of(&page("html:root{font-size:20px}", body), "html"),
            20
        );
        assert_eq!(size_of(&page(":ROOT{font-size:20px}", body), "html"), 20);
        // Not the root: body, descendants, an ancestor-qualified root.
        assert_eq!(
            size_of(&page("body:root{font-size:20px}", body), "body"),
            16
        );
        assert_eq!(size_of(&page("p:root{font-size:20px}", body), "p"), 16);
        assert_eq!(
            size_of(&page("div :root{font-size:20px}", body), "html"),
            16
        );
        assert_eq!(
            size_of(&page(":root > body{font-size:20px}", body), "body"),
            20
        );
        assert_eq!(
            size_of(&page(":root body p{font-size:20px}", body), "p"),
            20
        );
        assert_eq!(size_of(&page(":root > p{font-size:20px}", body), "p"), 16);
    }
    #[test]
    fn root_with_class_and_parser_normalized_nested_html() {
        let html = "<!doctype html><style>:root.big{font-size:30px}html.big > body{font-size:11px}</style><html class=big><body><div>x</div></body></html>";
        assert_eq!(size_of(html, "html"), 30);
        assert_eq!(size_of(html, "body"), 11);
        // A second <html> inside body is merged into the root by the parser,
        // not created as a nested element, so `:root` still names one element.
        let nested = "<!doctype html><style>:root.late{font-size:25px}</style><body><div><html class=late><p>x</p></div></body>";
        let (unsupported, v, _) = sizes(nested);
        assert!(!unsupported);
        assert_eq!(v.iter().filter(|(t, _)| t == "html").count(), 1);
        assert_eq!(v.iter().find(|(t, _)| t == "html").unwrap().1, 25);
        assert_eq!(v.iter().find(|(t, _)| t == "div").unwrap().1, 25);
    }
    #[test]
    fn root_specificity_is_a_pseudo_class_not_a_type() {
        let b = "<p>x</p>";
        // (0,1,0) beats html (0,0,1) in either order.
        for css in [
            "html{font-size:10px}:root{font-size:20px}",
            ":root{font-size:20px}html{font-size:10px}",
        ] {
            assert_eq!(size_of(&page(css, b), "html"), 20, "{css}");
        }
        // Equal to a class (0,1,0): source order decides.
        let html = "<!doctype html><style>.c{font-size:10px}:root{font-size:20px}</style><html class=c><body>x</body></html>";
        assert_eq!(size_of(html, "html"), 20);
        let html = "<!doctype html><style>:root{font-size:20px}.c{font-size:10px}</style><html class=c><body>x</body></html>";
        assert_eq!(size_of(html, "html"), 10);
        // An id beats :root; !important on html beats a normal :root; inline wins.
        let html = "<!doctype html><style>#i{font-size:12px}:root{font-size:20px}</style><html id=i><body>x</body></html>";
        assert_eq!(size_of(html, "html"), 12);
        assert_eq!(
            size_of(
                &page("html{font-size:10px!important}:root{font-size:20px}", b),
                "html"
            ),
            10
        );
        assert_eq!(
            size_of(
                &page(
                    ":root{font-size:20px!important}html{font-size:10px!important}",
                    b
                ),
                "html"
            ),
            20
        );
        let html = "<!doctype html><style>:root{font-size:20px!important}</style><html style='font-size:9px'><body>x</body></html>";
        assert_eq!(size_of(html, "html"), 20);
        let html = "<!doctype html><style>:root{font-size:20px}</style><html style='font-size:9px'><body>x</body></html>";
        assert_eq!(size_of(html, "html"), 9);
    }
    #[test]
    fn other_pseudo_forms_fail_closed_with_selector_refusal() {
        for sel in [
            ":hover",
            "a:hover",
            ":root:hover",
            ":root:root",
            "::before",
            ":root::before",
            ":not(p)",
            ":is(html)",
            ":rootx",
            ":root(",
            ":",
            ":root:",
            "p::root",
            ":first-child",
            ":root:not(p)",
        ] {
            let (unsupported, _, selectors) =
                sizes(&page(&format!("{sel}{{font-size:20px}}"), "<p>x</p>"));
            assert!(unsupported, "{sel}");
            // An unbalanced paren makes the whole stylesheet malformed, which the
            // reader refuses under another reason before selectors are parsed.
            assert!(selectors >= 1 || sel.contains('('), "{sel}");
        }
        // Grouped: one bad member refuses, the good one is not silently kept.
        let (unsupported, _, _) = sizes(&page(":root,:hover{font-size:20px}", "<p>x</p>"));
        assert!(unsupported);
    }
}

/// MEASUREMENT-ONLY prototype: padding length = exact px, or `<n>em` carried as
/// `PADDING_EM_FLAG | ten-thousandths of an em`, resolved at the end of the cascade against the
/// element's OWN computed font size (exact whole px only, else the `PADDING_UNREPRESENTABLE` marker).
fn parse_padding(value: &str) -> Option<u32> {
    if value.ends_with("rem") {
        let tt = parse_rem(value)?;
        return (tt < crate::computed_style::PADDING_REM_FLAG).then_some(crate::computed_style::PADDING_REM_FLAG | tt);
    }
    if let Some(number) = value.strip_suffix("em") {
        if let Some(Length::Percent(hundredths)) = parse_width(&format!("{number}%")) {
            let ten_thousandths = hundredths.checked_mul(100)?;
            return (ten_thousandths < crate::computed_style::PADDING_REM_FLAG)
                .then_some(crate::computed_style::PADDING_EM_FLAG | ten_thousandths);
        }
        return None;
    }
    match parse_width(value)? {
        Length::Px(px) if px < crate::computed_style::PADDING_REM_FLAG => Some(px),
        _ => None,
    }
}

/// MEASUREMENT-ONLY: margin side value, `<n>rem` (signed, exact ten-thousandths) or the existing margin forms.
fn parse_flex_shorthand(value: &str) -> Option<(u32, u32, Length)> {
    let tokens: Vec<&str> = value.split_ascii_whitespace().collect();
    let number = |t: &str| -> Option<u32> {
        (!t.is_empty() && t.len() <= 4 && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| t.parse().ok())
            .flatten()
    };
    let basis = |t: &str| -> Option<Length> {
        match t {
            "0%" => Some(Length::Px(0)), // definite container: 0% == 0px
            "auto" => Some(Length::Auto),
            _ if t.ends_with('%') => None,
            _ => match parse_width(t) {
                Some(l @ Length::Px(_)) => Some(l),
                _ => None,
            },
        }
    };
    match tokens.as_slice() {
        ["none"] => Some((0, 0, Length::Auto)),
        ["auto"] => Some((1, 1, Length::Auto)),
        [one] => match number(one) {
            Some(n) => Some((n, 1, Length::Px(0))),
            None => basis(one).map(|b| (1, 1, b)),
        },
        [a, b] => match (number(a), number(b)) {
            (Some(g), Some(s)) => Some((g, s, Length::Px(0))),
            (Some(g), None) => basis(b).map(|bs| (g, 1, bs)),
            (None, Some(g)) => basis(a).map(|bs| (g, 1, bs)),
            _ => None,
        },
        [a, b, c] => match (number(a), number(b), number(c)) {
            (Some(g), Some(s), _) => basis(c).map(|bs| (g, s, bs)),
            (None, Some(g), Some(s)) => basis(a).map(|bs| (g, s, bs)),
            _ => None,
        },
        _ => None,
    }
}

fn parse_margin_value(value: &str) -> Option<Value> {
    let (negative, unsigned) = value.strip_prefix('-').map_or((false, value), |r| (true, r));
    if unsigned.ends_with("rem") {
        let tt = i32::try_from(parse_rem(unsigned)?).ok()?;
        return Some(Value::MarginRem(if negative { tt.checked_neg()? } else { tt }));
    }
    parse_margin(value).map(Value::Margin)
}
