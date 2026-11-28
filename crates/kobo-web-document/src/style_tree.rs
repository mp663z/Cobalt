//! A small retained, style-bearing view of the repaired HTML tree.
//!
//! The reader's semantic `Document` intentionally discards geometry. This
//! arena keeps DOM ancestry and computed values for a separate box renderer.
//! It is not painted yet. Unsupported CSS does not become a guessed layout.

use crate::computed_style::{
    AlignItems, BorderColor, BoxSizing, Clear, Clip, Computed, Declaration, Direction, Display,
    Float, FontStyle, JustifyContent, Length, Margin, Origin, Position, Property, Shadow,
    TextAlign, Value,
};
use crate::css;
use crate::custom_props::{self, Def, Map};
use crate::dom::{Data, Handle, Node, DOCUMENT};
use crate::style_diagnostics::{RefusalReason, StyleDiagnostics};
use crate::style_syntax;
use std::collections::BTreeMap;
use std::rc::Rc;

const MAX_DECLARATIONS_PER_ELEMENT: usize = 256;
const MAX_RULES: usize = 4096;

/// Ext-entry prefilter candidate keys for one element, normalized EXACTLY as
/// the matcher compares (css.rs): tag folded ASCII-lowercase, attribute names
/// folded ASCII-lowercase, class tokens split on the 5 ASCII whitespace chars
/// and compared case-sensitively, id via the exact-name `attribute()` lookup.
/// Duplicates are removed so each bucket is consulted at most once per
/// element (exactly one charge per candidate rule; `class="a a"` oracle).
/// `Universal` is appended by the caller, not stored here.
fn ext_element_keys(nodes: &[Node], handle: Handle) -> Vec<css::PrefilterKey> {
    let mut keys: Vec<css::PrefilterKey> = Vec::new();
    let Data::Element { name, attrs, .. } = &nodes[handle].data else {
        return keys;
    };
    let mut push = |key: css::PrefilterKey| {
        if !keys.contains(&key) {
            keys.push(key);
        }
    };
    push(css::PrefilterKey::Tag((&*name.local).to_ascii_lowercase()));
    if let Some(id) = css::attribute(nodes, handle, "id") {
        push(css::PrefilterKey::Id(id.to_owned()));
    }
    if let Some(class) = css::attribute(nodes, handle, "class") {
        // Candidate class keys are the UNION of BOTH tokenizations, deduped:
        // the ext matcher splits on ASCII whitespace, the base matcher on
        // Unicode whitespace (str::split_whitespace), and neither token set is
        // a superset of the other ("a\u{00A0}b" ASCII-splits to one whole
        // token, Unicode-splits to "a","b"). Preserving whole ASCII tokens AND
        // Unicode subtokens covers both matcher grammars and any future
        // grammar that admits non-ASCII-whitespace bytes inside a class
        // literal (today unreachable: take_ident admits only [A-Za-z0-9_-],
        // so no selector can name such a class - proven by oracle). Over-
        // inclusion is harmless: the matcher still decides the match.
        for token in class
            .split(|c: char| matches!(c, ' ' | '\t' | '\n' | '\u{c}' | '\r'))
            .chain(class.split_whitespace())
        {
            if !token.is_empty() {
                push(css::PrefilterKey::Class(token.to_owned()));
            }
        }
    }
    for (attr_name, _) in attrs {
        push(css::PrefilterKey::AttrName(
            (&**attr_name).to_ascii_lowercase(),
        ));
    }
    keys
}
const MAX_MATCH_WORK: usize = 4_000_000;
/// CANDIDATE: an element's style attribute outranks every selector of the same
/// importance (CSS Cascade 4: style attribute beats any selector specificity).
/// The legacy value (1,0,0) tied with `#id` rules and lost on source order.
const INLINE_SPECIFICITY: (u16, u16, u16) = (u16::MAX, u16::MAX, u16::MAX);
/// Tree-wide budgets for custom property work.
const MAX_VAR_MAP_BYTES_TOTAL: usize = 16 * 1024 * 1024;
const MAX_SUBSTITUTED_BYTES_PER_ELEMENT: usize = 64 * 1024;

/// Replaced-content facts captured from an `img` element's attributes.
/// `src_data_uri` keeps only `data:image/` sources; every other source is
/// out of scope for the offline basic engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageFacts {
    pub src_data_uri: Option<String>,
    pub alt: Option<String>,
    pub attr_width: Option<u32>,
    pub attr_height: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyledNode {
    /// Original retained non-replaced phrase with no event/editing attributes.
    pub inline_phrase: bool,
    /// Retained attributes are safe for a legacy direct single-text inline.
    pub inline_attributes: bool,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub tag: String,
    /// The link target of an `a` element, as written. Page view hit testing
    /// resolves it against the document address; nothing else reads it.
    pub href: Option<String>,
    /// Replaced-content facts for an `img` element; `None` on every other node.
    pub image: Option<ImageFacts>,
    /// HTML cell spans; defaults to (1, 1). Retained for table layout.
    pub table_span: (u32, u32),
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
    /// CANDIDATE-local API growth (disclosed): true only when this tree came from the extended-CSS opt-in entry. It gates
    /// BoxTree geometry that is only admitted there (stretch and margin:auto centring of absolute leaves).
    pub ext: bool,
    /// Host-path stylesheet links the engine claimed while walking the
    /// document with its live link classifier (media fit, `disabled`, retained
    /// data: hrefs and the two-sheet ceiling included). `Some` only when the
    /// link walk finished with no refusal anywhere, so the count is the full
    /// host-path inventory of the parsed document; `None` otherwise.
    pub host_sheets_claimed: Option<usize>,
    /// Frozen interned `font-family` list texts; the `FamilyKey`s in computed styles resolve here and only here.
    /// Retention only (FL50 increment 1): nothing consumes the lists yet. `None` for a default-constructed tree.
    pub families: Option<crate::font_family::FamilyView>,
    /// BASIC-LANE generated content: declarations (and whole rules) skipped
    /// by the tight ::before/::after enrollment allowlist. Counted, never
    /// refused, so existing parse behavior is unchanged; the basic painter
    /// folds this into its skip census.
    pub generated_pseudo_skips: usize,
}

struct MatchedRule {
    selector: css::Selector,
    property: Property,
    value: Value,
    important: bool,
    source_order: u32,
    /// Index into the deferred table: a custom property definition or a
    /// declaration whose value contains `var()`. `property`/`value` are
    /// placeholders then and never reach the cascade directly.
    deferred: Option<u32>,
}

/// A declaration resolved per element after the cascade of custom properties.
struct Deferred {
    /// Custom property: exact (case-sensitive) name. Otherwise the lowercased
    /// property name.
    name: String,
    value: String,
    custom: bool,
}

enum Entry {
    Typed(Property, Value, bool),
    Deferred(u32, bool),
}

