//! Bounded, fail-closed CSS visibility subset for static article rendering.
//! No URLs, imports, pseudo selectors, font downloads or script are evaluated.
use crate::dom::{Data, Handle, Node};

const MAX_SHEETS: usize = 8;
const MAX_BYTES: usize = 64 * 1024;
const MAX_RULES: usize = 1024;
const MAX_SELECTORS: usize = 16;
const MAX_STEPS: usize = 8;
pub(crate) const EXT_FUEL: u32 = 4096;
const EXT_MAX_SIBLINGS: usize = 4096;

/// Explicit caller-asserted snapshot state for state pseudo-classes (ext
/// entries only). The engine never infers this state: a caller passes it only
/// when it can guarantee it, and the guarantee is a written caller contract.
/// History and input guarantees are INDEPENDENTLY scoped: a link-only
/// guarantee never enrolls `:hover`/`:active`, and an input-only guarantee
/// never enrolls `:link`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SnapshotState {
    pub history: Option<HistoryState>,
    pub input: Option<InputState>,
    /// Focus guarantee, scoped independently of history and input: a
    /// link-only or quiescent-only guarantee never enrolls the focus family.
    pub focus: Option<FocusState>,
    /// Target guarantee, scoped independently of history, input and focus:
    /// no other guarantee enrolls `:target`, and `:target` never enrolls or
    /// implies any of them.
    pub target: Option<TargetState>,
}
/// Target guarantees a caller can explicitly assert (HTML 7.4.6.4 state
/// contract). Without an explicit guarantee `:target` fails closed at rule
/// enrollment exactly like an unparseable selector. `:target` never writes
/// or infers focus state, and focus state never writes or infers the target
/// element; the focus/autofocus guards are exactly unchanged by this lane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetState {
    /// The caller guarantees, as a written contract: the final capture URL
    /// has NO fragment identifier; the DOM is the fresh static capture loaded
    /// under static conditions (no arbitrary URL, mid-parse, JS-modified or
    /// history-restored capture can assert this). CSS: "If the document's URL
    /// has no fragment identifier, then the document has no target elements."
    FragmentFreeCapture,
    /// The caller guarantees, as a written contract: the String is the FINAL
    /// post-navigation URL fragment identifier (raw, NOT percent-decoded);
    /// fragment selection per HTML 7.4.6.4 COMPLETED against THIS exact
    /// fully-parsed static DOM; and no subsequent DOM/history/fragment
    /// changes occurred. The engine resolves the target element once on this
    /// engine-parsed DOM per the verbatim algorithm; it does NOT implement
    /// scroll/focus side effects.
    Fragment(String),
}
/// History guarantees a caller can explicitly assert. v1: only a fresh profile
/// with no prior navigation. Without an explicit guarantee, `:link` fails
/// closed at rule enrollment exactly like an unparseable selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryState {
    /// The caller guarantees a fresh profile / no prior navigation, so every
    /// href-present hyperlink element is unvisited.
    FreshNoHistory,
}
/// Input guarantees a caller can explicitly assert. The guarantee is the
/// caller's written contract about the capture environment (backed by a fresh
/// desktop profile, no touch emulation, a fresh page, and no real Input
/// dispatch in the capture path); it is not proven by the absence of input
/// dispatch alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputState {
    /// The caller guarantees ALL of: no pointer position over any part of the
    /// viewport (covers a stationary pointer), no key held and no activation
    /// in progress on any element (covers keyboard-driven `:active`), and no
    /// active touch contact. Under this guarantee `:hover` and `:active`
    /// never match - that is the CSS semantic given the state.
    QuiescentCapture,
}
/// Focus guarantees a caller can explicitly assert. v1: only the unfocused
/// initial capture state. Without an explicit guarantee the focus family
/// (`:focus`, `:focus-visible`, `:focus-within`) fails closed at rule
/// enrollment exactly like an unparseable selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusState {
    /// The caller guarantees ALL of the following, as a written contract:
    /// (1) the capture URL has no fragment (a fragment can focus a
    /// `tabindex=-1` target on navigation - measured Chrome 157), (2) a fresh
    /// profile with no history navigation and no bfcache restore, (3) no real
    /// or synthetic input dispatch, and (4) no programmatic focus (no script
    /// in this engine). A fifth condition is ENGINE-ENFORCED, not asserted:
    /// the DOM must contain no `autofocus` attribute (autofocus focuses the
    /// element on load - measured Chrome 157); when one is present the engine
    /// voids this guarantee at enrollment and focus-family rules fail closed
    /// exactly like the no-guarantee path. Under this guarantee no element is
    /// focused at capture, so `:focus`, `:focus-visible` and `:focus-within`
    /// never match - that is the CSS semantic given the state, not a silent
    /// never-match.
    UnfocusedInitial,
}
/// Enablement pseudo-classes the extended grammar can represent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Enablement {
    /// `:enabled` / `:disabled`: pure DOM-function pseudo-classes (tag,
    /// `disabled` attribute, bounded ancestor state per the current HTML
    /// Standard algorithms - concept-fe-disabled, concept-fieldset-disabled
    /// (recursive), concept-option-disabled (nearest-ancestor scan with
    /// select/hr/datalist/option/optgroup stops), and 4.15 "nearest ancestor
    /// select is disabled"). No input/history/focus guarantee needed.
    Enabled,
    Disabled,
}
/// State pseudo-classes the extended grammar can represent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StatePseudo {
    /// `:link`: an unvisited hyperlink. Enrolled only under an explicit
    /// `HistoryState::FreshNoHistory` guarantee; matching is then the DOM
    /// predicate (href-present == unvisited under the guarantee).
    Link,
    /// `:hover` / `:active`: enrolled only under an explicit
    /// `InputState::QuiescentCapture` guarantee; under that guarantee they
    /// never match (the correct semantic, not a silent never-match).
    Hover,
    Active,
    /// The focus family: enrolled only under an explicit
    /// `FocusState::UnfocusedInitial` guarantee (engine-voided when the DOM
    /// carries `autofocus`); under that guarantee they never match (the
    /// correct semantic for an unfocused initial capture).
    Focus,
    FocusVisible,
    FocusWithin,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct Simple {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    /// `:root`: the document element (the parent is the Document node).
    root: bool,
    /// Extended grammar only: at most one state pseudo-class (`:link` in v1).
    state: Option<StatePseudo>,
    /// Extended grammar only: at most one enablement pseudo-class
    /// (`:enabled` / `:disabled`); matching is charged and fail-closed.
    enablement: Option<Enablement>,
    target: bool,
    /// Extended grammar only: type selectors compare ASCII case-insensitively
    /// (Chrome matches `linearGradient` and `lineargradient` on SVG elements) and
    /// class tokens split on ASCII whitespace only.
    ext: bool,
    /// Extended grammar only: attribute selectors and structural pseudo-classes.
    attrs: Vec<AttrSel>,
    /// Bit 1 `:first-child`, bit 2 `:last-child`, bit 4 `:only-child`.
    structural: u8,
    /// Shared occurrence budget for structural/enablement/target pseudos and
    /// bounded :not tokens. Repeated :first-child counts twice. :not tokens
    /// consume budget but their specificity is replaced by the argument's
    /// (ext_specificity subtracts their own count before adding arguments).
    structural_n: u8,
    /// Extended grammar only: `:nth-child(An+B)` arguments, conjunctive.
    nth: Vec<(i64, i64)>,
    /// Reverse all-element sibling index, bounded An+B only.
    nth_last: Vec<(i64, i64)>,
    /// Type-relative structural flags and bounded forward An+B formulas.
    /// Type is the tested element's own exact namespace/canonical local name.
    of_type: u8,
    /// Browser-compatible Selectors3: every nonzero text child disqualifies.
    empty: bool,
    nth_of_type: Vec<(i64, i64)>,
    /// Bounded negation arguments: each is exactly one compound, no nesting.
    /// Each token consumes structural budget but adds only argument specificity.
    not_args: Vec<Selector>,
    /// Same-node bounded zero-specificity filters; argument rank discarded.
    where_args: Vec<Selector>,
    /// First compound relative mode plus at most one fixed child compound.
    has_args: Vec<HasArgument>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
struct HasArgument {
    first: Selector,
    child: Option<Selector>,
}
impl HasArgument {
    fn selectors(&self) -> impl Iterator<Item=&Selector> {
        std::iter::once(&self.first).chain(self.child.iter())
    }
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
    /// Extended grammar only: the subject is a `::before`/`::after`
    /// pseudo-element of the element the steps match. Never matches an element
    /// itself.
    pub(crate) pseudo: Pseudo,
}
/// Pseudo-element subject of an extended-grammar selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Pseudo {
    None,
    Before,
    After,
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
        pseudo: Pseudo::None,
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
        state: None,
        enablement: None,
            target: false,
        ext: false,
        attrs: Vec::new(),
        structural: 0,
        structural_n: 0,
        nth: Vec::new(),
        nth_last: Vec::new(),
        of_type: 0,
        empty: false,
        nth_of_type: Vec::new(),
        not_args: Vec::new(),
        where_args: Vec::new(),
        has_args: Vec::new(),
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
/// Parses the bounded `:nth-child(An+B)` argument and returns (A, B) and
/// the position after the closing paren. Grammar (grounded against Chrome
/// 157 `CSS.supports("selector(...)")` on each token shape): `odd`/`even`
/// (ASCII case-insensitive), a signed integer with ATTACHED sign (`+5` ok,
/// `- 3` refused), or `[+-]? [0-9]* n [ ws* [+-] ws* [0-9]+ ]?` with `n`
/// ASCII case-insensitive. A doubled sign (`2n + -1`) is refused - Chrome
/// refuses it too (probed FALSE). Comments (`2n/**/+1`, probed TRUE) and
/// escaped n (`\6e `, probed TRUE) ARE valid CSS but stay refused here as
/// an unsupported SUBSET (a grammar bound, not an invalid-CSS claim). Bounds: argument <= 32 chars, |A| <= 1024,
/// |B| <= 65535. Every deviation refuses the selector.
fn nth_child_arg(s: &str, at: usize) -> Option<((i64, i64), usize)> {
    let b = s.as_bytes();
    if b.get(at) != Some(&b'(') {
        return None;
    }
    let close = b[at + 1..]
        .iter()
        .position(|&c| c == b')' || c == b'(')
        .map(|n| at + 1 + n)?;
    if b[close] != b')' {
        return None;
    }
    let arg = &s[at + 1..close];
    if arg.len() > 32 {
        return None;
    }
    Some((an_plus_b(arg)?, close + 1))
}
fn an_plus_b(arg: &str) -> Option<(i64, i64)> {
    let ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\u{c}' | '\r');
    let t = arg.trim_matches(ws);
    if t.is_empty() || t.contains("/*") || t.contains('*') || t.contains('\\') {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    if lower == "odd" {
        return Some((2, 1));
    }
    if lower == "even" {
        return Some((2, 0));
    }
    let signed_int = |v: &str| -> Option<i64> {
        // An integer with an ATTACHED optional sign (`+5`, `-3`; `- 3` no).
        let (sign, digits) = match v.as_bytes().first() {
            Some(b'+') => (1_i64, &v[1..]),
            Some(b'-') => (-1_i64, &v[1..]),
            _ => (1_i64, v),
        };
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        Some(sign * digits.parse::<i64>().ok()?)
    };
    let n_at = lower.find('n');
    if n_at.is_none() {
        let b_val = signed_int(t)?;
        return (b_val.abs() <= 65535).then_some((0, b_val));
    }
    let n_at = n_at.unwrap();
    // Exactly one `n`, and no other alphabetic characters anywhere.
    if lower[n_at + 1..].contains('n')
        || t.bytes().any(|c| c.is_ascii_alphabetic() && c.to_ascii_lowercase() != b'n')
    {
        return None;
    }
    let coeff_raw = &t[..n_at];
    let coeff = coeff_raw.trim_matches(ws);
    // The sign/digits must be ATTACHED to `n` (one `n`-dimension token):
    // `- n + 2` and `2 n` refuse (both probed FALSE on Chrome 157: the
    // sign/digits and the `n` must form ONE dimension token).
    if !coeff.is_empty() && coeff_raw != coeff {
        return None;
    }
    let a_val = match coeff {
        "" | "+" => 1,
        "-" => -1,
        other => signed_int(other)?,
    };
    let rest = t[n_at + 1..].trim_matches(ws);
    let b_val = if rest.is_empty() {
        0
    } else {
        // One sign, optional whitespace, unsigned digits (`2n+ 1` ok,
        // `2n + -1` and `2n++1` refused).
        let rb = rest.as_bytes();
        let (sign, after) = match rb.first() {
            Some(b'+') => (1_i64, &rest[1..]),
            Some(b'-') => (-1_i64, &rest[1..]),
            _ => return None,
        };
        let digits = after.trim_matches(ws);
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        sign * digits.parse::<i64>().ok()?
    };
    (a_val.abs() <= 1024 && b_val.abs() <= 65535).then_some((a_val, b_val))
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
fn compound_ext(s: &str, at: usize) -> Option<(Simple, usize)> {
    compound_ext_in(s, at, true, true, true)
}
fn compound_ext_in(s: &str, mut at: usize, allow_not: bool, allow_where: bool, allow_has: bool) -> Option<(Simple, usize)> {
    let b = s.as_bytes();
    let mut simple = Simple {
        tag: None,
        id: None,
        classes: Vec::new(),
        root: false,
        state: None,
        enablement: None,
            target: false,
        ext: true,
        attrs: Vec::new(),
        structural: 0,
        structural_n: 0,
        nth: Vec::new(),
        nth_last: Vec::new(),
        of_type: 0,
        empty: false,
        nth_of_type: Vec::new(),
        not_args: Vec::new(),
        where_args: Vec::new(),
        has_args: Vec::new(),
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
                let lowered = token.to_ascii_lowercase();
                if matches!(lowered.as_str(), "not" | "where" | "has") {
                    let is_not = lowered == "not";
                    let is_where = lowered == "where";
                    let is_has = lowered == "has";
                    if (is_not && !allow_not) || (is_where && !allow_where) || (is_has && !allow_has)
                        || simple.structural_n >= 8 || b.get(next) != Some(&b'(') {
                        return None;
                    }
                    // Parse directly with the same compound grammar. Only the
                    // enclosing ')' may follow: no list, complex selector,
                    // pseudo-element or nested negation can be admitted.
                    let mut arg_start = skip_ws(b, next + 1);
                    // The stored relative mode is explicit even for a single
                    // compound. Existing child mode remains a distinct path.
                    let arg_relation = if is_has && b.get(arg_start) == Some(&b'>') {
                        arg_start = skip_ws(b, arg_start + 1);
                        Relation::Child
                    } else { Relation::Descendant };
                    let (arg, end) = compound_ext_in(s, arg_start,
                        allow_not && !is_not, allow_where && !is_where, allow_has && !is_has)?;
                    let mut end = skip_ws(b, end);
                    let child = if is_has && matches!(arg_relation, Relation::Child) && b.get(end) == Some(&b'>') {
                        let (tail, after) = compound_ext_in(s, skip_ws(b, end + 1),
                            allow_not, allow_where, false)?;
                        end = skip_ws(b, after);
                        let specificity = tail.ext_specificity();
                        Some(Selector { steps: vec![(tail, Relation::Child)], specificity,
                            ext: true, pseudo: Pseudo::None })
                    } else { None };
                    if b.get(end) != Some(&b')') { return None; }
                    let specificity = arg.ext_specificity();
                    let parsed = Selector {
                        steps: vec![(arg, arg_relation)],
                        specificity, ext: true, pseudo: Pseudo::None,
                    };
                    if is_not { simple.not_args.push(parsed); }
                    else if is_where { simple.where_args.push(parsed); }
                    else { simple.has_args.push(HasArgument { first: parsed, child }); }
                    simple.structural_n += 1;
                    at = end + 1;
                    // Do NOT take the class-level fall-through increment:
                    // Not replaces rank with its argument; where discards it.
                    continue;
                }
                if matches!(lowered.as_str(), "nth-child" | "nth-of-type" | "nth-last-child") {
                    // Functional structural pseudo-class: bounded An+B only.
                    let (an_b, after) = nth_child_arg(s, next)?;
                    if simple.structural_n >= 8 {
                        return None;
                    }
                    simple.structural_n += 1;
                    if lowered == "nth-last-child" { simple.nth_last.push(an_b); }
                    else if lowered == "nth-of-type" { simple.nth_of_type.push(an_b); }
                    else { simple.nth.push(an_b); }
                    at = after;
                    continue;
                }
                match lowered.as_str() {
                    "root" if !simple.root => simple.root = true,
                    // State pseudo-classes: at most one per compound. A
                    // repetition (`a:link:link`, `a:hover:hover`) falls
                    // through to the refusal arm: our bounded grammar refuses
                    // the repetition as a grammar bound, NOT a claim that the
                    // CSS is invalid.
                    "link" if simple.state.is_none() => {
                        simple.state = Some(StatePseudo::Link);
                    }
                    "hover" if simple.state.is_none() => {
                        simple.state = Some(StatePseudo::Hover);
                    }
                    "active" if simple.state.is_none() => {
                        simple.state = Some(StatePseudo::Active);
                    }
                    "focus" if simple.state.is_none() => {
                        simple.state = Some(StatePseudo::Focus);
                    }
                    "focus-visible" if simple.state.is_none() => {
                        simple.state = Some(StatePseudo::FocusVisible);
                    }
                    "focus-within" if simple.state.is_none() => {
                        simple.state = Some(StatePseudo::FocusWithin);
                    }
                    // Enablement pseudo-classes: at most one per compound; a
                    // repetition refuses as a grammar bound (not an
                    // invalid-CSS claim), exactly like the state family.
                    "enabled" if simple.enablement.is_none() => {
                        simple.enablement = Some(Enablement::Enabled);
                    }
                    "disabled" if simple.enablement.is_none() => {
                        simple.enablement = Some(Enablement::Disabled);
                    }
                    // `:target`: bare form only, at most one per compound
                    // (repetition falls to `_` and refuses). Caller-state
                    // dependent: resolved under TargetState at match time.
                    "target" if !simple.target => {
                        simple.target = true;
                    }
                    "empty" => simple.empty = true,
                    "first-child" => simple.structural |= 1,
                    "last-child" => simple.structural |= 2,
                    "only-child" => simple.structural |= 4,
                    "first-of-type" => simple.of_type |= 1,
                    "last-of-type" => simple.of_type |= 2,
                    "only-of-type" => simple.of_type |= 4,
                    _ => return None,
                }
                if !matches!(
                    lowered.as_str(),
                    "root"
                        | "link"
                        | "hover"
                        | "active"
                        | "focus"
                        | "focus-visible"
                        | "focus-within"
                ) {
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
/// Opt-in grammar: bounded compounds of type/`*`/`#id`/`.class`/
/// `[attr op value]`, `:root`, browser-compatible Selectors3 `:empty`
/// (all nonzero text affects emptiness), first/last/only-child, bounded
/// `:nth-child(An+B)`/`:nth-last-child(An+B)`, first/last/only-of-type, `:nth-of-type(An+B)`,
/// `:enabled`/`:disabled`, `:target` and the state family
/// `:link`/`:hover`/`:active`/`:focus`/`:focus-visible`/`:focus-within`,
/// joined by descendant, `>`, `+` and `~`. Bounded `:where(S)` is a nonempty
/// single-compound zero-specificity filter, not general forgiving-list support.
/// `:has(S)` and `:has(> S)` accept one compound, not general has.
/// Not/where/has may cross once each but none nests itself on any path.
/// Bounded `:not(S)` accepts exactly
/// one compound of those features; no nested negation, lists, complex
/// arguments or argument pseudo-elements. Only exact trailing `::before`/
/// `::after` suffixes on a non-bare, state-free originating selector are
/// accepted; other pseudo-elements and single-colon forms are refused.
/// Other pseudo-classes/functions (including `:visited`, `:nth-last-of-type`), the Selectors-4 `of S` form, namespaces, escapes and
/// attribute case flags remain refused. State/target dependencies anywhere,
/// including inside negation, enroll only under an explicit caller
/// `SnapshotState` covering every family (see `matched_rules`); without it
/// the rule fails closed exactly like an unparseable selector.
pub(crate) fn selector_ext(input: &str) -> Option<Selector> {
    if input.is_empty() || input.len() > 256 {
        return None;
    }
    // A pseudo-element is only legal as the LAST part of the selector, and only
    // an exact trailing `::before` or `::after` (the single-colon legacy forms
    // and every other pseudo-element stay refused). A bare pseudo-element
    // selector is not accepted.
    let trimmed = input.trim_end();
    for (suffix, pseudo) in [("::after", Pseudo::After), ("::before", Pseudo::Before)] {
        let n = suffix.len();
        if trimmed.len() >= n && trimmed.is_char_boundary(trimmed.len() - n) {
            let (head, tail) = trimmed.split_at(trimmed.len() - n);
            if tail.eq_ignore_ascii_case(suffix) {
                let head = head.trim_end_matches(|c: char| c.is_ascii_whitespace());
                if head.is_empty() || head.len() != trimmed[..trimmed.len() - n].len() {
                    // `a ::after` means `a *::after`; refuse rather than guess.
                    return None;
                }
                let mut inner = selector_ext(head)?;
                if inner.pseudo != Pseudo::None || inner.has_state() {
                    // A second pseudo-element or any state pseudo-class on a
                    // pseudo-element selector is out of scope (a grammar bound,
                    // not an invalid-CSS claim).
                    return None;
                }
                inner.pseudo = pseudo;
                inner.specificity.2 += 1;
                return Some(inner);
            }
        }
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
    let specificity =
        steps
            .iter()
            .fold((0_u16, 0_u16, 0_u16), |(ids, classes, types), (part, _)| {
                let (a, b, c) = part.ext_specificity();
                (ids + a, classes + b, types + c)
            });
    steps.reverse();
    Some(Selector {
        steps,
        specificity,
        ext: true,
        pseudo: Pseudo::None,
    })
}
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
/// Ext-entry match prefilter bucket key: a provable conjunct of the
/// selector's RIGHTMOST compound, normalized EXACTLY as the matcher compares
/// (attribute names always ASCII-insensitive per `AttrSel::matches`, ext tag
/// names ASCII-insensitive per `Simple::matches`, class tokens and id values
/// exact). `Universal` is the conservative fallback for anything not proven
/// (root-only, bare state/structural, `*`): a candidate for every element,
/// never a rejection.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum PrefilterKey {
    Id(String),
    Class(String),
    AttrName(String),
    Tag(String),
    Universal,
}
impl Selector {
    /// Extraction order id > class > attr > tag > universal over the rightmost
    /// compound (conjunctive: any one conjunct is a sound filter). Only used
    /// for ext-entry parses, where every sheet selector is ext grammar, so the
    /// ext folding above is the only normalization that ever runs.
    pub(crate) fn prefilter_key(&self) -> PrefilterKey {
        let Some((simple, _)) = self.steps.first() else {
            return PrefilterKey::Universal;
        };
        if let Some(id) = &simple.id {
            return PrefilterKey::Id(id.clone());
        }
        if let Some(class) = simple.classes.first() {
            return PrefilterKey::Class(class.clone());
        }
        if let Some(attr) = simple.attrs.first() {
            return PrefilterKey::AttrName(attr.name.to_ascii_lowercase());
        }
        if let Some(tag) = &simple.tag {
            return PrefilterKey::Tag(tag.to_ascii_lowercase());
        }
        PrefilterKey::Universal
    }
}
/// Why a bounded match refused. `Fuel`: the per-match work budget ran out
/// (selector steps AND every visited sibling in a scan each cost one unit -
/// the caller treats the tree as truncated). `OversizeSiblings`:
/// the element's sibling list exceeds `EXT_MAX_SIBLINGS` (a rule-scoped
/// guard - only this rule refuses for this element; every other rule and
/// element is unaffected, and the tree is NOT truncated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MatchRefusal {
    Fuel,
    OversizeSiblings,
}
impl Selector {
    /// `None` means the bounded matcher refused (out of fuel or an oversize
    /// sibling list): the caller must refuse, never treat it as a mismatch.
    /// Callers that must distinguish the refusal cause (rule-scoped versus
    /// tree-truncating) use `matches_checked_scoped` instead.
    pub(crate) fn matches_checked(&self, nodes: &[Node], handle: Handle) -> Option<bool> {
        self.matches_checked_scoped(nodes, handle).unwrap_or(None)
    }
    pub(crate) fn matches_checked_scoped(
        &self,
        nodes: &[Node],
        handle: Handle,
    ) -> Result<Option<bool>, MatchRefusal> {
        let mut fuel = EXT_FUEL;
        self.matches_checked_scoped_budget(nodes, handle, &mut fuel, None, None)
    }
    /// Budgeted form: the caller supplies the per-match fuel (EXT_FUEL) and
    /// reads back what the match actually consumed (steps + every visited
    /// sibling in a scan), charging its own global work budget. `position`
    /// is the walk's O(1) knowledge of the RIGHTMOST element's place in its
    /// parent's child list: (raw child index, 1-based index among ELEMENT
    /// siblings). Supplying it makes a rightmost `:nth-child` test and the
    /// first sibling-combinator transition O(1) - no rescan per rule.
    pub(crate) fn matches_checked_scoped_budget(
        &self,
        nodes: &[Node],
        handle: Handle,
        fuel: &mut u32,
        position: Option<(u32, u32)>,
        target: Option<Option<Handle>>,
    ) -> Result<Option<bool>, MatchRefusal> {
        if self.pseudo != Pseudo::None {
            return Ok(Some(false));
        }
        self.matches_originating_checked_scoped_budget(nodes, handle, fuel, position, target)
    }
    /// `:target`-aware checked match: resolves the target element under the
    /// caller-asserted TargetState (charged, verbatim 7.4.6.4 algorithm) and
    /// matches with it. No assertion is represented by NOT calling this: the
    /// plain checked entries carry no resolution and `:target` answers
    /// UNRESOLVED (None), never a guess.
    /// NOTE: this entry re-resolves the target on EVERY call (two charged
    /// budgets per call). Current consumers are the tests and the
    /// once-per-tree sheet walk only; a future per-selector consumer would
    /// multiply resolution work and must be budget-reviewed first.
    pub(crate) fn matches_checked_target(
        &self,
        nodes: &[Node],
        handle: Handle,
        target_state: &TargetState,
    ) -> Option<bool> {
        let mut fuel = EXT_FUEL;
        let resolved = resolve_target_element(nodes, target_state, &mut fuel).ok()?;
        let mut fuel = EXT_FUEL;
        self.matches_checked_scoped_budget(nodes, handle, &mut fuel, None, Some(resolved))
            .unwrap_or(None)
    }
    /// Any state pseudo-class anywhere in the selector (ext grammar only).
    pub(crate) fn has_state(&self) -> bool {
        self.steps
            .iter()
            .any(|(simple, _)| simple.state.is_some() || simple.target
                || simple.not_args.iter().chain(&simple.where_args).chain(simple.has_args.iter().flat_map(HasArgument::selectors)).any(Selector::has_state))
    }
    fn has_context_wrapper(&self) -> bool {
        self.steps.iter().any(|(s,_)| !s.where_args.is_empty() || !s.has_args.is_empty()
            || s.not_args.iter().any(Selector::has_context_wrapper))
    }
    // Raw checked helpers have no SnapshotState. A resolved target is their
    // only explicit context; never let an ordinary false predicate bypass
    // missing state in a where/has-bearing selector. Production preflights the
    // actual same SnapshotState before matching.
    fn raw_dependencies_satisfied(&self, target: Option<Option<Handle>>) -> bool {
        self.steps.iter().all(|(s,_)| s.state.is_none() && (!s.target || target.is_some())
            && s.not_args.iter().chain(&s.where_args).chain(s.has_args.iter().flat_map(HasArgument::selectors)).all(|a|a.raw_dependencies_satisfied(target)))
    }
    /// Every state pseudo-class in the selector is covered by the caller's
    /// explicit guarantee. History and input are scoped independently: a
    /// link-only guarantee does not enroll `:hover`/`:active`, and an
    /// input-only guarantee does not enroll `:link`.
    pub(crate) fn state_satisfied_by(&self, snapshot: Option<&SnapshotState>) -> bool {
        self.steps.iter().all(|(simple, _)| {
            if !simple.not_args.iter().chain(&simple.where_args).chain(simple.has_args.iter().flat_map(HasArgument::selectors)).all(|arg| arg.state_satisfied_by(snapshot)) {
                return false;
            }
            if simple.target
                && !matches!(
                    snapshot,
                    Some(SnapshotState {
                        target: Some(_),
                        ..
                    })
                )
            {
                return false;
            }
            match simple.state {
            None => true,
            Some(StatePseudo::Link) => matches!(
                snapshot,
                Some(SnapshotState {
                    history: Some(HistoryState::FreshNoHistory),
                    ..
                })
            ),
            Some(StatePseudo::Hover | StatePseudo::Active) => matches!(
                snapshot,
                Some(SnapshotState {
                    input: Some(InputState::QuiescentCapture),
                    ..
                })
            ),
            Some(StatePseudo::Focus | StatePseudo::FocusVisible | StatePseudo::FocusWithin) => {
                matches!(
                    snapshot,
                    Some(SnapshotState {
                        focus: Some(FocusState::UnfocusedInitial),
                        ..
                    })
                )
            }
            }
        })
    }
    /// For a `::after` selector: does the ORIGINATING element match the steps?
    pub(crate) fn matches_originating_checked(
        &self,
        nodes: &[Node],
        handle: Handle,
    ) -> Option<bool> {
        self.matches_originating_checked_scoped(nodes, handle)
            .unwrap_or(None)
    }
    pub(crate) fn matches_originating_checked_scoped(
        &self,
        nodes: &[Node],
        handle: Handle,
    ) -> Result<Option<bool>, MatchRefusal> {
        let mut fuel = EXT_FUEL;
        self.matches_originating_checked_scoped_budget(nodes, handle, &mut fuel, None, None)
    }
    pub(crate) fn matches_originating_checked_scoped_budget(
        &self,
        nodes: &[Node],
        handle: Handle,
        fuel: &mut u32,
        position: Option<(u32, u32)>,
        target: Option<Option<Handle>>,
    ) -> Result<Option<bool>, MatchRefusal> {
        if !self.ext {
            return Ok(Some(self.matches(nodes, handle)));
        }
        if self.has_context_wrapper() && !self.raw_dependencies_satisfied(target) { return Ok(None); }
        self.ext_match(nodes, handle, 0, fuel, position, target, false, false)
    }
    /// Production walk entries use the same snapshot as enrollment and the
    /// once-per-walk resolved target. Arguments never resolve target again.
    pub(crate) fn matches_checked_scoped_budget_with_snapshot(
        &self, nodes: &[Node], handle: Handle, fuel: &mut u32,
        position: Option<(u32, u32)>, target: Option<Option<Handle>>,
        snapshot: Option<&SnapshotState>,
    ) -> Result<Option<bool>, MatchRefusal> {
        if self.pseudo != Pseudo::None { return Ok(Some(false)); }
        self.matches_originating_checked_scoped_budget_with_snapshot(
            nodes, handle, fuel, position, target, snapshot)
    }
    pub(crate) fn matches_originating_checked_scoped_budget_with_snapshot(
        &self, nodes: &[Node], handle: Handle, fuel: &mut u32,
        position: Option<(u32, u32)>, target: Option<Option<Handle>>,
        snapshot: Option<&SnapshotState>,
    ) -> Result<Option<bool>, MatchRefusal> {
        if !self.ext { return Ok(Some(self.matches(nodes, handle))); }
        if !self.state_satisfied_by(snapshot) { return Ok(None); }
        self.ext_match(nodes, handle, 0, fuel, position, target, true, false)
    }
    /// Search direct children only. Errors propagate immediately; otherwise
    /// any true wins, then any unknown, then all false. Charge raw visits
    /// before lookup/parent validation, including failed membership searches.
    fn matches_direct_child_charged(&self, nodes: &[Node], anchor: Handle,
        fuel: &mut u32, target: Option<Option<Handle>>, state_enrolled: bool)
        -> Result<Option<bool>, MatchRefusal> {
        let Some(Node {data: Data::Element {..}, children, ..}) = nodes.get(anchor)
            else { return Ok(None); };
        if children.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
        let mut unknown = false;
        for &child in children {
            not_charge(fuel)?;
            let Some(candidate) = nodes.get(child) else { unknown = true; continue; };
            if candidate.parent != Some(anchor) { unknown = true; continue; }
            if !matches!(candidate.data, Data::Element {..}) { continue; }
            // Anchor position describes the wrong node. Candidate predicates
            // must compute their own typed/forward/reverse sibling positions.
            match self.ext_match(nodes, child, 0, fuel, None, target, state_enrolled, true)? {
                Some(true) => return Ok(Some(true)),
                Some(false) => {},
                None => unknown = true,
            }
        }
        Ok(if unknown {None} else {Some(false)})
    }
    /// Stackless DFS over validated element edges. Ascending rescans the
    /// borrowed parent slice, deliberately O(n²) worst case, with every visit
    /// charged. No DOM recursion, allocated stack, or max_depth assumption.
    fn matches_descendant_charged(&self, nodes: &[Node], anchor: Handle,
        fuel: &mut u32, target: Option<Option<Handle>>, state_enrolled: bool)
        -> Result<Option<bool>, MatchRefusal> {
        if !matches!(nodes.get(anchor).map(|n| &n.data), Some(Data::Element {..})) {
            return Ok(None);
        }
        let mut container = anchor;
        let mut next = 0;
        let mut unknown = false;
        loop {
            let Some(current) = nodes.get(container) else { return Ok(None); };
            if current.children.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
            if let Some(&child) = current.children.get(next) {
                not_charge(fuel)?;
                next += 1;
                let Some(candidate) = nodes.get(child) else { unknown = true; continue; };
                if child == anchor || candidate.parent != Some(container) { unknown = true; continue; }
                if !matches!(candidate.data, Data::Element {..}) { continue; }
                match self.ext_match(nodes, child, 0, fuel, None, target, state_enrolled, true)? {
                    Some(true) => return Ok(Some(true)),
                    Some(false) => {},
                    None => unknown = true,
                }
                container = child;
                next = 0;
            } else {
                // The boundary check precedes parent access: never search an
                // anchor's siblings or ancestors, even when it has a parent.
                if container == anchor { break; }
                let Some((parent, resume)) = Self::descendant_resume(nodes, container, fuel)?
                    else { return Ok(None); };
                container = parent;
                next = resume;
            }
        }
        Ok(if unknown { None } else { Some(false) })
    }
    /// Only called after the anchor boundary check. Kept separate so even
    /// inconsistent-parent and oversized-rescan refusal branches are pinned
    /// independently, although immutable validated entry makes them unreachable
    /// in a well-formed traversal.
    fn descendant_resume(nodes: &[Node], container: Handle, fuel: &mut u32)
        -> Result<Option<(Handle, usize)>, MatchRefusal> {
        not_charge(fuel)?;
        let Some(parent) = nodes.get(container).and_then(|n| n.parent) else { return Ok(None); };
        let Some(parent_node) = nodes.get(parent) else { return Ok(None); };
        if !matches!(parent_node.data, Data::Element {..}) { return Ok(None); }
        if parent_node.children.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
        for (i, &raw) in parent_node.children.iter().enumerate() {
            not_charge(fuel)?;
            if raw == container { return Ok(Some((parent, i + 1))); }
        }
        Ok(None)
    }
    /// Exactly anchor > first > tail. No generic complex matcher may
    /// ascend past the anchor. S unknown conservatively skips the T branch.
    fn matches_two_child_charged(&self, tail: &Selector, nodes: &[Node], anchor: Handle,
        fuel: &mut u32, target: Option<Option<Handle>>, state_enrolled: bool)
        -> Result<Option<bool>, MatchRefusal> {
        let Some(Node { data: Data::Element {..}, children, .. }) = nodes.get(anchor)
            else { return Ok(None); };
        if children.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
        let mut unknown = false;
        for &middle in children {
            not_charge(fuel)?;
            let Some(first) = nodes.get(middle) else { unknown = true; continue; };
            if middle == anchor || first.parent != Some(anchor) { unknown = true; continue; }
            if !matches!(first.data, Data::Element {..}) { continue; }
            match self.ext_match(nodes, middle, 0, fuel, None, target, state_enrolled, true)? {
                Some(true) => {},
                Some(false) => continue,
                None => { unknown = true; continue; },
            }
            if first.children.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
            for &leaf in &first.children {
                not_charge(fuel)?;
                let Some(last) = nodes.get(leaf) else { unknown = true; continue; };
                if leaf == anchor || leaf == middle || last.parent != Some(middle) {
                    unknown = true; continue;
                }
                if !matches!(last.data, Data::Element {..}) { continue; }
                match tail.ext_match(nodes, leaf, 0, fuel, None, target, state_enrolled, true)? {
                    Some(true) => return Ok(Some(true)),
                    Some(false) => {},
                    None => unknown = true,
                }
            }
        }
        Ok(if unknown { None } else { Some(false) })
    }
    fn ext_match(
        &self,
        nodes: &[Node],
        node: Handle,
        index: usize,
        fuel: &mut u32,
        position: Option<(u32, u32)>,
        target: Option<Option<Handle>>,
        state_enrolled: bool,
        charged_argument: bool,
    ) -> Result<Option<bool>, MatchRefusal> {
        if *fuel == 0 {
            return Err(MatchRefusal::Fuel);
        }
        *fuel -= 1;
        let (simple, relation) = &self.steps[index];
        // `:nth-child` on this step: the walk supplies the element's index
        // O(1) for the rightmost step (one fuel unit per test, zero
        // traversal); any other step scans with every visited sibling
        // charged to fuel. The oversize gate is O(1) on the borrowed child
        // slice (no Vec is ever built) and refuses BEFORE any traversal.
        if !simple.nth.is_empty() {
            let nth_ix = match position.filter(|_| index == 0) {
                Some((_, element_index)) => {
                    if let Some(sibs) = element_siblings(nodes, node) {
                        if sibs.len() > EXT_MAX_SIBLINGS {
                            return Err(MatchRefusal::OversizeSiblings);
                        }
                    }
                    Some(i64::from(element_index))
                }
                None => nth_index_charged(nodes, node, fuel)?,
            };
            let Some(nth_ix) = nth_ix else {
                return Ok(Some(false));
            };
            if !simple.nth.iter().all(|&an_b| nth_formula(nth_ix, an_b)) {
                return Ok(Some(false));
            }
        }
        if simple.empty {
            let Some(empty) = empty_children_charged(nodes, node, fuel)?
                else { return Ok(None); };
            if !empty { return Ok(Some(false)); }
        }
        if !simple.nth_last.is_empty() {
            // Forward walk position lacks a total, so cannot supply reverse
            // index. Repeated formulas share one borrowed charged census.
            let Some(reverse_index) = nth_last_index_charged(nodes, node, fuel)?
                else { return Ok(None); };
            if !simple.nth_last.iter().all(|&ab| nth_formula(reverse_index, ab)) {
                return Ok(Some(false));
            }
        }
        if simple.of_type != 0 || !simple.nth_of_type.is_empty() {
            // The walk's all-element position is NOT a typed index. One
            // borrowed, charged census supplies this compound's typed index
            // and (only when needed) total, shared by repeated formulas.
            let need_total = simple.of_type & (2 | 4) != 0;
            let Some((typed_index, total)) = typed_position_charged(nodes, node, need_total, fuel)?
                else { return Ok(None); }; // inconsistent membership, never index zero
            if (simple.of_type & 1 != 0 && typed_index != 1)
                || (simple.of_type & 2 != 0 && typed_index != total)
                || (simple.of_type & 4 != 0 && total != 1)
                || !simple.nth_of_type.iter().all(|&ab| nth_formula(typed_index, ab)) {
                    return Ok(Some(false));
                }
        }
        // `:enabled` / `:disabled` on this step: the spec algorithms walk
        // ancestors (fieldset propagation + first-legend exemption, recursive
        // fieldset disabledness, option's nearest-ancestor stop scan, the
        // nearest-select composition). Every visited ancestor/child is
        // charged to fuel; exhaustion refuses fail-closed, never guesses.
        if let Some(enablement) = simple.enablement {
            match element_disabled(nodes, node, fuel)? {
                // Not a disable-able element: matches NEITHER pseudo.
                None => return Ok(Some(false)),
                Some(disabled) => {
                    let ok = match enablement {
                        Enablement::Enabled => !disabled,
                        Enablement::Disabled => disabled,
                    };
                    if !ok {
                        return Ok(Some(false));
                    }
                }
            }
        }
        // `:target` on this step: resolved caller-asserted target element.
        // None = no TargetState assertion: refuse to answer (UNRESOLVED),
        // never guessed. Some(resolved): exact element identity, O(1).
        if simple.target {
            match target {
                None => return Ok(None),
                Some(resolved) => {
                    if resolved != Some(node) {
                        return Ok(Some(false));
                    }
                }
            }
        }
        let matches = if charged_argument {
            simple.matches_not_charged(nodes, node, fuel)?
        } else { simple.matches_inner(nodes, node, false) };
        if !matches {
            return Ok(Some(false));
        }
        for arg in &simple.has_args {
            let result = if let Some(tail) = &arg.child {
                arg.first.matches_two_child_charged(tail, nodes, node, fuel, target, state_enrolled)
            } else { match arg.first.steps[0].1 {
                Relation::Child => arg.first.matches_direct_child_charged(nodes, node, fuel, target, state_enrolled),
                Relation::Descendant => arg.first.matches_descendant_charged(nodes, node, fuel, target, state_enrolled),
                _ => return Ok(None),
            }};
            match result? {
                Some(true) => {},
                Some(false) => return Ok(Some(false)),
                None => return Ok(None),
            }
        }
        for arg in &simple.where_args {
            match arg.ext_match(nodes, node, 0, fuel,
                position.filter(|_| index == 0), target, state_enrolled, true)? {
                Some(true) => {},
                Some(false) => return Ok(Some(false)),
                None => return Ok(None),
            }
        }
        for arg in &simple.not_args {
            // Missing assertions remain unknown, never inverted. The public
            // walk verifies them against the SAME snapshot as enrollment.
            // A target-only argument can use its explicit resolved context.
            if !state_enrolled && arg.steps.iter().any(|(p, _)| p.state.is_some()) {
                return Ok(None);
            }
            match arg.ext_match(nodes, node, 0, fuel,
                position.filter(|_| index == 0), target, state_enrolled, true)? {
                Some(true) => return Ok(Some(false)),
                Some(false) => {},
                None => return Ok(None),
            }
        }
        if index + 1 == self.steps.len() {
            return Ok(Some(true));
        }
        match relation {
            Relation::Child => match nodes[node].parent {
                Some(parent) => self.ext_match(nodes, parent, index + 1, fuel, None, target, state_enrolled, charged_argument),
                None => Ok(Some(false)),
            },
            Relation::Descendant => {
                let mut ancestor = nodes[node].parent;
                let mut unknown = false;
                while let Some(candidate) = ancestor {
                    match self.ext_match(nodes, candidate, index + 1, fuel, None, target, state_enrolled, charged_argument)? {
                        Some(true) => return Ok(Some(true)),
                        None => unknown = true,
                        Some(false) => {},
                    }
                    ancestor = nodes[candidate].parent;
                }
                Ok(if unknown { None } else { Some(false) })
            }
            Relation::NextSibling | Relation::LaterSibling => {
                let Some(siblings) = element_siblings(nodes, node) else {
                    return Ok(Some(false));
                };
                if siblings.len() > EXT_MAX_SIBLINGS {
                    return Err(MatchRefusal::OversizeSiblings);
                }
                // The walk knows the rightmost element's raw child index
                // O(1); any deeper step scans, with every visited sibling
                // charged to fuel.
                let at = match position.filter(|_| index == 0) {
                    Some((raw_index, _)) => raw_index as usize,
                    None => {
                        let mut found = None;
                        for (at, &n) in siblings.iter().enumerate() {
                            if *fuel == 0 {
                                return Err(MatchRefusal::Fuel);
                            }
                            *fuel -= 1;
                            if n == node {
                                found = Some(at);
                                break;
                            }
                        }
                        found.ok_or(MatchRefusal::Fuel)?
                    }
                };
                let mut unknown = false;
                for &earlier in siblings[..at].iter().rev() {
                    if *fuel == 0 {
                        return Err(MatchRefusal::Fuel);
                    }
                    *fuel -= 1;
                    if !is_element(nodes, earlier) {
                        continue;
                    }
                    match self.ext_match(nodes, earlier, index + 1, fuel, None, target, state_enrolled, charged_argument)? {
                        Some(true) => return Ok(Some(true)),
                        None => unknown = true,
                        Some(false) => {},
                    }
                    if *relation == Relation::NextSibling {
                        break;
                    }
                }
                Ok(if unknown { None } else { Some(false) })
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
/// `:link` under an enrolled `FreshNoHistory` guarantee: an HTML-namespace
/// `a` or `area` element that HAS an `href` attribute (any value, including
/// empty). Under the guarantee href-present == unvisited, so this is genuine
/// `:link`, not `:any-link`; matching never consults or infers history.
/// SVG `a` is narrowed OUT in v1: the DOM sink flattens `xlink:href` to the
/// local name `href` (dom.rs `keep_attrs`, `attr_namespace_discarded`), so an
/// SVG `href`'s provenance is ambiguous and no honest per-node match exists.
/// Narrowing an unsupported case, not a bogus localname guess.
fn link_href(nodes: &[Node], handle: Handle) -> bool {
    let Data::Element { name, attrs, .. } = &nodes[handle].data else {
        return false;
    };
    if name.ns != html5ever::ns!(html) || !matches!(&*name.local, "a" | "area") {
        return false;
    }
    attrs
        .iter()
        .any(|(key, _)| (&**key).eq_ignore_ascii_case("href"))
}
/// Engine-observable focus-guarantee gate: does any element in the document
/// carry an `autofocus` attribute? Autofocus focuses the element on load
/// (measured Chrome 157: `:focus`=1, `:focus-visible`=1, `:focus-within`=3),
/// so its presence voids a caller's `FocusState::UnfocusedInitial` contract.
pub(crate) fn has_autofocus(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match &node.data {
        Data::Element { attrs, .. } => attrs
            .iter()
            .any(|(key, _)| (&**key).eq_ignore_ascii_case("autofocus")),
        _ => false,
    })
}
/// Negation arguments deliberately charge actual byte/iteration work, unlike
/// the legacy bounded compares in matches_inner. No blanket multipliers and
/// no allocation: borrowed byte comparisons and token ranges only. Each
/// loop charges BEFORE the operation; predicate failure short-circuits.
fn not_charge(fuel: &mut u32) -> Result<(), MatchRefusal> {
    if *fuel == 0 { return Err(MatchRefusal::Fuel); }
    *fuel -= 1;
    Ok(())
}
fn not_eq(a: &[u8], b: &[u8], fold: bool, fuel: &mut u32) -> Result<bool, MatchRefusal> {
    // Length inspection is O(1); unequal lengths require no byte comparison.
    if a.len() != b.len() { return Ok(false); }
    for (&x, &y) in a.iter().zip(b) {
        not_charge(fuel)?;
        if if fold { !x.eq_ignore_ascii_case(&y) } else { x != y } {
            return Ok(false);
        }
    }
    Ok(true)
}
fn not_attr<'a>(attrs: &'a [(html5ever::LocalName, String)], name: &str,
    fold: bool, fuel: &mut u32) -> Result<Option<&'a str>, MatchRefusal> {
    for (key, value) in attrs {
        not_charge(fuel)?; // actual lookup iteration
        if not_eq(key.as_bytes(), name.as_bytes(), fold, fuel)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
fn not_words(actual: &[u8], want: &[u8], fold: bool, fuel: &mut u32)
    -> Result<bool, MatchRefusal> {
    let mut start = 0;
    for at in 0..actual.len() {
        not_charge(fuel)?; // byte inspected by ASCII-whitespace split
        if matches!(actual[at], b' ' | b'\t' | b'\n' | b'\r' | 0x0c) {
            if at > start && not_eq(&actual[start..at], want, fold, fuel)? { return Ok(true); }
            start = at + 1;
        }
    }
    Ok(start < actual.len() && not_eq(&actual[start..], want, fold, fuel)?)
}
impl AttrSel {
    fn matches_not_charged(&self, nodes: &[Node], handle: Handle, fuel: &mut u32)
        -> Result<bool, MatchRefusal> {
        let Data::Element { name, attrs, .. } = &nodes[handle].data else { return Ok(false); };
        let Some(actual) = not_attr(attrs, &self.name, true, fuel)? else { return Ok(false); };
        if self.op == AttrOp::Exists { return Ok(true); }
        // Existing attribute-value folding policy, without a lowercase copy.
        let mut fold = false;
        if name.ns == html5ever::ns!(html) {
            for candidate in CI_VALUE_ATTRS {
                not_charge(fuel)?;
                if not_eq(self.name.as_bytes(), candidate.as_bytes(), true, fuel)? {
                    fold = true; break;
                }
            }
        }
        let a = actual.as_bytes(); let b = self.value.as_bytes();
        match self.op {
            AttrOp::Exists => Ok(true),
            AttrOp::Equals => not_eq(a,b,fold,fuel),
            AttrOp::Word => {
                if b.is_empty() { return Ok(false); }
                for &byte in b {
                    not_charge(fuel)?;
                    if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c) { return Ok(false); }
                }
                not_words(a,b,fold,fuel)
            }
            AttrOp::Dash => {
                if not_eq(a,b,fold,fuel)? { return Ok(true); }
                if a.len() <= b.len() { return Ok(false); }
                if !not_eq(&a[..b.len()],b,fold,fuel)? { return Ok(false); }
                not_charge(fuel)?;
                Ok(a[b.len()] == b'-')
            }
            AttrOp::Prefix => Ok(!b.is_empty() && a.len() >= b.len()
                && not_eq(&a[..b.len()],b,fold,fuel)?),
            AttrOp::Suffix => Ok(!b.is_empty() && a.len() >= b.len()
                && not_eq(&a[a.len()-b.len()..],b,fold,fuel)?),
            AttrOp::Substring => {
                if b.is_empty() || a.len() < b.len() { return Ok(false); }
                for window in a.windows(b.len()) {
                    not_charge(fuel)?; // candidate substring position
                    if not_eq(window,b,fold,fuel)? { return Ok(true); }
                }
                Ok(false)
            }
        }
    }
}
impl Simple {
    fn matches_not_charged(&self, nodes: &[Node], handle: Handle, fuel: &mut u32)
        -> Result<bool, MatchRefusal> {
        let Data::Element { name, attrs, .. } = &nodes[handle].data else { return Ok(false); };
        match self.state {
            None => {},
            Some(StatePseudo::Link) => {
                if name.ns != html5ever::ns!(html)
                    || !matches!(&*name.local, "a" | "area")
                    || not_attr(attrs,"href",true,fuel)?.is_none() { return Ok(false); }
            }
            Some(_) => return Ok(false), // snapshot enrolled; quiescent/unfocused
        }
        if self.root && !nodes[handle].parent
            .is_some_and(|p| matches!(nodes[p].data, Data::Document)) { return Ok(false); }
        if let Some(tag) = &self.tag {
            if !not_eq(name.local.as_bytes(),tag.as_bytes(),true,fuel)? { return Ok(false); }
        }
        if let Some(want) = &self.id {
            let Some(actual) = not_attr(attrs,"id",false,fuel)? else { return Ok(false); };
            if !not_eq(actual.as_bytes(),want.as_bytes(),false,fuel)? { return Ok(false); }
        }
        for class in &self.classes {
            let Some(actual) = not_attr(attrs,"class",false,fuel)? else { return Ok(false); };
            if !not_words(actual.as_bytes(),class.as_bytes(),false,fuel)? { return Ok(false); }
        }
        for attr in &self.attrs {
            if !attr.matches_not_charged(nodes,handle,fuel)? { return Ok(false); }
        }
        if self.structural != 0 {
            if let Some(siblings) = element_siblings(nodes,handle) {
                if siblings.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
                // Same two directional scans as structural_ok, charged per
                // visited sibling, stopping at the first element each way.
                let mut first = None; let mut last = None;
                for &h in siblings {
                    not_charge(fuel)?;
                    if is_element(nodes,h) { first=Some(h);break; }
                }
                for &h in siblings.iter().rev() {
                    not_charge(fuel)?;
                    if is_element(nodes,h) { last=Some(h);break; }
                }
                if (self.structural & 1 != 0 && first != Some(handle))
                    || (self.structural & 2 != 0 && last != Some(handle))
                    || (self.structural & 4 != 0 && (first != Some(handle) || last != Some(handle))) {
                        return Ok(false);
                    }
            }
        }
        Ok(true)
    }
}
impl Simple {
    fn ext_specificity(&self) -> (u16, u16, u16) {
        let own = (
            u16::from(self.id.is_some()),
            (self.classes.len() + self.attrs.len()) as u16
                + u16::from(self.root) + u16::from(self.state.is_some())
                + u16::from(self.structural_n) - self.not_args.len() as u16 - self.where_args.len() as u16 - self.has_args.len() as u16,
            u16::from(self.tag.is_some()),
        );
        self.not_args.iter().chain(self.has_args.iter().flat_map(HasArgument::selectors)).fold(own, |(a,b,c), arg| {
            (a + arg.specificity.0, b + arg.specificity.1, c + arg.specificity.2)
        })
    }
}
impl Simple {
    fn matches_inner(&self, nodes: &[Node], handle: Handle, check_nth: bool) -> bool {
        let Data::Element { name, .. } = &nodes[handle].data else {
            return false;
        };
        match self.state {
            None => {}
            Some(StatePseudo::Link) if link_href(nodes, handle) => {}
            // `:hover`/`:active` under QuiescentCapture and the focus family
            // under UnfocusedInitial NEVER match: the correct CSS semantic
            // for a quiescent, unfocused capture, not a silent never-match.
            // Any future non-quiescent InputState or focused FocusState
            // variant must revisit this arm.
            Some(_) => return false,
        }
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
            && (!check_nth || self.nth.iter().all(|&an_b| nth_ok(nodes, handle, an_b)))
    }
}
impl Simple {
    fn matches(&self, nodes: &[Node], handle: Handle) -> bool {
        // The pure path never answers enablement (it needs charged ancestor
        // walks; ext_match owns it) or `:target` (it needs the caller-asserted
        // TargetState resolution): refuse to guess rather than infer from
        // attribute absence.
        if self.enablement.is_some() || self.target || !self.not_args.is_empty()
            || self.empty || !self.has_args.is_empty() || !self.where_args.is_empty() || self.of_type != 0 || !self.nth_of_type.is_empty() || !self.nth_last.is_empty() {
            return false;
        }
        self.matches_inner(nodes, handle, true)
    }
}
/// `:nth-child(An+B)` formula check: 1-based index among ELEMENT siblings
/// matches iff index = A*k + B for some integer k >= 0.
fn nth_formula(index: i64, (a, b): (i64, i64)) -> bool {
    if a == 0 {
        return index == b;
    }
    let d = index - b;
    d % a == 0 && d / a >= 0
}
/// `:nth-child(An+B)` DOM predicate: 1-based index among ELEMENT siblings in
/// tree order; text/comment nodes never shift the index. A detached node
/// follows the existing `structural_ok` convention (it is its own sibling
/// list, index 1).
fn nth_ok(nodes: &[Node], handle: Handle, an_b: (i64, i64)) -> bool {
    let index = match element_siblings(nodes, handle) {
        Some(_) => match nth_index(nodes, handle) {
            Some(index) => index,
            None => return false,
        },
        None => 1,
    };
    nth_formula(index, an_b)
}
/// 1-based index among ELEMENT siblings; None when the node is not among its
/// parent's children (an inconsistent tree is never a match).
fn nth_index(nodes: &[Node], handle: Handle) -> Option<i64> {
    let siblings = element_siblings(nodes, handle)?;
    let mut at = 0_i64;
    for &sib in siblings {
        if !is_element(nodes, sib) {
            continue;
        }
        at += 1;
        if sib == handle {
            return Some(at);
        }
    }
    None
}
fn tag_is(nodes: &[Node], handle: Handle, names: &[&str]) -> bool {
    match &nodes[handle].data {
        Data::Element { name, .. } => names
            .iter()
            .any(|t| name.local.as_ref().eq_ignore_ascii_case(t)),
        _ => false,
    }
}
fn has_disabled_attr(nodes: &[Node], handle: Handle) -> bool {
    match &nodes[handle].data {
        Data::Element { attrs, .. } => attrs
            .iter()
            .any(|(key, _)| (&**key).eq_ignore_ascii_case("disabled")),
        _ => false,
    }
}
/// One charged step up the ancestor chain; Fuel exhaustion refuses.
fn parent_charged(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<Option<Handle>, MatchRefusal> {
    match nodes[handle].parent {
        None => Ok(None),
        Some(parent) => {
            if *fuel == 0 {
                return Err(MatchRefusal::Fuel);
            }
            *fuel -= 1;
            Ok(Some(parent))
        }
    }
}
/// The first `legend` ELEMENT child of a fieldset, if any: one bounded scan
/// of the fieldset's children, every visited child charged.
fn first_legend_child(
    nodes: &[Node],
    fieldset: Handle,
    fuel: &mut u32,
) -> Result<Option<Handle>, MatchRefusal> {
    for &child in &nodes[fieldset].children {
        if *fuel == 0 {
            return Err(MatchRefusal::Fuel);
        }
        *fuel -= 1;
        if tag_is(nodes, child, &["legend"]) {
            return Ok(Some(child));
        }
    }
    Ok(None)
}
/// Shared fieldset-propagation walk (concept-fe-disabled second bullet and
/// recursive concept-fieldset-disabled second condition): is `handle`
/// disabled by an ANCESTOR fieldset[disabled], honoring each fieldset's
/// first-legend-child exemption? Every ancestor and every child visited
/// while identifying a first legend is charged. The path child is tracked so
/// the exemption applies exactly when the path passes through that legend.
fn disabled_by_ancestor_fieldset(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<bool, MatchRefusal> {
    let mut at = handle;
    while let Some(ancestor) = parent_charged(nodes, at, fuel)? {
        if tag_is(nodes, ancestor, &["fieldset"]) && has_disabled_attr(nodes, ancestor) {
            // First-legend exemption: if the element one step below this
            // fieldset on the path is a legend that is the fieldset's first
            // legend element child, this fieldset does not disable it.
            let exempt = first_legend_child(nodes, ancestor, fuel)? == Some(at);
            if !exempt {
                return Ok(true);
            }
        }
        at = ancestor;
    }
    Ok(false)
}
/// URL percent-decoding for a fragment string: each `%XX` (two hex digits)
/// becomes that byte; a `%` not followed by two hex digits passes through
/// literally (URL Standard percent-decode).
/// UTF-8 decode WITHOUT BOM with lossy replacement, built in two passes:
/// pass 1 computes the EXACT output length (O(L), no allocation), pass 2
/// builds into String::with_capacity(exact_len) - a single exact-capacity
/// allocation. Replacement semantics are byte-identical to
/// String::from_utf8_lossy (same std Utf8Error walk: valid prefix kept,
/// one U+FFFD per error, advance by error_len / rest-on-incomplete), which
/// is the 7.4.6.4 "UTF-8 decode without BOM" behavior. This is what makes
/// the decode charge honest BY CONSTRUCTION: capacity is exact, with zero
/// reliance on Vec growth, reallocation transients, or allocator behavior.
/// (with_capacity promises AT LEAST the requested capacity; allocator
/// rounding is possible, so no exact heap-peak claim is made anywhere.)
fn utf8_lossy_exact(bytes: &[u8]) -> String {
    let mut len = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        match std::str::from_utf8(&bytes[i..]) {
            Ok(rest) => {
                len += rest.len();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                len += valid + 3; // valid prefix + one U+FFFD
                let bad = e.error_len().unwrap_or(bytes.len() - i - valid);
                i += valid + bad;
            }
        }
    }
    let mut out = String::with_capacity(len);
    let mut i = 0usize;
    while i < bytes.len() {
        match std::str::from_utf8(&bytes[i..]) {
            Ok(rest) => {
                out.push_str(rest);
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(
                    std::str::from_utf8(&bytes[i..i + valid]).expect("valid prefix per Utf8Error"),
                );
                out.push('\u{FFFD}');
                let bad = e.error_len().unwrap_or(bytes.len() - i - valid);
                i += valid + bad;
            }
        }
    }
    debug_assert_eq!(out.len(), len, "exact-capacity construction");
    out
}
fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}
/// HTML 7.4.6.4 "find a potential indicated element": the first element in
/// tree order whose ID equals `fragment`; failing that, the first `a`
/// element in tree order whose `name` attribute equals it. Every visited
/// element charges one fuel; exhaustion refuses (never a partial answer).
/// Charge `cost` against the budget (u64 math; shortfall zeroes and
/// refuses - fail-closed, never a silent truncation).
fn charge(fuel: &mut u32, cost: u64) -> Result<(), MatchRefusal> {
    if cost > *fuel as u64 {
        *fuel = 0;
        return Err(MatchRefusal::Fuel);
    }
    *fuel -= cost as u32;
    Ok(())
}
/// One charged preorder traversal over the engine-parsed tree. Every
/// visited element costs 1 fuel; when the element carries `attr`, its
/// BYTE LENGTH is charged before the equality probe - a conservative
/// pre-charge for the compare work, since str equality compares at most
/// min(fragment.len(), attr.len()) <= attr.len() bytes. This makes the
/// raw and decoded scans' comparison cost explicitly budgeted instead of
/// relying on the engine's finite input as an implicit proof. The
/// explicit stack is proportional to tree breadth/depth (bounded by the
/// engine's byte-limited intake); NO allocation grows with the
/// caller-supplied fragment.
fn find_first_by_attr(
    nodes: &[Node],
    fragment: &str,
    fuel: &mut u32,
    attr_name: &str,
    only_a: bool,
) -> Result<Option<Handle>, MatchRefusal> {
    let mut stack = vec![crate::dom::DOCUMENT];
    while let Some(h) = stack.pop() {
        stack.extend(nodes[h].children.iter().rev());
        if !is_element(nodes, h) {
            continue;
        }
        if only_a && !tag_is(nodes, h, &["a"]) {
            charge(fuel, 1)?;
            continue;
        }
        charge(fuel, 1)?;
        if let Some(value) = attribute(nodes, h, attr_name) {
            charge(fuel, value.len() as u64)?;
            if value == fragment {
                return Ok(Some(h));
            }
        }
    }
    Ok(None)
}
fn find_potential_indicated(
    nodes: &[Node],
    fragment: &str,
    fuel: &mut u32,
) -> Result<Option<Handle>, MatchRefusal> {
    // Spec order: ALL id matches before ANY a[name] match, first in tree
    // order within each pass (two charged early-return traversals).
    if let Some(hit) = find_first_by_attr(nodes, fragment, fuel, "id", false)? {
        return Ok(Some(hit));
    }
    find_first_by_attr(nodes, fragment, fuel, "name", true)
}
/// Fuel charged per fragment byte before ANY percent-decode work.
/// Logical-work accounting per input byte: percent-decode scan+write ~2,
/// exact-length validation pass 1, build pass <= 3 (lossy expansion) -
/// 6 covers the total with the per-element visit/compare charges handled
/// separately in find_first_by_attr. Capacity BY CONSTRUCTION: the
/// percent-decode Vec is with_capacity(L) exactly and the decoded String
/// is with_capacity(exact computed length <= 3L) exactly; with_capacity
/// promises AT LEAST the request and allocator rounding is possible, so
/// this bounds logical work and requested capacity - NO exact heap-peak
/// claim is made. The caller-supplied fragment String has no engine-input
/// limit; this charge is what makes its decoding bounded and fail-closed:
/// exhaustion refuses (Err(Fuel) => truncated walk => no paint), never a
/// silent truncation, and no default/caps value changes (EXT_FUEL itself
/// is untouched).
const DECODE_FUEL_PER_BYTE: u32 = 6;
fn charge_decode(fragment: &str, fuel: &mut u32) -> Result<(), MatchRefusal> {
    charge(fuel, (fragment.len() as u64) * (DECODE_FUEL_PER_BYTE as u64))
}
/// Resolve the Document's target element under a caller-asserted TargetState
/// per the VERBATIM HTML 7.4.6.4 algorithm on THIS engine-parsed DOM
/// (select the indicated part + find a potential indicated element):
/// raw-string ID lookup first (literal `%74op` / `top` IDs WIN), then
/// percent-decode + UTF-8-without-BOM retry, then the ASCII-case-insensitive
/// "top" fallback; empty fragment and "top" resolve to top-of-document,
/// whose target element is NULL. Matcher use only: no scroll/focus effects.
pub(crate) fn resolve_target_element(
    nodes: &[Node],
    state: &TargetState,
    fuel: &mut u32,
) -> Result<Option<Handle>, MatchRefusal> {
    let TargetState::Fragment(fragment) = state else {
        // FragmentFreeCapture: "If the document's URL has no fragment
        // identifier, then the document has no target elements." (Selectors 4)
        return Ok(None);
    };
    if fragment.is_empty() {
        // Empty fragment => top of the document => target element null.
        return Ok(None);
    }
    if let Some(hit) = find_potential_indicated(nodes, fragment, fuel)? {
        return Ok(Some(hit));
    }
    charge_decode(fragment, fuel)?;
    let decoded = utf8_lossy_exact(&percent_decode(fragment));
    if decoded != *fragment {
        if let Some(hit) = find_potential_indicated(nodes, &decoded, fuel)? {
            return Ok(Some(hit));
        }
    }
    if decoded.eq_ignore_ascii_case("top") {
        return Ok(None);
    }
    Ok(None)
}
/// concept-fe-disabled for button/input/select/textarea.
fn control_disabled(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<bool, MatchRefusal> {
    if has_disabled_attr(nodes, handle) {
        return Ok(true);
    }
    disabled_by_ancestor_fieldset(nodes, handle, fuel)
}
/// concept-fieldset-disabled (RECURSIVE per the current HTML Standard).
fn fieldset_disabled(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<bool, MatchRefusal> {
    control_disabled(nodes, handle, fuel)
}
/// Nearest ancestor `select`, charged per visited ancestor.
fn nearest_ancestor_select(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<Option<Handle>, MatchRefusal> {
    let mut at = handle;
    while let Some(ancestor) = parent_charged(nodes, at, fuel)? {
        if tag_is(nodes, ancestor, &["select"]) {
            return Ok(Some(ancestor));
        }
        at = ancestor;
    }
    Ok(None)
}
/// Is the nearest ancestor select (if any) disabled? (4.15 composition.)
fn nearest_select_disabled(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<bool, MatchRefusal> {
    match nearest_ancestor_select(nodes, handle, fuel)? {
        Some(select) => control_disabled(nodes, select, fuel),
        None => Ok(false),
    }
}
/// concept-option-disabled: own attribute, else the nearest-ancestor scan
/// that STOPS at the first select/hr/datalist/option/optgroup ancestor
/// (optgroup's disabled attribute decides; the other four decide false).
fn option_disabled_algo(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<bool, MatchRefusal> {
    if has_disabled_attr(nodes, handle) {
        return Ok(true);
    }
    let mut at = handle;
    while let Some(ancestor) = parent_charged(nodes, at, fuel)? {
        if tag_is(nodes, ancestor, &["select", "hr", "datalist", "option"]) {
            return Ok(false);
        }
        if tag_is(nodes, ancestor, &["optgroup"]) {
            return Ok(has_disabled_attr(nodes, ancestor));
        }
        at = ancestor;
    }
    Ok(false)
}
/// 4.15 "actually disabled" for the disable-able set; None = the element is
/// outside the set and matches NEITHER :enabled nor :disabled (never
/// inferred from attribute absence). Form-associated custom elements are out
/// of engine scope (no custom-element host) and refuse by landing here.
fn element_disabled(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<Option<bool>, MatchRefusal> {
    if tag_is(nodes, handle, &["button", "input", "select", "textarea"]) {
        return control_disabled(nodes, handle, fuel).map(Some);
    }
    if tag_is(nodes, handle, &["fieldset"]) {
        return fieldset_disabled(nodes, handle, fuel).map(Some);
    }
    if tag_is(nodes, handle, &["optgroup"]) {
        if has_disabled_attr(nodes, handle) {
            return Ok(Some(true));
        }
        return nearest_select_disabled(nodes, handle, fuel).map(Some);
    }
    if tag_is(nodes, handle, &["option"]) {
        if option_disabled_algo(nodes, handle, fuel)? {
            return Ok(Some(true));
        }
        return nearest_select_disabled(nodes, handle, fuel).map(Some);
    }
    Ok(None)
}
/// Explicit browser-compatible Selectors3 policy, NOT draft4 whitespace
/// relaxation: all nonzero text, including whitespace, affects emptiness.
/// Inspect own children only, never template content handle. Retained string
/// length is O(1); no text scan/allocation. Charge visit before kind/length.
fn empty_children_charged(nodes: &[Node], handle: Handle, fuel: &mut u32)
    -> Result<Option<bool>, MatchRefusal> {
    let Some(Node {data: Data::Element {..}, children, ..}) = nodes.get(handle)
        else { return Ok(None); };
    if children.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
    for &child in children {
        not_charge(fuel)?;
        let Some(node) = nodes.get(child) else { return Ok(None); };
        match &node.data {
            Data::Element {..} => return Ok(Some(false)),
            Data::Text(text) if !text.is_empty() => return Ok(Some(false)),
            _ => {},
        }
    }
    Ok(Some(true))
}

/// Reverse all-element sibling index. Every raw entry visited, including
/// comments/text and full failed membership searches, costs one fuel unit.
/// Parentless=>1, inconsistent membership=>None; no type/namespace filtering.
/// Raw borrowed-slice gate precedes traversal. No forward-position shortcut.
fn nth_last_index_charged(nodes: &[Node], handle: Handle, fuel: &mut u32)
    -> Result<Option<i64>, MatchRefusal> {
    let Some(siblings) = element_siblings(nodes, handle) else { return Ok(Some(1)); };
    if siblings.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
    let mut reverse_index = 0_i64;
    for &sibling in siblings.iter().rev() {
        not_charge(fuel)?;
        if !is_element(nodes, sibling) { continue; }
        reverse_index += 1;
        if sibling == handle { return Ok(Some(reverse_index)); }
    }
    Ok(None)
}

/// Typed sibling census: visits ALL raw entries up to the tested node for
/// first/nth, or the full slice for last/only. No copied sibling list or
/// reliance on the walk's all-element index. Exact ns/local byte comparisons
/// ignore prefix. Detached convention is index/total1; absent membership is
/// unknown. The raw borrowed-slice oversize guard precedes any traversal.
fn typed_position_charged(
    nodes: &[Node], handle: Handle, need_total: bool, fuel: &mut u32,
) -> Result<Option<(i64, i64)>, MatchRefusal> {
    let Some(Node { data: Data::Element { name, .. }, .. }) = nodes.get(handle)
        else { return Ok(None); };
    let Some(siblings) = element_siblings(nodes, handle) else {
        return Ok(Some((1, 1)));
    };
    if siblings.len() > EXT_MAX_SIBLINGS { return Err(MatchRefusal::OversizeSiblings); }
    let mut count = 0_i64;
    let mut index = None;
    for &sibling in siblings {
        not_charge(fuel)?; // raw child visit, including text/comment
        let Some(Node { data: Data::Element { name: candidate, .. }, .. }) = nodes.get(sibling)
            else { continue; };
        if not_eq(name.ns.as_bytes(), candidate.ns.as_bytes(), false, fuel)?
            && not_eq(name.local.as_bytes(), candidate.local.as_bytes(), false, fuel)? {
                count += 1;
            }
        if sibling == handle {
            index = Some(count);
            if !need_total { return Ok(index.map(|ix| (ix, count))); }
        }
    }
    Ok(index.map(|ix| (ix, count)))
}

/// Budget-charged `nth_index` for callers without the walk's O(1) position:
/// the oversize gate refuses BEFORE any traversal, then every visited child
/// (element or not) costs one fuel unit. Detached => Some(1), never charged.
fn nth_index_charged(
    nodes: &[Node],
    handle: Handle,
    fuel: &mut u32,
) -> Result<Option<i64>, MatchRefusal> {
    let Some(siblings) = element_siblings(nodes, handle) else {
        return Ok(Some(1));
    };
    if siblings.len() > EXT_MAX_SIBLINGS {
        return Err(MatchRefusal::OversizeSiblings);
    }
    let mut at = 0_i64;
    for &sib in siblings {
        if *fuel == 0 {
            return Err(MatchRefusal::Fuel);
        }
        *fuel -= 1;
        if !is_element(nodes, sib) {
            continue;
        }
        at += 1;
        if sib == handle {
            return Ok(Some(at));
        }
    }
    Ok(None)
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
mod link_matcher_pin {
    //! WRONG-MATCH defense at the matcher level only: `:link` never matches a
    //! non-HTML element. This test locks that matching limitation; it does NOT
    //! guard the under-match direction. If the document-level SVG refusal is
    //! ever lifted, the separate document-refusal regression test
    //! (`style_tree::link_state_tests::link_never_matches_svg_a_in_v1`) is what
    //! fails loudly and forces explicit review of SVG `:link` at that moment.
    use super::*;
    use html5ever::{ns, LocalName, QualName};
    fn doc_with(elements: Vec<Node>) -> Vec<Node> {
        let mut nodes = vec![Node {
            data: Data::Document,
            parent: None,
            children: (1..=elements.len()).collect(),
        }];
        nodes.extend(elements);
        nodes
    }
    fn el(ns_uri: html5ever::Namespace, local: &str, attrs: &[(&str, &str)]) -> Node {
        Node {
            data: Data::Element {
                name: QualName::new(None, ns_uri, local.into()),
                attrs: attrs
                    .iter()
                    .map(|(k, v)| (LocalName::from(*k), (*v).to_owned()))
                    .collect(),
                template: None,
            },
            parent: Some(0),
            children: vec![],
        }
    }
    #[test]
    fn link_matches_only_html_a_area_with_href_at_the_matcher() {
        let nodes = doc_with(vec![
            el(ns!(html), "a", &[("href", "https://x")]), // 1: match
            el(ns!(html), "a", &[("href", "")]),          // 2: empty href counts
            el(ns!(html), "a", &[]),                      // 3: no href
            el(ns!(html), "area", &[("href", "https://y")]), // 4: match
            el(ns!(svg), "a", &[("href", "https://z")]),  // 5: SVG narrowed out
            el(ns!(svg), "a", &[("href", "https://w")]),  // 6: flattened xlink shape
            el(ns!(html), "map", &[("href", "https://m")]), // 7: wrong element
        ]);
        // Bare `:link`: the element predicate alone decides, no tag filter.
        let sel = selector_ext(":link").expect(":link parses in the ext grammar");
        assert!(sel.has_state());
        let matched: Vec<bool> = (1..nodes.len())
            .map(|h| sel.matches_checked(&nodes, h))
            .collect::<Option<Vec<_>>>()
            .expect("within fuel");
        assert_eq!(matched, [true, true, false, true, false, false, false]);
        // Case 6 stands in for a flattened `xlink:href`: its stored attribute
        // IS the local name "href", yet it cannot match - provenance
        // ambiguity is exactly why SVG is narrowed out in v1.
    }

    #[test]
    fn hover_active_never_match_any_element_at_the_matcher() {
        // Under the only input guarantee (QuiescentCapture) :hover/:active
        // never match: pin the matcher arm so a future InputState variant must
        // deliberately revisit it.
        let nodes = doc_with(vec![
            el(ns!(html), "div", &[]),
            el(ns!(html), "a", &[("href", "https://x")]),
            el(ns!(svg), "rect", &[]),
        ]);
        for text in [":hover", ":active", "div:hover", "a:active"] {
            let sel = selector_ext(text).expect(text);
            assert!(sel.has_state(), "{text}");
            for h in 1..nodes.len() {
                assert_eq!(
                    sel.matches_checked(&nodes, h),
                    Some(false),
                    "{text} on node {h}"
                );
            }
        }
    }

    #[test]
    fn focus_family_parses_and_never_matches_any_element_at_the_matcher() {
        // Under the only focus guarantee (UnfocusedInitial) the focus family
        // never matches: pin the matcher arm so a future FocusState variant
        // must deliberately revisit it. Enrollment gating is pinned in
        // style_tree state tests; here the raw matcher is quarantined.
        let nodes = doc_with(vec![
            el(ns!(html), "div", &[]),
            el(ns!(html), "input", &[("autofocus", "")]),
            el(ns!(html), "a", &[("href", "https://x")]),
        ]);
        for text in [
            ":focus",
            ":focus-visible",
            ":focus-within",
            "div:focus",
            "input:focus-visible",
            "div:focus-within a",
            ".x:focus",
        ] {
            let sel = selector_ext(text).expect(text);
            assert!(sel.has_state(), "{text}");
            for h in 1..nodes.len() {
                assert_eq!(
                    sel.matches_checked(&nodes, h),
                    Some(false),
                    "{text} on node {h}"
                );
            }
        }
        // Grammar bounds stay fail-closed: repetition, mixing with another
        // state pseudo-class remain refused. Bounded :not is now accepted
        // under its separate lane; nested wrappers remain refused.
        for text in ["a:focus:focus", "a:link:focus", ":not(:not(:focus))", "a:focus:hover"] {
            assert!(selector_ext(text).is_none(), "{text}");
        }
    }

    #[test]
    fn has_autofocus_is_engine_observable() {
        let with = doc_with(vec![
            el(ns!(html), "div", &[]),
            el(ns!(html), "input", &[("AUTOFOCUS", "autofocus")]),
        ]);
        assert!(has_autofocus(&with));
        let without = doc_with(vec![
            el(ns!(html), "div", &[]),
            el(ns!(html), "input", &[("type", "text")]),
        ]);
        assert!(!has_autofocus(&without));
    }
}

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
            "p:not(.a,.b)", "p:is(.a)", "p:nth-last-of-type(2)", "p:empty()", "p[a=b i]",
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
        use crate::style_diagnostics::RefusalReason;
        let mut html = String::from(
            "<style>i + i{background-color:#ff0000} i{background-color:#0000ff}</style><div>",
        );
        for _ in 0..4100 {
            html.push_str("<i></i>");
        }
        html.push_str("</div><div><i>x</i><i>y</i></div>");
        let tree = run(true, &html);
        // Rule-scoped refusal: one precise diagnostic per oversize element,
        // no unsupported flag, control rule applies to all, and the
        // small-list `i + i` still matches its second element (the gate
        // costs no unrelated rule, inside or outside the oversize list).
        // The PAGE is flagged truncated (a potential match was dropped), so
        // the painter refuses it whole.
        assert!(!tree.unsupported && tree.truncated);
        assert_eq!(tree.diagnostics.count(RefusalReason::MatchWork), 4100);
        let mut expected = vec![false; 4100];
        expected.extend([false, true]);
        assert_eq!(reds(&tree, "i"), expected);
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
        // Expectation change (disclosed, fl12): the ext entry now refuses `svg` (unmodelled replaced content), so the page is
        // unsupported; this test checks selector matching only, which the per-node assertions below still do.
        assert!(tree.unsupported);
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
    #[test]
    fn after_pseudo_element_grammar_is_extended_only_trailing_and_exact() {
        let ext = |s: &str| super::selector_ext(s);
        let p = ext("div.p::after").expect("extended accepts a trailing ::after");
        assert!(p.pseudo == super::Pseudo::After);
        // Specificity: (0,1,1) for div.p plus one type for the pseudo-element.
        assert_eq!(p.specificity, (0, 1, 2));
        assert_eq!(
            ext("DIV.p::AFTER").map(|s| s.pseudo),
            Some(super::Pseudo::After)
        );
        for bad in [
            "::after",
            "a ::after",
            "a:after",
            "a::after::after",
            "a::after b",
            "a::first-line",
            "a::after,b",
            "a::afterx",
            "a:hover::after",
        ] {
            assert!(ext(bad).is_none(), "{bad}");
        }
        // `::before` mirrors `::after` exactly: trailing, exact, one only.
        let b = ext("div.p::before").expect("extended accepts a trailing ::before");
        assert!(b.pseudo == super::Pseudo::Before);
        assert_eq!(b.specificity, (0, 1, 2));
        assert_eq!(
            ext("DIV.p::BEFORE").map(|s| s.pseudo),
            Some(super::Pseudo::Before)
        );
        for bad in [
            "::before",
            "a ::before",
            "a:before",
            "a::before::after",
            "a::before b",
            "a::beforex",
            "a:hover::before",
        ] {
            assert!(ext(bad).is_none(), "{bad}");
        }
        // The legacy grammar never accepts a pseudo-element.
        assert!(super::selector("div.p::after").is_none());
        assert!(ext("a").unwrap().pseudo == super::Pseudo::None);
    }
    #[test]
    fn nth_child_parser_token_pins() {
        // Valid token shapes (grounded against the Chrome 157 CSS.supports
        // selector() oracle, probe 2026-11-10): attached signs, ASCII
        // case-insensitive n, odd/even, whitespace around the B sign.
        for ok in [
            "2n+1", "2N+1", "2n + 1", "2n+ 1", "2n +1", "+2n+1", "-2n+1", "-n+2", "n", "+n",
            "-n", "N", "2n", "odd", "ODD", "even", "3n-1", "3n - 1", "0", "5", "+5", " 2n+1 ",
        ] {
            assert!(super::selector_ext(&format!("p:nth-child({ok})")).is_some(), "{ok}");
        }
        // Invalid CSS (Chrome refuses these too): doubled signs, separated
        // signs, scientific/decimal forms, dangling sign.
        for bad in [
            "2n + -1", "2n+-1", "- n + 2", "- 3", "1e2", "2.0n", "2n+1.0", "2n++1", "2n+",
            "of .x", "n\\", "\\36 e",
        ] {
            assert!(super::selector_ext(&format!("p:nth-child({bad})")).is_none(), "{bad}");
        }
        // VALID CSS refused as an unsupported subset (a grammar bound, not an
        // invalid-CSS claim): comments, escaped n, the Selectors-4 of-S form.
        for subset in ["2n/**/+1", "\\6e ", "2 of .x"] {
            assert!(super::selector_ext(&format!("p:nth-child({subset})")).is_none(), "{subset}");
        }
        // Bounds: argument <= 32 chars, |A| <= 1024, |B| <= 65535.
        assert!(super::selector_ext("p:nth-child(1024n+65535)").is_some());
        assert!(super::selector_ext("p:nth-child(1025n+1)").is_none());
        assert!(super::selector_ext("p:nth-child(2n+65536)").is_none());
        assert!(super::selector_ext("p:nth-child(111111111111111111111111111111111)").is_none());
        // Parsed (A, B) values.
        let vals = |a: &str| super::selector_ext(&format!("p:nth-child({a})")).unwrap().steps[0].0.nth.clone();
        assert_eq!(vals("odd"), [(2, 1)]);
        assert_eq!(vals("even"), [(2, 0)]);
        assert_eq!(vals("-n+2"), [(-1, 2)]);
        assert_eq!(vals("5"), [(0, 5)]);
        assert_eq!(vals("3n - 1"), [(3, -1)]);
        // Reverse type-relative nth and of-S remain outside the accepted subset.
        for out in ["p:nth-last-child(2 of p)", "p:nth-last-of-type(2)", "p:nth-child(2 of p)"] {
            assert!(super::selector_ext(out).is_none(), "{out}");
        }
    }
    #[test]
    fn nth_child_actual_path_positions_signs_zero_negative_and_default_identity() {
        // Element-only 1-based indexing: text and comments never count.
        let html = "<style>p:nth-child(1){background-color:#ff0000}</style><div>text<!--c--><p>a</p><p>b</p></div><section><p>c</p></section>";
        let tree = run(true, html);
        assert!(!tree.unsupported);
        assert_eq!(reds(&tree, "p"), [true, false, true]);
        // Negative A.
        let html = "<style>p:nth-child(-n+2){background-color:#ff0000}</style><div><p>1</p><p>2</p><p>3</p><p>4</p></div>";
        assert_eq!(reds(&run(true, html), "p"), [true, true, false, false]);
        // Positive A with offset.
        let html = "<style>p:nth-child(2n+1){background-color:#ff0000}</style><div><p>1</p><p>2</p><p>3</p><p>4</p></div>";
        assert_eq!(reds(&run(true, html), "p"), [true, false, true, false]);
        // Zero matches nothing.
        let html = "<style>p:nth-child(0){background-color:#ff0000}</style><div><p>1</p></div>";
        assert_eq!(reds(&run(true, html), "p"), [false]);
        // Plain index.
        let html = "<style>p:nth-child(3){background-color:#ff0000}</style><div><p>1</p><p>2</p><p>3</p></div>";
        assert_eq!(reds(&run(true, html), "p"), [false, false, true]);
        // Default entry identity: the same selector stays fail-closed there.
        let tree = run(false, "<style>p:nth-child(2){background-color:#ff0000}</style><p>a</p><p>b</p>");
        assert!(tree.unsupported);
        assert_eq!(reds(&tree, "p"), [false, false]);
    }
    #[test]
    fn sibling_traversal_is_charged_to_the_global_match_work_budget() {
        use crate::style_diagnostics::RefusalReason;
        // NEGATIVE CONTROL (budget exhaustion): 3000 <i> elements, one rule
        // `span ~ i` whose LaterSibling scan NEVER matches. Before traversal
        // charging this page cost 3000 match-work units (1 per test); now
        // each test charges its scanned siblings (element at position p
        // costs ~2p fuel), so the 4M budget exhausts mid-walk: the tree is
        // flagged truncated with a MatchWork diagnostic and the page can
        // never paint with an incomplete style computation. Total sibling
        // visits are bounded by MAX_MATCH_WORK + EXT_FUEL by construction.
        let mut html =
            String::from("<style>span ~ i{background-color:#ff0000}</style><div>");
        for _ in 0..3000 {
            html.push_str("<i>x</i>");
        }
        html.push_str("</div>");
        let tree = run(true, &html);
        assert!(!tree.unsupported);
        assert!(tree.truncated);
        assert!(tree.diagnostics.count(RefusalReason::MatchWork) >= 1);
    }
    #[test]
    fn nth_scan_off_the_rightmost_step_is_fuel_charged_and_fails_closed() {
        use crate::style_diagnostics::RefusalReason;
        // NEGATIVE CONTROL (nth traversal): nth on a NON-rightmost step gets
        // no walk position, so its index scan is charged to the per-match
        // fuel (one unit per visited sibling). The quadratic scan cost of
        // `i:nth-child(4096) ~ i` over a growing list exhausts EXT_FUEL on a
        // single match long before position 4096: the refusal is fail-closed
        // (truncated + MatchWork), never a silent mismatch.
        let mut html =
            String::from("<style>i:nth-child(4096) ~ i{background-color:#ff0000}</style><div>");
        for _ in 0..3000 {
            html.push_str("<i>x</i>");
        }
        html.push_str("</div>");
        let tree = run(true, &html);
        assert!(!tree.unsupported);
        assert!(tree.truncated);
        assert!(tree.diagnostics.count(RefusalReason::MatchWork) >= 1);
    }
    #[test]
    fn nth_child_oversize_sibling_gate_is_precise_at_the_exact_boundary() {
        use crate::style_diagnostics::RefusalReason;
        const BLUE: Option<u32> = Some(0x00_00_ff);
        // Isolated under every other budget: Limits::DEFAULT (120k nodes,
        // 64KB sheets, 4M match-work, EXT_FUEL 4096 per match) - ONLY the
        // EXT_MAX_SIBLINGS=4096 gate differs between the three pages.
        let build = |n: usize| {
            let mut html = String::from(
                "<style>p:nth-child(4096){background-color:#ff0000} p{background-color:#0000ff}</style><div>",
            );
            for _ in 0..n {
                html.push_str("<p>x</p>");
            }
            html.push_str("</div><div><p>lone</p></div>");
            html
        };
        let backgrounds = |tree: &StyleTree| -> Vec<Option<u32>> {
            tree.nodes
                .iter()
                .filter(|n| n.tag == "p")
                .map(|n| n.style.background_color)
                .collect()
        };
        // BELOW (4095 siblings): gate untripped; position 4096 absent, so the
        // rule simply does not match - no diagnostic, control applies to all.
        let tree = run(true, &build(4095));
        assert!(!tree.unsupported && !tree.truncated);
        assert_eq!(tree.diagnostics.count(RefusalReason::MatchWork), 0);
        assert_eq!(backgrounds(&tree).len(), 4096);
        assert!(backgrounds(&tree).iter().all(|&c| c == BLUE));
        // AT (4096 siblings): gate untripped (strictly-greater test); the rule
        // MATCHES exactly position 4096 (the last one); no refusal recorded.
        let tree = run(true, &build(4096));
        assert!(!tree.unsupported && !tree.truncated);
        assert_eq!(tree.diagnostics.count(RefusalReason::MatchWork), 0);
        let colors = backgrounds(&tree);
        assert_eq!(colors.len(), 4097);
        assert!(colors[..4095].iter().all(|&c| c == BLUE));
        assert_eq!(colors[4095], RED);
        assert_eq!(colors[4096], BLUE);
        // ABOVE (4097 siblings): the nth rule REFUSES for each of the 4097
        // oversize elements - exactly 4097 precise rule-scoped diagnostics.
        // No silent non-match: the refusal is recorded, not dropped. No
        // unrelated rule loss: the control rule still applies to EVERY
        // element including the oversize ones, and the lone outside element
        // is untouched by the gate. AND the page is flagged truncated: a
        // potentially-matching rule was dropped, so this page is incomplete
        // and the all-or-nothing painter confinement refuses it (negative
        // paint-path proof in tests/css_paint_bridge.rs).
        let tree = run(true, &build(4097));
        assert!(!tree.unsupported && tree.truncated);
        assert_eq!(tree.diagnostics.count(RefusalReason::MatchWork), 4097);
        let colors = backgrounds(&tree);
        assert_eq!(colors.len(), 4098);
        assert!(colors.iter().all(|&c| c == BLUE));
    }
    #[test]
    fn nth_ok_detached_element_is_index_one() {
        use html5ever::{ns, LocalName, QualName};
        let nodes = vec![super::Node {
            data: super::Data::Element {
                name: QualName::new(None, ns!(html), LocalName::from("p")),
                attrs: Default::default(),
                template: None,
            },
            parent: None,
            children: vec![],
        }];
        assert!(super::nth_ok(&nodes, 0, (0, 1)));
        assert!(!super::nth_ok(&nodes, 0, (0, 2)));
        assert!(super::nth_ok(&nodes, 0, (2, 1)));
        assert!(!super::nth_ok(&nodes, 0, (2, 0)));
        assert!(!super::nth_ok(&nodes, 0, (0, 0)));
    }

    // ---- bounded opt-in :enabled / :disabled (HTML 4.15, spec-verbatim
    // algorithms; oracle: Chrome 157 engine-DOM replay, 60 cases) ----
    #[test]
    fn enablement_grammar_pins() {
        use super::selector_ext;
        // At most one enablement per compound; repetition refuses (grammar bound).
        assert!(selector_ext("input:enabled:disabled").is_none());
        assert!(selector_ext("input:enabled:enabled").is_none());
        assert!(selector_ext("input:disabled:disabled").is_none());
        // Functional form is not this family.
        assert!(selector_ext("input:enabled(1)").is_none());
        assert!(selector_ext("input:disabled(2n+1)").is_none());
        // Bare forms parse (any element; non-FACE matches neither).
        assert!(selector_ext("input:enabled").is_some());
        assert!(selector_ext(":disabled").is_some());
        assert!(selector_ext("div:enabled").is_some());
    }
    #[test]
    fn base_entry_still_refuses_enablement() {
        // Base grammar refuses enablement at the parser level...
        assert!(super::selector("input:enabled").is_none());
        assert!(super::selector("button:disabled").is_none());
        assert!(super::selector(":enabled").is_none());
        let html = "<style>input:enabled{background-color:#ff0000}</style><input>";
        assert!(run(false, html).unsupported);
        let html = "<style>button:disabled{background-color:#ff0000}</style><button disabled>x</button>";
        assert!(run(false, html).unsupported);
    }
    #[test]
    fn non_face_element_matches_neither_enablement() {
        // div is not a FACE / disabled-capable element: BOTH pseudo-classes
        // fail (never inferred from attribute absence). Not unsupported.
        let html = "<style>div:enabled{background-color:#ff0000}</style><div>x</div><div disabled>y</div>";
        let tree = run(true, html);
        assert!(!tree.unsupported);
        assert!(!tree.truncated);
        assert_eq!(reds(&tree, "div"), vec![false, false]);
        let html = "<style>div:disabled{background-color:#ff0000}</style><div>x</div><div disabled>y</div>";
        let tree = run(true, html);
        assert!(!tree.unsupported);
        assert_eq!(reds(&tree, "div"), vec![false, false]);
    }
    #[test]
    fn first_legend_exemption_and_recursive_fieldset_walk_pins() {
        // <fieldset disabled><legend><input></legend><button></fieldset>:
        // input inside the FIRST legend child is enabled; the button is disabled.
        let html = "<style>input:disabled{background-color:#ff0000}button:disabled{background-color:#ff0000}</style><fieldset disabled><legend>l<input></legend><button>x</button></fieldset>";
        let tree = run(true, html);
        // Pages containing button/input independently flag CssDeclaration
        // (pre-existing form-control refusal, reproduced with a plain
        // `button{}` rule on this build), so the pin is matcher truth only.
        assert!(!tree.truncated);
        assert_eq!(reds(&tree, "input"), vec![false]);
        assert_eq!(reds(&tree, "button"), vec![true]);
        // Recursive concept-fieldset-disabled: inner fieldset (no own attr)
        // under a disabled outer fieldset disables its controls.
        let html = "<style>button:disabled{background-color:#ff0000}</style><fieldset disabled><fieldset><button>x</button></fieldset></fieldset>";
        let tree = run(true, html);
        assert_eq!(reds(&tree, "button"), vec![true]);
        // fieldset:disabled itself is recursive: inner matches too.
        let html = "<style>fieldset:disabled{background-color:#ff0000}</style><fieldset disabled><fieldset></fieldset></fieldset><fieldset></fieldset>";
        let tree = run(true, html);
        assert_eq!(reds(&tree, "fieldset"), vec![true, true, false]);
    }
    #[test]
    fn select_option_composition_walk_pins() {
        // Disabled select disables its option descendants.
        let html = "<style>option:disabled{background-color:#ff0000}</style><select disabled><option>a</option></select><select><option>b</option></select>";
        let tree = run(true, html);
        assert!(!tree.truncated);
        assert_eq!(reds(&tree, "option"), vec![true, false]);
        // optgroup's own disabled attr disables its option children even
        // under an ENABLED select (nearest-ancestor stop at optgroup).
        let html = "<style>option:disabled{background-color:#ff0000}</style><select><optgroup disabled><option>a</option></optgroup><optgroup><option>b</option></optgroup></select>";
        let tree = run(true, html);
        assert_eq!(reds(&tree, "option"), vec![true, false]);
        // select:disabled by its own attr.
        let html = "<style>select:disabled{background-color:#ff0000}</style><select disabled></select><select></select>";
        let tree = run(true, html);
        assert_eq!(reds(&tree, "select"), vec![true, false]);
    }
    #[test]
    fn enablement_ancestor_walk_is_charged_to_the_global_match_work_budget() {
        use crate::style_diagnostics::RefusalReason;
        // NEGATIVE CONTROL (budget exhaustion): 3000 nested <div><input> -
        // each input's :enabled check walks its full ancestry (depth d costs
        // ~d fuel), summing past the 4M match-work budget: the tree is
        // flagged truncated with a MatchWork diagnostic, never a silent
        // incomplete style computation.
        let mut html =
            String::from("<style>input:enabled{background-color:#ff0000}</style>");
        for _ in 0..3000 {
            html.push_str("<div><input>");
        }
        let tree = run(true, &html);
        // (input pages independently flag CssDeclaration; the pin is the
        // budget-exhaustion truncation, not page support.)
        assert!(tree.truncated);
        assert!(tree.diagnostics.count(RefusalReason::MatchWork) >= 1);
    }
}


#[cfg(test)]
mod attribution_harness {
    //! TEST-ONLY, measurement-only (private research): actual-parser attribution
    //! of the corpus refused-selector inventory. Calls the real internal parser
    //! entry points (`selector_ext`, `compound_ext`, `attr_selector`) - never a
    //! textual proxy - and records the exact failing compound plus the token
    //! classes present in it. Ambiguity is preserved: a failing compound with
    //! several unsupported-looking tokens is reported as the SET, not a guess.
    //! No shipping code; `cargo test -- --ignored` only. Input: /tmp/inv lists.
    fn compounds_top_level(sel: &str) -> Vec<String> {
        // Split on top-level combinators/whitespace, paren+bracket aware.
        // Combinator tokens themselves are grammar terminals in `selector_ext`,
        // so splitting at this level never crosses a compound boundary.
        let mut out = Vec::new();
        let mut cur = String::new();
        let (mut depth_p, mut depth_b, mut prev_ws) = (0i32, 0i32, false);
        for c in sel.chars() {
            match c {
                '(' => { depth_p += 1; cur.push(c); }
                ')' => { depth_p -= 1; cur.push(c); }
                '[' => { depth_b += 1; cur.push(c); }
                ']' => { depth_b -= 1; cur.push(c); }
                '>' | '+' | '~' if depth_p == 0 && depth_b == 0 => {
                    if !cur.trim().is_empty() { out.push(cur.trim().to_string()); }
                    cur.clear();
                    prev_ws = false;
                }
                c if c.is_ascii_whitespace() && depth_p == 0 && depth_b == 0 => {
                    if !prev_ws && !cur.trim().is_empty() { out.push(cur.trim().to_string()); cur.clear(); }
                    prev_ws = true;
                }
                _ => { cur.push(c); prev_ws = false; }
            }
        }
        if !cur.trim().is_empty() { out.push(cur.trim().to_string()); }
        out
    }
    fn token_classes(compound: &str) -> Vec<String> {
        let mut classes = Vec::new();
        let b = compound.as_bytes();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b':' => {
                    let double = b.get(i + 1) == Some(&b':');
                    let start = i + if double { 2 } else { 1 };
                    let mut j = start;
                    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-') { j += 1; }
                    let name = &compound[start..j];
                    let functional = b.get(j) == Some(&b'(');
                    let kind = if double { "pseudo-element" } else if functional { "functional-pseudo" } else { "pseudo-class" };
                    classes.push(format!("{kind}:{name}"));
                    i = j;
                }
                b'[' => {
                    let end = compound[i..].find(']').map(|k| i + k).unwrap_or(compound.len());
                    let inner = &compound[i + 1..end];
                    let flag = inner.contains(" i]") || inner.ends_with(" i") || inner.ends_with(" s");
                    classes.push(format!("attr:{}", if flag { "with-flag" } else { "plain" }));
                    i = end;
                }
                b'\\' => { classes.push("escape".to_string()); i += 1; }
                b'#' => { classes.push("id".to_string()); i += 1; }
                b'.' => { classes.push("class".to_string()); i += 1; }
                _ => i += 1,
            }
        }
        classes.sort();
        classes.dedup();
        classes
    }
    /// Attribute one selector through the REAL parser. Returns
    /// (selector, verdict) where verdict is `parses`, `composition:<why>`,
    /// `compound:<index>:<token-class-set>`, or `ambiguous:<detail>`.
    fn attribute(sel: &str) -> (String, String) {
        if super::selector_ext(sel).is_some() {
            return (sel.to_string(), "parses".to_string());
        }
        // Pseudo-element placement/composition refusals: trailing ::before/::after
        // handled by selector_ext itself; if the head parses alone, composition failed.
        for suffix in ["::before", "::after"] {
            if let Some(head) = sel.strip_suffix(suffix) {
                if super::selector_ext(head.trim_end()).is_some() {
                    return (sel.to_string(), format!("composition:state-on-pseudo-element-or-second-pseudo:{suffix}"));
                }
            }
        }
        let compounds = compounds_top_level(sel);
        let mut failing = Vec::new();
        for (idx, compound) in compounds.iter().enumerate() {
            if super::compound_ext(compound, 0).map(|(_, next)| next) != Some(compound.len()) {
                failing.push((idx, token_classes(compound)));
            }
        }
        match failing.len() {
            0 => (sel.to_string(), "ambiguous:compounds-parse-but-selector-refused".to_string()),
            1 => {
                let (idx, classes) = failing.into_iter().next().unwrap();
                (sel.to_string(), format!("compound:{idx}:{}", classes.join("+")))
            }
            _ => {
                let detail: Vec<String> = failing.iter().map(|(i, c)| format!("{i}:{}", c.join("+"))).collect();
                (sel.to_string(), format!("ambiguous:multiple-failing-compounds:{}", detail.join("|")))
            }
        }
    }
    #[test]
    #[ignore = "measurement harness; run explicitly"]
    fn corpus_refused_selector_attribution() {
        let dir = std::env::var("ATTRIB_INV_DIR").unwrap_or_else(|_| "/tmp/inv".to_string());
        for pg in ["gov", "blog", "docs", "news-npr", "wikipedia"] {
            let path = format!("{dir}/{pg}-still-refused-under-state.txt");
            let Ok(data) = std::fs::read_to_string(&path) else { continue };
            let mut histogram: std::collections::BTreeMap<String, usize> = Default::default();
            let mut n = 0;
            for line in data.lines().filter(|l| !l.is_empty()) {
                let (_, verdict) = attribute(line);
                *histogram.entry(verdict).or_default() += 1;
                n += 1;
            }
            println!("## {pg} total={n}");
            for (verdict, count) in histogram {
                println!("   {count:5}  {verdict}");
            }
        }
    }
}

#[cfg(test)]
mod enablement_oracle_replay_tests {
    //! ENGINE-PARSED-DOM replay of the full Chrome 157 oracle (60 element
    //! cases; rounds 1-3, raw artifacts in
    //! oracle-enabled-disabled-20261110/): markup is byte-verbatim from the
    //! probe page/scripts, parsed by the engine's own html5ever Sink
    //! (scripting disabled), matched via the REAL bounded matcher. Matcher
    //! truth only - no paint or readiness claim.
    use super::*;
    use html5ever::tendril::TendrilSink;
    fn engine_nodes(html: &str) -> Vec<Node> {
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
        parser.process(html.to_string().into());
        parser.finish().into_nodes()
    }
    fn find_by_id(nodes: &[Node], id: &str) -> Handle {
        let mut found = None;
        for (h, n) in nodes.iter().enumerate() {
            if let Data::Element { attrs, .. } = &n.data {
                if attrs.iter().any(|(k, v)| &**k == "id" && v == id) {
                    assert!(found.is_none(), "duplicate id {id}");
                    found = Some(h);
                }
            }
        }
        found.unwrap_or_else(|| panic!("id {id} not in engine DOM"))
    }
    fn ancestry(nodes: &[Node], handle: Handle) -> String {
        let mut chain = Vec::new();
        let mut at = nodes[handle].parent;
        while let Some(p) = at {
            if let Data::Element { name, attrs, .. } = &nodes[p].data {
                let mut t = name.local.to_string().to_uppercase();
                if attrs.iter().any(|(k, _)| (&**k).eq_ignore_ascii_case("disabled")) {
                    t.push_str("[disabled]");
                }
                chain.push(t);
            }
            at = nodes[p].parent;
        }
        chain.retain(|t| t != "HTML");
        chain.join(">")
    }
    const CASES: &[(&str, bool, bool)] = &[
        ("i1", true, false),
        ("i2", false, true),
        ("i3", true, false),
        ("i4", false, true),
        ("d1", false, false),
        ("d2", false, false),
        ("b1", true, false),
        ("b2", false, true),
        ("s1", true, false),
        ("s2", false, true),
        ("t1", true, false),
        ("t2", false, true),
        ("og1", true, false),
        ("og2", false, true),
        ("o1", true, false),
        ("o2", false, true),
        ("o3", false, true),
        ("o4", true, false),
        ("o5", false, true),
        ("f1", true, false),
        ("f2", false, true),
        ("fi1", false, true),
        ("fi2", true, false),
        ("fi3", false, true),
        ("fi4", false, true),
        ("fi5", true, false),
        ("a1", false, false),
        ("u1", false, false),
        ("u2", false, false),
        ("x1", false, true),
        ("x2", false, true),
        ("x3", true, false),
        ("x4", false, true),
        ("x5", false, true),
        ("x6", true, false),
        ("x7", false, true),
        ("x8", false, true),
        ("x9", false, true),
        ("x10", true, false),
        ("x11", true, false),
        ("x12", false, false),
        ("x13", false, true),
        ("x14", false, true),
        ("x15", false, false),
        ("g1", false, true),
        ("g1o", false, true),
        ("g2o", false, true),
        ("fs", false, true),
        ("fog", false, true),
        ("foo", false, true),
        ("fod", false, true),
        ("inner", false, true),
        ("inneri", false, true),
        ("inner2", false, true),
        ("inner2i", false, true),
        ("leg1", true, false),
        ("r10", true, false),
        ("r11", true, false),
        ("r12", true, false),
        ("r13", true, false),
    ];
    #[test]
    fn enablement_oracle_replay_all_cases_on_engine_parsed_dom() {
        let nodes = engine_nodes(PAGE);
        assert_eq!(CASES.len(), 60);
        for &(id, en, dis) in CASES {
            let handle = find_by_id(&nodes, id);
            let en_sel = selector_ext(&format!("#{id}:enabled")).unwrap();
            let dis_sel = selector_ext(&format!("#{id}:disabled")).unwrap();
            assert_eq!(
                en_sel.matches_checked(&nodes, handle),
                Some(en),
                ":enabled mismatch on #{id} (ancestry {})",
                ancestry(&nodes, handle)
            );
            assert_eq!(
                dis_sel.matches_checked(&nodes, handle),
                Some(dis),
                ":disabled mismatch on #{id} (ancestry {})",
                ancestry(&nodes, handle)
            );
        }
    }
    #[test]
    fn engine_parsed_ancestry_matches_oracle_disclosure() {
        // Ancestry fidelity: the engine's parser produces the same structure
        // the oracle disclosed (Chrome's parser), incl. NO select ancestor
        // for x10/x11 and the fieldset>select>optgroup>option chain.
        let nodes = engine_nodes(PAGE);
        assert_eq!(ancestry(&nodes, find_by_id(&nodes, "x11")), "OPTGROUP>FIELDSET[disabled]>DIV>BODY");
        assert_eq!(ancestry(&nodes, find_by_id(&nodes, "fog")), "SELECT>FIELDSET[disabled]>BODY");
        assert_eq!(ancestry(&nodes, find_by_id(&nodes, "foo")), "OPTGROUP>SELECT>FIELDSET[disabled]>BODY");
        assert_eq!(ancestry(&nodes, find_by_id(&nodes, "inneri")), "FIELDSET>FIELDSET[disabled]>BODY");
        assert_eq!(ancestry(&nodes, find_by_id(&nodes, "leg1")), "LEGEND>FIELDSET[disabled]>BODY");
    }
    const PAGE: &str = r#"<!doctype html><body>

<input id=i1>
<input id=i2 disabled>
<input id=i3 type=hidden>
<input id=i4 type=hidden disabled>
<div id=d1></div><div id=d2 disabled></div>
<button id=b1></button><button id=b2 disabled></button>
<select id=s1></select><select id=s2 disabled></select>
<textarea id=t1></textarea><textarea id=t2 disabled></textarea>
<select><optgroup id=og1><option id=o1>x</option></optgroup></select>
<select><optgroup id=og2 disabled><option id=o2>x</option></optgroup></select>
<select><option id=o3 disabled>x</option><option id=o4>x</option></select>
<select disabled><option id=o5>x</option></select>
<fieldset id=f1></fieldset><fieldset id=f2 disabled></fieldset>
<fieldset disabled><input id=fi1></fieldset>
<fieldset disabled><legend>l<input id=fi2></legend><input id=fi3></fieldset>
<fieldset disabled><legend>l1</legend><legend>l2<input id=fi4></legend></fieldset>
<fieldset><input id=fi5></fieldset>
<a id=a1 href=x>a</a><foo id=u1 disabled></foo><foo id=u2></foo>

<div><fieldset disabled><div><input id=x1></div></fieldset>
<fieldset disabled><div><legend>l<input id=x2></legend></div></fieldset>
<fieldset disabled><span></span><legend>l<input id=x3></legend></fieldset>
<fieldset disabled><legend>l</legend><fieldset><input id=x4></fieldset></fieldset>
<fieldset disabled><fieldset><input id=x5></fieldset></fieldset>
<form disabled><input id=x6></form>
<fieldset disabled><button id=x7></button><select id=x8></select><textarea id=x9></textarea></fieldset>
<fieldset disabled><optgroup id=x10><option id=x11>o</option></optgroup></fieldset>
<menu><li id=x12 disabled>i</li></menu>
<input id=x13 disabled="">
<input id=x14 disabled="disabled">
<object id=x15 disabled></object></div>

<select disabled><optgroup id=g1><option id=g1o>x</option></optgroup></select>
<select disabled><option id=g2o>x</option></select>
<fieldset disabled><select id=fs><optgroup id=fog><option id=foo>x</option></optgroup><option id=fod>x</option></select></fieldset>
<fieldset disabled><fieldset id=inner><input id=inneri></fieldset></fieldset>
<fieldset disabled><fieldset id=inner2 disabled><input id=inner2i></fieldset></fieldset>
<fieldset disabled><legend>L<input id=leg1></legend></fieldset>

<div><fieldset disabled><optgroup id=r10><option id=r11>o</option></optgroup></fieldset><fieldset disabled><div><optgroup id=r12><option id=r13>o</option></optgroup></div></fieldset></div>
</body>"#;
}

#[cfg(test)]
mod target_tests {
    //! `:target` bounded opt-in: grammar pins, charged-resolution pins and the
    //! verbatim 7.4.6.4 truth table on the ENGINE-PARSED DOM. Matcher truth
    //! only - no paint/readiness/scroll/focus claim.
    use super::*;
    fn engine_nodes(html: &str) -> Vec<Node> {
        use html5ever::tendril::TendrilSink;
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
        parser.process(html.to_string().into());
        parser.finish().into_nodes()
    }
    fn find_by_id(nodes: &[Node], id: &str) -> Vec<Handle> {
        let mut out = Vec::new();
        for (h, n) in nodes.iter().enumerate() {
            if let Data::Element { attrs, .. } = &n.data {
                if attrs.iter().any(|(k, v)| &**k == "id" && v == id) {
                    out.push(h);
                }
            }
        }
        out
    }
    fn find_a_name(nodes: &[Node], name: &str) -> Vec<Handle> {
        let mut out = Vec::new();
        for (h, n) in nodes.iter().enumerate() {
            if let Data::Element { name: tag, attrs, .. } = &n.data {
                if &*tag.local == "a" && attrs.iter().any(|(k, v)| &**k == "name" && v == name) {
                    out.push(h);
                }
            }
        }
        out
    }
    const FIXTURE: &str = r#"<!doctype html><body>
<div id=a></div><p id=b></p>
<span id=dup>first</span><span id=dup>second</span>
<div id="café"></div><div id="sec%20x"></div><div id="%74op"></div><div id=top></div>
<a name=legacy></a><a name=dupname>first</a><a name=dupname>second</a>
</body>"#;
    #[test]
    fn target_resolution_truth_table_engine_parsed_dom() {
        let nodes = engine_nodes(FIXTURE);
        let cases: &[(&str, Option<Handle>)] = &[
            ("a", Some(find_by_id(&nodes, "a")[0])),
            ("missing", None),
            ("dup", Some(find_by_id(&nodes, "dup")[0])), // duplicate IDs: FIRST in tree order
            ("legacy", Some(find_a_name(&nodes, "legacy")[0])),
            ("caf\u{e9}", Some(find_by_id(&nodes, "caf\u{e9}")[0])),
            ("caf%C3%A9", Some(find_by_id(&nodes, "caf\u{e9}")[0])), // decode retry
            ("sec%2520x", Some(find_by_id(&nodes, "sec%20x")[0])), // decode once -> literal % id
            ("sec%20x", Some(find_by_id(&nodes, "sec%20x")[0])), // raw literal-% id wins
            ("sec%20y", None),                        // decodes to "sec y": no such id
            ("", None),                               // empty fragment => top => NULL
            ("top", Some(find_by_id(&nodes, "top")[0])), // raw ID wins BEFORE top fallback
            ("TOP", None),                            // no id TOP; decoded TOP ~ top => NULL
            ("%74op", Some(find_by_id(&nodes, "%74op")[0])), // literal %74op ID wins raw
            ("dupname", Some(find_a_name(&nodes, "dupname")[0])), // first a[name]
        ];
        for &(fragment, expected) in cases {
            let mut fuel = EXT_FUEL;
            let got = resolve_target_element(&nodes, &TargetState::Fragment(fragment.to_string()), &mut fuel)
                .unwrap_or_else(|e| panic!("resolution refused on {fragment:?}: {e:?}"));
            assert_eq!(got, expected, "fragment {fragment:?}");
        }
        // FragmentFreeCapture: never a target.
        let mut fuel = EXT_FUEL;
        assert_eq!(
            resolve_target_element(&nodes, &TargetState::FragmentFreeCapture, &mut fuel),
            Ok(None)
        );
    }
    #[test]
    fn target_decode_retry_hits_top_id_and_top_fallback_uses_decoded() {
        // EXPLICIT reviewer pin: fragment "%74op" on a DOM whose ONLY
        // candidate is id=top - raw scan misses, the decode-retry (7.4.6.4:
        // percent-decode + UTF-8-without-BOM, then find again) HITS id=top.
        let nodes = engine_nodes("<!doctype html><body><div id=top></div></body>");
        let top = find_by_id(&nodes, "top")[0];
        let mut fuel = EXT_FUEL;
        assert_eq!(
            resolve_target_element(&nodes, &TargetState::Fragment("%74op".into()), &mut fuel),
            Ok(Some(top)),
            "decode-retry must hit id=top"
        );
        // Complement: same fragment on a DOM with NO id=top and NO id=%74op:
        // raw miss, decode to "top", retry miss, then the ASCII-ci "top"
        // fallback (checked on the DECODED string) => NULL.
        let bare = engine_nodes("<!doctype html><body><div id=other></div></body>");
        let mut fuel = EXT_FUEL;
        assert_eq!(
            resolve_target_element(&bare, &TargetState::Fragment("%74op".into()), &mut fuel),
            Ok(None),
            "decoded 'top' fallback yields NULL target"
        );
    }
    #[test]
    fn target_resolution_is_charged_and_refuses_on_exhaustion() {
        // NEGATIVE CONTROL: resolution scans charge one fuel per visited
        // element per scan; a starved budget refuses (never a partial scan).
        let nodes = engine_nodes(FIXTURE);
        let mut fuel = 1;
        assert_eq!(
            resolve_target_element(&nodes, &TargetState::Fragment("zz-absent".into()), &mut fuel),
            Err(MatchRefusal::Fuel)
        );
        // Full scans with enough fuel answer and REPORT their consumption.
        // "zz-absent" (9 bytes) misses raw: 12 id + 12 a[name] visits, then
        // decode (charged DECODE_FUEL_PER_BYTE/byte = 6*9 = 54), then
        // decoded equals raw ("zz-absent" has no %) so NO decoded rescan -
        // the decode cost still applies but the rescan is skipped as
        // observably identical.
        let mut fuel = EXT_FUEL;
        let _ = resolve_target_element(&nodes, &TargetState::Fragment("zz-absent".into()), &mut fuel);
        // Exact composition, computed from the tree: per pass, 1 fuel per
        // element visit + attr byte-length per compare (id pass compares
        // every element's id; name pass compares only a elements' name);
        // plus 4 fuel per fragment byte for the decode. "zz-absent" has
        // no % so the decoded rescan is skipped (observably identical).
        let nelem = nodes
            .iter()
            .filter(|n| matches!(n.data, Data::Element { .. }))
            .count() as u64;
        let id_bytes: u64 = nodes
            .iter()
            .filter_map(|n| match &n.data {
                Data::Element { attrs, .. } => attrs
                    .iter()
                    .find(|(k, _)| &**k == "id")
                    .map(|(_, v)| v.len() as u64),
                _ => None,
            })
            .sum();
        let name_bytes: u64 = nodes
            .iter()
            .filter_map(|n| match &n.data {
                Data::Element { name: tag, attrs, .. } if &*tag.local == "a" => attrs
                    .iter()
                    .find(|(k, _)| &**k == "name")
                    .map(|(_, v)| v.len() as u64),
                _ => None,
            })
            .sum();
        assert_eq!(
            (EXT_FUEL - fuel) as u64,
            2 * nelem + id_bytes + name_bytes + (DECODE_FUEL_PER_BYTE as u64) * 9,
            "visits + conservative compared-byte pre-charge + charged decode"
        );
        // NEGATIVE CONTROL: decode refuses BEFORE any allocation when the
        // budget cannot cover DECODE_FUEL_PER_BYTE times the fragment
        // bytes - an empty tree (zero
        // scan cost) with fuel just under the decode cost.
        let empty = engine_nodes("<!doctype html><body></body>");
        let mut fuel = DECODE_FUEL_PER_BYTE * 9 - 1;
        assert_eq!(
            resolve_target_element(&empty, &TargetState::Fragment("zz-absent".into()), &mut fuel),
            Err(MatchRefusal::Fuel)
        );
        // NEGATIVE CONTROL: a caller fragment larger than the whole budget
        // refuses without scanning or allocating (no unbounded allocation).
        let huge = "x".repeat(EXT_FUEL as usize); // 6x this > budget
        let mut fuel = EXT_FUEL;
        assert_eq!(
            resolve_target_element(&empty, &TargetState::Fragment(huge), &mut fuel),
            Err(MatchRefusal::Fuel)
        );
        // COMPARE-CHARGE CONTROLS: long matching prefix (compare cost is
        // real, attr-len pre-charge applies), raw hit, and decoded retry.
        let pre = engine_nodes(
            "<!doctype html><body><div id=prefixmatch-aaaaaaaa></div><a name=prefixmatch-aaaaaaaa></a></body>",
        );
        // (1) long common prefix, differs at last byte: miss; charged
        // visit + FULL id byte length before the comparison happened.
        // fuel sized to exhaust exactly AT the long-prefix compare:
        // visits for html/head/body/div (4) + full id byte length (20) - 1.
        let mut fuel = 4 + 20 - 1;
        assert_eq!(
            resolve_target_element(
                &pre,
                &TargetState::Fragment("prefixmatch-aaaaaaab".into()),
                &mut fuel
            ),
            Err(MatchRefusal::Fuel)
        );
        // (2) same fragment, enough fuel: miss (Ok(None)), NOT a hit.
        let mut fuel = EXT_FUEL;
        assert_eq!(
            resolve_target_element(
                &pre,
                &TargetState::Fragment("prefixmatch-aaaaaaab".into()),
                &mut fuel
            ),
            Ok(None)
        );
        // (3) raw hit under compare charging: exact fragment resolves.
        let want = find_by_id(&pre, "prefixmatch-aaaaaaaa")[0];
        let mut fuel = EXT_FUEL;
        assert_eq!(
            resolve_target_element(
                &pre,
                &TargetState::Fragment("prefixmatch-aaaaaaaa".into()),
                &mut fuel
            ),
            Ok(Some(want))
        );
        // SEMANTICS PIN: the two-pass exact-capacity decode is
        // byte-identical to from_utf8_lossy on valid, invalid and
        // truncated sequences (incl. consecutive errors).
        for raw in [
            &b"caf\xc3\xa9"[..],
            &b"a\xffb"[..],
            &b"a\xff\xfeb"[..],
            &b"ok\xc3"[..],
            &b"\xe2\x82"[..],
            &b"\xef\xbb\xbfbom-kept"[..],
        ] {
            assert_eq!(
                utf8_lossy_exact(raw),
                String::from_utf8_lossy(raw).into_owned()
            );
        }
        // (4) decoded retry under compare charging: %70refixmatch-aaaaaaaa
        // decodes to the id and resolves via the retry pass.
        let mut fuel = EXT_FUEL;
        assert_eq!(
            resolve_target_element(
                &pre,
                &TargetState::Fragment("%70refixmatch-aaaaaaaa".into()),
                &mut fuel
            ),
            Ok(Some(want))
        );
    }
    #[test]
    fn target_selector_match_and_no_assertion_unresolved() {
        let nodes = engine_nodes(FIXTURE);
        let a = find_by_id(&nodes, "a")[0];
        let b = find_by_id(&nodes, "b")[0];
        let sel = selector_ext("#a:target").unwrap();
        assert_eq!(
            sel.matches_checked_target(&nodes, a, &TargetState::Fragment("a".into())),
            Some(true)
        );
        assert_eq!(
            sel.matches_checked_target(&nodes, a, &TargetState::Fragment("b".into())),
            Some(false)
        );
        assert_eq!(
            sel.matches_checked_target(&nodes, a, &TargetState::FragmentFreeCapture),
            Some(false)
        );
        // No assertion at all: UNRESOLVED, never a guess.
        let mut fuel = EXT_FUEL;
        assert_eq!(
            sel.matches_checked_scoped_budget(&nodes, a, &mut fuel, None, None),
            Ok(None)
        );
        assert_eq!(
            sel.matches_checked_target(&nodes, b, &TargetState::Fragment("a".into())),
            Some(false)
        );
    }
    #[test]
    fn target_grammar_pins() {
        assert!(selector_ext("p:target").is_some());
        assert!(selector_ext(":target").is_some());
        assert!(selector_ext(".citation:target").is_some());
        assert!(selector_ext(":target .mw-cite-up-arrow").is_some());
        assert!(selector_ext("p:target:target").is_none()); // repetition refuses
        assert!(selector_ext("p:target(1)").is_none()); // functional form refuses
        assert!(selector_ext("p:target:enabled").is_some()); // combines with enablement
    }
    #[test]
    fn base_entry_still_refuses_target() {
        assert!(super::selector("p:target").is_none());
        assert!(super::selector(":target").is_none());
    }
}

#[cfg(test)]
mod target_walk_tests {
    //! Sheet-level enrollment + walk pins through the real state entry.
    use super::*;
    const RED: Option<u32> = Some(0xff_00_00);
    fn tree_with(html: &str, snap: &crate::SnapshotState) -> crate::style_tree::StyleTree {
        crate::parse_style_tree_with_state(
            html.as_bytes(),
            &[],
            &crate::Limits::DEFAULT,
            None,
            crate::SupplyBudget::CANONICAL,
            snap,
        )
    }
    fn reds(tree: &crate::style_tree::StyleTree, tag: &str) -> Vec<bool> {
        tree.nodes
            .iter()
            .filter(|n| n.tag == tag)
            .map(|n| n.style.background_color == RED)
            .collect()
    }
    const PAGE: &str = "<style>#a:target{background-color:#ff0000}#b:target{background-color:#ff0000}</style><div id=a></div><div id=b></div>";
    #[test]
    fn fragment_assertion_enrolls_and_matches_resolved_target_only() {
        let snap = crate::SnapshotState {
            target: Some(TargetState::Fragment("a".into())),
            ..Default::default()
        };
        let tree = tree_with(PAGE, &snap);
        assert!(!tree.unsupported && !tree.truncated);
        assert_eq!(reds(&tree, "div"), vec![true, false]);
    }
    #[test]
    fn fragment_free_assertion_enrolls_and_matches_nothing() {
        let snap = crate::SnapshotState {
            target: Some(TargetState::FragmentFreeCapture),
            ..Default::default()
        };
        let tree = tree_with(PAGE, &snap);
        assert!(!tree.unsupported && !tree.truncated);
        assert_eq!(reds(&tree, "div"), vec![false, false]);
    }
    #[test]
    fn no_assertion_fails_closed_at_enrollment() {
        // Snapshot present but NO target guarantee: :target refuses exactly
        // like an unparseable selector (page incomplete).
        let tree = tree_with(PAGE, &crate::SnapshotState::default());
        assert!(tree.unsupported);
        // No snapshot entry at all also refuses.
        let plain = crate::parse_style_tree_with_inline_sheets_and_selectors(
            PAGE.as_bytes(),
            &[],
            &crate::Limits::DEFAULT,
            None,
            crate::SupplyBudget::CANONICAL,
        );
        assert!(plain.unsupported);
    }
    #[test]
    fn focus_guards_unchanged_by_target_assertion() {
        // :focus stays fail-closed under a target-only assertion (no cross
        // inference): a page with BOTH rules is incomplete because the focus
        // rule refuses, while the :target rule alone would have enrolled.
        let html = "<style>#a:target{background-color:#ff0000}#a:focus{border-color:#ff0000}</style><div id=a></div>";
        let snap = crate::SnapshotState {
            target: Some(TargetState::Fragment("a".into())),
            ..Default::default()
        };
        let tree = tree_with(html, &snap);
        assert!(tree.unsupported);
    }
}

#[cfg(test)]
mod not_bounded_tests {
    use super::*;
    fn nodes(html: &str) -> Vec<Node> {
        use html5ever::tendril::TendrilSink;
        let sink = crate::dom::Sink::new(64 * 1024 * 1024);
        let mut parser = html5ever::parse_document(sink, html5ever::ParseOpts {
            tree_builder: html5ever::tree_builder::TreeBuilderOpts {
                scripting_enabled: false, ..Default::default()
            }, ..Default::default()
        });
        parser.process(html.to_string().into());
        parser.finish().into_nodes()
    }
    fn id(ns: &[Node], value: &str) -> Handle {
        ns.iter().enumerate().find(|(_,n)| match &n.data {
            Data::Element {attrs,..} => attrs.iter().any(|(k,v)| &**k=="id" && v==value),
            _ => false,
        }).unwrap().0
    }
    #[test]
    fn not_specificity_replaces_token_and_counts_every_argument_piece() {
        for (text, want) in [
            (".a:not(.b)", (0,2,0)), ("p:not(#x)",(1,0,1)),
            (":not(p)",(0,0,1)), (":not(*)",(0,0,0)),
            (":not([x])",(0,1,0)), (":not(:root)",(0,1,0)),
            (":not(:first-child:first-child)",(0,2,0)),
            (":not(:nth-child(2n+1))",(0,1,0)),
            (":not(:hover)",(0,1,0)), ("button:not(:enabled)",(0,1,1)),
            (":not(:disabled)",(0,1,0)), (":not(:target)",(0,1,0)),
            (".a:not(#x.b[x]:root:first-child:enabled:target)",(1,7,0)),
            (":not(.a):not(.b)",(0,2,0)),
        ] { assert_eq!(selector_ext(text).unwrap().specificity, want, "{text}"); }
    }
    #[test]
    fn not_bounds_base_refusal_and_separate_inner_state_budget() {
        for bad in [":not()", ":not( )", ":not(.a,.b)", ":not(.a .b)",
            ":not(.a>.b)", ":not(:not(.a))", ":not(::before)",
            ":not(.a::after)", ":not(:is(.a))", ":not(:has(.a .b))",
            ":not(:hover:focus)", ":focus:not(:focus-visible):hover",
            ":not(.a\\:focus)", ":not([x i])"] {
            assert!(selector_ext(bad).is_none(), "{bad}");
        }
        for good in [":not( .a )", ":not([x=')'])", ":not(:nth-child(2n + 1))",
            ":focus:not(:focus-visible)", ":not(.a):not(.b)"] {
            assert!(selector_ext(good).is_some(), "{good}");
            assert!(selector(good).is_none(), "base must refuse {good}");
        }
        let eight = format!("p{}", ":not(.a)".repeat(8));
        assert_eq!(selector_ext(&eight).unwrap().specificity,(0,8,1));
        assert!(selector_ext(&(eight.clone()+":not(.b)")).is_none());
        assert!(selector_ext(&(eight+":first-child")).is_none());
        assert!(selector_ext(&format!(":not({})", ".a".repeat(9))).is_none());
    }
    #[test]
    fn not_three_valued_and_recursive_enrollment_dependencies() {
        let ns=nodes("<!doctype html><p id=x></p>"); let h=id(&ns,"x");
        let sel=selector_ext(":not(:hover)").unwrap();
        assert!(sel.has_state());
        assert!(!sel.state_satisfied_by(None));
        assert_eq!(sel.matches_checked(&ns,h),None,"unknown never inverted");
        let snap=SnapshotState {history:None,input:Some(InputState::QuiescentCapture),focus:None,target:None};
        let mut fuel=EXT_FUEL;
        assert_eq!(sel.matches_checked_scoped_budget_with_snapshot(&ns,h,&mut fuel,None,None,Some(&snap)),Ok(Some(true)));
        let target=selector_ext(":not(:target)").unwrap();
        assert!(target.has_state()); assert!(!target.state_satisfied_by(Some(&snap)));
        assert_eq!(target.matches_checked(&ns,h),None);
        assert_eq!(target.matches_checked_target(&ns,h,&TargetState::Fragment("x".into())),Some(false));
        assert_eq!(target.matches_checked_target(&ns,h,&TargetState::FragmentFreeCapture),Some(true));
        let both=selector_ext(":hover:not(:focus-visible)").unwrap();
        assert!(!both.state_satisfied_by(Some(&snap)),"all inner/outer deps required");
    }
    #[test]
    fn not_submatch_fuel_attr_and_structural_exhaustion_never_inverted() {
        let ns=nodes(&format!("<!doctype html><div><p id=x data-x='{}'></p><p></p></div>","a".repeat(1000)));
        let h=id(&ns,"x");
        let attr=selector_ext(":not([data-x*=aaaaab])").unwrap();
        let mut fuel=EXT_FUEL;
        assert_eq!(attr.matches_checked_scoped_budget(&ns,h,&mut fuel,None,None),Err(MatchRefusal::Fuel));
        assert_eq!(fuel,0);
        let ns=nodes("<!doctype html><div><p></p><p></p><p></p></div>");
        let h=ns.iter().position(|n|matches!(&n.data,Data::Element {name,..} if &*name.local=="p")).unwrap();
        let structural=selector_ext(":not(:last-child)").unwrap();
        let mut fuel=3; // outer+inner steps and first sibling; reverse scan exhausts
        assert_eq!(structural.matches_checked_scoped_budget(&ns,h,&mut fuel,None,None),Err(MatchRefusal::Fuel));
        assert_eq!(structural.matches_checked(&ns,h),Some(true));
    }
    #[test]
    fn not_inner_attribute_structural_enablement_truth() {
        let ns=nodes("<!doctype html><div id=d><p id=x class='a b' data-x='one two-en'>x</p><button id=y disabled></button></div>");
        let x=id(&ns,"x"); let y=id(&ns,"y");
        for (selector,want) in [
            (":not(.a)",false), (":not(.c)",true), (":not(#x)",false),
            (":not(p)",false), (":not(span)",true), (":not(*)",false),
            (":not([absent])",true), (":not([data-x])",false),
            (":not([data-x='one two-en'])",false),
            (":not([data-x~=one])",false), (":not([data-x|=one])",true),
            (":not([data-x^=one])",false), (":not([data-x$=en])",false),
            (":not([data-x*=two])",false), (":not(:root)",true),
            (":not(:first-child)",false), (":not(:last-child)",true),
            (":not(:only-child)",true), (":not(:nth-child(odd))",false),
            (":not(:nth-child(even))",true), (":not(:enabled)",true),
            (":not(:disabled)",true), // p matches neither -> both negations true
        ] { assert_eq!(selector_ext(selector).unwrap().matches_checked(&ns,x),Some(want),"{selector}"); }
        assert_eq!(selector_ext(":not(:disabled)").unwrap().matches_checked(&ns,y),Some(false));
        assert_eq!(selector_ext(":not(:enabled)").unwrap().matches_checked(&ns,y),Some(true));
        let ns=nodes("<!doctype html><a id=x href=x>x</a>");let x=id(&ns,"x");
        let link=selector_ext(":not(:link)").unwrap();
        assert_eq!(link.matches_checked(&ns,x),None);
        let snap=SnapshotState {history:Some(HistoryState::FreshNoHistory),input:None,focus:None,target:None};
        let mut fuel=EXT_FUEL;
        assert_eq!(link.matches_checked_scoped_budget_with_snapshot(&ns,x,&mut fuel,None,None,Some(&snap)),Ok(Some(false)));
    }
    #[test]
    fn not_charging_exact_work_short_circuits_and_ignores_unread_values() {
        let ns=nodes("<!doctype html><p id=x class='aa bb'></p>");let h=id(&ns,"x");
        // Lookup: id iteration1 (length mismatch); class iteration1 +5 bytes.
        // Split scans aa +space (3), first token comparison (2). Total12.
        let arg=selector_ext(":not(.aa)").unwrap();
        let mut fuel=100;
        assert!(arg.steps[0].0.not_args[0].steps[0].0.matches_not_charged(&ns,h,&mut fuel).unwrap());
        assert_eq!(100-fuel,12);
        let huge="z".repeat(2000);
        let ns=nodes(&format!("<!doctype html><p id=x title='{huge}' class=aa></p>"));let h=id(&ns,"x");
        let mut fuel=100;
        assert!(arg.steps[0].0.not_args[0].steps[0].0.matches_not_charged(&ns,h,&mut fuel).unwrap());
        // title's VALUE was never inspected; its lookup iteration and first
        // mismatching name byte are charged (same name length as class).
        assert_eq!(100-fuel,13);
        // First false conjunct stops: the later expensive substring does not run.
        let ns=nodes(&format!("<!doctype html><p id=x title='{huge}'></p>"));let h=id(&ns,"x");
        let sel=selector_ext(":not(.absent[title*=zzzzzy])").unwrap();
        let mut fuel=40;
        assert_eq!(sel.matches_checked_scoped_budget(&ns,h,&mut fuel,None,None),Ok(Some(true)));
        assert!(fuel>0);
    }
    #[test]
    fn not_charged_attribute_predicates_match_legacy_truth_without_allocations() {
        let ns=nodes("<!doctype html><p id=x data-x='one two-en' type='TeXT' class='aa&#11;bb'></p>");
        let h=id(&ns,"x");
        for text in ["[data-x]","[missing]","[data-x='one two-en']","[data-x=z]",
            "[data-x~=one]","[data-x~=two-en]","[data-x|=one]","[data-x^=one]",
            "[data-x$=en]","[data-x*=two]","[data-x*='']","[type=text]",
            "[type^=TE]","[type$=XT]","[type*=EX]","[data-x~=aa]", "[DATA-X]"] {
            let sel=selector_ext(text).unwrap();let attr=&sel.steps[0].0.attrs[0];
            let mut fuel=EXT_FUEL;
            assert_eq!(attr.matches_not_charged(&ns,h,&mut fuel),Ok(attr.matches(&ns,h)),"{text}");
        }
        // CSS ASCII whitespace excludes vertical tab. No widening to Rust's
        // is_ascii_whitespace set in the new byte-split implementation.
        assert_eq!(selector_ext(":not(.aa)").unwrap().matches_checked(&ns,h),Some(true));
    }
    #[test]
    fn not_unknown_left_search_steps_propagate_and_resolved_matches_succeed() {
        let ns=nodes("<!doctype html><div><p id=x></p></div><div></div><p id=y></p>");
        for (text,want_id) in [("div:not(:hover) p","x"),("div:not(:hover) ~ p","y"),
            ("div:not(:hover) + p","y"),("div:not(:hover) > p","x")] {
            assert_eq!(selector_ext(text).unwrap().matches_checked(&ns,id(&ns,want_id)),None,"{text}");
        }
        // Explicit target context yields proven true candidates.
        let ns=nodes("<!doctype html><div><a href=x><p id=x></p></a></div><div></div><a href=x></a><p id=y></p>");
        // Ordinary false candidate path remains a proven false.
        assert_eq!(selector_ext("aside:not(:hover) p").unwrap().matches_checked(&ns,id(&ns,"x")),Some(false));
        // Explicit resolved target shared by every scanned candidate.
        let sel=selector_ext(":not(:target) p").unwrap();
        let mut fuel=EXT_FUEL;
        assert_eq!(sel.matches_checked_scoped_budget(&ns,id(&ns,"x"),&mut fuel,None,Some(None)),Ok(Some(true)));
        let mut fuel=1;
        assert_eq!(sel.matches_checked_scoped_budget(&ns,id(&ns,"x"),&mut fuel,None,Some(None)),Err(MatchRefusal::Fuel));
        assert_eq!(selector_ext("aside:not(:hover) ~ p").unwrap().matches_checked(&ns,id(&ns,"y")),Some(false));
        // Unknown paths still share one finite fuel counter across candidates.
        let sel=selector_ext("div:not(:hover) ~ p").unwrap();
        let mut fuel=2;
        assert_eq!(sel.matches_checked_scoped_budget(&ns,id(&ns,"y"),&mut fuel,None,None),Err(MatchRefusal::Fuel));
        assert_eq!(fuel,0);
        // An earlier-unknown/later-true fixture is not representable by the
        // admitted state model: missing assertions are uniform per selector,
        // and every successful path would have to evaluate the same unknown
        // predicate. The any-true priority is source-verified, not exercised
        // by the separate resolved-target fixture above.

    }
    #[test]
    #[ignore = "measurement: NOT_LINES path supplies original 542-line inventory"]
    fn not_parser_inventory() {
        let path=std::env::var("NOT_LINES").unwrap();
        let lines=std::fs::read_to_string(path).unwrap();
        let mut accepted=0;let mut refused=0;
        for (ix,line) in lines.lines().enumerate() {
            let ok=selector_ext(line.trim()).is_some();
            if ok {accepted+=1;}else {refused+=1;}
            println!("NOT-PARSER line={} admitted={} selector={}",ix+1,ok,line);
        }
        println!("NOT-PARSER total={} admitted={} refused={}",accepted+refused,accepted,refused);
    }
}

#[cfg(test)]
mod of_type_bounded_tests {
 use super::*;
 fn nodes(html:&str)->Vec<Node>{
  use html5ever::tendril::TendrilSink;
  let sink=crate::dom::Sink::new(64*1024*1024);
  let mut parser=html5ever::parse_document(sink,html5ever::ParseOpts {
   tree_builder:html5ever::tree_builder::TreeBuilderOpts{scripting_enabled:false,..Default::default()},..Default::default()
  });parser.process(html.to_string().into());parser.finish().into_nodes()
 }
 fn id(ns:&[Node],value:&str)->Handle{
  ns.iter().position(|n|matches!(&n.data,Data::Element {attrs,..} if attrs.iter().any(|(k,v)|&**k=="id" && v==value))).unwrap()
 }
 #[test]
 fn typed_html_and_foreign_oracle_21_rows(){
  let ns=nodes(include_str!("../tests/fixtures/of_type_oracle.html"));
  let expected=include_str!("../tests/fixtures/of_type_oracle_expected.tsv");
  let sels=[":first-of-type",":last-of-type",":only-of-type",":nth-of-type(2)",":nth-of-type(odd)",":not(:first-of-type)"];
  let mut rows=0;
  for line in expected.lines(){let vals:Vec<_>=line.split('\t').collect();let h=id(&ns,vals[0]);
   for (i,text) in sels.iter().enumerate(){
    assert_eq!(selector_ext(text).unwrap().matches_checked(&ns,h),Some(vals[i+1]=="1"),"{} {}",vals[0],text);
   }rows+=1;
  }assert_eq!(rows,21);
 }
 #[test]
 fn typed_synthetic_namespace_prefix_and_exact_case_not_html_parse_claim(){
  let mut ns=nodes("<!doctype html><div id=parent><x id=x1></x><x id=x2></x><x id=x3></x><x id=x4></x><x id=x5></x></div>");
  for (label,uri,local,prefix) in [("x1","urn:a","x","a"),("x2","urn:b","x","b"),("x3","urn:a","x","a"),("x4","urn:a","X","a"),("x5","urn:a","x","c")] {
   let h=id(&ns,label);if let Data::Element{name,..}=&mut ns[h].data{*name=html5ever::QualName::new(Some(prefix.into()),uri.into(),local.into());}
  }
  for (label,first,last,only,nth2) in [("x1",true,false,false,false),("x2",true,true,true,false),("x3",false,false,false,true),("x4",true,true,true,false),("x5",false,true,false,false)] {
   let h=id(&ns,label);
   for (text,want) in [(":first-of-type",first),(":last-of-type",last),(":only-of-type",only),(":nth-of-type(2)",nth2)]{
    assert_eq!(selector_ext(text).unwrap().matches_checked(&ns,h),Some(want),"{label} {text}");
   }
  }
 }
 #[test]
 fn typed_specificity_bounds_base_refusal_and_anb(){
  for text in ["p:first-of-type","p:last-of-type","p:only-of-type","p:nth-of-type(2n+1)"]{
   assert_eq!(selector_ext(text).unwrap().specificity,(0,1,1));assert!(selector(text).is_none());
  }
  assert_eq!(selector_ext(":not(:first-of-type)").unwrap().specificity,(0,1,0));
  assert_eq!(selector_ext("*:first-of-type:first-of-type").unwrap().specificity,(0,2,0));
  assert!(selector_ext(&format!("p{}",":first-of-type".repeat(8))).is_some());
  assert!(selector_ext(&format!("p{}",":first-of-type".repeat(9))).is_none());
  for text in [":nth-of-type(1025n)",":nth-of-type(n+65536)",":nth-last-of-type(2)",":nth-of-type(2 of p)"]{assert!(selector_ext(text).is_none(),"{text}");}
  let ns=nodes("<!doctype html><div><p id=a></p>text<span></span><!--c--><p id=b></p><p id=c></p></div>");
  for (text,wants) in [(":nth-of-type(-n+2)",[true,true,false]),(":nth-of-type(0)",[false,false,false]),(":nth-of-type(even)",[false,true,false])]{
   for (label,want) in ["a","b","c"].iter().zip(wants){assert_eq!(selector_ext(text).unwrap().matches_checked(&ns,id(&ns,label)),Some(want));}
  }
 }
 #[test]
 fn typed_scan_cost_reuse_position_and_left_unknown(){
  let ns=nodes("<!doctype html><div><span></span>text<p id=x></p><p id=y></p></div>");let x=id(&ns,"x");
  let sel=selector_ext(":nth-of-type(1):nth-of-type(odd)").unwrap();let mut fuel=EXT_FUEL;
  assert_eq!(sel.matches_checked_scoped_budget(&ns,x,&mut fuel,Some((999,99)),None),Ok(Some(true)),"all-element position never reused");
  let cost=EXT_FUEL-fuel;let mut fuel2=EXT_FUEL;
  assert_eq!(selector_ext(":nth-of-type(1)").unwrap().matches_checked_scoped_budget(&ns,x,&mut fuel2,None,None),Ok(Some(true)));
  assert_eq!(EXT_FUEL-fuel2,cost,"repeated formulas reuse same charged typed census");
  // Malformed direct-node membership left of a descendant remains unknown.
  let mut ns=nodes("<!doctype html><div id=d><p id=x></p></div>");let d=id(&ns,"d");let x=id(&ns,"x");let parent=ns[d].parent.unwrap();ns[parent].children.retain(|&h|h!=d);
  assert_eq!(selector_ext("div:first-of-type p").unwrap().matches_checked(&ns,x),None);
 }
 #[test]
 fn typed_actual_cost_non_elements_and_ns_local_bytes(){
  let ns=nodes("<!doctype html><div><!--comment-->text<p id=x></p></div>");let x=id(&ns,"x");let parent=ns[x].parent.unwrap();
  let mut fuel=EXT_FUEL;
  assert_eq!(typed_position_charged(&ns,x,false,&mut fuel),Ok(Some((1,1))));
  // Raw comment+text+element visits, plus exact HTML namespace and p bytes.
  let Data::Element{name,..}=&ns[x].data else{panic!()};
  assert_eq!(EXT_FUEL-fuel,ns[parent].children.len() as u32+name.ns.len() as u32+name.local.len() as u32);
  let mut modified=nodes("<!doctype html><div><!--comment-->text<p id=x></p></div>");let x=id(&modified,"x");if let Data::Element{name,..}=&mut modified[x].data{name.ns="a".repeat(5000).into();}
  let mut fuel=EXT_FUEL;
  assert_eq!(typed_position_charged(&modified,x,false,&mut fuel),Err(MatchRefusal::Fuel),"self namespace compare is real charged work");
 }
 #[test]
 fn typed_detached_inconsistent_oversize_fuel_and_negation(){
  let mut ns=nodes("<!doctype html><div><p id=x></p><p id=y></p></div>");let x=id(&ns,"x");let y=id(&ns,"y");
  let sel=selector_ext(":not(:first-of-type)").unwrap();let mut fuel=1;
  assert_eq!(sel.matches_checked_scoped_budget(&ns,x,&mut fuel,None,None),Err(MatchRefusal::Fuel));
  let parent=ns[x].parent.unwrap();ns[parent].children.retain(|&h|h!=x);
  assert_eq!(selector_ext(":nth-of-type(1)").unwrap().matches_checked(&ns,x),None);
  assert_eq!(sel.matches_checked(&ns,x),None,"inconsistent census never inverted");
  ns[x].parent=None;
  assert_eq!(selector_ext(":only-of-type").unwrap().matches_checked(&ns,x),Some(true));
  ns[parent].children=vec![y;EXT_MAX_SIBLINGS+1];let mut fuel=EXT_FUEL;
  assert_eq!(selector_ext(":nth-of-type(1)").unwrap().matches_checked_scoped_budget(&ns,y,&mut fuel,None,None),Err(MatchRefusal::OversizeSiblings));
  assert_eq!(EXT_FUEL-fuel,1,"gate before traversal, only outer step charged");
 }
}

#[cfg(test)]
mod reverse_child_bounded_tests {
 use super::*;
 fn nodes(html:&str)->Vec<Node>{
  use html5ever::tendril::TendrilSink;
  let sink=crate::dom::Sink::new(64*1024*1024);
  let mut parser=html5ever::parse_document(sink,html5ever::ParseOpts {tree_builder:html5ever::tree_builder::TreeBuilderOpts{scripting_enabled:false,..Default::default()},..Default::default()});
  parser.process(html.to_string().into());parser.finish().into_nodes()
 }
 fn id(ns:&[Node],value:&str)->Handle{ns.iter().position(|n|matches!(&n.data,Data::Element{attrs,..} if attrs.iter().any(|(k,v)|&**k=="id" && v==value))).unwrap()}
 #[test]
 fn browser_parsed_html_mixed_type_namespace_truth_21rows(){
  let ns=nodes(include_str!("../tests/fixtures/reverse_child_oracle.html"));let expected=include_str!("../tests/fixtures/reverse_child_oracle_expected.tsv");
  let sels=[":nth-last-child(1)",":nth-last-child(2)",":nth-last-child(odd)",":nth-last-child(even)",":nth-last-child(-n+2)",":nth-last-child(0)",":first-child:nth-last-child(2)",":not(:nth-last-child(2))"];
  let mut count=0;for line in expected.lines(){let vals:Vec<_>=line.split('\t').collect();let h=id(&ns,vals[0]);for (i,text) in sels.iter().enumerate(){assert_eq!(selector_ext(text).unwrap().matches_checked(&ns,h),Some(vals[i+1]=="1"),"{} {}",vals[0],text);}count+=1;}assert_eq!(count,21);
 }
 #[test]
 fn specificity_bounds_formula_reuse_and_wrong_forward_position(){
  assert_eq!(selector_ext("p:nth-last-child(2)").unwrap().specificity,(0,1,1));assert_eq!(selector_ext(":not(:nth-last-child(2))").unwrap().specificity,(0,1,0));
  assert!(selector("p:nth-last-child(2)").is_none());
  assert!(selector_ext(&format!("p{}",":nth-last-child(1)".repeat(8))).is_some());assert!(selector_ext(&format!("p{}",":nth-last-child(1)".repeat(9))).is_none());
  for text in ["p:nth-last-child(1025n)","p:nth-last-child(n+65536)","p:nth-last-child(2 of p)","p:nth-last-of-type(2)"]{assert!(selector_ext(text).is_none(),"{text}");}
  let ns=nodes("<!doctype html><div><p id=x></p><span></span>text<!--c--></div>");let x=id(&ns,"x");let mut fuel=EXT_FUEL;
  assert_eq!(selector_ext(":nth-last-child(2):nth-last-child(even)").unwrap().matches_checked_scoped_budget(&ns,x,&mut fuel,Some((99,999)),None),Ok(Some(true)));
  let cost=EXT_FUEL-fuel;let mut f=EXT_FUEL;assert_eq!(selector_ext(":nth-last-child(2)").unwrap().matches_checked_scoped_budget(&ns,x,&mut f,None,None),Ok(Some(true)));assert_eq!(cost,EXT_FUEL-f);
 }
 #[test]
 fn synthetic_namespace_must_not_filter_all_element_reverse_census(){
  let mut ns=nodes("<!doctype html><div><p id=x></p><span id=y></span></div>");let x=id(&ns,"x");let y=id(&ns,"y");if let Data::Element{name,..}=&mut ns[y].data{*name=html5ever::QualName::new(Some("z".into()),"urn:other".into(),"p".into());}
  assert_eq!(selector_ext(":nth-last-child(2)").unwrap().matches_checked(&ns,x),Some(true));
 }
 #[test]
 fn exact_raw_cost_detached_unknown_failed_search_oversize_and_negation(){
  let mut ns=nodes("<!doctype html><div><p id=x></p><span></span>text<!--c--></div>");let x=id(&ns,"x");let parent=ns[x].parent.unwrap();let n=ns[parent].children.len();let mut fuel=EXT_FUEL;
  assert_eq!(nth_last_index_charged(&ns,x,&mut fuel),Ok(Some(2)));assert_eq!(EXT_FUEL-fuel,n as u32,"all raw visits charged");
  ns[parent].children.retain(|&h|h!=x);let mut fuel=EXT_FUEL;assert_eq!(nth_last_index_charged(&ns,x,&mut fuel),Ok(None));assert_eq!(EXT_FUEL-fuel,(n-1) as u32);
  assert_eq!(selector_ext(":not(:nth-last-child(1))").unwrap().matches_checked(&ns,x),None);
  ns[x].parent=None;assert_eq!(selector_ext(":nth-last-child(1)").unwrap().matches_checked(&ns,x),Some(true));
  ns[x].parent=Some(parent);ns[parent].children=vec![x;EXT_MAX_SIBLINGS+1];let mut fuel=EXT_FUEL;assert_eq!(selector_ext(":not(:nth-last-child(1))").unwrap().matches_checked_scoped_budget(&ns,x,&mut fuel,None,None),Err(MatchRefusal::OversizeSiblings));assert_eq!(EXT_FUEL-fuel,2,"outer+not substep only, gate before traversal");
 }
 #[test]
 fn cumulative_scans_fuel_and_left_unknown(){
  let ns=nodes("<!doctype html><div><p id=x></p><span></span>text<!--c--></div>");let x=id(&ns,"x");let mut fuel=8;
  assert_eq!(selector_ext(":nth-last-child(2):not(:nth-last-child(1))").unwrap().matches_checked_scoped_budget(&ns,x,&mut fuel,None,None),Err(MatchRefusal::Fuel));
  let mut ns=nodes("<!doctype html><div id=d><p id=x></p></div>");let d=id(&ns,"d");let x=id(&ns,"x");let p=ns[d].parent.unwrap();ns[p].children.retain(|&h|h!=d);
  assert_eq!(selector_ext("div:nth-last-child(1) p").unwrap().matches_checked(&ns,x),None);
 }
}

#[cfg(test)]
mod where_bounded_tests {
 use super::*;
 fn nodes(html:&str)->Vec<Node>{use html5ever::tendril::TendrilSink;let sink=crate::dom::Sink::new(64*1024*1024);let mut p=html5ever::parse_document(sink,html5ever::ParseOpts{tree_builder:html5ever::tree_builder::TreeBuilderOpts{scripting_enabled:false,..Default::default()},..Default::default()});p.process(html.to_string().into());p.finish().into_nodes()}
 fn id(ns:&[Node],v:&str)->Handle{ns.iter().position(|n|matches!(&n.data,Data::Element{attrs,..} if attrs.iter().any(|(k,a)|&**k=="id" && a==v))).unwrap()}
 #[test]
 fn browser_actual_parsed_match_oracle_5rows_16selectors(){
  let ns=nodes(include_str!("../tests/fixtures/where_match_oracle.html"));let expected=include_str!("../tests/fixtures/where_match_oracle_expected.tsv");
  let sels=[":where(#a)",":where(p.x)",":where(:not(.no))",":not(:where(.no))",":where(.x):where(#a)",":where(:first-of-type)",":where(:nth-last-child(2))","div:where(#d) > p","div:where(:not(.no)) p",":where(#a) + span",":where(#a) ~ p",":where(:link)",":where(:hover)",":where(:focus)",":where(:target)",":not(:where(:hover))"];
  let snap=SnapshotState{history:Some(HistoryState::FreshNoHistory),input:Some(InputState::QuiescentCapture),focus:Some(FocusState::UnfocusedInitial),target:Some(TargetState::FragmentFreeCapture)};
  let mut count=0;for line in expected.lines(){let vals:Vec<_>=line.split('\t').collect();let h=id(&ns,vals[0]);for (i,t) in sels.iter().enumerate(){let mut f=EXT_FUEL;assert_eq!(selector_ext(t).unwrap().matches_checked_scoped_budget_with_snapshot(&ns,h,&mut f,None,Some(None),Some(&snap)),Ok(Some(vals[i+1]=="1")),"{} {}",vals[0],t);}count+=1;}assert_eq!(count,5);
 }
 #[test]
 fn zero_rank_all_arguments_cross_wrappers_and_bounds(){
  for t in [":where(#x)",":where(p.a)",":where(:not(#x))",":not(:where(#x))",":where(.a):where(#x)"]{assert_eq!(selector_ext(t).unwrap().specificity,(0,0,0),"{t}");assert!(selector(t).is_none());}
  assert_eq!(selector_ext("p.a:where(#x):not(.b)").unwrap().specificity,(0,2,1));
  for bad in [":where()",":where(.a,.b)",":where(p .a)",":where(::before)",":where(:where(.a))",":not(:where(:not(.a)))",":where(:not(:where(.a)))",":where(:has(.a .b))"]{assert!(selector_ext(bad).is_none(),"bounded refusal, not invalid CSS:{bad}");}
  assert!(selector_ext(&":where(.a)".repeat(8)).is_some());assert!(selector_ext(&":where(.a)".repeat(9)).is_none());assert!(selector_ext(&format!(":where({})",".a".repeat(9))).is_none());
 }
 #[test]
 fn recursive_deps_preflight_before_false_and_pseudo_guard(){
  let ns=nodes("<!doctype html><p id=x></p>");let h=id(&ns,"x");
  for t in [".absent:where(:hover)",":where(:not(:focus-visible))",":not(:where(:target))",".absent:where(:link)"]{let s=selector_ext(t).unwrap();assert!(s.has_state());assert!(!s.state_satisfied_by(None));assert_eq!(s.matches_checked(&ns,h),None,"preflight:{t}");}
  for t in ["p:where(:hover)::before","p:not(:where(:target))::after"]{assert!(selector_ext(t).is_none(),"outer pseudo guard:{t}");}
  let s=selector_ext(":where(:link):not(:where(:focus))");assert!(s.is_some(),"separate sibling argument paths may each contain where");
 }
 #[test]
 fn same_resolved_target_and_mixed_snapshot_dependencies(){
  let ns=nodes("<!doctype html><p id=x></p>");let h=id(&ns,"x");let s=selector_ext(":where(:target)").unwrap();assert_eq!(s.matches_checked_target(&ns,h,&TargetState::Fragment("x".into())),Some(true));assert_eq!(s.matches_checked_target(&ns,h,&TargetState::FragmentFreeCapture),Some(false));
  let s=selector_ext(":where(:link):not(:where(:hover))").unwrap();let history=SnapshotState{history:Some(HistoryState::FreshNoHistory),input:None,focus:None,target:None};assert!(!s.state_satisfied_by(Some(&history)));let all=SnapshotState{input:Some(InputState::QuiescentCapture),..history};assert!(s.state_satisfied_by(Some(&all)));
 }
 #[test]
 fn exact_repeat_cost_and_left_unknown_and_cumulative_fuel(){
  let ns=nodes("<!doctype html><div><p id=x class=a></p><p></p></div>");let h=id(&ns,"x");
  let mut one=EXT_FUEL;assert_eq!(selector_ext(":where(.a)").unwrap().matches_checked_scoped_budget(&ns,h,&mut one,None,None),Ok(Some(true)));let one_cost=EXT_FUEL-one;
  let mut two=EXT_FUEL;assert_eq!(selector_ext(":where(.a):where(.a)").unwrap().matches_checked_scoped_budget(&ns,h,&mut two,None,None),Ok(Some(true)));let two_cost=EXT_FUEL-two;assert_eq!(two_cost,1+2*(one_cost-1),"outer step once, every argument/substep work actually charged");
  let mut f=two_cost-1;assert_eq!(selector_ext(":where(.a):where(.a)").unwrap().matches_checked_scoped_budget(&ns,h,&mut f,None,None),Err(MatchRefusal::Fuel));
  let mut ns=nodes("<!doctype html><div id=d><p id=x></p></div>");let d=id(&ns,"d");let h=id(&ns,"x");let parent=ns[d].parent.unwrap();ns[parent].children.retain(|&h|h!=d);
  assert_eq!(selector_ext("div:where(:nth-last-child(1)) p").unwrap().matches_checked(&ns,h),None);
 }
 #[test]
 fn shared_work_unknown_and_errors_never_inverted(){
  let mut ns=nodes(&format!("<!doctype html><div><p id=x data-x='{}'></p><p></p></div>","a".repeat(1000)));let h=id(&ns,"x");let mut fuel=EXT_FUEL;
  assert_eq!(selector_ext(":where([data-x*=aaaaab])").unwrap().matches_checked_scoped_budget(&ns,h,&mut fuel,None,None),Err(MatchRefusal::Fuel));assert_eq!(fuel,0);
  let parent=ns[h].parent.unwrap();ns[parent].children.retain(|&x|x!=h);
  for t in [":where(:nth-last-child(1))",":not(:where(:first-of-type))",":where(:not(:nth-last-child(1)))"]{assert_eq!(selector_ext(t).unwrap().matches_checked(&ns,h),None,"{t}");}
  ns[parent].children=vec![h;EXT_MAX_SIBLINGS+1];for t in [":not(:where(:first-of-type))",":where(:not(:nth-last-child(1)))"]{let mut f=EXT_FUEL;assert_eq!(selector_ext(t).unwrap().matches_checked_scoped_budget(&ns,h,&mut f,None,None),Err(MatchRefusal::OversizeSiblings));}
 }
}

#[cfg(test)]
mod empty_bounded_tests {
 use super::*;
 fn nodes(html:&str)->Vec<Node>{use html5ever::tendril::TendrilSink;let sink=crate::dom::Sink::new(64*1024*1024);let mut p=html5ever::parse_document(sink,html5ever::ParseOpts{tree_builder:html5ever::tree_builder::TreeBuilderOpts{scripting_enabled:false,..Default::default()},..Default::default()});p.process(html.to_string().into());p.finish().into_nodes()}
 fn id(ns:&[Node],v:&str)->Handle{ns.iter().position(|n|matches!(&n.data,Data::Element{attrs,..} if attrs.iter().any(|(k,a)|&**k=="id" && a==v))).unwrap()}
 #[test]
 fn actual_html_browser17_and_template5_plus_parent_replay(){
  for (html,expected,count) in [(include_str!("../tests/fixtures/empty_oracle.html"),include_str!("../tests/fixtures/empty_oracle_expected.tsv"),17),(include_str!("../tests/fixtures/empty_template_oracle.html"),include_str!("../tests/fixtures/empty_template_expected.tsv"),6)]{
   let ns=nodes(html);let mut n=0;for l in expected.lines(){let v:Vec<_>=l.split('\t').collect();let h=id(&ns,v[0]);let b=v[1]=="1";assert_eq!(selector_ext(":empty").unwrap().matches_checked(&ns,h),Some(b),"{}",v[0]);assert_eq!(selector_ext(":not(:empty)").unwrap().matches_checked(&ns,h),Some(!b));assert_eq!(selector_ext(":where(:empty)").unwrap().matches_checked(&ns,h),Some(b));n+=1;}assert_eq!(n,count);
  }
 }
 #[test]
 fn specificity_grammar_repeated_census_and_exact_raw_cost(){
  assert_eq!(selector_ext("p:empty:empty").unwrap().specificity,(0,2,1));assert_eq!(selector_ext(":where(:empty)").unwrap().specificity,(0,0,0));assert_eq!(selector_ext(":not(:empty)").unwrap().specificity,(0,1,0));assert!(selector("p:empty").is_none());assert!(selector_ext(&":empty".repeat(8)).is_some());assert!(selector_ext(&":empty".repeat(9)).is_none());assert!(selector_ext(":empty()").is_none());
  let ns=nodes("<!doctype html><p id=x><!--c--><!--d--></p>");let h=id(&ns,"x");let mut f=EXT_FUEL;assert_eq!(selector_ext(":empty:empty").unwrap().matches_checked_scoped_budget(&ns,h,&mut f,None,None),Ok(Some(true)));assert_eq!(EXT_FUEL-f,1+ns[h].children.len() as u32);
 }
 #[test]
 fn synthetic_empty_text_other_detached_namespace_invalid_none_and_oversize(){
  // Explicit synthetic empty Text, NOT browser row named emptytext (no nodes).
  let mut ns=nodes("<!doctype html><p id=x></p>");let h=id(&ns,"x");ns[h].parent=None;let t=ns.len();ns.push(Node{data:Data::Text(String::new()),parent:Some(h),children:Vec::new()});let o=ns.len();ns.push(Node{data:Data::Other,parent:Some(h),children:Vec::new()});ns[h].children=vec![t,o];let mut f=EXT_FUEL;assert_eq!(empty_children_charged(&ns,h,&mut f),Ok(Some(true)));assert_eq!(EXT_FUEL-f,2);
  ns[t].data=Data::Text(" ".into());assert_eq!(selector_ext(":empty").unwrap().matches_checked(&ns,h),Some(false));
  ns[t].data=Data::Element{name:html5ever::QualName::new(None,"urn:foreign".into(),"x".into()),attrs:Vec::new(),template:None};assert_eq!(selector_ext(":empty").unwrap().matches_checked(&ns,h),Some(false));
  ns[h].children=vec![ns.len()+9];for s in [":empty",":not(:empty)",":where(:empty)",":not(:where(:empty))"]{assert_eq!(selector_ext(s).unwrap().matches_checked(&ns,h),None,"{s}");}
  let mut f=EXT_FUEL;assert_eq!(empty_children_charged(&ns,o,&mut f),Ok(None),"non-element never true");ns[h].children=vec![o;EXT_MAX_SIBLINGS+1];let mut f=EXT_FUEL;assert_eq!(empty_children_charged(&ns,h,&mut f),Err(MatchRefusal::OversizeSiblings));assert_eq!(f,EXT_FUEL);
 }
 #[test]
 fn raw_engine150_unique_ids_complete_chain_not_flattened(){
  let n=150;let html=format!("<!doctype html>{}<p id=sentinel>SENTINEL</p><aside id=leaf></aside>{}",(0..n).map(|i|format!("<div id=d{}>",i)).collect::<String>(),"</div>".repeat(n));let ns=nodes(&html);let mut at=id(&ns,"sentinel");
  for i in (0..n).rev(){at=ns[at].parent.unwrap();assert_eq!(at,id(&ns,&format!("d{}",i)));assert_eq!(selector_ext(":empty").unwrap().matches_checked(&ns,at),Some(false));}
  assert_eq!(selector_ext(":empty").unwrap().matches_checked(&ns,id(&ns,"leaf")),Some(true));
 }
 #[test]
 fn full_ignored_children_cumulative_fuel_and_left_unknown(){
  let mut ns=nodes("<!doctype html><div id=d><p id=x></p></div>");let d=id(&ns,"d");let h=id(&ns,"x");let o=ns.len();ns.push(Node{data:Data::Other,parent:Some(h),children:Vec::new()});ns[h].children=vec![o;3];let mut f=4;assert_eq!(selector_ext(":where(:empty):not(:empty)").unwrap().matches_checked_scoped_budget(&ns,h,&mut f,None,None),Err(MatchRefusal::Fuel));
  ns[d].children=vec![ns.len()+1];assert_eq!(selector_ext("div:empty p").unwrap().matches_checked(&ns,h),None);
 }
}

#[cfg(test)]
mod child_has_bounded_tests {
 use super::*;
 fn nodes(html:&str)->Vec<Node>{use html5ever::tendril::TendrilSink;let sink=crate::dom::Sink::new(64*1024*1024);let mut p=html5ever::parse_document(sink,html5ever::ParseOpts{tree_builder:html5ever::tree_builder::TreeBuilderOpts{scripting_enabled:false,..Default::default()},..Default::default()});p.process(html.to_string().into());p.finish().into_nodes()}
 fn id(ns:&[Node],v:&str)->Handle{ns.iter().position(|n|matches!(&n.data,Data::Element{attrs,..} if attrs.iter().any(|(k,a)|&**k=="id" && a==v))).unwrap()}
 fn all()->SnapshotState{SnapshotState{history:Some(HistoryState::FreshNoHistory),input:Some(InputState::QuiescentCapture),focus:Some(FocusState::UnfocusedInitial),target:Some(TargetState::FragmentFreeCapture)}}
 #[test]
 fn browser_match_actual_html_oracle(){
  let ns=nodes(include_str!("../tests/fixtures/child_has_oracle.html"));let expected=include_str!("../tests/fixtures/child_has_expected.tsv");let sels=[":has(> *)",":has(> p)",":has(> code)",":has(> .x)",":not(:has(> p))",":where(:has(> #p))",":has(> :not(.no))",":has(> :where(.x))",":has(> :nth-child(2))",":has(> :nth-last-child(2))",":has(> :first-of-type)",":has(> :empty)",":has(> :link)",":has(> :hover)",":has(> :target)",":has(> :disabled)","div:has(> p) + div","div:has(> p) ~ div",":has(> .x):has(> span)"];
  let snap=all();for line in expected.lines(){let v:Vec<_>=line.split('\t').collect();let h=id(&ns,v[0]);for (i,s)in sels.iter().enumerate(){let mut f=EXT_FUEL;assert_eq!(selector_ext(s).unwrap().matches_checked_scoped_budget_with_snapshot(&ns,h,&mut f,None,Some(None),Some(&snap)),Ok(Some(v[i+1]=="1")),"{} {}",v[0],s);}}
 }
 #[test]
 fn grammar_whole_refusal_flags_rank_and_bounds(){
  for bad in [":has(+ p)",":has(~ p)",":has(> p > span > i)",":has(> p, > span)",":has(>)",":has(>>p)",":has(>p +)",":has(>p junk)",":has(>p::before)",":has(> :not(:has(> p)))",":where(:has(> :not(:where(.x))))",":not(:has(> :where(:not(.x))))"]{assert!(selector_ext(bad).is_none(),"scope refusal not invalid CSS:{bad}");}
  for good in [":has(> :where(:not(.x)))",":not(:where(:has(> .x)))",":where(:not(:has(> .x)))",":has( > p )"]{assert!(selector_ext(good).is_some(),"{good}");}
  for (s,want)in [(":has(> *)",(0,0,0)),(":has(> #x)",(1,0,0)),(":has(> p.a)",(0,1,1)),(":where(:has(> #x))",(0,0,0)),(":not(:has(> .x))",(0,1,0))]{assert_eq!(selector_ext(s).unwrap().specificity,want);assert!(selector(s).is_none());}
  assert!(selector_ext(&":has(> *)".repeat(8)).is_some());assert!(selector_ext(&":has(> *)".repeat(9)).is_none());
 }
 #[test]
 fn genuine_invalid_unknown_then_true_false_parent_mismatch_and_detached(){
  let mut ns=nodes("<!doctype html><div id=a><p id=p class=x></p><span id=s></span></div>");let a=id(&ns,"a");let p=id(&ns,"p");let sp=id(&ns,"s");let invalid=ns.len()+1;
  ns[a].children=vec![invalid,p];assert_eq!(selector_ext(":has(> .x)").unwrap().matches_checked(&ns,a),Some(true),"genuine unknown then validtrue");ns[a].children=vec![invalid,sp];assert_eq!(selector_ext(":has(> .x)").unwrap().matches_checked(&ns,a),None,"unknown+false");
  ns[a].children=vec![p];ns[p].parent=None;assert_eq!(selector_ext(":has(> .absent)").unwrap().matches_checked(&ns,a),None,"parent mismatch before falsepredicate");ns[p].parent=Some(a);ns[a].parent=None;assert_eq!(selector_ext(":has(> .x)").unwrap().matches_checked(&ns,a),Some(true));
  let arg=selector_ext("*").unwrap();let mut f=EXT_FUEL;assert_eq!(arg.matches_direct_child_charged(&ns,0,&mut f,None,true),Ok(None),"non-elementanchor");
 }
 #[test]
 fn candidate_index_not_anchor_position_repeat_actualcost_and_immediate_error(){
  let ns=nodes("<!doctype html><div id=a>text<!--c--><p></p><span class=x></span></div>");let a=id(&ns,"a");let mut one=EXT_FUEL;let sel=selector_ext(":has(> span:nth-child(2):nth-last-child(1):first-of-type)").unwrap();assert_eq!(sel.matches_checked_scoped_budget(&ns,a,&mut one,Some((999,999)),None),Ok(Some(true)));
  let mut f=EXT_FUEL;assert_eq!(sel.matches_checked_scoped_budget(&ns,a,&mut f,None,None),Ok(Some(true)));assert_eq!(one,f);
  let mut f=EXT_FUEL;assert_eq!(selector_ext(":has(> .x)").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Ok(Some(true)));let cost=EXT_FUEL-f;let mut f=EXT_FUEL;assert_eq!(selector_ext(":has(> .x):has(> .x)").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Ok(Some(true)));assert_eq!(EXT_FUEL-f,1+2*(cost-1),"repeated rescans charged");
  let ns=nodes(&format!("<!doctype html><div id=a><p data-x='{}'></p><p data-x=aaaaab></p></div>","a".repeat(1000)));let a=id(&ns,"a");let mut f=EXT_FUEL;assert_eq!(selector_ext(":has(> [data-x*=aaaaab])").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Err(MatchRefusal::Fuel),"errorbefore wouldbetrue propagates");
 }
 #[test]
 fn exact_non_element_failed_search_oversize_and_left_error_pins(){
  let mut ns=nodes("<!doctype html><div id=a>text<!--c--></div>");let a=id(&ns,"a");let mut f=EXT_FUEL;assert_eq!(selector_ext(":has(> .missing)").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Ok(Some(false)));assert_eq!(EXT_FUEL-f,1+ns[a].children.len() as u32);
  let o=ns[a].children[0];ns[a].children=vec![o;EXT_MAX_SIBLINGS+1];let mut f=EXT_FUEL;assert_eq!(selector_ext(":not(:has(> *))").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Err(MatchRefusal::OversizeSiblings));assert_eq!(EXT_FUEL-f,2);
  let mut ns=nodes("<!doctype html><div id=a><p id=p></p></div>");let a=id(&ns,"a");let p=id(&ns,"p");ns[a].children=vec![ns.len()+1];assert_eq!(selector_ext("div:has(> *) p").unwrap().matches_checked(&ns,p),None);
 }
 #[test]
 fn every_wrapper_dependency_order_preflight_before_false_and_same_target(){
  let ns=nodes("<!doctype html><div id=a><p id=p></p></div>");let a=id(&ns,"a");for s in [".absent:has(> :hover)",".absent:not(:where(:has(> :focus)))",".absent:where(:has(> :not(:link)))",":has(> :where(:not(:target)))"]{let x=selector_ext(s).unwrap();assert!(x.has_state());assert!(!x.state_satisfied_by(None));assert_eq!(x.matches_checked(&ns,a),None,"{s}");}
  let x=selector_ext(":has(> :target)").unwrap();assert_eq!(x.matches_checked_target(&ns,a,&TargetState::Fragment("p".into())),Some(true));assert_eq!(x.matches_checked_target(&ns,a,&TargetState::Fragment("a".into())),Some(false));
  assert!(selector_ext("div:has(> :hover)::before").is_none());
 }
}

#[cfg(test)]
mod desc_has_bounded_tests {
 use super::*;
 fn nodes(html:&str)->Vec<Node>{use html5ever::tendril::TendrilSink;let sink=crate::dom::Sink::new(64*1024*1024);let mut p=html5ever::parse_document(sink,html5ever::ParseOpts{tree_builder:html5ever::tree_builder::TreeBuilderOpts{scripting_enabled:false,..Default::default()},..Default::default()});p.process(html.to_string().into());p.finish().into_nodes()}
 fn id(ns:&[Node],v:&str)->Handle{ns.iter().position(|n|matches!(&n.data,Data::Element{attrs,..} if attrs.iter().any(|(k,a)|&**k=="id" && a==v))).unwrap()}
 fn all()->SnapshotState{SnapshotState{history:Some(HistoryState::FreshNoHistory),input:Some(InputState::QuiescentCapture),focus:Some(FocusState::UnfocusedInitial),target:Some(TargetState::FragmentFreeCapture)}}
 #[test]
 fn browser_match_actual_html_oracle(){
  let ns=nodes(include_str!("../tests/fixtures/desc_has_oracle.html"));let expected=include_str!("../tests/fixtures/desc_has_expected.tsv");let sels=[":has(*)",":has(p)",":has(code)",":has(.x)",":not(:has(p))",":where(:has(#p))",":has(:not(.no))",":has(:where(.x))",":has(:nth-child(2))",":has(:nth-last-child(2))",":has(:first-of-type)",":has(:empty)",":has(:link)",":has(:hover)",":has(:target)",":has(:disabled)","div:has(p) + div","div:has(p) ~ div",":has(.x):has(span)"];
  assert_eq!(expected.lines().count(),18);let snap=all();for line in expected.lines(){let v:Vec<_>=line.split('\t').collect();let h=id(&ns,v[0]);for (i,s)in sels.iter().enumerate(){let mut f=EXT_FUEL;assert_eq!(selector_ext(s).unwrap().matches_checked_scoped_budget_with_snapshot(&ns,h,&mut f,None,Some(None),Some(&snap)),Ok(Some(v[i+1]=="1")),"{} {}",v[0],s);}}
 }
 #[test]
 fn grammar_whole_refusal_flags_rank_and_bounds(){
  for bad in [":has()",":has( )",":has(p,span)",":has(p span)",":has(p + span)",":has(+ p)",":has(~ p)",":has(p > span)",":has(p, > span)",":has(>)",":has(>>p)",":has(>p +)",":has(>p junk)",":has(>p::before)",":has(:not(:has(p)))",":where(:has(:not(:where(.x))))",":not(:has(:where(:not(.x))))"]{assert!(selector_ext(bad).is_none(),"scope refusal not invalid CSS:{bad}");}
  for good in [":has(:where(:not(.x)))",":not(:where(:has(.x)))",":where(:not(:has(.x)))",":has( p )"]{assert!(selector_ext(good).is_some(),"{good}");}
  for (s,want)in [(":has(*)",(0,0,0)),(":has(#x)",(1,0,0)),(":has(p.a)",(0,1,1)),(":where(:has(#x))",(0,0,0)),(":not(:has(.x))",(0,1,0))]{assert_eq!(selector_ext(s).unwrap().specificity,want);assert!(selector(s).is_none());}
  assert!(matches!(selector_ext(":has(p)").unwrap().steps[0].0.has_args[0].first.steps[0].1,Relation::Descendant));assert!(matches!(selector_ext(":has(>p)").unwrap().steps[0].0.has_args[0].first.steps[0].1,Relation::Child));
  assert!(selector_ext(&":has(*)".repeat(8)).is_some());assert!(selector_ext(&":has(*)".repeat(9)).is_none());
 }
 #[test]
 fn genuine_invalid_unknown_then_true_false_parent_mismatch_and_detached(){
  let mut ns=nodes("<!doctype html><div id=a><p id=p class=x></p><span id=s></span></div>");let a=id(&ns,"a");let p=id(&ns,"p");let sp=id(&ns,"s");let invalid=ns.len()+1;
  ns[a].children=vec![invalid,p];assert_eq!(selector_ext(":has(.x)").unwrap().matches_checked(&ns,a),Some(true),"genuine unknown then validtrue");ns[a].children=vec![invalid,sp];assert_eq!(selector_ext(":has(.x)").unwrap().matches_checked(&ns,a),None,"unknown+false");
  ns[a].children=vec![p];ns[p].parent=None;assert_eq!(selector_ext(":has(.absent)").unwrap().matches_checked(&ns,a),None,"parent mismatch before falsepredicate");ns[p].parent=Some(a);ns[a].parent=None;assert_eq!(selector_ext(":has(.x)").unwrap().matches_checked(&ns,a),Some(true));
  let arg=selector_ext("*").unwrap();let mut f=EXT_FUEL;assert_eq!(arg.matches_descendant_charged(&ns,0,&mut f,None,true),Ok(None),"non-elementanchor");
 }
 #[test]
 fn candidate_index_not_anchor_position_repeat_actualcost_and_immediate_error(){
  let ns=nodes("<!doctype html><div id=a>text<!--c--><p></p><span class=x></span></div>");let a=id(&ns,"a");let mut one=EXT_FUEL;let sel=selector_ext(":has(span:nth-child(2):nth-last-child(1):first-of-type)").unwrap();assert_eq!(sel.matches_checked_scoped_budget(&ns,a,&mut one,Some((999,999)),None),Ok(Some(true)));
  let mut f=EXT_FUEL;assert_eq!(sel.matches_checked_scoped_budget(&ns,a,&mut f,None,None),Ok(Some(true)));assert_eq!(one,f);
  let mut f=EXT_FUEL;assert_eq!(selector_ext(":has(.x)").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Ok(Some(true)));let cost=EXT_FUEL-f;let mut f=EXT_FUEL;assert_eq!(selector_ext(":has(.x):has(.x)").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Ok(Some(true)));assert_eq!(EXT_FUEL-f,1+2*(cost-1),"repeated rescans charged");
  let ns=nodes(&format!("<!doctype html><div id=a><p data-x='{}'></p><p data-x=aaaaab></p></div>","a".repeat(1000)));let a=id(&ns,"a");let mut f=EXT_FUEL;assert_eq!(selector_ext(":has([data-x*=aaaaab])").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Err(MatchRefusal::Fuel),"errorbefore wouldbetrue propagates");
 }
 #[test]
 fn exact_non_element_failed_search_oversize_and_left_error_pins(){
  let mut ns=nodes("<!doctype html><div id=a>text<!--c--></div>");let a=id(&ns,"a");let mut f=EXT_FUEL;assert_eq!(selector_ext(":has(.missing)").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Ok(Some(false)));assert_eq!(EXT_FUEL-f,1+ns[a].children.len() as u32);
  let o=ns[a].children[0];ns[a].children=vec![o;EXT_MAX_SIBLINGS+1];let mut f=EXT_FUEL;assert_eq!(selector_ext(":not(:has(*))").unwrap().matches_checked_scoped_budget(&ns,a,&mut f,None,None),Err(MatchRefusal::OversizeSiblings));assert_eq!(EXT_FUEL-f,2);
  let mut ns=nodes("<!doctype html><div id=a><p id=p></p></div>");let a=id(&ns,"a");let p=id(&ns,"p");ns[a].children=vec![ns.len()+1];assert_eq!(selector_ext("div:has(*) p").unwrap().matches_checked(&ns,p),None);
 }
 #[test]
 fn every_wrapper_dependency_order_preflight_before_false_and_same_target(){
  let ns=nodes("<!doctype html><div id=a><p id=p></p></div>");let a=id(&ns,"a");for s in [".absent:has(:hover)",".absent:not(:where(:has(:focus)))",".absent:where(:has(:not(:link)))",":has(:where(:not(:target)))"]{let x=selector_ext(s).unwrap();assert!(x.has_state());assert!(!x.state_satisfied_by(None));assert_eq!(x.matches_checked(&ns,a),None,"{s}");}
  let x=selector_ext(":has(:target)").unwrap();assert_eq!(x.matches_checked_target(&ns,a,&TargetState::Fragment("p".into())),Some(true));assert_eq!(x.matches_checked_target(&ns,a,&TargetState::Fragment("a".into())),Some(false));
  assert!(selector_ext("div:has(:hover)::before").is_none());
 }
}

#[cfg(test)]
mod desc_has_cursor_tests {
 use super::*;
 fn nodes(h:&str)->Vec<Node>{use html5ever::tendril::TendrilSink;let sink=crate::dom::Sink::new(64*1024*1024);let mut p=html5ever::parse_document(sink,html5ever::ParseOpts::default());p.process(h.to_string().into());p.finish().into_nodes()}
 fn id(n:&[Node],s:&str)->Handle{n.iter().position(|n|matches!(&n.data,Data::Element{attrs,..}if attrs.iter().any(|(k,v)|&**k=="id" && v==s))).unwrap()}
 fn run(n:&[Node],a:Handle,s:&str,f:&mut u32)->Result<Option<bool>,MatchRefusal>{selector_ext(s).unwrap().matches_checked_scoped_budget(n,a,f,None,None)}
 #[test] fn exact_ascent_rescans_repeat_and_budget_bound(){
 let n=nodes("<!doctype html><div id=a><p></p><p></p></div>");let a=id(&n,"a");let mut f=EXT_FUEL;assert_eq!(run(&n,a,":has(.no)",&mut f),Ok(Some(false)));assert_eq!(EXT_FUEL-f,10,"outer1+two(raw1+predicate1+ascent1)+rescan1+rescan2");
 let mut f=EXT_FUEL;assert_eq!(run(&n,a,":has(.no):has(.no)",&mut f),Ok(Some(false)));assert_eq!(EXT_FUEL-f,10,"firstfalse shortcircuits");
 let mut f=9;assert_eq!(run(&n,a,":has(.no)",&mut f),Err(MatchRefusal::Fuel));assert_eq!(f,0);
 }
 #[test] fn malformed_cycles_duplicates_boundary_and_detached(){
 let mut n=nodes("<!doctype html><div id=a><p id=p></p><span id=s></span></div><aside id=outside></aside>");let a=id(&n,"a");let p=id(&n,"p");let out=id(&n,"outside");
 n[p].children=vec![a,p,out,n.len()+9]; // anchorbackedge, selfcycle, mismatchedoutside must not match
 let mut f=EXT_FUEL;assert_eq!(run(&n,a,":has(#outside)",&mut f),Ok(None));assert!(f<EXT_FUEL);
 n[p].children.clear();n[a].children=vec![p,p];let mut f=40;assert_eq!(run(&n,a,":has(#outside)",&mut f),Err(MatchRefusal::Fuel));assert_eq!(f,0,"duplicate firstmembership resumes same index, chargedtermination");
 n[a].children=vec![p];n[a].parent=None;let mut f=EXT_FUEL;assert_eq!(run(&n,a,":has(p)",&mut f),Ok(Some(true)));
 n[p].children=vec![out]; // parent mismatch before predicate false/true
 let mut f=EXT_FUEL;assert_eq!(run(&n,a,":has(#outside)",&mut f),Ok(None));
 }
 #[test] fn nested_oversize_before_later_true_and_non_element_leaves(){
 let mut n=nodes("<!doctype html><div id=a><p id=p></p><span class=x></span></div>");let a=id(&n,"a");let p=id(&n,"p");n[p].children=vec![n.len()+3;EXT_MAX_SIBLINGS+1];let mut f=EXT_FUEL;assert_eq!(run(&n,a,":has(.x)",&mut f),Err(MatchRefusal::OversizeSiblings));assert_eq!(EXT_FUEL-f,4,"outer/raw/candidate/classlookup then nestedguard");
 }
 #[test] fn rightmost_candidate_position_none_and_grandchild_only(){
 let n=nodes("<!doctype html><div id=a><section><p></p><span></span></section></div><aside></aside>");let a=id(&n,"a");let mut f=EXT_FUEL;let x=selector_ext(":has(span:nth-child(2):nth-last-child(1):first-of-type)").unwrap();assert_eq!(x.matches_checked_scoped_budget(&n,a,&mut f,Some((999,999)),None),Ok(Some(true)));assert_eq!(selector_ext(":has(> span)").unwrap().matches_checked(&n,a),Some(false));assert_eq!(selector_ext(":has(aside)").unwrap().matches_checked(&n,a),Some(false));assert_eq!(selector_ext(":has(#a)").unwrap().matches_checked(&n,a),Some(false));
 }
}

#[cfg(test)]
mod desc_has_ascent_refusal_tests {
 use super::*;
 #[test] fn exact_initial_nested_and_rescan_gates_missing_membership_and_invalid_parent(){
  use html5ever::tendril::TendrilSink;
  let sink=crate::dom::Sink::new(10000);let mut p=html5ever::parse_document(sink,html5ever::ParseOpts::default());p.process("<!doctype html><div><p></p>t</div>".to_string().into());let mut n=p.finish().into_nodes();let a=n.iter().position(|x|matches!(&x.data,Data::Element{name,..}if &*name.local=="div")).unwrap();let c=n[a].children[0];let t=n[a].children[1];
  n[a].children=vec![t;EXT_MAX_SIBLINGS];let mut f=10000;assert_eq!(selector_ext("*").unwrap().matches_descendant_charged(&n,a,&mut f,None,true),Ok(Some(false)));assert_eq!(10000-f,EXT_MAX_SIBLINGS as u32);
  let mut f=10000;assert_eq!(Selector::descendant_resume(&n,c,&mut f),Ok(None));assert_eq!(10000-f,1+EXT_MAX_SIBLINGS as u32,"ascent+everyrawmissingmembership");
  n[a].children.push(t);let mut f=10000;assert_eq!(selector_ext("*").unwrap().matches_descendant_charged(&n,a,&mut f,None,true),Err(MatchRefusal::OversizeSiblings));assert_eq!(f,10000,"initialpreguard");
  let mut f=10000;assert_eq!(Selector::descendant_resume(&n,c,&mut f),Err(MatchRefusal::OversizeSiblings));assert_eq!(10000-f,1,"ascentcharged before rescanpreguard");
  n[c].parent=None;let mut f=10000;assert_eq!(Selector::descendant_resume(&n,c,&mut f),Ok(None));assert_eq!(10000-f,1);
  n[c].parent=Some(n.len()+1);let mut f=10000;assert_eq!(Selector::descendant_resume(&n,c,&mut f),Ok(None));assert_eq!(10000-f,1);
 }
}

#[cfg(test)]
mod two_child_has_tests {
 use super::*;
 fn nodes(h:&str)->Vec<Node>{use html5ever::tendril::TendrilSink;let sink=crate::dom::Sink::new(64*1024*1024);let mut p=html5ever::parse_document(sink,html5ever::ParseOpts::default());p.process(h.to_string().into());p.finish().into_nodes()}
 fn id(n:&[Node],s:&str)->Handle{n.iter().position(|n|matches!(&n.data,Data::Element{attrs,..}if attrs.iter().any(|(k,v)|&**k=="id" && v==s))).unwrap()}
 fn run(n:&[Node],a:Handle,s:&str,f:&mut u32)->Result<Option<bool>,MatchRefusal>{selector_ext(s).unwrap().matches_checked_scoped_budget(n,a,f,None,None)}
 #[test] fn grammar_inherited_flags_both_ranks_and_fixed_bounds(){
 for bad in [":has(>p>i>b)",":has(p>i)",":has(>p i)",":has(>p + i)",":has(>p>i,>b)",":has(>p>)",":has(>p>>i)",":has(>p>i::before)",":has(>:not(:has(p))>i)",":has(>p>:where(:has(i)))",":not(:has(>p>:not(i)))",":where(:has(>:where(p)>i))"]{assert!(selector_ext(bad).is_none(),"whole scoped refusal:{bad}");}
 for good in [":has( > p > i )",":has(>:where(p)>:where(i))",":has(>:not(p)>:not(i))",":not(:has(>:where(p)>:where(i)))",":where(:has(>:not(p)>:not(i)))"]{assert!(selector_ext(good).is_some(),"{good}");}
 for (x,w) in [(":has(>* >*)",(0,0,0)),(":has(>#x>p.a)",(1,1,1)),(":where(:has(>#x>p.a))",(0,0,0)),(":not(:has(>p.a>#x))",(1,1,1))]{assert_eq!(selector_ext(x).unwrap().specificity,w);assert!(selector(x).is_none());}
 assert!(selector_ext(&":has(>*>*)".repeat(8)).is_some());assert!(selector_ext(&":has(>*>*)".repeat(9)).is_none());assert!(selector_ext(&format!(":has(>{}>*)",":empty".repeat(9))).is_none());assert!(selector_ext(&format!(":has(> * >{})",":empty".repeat(9))).is_none());
 }
 #[test] fn real_unknown_middle_leaf_then_true_false_mismatch_and_cycles(){
 let mut n=nodes("<!doctype html><div id=a><section id=m><p id=p class=x></p><b id=b></b></section><section id=m2><p class=x></p></section></div><aside id=outside></aside>");let a=id(&n,"a");let m=id(&n,"m");let m2=id(&n,"m2");let p=id(&n,"p");let b=id(&n,"b");let out=id(&n,"outside");let bad=n.len()+1;
 n[a].children=vec![bad,m];assert_eq!(selector_ext(":has(>section>.x)").unwrap().matches_checked(&n,a),Some(true));assert_eq!(selector_ext(":has(>section>.no)").unwrap().matches_checked(&n,a),None);
 n[a].children=vec![m];n[m].children=vec![bad,p];assert_eq!(selector_ext(":has(>section>.x)").unwrap().matches_checked(&n,a),Some(true));n[m].children=vec![bad,b];assert_eq!(selector_ext(":has(>section>.x)").unwrap().matches_checked(&n,a),None);
 n[m].children=vec![p];n[p].parent=None;assert_eq!(selector_ext(":has(>section>.no)").unwrap().matches_checked(&n,a),None,"leaf mismatchbeforefalse");n[m].parent=None;assert_eq!(selector_ext(":has(>.absent>*)").unwrap().matches_checked(&n,a),None,"middle mismatchbeforefalse");n[m].parent=Some(a);n[p].parent=Some(m);
 n[a].parent=Some(m);n[m].children=vec![a,m,out];assert_eq!(selector_ext(":has(>section>#a)").unwrap().matches_checked(&n,a),None,"originalanchor excluded even parentconsistent");assert_eq!(selector_ext(":has(>section>#outside)").unwrap().matches_checked(&n,a),None);
 n[a].children=vec![m,m2];assert_eq!(selector_ext(":has(>section>.x)").unwrap().matches_checked(&n,a),Some(true),"later validtrue wins corruptedbranchunknown");
 }
 #[test] fn exact_two_loops_cost_duplicate_repeat_and_gates(){
 let mut n=nodes("<!doctype html><div id=a><section id=m>text<!--c--><p></p></section></div>");let a=id(&n,"a");let m=id(&n,"m");let mut f=100;assert_eq!(run(&n,a,":has(>*>.no)",&mut f),Ok(Some(false)));assert_eq!(100-f,7,"outer1+rawmiddle1+Smatch1+3rawleaf+Tmatch1; noattrs noclasslookup");let text=n[m].children[0];n[m].children=vec![text;EXT_MAX_SIBLINGS+1];let mut f=100;assert_eq!(run(&n,a,":has(>*>*)",&mut f),Err(MatchRefusal::OversizeSiblings));assert_eq!(100-f,3);
 n[a].children=vec![m;EXT_MAX_SIBLINGS+1];let mut f=100;assert_eq!(run(&n,a,":has(>*>*)",&mut f),Err(MatchRefusal::OversizeSiblings));assert_eq!(100-f,1);
 n[a].children=vec![m,m];n[m].children=vec![text,text];let mut f=100;assert_eq!(run(&n,a,":has(>*>*)",&mut f),Ok(Some(false)));assert_eq!(100-f,9,"outer1+2*(middlevisit1+Smatch1+2textvisits)");
 let n=nodes("<!doctype html><div id=a><section><p></p></section></div>");let a=id(&n,"a");let mut f=100;assert_eq!(run(&n,a,":has(>*>*)",&mut f),Ok(Some(true)));let cost=100-f;assert_eq!(cost,5);let mut f=100;assert_eq!(run(&n,a,":has(>*>*):has(>*>*)",&mut f),Ok(Some(true)));assert_eq!(100-f,1+2*(cost-1));let mut f=4;assert_eq!(run(&n,a,":has(>*>*)",&mut f),Err(MatchRefusal::Fuel));assert_eq!(f,0);
 }
 #[test] fn s_unknown_skips_t_and_errors_before_later_true(){
 let mut n=nodes("<!doctype html><div id=a><section id=m><p></p></section></div>");let a=id(&n,"a");let m=id(&n,"m");let invalid=n.len()+1;n[m].children=vec![invalid];let mut f=4;assert_eq!(run(&n,a,":has(>:empty>*)",&mut f),Ok(None),"Sunknown skips T even with zero fuel remaining, not exactfalse");assert_eq!(f,0);

 let n=nodes(&format!("<!doctype html><div id=a><section data-x='{}'><p></p></section><section data-x=aaaaab><p></p></section></div>","a".repeat(1000)));let a=id(&n,"a");let mut f=EXT_FUEL;assert_eq!(run(&n,a,":has(>[data-x*=aaaaab]>*)",&mut f),Err(MatchRefusal::Fuel));
 let n=nodes(&format!("<!doctype html><div id=a><section><p data-x='{}'></p><p data-x=aaaaab></p></section></div>","a".repeat(1000)));let a=id(&n,"a");let mut f=EXT_FUEL;assert_eq!(run(&n,a,":has(>*>[data-x*=aaaaab])",&mut f),Err(MatchRefusal::Fuel));
 }
 #[test] fn both_position_none_and_both_dependencies_preflight(){
 let n=nodes("<!doctype html><div id=a><b></b><section><b></b><p id=p></p></section></div>");let a=id(&n,"a");let mut f=EXT_FUEL;assert_eq!(selector_ext(":has(>section:nth-child(2):last-of-type>p:nth-child(2):nth-last-child(1):last-of-type)").unwrap().matches_checked_scoped_budget(&n,a,&mut f,Some((999,999)),None),Ok(Some(true)));
 for s in [".absent:has(>.absent>:hover)",":has(>.absent>:where(:not(:link)))",":not(:has(>.absent>:where(:focus)))",":where(:has(>:not(.absent)>:target))"]{let x=selector_ext(s).unwrap();assert!(x.has_state());assert!(!x.state_satisfied_by(None));assert_eq!(x.matches_checked(&n,a),None,"bothdeps/rawpreflight:{s}");}
 assert_eq!(selector_ext(":has(>section>:target)").unwrap().matches_checked_target(&n,a,&TargetState::Fragment("p".into())),Some(true));
 }
 #[test] fn browser_two_child_truth(){let n=nodes(include_str!("../tests/fixtures/two_child_has_oracle.html"));let sels=[":has(>section>p)",":has(>*>*)",":has(>section>.x)",":has(>:not(.no)>:where(.x))",":not(:has(>section>p))",":where(:has(>section>#p))",":has(>section:nth-child(2)>p:last-child)",":has(>svg>g)",":has(>template>p)",":has(>section>:empty)",":has(>section>p):has(>section>.x)"];
 let t=include_str!("../tests/fixtures/two_child_has_expected.tsv");assert_eq!(t.lines().count(),15);for l in t.lines(){let v:Vec<_>=l.split('\t').collect();for(i,s)in sels.iter().enumerate(){assert_eq!(selector_ext(s).unwrap().matches_checked(&n,id(&n,v[0])),Some(v[i+1]=="1"),"{} {s}",v[0]);}}}
}