/// Parse one declaration block. Blocks with neither a custom property nor a
/// `var()` take the unchanged legacy path. Other blocks are parsed
/// declaration by declaration, preserving order.
fn block_entries(
    body: &str,
    deferred: &mut Vec<Deferred>,
    text_deco: bool,
    family_bind: bool,
    em_font: bool,
    fam: &mut crate::font_family::FamilyTable,
) -> (Vec<Entry>, bool) {
    let stripped = css::strip_comments(body);
    if !stripped.contains("--") && !custom_props::has_var(&stripped) {
        let (typed, unknown) = properties_with(body, text_deco, family_bind, em_font, fam);
        return (
            typed
                .into_iter()
                .map(|(p, v, i)| Entry::Typed(p, v, i))
                .collect(),
            unknown,
        );
    }
    let mut entries = Vec::new();
    let mut unknown = false;
    for (raw_name, raw) in style_syntax::declarations(body) {
        let name = css::strip_comments(raw_name).trim().to_owned();
        let cleaned = css::strip_comments(raw);
        let trimmed = cleaned.trim();
        let lower_tail = trimmed.to_ascii_lowercase();
        let important = lower_tail.ends_with("!important");
        let value = if important {
            trimmed[..trimmed.len() - "!important".len()].trim()
        } else {
            trimmed
        };
        // A comment separates tokens in CSS (`b/**/lue` is two tokens), but the
        // shared comment stripper deletes it. For custom properties and var()
        // values that would merge tokens, so any raw comment there refuses.
        let raw_comment = raw.contains("/*") || raw_name.contains("/*");
        if (name.starts_with("--") || custom_props::has_var(value)) && raw_comment {
            unknown = true;
            continue;
        }
        if name.starts_with("--") {
            if !custom_props::valid_name(&name)
                || !(custom_props::valid_definition(value)
                    || (em_font && custom_props::valid_definition_with_strings(value)))
            {
                unknown = true;
                continue;
            }
            deferred.push(Deferred {
                name,
                value: value.to_owned(),
                custom: true,
            });
            entries.push(Entry::Deferred(
                u32::try_from(deferred.len() - 1).unwrap_or(u32::MAX),
                important,
            ));
        } else if custom_props::has_var(value) {
            if custom_props::find_vars(value).is_none() {
                unknown = true;
                continue;
            }
            deferred.push(Deferred {
                name: name.to_ascii_lowercase(),
                value: value.to_owned(),
                custom: false,
            });
            entries.push(Entry::Deferred(
                u32::try_from(deferred.len() - 1).unwrap_or(u32::MAX),
                important,
            ));
        } else {
            let one = format!("{name}:{raw}");
            let (typed, bad) = properties_with(&one, text_deco, family_bind, em_font, fam);
            unknown |= bad;
            entries.extend(typed.into_iter().map(|(p, v, i)| Entry::Typed(p, v, i)));
        }
    }
    (entries, unknown)
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
        Self::from_dom_with(
            nodes,
            sheets,
            media,
            SupplyBudget::CANONICAL,
            false,
            false,
            false,
            None,
            false,
        )
    }

    #[allow(clippy::too_many_lines)] // One bounded tree walk retains parentage and cascade.
    pub(crate) fn from_dom_with(
        nodes: &[Node],
        sheets: &[Vec<u8>],
        media: Option<crate::media_query::MediaContext>,
        budget: SupplyBudget,
        ext_selectors: bool,
        text_deco: bool,
        family_bind: bool,
        snapshot: Option<&crate::css::SnapshotState>,
        page_tables: bool,
    ) -> Self {
        Self::from_dom_recording_sources(
            nodes, sheets, media, budget, ext_selectors, text_deco, family_bind,
            snapshot, page_tables, None,
        )
    }

    /// Opt-in same-DOM provenance for a future semantic colour path. The map
    /// is parallel to the retained style arena, not a DOM-indexed array.
    /// Generated pseudo nodes have no DOM handle. Normal parse paths allocate
    /// no map. A partial tree carries only mappings for its retained nodes.
    #[allow(dead_code)] // First bounded provenance increment, not wired to Reader.
    pub(crate) fn from_dom_with_sources(
        nodes: &[Node],
        sheets: &[Vec<u8>],
        media: Option<crate::media_query::MediaContext>,
    ) -> (Self, Vec<Option<Handle>>) {
        let mut sources = Vec::new();
        let tree = Self::from_dom_recording_sources(
            nodes, sheets, media, SupplyBudget::CANONICAL, true, true, false,
            None, true, Some(&mut sources),
        );
        (tree, sources)
    }

    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    fn from_dom_recording_sources(
        nodes: &[Node],
        sheets: &[Vec<u8>],
        media: Option<crate::media_query::MediaContext>,
        budget: SupplyBudget,
        ext_selectors: bool,
        text_deco: bool,
        family_bind: bool,
        snapshot: Option<&crate::css::SnapshotState>,
        page_tables: bool,
        mut sources: Option<&mut Vec<Option<Handle>>>,
    ) -> Self {
        let mut tree = Self::default();
        tree.ext = ext_selectors;
        let mut fam = crate::font_family::FamilyTable::new();
        let (rules, deferred_table, incomplete, claimed, pseudo_skips) = matched_rules(
            nodes,
            sheets,
            &mut tree.diagnostics,
            media,
            budget,
            ext_selectors,
            text_deco,
            family_bind,
            &mut fam,
            snapshot,
        );
        tree.generated_pseudo_skips = pseudo_skips;
        tree.host_sheets_claimed = (!incomplete).then_some(claimed);
        tree.diagnostics.expanded_declarations = rules.len();
        tree.unsupported = incomplete;
        let mut stack = vec![(
            DOCUMENT,
            None::<usize>,
            Computed::INITIAL,
            Rc::new(Map::new()),
        )];
        let mut match_work = 0_usize;
        // `:target` resolution: exactly once per tree, ONLY under a caller-
        // asserted TargetState (enrollment already refused `:target` rules
        // otherwise). Charged to the global match-work budget (one unit per
        // visited element per scan). Resolution failure flags the page
        // truncated BEFORE the walk: an unresolved target can never yield an
        // admitted paint, and no :target step is ever guessed.
        let resolved_target: Option<Option<crate::dom::Handle>> = match snapshot
            .and_then(|state| state.target.as_ref())
        {
            None => None,
            Some(target_state) => {
                let mut fuel = crate::css::EXT_FUEL;
                let resolved = crate::css::resolve_target_element(nodes, target_state, &mut fuel);
                match_work =
                    match_work.saturating_add(crate::css::EXT_FUEL as usize - fuel as usize);
                match resolved {
                    Ok(handle) if match_work <= MAX_MATCH_WORK => Some(handle),
                    _ => {
                        tree.diagnostics.record(RefusalReason::MatchWork);
                        tree.truncated = true;
                        // Unresolved: every `:target` step answers UNRESOLVED,
                        // and the truncation above already refuses the page.
                        None
                    }
                }
            }
        };
        // Ext entry only: rightmost-key prefilter over the active rule list
        // (conditions already resolved at collection). One bucket per rule,
        // values in ascending collection order. The default entry never builds
        // or consults it: its candidate set is the identity in collection
        // order, so the flat scan, per-rule charging and admission are
        // unchanged by construction. Memory: total stored indices == active
        // rules (<= MAX_RULES) plus small cloned key strings.
        let prefilter: Option<std::collections::HashMap<css::PrefilterKey, Vec<u32>>> =
            ext_selectors.then(|| {
                let mut map: std::collections::HashMap<css::PrefilterKey, Vec<u32>> =
                    std::collections::HashMap::new();
                for (index, rule) in rules.iter().enumerate() {
                    map.entry(rule.selector.prefilter_key())
                        .or_default()
                        .push(u32::try_from(index).unwrap_or(u32::MAX));
                }
                map
            });
        let identity_candidates: Vec<u32> = if prefilter.is_none() {
            (0..u32::try_from(rules.len()).unwrap_or(u32::MAX)).collect()
        } else {
            Vec::new()
        };
        let mut candidates: Vec<u32> = Vec::new();
        let mut var_map_bytes = 0_usize;
        let mut root_font_px: u32 = 16;
        // EXTENDED-CSS generated boxes (`::before`/`::after`). ONE pending list
        // for both families, sharing one GENERATED sentinel space and one
        // aggregate MAX_GENERATED bound; the bool is `is_before` (tag only).
        // Stack placement (LIFO) decides child position: a pending entry pushed
        // BEFORE the real children pops AFTER them (LAST child, ::after); one
        // pushed AFTER the real children pops FIRST (FIRST child, ::before).
        const GENERATED: usize = usize::MAX / 2;
        const MAX_GENERATED: usize = 64;
        let mut pending: Vec<(bool, Computed)> = Vec::new();
        // Set when any rule refused an element on the oversize-sibling gate.
        // Applied to `tree.truncated` only AFTER the walk, so the delivered
        // tree keeps the invariant consumers rely on (a MatchWork refusal on
        // the tree means the page is flagged incomplete) while the walk
        // itself evaluates EVERY candidate for EVERY element: exact per-pair
        // diagnostics, no unrelated rule lost.
        let mut oversize_dropped = false;
        // Per-DOM-parent visit counters: (children visited, ELEMENT children
        // visited). Children are pushed in reverse and popped in child order,
        // so when an element pops, its counters ARE its O(1) position: raw
        // child index and 1-based `:nth-child` index. This is what makes a
        // rightmost nth test and the first sibling-combinator transition
        // cost one fuel unit with ZERO repeated sibling scans; every other
        // scan charges its visited siblings to the match fuel, and the walk
        // charges consumed fuel to match_work (bounded by MAX_MATCH_WORK).
        // Memory: one entry per parent with visited children (<= node cap).
        let mut child_positions: std::collections::HashMap<Handle, (u32, u32)> =
            std::collections::HashMap::new();
        while let Some((handle, parent, inherited, inherited_vars)) = stack.pop() {
            if handle >= GENERATED {
                let (is_before, style) = pending[handle - GENERATED];
                // Generated boxes are bounded separately (MAX_GENERATED over BOTH
                // families) and never spend the DOM node cap that real nodes are
                // checked against.
                if tree.nodes.len() >= nodes.len() + pending.len() {
                    tree.truncated = true;
                    tree.diagnostics.record(RefusalReason::RetainedNodes);
                    break;
                }
                let index = tree.nodes.len();
                if let Some(sources) = sources.as_deref_mut() {
                    sources.push(None);
                }
                tree.nodes.push(StyledNode {
                    inline_phrase: false,
                    inline_attributes: false,
                    parent,
                    children: Vec::new(),
                    tag: if is_before { "::before" } else { "::after" }.to_owned(),
                    href: None,
                    image: None,
                    table_span: (1, 1),
                    text: None,
                    style,
                });
                if let Some(parent) = parent {
                    tree.nodes[parent].children.push(index);
                }
                continue;
            }
            if let Data::Text(text) = &nodes[handle].data {
                if let Some(dom_parent) = nodes[handle].parent {
                    child_positions.entry(dom_parent).or_default().0 += 1;
                }
                if tree.nodes.len() >= nodes.len() + pending.len() {
                    tree.truncated = true;
                    tree.diagnostics.record(RefusalReason::RetainedNodes);
                    break;
                }
                let index = tree.nodes.len();
                if let Some(sources) = sources.as_deref_mut() {
                    sources.push(Some(handle));
                }
                tree.nodes.push(StyledNode {
                    inline_phrase: false,
                    inline_attributes: false,
                    parent,
                    children: Vec::new(),
                    tag: "#text".to_owned(),
                    href: None,
                    image: None,
                    table_span: (1, 1),
                    text: Some(text.clone()),
                    style: inherited,
                });
                if let Some(parent) = parent {
                    tree.nodes[parent].children.push(index);
                }
                continue;
            }
            let Data::Element { name, attrs, .. } = &nodes[handle].data else {
                if !matches!(nodes[handle].data, Data::Document) {
                    if let Some(dom_parent) = nodes[handle].parent {
                        child_positions.entry(dom_parent).or_default().0 += 1;
                    }
                }
                for &child in nodes[handle].children.iter().rev() {
                    stack.push((child, parent, inherited, Rc::clone(&inherited_vars)));
                }
                continue;
            };
            if tree.nodes.len() >= nodes.len() + pending.len() || match_work >= MAX_MATCH_WORK {
                tree.diagnostics.record(if match_work >= MAX_MATCH_WORK {
                    RefusalReason::MatchWork
                } else {
                    RefusalReason::RetainedNodes
                });
                tree.truncated = true;
                break;
            }
            // O(1) position of THIS element among its DOM parent's children.
            let position = nodes[handle].parent.map(|dom_parent| {
                let entry = child_positions.entry(dom_parent).or_default();
                // (0-based raw child index, 1-based :nth-child element index)
                let position = (entry.0, entry.1 + 1);
                entry.0 += 1;
                entry.1 += 1;
                position
            });
            let mut declarations = Vec::new();
            let mut local_deferred: Vec<Deferred> = Vec::new();
            let mut inline_hits: Vec<(u32, bool, (u16, u16, u16), u32)> = Vec::new();
            // (deferred index, important, specificity, source order) of matched
            // custom property definitions and var()-bearing declarations.
            let mut hits: Vec<(u32, bool, (u16, u16, u16), u32)> = Vec::new();
            // Candidates in ascending global collection order (sorted+deduped),
            // so test order, charge order and the declaration-cap trip points are
            // identical to the flat scan; only guaranteed-false tests are
            // skipped, which is the intended ext accounting delta. Bounded
            // allocation: one reused Vec of at most MAX_RULES indices per
            // element; no DOM data retained.
            let candidate_slice: &[u32] = if let Some(prefilter) = &prefilter {
                candidates.clear();
                for key in ext_element_keys(nodes, handle)
                    .iter()
                    .chain(std::iter::once(&css::PrefilterKey::Universal))
                {
                    if let Some(v) = prefilter.get(key) {
                        candidates.extend_from_slice(v);
                    }
                }
                candidates.sort_unstable();
                candidates.dedup();
                &candidates
            } else {
                &identity_candidates
            };
            for &candidate_index in candidate_slice {
                let rule = &rules[candidate_index as usize];
                // Pseudo rules never select an element here; they are charged exactly
                // once, in the pseudo pass below.
                if rule.selector.pseudo != crate::css::Pseudo::None {
                    continue;
                }
                match_work += 1;
                if match_work > MAX_MATCH_WORK {
                    tree.diagnostics.record(RefusalReason::MatchWork);
                    tree.truncated = true;
                    break;
                }
                let mut fuel = css::EXT_FUEL;
                let outcome = rule.selector.matches_checked_scoped_budget_with_snapshot(
                    nodes,
                    handle,
                    &mut fuel,
                    position,
                    resolved_target,
                    snapshot,
                );
                // Charge what the match actually consumed (selector steps and
                // every visited sibling in a scan) to the global work budget;
                // one unit was already charged for the test itself.
                match_work =
                    match_work.saturating_add(((css::EXT_FUEL - fuel) as usize).saturating_sub(1));
                if match_work > MAX_MATCH_WORK {
                    tree.diagnostics.record(RefusalReason::MatchWork);
                    tree.truncated = true;
                    break;
                }
                let selected = match outcome {
                    Ok(Some(selected)) => selected,
                    // Rule-scoped refusal (sibling list over EXT_MAX_SIBLINGS):
                    // this rule does not apply to THIS element. One precise
                    // diagnostic per refused rule-element pair; no truncation,
                    // no skipped candidates, every other rule unaffected.
                    Err(crate::css::MatchRefusal::OversizeSiblings) => {
                        tree.diagnostics.record(RefusalReason::MatchWork);
                        // A potentially-matching rule was DROPPED for this
                        // element: flag the page incomplete AFTER the walk
                        // (see oversize_dropped), so diagnostics stay exact
                        // and unrelated rules still compute, while the
                        // all-or-nothing painter confinement stays intact.
                        oversize_dropped = true;
                        continue;
                    }
                    // Fuel exhaustion stays a global work guard: truncate.
                    Err(crate::css::MatchRefusal::Fuel) => {
                        tree.diagnostics.record(RefusalReason::MatchWork);
                        tree.truncated = true;
                        break;
                    }
                    Ok(None) => false,
                };
                if selected {
                    if let Some(index) = rule.deferred {
                        hits.push((
                            index,
                            rule.important,
                            rule.selector.specificity,
                            rule.source_order,
                        ));
                        if hits.len() + declarations.len() >= MAX_DECLARATIONS_PER_ELEMENT {
                            tree.diagnostics.record(RefusalReason::MatchedDeclarations);
                            tree.truncated = true;
                            break;
                        }
                        continue;
                    }
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
            // Pseudo-element rules whose ORIGINATING element is this one. Deferred
            // (var()) and rem-valued declarations are not resolved for
            // pseudo-elements: refuse.
            let mut pseudo_decls: Vec<Declaration> = Vec::new();
            let mut pseudo_decls_before: Vec<Declaration> = Vec::new();
            let mut pseudo_hit = false;
            let mut pseudo_hit_before = false;
            let mut pseudo_bad = false;
            for &candidate_index in candidate_slice {
                let rule = &rules[candidate_index as usize];
                if rule.selector.pseudo == crate::css::Pseudo::None {
                    continue;
                }
                match_work += 1;
                if match_work > MAX_MATCH_WORK {
                    tree.diagnostics.record(RefusalReason::MatchWork);
                    tree.truncated = true;
                    break;
                }
                let mut fuel = css::EXT_FUEL;
                let outcome = rule
                    .selector
                    .matches_originating_checked_scoped_budget_with_snapshot(
                        nodes,
                        handle,
                        &mut fuel,
                        position,
                        resolved_target,
                        snapshot,
                    );
                match_work =
                    match_work.saturating_add(((css::EXT_FUEL - fuel) as usize).saturating_sub(1));
                if match_work > MAX_MATCH_WORK {
                    tree.diagnostics.record(RefusalReason::MatchWork);
                    tree.truncated = true;
                    break;
                }
                match outcome {
                    Err(crate::css::MatchRefusal::OversizeSiblings) => {
                        tree.diagnostics.record(RefusalReason::MatchWork);
                        // Same deferred page-completeness flag as the
                        // element pass.
                        oversize_dropped = true;
                        continue;
                    }
                    Err(crate::css::MatchRefusal::Fuel) => {
                        tree.diagnostics.record(RefusalReason::MatchWork);
                        tree.truncated = true;
                        break;
                    }
                    Ok(None) | Ok(Some(false)) => {}
                    Ok(Some(true)) => {
                        let is_before = rule.selector.pseudo == crate::css::Pseudo::Before;
                        if is_before {
                            pseudo_hit_before = true;
                        } else {
                            pseudo_hit = true;
                        }
                        // CONSERVATIVE LIMIT (not a Chrome claim): only
                        // block-container originating elements generate boxes
                        // here. Chrome CAN generate on other elements (e.g. a
                        // span); anything outside this allowlist refuses
                        // rather than guesses.
                        let allowed = matches!(
                            &nodes[handle].data,
                            Data::Element { name, .. }
                                if matches!(
                                    name.local.as_ref(),
                                    "div" | "section" | "article" | "header" | "footer" | "main"
                                        | "nav" | "aside" | "blockquote" | "figure"
                                )
                        )
                        // Generated decoration on a `span` (the hamburger-bar
                        // case), EITHER lane. A span-admitted rule is confined
                        // to the tight basic declaration allowlist below;
                        // block-container ext rules keep full declarations.
                        || (true
                            && matches!(
                                &nodes[handle].data,
                                Data::Element { name, .. } if name.local.as_ref() == "span"
                            ));
                        let block_allowed = matches!(
                            &nodes[handle].data,
                            Data::Element { name, .. }
                                if matches!(
                                    name.local.as_ref(),
                                    "div" | "section" | "article" | "header" | "footer" | "main"
                                        | "nav" | "aside" | "blockquote" | "figure"
                                )
                        );
                        let via_span = allowed && !block_allowed;
                        // Span-admitted rules enroll ONLY the tight
                        // generated-decoration allowlist; anything else
                        // (deferred var() included) is skipped and counted at
                        // the top level, never refused.
                        if via_span
                            && !pseudo_basic_entry_admitted(
                                rule.property,
                                &rule.value,
                                rule.deferred.is_some(),
                            )
                        {
                            tree.generated_pseudo_skips += 1;
                            continue;
                        }
                        if rule.deferred.is_some() || !allowed {
                            pseudo_bad = true;
                        } else {
                            let decls = if is_before {
                                &mut pseudo_decls_before
                            } else {
                                &mut pseudo_decls
                            };
                            decls.push(Declaration {
                                property: rule.property,
                                value: rule.value,
                                origin: Origin::Author,
                                important: rule.important,
                                specificity: rule.selector.specificity,
                                source_order: rule.source_order,
                            });
                            // Same boundary as an element: the 256th matched declaration refuses.
                            if decls.len() == MAX_DECLARATIONS_PER_ELEMENT {
                                tree.diagnostics.record(RefusalReason::MatchedDeclarations);
                                tree.truncated = true;
                                break;
                            }
                        }
                    }
                }
            }
            if tree.truncated {
                break;
            }
            if let Some(inline) = css::attribute(nodes, handle, "style") {
                let mut inline_table = Vec::new();
                let (entries, unknown) = block_entries(
                    inline,
                    &mut inline_table,
                    text_deco,
                    family_bind,
                    ext_selectors,
                    &mut fam,
                );
                let mut parsed: Vec<(Property, Value, bool, u32)> = Vec::new();
                for (offset, entry) in entries.iter().enumerate() {
                    // ONE authored offset per entry, shared by typed and deferred
                    // inline declarations, so same-specificity ties follow source order.
                    match *entry {
                        Entry::Typed(p, v, i) => {
                            parsed.push((p, v, i, u32::try_from(offset).unwrap_or(u32::MAX)));
                        }
                        Entry::Deferred(index, important) => {
                            // Inline deferred entries join the same per-element
                            // resolution with inline specificity and order.
                            let slot = u32::try_from(local_deferred.len()).unwrap_or(u32::MAX);
                            local_deferred.push(Deferred {
                                name: inline_table[index as usize].name.clone(),
                                value: inline_table[index as usize].value.clone(),
                                custom: inline_table[index as usize].custom,
                            });
                            inline_hits.push((
                                slot,
                                important,
                                INLINE_SPECIFICITY,
                                u32::try_from(offset).unwrap_or(u32::MAX),
                            ));
                        }
                    }
                }
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
                declarations.extend(parsed.into_iter().map(
                    |(property, value, important, offset)| Declaration {
                        property,
                        value,
                        origin: Origin::Author,
                        important,
                        specificity: INLINE_SPECIFICITY,
                        source_order: offset,
                    },
                ));
            }
            // CANDIDATE (extended-CSS opt-in entry only). UA presentation the base never modelled (Chrome 156 computed
            // style, unforced light scheme, 16px parent; private-evidence ua1): these elements were admitted as plain
            // text (e.g. an `h1` as 16px normal weight with no margins, a `ul` without 40px padding). Under this
            // entry each listed element must have an AUTHOR declaration (any origin-Author value, inline or sheet) for
            // every property Chrome's UA sheet sets away from what the engine computes; otherwise the page refuses.
            // `None` = refused unconditionally (form controls, details/summary disclosure, fieldset/legend, sub/sup,
            // pre/textarea white-space, center; fl11: table family, hr, input, iframe, ruby/rt, q; fl12: img, svg, video, audio, canvas, object, embed, hgroup: Chrome presentation never modelled,
            // private-evidence ua2). Over-refusal by design; the default entry is unchanged. This checks
            // that the property is authored with a non-revert value, not that the author value is supported (unsupported values refuse on
            // their own). Elements not listed are NOT audited here.
            let table_display = page_tables
                .then(|| basic_table_display(&name.local))
                .flatten();
            declarations.push(Declaration {
                property: Property::Display,
                value: Value::Display(table_display.unwrap_or_else(|| {
                    if ext_selectors {
                        ua_display_ext(&name.local)
                    } else {
                        ua_display(&name.local)
                    }
                })),
                origin: Origin::UserAgent,
                important: false,
                specificity: (0, 0, 0),
                source_order: 0,
            });
            // CANDIDATE (extended-CSS opt-in entry only). The HTML user-agent sheet underlines `a[href]`, `u` and
            // `ins`, and colours `a[href]` rgb(0,0,238) (unvisited; a static render has no visited state). The base
            // did not model either, so an unstyled link was admitted and painted black without an underline while
            // Chrome draws it blue and underlined. Under this entry the UA values are declared (author declarations
            // still win by origin): underline is refused by the paint-tree gate unless an author turns it off with
            // `text-decoration: none` (admitted below). `s`/`strike`/`del` get an unpainted decoration below; so do `abbr[title]`/`acronym[title]` (dotted underline); `mark` gets its UA yellow background/black text above. Default entry unchanged.
            if ext_selectors
                && (matches!(name.local.as_ref(), "s" | "strike" | "del")
                    || (matches!(name.local.as_ref(), "abbr" | "acronym")
                        && css::attribute(nodes, handle, "title").is_some()))
            {
                // UA line-through (s/strike/del) and dotted underline (abbr/acronym with title): unpainted, so the page
                // is refused unless the author sets `text-decoration: none`.
                declarations.push(Declaration {
                    property: Property::TextDecoration,
                    value: Value::TextDecoration(crate::computed_style::TextDecoration::Unpainted),
                    origin: Origin::UserAgent,
                    important: false,
                    specificity: (0, 0, 0),
                    source_order: 0,
                });
            }
            // `mark`: Chrome 156 (unforced, light scheme) computes background rgb(255,255,0) and color rgb(0,0,0).
            if ext_selectors && name.local.as_ref() == "mark" {
                for (property, value) in [
                    (
                        Property::BackgroundColor,
                        Value::BackgroundColor(Some(0xff_ff_00)),
                    ),
                    (Property::Color, Value::Color(0x00_00_00)),
                ] {
                    declarations.push(Declaration {
                        property,
                        value,
                        origin: Origin::UserAgent,
                        important: false,
                        specificity: (0, 0, 0),
                        source_order: 0,
                    });
                }
            }
            if ext_selectors
                && (matches!(name.local.as_ref(), "u" | "ins")
                    || (name.local.as_ref() == "a"
                        && css::attribute(nodes, handle, "href").is_some()))
            {
                declarations.push(Declaration {
                    property: Property::TextDecoration,
                    value: Value::TextDecoration(crate::computed_style::TextDecoration::Underline),
                    origin: Origin::UserAgent,
                    important: false,
                    specificity: (0, 0, 0),
                    source_order: 0,
                });
                if name.local.as_ref() == "a" {
                    declarations.push(Declaration {
                        property: Property::Color,
                        value: Value::Color(0x00_00_ee),
                        origin: Origin::UserAgent,
                        important: false,
                        specificity: (0, 0, 0),
                        source_order: 0,
                    });
                }
            }
            if matches!(name.local.as_ref(), "ul" | "ol") {
                // The HTML user-agent sheet declares list-style-type on ul/ol (disc, circle for nested
                // ul, decimal), so an AUTHOR `none` on an ancestor (body, div) does not reach their li
                // children. Every UA value is the unmodelled default marker (`ListStyleNone(false)`).
                declarations.push(Declaration {
                    property: Property::ListStyleType,
                    value: Value::ListStyleNone(false),
                    origin: Origin::UserAgent,
                    important: false,
                    specificity: (0, 0, 0),
                    source_order: 0,
                });
            }
            // CANDIDATE (extended-CSS opt-in entry only; default entry byte-for-byte unchanged). Chrome's UA sheet:
            // `ul, ol { margin-block: 1em; padding-inline-start: 40px }`, margins zeroed when any dl/ul/ol/menu/dir
            // ANCESTOR exists (same UA rule group and the same ancestor walk as dl below). Margins are 1em resolved
            // against the element's OWN computed font size (whole px, exact); padding-left is the physical mapping
            // of padding-inline-start, valid only for horizontal-tb + ltr: `dir` attributes are refused upstream and
            // a computed RTL ul/ol is refused below (mirroring dd). Admission-by-supply: the required-props audit
            // drops its ul/ol arm in the same gated change, so ul/ol join the dl/dt/dd wildcard class. List markers
            // (disc/decimal) stay unmodelled (the ListStyleType supply above is unchanged); no paint/readiness scope.
            if ext_selectors && matches!(name.local.as_ref(), "ul" | "ol") {
                let mut list_ancestor = false;
                let mut up = parent;
                while let Some(i) = up {
                    if matches!(
                        tree.nodes[i].tag.as_str(),
                        "dl" | "ul" | "ol" | "menu" | "dir"
                    ) {
                        list_ancestor = true;
                        break;
                    }
                    up = tree.nodes[i].parent;
                }
                if !list_ancestor {
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
                declarations.push(Declaration {
                    property: Property::PaddingLeft,
                    value: Value::Padding(40),
                    origin: Origin::UserAgent,
                    important: false,
                    specificity: (0, 0, 0),
                    source_order: 0,
                });
            }
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
            // CANDIDATE (U1, extended-CSS opt-in entry only; default entry byte-for-byte unchanged). Chrome's UA sheet:
            // `dl { display:block; margin-block:1em }` (0 when any dl/ul/ol/menu/dir ANCESTOR exists), `dt { display:block }`,
            // `dd { display:block; margin-inline-start:40px }`. Display comes from ua_display_ext (single declaration above).
            // The dd 40px is physical margin-left, valid only for horizontal-tb + ltr: a computed RTL dd is refused below, and
            // non-horizontal-tb writing modes / `dir` attributes are refused by their existing paths. The required-props
            // audit below is deliberately UNCHANGED in this batch (these declarations are UserAgent origin and cannot satisfy it).
            if ext_selectors && name.local.as_ref() == "dl" {
                let mut list_ancestor = false;
                let mut up = parent;
                while let Some(i) = up {
                    if matches!(
                        tree.nodes[i].tag.as_str(),
                        "dl" | "ul" | "ol" | "menu" | "dir"
                    ) {
                        list_ancestor = true;
                        break;
                    }
                    up = tree.nodes[i].parent;
                }
                if !list_ancestor {
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
            }
            if ext_selectors && name.local.as_ref() == "dd" {
                declarations.push(Declaration {
                    property: Property::MarginLeft,
                    value: Value::Margin(Margin::Px(40)),
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
            // Custom properties: cascade the winning definitions, compute this
            // element's map from the parent's, then substitute var() in every
            // matched declaration with the SAME cascade key it was authored
            // with. Anything not implemented refuses.
            let mut vars_now = Rc::clone(&inherited_vars);
            let mut var_refused = false;
            if !hits.is_empty() || !inline_hits.is_empty() {
                let mut all: Vec<(&Deferred, bool, (u16, u16, u16), u32)> = Vec::new();
                for &(i, imp, sp, ord) in &hits {
                    all.push((&deferred_table[i as usize], imp, sp, ord));
                }
                for &(i, imp, sp, ord) in &inline_hits {
                    all.push((&local_deferred[i as usize], imp, sp, ord));
                }
                type Key = (bool, (u16, u16, u16), u32);
                let mut winners: BTreeMap<&str, (Key, &str)> = BTreeMap::new();
                for &(d, imp, sp, ord) in &all {
                    if !d.custom {
                        continue;
                    }
                    let key: Key = (imp, sp, ord);
                    if winners.get(d.name.as_str()).is_none_or(|(k, _)| *k < key) {
                        winners.insert(d.name.as_str(), (key, d.value.as_str()));
                    }
                }
                let mut local = BTreeMap::new();
                for (name, (_, value)) in &winners {
                    match custom_props::classify(value) {
                        Ok(def) => {
                            local.insert((*name).to_owned(), def);
                        }
                        Err(_) => var_refused = true,
                    }
                }
                if !var_refused {
                    match custom_props::compute_map(&inherited_vars, &local) {
                        Ok(map) => {
                            var_map_bytes =
                                var_map_bytes.saturating_add(custom_props::map_bytes(&map));
                            if var_map_bytes > MAX_VAR_MAP_BYTES_TOTAL {
                                var_refused = true;
                            }
                            vars_now = Rc::new(map);
                        }
                        Err(_) => var_refused = true,
                    }
                }
                let _: Option<&Def> = None;
                let mut substituted_total = 0_usize;
                if !var_refused {
                    for &(d, imp, sp, ord) in &all {
                        if d.custom {
                            continue;
                        }
                        let mut lookup = |n: &str| Ok(vars_now.get(n).cloned());
                        match custom_props::substitute(&d.value, &mut lookup, 0) {
                            Ok(sub) => {
                                substituted_total = substituted_total.saturating_add(sub.len());
                                if substituted_total > MAX_SUBSTITUTED_BYTES_PER_ELEMENT {
                                    var_refused = true;
                                    break;
                                }
                                let text = format!(
                                    "{}:{}{}",
                                    d.name,
                                    sub,
                                    if imp { " !important" } else { "" }
                                );
                                let (typed, bad) = properties_with(
                                    &text,
                                    text_deco,
                                    family_bind,
                                    ext_selectors,
                                    &mut fam,
                                );
                                var_refused |= bad;
                                for (property, value, important) in typed {
                                    declarations.push(Declaration {
                                        property,
                                        value,
                                        origin: Origin::Author,
                                        important,
                                        specificity: sp,
                                        source_order: ord,
                                    });
                                }
                            }
                            Err(_) => var_refused = true,
                        }
                    }
                }
            }
            if var_refused {
                tree.diagnostics.record(RefusalReason::ComputedValue);
                tree.unsupported = true;
            }
            // (audit runs AFTER var() substitution so var-resolved declarations are the ones inspected.
            // A var resolving to `revert` never reaches here (refused upstream by classify, page unsupported).)
            if ext_selectors {
                let required: Option<&[Property]> = match name.local.as_ref() {
                    "h1" | "h2" | "h3" | "h5" | "h6" => Some(&[
                        Property::FontSize,
                        Property::FontWeight,
                        Property::MarginTop,
                        Property::MarginBottom,
                    ]),
                    "h4" => Some(&[
                        Property::FontWeight,
                        Property::MarginTop,
                        Property::MarginBottom,
                    ]),
                    "blockquote" | "figure" => Some(&[
                        Property::MarginTop,
                        Property::MarginBottom,
                        Property::MarginLeft,
                        Property::MarginRight,
                    ]),
                    "code" | "samp" | "kbd" | "tt" => Some(&[Property::FontFamily]),
                    "small" | "big" => Some(&[Property::FontSize]),
                    // dl/dt/dd have no arm on purpose (U2): their defaults are modelled (ext-only UA-origin declarations plus the
                    // computed-RTL dd refusal), so they fall to the wildcard `Some(&[])` and need no author declaration.
                    // ul/ol join that class in the same gated change that supplies their UA defaults above
                    // (margin-block 1em when no list ancestor, padding-inline-start 40px, computed-RTL refused).
                    "address" => Some(&[Property::Display, Property::FontStyle]),
                    "cite" | "dfn" | "var" => Some(&[Property::FontStyle]),
                    "pre" | "sub" | "sup" | "button" | "textarea" | "select" | "details"
                    | "summary" | "fieldset" | "legend" | "center" | "table" | "caption"
                    | "colgroup" | "col" | "thead" | "tbody" | "tfoot" | "tr" | "th" | "td"
                    | "hr" | "input" | "iframe" | "ruby" | "rt" | "q" | "img" | "svg" | "video"
                    | "audio" | "canvas" | "object" | "embed" | "hgroup" => None,
                    _ => Some(&[]),
                };
                let missing = match required {
                    None => true,
                    // `revert` (any author declaration of the property, winning or not: conservative) hands the
                    // property back to the UA sheet, which Chrome applies (h1 32px/700/21.44px) but this engine does not
                    // model (it falls back to inherited/initial), so it is not a replacement.
                    // a var() with no definition and no fallback REFUSES the whole page (substitution Err => var_refused), it is not an
                    // admitted replacement; `--r:revert` through var() is Chrome unset-like but refused upstream by classify (no unset
                    // emulation). unset/initial/inherit literals are replacements only when otherwise supported (Chrome probe ua1/revert).
                    Some(props) => props.iter().any(|want| {
                        !declarations
                            .iter()
                            .any(|d| d.origin == Origin::Author && d.property == *want)
                            || declarations.iter().any(|d| {
                                d.origin == Origin::Author
                                    && d.property == *want
                                    && matches!(d.value, Value::Revert)
                            })
                    }),
                };
                if missing {
                    tree.diagnostics.record(RefusalReason::CssDeclaration);
                    tree.unsupported = true;
                }
            }
            let rem_refused = normalize_rem(&mut declarations, root_font_px);
            if name.local.as_ref() == "pre" {
                declarations.insert(
                    0,
                    Declaration {
                        property: Property::WhiteSpace,
                        value: Value::WhiteSpacePre,
                        origin: Origin::UserAgent,
                        important: false,
                        specificity: (0, 0, 0),
                        source_order: 0,
                    },
                );
            }
            let style = Computed::cascade(Some(inherited), &declarations);
            if style.white_space_pre || style.unicode_bidi != crate::computed_style::UnicodeBidi::Normal {
                tree.unsupported = true;
            }
            if parent.is_none() {
                root_font_px = style.font_size;
            }
            // `content` is meaningful only on a pseudo-element; on an element any
            // non-initial value is refused rather than guessed.
            let refused = rem_refused
                || (ext_selectors
                    && matches!(name.local.as_ref(), "dd" | "ul" | "ol")
                    && style.direction == crate::computed_style::Direction::Rtl)
                || style.positioned.content != crate::computed_style::Content::Normal
                || style.line_height == Some(0)
                || style.font_size == 0
                || crate::computed_style::length_em_unresolved(style.width)
                || crate::computed_style::length_em_unresolved(style.height)
                || [
                    style.padding_top,
                    style.padding_right,
                    style.padding_bottom,
                    style.padding_left,
                ]
                .contains(&crate::computed_style::PADDING_UNREPRESENTABLE);
            if refused {
                tree.diagnostics.record(RefusalReason::ComputedValue);
            }
            tree.unsupported |= refused;
            // CANDIDATE (FL46, soundness): three presentational HTML attributes change the used style through the UA sheet and were
            // ignored (Chrome oracle private-evidence ul1/at65 w01/w08/w15: the box painted although Chrome hides it, or a child sat at
            // the wrong side). `hidden` (any value but `until-found`, which only sets content-visibility) and `popover` are display:none
            // in the UA sheet; `dir` other than `ltr` sets `direction`. This build models none of them, so the page refuses (all entries).
            let attr_refused = attrs.iter().any(|(attr, value)| match attr.as_ref() {
                "hidden" => !value.trim().eq_ignore_ascii_case("until-found"),
                "popover" => true,
                "dir" => !value.trim().eq_ignore_ascii_case("ltr"),
                // CANDIDATE (FL47, soundness): presentational hints. `align` maps to text-align (-webkit-center/right on a div centres or
                // right-aligns BLOCK children too: ul1/bg66 b1,b2,b3,c5,c7 differed by 200 px); refused on every element (a few tags ignore
                // it, e.g. section d1, but this build does not model which). On `body` the legacy colour/margin attributes set the canvas or
                // body background, text colour and margins (a1,a2,c9,c1,c2 differed). Author declarations beat hints, so a body that
                // overrides them with an explicit style would match (c8), but that is not modelled either: refuse.
                "align" => true,
                "bgcolor" | "background" | "text" | "link" | "vlink" | "alink" | "leftmargin"
                | "rightmargin" | "topmargin" | "bottommargin" | "marginwidth" | "marginheight" => {
                    name.local.as_ref() == "body"
                }
                _ => false,
            });
            if attr_refused {
                tree.diagnostics.record(RefusalReason::DroppedAttributes);
                tree.unsupported = true;
            }
            let index = tree.nodes.len();
            if let Some(sources) = sources.as_deref_mut() {
                sources.push(Some(handle));
            }
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
                href: if name.local.as_ref() == "a" {
                    attrs
                        .iter()
                        .find(|(attr, _)| attr.as_ref() == "href")
                        .map(|(_, value)| value.clone())
                } else {
                    None
                },
                image: if name.local.as_ref() == "img" {
                    let attr = |key: &str| {
                        attrs
                            .iter()
                            .find(|(attr, _)| attr.as_ref() == key)
                            .map(|(_, value)| value.clone())
                    };
                    let dim = |key: &str| attr(key).and_then(|v| v.parse::<u32>().ok());
                    Some(ImageFacts {
                        src_data_uri: attr("src").filter(|s| s.starts_with("data:image/")),
                        alt: attr("alt"),
                        attr_width: dim("width"),
                        attr_height: dim("height"),
                    })
                } else {
                    None
                },
                table_span: {
                    let span = |key: &str| {
                        attrs
                            .iter()
                            .find(|(attr, _)| attr.as_ref() == key)
                            .and_then(|(_, value)| value.parse::<u32>().ok())
                            .unwrap_or(1)
                    };
                    (span("colspan"), span("rowspan"))
                },
                text: None,
                style,
            });
            if let Some(parent) = parent {
                tree.nodes[parent].children.push(index);
            }
            let mut before_handle: Option<(usize, Computed)> = None;
            if pseudo_bad {
                tree.diagnostics.record(RefusalReason::CssDeclaration);
                tree.unsupported = true;
            } else {
                // One shared cascade/push path per family, both bounded by the
                // ONE aggregate MAX_GENERATED over `pending` (both families).
                for (is_before, hit, decls) in [
                    (true, pseudo_hit_before, &mut pseudo_decls_before),
                    (false, pseudo_hit, &mut pseudo_decls),
                ] {
                    if !hit {
                        continue;
                    }
                    // Same normalisation and validation as an element's own cascade.
                    let pseudo_rem_refused = normalize_rem(decls, root_font_px);
                    let pstyle = Computed::cascade(Some(style), decls);
                    let pseudo_refused = pseudo_rem_refused
                        || pstyle.line_height == Some(0)
                        || pstyle.font_size == 0
                        || crate::computed_style::length_em_unresolved(pstyle.width)
                        || crate::computed_style::length_em_unresolved(pstyle.height)
                        || [
                            pstyle.padding_top,
                            pstyle.padding_right,
                            pstyle.padding_bottom,
                            pstyle.padding_left,
                        ]
                        .contains(&crate::computed_style::PADDING_UNREPRESENTABLE);
                    if pseudo_refused {
                        tree.diagnostics.record(RefusalReason::ComputedValue);
                        tree.unsupported = true;
                    }
                    // content normal/none: no box. content "": a box unless display:none.
                    if pstyle.positioned.content == crate::computed_style::Content::Empty
                        && pstyle.display != Display::None
                    {
                        if pending.len() >= MAX_GENERATED {
                            tree.diagnostics.record(RefusalReason::RetainedNodes);
                            tree.truncated = true;
                            break;
                        }
                        pending.push((is_before, pstyle));
                        // LIFO placement: the ::after entry goes on the stack NOW
                        // (below the real children pushed afterwards) so it pops
                        // LAST; the ::before entry is pushed after the children
                        // (see below) so it pops FIRST.
                        if is_before {
                            before_handle = Some((GENERATED + pending.len() - 1, pstyle));
                        } else {
                            stack.push((
                                GENERATED + pending.len() - 1,
                                Some(index),
                                pstyle,
                                Rc::clone(&vars_now),
                            ));
                        }
                    }
                }
            }
            if tree.truncated {
                break;
            }
            for &child in nodes[handle].children.iter().rev() {
                stack.push((child, Some(index), style, Rc::clone(&vars_now)));
            }
            // ::before pops FIRST: pushed after the real children.
            if let Some((gen, pstyle)) = before_handle {
                stack.push((gen, Some(index), pstyle, Rc::clone(&vars_now)));
            }
        }
        // Delivered-tree invariant: any oversize-sibling refusal during the
        // walk flags the WHOLE page incomplete (truncated), so no consumer
        // can present a page with a dropped potential match as complete.
        if oversize_dropped {
            tree.truncated = true;
        }
        tree.diagnostics.retained_nodes = tree.nodes.len();
        tree.diagnostics.match_attempts = match_work;
        tree.families = Some(fam.freeze());
        tree
    }
}

/// MEASUREMENT-ONLY rem lengths and `font-size` rem against the root computed font size,
/// shared by element declarations and matched `::after` declarations. Returns true when a
/// value is not representable (refused as ComputedValue).
fn normalize_rem(declarations: &mut [Declaration], root_font_px: u32) -> bool {
    let mut rem_refused = false;
    for declaration in declarations.iter_mut() {
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
            Value::WidthRem(tt) | Value::HeightRem(tt) => {
                let product = u64::from(tt) * u64::from(root_font_px);
                let px = (product % 10_000 == 0)
                    .then(|| u32::try_from(product / 10_000).ok())
                    .flatten();
                match px {
                    Some(px) => {
                        declaration.value = if matches!(declaration.value, Value::WidthRem(_)) {
                            Value::Width(Length::Px(px))
                        } else {
                            Value::Height(Length::Px(px))
                        };
                    }
                    None => rem_refused = true,
                }
            }
            Value::MaxWidthRem(tt) | Value::MinWidthRem(tt) | Value::MinHeightRem(tt) => {
                let product = u64::from(tt) * u64::from(root_font_px);
                let px = (product % 10_000 == 0)
                    .then(|| u32::try_from(product / 10_000).ok())
                    .flatten();
                match px {
                    Some(px) => {
                        declaration.value = match declaration.value {
                            Value::MaxWidthRem(_) => Value::MaxWidth(Length::Px(px)),
                            Value::MinWidthRem(_) => Value::MinWidth(Length::Px(px)),
                            _ => Value::MinHeight(Length::Px(px)),
                        };
                    }
                    None => rem_refused = true,
                }
            }
            Value::Padding(v)
                if v & crate::computed_style::PADDING_REM_FLAG != 0
                    && v & crate::computed_style::PADDING_EM_FLAG == 0
                    && matches!(
                        declaration.property,
                        Property::PaddingTop
                            | Property::PaddingRight
                            | Property::PaddingBottom
                            | Property::PaddingLeft
                    ) =>
            {
                let product = u64::from(v & !crate::computed_style::PADDING_REM_FLAG)
                    * u64::from(root_font_px);
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
                product
                    .filter(|p| p % 10_000 == 0)
                    .map_or(0, |p| p / 10_000),
            );
        }
    }
    rem_refused
}

/// Extended-CSS entry only: `ua_display` plus Chrome's block display for exactly `dl`, `dt`, `dd`.
/// address/hr/q and every other tag are unchanged. The default entry keeps using `ua_display`.
fn ua_display_ext(tag: &str) -> Display {
    match tag {
        "dl" | "dt" | "dd" => Display::Block,
        _ => ua_display(tag),
    }
}

fn basic_table_display(tag: &str) -> Option<Display> {
    Some(match tag {
        "table" => Display::Table,
        "thead" | "tbody" | "tfoot" => Display::TableRowGroup,
        "tr" => Display::TableRow,
        "td" | "th" => Display::TableCell,
        "caption" => Display::TableCaption,
        "col" | "colgroup" => Display::TableColumn,
        _ => return None,
    })
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

/// Result of the host-side link census: which external stylesheets the parse will ask the host to supply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostSheetLinks {
    /// One entry per external (non-`data:`) screen-fit `rel=stylesheet` link, in the order the parse indexes `sheets`.
    /// `None` is a link the host cannot fetch (no usable href, or a scheme other than http/https).
    pub links: Vec<Option<crate::url::Url>>,
    /// `data:` stylesheet links; they consume a ceiling slot but no host slot.
    pub data_links: usize,
    /// Links whose `media` the parse refuses as unknown (`StylesheetMedia`); the parse is already doomed.
    pub unknown_media: usize,
    /// One entry per enabled `rel=stylesheet` link in document order, including links skipped by `media`:
    /// true only when the link consumes a host slot (screen-fit and external). A host that keeps one file per
    /// link, in link order, keeps exactly the entries where this is true.
    pub link_takes_host_slot: Vec<bool>,
    /// False when the input or the DOM was cut short or attributes were dropped, so the census is not an inventory.
    pub complete: bool,
}

/// Same predicates as the link arm of `matched_rules`, without touching it: `rel` has a `stylesheet` token, no
/// `disabled`, `media` evaluated by the same function, `data:` href classified as there. Cross-checked against
/// `host_sheets_claimed` in tests. The base address follows the document converter: the first `<base href>`.
pub(crate) fn host_sheet_links_in(
    nodes: &[Node],
    media: Option<crate::media_query::MediaContext>,
    page: &crate::url::Url,
    complete: bool,
) -> HostSheetLinks {
    let mut base = page.clone();
    let mut stack = vec![DOCUMENT];
    while let Some(handle) = stack.pop() {
        if let Data::Element { name, .. } = &nodes[handle].data {
            if &*name.local == "base" {
                if let Some(href) = css::attribute(nodes, handle, "href") {
                    if let Ok(next) = base.join(href) {
                        base = next;
                    }
                    break;
                }
            }
        }
        stack.extend(nodes[handle].children.iter().rev());
    }
    let mut out = HostSheetLinks {
        links: Vec::new(),
        data_links: 0,
        unknown_media: 0,
        link_takes_host_slot: Vec::new(),
        complete,
    };
    let mut stack = vec![DOCUMENT];
    while let Some(handle) = stack.pop() {
        stack.extend(nodes[handle].children.iter().rev());
        let Data::Element { name, .. } = &nodes[handle].data else {
            continue;
        };
        if &*name.local != "link" {
            continue;
        }
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
            MediaFit::Skip => {
                out.link_takes_host_slot.push(false);
                continue;
            }
            MediaFit::Unknown => {
                out.unknown_media += 1;
                out.link_takes_host_slot.push(false);
                continue;
            }
        }
        let href = css::attribute(nodes, handle, "href");
        if href.is_some_and(|h| {
            h.as_bytes()
                .get(..5)
                .is_some_and(|p| p.eq_ignore_ascii_case(b"data:"))
        }) {
            out.data_links += 1;
            out.link_takes_host_slot.push(false);
            continue;
        }
        out.link_takes_host_slot.push(true);
        out.links.push(href.and_then(|h| base.join(h).ok()));
    }
    out
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
/// Tight BASIC-lane `::before`/`::after` declaration allowlist: empty
/// generated boxes carrying a background and a resolved size (the
/// generated-decoration shape, e.g. a hamburger bar). `content` enrolls
/// only in its modelled forms (the value model has no string variant, so
/// text content stays skipped); position must be absolute; offsets are px
/// or auto. Deferred `var()` declarations never enroll. Anything else is
/// skipped by the caller (counted), never refused: existing parse
/// behavior is unchanged.
fn pseudo_basic_entry_admitted(property: Property, value: &Value, deferred: bool) -> bool {
    if deferred {
        return false;
    }
    match (property, value) {
        (
            Property::Content,
            Value::Content(
                crate::computed_style::Content::Empty | crate::computed_style::Content::Normal,
            ),
        ) => true,
        (Property::Width, Value::Width(crate::computed_style::Length::Px(_))) => true,
        (Property::Width, Value::Width(crate::computed_style::Length::Percent(percent))) => {
            percent & crate::computed_style::LENGTH_EM_FLAG == 0
        }
        (Property::Height, Value::Height(crate::computed_style::Length::Px(_))) => true,
        (
            Property::BackgroundColor,
            Value::BackgroundColor(_) | Value::BackgroundCurrentColor | Value::Inherit,
        ) => true,
        (Property::BackgroundClip | Property::BackgroundLayers, Value::Initial) => true,
        (
            Property::Position,
            Value::Position(crate::computed_style::Position::Absolute),
        ) => true,
        (
            Property::Top | Property::Right | Property::Bottom | Property::Left,
            Value::Offset(crate::computed_style::Margin::Px(_) | crate::computed_style::Margin::Auto),
        ) => true,
        _ => false,
    }
}

fn matched_rules(
    nodes: &[Node],
    sheets: &[Vec<u8>],
    diagnostics: &mut StyleDiagnostics,
    media: Option<crate::media_query::MediaContext>,
    budget: SupplyBudget,
    ext_selectors: bool,
    text_deco: bool,
    family_bind: bool,
    fam: &mut crate::font_family::FamilyTable,
    snapshot: Option<&crate::css::SnapshotState>,
) -> (Vec<MatchedRule>, Vec<Deferred>, bool, usize, usize) {
    let mut pseudo_skips: usize = 0;
    // Engine-observable focus gate: an `autofocus` attribute anywhere in the
    // DOM voids the caller's `FocusState::UnfocusedInitial` guarantee (the
    // browser focuses that element on load). Fail closed: enroll with the
    // focus guarantee REMOVED, so focus-family rules refuse exactly like the
    // no-guarantee path. History and input guarantees are unaffected.
    let gated_snapshot;
    let snapshot = match snapshot {
        Some(state) if state.focus.is_some() && crate::css::has_autofocus(nodes) => {
            gated_snapshot = crate::css::SnapshotState {
                focus: None,
                ..state.clone()
            };
            Some(&gated_snapshot)
        }
        other => other,
    };
    let mut rules = Vec::new();
    let mut deferred_table: Vec<Deferred> = Vec::new();
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
                return (rules, deferred_table, true, supplied, pseudo_skips);
            }
            match style_syntax::conditions_fit(&rule.conditions, media) {
                style_syntax::MediaFit::Screen => {}
                style_syntax::MediaFit::Skip | style_syntax::MediaFit::Unknown => continue,
            }
            let mut selectors: Box<dyn Iterator<Item = &str> + '_> = if ext_selectors {
                Box::new(css::ExtSelectorMembers::new(rule.prelude))
            } else {
                Box::new(rule.prelude.split(','))
            };
            // BASIC-LANE generated content: a basic sheet rule mentioning
            // `::` parses its block with the content grammar admitted, so
            // `content: ''` can enroll. The enrollment allowlist below keeps
            // this tight.
            let mentions_pseudo = !ext_selectors && rule.prelude.contains("::");
            let (properties, unknown) = block_entries(
                rule.declarations,
                &mut deferred_table,
                text_deco,
                family_bind,
                ext_selectors || mentions_pseudo,
                fam,
            );
            let decls_incomplete = !style_syntax::declarations_complete(rule.declarations);
            // A rule whose WHOLE selector group only parses through the
            // basic ::before/::after hook: declarations outside the
            // allowlist (or unparseable ones) are skipped and counted, never
            // refused. Existing refusal behavior everywhere else is
            // unchanged, including for mixed groups.
            let all_pseudo = mentions_pseudo
                && rule.prelude.split(',').all(|member| {
                    let member = member.trim();
                    !member.is_empty()
                        && css::selector(member).is_none()
                        && css::selector_basic_pseudo(member).is_some()
                });
            if all_pseudo && (unknown || decls_incomplete) {
                pseudo_skips += 1;
            }
            let refused = (unknown || decls_incomplete) && !all_pseudo;
            if refused {
                diagnostics.record(RefusalReason::CssDeclaration);
            }
            unsupported |= refused;
            for selector in selectors.by_ref().take(16) {
                let parsed_selector = if ext_selectors {
                    css::selector_ext(selector)
                } else {
                    css::selector(selector.trim())
                        .or_else(|| css::selector_basic_pseudo(selector.trim()))
                };
                let Some(selector) = parsed_selector else {
                    diagnostics.record(RefusalReason::CssSelector);
                    unsupported = true;
                    continue;
                };
                // State pseudo-classes: NO absent-state path. A rule carrying
                // one is enrolled only when the caller's explicit SnapshotState
                // covers EVERY state member, with history and input scoped
                // independently (`:link` needs FreshNoHistory; `:hover`/`:active`
                // need QuiescentCapture). Without the matching guarantee the rule
                // fails closed exactly like an unparseable selector; unknown
                // state is never treated as guaranteed and state styles are
                // neither applied nor silently dropped under uncertainty.
                if selector.has_state() && !selector.state_satisfied_by(snapshot) {
                    diagnostics.record(RefusalReason::CssSelector);
                    unsupported = true;
                    continue;
                }
                let pseudo_basic = !ext_selectors && selector.pseudo != crate::css::Pseudo::None;
                for entry in &properties {
                    let (property, value, important, deferred) = match *entry {
                        Entry::Typed(p, v, i) => (p, v, i, None),
                        Entry::Deferred(index, i) => (
                            Property::Display,
                            Value::Display(Display::Block),
                            i,
                            Some(index),
                        ),
                    };
                    // BASIC hook: only allowlisted generated-decoration
                    // declarations enroll; the rest are skipped (counted).
                    if pseudo_basic
                        && !pseudo_basic_entry_admitted(property, &value, deferred.is_some())
                    {
                        pseudo_skips += 1;
                        continue;
                    }
                    rules.push(MatchedRule {
                        selector: selector.clone(),
                        property,
                        value,
                        important,
                        source_order: u32::try_from(rules.len()).unwrap_or(u32::MAX),
                        deferred,
                    });
                    if rules.len() >= MAX_RULES {
                        diagnostics.record(RefusalReason::ExpandedRules);
                        return (rules, deferred_table, true, supplied, pseudo_skips);
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
    (rules, deferred_table, unsupported, supplied, pseudo_skips)
}

/// FL44: longhands that accept a folded px `calc()` (ext only).
fn is_px_calc_longhand(name: &str) -> bool {
    matches!(
        name,
        "width"
            | "height"
            | "min-width"
            | "max-width"
            | "min-height"
            | "max-height"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
    )
}

/// FL44: `calc(<n>px (+|-) <n>px ...)` with whole non-negative px terms and single-space delimiters, folded to `<result>px`.
/// `None` for anything else, including a negative or overflowing result.
fn fold_px_calc(value: &str) -> Option<String> {
    let inner = value.strip_prefix("calc(")?.strip_suffix(')')?;
    let mut total: i64 = 0;
    let mut sign = 1_i64;
    let mut expect_term = true;
    let mut terms = 0;
    for tok in inner.split(' ') {
        if expect_term {
            let digits = tok.strip_suffix("px")?;
            if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            total += sign * digits.parse::<i64>().ok()?;
            terms += 1;
        } else {
            sign = match tok {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
        }
        expect_term = !expect_term;
    }
    if expect_term || terms < 2 || total < 0 || total > 1_000_000 {
        return None;
    }
    Some(format!("{total}px"))
}

#[allow(clippy::too_many_lines)] // Explicit supported-property parsing rejects all unknown CSS.
pub(crate) fn properties(body: &str) -> (Vec<(Property, Value, bool)>, bool) {
    properties_with(
        body,
        false,
        false,
        false,
        &mut crate::font_family::FamilyTable::new(),
    )
}

/// `text_deco` is the opt-in for the EXPERIMENTAL `text-decoration` declaration. When false the
/// name is unrecognized exactly as in the reviewed r1 base.
/// `em_font` (CANDIDATE, only the extended-CSS opt-in entry passes true) accepts `<number>em` for
/// `font-size` as the exact parent-relative multiplier already used for percentages; every other
/// entry, including the default, keeps `em` unrecognized there.
pub(crate) fn properties_with(
    body: &str,
    text_deco: bool,
    family_bind: bool,
    em_font: bool,
    fam: &mut crate::font_family::FamilyTable,
) -> (Vec<(Property, Value, bool)>, bool) {
    let mut result = Vec::new();
    let mut unsupported = false;
    for (name, raw) in style_syntax::declarations(body) {
        let name = css::strip_comments(name);
        let cleaned = css::strip_comments(raw);
        let value = cleaned.trim().to_ascii_lowercase();
        let important = value.ends_with("!important");
        let value = value.strip_suffix("!important").unwrap_or(&value).trim();
        let name = name.trim().to_ascii_lowercase();
        // CANDIDATE (FL44, ext entry only): a WHOLE-value `calc()` over whole-px terms joined by `+`/`-` (space-delimited, no nesting,
        // no `*` `/`, no other unit, no var()) is folded to its constant `Npx` BEFORE the normal longhand parse, for width/height/
        // min-/max- size, margin and padding LONGHANDS only. A negative or overflowing result is NOT folded (calc clamps width/height
        // /padding to 0 and the longhand parsers refuse negative values, so it stays refused). Shorthands and every other property
        // keep refusing calc.
        let folded_calc;
        let value = if em_font && is_px_calc_longhand(&name) {
            if let Some(px) = fold_px_calc(value) {
                folded_calc = px;
                folded_calc.as_str()
            } else {
                value
            }
        } else {
            value
        };
        // CANDIDATE (extended-CSS opt-in entry only). `text-size-adjust: 100%` and `none` are
        // accepted as no-ops. Contract: this renderer performs NO text autosizing/inflation, so
        // there is nothing for the property to disable. CSS Size Adjust 1 (adjustment-control):
        // `none` disables the adjustment and `100%` multiplies the computed font size by 1; they
        // are not a universal equivalent of the property being absent (`auto`/mobile UAs may
        // inflate). This is an opt-in contract of the ext entry, with no runtime capability
        // check: REVISIT if autosizing/inflation is ever added. The engine computed-style
        // equality in tests is an engine property, not the standards computed value. Any other
        // value stays refused through the unknown-property path below.
        if em_font && name == "text-size-adjust" && matches!(value, "100%" | "none") {
            continue;
        }
        // CANDIDATE (extended-CSS opt-in entry only). `pointer-events: none` and `user-select: none`
        // are accepted as no-ops ONLY under this contract: the output is a static render with no
        // selection or focus state, observed as pixels. CSS UI 4 (#pointer-events-control): pointer-events
        // is inherited and controls hit-testing, not every interaction; user-select is not inherited
        // but the used value of `auto` depends on the parent. So this is NOT browser-computed-style or
        // interactive-correctness equivalence. This ext entry has no runtime capability guard or UI-state
        // retention: the limit is a comment-level contract, not fail-closed. Other repository code may
        // do its own hit-testing; no linkage to this entry was observed. Resolved values count: a defined
        // `var()` that resolves to `none` is admitted (substitution runs first); resolved auto/text refuse.
        // Every other value, `-webkit-`/`-moz-` alias and global keyword stays refused. REVISIT before
        // using this entry for anything interactive or any selection/focus painting.
        if em_font && matches!(name.as_str(), "pointer-events" | "user-select") && value == "none" {
            continue;
        }
        // CANDIDATE (extended-CSS opt-in entry only, FL20). INITIAL-VALUE declarations accepted as no-ops: the exact
        // (property, value) pairs in INITIAL_NOOPS, each measured in Chrome 156 as computed-style-identical to the
        // undeclared div (private-evidence ul1/ini19.json, tested on a plain div only). Every NON-initial value of these
        // properties stays refused, so an initial value cannot override an inherited non-initial one (none is admitted).
        // Nothing here models any of these properties. Excluded on purpose: pointer-events/user-select (own earlier none-only contract), flex-flow (earlier tests pin it refused), font-* and text-rendering (font lane held),
        // unicode-bidi:normal and outline:0 (Chrome computed style differs from the undeclared div), tab-size, color-scheme,
        // appearance, font-synthesis. A defined var() resolving to one of these is admitted through deferred substitution.
        if em_font
            && INITIAL_NOOPS
                .iter()
                .any(|(n, v)| *n == name.as_str() && *v == value)
        {
            continue;
        }
        // CANDIDATE (extended-CSS opt-in entry only). `overflow-wrap: break-word|normal` (and the `word-wrap` alias):
        // a break inside a word happens only when that word is wider than an empty line, and the line placer already
        // refuses a word that cannot fit, so for every page the engine admits the used layout is identical to normal.
        // `break-word` does not change min-content (unlike `anywhere`), so shrink-to-fit also agrees. `anywhere`,
        // global keywords, `word-break`, `hyphens` and everything else stay refused. The word-fit refusal lives in the
        // placer, not here: REVISIT if the placer ever learns to break inside words.
        if em_font
            && matches!(name.as_str(), "overflow-wrap" | "word-wrap")
            && matches!(value, "break-word" | "normal")
        {
            continue;
        }
        // CANDIDATE (extended-CSS opt-in entry only). `flex-direction: row` and `flex-wrap: nowrap` are the INITIAL
        // values, so declaring them changes nothing in the used layout: the flex slice already implements exactly a
        // single-line LTR row and refuses everything else it cannot prove. Not set-membership of other values:
        // `column*`, `row-reverse`, `wrap*`, `flex-flow`, global keywords stay refused. On a non-flex box the
        // properties have no effect in Chrome either. Not browser-computed-style equivalence.
        if em_font
            && ((name == "flex-direction" && value == "row")
                || (name == "flex-wrap" && value == "nowrap"))
        {
            continue;
        }
        // CANDIDATE (extended-CSS opt-in entry only). Zero-paint, zero-layout declarations of a static render:
        // `cursor` (a single standard keyword; the output is pixels with no pointer), `border-image: none` and
        // `border-image: none` (a shorthand reset: `none` sets border-image-source to none, so the ordinary border
        // still draws). `box-shadow: none` is NOT skipped any more (FL26): it is the typed BoxShadow(None) reset below.
        // Not browser-computed-style equivalence. Anything else (`cursor: url(..)`, lists, global keywords,
        // any other border-image/box-shadow value) stays refused through the unknown-property path.
        if em_font
            && ((name == "cursor"
                && matches!(
                    value,
                    "auto"
                        | "default"
                        | "pointer"
                        | "text"
                        | "not-allowed"
                        | "move"
                        | "grab"
                        | "grabbing"
                        | "help"
                        | "wait"
                        | "progress"
                        | "crosshair"
                        | "none"
                        | "context-menu"
                        | "cell"
                        | "vertical-text"
                        | "alias"
                        | "copy"
                        | "no-drop"
                        | "all-scroll"
                        | "zoom-in"
                        | "zoom-out"
                        | "col-resize"
                        | "row-resize"
                        | "n-resize"
                        | "e-resize"
                        | "s-resize"
                        | "w-resize"
                        | "ew-resize"
                        | "ns-resize"
                ))
                || (name == "border-image" && value == "none"))
        {
            continue;
        }
        // CANDIDATE (extended-CSS opt-in entry only), `min-height` (parsed in the property match below, FL17 corrected).
        // `0` / `0px` / `auto` emit an explicit MinHeight(Auto) RESET so cascade order, specificity, !important, inline style
        // and a resolved var() all apply (FL16 skipped them and left an earlier positive value in force: fixed). Computed
        // identity differs (`auto` stays `auto`); only the USED minimum 0 is claimed, in the bounded slice (block, abspos,
        // flex CROSS axis of a single-line horizontal row; no column/wrap/grid/replaced/aspect-ratio). `min-height` is
        // inapplicable to a non-replaced inline box. A POSITIVE px length (any `parse_width` px lexeme, including decimals
        // such as `1.0px`/`1.00px`) is parsed into MinHeight and the box tree admits or refuses the box; percentages, calc,
        // `min-content`/`fit-content`, global keywords, em and negatives refuse. A `var()` is substituted BEFORE this
        // parser, so a defined `--m:0`/`auto`/`50px` or `var(--missing,0)` is handled as the resolved value; an undefined
        // var without fallback refuses the page. Tested by engine layout equality, not browser-computed-style equivalence.
        // REVISIT before column flex, grid, replaced items, aspect-ratio or any vertical writing mode is admitted.
        // CANDIDATE (extended-CSS opt-in entry only). `transition` and its longhands (property, duration, delay,
        // timing-function) are accepted as no-ops ONLY under this contract: the renderer consumes the FIRST computed style
        // of the assembled static input and never re-styles (no later or earlier restyle, no ancestor/active-state
        // effects, no `@starting-style`, no animation), and CSS Transitions 1 starts a transition only on a computed-value
        // change, so none runs. Not browser-computed-style equivalence, and the contract is a code comment, not an
        // enforced runtime provenance check. `@starting-style` and animation stay refused. The grammar is a TOKEN FILTER,
        // not CSS validation: a comma list of at most 4 items, each at most 4 tokens of {lowercase ident, time like
        // `.2s`/`200ms`, plain timing keyword} or `none`. It is NOT property-specific, so invalid combinations (e.g.
        // `transition-duration:color`, `transition-timing-function:potato`, `transition:color color color color`) are also
        // accepted as inert under the contract; do not read this as "all invalid values refuse". `cubic-bezier(`,
        // `steps(`, `linear(`, global keywords, `allow-discrete`, `normal`, empty, bad units and other shapes refuse.
        // REVISIT if the renderer ever re-styles.
        if em_font
            && matches!(
                name.as_str(),
                "transition"
                    | "transition-property"
                    | "transition-duration"
                    | "transition-delay"
                    | "transition-timing-function"
            )
        {
            let item_ok = |item: &str| {
                let toks: Vec<&str> = item.split_ascii_whitespace().collect();
                !toks.is_empty()
                    && toks.len() <= 4
                    && toks.iter().all(|t| {
                        let time = t
                            .strip_suffix("ms")
                            .or_else(|| t.strip_suffix('s'))
                            .is_some_and(|n| {
                                !n.is_empty()
                                    && n.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                                    && n.bytes().filter(|&b| b == b'.').count() <= 1
                                    && n.bytes().any(|b| b.is_ascii_digit())
                            });
                        let timing = matches!(
                            *t,
                            "ease"
                                | "linear"
                                | "ease-in"
                                | "ease-out"
                                | "ease-in-out"
                                | "step-start"
                                | "step-end"
                        );
                        let ident = !t.is_empty()
                            && t.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
                            && !t.starts_with('-')
                            && !matches!(
                                *t,
                                "inherit"
                                    | "initial"
                                    | "unset"
                                    | "revert"
                                    | "revert-layer"
                                    | "allow-discrete"
                                    | "normal"
                            );
                        time || timing || ident
                    })
            };
            let items: Vec<&str> = value.split(',').collect();
            if value == "none" || (items.len() <= 4 && items.iter().all(|i| item_ok(i.trim()))) {
                continue;
            }
        }
        // CANDIDATE (extended-CSS opt-in entry only). `border-radius: 0` / `0px` is the initial
        // value (CSS Backgrounds 3, border-radius initial: 0), i.e. square corners, which is all
        // this renderer paints. Accepted as a no-op. Any non-zero, multi-value, `/` or
        // global-keyword value stays refused. Contract: engine paints no rounded corners and no
        // overflow clipping depends on radius; REVISIT if either is added.
        if em_font && name == "border-radius" && matches!(value, "0" | "0px") {
            continue;
        }
        // CANDIDATE (extended-CSS opt-in entry only). `inset` shorthand (CSS Position 3): 1-4 values of `auto` or
        // integer px expand to top/right/bottom/left with the margin-style rule (1: all; 2: top+bottom / left+right;
        // 3: top / left+right / bottom; 4: top right bottom left). Same value domain as the longhands; keywords,
        // percentages, calc, logical `inset-*` and anything else refuse. It sets offsets only; whether a box may
        // use them stays with the positioned-box validation.
        // CANDIDATE (ext only, FL28): `outline` shorthand = `[<width>] [none|solid] [<colour>]` in any order, each at most once;
        // width whole px (or 0), colour explicit opaque or `currentcolor`; EVERY longhand not named resets to its initial
        // (width 3px=medium, style none, colour currentcolor); outline-offset is not part of the shorthand and is untouched.
        // `thin|medium|thick`, dashed/dotted/double/groove/ridge/inset/outset/hidden/auto, `invert` and lists refuse.
        if em_font && name == "outline" {
            if let Some(keyword) = parse_keyword(value) {
                for property in [
                    Property::OutlineSolid,
                    Property::OutlineWidth,
                    Property::OutlineColor,
                ] {
                    result.push((property, keyword, important));
                }
                continue;
            }
            let (mut solid, mut width, mut colour, mut seen) = (None, None, None, [false; 3]);
            let mut ok = !value.is_empty();
            for token in value.split_ascii_whitespace() {
                if let Some(v) = parse_outline_style(token) {
                    ok &= !std::mem::replace(&mut seen[0], true);
                    solid = Some(v);
                } else if let Some(v) = parse_outline_width(token) {
                    ok &= !std::mem::replace(&mut seen[1], true);
                    width = Some(v);
                } else if let Some(v) = parse_outline_colour(token) {
                    ok &= !std::mem::replace(&mut seen[2], true);
                    colour = Some(v);
                } else {
                    ok = false;
                }
            }
            if !ok {
                unsupported = true;
                continue;
            }
            result.push((
                Property::OutlineSolid,
                Value::OutlineSolid(solid.unwrap_or(false)),
                important,
            ));
            result.push((
                Property::OutlineWidth,
                Value::OutlineWidth(width.unwrap_or(3)),
                important,
            ));
            result.push((
                Property::OutlineColor,
                Value::OutlineColor(colour.unwrap_or(None)),
                important,
            ));
            continue;
        }
        if em_font && name == "inset" {
            let sides: Vec<_> = value.split_ascii_whitespace().collect();
            let one =
                |v: &str| parse_margin(v).filter(|m| matches!(m, Margin::Auto | Margin::Px(_)));
            let parsed = match sides.as_slice() {
                [a] => one(a).map(|a| [a; 4]),
                [a, b] => one(a).zip(one(b)).map(|(a, b)| [a, b, a, b]),
                [a, b, c] => one(a)
                    .zip(one(b))
                    .zip(one(c))
                    .map(|((a, b), c)| [a, b, c, b]),
                [a, b, c, d] => one(a)
                    .zip(one(b))
                    .zip(one(c))
                    .zip(one(d))
                    .map(|(((a, b), c), d)| [a, b, c, d]),
                _ => None,
            };
            let Some(offsets) = parsed else {
                unsupported = true;
                continue;
            };
            for (property, offset) in [
                Property::Top,
                Property::Right,
                Property::Bottom,
                Property::Left,
            ]
            .into_iter()
            .zip(offsets)
            {
                result.push((property, Value::Offset(offset), important));
            }
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
        match border_declarations(&name, value, em_font) {
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
        // Every accepted background has no image. An explicit `none` makes
        // that invariant clear without manufacturing a color declaration or
        // changing its cascade order. Other image values remain unsupported.
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
                for property in [
                    Property::FlexGrow,
                    Property::FlexShrink,
                    Property::FlexBasis,
                ] {
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
        // empty ancestors. Never classify these as harmless bare phrases. CROSS-REF (FL21): under the ext entry ONLY, the
        // exact INITIAL values `visibility:visible`, `letter-spacing:normal`, `word-spacing:normal` and (FL24) `text-transform:none` are skipped earlier by
        // INITIAL_NOOPS and never reach this gate; every other value of these properties, and every value on the default
        // entry, still lands here and refuses.
        if matches!(
            name.as_str(),
            "text-transform" | "letter-spacing" | "word-spacing"
        ) || (name == "visibility" && !em_font)
        {
            unsupported = true;
            continue;
        }
        let parsed = match name.as_str() {
            "position" => parse_keyword(value)
                .or(match value {
                    "static" => Some(Value::Position(Position::Static)),
                    "absolute" => Some(Value::Position(Position::Absolute)),
                    // Ext entry only; BoxTree validation admits it only in the no-offset domain without the modelled overlap sources (not a proof of no overlap).
                    "relative" if em_font => Some(Value::Position(Position::Relative)),
                    "fixed" if em_font => Some(Value::Position(Position::Fixed)),
                    "sticky" if em_font => Some(Value::Position(Position::Sticky)),
                    _ => None,
                })
                .map(|v| (Property::Position, v)),
            "float" => parse_keyword(value)
                .or(match value {
                    "left" => Some(Value::Float(Float::Left)),
                    "right" => Some(Value::Float(Float::Right)),
                    "none" => Some(Value::Float(Float::None)),
                    _ => None,
                })
                .map(|v| (Property::Float, v)),
            "clear" => parse_keyword(value)
                .or(match value {
                    "both" => Some(Value::Clear(Clear::Both)),
                    "none" => Some(Value::Clear(Clear::None)),
                    _ => None,
                })
                .map(|v| (Property::Clear, v)),
            // EXTENDED-CSS opt-in only. `""` and normal/none; any other `content` value
            // (text, attr(), counters, urls) stays an unknown declaration and refuses.
            "content" if em_font => parse_keyword(value)
                .or(match value {
                    "\"\"" | "''" => Some(Value::Content(crate::computed_style::Content::Empty)),
                    "normal" | "none" => {
                        Some(Value::Content(crate::computed_style::Content::Normal))
                    }
                    _ => None,
                })
                .map(|v| (Property::Content, v)),
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
                    // Table roles are retained in the model, but author display
                    // activation waits for anonymous-table fixup and grid paint.
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
                    } else if let Some(high_precision) = em_font
                        .then(|| unitless_number_3_to_5_decimals(value))
                        .flatten()
                    {
                        // CANDIDATE (ext entry only): 3-5 decimal unitless multiplier, floored to 1/64 px like Chrome's
                        // LayoutUnit (see Computed::used_line_height). Default entry keeps refusing these.
                        Some(Value::LineHeightPrecise(high_precision))
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
            "font-family" if family_bind && pinned_family_list(value) => {
                // CANDIDATE, opt-in, unadopted: exactly the pinned lists of
                // `pinned_family_list` are retained as the explicit generic
                // SansSerif without the whole-render refusal. Painting still
                // requires the family-bound painter with proven face hashes.
                match fam.intern(value) {
                    Some(key) => Some((
                        Property::FontFamily,
                        Value::FontFamily(
                            Some(crate::computed_style::GenericFamily::SansSerif),
                            Some(key),
                        ),
                    )),
                    None => {
                        unsupported = true; // handle capacity exhausted: refuse, never wrap
                        None
                    }
                }
            }
            "font-family" => {
                // Retention is not provider binding. The existing whole-render
                // refusal stays for every value except the one narrow ext-entry
                // admission below.
                match parse_keyword(value) {
                    Some(v) => {
                        unsupported = true;
                        Some((Property::FontFamily, v))
                    }
                    None => {
                        let mut return_family = None;
                        let generic = match value {
                            "serif" => Some(crate::computed_style::GenericFamily::Serif),
                            "sans-serif" => Some(crate::computed_style::GenericFamily::SansSerif),
                            "monospace" => Some(crate::computed_style::GenericFamily::Monospace),
                            _ => None,
                        };
                        // Interned retention of the (lowercased, trimmed) declared list. Only values that
                        // already produced a FontFamily value before keep producing one (no new acceptance).
                        let produced = generic.map(|g| {
                            (
                                Property::FontFamily,
                                Value::FontFamily(Some(g), fam.intern(value)),
                            )
                        });
                        // CANDIDATE (ext entries only: em_font == ext_selectors at
                        // both block_entries call sites): exactly the generic
                        // `sans-serif` keyword with successful internment drops the
                        // refusal. Intern failure keeps BOTH the old produced value
                        // (key None) AND the refusal - that corner is unchanged.
                        // Named families, multi-item lists, global keywords, serif
                        // and monospace keep the existing refusal. Painting is
                        // untouched: retained families still paint only through the
                        // family-bound painter with proven face hashes.
                        unsupported = !matches!(
                            (em_font, generic, &produced),
                            (
                                true,
                                Some(crate::computed_style::GenericFamily::SansSerif),
                                Some((_, Value::FontFamily(_, Some(_))))
                            )
                        );
                        // CANDIDATE (ext entries only): a NAMED list whose first honorable
                        // entry is sans-serif or monospace per `font_family::device_generic`
                        // (named entries before it must be known-absent on the basic painter;
                        // see that function for every refusal). Intern failure keeps the
                        // refusal. The painter maps the generic to its own sans/mono face.
                        if em_font && generic.is_none() {
                            if let Ok(g) = crate::font_family::device_generic(value) {
                                if let Some(key) = fam.intern(value) {
                                    unsupported = false;
                                    let g = match g {
                                        crate::font_family::Generic::Monospace => {
                                            crate::computed_style::GenericFamily::Monospace
                                        }
                                        _ => crate::computed_style::GenericFamily::SansSerif,
                                    };
                                    return_family = Some((
                                        Property::FontFamily,
                                        Value::FontFamily(Some(g), Some(key)),
                                    ));
                                }
                            }
                        }
                        return_family.or(produced)
                    }
                }
            }
            "font-size" => parse_keyword(value)
                .or_else(|| match parse_width(value) {
                    Some(Length::Px(px)) if px > 0 => Some(Value::FontSize(px)),
                    Some(Length::Percent(percent)) if percent > 0 => {
                        Some(Value::FontSizePercent(percent))
                    }
                    _ => parse_rem(value).map(Value::FontSizeRem).or_else(|| {
                        // rem is tried first. em is a parent-relative multiplier, the same
                        // fixed point as a percentage; a fractional pixel still computes to 0.
                        if em_font {
                            parse_em(value).map(Value::FontSizePercent)
                        } else {
                            None
                        }
                    }),
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
                .or_else(|| {
                    if em_font {
                        parse_rem(value).map(Value::WidthRem)
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    if em_font {
                        parse_em_length(value).map(Value::Width)
                    } else {
                        None
                    }
                })
                .map(|value| (Property::Width, value)),
            // CANDIDATE (ext only): a POSITIVE px `min-height` (0/auto emit an explicit reset below). Admission of the
            // resulting box is decided by the box tree (block, auto height, in-flow, not a flex item, no escaping margin).
            // CANDIDATE FL40 (ext only): `max-height` as a px length (zero included) or `none`; every other form refuses. The box
            // tree folds it into a px `height` only; any other use of a non-none max-height refuses there.
            "max-height" if em_font => parse_keyword(value)
                .or_else(|| match value {
                    "none" => Some(Value::MaxHeight(Length::Auto)),
                    _ => match parse_width(value) {
                        Some(l @ Length::Px(_)) => Some(Value::MaxHeight(l)),
                        _ => None,
                    },
                })
                .map(|v| (Property::MaxHeight, v)),
            "min-height" if em_font => match parse_width(value) {
                Some(l @ Length::Px(n)) if n > 0 => {
                    Some((Property::MinHeight, Value::MinHeight(l)))
                }
                Some(Length::Auto) | Some(Length::Px(0)) => {
                    Some((Property::MinHeight, Value::MinHeight(Length::Auto)))
                }
                _ => parse_rem(value).map(|tt| (Property::MinHeight, Value::MinHeightRem(tt))),
            },
            // CANDIDATE (ext only, FL18): `column-gap`/`gap` as ONE non-negative px value or `normal` (=0). Only a single-line
            // LTR row flex container reads it (flex_row_items); `gap` is a single-value shorthand here, so row-gap is not
            // modelled (two-value gap, row-gap, percentages, calc, em refuse). Ignored on non-flex boxes (as in CSS).
            "column-gap" | "gap" if em_font => parse_keyword(value)
                .or_else(|| match value {
                    "normal" => Some(Value::ColumnGap(Length::Auto)),
                    _ => match parse_width(value) {
                        Some(l @ Length::Px(_)) => Some(Value::ColumnGap(l)),
                        _ => None,
                    },
                })
                .map(|value| (Property::ColumnGap, value)),
            // CANDIDATE (ext only, FL22): the SINGLE-value `overflow` shorthand, `visible` or `hidden`, as an explicit typed
            // value (visible RESETS an earlier hidden through the normal cascade). `overflow-x`/`overflow-y`, two-value forms
            // and every other keyword refuse (FL19's overflow-x/-y:visible skip is removed: it could combine with a hidden
            // shorthand into a non-uniform pair). The box tree admits `hidden` only for a text-free, float-free block whose
            // descendants provably stay inside its padding box; see BoxTree::stray_overflow_hidden.
            "overflow" if em_font => parse_keyword(value)
                .or(match value {
                    "visible" => Some(Value::OverflowHidden(false)),
                    "hidden" | "auto" => Some(Value::OverflowHidden(true)),
                    _ => None,
                })
                .map(|value| (Property::OverflowHidden, value)),
            // CANDIDATE (ext only, FL33): the `overflow-x` / `overflow-y` longhands, ONLY `hidden` or `auto`. Either axis non-visible
            // makes the other axis compute to `auto`, so both axes are non-visible: exactly the state `overflow:hidden` already
            // models (BoxTree::stray_overflow_hidden admits it only when nothing can overflow, so clip, scroll and scrollbars are
            // unobservable). `visible` on a longhand refuses (it could combine with a non-visible longhand or shorthand into an
            // unmodelled pair, FL19); `scroll`, `clip`, `overlay` and everything else refuse. A later `overflow:visible` shorthand
            // resets both axes through the normal cascade.
            "overflow-x" | "overflow-y" if em_font => parse_keyword(value)
                .or(match value {
                    "hidden" | "auto" => Some(Value::OverflowHidden(true)),
                    _ => None,
                })
                .map(|value| (Property::OverflowHidden, value)),
            // CANDIDATE (ext only, FL26): ONE hard outer `box-shadow`: `<dx> <dy> [0 [<spread>]] <colour>` (colour first or last,
            // explicit and opaque; lengths in whole px or 0; blur must be 0; spread >= 0). `none` is a typed RESET so the cascade
            // (order, specificity, !important, inherit, var) applies. Inset, lists, blur, negative spread, omitted/alpha/
            // currentcolor colours, em/percent/fractional lengths refuse. The painter (decorated rectangles only) draws the part
            // of the shadow outside the border box; every other painter refuses a box carrying a shadow.
            "box-shadow" if em_font => parse_keyword(value)
                .or_else(|| parse_hard_shadow(value).map(Value::BoxShadow))
                .map(|value| (Property::BoxShadow, value)),
            // CANDIDATE (ext only, FL28): outline longhands, see the `outline` shorthand expansion above.
            "outline-style" if em_font => parse_keyword(value)
                .or_else(|| parse_outline_style(value).map(Value::OutlineSolid))
                .map(|value| (Property::OutlineSolid, value)),
            "outline-width" if em_font => parse_keyword(value)
                .or_else(|| parse_outline_width(value).map(Value::OutlineWidth))
                .map(|value| (Property::OutlineWidth, value)),
            "outline-color" if em_font => parse_keyword(value)
                .or_else(|| parse_outline_colour(value).map(Value::OutlineColor))
                .map(|value| (Property::OutlineColor, value)),
            "outline-offset" if em_font => parse_keyword(value)
                .or_else(|| match parse_width(value) {
                    Some(Length::Px(n)) if n <= 10_000 => Some(Value::OutlineOffset(n)),
                    _ => None,
                })
                .map(|value| (Property::OutlineOffset, value)),
            "min-width" => parse_keyword(value)
                .or_else(|| match parse_width(value) {
                    Some(l @ (Length::Auto | Length::Px(_))) => Some(Value::MinWidth(l)),
                    _ => None,
                })
                .or_else(|| {
                    if em_font {
                        parse_rem(value).map(Value::MinWidthRem)
                    } else {
                        None
                    }
                })
                .map(|value| (Property::MinWidth, value)),
            "max-width" => parse_keyword(value)
                .or_else(|| {
                    match if value == "none" {
                        Some(Length::Auto)
                    } else {
                        parse_width(value)
                    } {
                        Some(l @ (Length::Auto | Length::Px(_))) => Some(Value::MaxWidth(l)),
                        _ => None,
                    }
                })
                .or_else(|| {
                    if em_font {
                        parse_rem(value).map(Value::MaxWidthRem)
                    } else {
                        None
                    }
                })
                .map(|value| (Property::MaxWidth, value)),
            "height" => parse_keyword(value)
                .or_else(|| parse_width(value).map(Value::Height))
                .or_else(|| {
                    if em_font {
                        parse_rem(value).map(Value::HeightRem)
                    } else {
                        None
                    }
                })
                .or_else(|| {
                    if em_font {
                        parse_em_length(value).map(Value::Height)
                    } else {
                        None
                    }
                })
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
                .or_else(|| parse_padding(value).map(Value::Padding))
                .map(|value| (Property::PaddingTop, value)),
            "padding-bottom" => parse_keyword(value)
                .or_else(|| parse_padding(value).map(Value::Padding))
                .map(|value| (Property::PaddingBottom, value)),
            "padding-left" => parse_keyword(value)
                .or_else(|| parse_padding(value).map(Value::Padding))
                .map(|value| (Property::PaddingLeft, value)),
            "padding-right" => parse_keyword(value)
                .or_else(|| parse_padding(value).map(Value::Padding))
                .map(|value| (Property::PaddingRight, value)),
            // EXPERIMENTAL: exactly `none` or `underline` (no color/style/thickness/line forms, no
            // longhands). The paint path decides separately whether the page may draw it.
            // CANDIDATE (ext entry only): exactly `none` (turns off the UA underline of a[href]/u/ins; longhands,
            // colour/style/thickness forms and every other value stay refused).
            "text-decoration" if em_font && !text_deco && value == "none" => Some((
                Property::TextDecoration,
                Value::TextDecoration(crate::computed_style::TextDecoration::None),
            )),
            "text-decoration" if text_deco => parse_keyword(value)
                .or(match value {
                    "none" => Some(Value::TextDecoration(
                        crate::computed_style::TextDecoration::None,
                    )),
                    "underline" => Some(Value::TextDecoration(
                        crate::computed_style::TextDecoration::Underline,
                    )),
                    _ => None,
                })
                .map(|v| (Property::TextDecoration, v)),
            "text-align" => parse_keyword(value)
                .or(match value {
                    "left" => Some(Value::TextAlign(TextAlign::Left)),
                    "right" => Some(Value::TextAlign(TextAlign::Right)),
                    "center" => Some(Value::TextAlign(TextAlign::Center)),
                    _ => None,
                })
                .map(|v| (Property::TextAlign, v)),
            // CANDIDATE (ext entry only): only the keyword `none` (no marker) is modelled; every other
            // list-style-type stays refused; the `list-style` shorthand is refused except exactly `none` (next arm).
            // CANDIDATE (ext entry only): the `list-style` shorthand with exactly `none` resets type and image to none
            // (position resets to outside, which only matters when a marker exists), so it equals `list-style-type: none`
            // here. Any other shorthand value (`none inside`, `disc`, url, global keywords) stays refused.
            "list-style" if em_font && value == "none" => {
                Some((Property::ListStyleType, Value::ListStyleNone(true)))
            }
            // CANDIDATE (ext entry only): `white-space: nowrap|normal`. Every other value (pre, pre-wrap, pre-line,
            // break-spaces, wrap/nowrap shorthands) refuses. `nowrap` is only paint-equivalent where the text has no
            // break opportunity; BoxTree::from_style refuses the rest.
            "white-space" if em_font => parse_keyword(value)
                .or(match value {
                    "pre" => Some(Value::WhiteSpacePre),
                    "nowrap" => Some(Value::WhiteSpaceNowrap(true)),
                    "normal" => Some(Value::WhiteSpaceNowrap(false)),
                    _ => None,
                })
                .map(|v| (Property::WhiteSpace, v)),
            // CANDIDATE FL38 (ext only): `visibility: hidden|visible` (collapse and everything else refuse).
            "visibility" if em_font => parse_keyword(value)
                .or(match value {
                    "hidden" => Some(Value::Visibility(true)),
                    "visible" => Some(Value::Visibility(false)),
                    _ => None,
                })
                .map(|v| (Property::Visibility, v)),
            "list-style-type" if em_font => parse_keyword(value)
                .or(match value {
                    "none" => Some(Value::ListStyleNone(true)),
                    _ => None,
                })
                .map(|v| (Property::ListStyleType, v)),
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
            "unicode-bidi" => parse_keyword(value).or(match value {
                "normal" => Some(Value::UnicodeBidi(crate::computed_style::UnicodeBidi::Normal)),
                "embed" => Some(Value::UnicodeBidi(crate::computed_style::UnicodeBidi::Embed)),
                "bidi-override" => Some(Value::UnicodeBidi(crate::computed_style::UnicodeBidi::Override)),
                "isolate" => Some(Value::UnicodeBidi(crate::computed_style::UnicodeBidi::Isolate)),
                "isolate-override" => Some(Value::UnicodeBidi(crate::computed_style::UnicodeBidi::IsolateOverride)),
                "plaintext" => Some(Value::UnicodeBidi(crate::computed_style::UnicodeBidi::Plaintext)),
                _ => None,
            }).map(|v| (Property::UnicodeBidi,v)),
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
    parse_ten_thousandths(value.strip_suffix("rem")?)
}

/// `<number>em` (not `rem`), same grammar as [`parse_rem`]. The result is the multiplier in
/// ten-thousandths, which is exactly the unit of `Value::FontSizePercent` (150% is 15000).
fn parse_em(value: &str) -> Option<u32> {
    if value.ends_with("rem") {
        return None;
    }
    parse_ten_thousandths(value.strip_suffix("em")?)
}

/// `<n>em` width/height carrier (ext only): `Length::Percent(LENGTH_EM_FLAG | ten-thousandths)`.
fn parse_em_length(value: &str) -> Option<Length> {
    let tt = parse_em(value)?;
    (tt < crate::computed_style::LENGTH_EM_FLAG)
        .then_some(Length::Percent(crate::computed_style::LENGTH_EM_FLAG | tt))
}

fn parse_ten_thousandths(number: &str) -> Option<u32> {
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
    let mut v = if whole.is_empty() {
        0
    } else {
        whole.parse::<u32>().ok()?.checked_mul(10_000)?
    };
    if !fractional.is_empty() {
        let scale = 10u32.pow(4 - u32::try_from(fractional.len()).ok()?);
        v = v.checked_add(fractional.parse::<u32>().ok()?.checked_mul(scale)?)?;
    }
    (v > 0).then_some(v)
}

/// The only family lists the opt-in family binding admits. Tokens are split on commas, CSS
/// whitespace is trimmed around each token only, and each token must equal the pinned text
/// exactly (double quotes only, no whitespace or other normalization inside the quoted name).
/// `properties_with` receives the value already ASCII-lowercased by the declaration layer
/// (line ~1042); CSS family names and the generic keyword are ASCII case-insensitive, so the
/// pinned text is compared in lowercase.
/// Chrome 156 on the measured host resolves both lists to Liberation Sans; that is a recorded
/// measurement of one host, not a portable rule.
fn pinned_family_list(value: &str) -> bool {
    const WS: &[char] = &[' ', '\t', '\n', '\r', '\x0c'];
    let tokens: Vec<&str> = value.split(',').map(|t| t.trim_matches(WS)).collect();
    matches!(
        tokens.as_slice(),
        ["sans-serif"] | ["\"gds transport\"", "arial", "sans-serif"]
    )
}

/// `<whole>.<3..5 decimals>` with whole in 0..=99, value > 0, as number * 100_000 (own `Value::LineHeightPrecise` domain).
fn unitless_number_3_to_5_decimals(value: &str) -> Option<u32> {
    let (whole, frac) = value.split_once('.')?;
    if whole.is_empty()
        || whole.len() > 2
        || !(3..=5).contains(&frac.len())
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || !frac.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let n = whole.parse::<u32>().ok()? * 100_000
        + frac.parse::<u32>().ok()? * 10u32.pow(5 - frac.len() as u32);
    (n > 0).then_some(n)
}

fn parse_width(value: &str) -> Option<Length> {
    if value == "auto" {
        return Some(Length::Auto);
    }
    // `pt` is an absolute unit (1pt = 4/3px, CSS 2.1 §4.3.2): it converts
    // into the same fixed-point hundredths domain as `px` and follows the
    // same whole-pixel used-value contract (a pt length that resolves to a
    // fractional pixel refuses, exactly like a fractional px).
    let (number, percent, pt) = if let Some(n) = value.strip_suffix("px") {
        (n, false, false)
    } else if let Some(n) = value.strip_suffix('%') {
        (n, true, false)
    } else if let Some(n) = value.strip_suffix("pt") {
        (n, false, true)
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
        // Bit 31 of a percent payload is the em carrier flag; an absurd percentage that would collide refuses.
        (value < crate::computed_style::LENGTH_EM_FLAG).then_some(Length::Percent(value))
    } else {
        // pt -> px hundredths: *4/3 rounded to the nearest hundredth. Exact
        // for whole-pixel results (9pt -> 1200); a fractional-pixel result
        // rounds to a non-whole hundredth and refuses below, the same
        // contract fractional px follows.
        if pt {
            value = value.checked_mul(4)?.checked_add(1)? / 3;
        }
        if value % 100 == 0 {
            Some(Length::Px(value / 100))
        } else {
            // Fractional px needs fixed-point support in used values.
            None
        }
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
/// FL26: parse `none` or one hard shadow; see the `box-shadow` arm. Returns `Some(None)` for `none`.
fn parse_hard_shadow(value: &str) -> Option<Option<Shadow>> {
    if value == "none" {
        return Some(None);
    }
    let mut tokens: Vec<&str> = Vec::new();
    let (mut depth, mut start) = (0usize, None);
    for (i, ch) in value.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => return None,
            c if c.is_ascii_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    tokens.push(&value[st..i]);
                }
                continue;
            }
            _ => {}
        }
        if start.is_none() {
            start = Some(i);
        }
    }
    if depth != 0 {
        return None;
    }
    if let Some(st) = start {
        tokens.push(&value[st..]);
    }
    if !(3..=5).contains(&tokens.len()) {
        return None;
    }
    let colour_at_start =
        !tokens[0].bytes().next()?.is_ascii_digit() && !tokens[0].starts_with('-');
    let (colour, lengths) = if colour_at_start {
        (tokens[0], &tokens[1..])
    } else {
        (tokens[tokens.len() - 1], &tokens[..tokens.len() - 1])
    };
    let rgb = opaque_color(colour)?;
    let px = |t: &str, signed: bool| -> Option<i32> {
        let (neg, body) = match t.strip_prefix('-') {
            Some(b) if signed => (true, b),
            Some(_) => return None,
            None => (false, t),
        };
        let n = match parse_width(body)? {
            Length::Px(n) if n <= 10_000 => i32::try_from(n).ok()?,
            _ => return None,
        };
        Some(if neg { -n } else { n })
    };
    if !(2..=4).contains(&lengths.len()) {
        return None;
    }
    let dx = px(lengths[0], true)?;
    let dy = px(lengths[1], true)?;
    if lengths.len() >= 3 && px(lengths[2], false)? != 0 {
        return None;
    }
    let spread = if lengths.len() == 4 {
        px(lengths[3], false)?
    } else {
        0
    };
    Some(Some(Shadow {
        dx,
        dy,
        spread: u32::try_from(spread).ok()?,
        rgb,
    }))
}

/// FL28: `none` -> no outline, `solid` -> solid; everything else (dashed, dotted, double, groove, ridge, inset, outset, hidden, auto) refuses.
fn parse_outline_style(value: &str) -> Option<bool> {
    match value {
        "none" => Some(false),
        "solid" => Some(true),
        _ => None,
    }
}

/// FL28: whole px or 0 (no thin/medium/thick, no fractions, no em/%).
fn parse_outline_width(value: &str) -> Option<u32> {
    match parse_width(value) {
        Some(Length::Px(n)) if n <= 10_000 => Some(n),
        _ => None,
    }
}

/// FL28: `Some(None)` is `currentcolor` (the initial value); `Some(Some(rgb))` an explicit opaque colour.
fn parse_outline_colour(value: &str) -> Option<Option<u32>> {
    if value == "currentcolor" {
        return Some(None);
    }
    opaque_color(value).map(Some)
}

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
    fn source_map_binds_duplicate_text_to_same_dom_not_style_indices() {
        let parser = html5ever::parse_document(
            dom::Sink::new(Limits::DEFAULT.max_text_bytes),
            html5ever::ParseOpts::default(),
        );
        let nodes = parser.one("<!doctype html><style>div::before{content:''}</style><!-- gap --><div><p style='color:red'>same</p><p style='color:blue'>same</p></div>").into_nodes();
        let (tree, sources) = StyleTree::from_dom_with_sources(&nodes, &[], None);
        assert_eq!(sources.len(), tree.nodes.len());
        let ordinary = StyleTree::from_dom_with(&nodes,&[],None,SupplyBudget::CANONICAL,
            true,true,false,None,true);
        assert_eq!(format!("{:?}", tree.nodes), format!("{:?}", ordinary.nodes));
        assert_eq!(tree.truncated,ordinary.truncated);
        assert_eq!(tree.unsupported,ordinary.unsupported);
        let same: Vec<_> = tree.nodes.iter().enumerate()
            .filter(|(_,n)|n.text.as_deref()==Some("same"))
            .map(|(i,n)|(i,sources[i].unwrap(),n.style.color))
            .collect();
        assert_eq!(same.len(),2);
        assert_ne!(same[0].1,same[1].1);
        assert_eq!(same[0].2,0xff0000);
        assert_eq!(same[1].2,0x0000ff);
        assert!(same.iter().any(|(i,dom,_)|i!=dom));
        assert!(tree.nodes.iter().enumerate().any(|(i,n)|n.tag=="::before" && sources[i].is_none()));
        for (styled, source) in tree.nodes.iter().zip(&sources) {
            let Some(handle) = source else {continue};
            match &nodes[*handle].data {
                Data::Text(text)=>assert_eq!(styled.text.as_deref(),Some(text.as_str())),
                Data::Element{name,..}=>assert_eq!(styled.tag,name.local.as_ref()),
                _=>panic!("retained style source must be element or text"),
            }
        }
    }

    #[test]
    fn source_map_remains_parallel_with_hidden_subtrees() {
        let parser = html5ever::parse_document(
            dom::Sink::new(Limits::DEFAULT.max_text_bytes),
            html5ever::ParseOpts::default(),
        );
        let nodes=parser.one("<!doctype html><div style='display:none'><p>hidden</p></div><p style='color:red'>kept</p>").into_nodes();
        let (tree,sources)=StyleTree::from_dom_with_sources(&nodes,&[],None);
        assert_eq!(sources.len(),tree.nodes.len());
        for (i,node) in tree.nodes.iter().enumerate().filter(|(_,n)|n.text.is_some()) {
            assert!(matches!(&nodes[sources[i].unwrap()].data,Data::Text(text) if Some(text)==node.text.as_ref()));
        }
        let (i,node)=tree.nodes.iter().enumerate().find(|(_,n)|n.text.as_deref()==Some("kept")).unwrap();
        assert_eq!(node.style.color,0xff0000);
        assert!(matches!(&nodes[sources[i].unwrap()].data,Data::Text(text) if text=="kept"));
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
    fn pt_lengths_convert_at_four_thirds_px() {
        // 12pt = 16px, 9pt = 12px exactly; a pt length resolving to a
        // fractional pixel (10pt = 13.33px) refuses like a fractional px.
        let t = tree(
            "<html><body><p style='font-size:12pt'>a</p><p style='font-size:9pt'>b</p><p style='font-size:10pt'>c</p></body></html>",
            &[],
        );
        let sizes: Vec<u32> = t
            .nodes
            .iter()
            .filter(|n| n.tag == "p")
            .map(|n| n.style.font_size)
            .collect();
        assert_eq!(sizes, [16, 12, 16]);
    }

    #[test]
    fn zero_pt_is_zero_px() {
        let t = tree(
            "<html><body><p style='margin-left:0pt;font-size:15pt'>a</p></body></html>",
            &[],
        );
        let p = t.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!(p.style.font_size, 20);
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
mod em_font_size_tests {
    //! CANDIDATE (UNREVIEWED): `font-size: <n>em` only through the extended-CSS opt-in entry.
    use crate::style_diagnostics::RefusalReason;
    use crate::style_tree::StyleTree;
    use crate::{
        parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget,
    };

    fn ext(html: &str) -> StyleTree {
        parse_style_tree_with_inline_sheets_and_selectors(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
        )
    }
    fn default(html: &str) -> StyleTree {
        parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT)
    }
    fn size(t: &StyleTree, tag: &str) -> u32 {
        t.nodes
            .iter()
            .find(|n| n.tag == tag)
            .unwrap()
            .style
            .font_size
    }
    fn sheet(css: &str, body: &str) -> String {
        format!("<!doctype html><html><head><style>{css}</style></head><body>{body}</body></html>")
    }
    fn shape(t: &StyleTree) -> Vec<(String, u32, Option<u32>)> {
        t.nodes
            .iter()
            .filter(|n| n.text.is_none())
            .map(|n| (n.tag.clone(), n.style.font_size, n.style.line_height))
            .collect()
    }

    #[test]
    fn default_and_other_entries_still_refuse_em() {
        for v in ["1em", "1.5em", ".5em"] {
            let h = format!("<!doctype html><p style='font-size:{v}'>a</p>");
            let t = default(&h);
            assert!(t.unsupported, "{v}");
            assert_eq!(
                t.diagnostics.first_reason(),
                Some(RefusalReason::CssDeclaration),
                "{v}"
            );
            let s = crate::parse_style_tree_with_supply(
                h.as_bytes(),
                &[],
                &Limits::DEFAULT,
                None,
                SupplyBudget::CANONICAL,
            );
            assert!(s.unsupported, "{v}");
            let i = crate::parse_style_tree_with_inline_sheets(
                h.as_bytes(),
                &[],
                &Limits::DEFAULT,
                None,
                SupplyBudget::CANONICAL,
            );
            assert!(i.unsupported, "{v}");
            // the stylesheet path is gated the same way as the style attribute
            let rule = sheet(&format!("p{{font-size:{v}}}"), "<p>a</p>");
            assert!(default(&rule).unsupported, "{v}");
            assert!(
                crate::parse_style_tree_with_inline_sheets(
                    rule.as_bytes(),
                    &[],
                    &Limits::DEFAULT,
                    None,
                    SupplyBudget::CANONICAL
                )
                .unsupported,
                "{v}"
            );
            assert!(!ext(&rule).unsupported, "{v}");
        }
    }

    #[test]
    fn em_resolves_against_the_parent_not_the_root() {
        // parent 20px, root 16px: 1.4em is 28 (parent-relative); 1.4rem is 22.4 and refuses.
        let t = ext("<!doctype html><section style='font-size:20px'><p style='font-size:1.4em'>a</p></section>");
        assert!(!t.unsupported);
        assert_eq!(size(&t, "p"), 28);
        let r = ext("<!doctype html><section style='font-size:20px'><p style='font-size:1.4rem'>a</p></section>");
        assert!(r.unsupported);
        assert_eq!(
            r.diagnostics.first_reason(),
            Some(RefusalReason::ComputedValue)
        );
        // nested compounding through each parent's computed pixels: 16 -> 24 -> 36
        let n = ext("<!doctype html><div style='font-size:1.5em'><p style='font-size:1.5em'>a<span>b</span></p></div>");
        assert!(!n.unsupported);
        assert_eq!(
            (size(&n, "div"), size(&n, "p"), size(&n, "span")),
            (24, 36, 36)
        );
        // root: the parent of html is the initial 16px
        let h = ext("<!doctype html><html style='font-size:1.5em'><body><p style='font-size:1em'>a</p></body></html>");
        assert!(!h.unsupported);
        assert_eq!((size(&h, "html"), size(&h, "p")), (24, 24));
        // 1em inside a 24px parent is 24, not 16
        let k = ext(
            "<!doctype html><div style='font-size:1.5rem'><p style='font-size:1em'>a</p></div>",
        );
        assert_eq!(size(&k, "p"), 24);
    }

    #[test]
    fn fractional_pixels_are_refused_never_rounded() {
        let t = ext("<!doctype html><p style='font-size:1.4em'>a</p>");
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::ComputedValue)
        );
        assert_eq!(size(&t, "p"), 0);
        let same = ext("<!doctype html><p style='font-size:140%'>a</p>");
        assert_eq!(
            same.diagnostics.first_reason(),
            t.diagnostics.first_reason()
        );
        assert_eq!(size(&same, "p"), 0);
    }

    #[test]
    fn malformed_em_is_refused_as_syntax() {
        for bad in [
            "0em",
            "0.0000em",
            "-1em",
            "+1em",
            "1.em",
            ".em",
            "em",
            "1e1em",
            "1EMx",
            "1emm",
            "1 em",
            "1.00001em",
            "99999999em",
            "1.5",
            "calc(1em)",
            "1em 2em",
            "1ex",
            "1remm",
        ] {
            let h = format!("<!doctype html><p style='font-size:{bad}'>a</p>");
            let t = ext(&h);
            assert!(t.unsupported, "{bad}");
            assert_eq!(
                t.diagnostics.first_reason(),
                Some(RefusalReason::CssDeclaration),
                "{bad}"
            );
        }
        for ok in [
            "1em",
            "1EM",
            " 1.5em ",
            ".5em",
            "2.0000em",
            "1.5em !important",
        ] {
            let h = format!("<!doctype html><p style='font-size:{ok}'>a</p>");
            assert!(!ext(&h).unsupported, "{ok}");
        }
    }

    #[test]
    fn em_matches_the_equivalent_percentage_exactly() {
        for (em, pct) in [
            ("1em", "100%"),
            ("1.5em", "150%"),
            ("0.5em", "50%"),
            (".5em", "50%"),
            ("2em", "200%"),
            ("1.25em", "125%"),
            ("1.4em", "140%"),
            ("0.95em", "95%"),
            ("1.2345em", "123.45%"),
        ] {
            for ctx in [
                "<p>a<span>b</span></p>",
                "<div style='font-size:20px'><p>a<span>b</span></p></div>",
                "<div style='font-size:1.5rem'><p>a</p></div>",
            ] {
                let build =
                    |v: &str| sheet(&format!("p{{font-size:{v}}} p{{line-height:120%}}"), ctx);
                let (a, b) = (ext(&build(em)), ext(&build(pct)));
                assert_eq!(shape(&a), shape(&b), "{em} vs {pct} in {ctx}");
                assert_eq!(a.unsupported, b.unsupported, "{em} vs {pct}");
                assert_eq!(
                    a.diagnostics.first_reason(),
                    b.diagnostics.first_reason(),
                    "{em} vs {pct}"
                );
            }
        }
    }

    #[test]
    fn em_follows_the_normal_cascade() {
        let body = "<div><p>a<span>b</span></p></div>";
        let t = ext(&sheet(
            "p{font-size:200%} p{font-size:1.5em !important} p{font-size:3em}",
            body,
        ));
        assert_eq!(size(&t, "p"), 24);
        let spec = ext(&sheet("p{font-size:2em} div p{font-size:1.5em}", body));
        assert_eq!(size(&spec, "p"), 24);
        let inherit = ext(&sheet("p{font-size:1.5em} span{font-size:inherit}", body));
        assert_eq!((size(&inherit, "p"), size(&inherit, "span")), (24, 24));
        // a fractional declaration that loses the cascade does not refuse the page
        let overridden = ext(&sheet("p{font-size:1.4em} p{font-size:1.5em}", body));
        assert!(!overridden.unsupported);
        assert_eq!(size(&overridden, "p"), 24);
        // one that wins does
        let wins = ext(&sheet(
            "p{font-size:1.4em !important} p{font-size:1.5em}",
            body,
        ));
        assert!(wins.unsupported);
        assert_eq!(
            wins.diagnostics.first_reason(),
            Some(RefusalReason::ComputedValue)
        );
    }

    #[test]
    fn em_through_custom_property_substitution_matches_percentage_and_stays_gated() {
        // The deferred var() path re-parses the substituted text with the same flag.
        let cases = [
            (
                "p{--s:1.5em;font-size:var(--s)}",
                "p{--s:150%;font-size:var(--s)}",
            ),
            (
                "p{--s:1.4em;font-size:var(--s)}",
                "p{--s:140%;font-size:var(--s)}",
            ),
            (
                "div{--s:1.5em} p{font-size:var(--s)}",
                "div{--s:150%} p{font-size:var(--s)}",
            ),
            (
                "div{font-size:1.5em;--s:1.5em} p{font-size:var(--s)}",
                "div{font-size:150%;--s:150%} p{font-size:var(--s)}",
            ),
            (
                "p{--s:1.5em;font-size:var(--s) !important} p{font-size:2em}",
                "p{--s:150%;font-size:var(--s) !important} p{font-size:200%}",
            ),
        ];
        for (em, pct) in cases {
            let body = "<div><p>a<span>b</span></p></div>";
            let (a, b) = (ext(&sheet(em, body)), ext(&sheet(pct, body)));
            assert_eq!(shape(&a), shape(&b), "{em}");
            assert_eq!(a.unsupported, b.unsupported, "{em}");
            assert_eq!(
                a.diagnostics.first_reason(),
                b.diagnostics.first_reason(),
                "{em}"
            );
            for r in RefusalReason::ALL {
                assert_eq!(a.diagnostics.count(r), b.diagnostics.count(r), "{em} {r:?}");
            }
            // the default entry refuses the em form, on the deferred path too
            let d = default(&sheet(em, body));
            assert!(d.unsupported, "{em}");
        }
        let ok = ext(&sheet("p{--s:1.5em;font-size:var(--s)}", "<p>a</p>"));
        assert!(!ok.unsupported);
        assert_eq!(size(&ok, "p"), 24);
    }

    #[test]
    fn em_font_size_does_not_change_other_em_properties_or_em_free_pages() {
        // margin/padding em keep their own-font-size base, in both entries
        let m = "<!doctype html><p style='font-size:20px;padding:1em;margin:0.5em'>a</p>";
        assert_eq!(shape(&ext(m)), shape(&default(m)));
        assert_eq!(ext(m).unsupported, default(m).unsupported);
        // a page with no font-size em parses identically through both entries
        let pages = [
            "<!doctype html><p style='font-size:150%'>a</p>",
            "<!doctype html><html style='font-size:20px'><body><div style='font-size:2rem'><span style='font-size:50%'>a</span></div></body></html>",
            "<!doctype html><p style='font-size:1.4rem'>a</p>",
        ];
        for h in pages {
            let (a, b) = (default(h), ext(h));
            assert_eq!(shape(&a), shape(&b), "{h}");
            assert_eq!(a.unsupported, b.unsupported, "{h}");
            assert_eq!(
                a.diagnostics.first_reason(),
                b.diagnostics.first_reason(),
                "{h}"
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
        let size = |tr: &crate::style_tree::StyleTree, tag: &str| {
            tr.nodes
                .iter()
                .find(|n| n.tag == tag)
                .unwrap()
                .style
                .font_size
        };
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
        for bad in [
            "font-size:0rem",
            "font-size:1.rem",
            "font-size:.rem",
            "font-size:1e1rem",
            "font-size:+1rem",
            "font-size:1.4rem",
            "font-size:1.00001rem",
            "font-size:-1rem",
            "font-size:1REMx",
            "font-size:99999999rem",
        ] {
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
    const _: () = assert!(
        crate::DEVICE_PX_PER_CSS_PX == 1,
        "border snap is the dsf == 1 case only"
    );
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
fn border_color(token: &str, ext: bool) -> Option<BorderColor> {
    match token {
        "transparent" => Some(BorderColor::Transparent),
        "currentcolor" => Some(BorderColor::CurrentColor),
        _ if ext && fully_transparent_rgba(token) => Some(BorderColor::Transparent),
        _ => opaque_color(token).map(BorderColor::Opaque),
    }
}

/// Exactly `rgba(R, G, B, 0)`: three decimal integers 0..=255 and the literal alpha `0`
/// (ASCII spaces around commas allowed; the declaration layer has already lowercased).
/// Every such colour paints nothing, like `transparent`; the border colour is only used by
/// paint, so the computed value difference is not observable in layout or paint. Anything
/// else (`0.0`, `0%`, a non-zero alpha, out-of-range or signed channels, other forms)
/// stays refused by the caller.
fn fully_transparent_rgba(token: &str) -> bool {
    let Some(inner) = token
        .strip_prefix("rgba(")
        .and_then(|t| t.strip_suffix(')'))
    else {
        return false;
    };
    let parts: Vec<&str> = inner.split(',').map(|p| p.trim_matches(' ')).collect();
    let [r, g, b, a] = parts.as_slice() else {
        return false;
    };
    let channel = |c: &str| {
        !c.is_empty()
            && c.len() <= 3
            && c.bytes().all(|x| x.is_ascii_digit())
            && c.parse::<u32>().is_ok_and(|v| v <= 255)
    };
    channel(r) && channel(g) && channel(b) && *a == "0"
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

fn border_declarations(name: &str, value: &str, ext: bool) -> BorderParse {
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
                _ => {
                    four_sides(&tokens, |t| border_color(t, ext)).map(|v| v.map(Value::BorderColor))
                }
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
            _ => border_color(value, ext).map(Value::BorderColor),
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
        } else if let (false, Some(c)) = (seen_colour, border_color(token, ext)) {
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
        return (tt < crate::computed_style::PADDING_REM_FLAG)
            .then_some(crate::computed_style::PADDING_REM_FLAG | tt);
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
    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or((false, value), |r| (true, r));
    if unsigned.ends_with("rem") {
        let tt = i32::try_from(parse_rem(unsigned)?).ok()?;
        return Some(Value::MarginRem(if negative {
            tt.checked_neg()?
        } else {
            tt
        }));
    }
    parse_margin(value).map(Value::Margin)
}

#[cfg(test)]
mod custom_property_tests {
    //! CANDIDATE: custom properties and var(). Expected values follow CSS
    //! Custom Properties Level 1; the same cases must also be probed in real Chrome.
    use super::*;
    use crate::{parse_style_tree, Limits};

    fn style(html: &str) -> StyleTree {
        parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT)
    }
    fn p_color(html: &str) -> Option<u32> {
        let t = style(html);
        if t.unsupported {
            return None;
        }
        t.nodes.iter().find(|n| n.tag == "p").map(|n| n.style.color)
    }
    fn refused(html: &str) -> bool {
        style(html).unsupported
    }

    #[test]
    fn defined_on_root_and_used_below_inherits() {
        let html = "<!doctype html><style>html{--c:#ff0000}p{color:var(--c)}</style><p>x</p>";
        assert_eq!(p_color(html), Some(0xff_00_00));
    }

    #[test]
    fn names_are_case_sensitive() {
        assert!(refused(
            "<!doctype html><style>html{--Foo:red}p{color:var(--foo)}</style><p>x</p>"
        ));
        assert_eq!(
            p_color("<!doctype html><style>html{--Foo:#00ff00;--foo:#0000ff}p{color:var(--Foo)}</style><p>x</p>"),
            Some(0x00_ff_00)
        );
    }

    #[test]
    fn fallback_used_only_when_undefined_and_may_nest() {
        assert_eq!(
            p_color("<!doctype html><style>p{color:var(--x,#00f)}</style><p>x</p>"),
            Some(0x00_00_ff)
        );
        assert_eq!(
            p_color("<!doctype html><style>p{color:var(--x,var(--y,#00f))}</style><p>x</p>"),
            Some(0x00_00_ff)
        );
        assert_eq!(
            p_color("<!doctype html><style>html{--x:#f00}p{color:var(--x,#00f)}</style><p>x</p>"),
            Some(0xff_00_00)
        );
    }

    #[test]
    fn cascade_uses_specificity_order_and_important() {
        // Later equal-specificity rule wins.
        assert_eq!(
            p_color("<!doctype html><style>html{--c:#f00}html{--c:#00f}p{color:var(--c)}</style><p>x</p>"),
            Some(0x00_00_ff)
        );
        // Higher specificity beats a later rule.
        assert_eq!(
            p_color("<!doctype html><style>p{--c:#f00}html p{--c:#0f0}p{--c:#00f}p{color:var(--c)}</style><p>x</p>"),
            Some(0x00_ff_00)
        );
        // !important beats a later, more specific normal declaration.
        assert_eq!(
            p_color("<!doctype html><style>p{--c:#f00 !important}html p{--c:#0f0}p{color:var(--c)}</style><p>x</p>"),
            Some(0xff_00_00)
        );
    }

    #[test]
    fn child_override_does_not_reresolve_parent_computed_tokens() {
        // --b was computed at html as red; body redefining --a must not change it.
        assert_eq!(
            p_color("<!doctype html><style>html{--a:#f00;--b:var(--a)}body{--a:#00f}p{color:var(--b)}</style><p>x</p>"),
            Some(0xff_00_00)
        );
        // A child that redefines --b itself resolves against its own --a.
        assert_eq!(
            p_color("<!doctype html><style>html{--a:#f00}body{--a:#00f}p{--b:var(--a);color:var(--b)}</style><p>x</p>"),
            Some(0x00_00_ff)
        );
    }

    #[test]
    fn invalid_winning_var_never_falls_back_to_a_lower_declaration() {
        // Chrome: the winner is invalid at computed-value time -> unset (inherited black), not red.
        assert!(refused(
            "<!doctype html><style>p{color:#f00}p{color:var(--nope)}</style><p>x</p>"
        ));
        // Even when the invalid var() declaration would lose, the page refuses (stricter).
        assert!(refused(
            "<!doctype html><style>p{color:var(--nope)}p{color:#00f}</style><p>x</p>"
        ));
    }

    #[test]
    fn inline_style_outranks_id_selector_for_custom_properties_and_typed_values() {
        // Normal: inline beats #id regardless of order (Chrome-probed).
        assert_eq!(
            p_color("<!doctype html><style>#i{--c:#f00}</style><p id=i style='--c:#00f;color:var(--c)'>x</p>"),
            Some(0x00_00_ff)
        );
        assert_eq!(
            p_color("<!doctype html><style>#i{color:#f00}</style><p id=i style='color:#00f'>x</p>"),
            Some(0x00_00_ff)
        );
        // Author !important beats inline normal; inline !important beats author !important.
        assert_eq!(
            p_color("<!doctype html><style>#i{--c:#f00 !important}</style><p id=i style='--c:#00f;color:var(--c)'>x</p>"),
            Some(0xff_00_00)
        );
        assert_eq!(
            p_color("<!doctype html><style>#i{--c:#f00 !important}</style><p id=i style='--c:#00f !important;color:var(--c)'>x</p>"),
            Some(0x00_00_ff)
        );
        // var() in an inline declaration versus an #id rule.
        assert_eq!(
            p_color("<!doctype html><style>html{--a:#00f}#i{color:#f00}</style><p id=i style='color:var(--a)'>x</p>"),
            Some(0x00_00_ff)
        );
    }

    #[test]
    fn var_shorthand_keeps_authored_cascade_position_against_longhands() {
        let get = |html: &str| {
            let t = style(html);
            assert!(!t.unsupported, "{html}");
            t.nodes
                .iter()
                .find(|n| n.tag == "p")
                .map(|n| n.style)
                .unwrap()
        };
        // Shorthand authored AFTER the longhand overrides it; before it, loses.
        let after = get(
            "<!doctype html><style>html{--m:5px}p{margin-top:9px;margin:var(--m)}</style><p>x</p>",
        );
        let lit_after = get("<!doctype html><style>p{margin-top:9px;margin:5px}</style><p>x</p>");
        assert_eq!(after, lit_after);
        let before = get(
            "<!doctype html><style>html{--m:5px}p{margin:var(--m);margin-top:9px}</style><p>x</p>",
        );
        let lit_before = get("<!doctype html><style>p{margin:5px;margin-top:9px}</style><p>x</p>");
        assert_eq!(before, lit_before);
    }

    #[test]
    fn inline_declaration_order_ties_follow_authored_order_both_directions() {
        // Later inline declaration wins whether typed or var(); Chrome-probed.
        assert_eq!(
            p_color("<!doctype html><style>html{--c:#f00}</style><p style='color:var(--c);color:#00f'>x</p>"),
            Some(0x00_00_ff)
        );
        assert_eq!(
            p_color("<!doctype html><style>html{--c:#00f}</style><p style='color:#f00;color:var(--c)'>x</p>"),
            Some(0x00_00_ff)
        );
        assert_eq!(
            p_color("<!doctype html><style>html{--c:#f00;--d:#00f}</style><p style='color:var(--c);color:var(--d)'>x</p>"),
            Some(0x00_00_ff)
        );
        // !important: the important declaration wins regardless of order.
        assert_eq!(
            p_color("<!doctype html><style>html{--c:#f00}</style><p style='color:var(--c) !important;color:#00f'>x</p>"),
            Some(0xff_00_00)
        );
        assert_eq!(
            p_color("<!doctype html><style>html{--c:#00f}</style><p style='color:#f00 !important;color:var(--c)'>x</p>"),
            Some(0xff_00_00)
        );
        let get = |html: &str| {
            let t = style(html);
            assert!(!t.unsupported, "{html}");
            t.nodes
                .iter()
                .find(|n| n.tag == "p")
                .map(|n| n.style)
                .unwrap()
        };
        // Shorthand via var() vs typed longhand, inline, both orders.
        assert_eq!(
            get("<!doctype html><style>html{--m:5px}</style><p style='margin:var(--m);margin-top:9px'>x</p>"),
            get("<!doctype html><p style='margin:5px;margin-top:9px'>x</p>")
        );
        assert_eq!(
            get("<!doctype html><style>html{--m:5px}</style><p style='margin-top:9px;margin:var(--m)'>x</p>"),
            get("<!doctype html><p style='margin-top:9px;margin:5px'>x</p>")
        );
        // Inline typed order unchanged by the offset fix (no custom property involved).
        assert_eq!(
            get("<!doctype html><p style='margin-top:9px;margin:5px'>x</p>"),
            get("<!doctype html><p style='margin:5px'>x</p>")
        );
    }

    #[test]
    fn raw_comments_in_custom_or_var_values_refuse_instead_of_merging_tokens() {
        for html in [
            "<!doctype html><style>p{--x:b/**/lue;color:var(--x)}</style><p>x</p>",
            "<!doctype html><style>p{--x:1p/**/x;margin-top:var(--x)}</style><p>x</p>",
            "<!doctype html><style>html{--x:red}p{color:var(--x)/**/}</style><p>x</p>",
            "<!doctype html><style>html{--x:1}p{margin-top:var(--x)/**/px}</style><p>x</p>",
            "<!doctype html><style>p{color:var(--nope,b/**/lue)}</style><p>x</p>",
            "<!doctype html><style>p{color:va/**/r(--x,red)}</style><p>x</p>",
            "<!doctype html><style>p{-/**/-x:red}p{color:red}</style><p>x</p>",
            "<!doctype html><style>html{--a:red}</style><p style='--x:b/**/lue;color:var(--x)'>x</p>",
            "<!doctype html><style>html{--a:red}</style><p style='color:var(--a,b/**/lue)'>x</p>",
            "<!doctype html><style>html{--a:red /* c */}p{color:var(--a)}</style><p>x</p>",
        ] {
            assert!(refused(html), "{html}");
        }
        // Comments outside custom/var values keep the legacy behavior.
        assert_eq!(
            p_color("<!doctype html><style>p{color:/*c*/#f00;/*x*/margin:1px}</style><p>x</p>"),
            Some(0xff_00_00)
        );
        assert_eq!(
            p_color(
                "<!doctype html><style>html{--a:#f00;/* note */}p{color:var(--a)}</style><p>x</p>"
            ),
            Some(0xff_00_00)
        );
    }

    #[test]
    fn custom_property_work_is_bounded() {
        // 300 distinct definitions in one element refuse (rule/declaration caps) rather than allocate unboundedly.
        let defs: String = (0..300).map(|i| format!("--v{i}:1;")).collect();
        assert!(refused(&format!(
            "<!doctype html><style>html{{{defs}}}</style><p>x</p>"
        )));
        // Over-long value refuses.
        let long = "a".repeat(2000);
        assert!(refused(&format!(
            "<!doctype html><style>html{{--l:{long}}}</style><p>x</p>"
        )));
    }

    #[test]
    fn tree_wide_custom_property_budget_trips_without_unbounded_allocation() {
        let v = "a".repeat(1000);
        let defs: String = (0..200).map(|i| format!("--v{i}:{v};")).collect();
        let divs: String = (0..2000).map(|_| "<div style='--z:1'>x</div>").collect();
        let html = format!("<!doctype html><style>html{{{defs}}}</style><body>{divs}");
        let t = style(&html);
        assert!(t.unsupported);
        assert!(t.diagnostics.count(RefusalReason::ComputedValue) >= 1);
        // The same page with a few elements is within budget.
        let few =
            format!("<!doctype html><style>html{{{defs}}}</style><body><div style='--z:1'>x</div>");
        assert!(!style(&few).unsupported);
    }

    #[test]
    fn invalid_custom_property_names_and_tokens_refuse() {
        for decl in [
            "--:1",
            "--a b:1",
            "--a:(1",
            "--a:1)",
            "--a:[1)",
            "--a:\"x",
            "--a:1\\",
            "--a:{}",
            "--a:1 !foo",
            "-x:1",
        ] {
            assert!(
                refused(&format!(
                    "<!doctype html><style>html{{{decl}}}</style><p>x</p>"
                )),
                "{decl}"
            );
        }
        // Empty and whitespace values are valid (spec: --foo: ; is an empty value).
        // KNOWN LIMITATION (measured): `--e:;` and `--e: ;` are valid empty values per the
        // spec (Chrome accepts them) but the existing declaration splitter rejects an empty
        // value, so the page refuses. Stricter than Chrome, never a wrong accept.
        for (css, expect_refused) in [
            ("html{--e: ;color:#f00}", true),
            ("html{--e:;color:#f00}", true),
        ] {
            assert_eq!(
                refused(&format!("<!doctype html><style>{css}</style><p>x</p>")),
                expect_refused,
                "{css}"
            );
        }
    }

    #[test]
    fn closer_definition_overrides_inherited_and_initial_resets() {
        assert_eq!(
            p_color("<!doctype html><style>html{--c:#f00}body{--c:#00f}p{color:var(--c)}</style><p>x</p>"),
            Some(0x00_00_ff)
        );
        assert_eq!(
            p_color("<!doctype html><style>html{--c:#f00}p{--c:initial;color:var(--c,#0f0)}</style><p>x</p>"),
            Some(0x00_ff_00)
        );
        assert_eq!(
            p_color(
                "<!doctype html><style>html{--c:#f00}p{--c:inherit;color:var(--c)}</style><p>x</p>"
            ),
            Some(0xff_00_00)
        );
    }

    #[test]
    fn shorthand_and_multi_token_values_substitute_before_parsing() {
        let html =
            "<!doctype html><style>html{--b:2px solid #00f}p{border:var(--b)}</style><p>x</p>";
        assert!(!refused(html));
        let plain = style("<!doctype html><style>p{border:2px solid #00f}</style><p>x</p>");
        let var = style(html);
        let get = |t: &StyleTree| {
            t.nodes
                .iter()
                .find(|n| n.tag == "p")
                .map(|n| n.style)
                .unwrap()
        };
        assert_eq!(get(&plain), get(&var));
    }

    #[test]
    fn custom_values_can_reference_each_other_and_inline_style_works() {
        assert_eq!(
            p_color("<!doctype html><style>html{--a:#f00;--b:var(--a)}p{color:var(--b)}</style><p>x</p>"),
            Some(0xff_00_00)
        );
        assert_eq!(
            p_color("<!doctype html><style>html{--a:#f00}</style><p style='--a:#00f;color:var(--a)'>x</p>"),
            Some(0x00_00_ff)
        );
    }

    #[test]
    fn unimplemented_or_invalid_forms_refuse() {
        for html in [
            // cycle, self reference, undefined without fallback
            "<!doctype html><style>html{--a:var(--b);--b:var(--a)}p{color:var(--a,red)}</style><p>x</p>",
            "<!doctype html><style>html{--a:var(--a,red)}p{color:var(--a)}</style><p>x</p>",
            "<!doctype html><style>p{color:var(--nope)}</style><p>x</p>",
            // glued tokens, CSS-wide keyword through var, malformed var
            "<!doctype html><style>html{--n:10}p{margin-top:var(--n)px}</style><p>x</p>",
            "<!doctype html><style>html{--k:inherit}p{color:var(--k)}</style><p>x</p>",
            "<!doctype html><style>p{color:var(--x}</style><p>x</p>",
            // strings, url, escapes, revert in a custom value
            "<!doctype html><style>html{--s:\"a\"}p{color:red}</style><p>x</p>",
            "<!doctype html><style>html{--u:url(x)}p{color:red}</style><p>x</p>",
            "<!doctype html><style>html{--r:revert}p{color:red}</style><p>x</p>",
            // substituted value is still an unsupported property/value
            "<!doctype html><style>html{--f:inline-start}p{float:var(--f)}</style><p>x</p>",
            "<!doctype html><style>html{--i:url(x)}p{background-image:var(--i)}</style><p>x</p>",
        ] {
            assert!(refused(html), "{html}");
        }
    }

    #[test]
    fn float_clear_cascade_var_and_non_inheritance() {
        use crate::computed_style::{Clear, Float};
        let t = style("<!doctype html><style>html{--f:left}.a{float:var(--f);clear:both}</style><div class=a><p>x</p></div><p class=b>y</p>");
        let div = t
            .nodes
            .iter()
            .find(|n| n.tag == "div")
            .unwrap()
            .style
            .positioned;
        assert_eq!((div.float, div.clear), (Float::Left, Clear::Both));
        assert!(!t.unsupported);
        // not inherited: the child p and a later p keep the initial values
        for n in t.nodes.iter().filter(|n| n.tag == "p") {
            assert_eq!(
                (n.style.positioned.float, n.style.positioned.clear),
                (Float::None, Clear::None)
            );
        }
        // later declaration wins; none/default parses
        let c = style("<!doctype html><style>p{float:left}p{float:none}</style><p>x</p>");
        let p = c
            .nodes
            .iter()
            .find(|n| n.tag == "p")
            .unwrap()
            .style
            .positioned;
        assert_eq!(p.float, Float::None);
        // inherit takes the parent's computed value
        let i = style(
            "<!doctype html><style>div{float:left}p{float:inherit}</style><div><p>x</p></div>",
        );
        let p = i
            .nodes
            .iter()
            .find(|n| n.tag == "p")
            .unwrap()
            .style
            .positioned;
        assert_eq!(p.float, Float::Left);
        // unset and initial on a non-inherited property give none
        let u =
            style("<!doctype html><style>div{float:left}p{float:unset}</style><div><p>x</p></div>");
        assert_eq!(
            u.nodes
                .iter()
                .find(|n| n.tag == "p")
                .unwrap()
                .style
                .positioned
                .float,
            Float::None
        );
    }

    #[test]
    fn blocks_without_custom_properties_are_unchanged_by_the_new_path() {
        // Adding an unused custom property switches the block to the
        // per-declaration path. Every computed style must stay identical.
        for body in [
            "color:#112233;margin:1px 2px 3px 4px;padding:2px;font-size:20px",
            "border:2px solid #ff0000;background:#00ff00;line-height:1.5",
            "width:50%;height:30px;box-sizing:border-box;display:block",
            "background-color:red;background-color:blue",
        ] {
            let a = style(&format!(
                "<!doctype html><style>p{{{body}}}</style><p>x</p>"
            ));
            let b = style(&format!(
                "<!doctype html><style>p{{{body};--unused:1}}</style><p>x</p>"
            ));
            assert_eq!(a.unsupported, b.unsupported, "{body}");
            assert_eq!(
                a.nodes.iter().map(|n| n.style).collect::<Vec<_>>(),
                b.nodes.iter().map(|n| n.style).collect::<Vec<_>>(),
                "{body}"
            );
        }
    }
}

/// FL20 candidate: initial-value declarations accepted as no-ops under the extended entry only.
const INITIAL_NOOPS: &[(&str, &str)] = &[
    ("opacity", "1"),
    ("text-transform", "none"),
    ("overflow-anchor", "auto"),
    ("text-shadow", "none"),
    ("transform-origin", "50% 50%"),
    // FL38: `visibility:visible` is now parsed as a typed (inherited) property under the ext entry, no longer a no-op.
    ("word-break", "normal"),
    ("hyphens", "manual"),
    ("z-index", "auto"),
    ("will-change", "auto"),
    ("isolation", "auto"),
    ("mix-blend-mode", "normal"),
    ("backface-visibility", "visible"),
    ("content-visibility", "visible"),
    ("contain", "none"),
    ("clip-path", "none"),
    ("filter", "none"),
    ("transform", "none"),
    ("perspective", "none"),
    ("object-fit", "fill"),
    ("scroll-behavior", "auto"),
    ("overscroll-behavior", "auto"),
    ("caret-color", "auto"),
    ("accent-color", "auto"),
    ("touch-action", "auto"),
    ("image-rendering", "auto"),
    ("forced-color-adjust", "auto"),
    ("text-indent", "0"),
    ("text-indent", "0px"),
    ("word-spacing", "normal"),
    ("letter-spacing", "normal"),
    ("vertical-align", "baseline"),
    ("table-layout", "auto"),
    ("border-collapse", "separate"),
    ("empty-cells", "show"),
    ("orphans", "2"),
    ("widows", "2"),
    ("writing-mode", "horizontal-tb"),
    ("order", "0"),
    ("align-self", "auto"),
    ("align-content", "normal"),
    ("aspect-ratio", "auto"),
    ("text-overflow", "clip"),
    ("resize", "none"),
    ("quotes", "auto"),
];

#[cfg(test)]
mod link_state_tests {
    use crate::style_diagnostics::RefusalReason;
    use crate::{
        parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors,
        parse_style_tree_with_state, Limits, SupplyBudget,
    };
    use crate::{style_tree::StyleTree, HistoryState, SnapshotState};

    const FRESH: SnapshotState = SnapshotState {
        history: Some(HistoryState::FreshNoHistory),
        input: None,
        focus: None,
        target: None,
    };
    const QUIESCENT: SnapshotState = SnapshotState {
        history: None,
        input: Some(crate::InputState::QuiescentCapture),
        focus: None,
        target: None,
    };
    const BOTH: SnapshotState = SnapshotState {
        history: Some(HistoryState::FreshNoHistory),
        input: Some(crate::InputState::QuiescentCapture),
        focus: None,
        target: None,
    };
    const UNFOCUSED: SnapshotState = SnapshotState {
        history: None,
        input: None,
        focus: Some(crate::FocusState::UnfocusedInitial),
        target: None,
    };
    const ALL: SnapshotState = SnapshotState {
        history: Some(HistoryState::FreshNoHistory),
        input: Some(crate::InputState::QuiescentCapture),
        focus: Some(crate::FocusState::UnfocusedInitial),
        target: None,
    };

    fn state(html: &str) -> StyleTree {
        parse_style_tree_with_state(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
            &FRESH,
        )
    }
    fn ext(html: &str) -> StyleTree {
        parse_style_tree_with_inline_sheets_and_selectors(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
        )
    }
    fn default(html: &str) -> StyleTree {
        parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT)
    }
    fn sheet(css: &str, body: &str) -> String {
        format!("<!doctype html><html><head><style>{css}</style></head><body>{body}</body></html>")
    }

    #[test]
    fn link_matches_only_html_href_elements_under_fresh_guarantee() {
        // Paired synthetic computed styles: non-empty href, empty (valueless)
        // href, no href, area with href. Only the HTML a/area elements WITH an
        // href attribute match.
        let h = sheet(
            "a:link{font-size:33px} area:link{font-size:44px}",
            "<a href=\"https://x\">one</a><a href>two</a><a>three</a>\
             <map><area href=\"https://y\"></map>",
        );
        let t = state(&h);
        assert!(!t.unsupported);
        let a_sizes: Vec<u32> = t
            .nodes
            .iter()
            .filter(|n| n.tag == "a")
            .map(|n| n.style.font_size)
            .collect();
        assert_eq!(
            a_sizes,
            [33, 33, 16],
            "html href, html empty href, html no href"
        );
        let area = t
            .nodes
            .iter()
            .find(|n| n.tag == "area")
            .expect("area retained");
        assert_eq!(area.style.font_size, 44);
    }

    #[test]
    fn link_never_matches_svg_a_in_v1() {
        // SVG a is narrowed out: the DOM flattens `xlink:href` to local `href`
        // (dom.rs keep_attrs), so an SVG href's provenance is ambiguous. NOTE:
        // the ext entry already refuses SVG content with a pre-existing
        // CssDeclaration refusal (unrelated to :link; verified identical on
        // `p{font-size:33px}` over bare `<svg><a>`), so `unsupported` here is
        // that pre-existing refusal, not the state enrollment gate.
        for body in [
            "<svg><a href=\"https://z\">four</a></svg>",
            "<svg><a xlink:href=\"https://w\">five</a></svg>",
        ] {
            let h = sheet("a:link{font-size:33px}", body);
            let t = state(&h);
            assert!(t.unsupported, "{body}");
            assert_eq!(
                t.diagnostics.first_reason(),
                Some(RefusalReason::CssDeclaration),
                "{body}"
            );
            let svg_a = t
                .nodes
                .iter()
                .find(|n| n.tag == "a")
                .expect("svg a retained");
            assert_eq!(svg_a.style.font_size, 16, "{body}");
        }
    }

    #[test]
    fn link_counts_as_a_class_for_specificity() {
        // a:link is (0,1,1) and beats the lone class .x (0,1,0).
        let h = sheet(
            ".x{font-size:22px} a:link{font-size:33px}",
            "<a class=x href=https://y>one</a>",
        );
        let t = state(&h);
        assert!(!t.unsupported);
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "a")
                .unwrap()
                .style
                .font_size,
            33
        );
    }

    #[test]
    fn link_applies_to_compounds_and_combinators() {
        let h = sheet(
            "div > a.x:link[href]{font-size:33px}",
            "<div><a class=x href=https://y>one</a></div><a class=x href=https://y>two</a>",
        );
        let t = state(&h);
        assert!(!t.unsupported);
        let sizes: Vec<u32> = t
            .nodes
            .iter()
            .filter(|n| n.tag == "a")
            .map(|n| n.style.font_size)
            .collect();
        assert_eq!(sizes, [33, 16]);
    }

    #[test]
    fn other_state_pseudos_stay_refused_under_state_entry() {
        for pseudo in [
            ":hover",
            ":active",
            ":focus",
            ":focus-visible",
            ":focus-within",
        ] {
            let h = sheet(
                &format!("a{pseudo}{{font-size:33px}}"),
                "<a href=https://x>one</a>",
            );
            let t = state(&h);
            assert!(t.unsupported, "{pseudo}");
            assert_eq!(
                t.diagnostics.first_reason(),
                Some(RefusalReason::CssSelector),
                "{pseudo}"
            );
        }
    }

    #[test]
    fn visited_enrolls_under_fresh_history_and_never_matches() {
        // Under FreshNoHistory a:visited ENROLLS (parsed, guaranteed) and
        // never matches: no hyperlink is visited in a fresh profile. The
        // a:link rule on the same element still applies.
        // Declaration order puts the never-matching rule LAST so that an
        // erroneous match would win the cascade and fail the assertion.
        let h = sheet(
            "a:link{font-size:22px} a:visited{font-size:44px}",
            "<a href=https://x>one</a><p>two</p>",
        );
        let t = state_with(&h, &FRESH);
        assert!(!t.unsupported);
        let a = t.nodes.iter().find(|n| n.tag == "a").unwrap();
        assert_eq!(a.style.font_size, 22);
        let p = t.nodes.iter().find(|n| n.tag == "p").unwrap();
        assert_eq!(p.style.font_size, 16);
    }

    #[test]
    fn visited_refuses_without_a_history_guarantee() {
        let h = sheet("a:visited{font-size:33px}", "<a href=https://x>one</a>");
        let t = state_with(&h, &QUIESCENT);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
    }

    #[test]
    fn repeated_link_compound_is_a_grammar_bound_refusal() {
        // `a:link:link` is valid CSS; the bounded grammar admits at most one
        // state pseudo-class per compound and REFUSES the repetition. This is
        // a grammar bound, not a claim that the CSS is invalid.
        let h = sheet("a:link:link{font-size:33px}", "<a href=https://x>one</a>");
        let t = state(&h);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
    }

    #[test]
    fn link_fails_closed_without_the_guarantee() {
        let h = sheet("a:link{font-size:33px}", "<a href=https://x>one</a>");
        // Ext entry WITHOUT a snapshot: no absent-state path; the rule is
        // refused at enrollment exactly like an unparseable selector.
        let t = ext(&h);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "a")
                .unwrap()
                .style
                .font_size,
            16
        );
        // Default entry: byte-unchanged refusal behavior.
        let d = default(&h);
        assert!(d.unsupported);
        assert_eq!(
            d.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
        assert_eq!(
            d.nodes
                .iter()
                .find(|n| n.tag == "a")
                .unwrap()
                .style
                .font_size,
            16
        );
    }

    fn state_with(html: &str, snapshot: &SnapshotState) -> StyleTree {
        parse_style_tree_with_state(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
            snapshot,
        )
    }

    #[test]
    fn hover_active_enroll_only_under_quiescent_and_never_match() {
        let h = sheet(
            "div:hover{font-size:33px} a:active{font-size:44px}",
            "<div>one</div><a href=https://x>two</a>",
        );
        // Under QuiescentCapture the rules ENROLL (parsed, guaranteed) and
        // never match: the correct CSS semantic for a quiescent capture.
        let t = state_with(&h, &QUIESCENT);
        assert!(!t.unsupported);
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "div")
                .unwrap()
                .style
                .font_size,
            16
        );
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "a")
                .unwrap()
                .style
                .font_size,
            16
        );
        // Both guarantees present: same outcome for the pointer family.
        let t = state_with(&h, &BOTH);
        assert!(!t.unsupported);
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "div")
                .unwrap()
                .style
                .font_size,
            16
        );
    }

    #[test]
    fn history_and_input_guarantees_are_independently_scoped() {
        // A link-only guarantee does NOT enroll :hover/:active.
        let h = sheet("div:hover{font-size:33px}", "<div>one</div>");
        let t = state_with(&h, &FRESH);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
        // An input-only guarantee does NOT enroll :link.
        let h = sheet("a:link{font-size:33px}", "<a href=https://x>one</a>");
        let t = state_with(&h, &QUIESCENT);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
        // No snapshot at all: :hover/:active fail closed like any unguaranteed state.
        let h = sheet("div:hover{font-size:33px}", "<div>one</div>");
        let t = ext(&h);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
        let d = default(&h);
        assert!(d.unsupported);
        assert_eq!(
            d.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
    }

    #[test]
    fn focus_family_enrolls_only_under_unfocused_and_never_matches() {
        // NB: no <input> in these bodies - the ext path has a pre-existing
        // unrelated CssDeclaration refusal for form controls.
        let h = sheet(
            "a:focus{font-size:33px} div:focus-within span{font-size:44px} p:focus-visible{font-size:55px}",
            "<div><span>one</span></div><a href=https://x>two</a><p>three</p>",
        );
        // Under UnfocusedInitial all three rules ENROLL and never match: the
        // correct CSS semantic for an unfocused initial capture.
        for snapshot in [&UNFOCUSED, &ALL] {
            let t = state_with(&h, snapshot);
            assert!(!t.unsupported);
            for (tag, size) in [("span", 16), ("a", 16), ("p", 16)] {
                assert_eq!(
                    t.nodes
                        .iter()
                        .find(|n| n.tag == tag)
                        .unwrap()
                        .style
                        .font_size,
                    size,
                    "{tag}"
                );
            }
        }
        // History-only, input-only and both non-focus guarantees do NOT
        // enroll the focus family (independent scoping).
        for snapshot in [&FRESH, &QUIESCENT, &BOTH] {
            let t = state_with(&h, snapshot);
            assert!(t.unsupported);
            assert_eq!(
                t.diagnostics.first_reason(),
                Some(RefusalReason::CssSelector)
            );
        }
        // No snapshot at all and the default entry: fail closed identically.
        let t = ext(&h);
        assert!(t.unsupported);
        let d = default(&h);
        assert!(d.unsupported);
        assert_eq!(
            d.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
    }

    #[test]
    fn autofocus_in_dom_voids_unfocused_guarantee_engine_side() {
        // The engine observes `autofocus` itself: with one present, the
        // caller's UnfocusedInitial guarantee cannot hold and enrollment
        // fails closed EXACTLY like the absent-guarantee path.
        let h = sheet("p:focus{font-size:33px}", "<p autofocus>one</p>");
        let gated = state_with(&h, &UNFOCUSED);
        let absent = state_with(&h, &BOTH);
        assert!(gated.unsupported);
        assert_eq!(
            gated.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
        assert_eq!(gated.unsupported, absent.unsupported);
        assert_eq!(
            gated.diagnostics.first_reason(),
            absent.diagnostics.first_reason()
        );
        // Without the autofocus attribute the same guarantee enrolls.
        let h2 = sheet("p:focus{font-size:33px}", "<p>one</p>");
        let t = state_with(&h2, &UNFOCUSED);
        assert!(!t.unsupported);
        // The gate strips ONLY the focus guarantee: a plain rule in the same
        // sheet still applies while the focus rule refuses.
        let h3 = sheet(
            "p:focus{font-size:33px} b{font-size:44px}",
            "<p autofocus>one</p><b>two</b>",
        );
        let t = state_with(&h3, &UNFOCUSED);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "b")
                .unwrap()
                .style
                .font_size,
            44
        );
    }

    #[test]
    fn mixed_family_compounds_refuse_even_with_all_guarantees() {
        // `:hover:focus` carries two state pseudo-classes in one compound:
        // a grammar bound (at most one), refused at parse even when every
        // family guarantee is present.
        for selector in [
            "a:hover:focus",
            "a:focus:hover",
            "a:focus-visible:focus-within",
        ] {
            let h = sheet(
                &format!("{selector}{{font-size:33px}}"),
                "<a href=https://x>one</a>",
            );
            let t = state_with(&h, &ALL);
            assert!(t.unsupported, "{selector}");
            assert_eq!(
                t.diagnostics.first_reason(),
                Some(RefusalReason::CssSelector),
                "{selector}"
            );
        }
    }

    #[test]
    fn focus_within_multilevel_ancestor_enrolls_and_never_matches() {
        // Multi-level ancestor shape: the state pseudo sits three steps from
        // the subject. Enrolled under the guarantee, never matches; refused
        // without it.
        let h = sheet(
            "html:focus-within div span{font-size:33px}",
            "<div><span>one</span></div>",
        );
        let t = state_with(&h, &UNFOCUSED);
        assert!(!t.unsupported);
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "span")
                .unwrap()
                .style
                .font_size,
            16
        );
        let t = state_with(&h, &BOTH);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
    }

    #[test]
    fn focus_group_member_is_independently_fail_closed() {
        // Group semantics: the :focus member enrolls or refuses on its own;
        // the plain member applies either way (the applying twin).
        let h = sheet(
            "a:focus, b{font-size:33px}",
            "<a href=https://x>one</a><b>two</b>",
        );
        let t = state_with(&h, &UNFOCUSED);
        assert!(!t.unsupported);
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "b")
                .unwrap()
                .style
                .font_size,
            33
        );
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "a")
                .unwrap()
                .style
                .font_size,
            16
        );
        let t = state_with(&h, &BOTH);
        assert!(t.unsupported);
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "b")
                .unwrap()
                .style
                .font_size,
            33
        );
    }

    #[test]
    fn focus_within_two_level_ancestor_chain_actual_path() {
        // >=2 ancestor chain on the actual enrollment/application path:
        // the state-bearing compound is TWO descendant steps above the
        // subject. Under UNFOCUSED the rule enrolls and never matches (deep
        // node's font unchanged); the plain control rule in the same sheet
        // applies; without the guarantee the focus rule refuses while the
        // control rule still applies.
        let h = sheet(
            "div:focus-within span b{font-size:33px} i{font-size:44px}",
            "<div><span><b>one</b></span></div><i>two</i>",
        );
        let t = state_with(&h, &UNFOCUSED);
        assert!(!t.unsupported);
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "b")
                .unwrap()
                .style
                .font_size,
            16
        );
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "i")
                .unwrap()
                .style
                .font_size,
            44
        );
        let t = state_with(&h, &BOTH);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssSelector)
        );
        assert_eq!(
            t.nodes
                .iter()
                .find(|n| n.tag == "i")
                .unwrap()
                .style
                .font_size,
            44
        );
    }

    #[test]
    fn repeated_focus_compounds_are_grammar_bound_refusals() {
        for selector in [
            "a:focus:focus",
            "a:focus-visible:focus-within",
            "a:focus:link",
        ] {
            let h = sheet(
                &format!("{selector}{{font-size:33px}}"),
                "<a href=https://x>one</a>",
            );
            let t = state_with(&h, &ALL);
            assert!(t.unsupported, "{selector}");
            assert_eq!(
                t.diagnostics.first_reason(),
                Some(RefusalReason::CssSelector),
                "{selector}"
            );
        }
    }

    #[test]
    fn repeated_hover_active_compounds_are_grammar_bound_refusals() {
        for selector in ["a:hover:hover", "a:active:active", "a:link:hover"] {
            let h = sheet(
                &format!("{selector}{{font-size:33px}}"),
                "<a href=https://x>one</a>",
            );
            let t = state_with(&h, &BOTH);
            assert!(t.unsupported, "{selector}");
            assert_eq!(
                t.diagnostics.first_reason(),
                Some(RefusalReason::CssSelector),
                "{selector}"
            );
        }
    }
}

#[cfg(test)]
mod before_after_tests {
    use crate::style_diagnostics::RefusalReason;
    use crate::{
        parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, style_tree::StyleTree,
        Limits, SupplyBudget,
    };
    fn ext(html: &str) -> StyleTree {
        parse_style_tree_with_inline_sheets_and_selectors(
            html.as_bytes(),
            &[],
            &Limits::DEFAULT,
            None,
            SupplyBudget::CANONICAL,
        )
    }
    fn sheet(css: &str, body: &str) -> String {
        format!("<!doctype html><html><head><style>{css}</style></head><body>{body}</body></html>")
    }
    fn child_tags(t: &StyleTree, tag: &str) -> Vec<String> {
        let (i, n) = t
            .nodes
            .iter()
            .enumerate()
            .find(|(_, n)| n.tag == tag)
            .expect("parent retained");
        n.children.iter().map(|&c| t.nodes[c].tag.clone()).collect()
    }

    #[test]
    fn before_box_is_first_child_and_after_last_on_one_element() {
        let h = sheet(
            "div::before{content:\"\"} div::after{content:\"\"}",
            "<div><p>real</p></div>",
        );
        let t = ext(&h);
        assert!(!t.unsupported);
        assert_eq!(child_tags(&t, "div"), ["::before", "p", "::after"]);
    }

    #[test]
    fn before_content_gate_mirrors_after() {
        // normal/none: no box; "": a box; display:none + "": no box.
        let h = sheet("div::before{content:\"\"}", "<div><p>real</p></div>");
        let t = ext(&h);
        assert!(!t.unsupported);
        assert_eq!(child_tags(&t, "div"), ["::before", "p"]);
        for content in ["normal", "none"] {
            let h = sheet(
                &format!("div::before{{content:{content}}}"),
                "<div><p>real</p></div>",
            );
            let t = ext(&h);
            assert!(!t.unsupported, "{content}");
            assert_eq!(child_tags(&t, "div"), ["p"], "{content}");
        }
        let h = sheet(
            "div::before{content:\"\";display:none}",
            "<div><p>real</p></div>",
        );
        let t = ext(&h);
        assert_eq!(child_tags(&t, "div"), ["p"]);
    }

    #[test]
    fn before_text_content_still_refused_at_intake() {
        // Only ""/normal/none are admitted; text content stays an unknown
        // declaration (no new content capability in this increment).
        let h = sheet("div::before{content:\"x\"}", "<div><p>real</p></div>");
        let t = ext(&h);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssDeclaration)
        );
    }

    #[test]
    fn span_admitted_confined_other_originating_elements_refuse() {
        // ITEM 2(a): span is admitted in EITHER lane, confined to the
        // generated-decoration allowlist with skipped declarations counted.
        // Anything outside BOTH allowlists still refuses (the conservative
        // limit, not a Chrome claim).
        let h = sheet("span::before{content:\"\"}", "<p><span>x</span></p>");
        let t = ext(&h);
        assert!(!t.unsupported);
        assert!(t.nodes.iter().any(|n| n.tag == "::before"));
        let h = sheet("i::before{content:\"\"}", "<p><i>x</i></p>");
        let t = ext(&h);
        assert!(t.unsupported);
        assert_eq!(
            t.diagnostics.first_reason(),
            Some(RefusalReason::CssDeclaration)
        );
    }

    #[test]
    fn generated_cap_is_aggregate_across_both_families() {
        // 65 ::before boxes: the 65th refuses (MAX_GENERATED = 64, shared).
        let body = "<div></div>".repeat(65);
        let h = sheet("div::before{content:\"\"}", &body);
        let t = ext(&h);
        assert!(t.truncated);
        assert!(t.diagnostics.first_reason().is_some());
        // 33 elements x (::before + ::after) = 66 > 64: aggregate cap, no
        // per-family bypass.
        let body = "<div></div>".repeat(33);
        let h = sheet("div::before{content:\"\"} div::after{content:\"\"}", &body);
        let t = ext(&h);
        assert!(t.truncated);
        // 32 elements x 2 = 64 exactly: within the bound.
        let body = "<div></div>".repeat(32);
        let h = sheet("div::before{content:\"\"} div::after{content:\"\"}", &body);
        let t = ext(&h);
        assert!(!t.truncated);
    }

    #[test]
    fn basic_hook_admits_tight_generated_rules_and_counts_skips() {
        // ITEM 2 (basic lane ::before/::after): the default entry now admits
        // generated-decoration rules through the tight hook. This test pins
        // the NEW boundary; it replaces the ext-era expectation that the
        // default entry refuses every ::before selector.
        // 1. An empty generated box on a block-container: enrolled, no skips.
        let h = sheet("div::before{content:\"\"}", "<div><p>real</p></div>");
        let d = parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT);
        assert!(!d.unsupported);
        assert_eq!(d.generated_pseudo_skips, 0);
        assert!(d.nodes.iter().any(|n| n.tag == "::before"));
        // 2. A non-allowlisted declaration is skipped and counted, never
        // refused; no rule enrolls, no box is generated.
        let d = parse_style_tree(
            b"<!doctype html><style>div::before{font-size:33px}</style><div>x</div>",
            &[],
            &Limits::DEFAULT,
        );
        assert!(!d.unsupported);
        assert_eq!(d.generated_pseudo_skips, 1);
        assert!(!d.nodes.iter().any(|n| n.tag == "::before"));
        // 3. The hamburger shape on a span: every allowlisted declaration
        // enrolls; the unknown `inset-inline-start` longhand is the one
        // counted skip.
        let h = sheet(
            "span::before{content:'';position:absolute;top:-8px;left:0;             height:2px;width:100%;background:inherit;inset-inline-start:0}",
            "<div><span>x</span></div>",
        );
        let d = parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT);
        assert!(!d.unsupported);
        assert_eq!(d.generated_pseudo_skips, 1);
        assert!(d.nodes.iter().any(|n| n.tag == "::before"));
        // 4. An originating element outside BOTH allowlists still refuses
        // (existing walker machinery, unchanged).
        let h = sheet("i::before{content:\"\"}", "<div><i>x</i></div>");
        let d = parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT);
        assert!(d.unsupported);
        assert_eq!(
            d.diagnostics.first_reason(),
            Some(RefusalReason::CssDeclaration)
        );
        // 5. A selector the hook refuses keeps the old refusal: pseudo-class
        // in the base, other pseudo-elements, junk after the suffix.
        for sheet_text in [
            "div:hover::before{content:\"\"}",
            "div::first-line{content:\"\"}",
            "div::before:hover{content:\"\"}",
        ] {
            let h = sheet(sheet_text, "<div>x</div>");
            let d = parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT);
            assert!(d.unsupported, "refuses:{sheet_text}");
            assert_eq!(
                d.diagnostics.first_reason(),
                Some(RefusalReason::CssSelector),
                "reason:{sheet_text}"
            );
        }
        // 7. EXT-grammar path (the corpus/harness entry): a span-originating
        // rule is confined to the same tight allowlist at the walker;
        // non-allowlisted declarations are skipped and counted, the rest
        // generate. Block-container ext rules keep full declarations.
        let h = sheet(
            "span::before{content:\"\";position:absolute;top:-8px;height:2px;             width:100%;background:inherit;font-size:33px}",
            "<div><span>x</span></div>",
        );
        let t = ext(&h);
        assert!(!t.unsupported);
        assert_eq!(t.generated_pseudo_skips, 1);
        assert!(t.nodes.iter().any(|n| n.tag == "::before"));
        let h = sheet("span::before{content:\"\";width:var(--w)}", "<div><span>x</span></div>");
        let t = ext(&h);
        assert!(!t.unsupported);
        assert!(t.generated_pseudo_skips >= 1);
        // A block-container ext rule is NOT confined: font-size enrolls.
        let h = sheet("div::before{content:\"\";font-size:33px}", "<div>x</div>");
        let t = ext(&h);
        assert_eq!(t.generated_pseudo_skips, 0);
        assert!(t.nodes.iter().any(|n| n.tag == "::before"));

        // 6. Deferred var() declarations never enroll through the hook:
        // skipped and counted, no refusal.
        let h = sheet(
            "div::before{content:\"\";width:var(--w)}",
            "<div>x</div>",
        );
        let d = parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT);
        assert!(!d.unsupported);
        assert!(d.generated_pseudo_skips >= 1);
    }
}
