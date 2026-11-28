//! Deterministic CSS `font-family` list resolution against explicitly available faces.
//!
//! Isolated plumbing, not wired into the style tree or any painter: the existing
//! `font-family` handling (whole-render refusal, or the pinned candidate lane) is unchanged.
//! Semantics are CSS Fonts 4 list fallback over a caller-declared catalog, nothing about any
//! host: the catalog says which family names it HAS, which names it asserts are ABSENT, and
//! which generic keywords it maps. Input case is normalised here (ASCII) before keyword or
//! generic classification. Anything the catalog cannot decide refuses; this module
//! never guesses a host alias (a Chrome host may map `helvetica` to another face, the catalog
//! must not claim `Absent` for a name its reference system aliases).
//!
//! Grammar (strict, everything else refuses): comma-separated entries; an entry is one quoted
//! string (no backslash, no embedded quote or newline) or one or more unquoted identifiers
//! joined by single spaces; at most 16 entries and 256 bytes; no `var()`, no CSS-wide keyword.

/// Opaque id the catalog assigns to an available family.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct FamilyId(pub u16);

/// Generic keywords this module can map. Every other generic keyword refuses
/// (`system-ui`, `cursive`, `fantasy`, `ui-*`, `emoji`, `math`, `fangsong`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Generic {
    Serif,
    SansSerif,
    Monospace,
}

/// What the catalog knows about one named family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lookup {
    Available(FamilyId),
    /// The catalog asserts no face and no alias of this name exists on its reference system.
    Absent,
    /// The catalog cannot say; the list refuses.
    Unknown,
}

/// Explicitly available faces. Names are passed ASCII-lowercased (normalised by the parser).
pub trait FaceCatalog {
    fn lookup(&self, lowercase_name: &str) -> Lookup;
    fn generic(&self, generic: Generic) -> Option<FamilyId>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Entry {
    Named(String),
    Generic(Generic),
    /// A generic keyword this module does not map; refuses only if resolution reaches it.
    UnsupportedGeneric,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// Not in the strict grammar (empty entry, escape, bad quote, `var(`, too long).
    Syntax,
    /// `inherit`, `initial`, `unset`, `revert`, `revert-layer`, `default` as the whole value or an entry.
    CssWideKeyword,
    /// A generic keyword this module does not map (system-ui, cursive, ...).
    UnsupportedGeneric,
    /// The catalog returned `Unknown` for a name, or has no mapping for a generic.
    Undecidable,
    /// Every entry was `Absent`; the CSS default font is host policy, so this refuses.
    NothingAvailable,
}

const MAX_ENTRIES: usize = 16;
const MAX_BYTES: usize = 256;
const WS: &[char] = &[' ', '\t', '\n', '\r', '\x0c'];

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}
fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || !c.is_ascii()
}
fn is_ident(word: &str) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else { return false };
    let ok_start = if first == '-' {
        // `-` followed by an identifier start; `--x` (custom-property style), bare `-` and `-1` refuse.
        matches!(chars.clone().next(), Some(c) if is_ident_start(c))
    } else {
        is_ident_start(first)
    };
    ok_start && word.chars().all(is_ident_char)
}

/// Split on commas outside quotes. Returns None on an unbalanced quote.
fn split_entries(value: &str) -> Option<Vec<&str>> {
    let mut out = Vec::new();
    let (mut start, mut quote) = (0usize, None::<char>);
    for (i, c) in value.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, ',') => {
                out.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return None;
    }
    out.push(&value[start..]);
    Some(out)
}

/// Parse a lowercased `font-family` value into entries.
///
/// # Errors
/// A [`Refusal`] naming why the value is outside the strict grammar.
pub fn parse_family_list(value: &str) -> Result<Vec<Entry>, Refusal> {
    // CSS keywords and family names match ASCII case-insensitively: classify on the lowercased
    // value so `SANS-SERIF`, `Inherit` or `System-UI` are never treated as plain names.
    let lowered = value.to_ascii_lowercase();
    let value = lowered.trim_matches(WS);
    if value.is_empty() || value.len() > MAX_BYTES || value.contains('\\') || value.contains('(') {
        return Err(Refusal::Syntax);
    }
    let parts = split_entries(value).ok_or(Refusal::Syntax)?;
    if parts.len() > MAX_ENTRIES {
        return Err(Refusal::Syntax);
    }
    let mut entries = Vec::with_capacity(parts.len());
    for raw in parts {
        let item = raw.trim_matches(WS);
        if item.is_empty() {
            return Err(Refusal::Syntax);
        }
        let first = item.chars().next().unwrap_or(' ');
        if first == '"' || first == '\'' {
            let inner = item
                .strip_prefix(first)
                .and_then(|rest| rest.strip_suffix(first))
                .ok_or(Refusal::Syntax)?;
            if inner.is_empty() || inner.contains(['"', '\'', '\n', '\r', '\x0c']) {
                return Err(Refusal::Syntax);
            }
            entries.push(Entry::Named(inner.to_ascii_lowercase()));
            continue;
        }
        let words: Vec<&str> = item.split(WS).filter(|w| !w.is_empty()).collect();
        if words.is_empty() || !words.iter().all(|w| is_ident(w)) {
            return Err(Refusal::Syntax);
        }
        // Conservative: a CSS-wide keyword or `default` in ANY position of an unquoted sequence
        // refuses (each unquoted word must be a valid <custom-ident>, which excludes them).
        if words.iter().any(|w| {
            matches!(*w, "inherit" | "initial" | "unset" | "revert" | "revert-layer" | "default")
        }) {
            return Err(Refusal::CssWideKeyword);
        }
        if words.len() == 1 {
            match words[0] {
                "serif" => { entries.push(Entry::Generic(Generic::Serif)); continue; }
                "sans-serif" => { entries.push(Entry::Generic(Generic::SansSerif)); continue; }
                "monospace" => { entries.push(Entry::Generic(Generic::Monospace)); continue; }
                "cursive" | "fantasy" | "system-ui" | "ui-serif" | "ui-sans-serif"
                | "ui-monospace" | "ui-rounded" | "emoji" | "math" | "fangsong" => {
                    entries.push(Entry::UnsupportedGeneric);
                    continue;
                }
                _ => {}
            }
        }
        entries.push(Entry::Named(words.join(" ").to_ascii_lowercase()));
    }
    Ok(entries)
}

/// Resolve a lowercased `font-family` value against `catalog`: the first entry that is
/// `Available` (or a mapped generic) wins; `Absent` names are skipped; anything undecidable
/// refuses the whole list.
///
/// # Errors
/// A [`Refusal`]; never a guessed face.
pub fn resolve_family(value: &str, catalog: &(impl FaceCatalog + ?Sized)) -> Result<FamilyId, Refusal> {
    for entry in parse_family_list(value)? {
        match entry {
            Entry::Generic(g) => return catalog.generic(g).ok_or(Refusal::Undecidable),
            Entry::UnsupportedGeneric => return Err(Refusal::UnsupportedGeneric),
            Entry::Named(name) => match catalog.lookup(&name) {
                Lookup::Available(id) => return Ok(id),
                Lookup::Absent => {}
                Lookup::Unknown => return Err(Refusal::Undecidable),
            },
        }
    }
    Err(Refusal::NothingAvailable)
}

/// Family names the Kobo basic painter is KNOWN not to have. The basic painter owns exactly three
/// faces (serif, sans, mono) and performs no host font lookup, so a name outside its bundle can
/// never match a face there; it is `Absent` for that renderer. This says nothing about any browser
/// host (a Chrome host may well have Menlo or Helvetica); the corpus has no Chrome reference.
/// Names the bundle might answer to (Atkinson Hyperlegible, DejaVu, Noto*, KoboPage*) and every
/// webfont-style name (declared by `@font-face` or unknown) are deliberately NOT listed: they refuse.
const DEVICE_ABSENT: &[&str] = &[
    // Sans system stack
    "-apple-system", "blinkmacsystemfont", "avenir next", "avenir", "segoe ui", "helvetica neue",
    "helvetica", "cantarell", "ubuntu", "roboto", "noto", "arial",
    // Monospace system stack
    "menlo", "consolas", "monaco", "liberation mono", "lucida console", "courier new", "courier",
    "sf mono", "sfmono-regular",
];

/// Resolve a lowercased `font-family` LIST to the generic the basic painter can honor.
///
/// Admits only a list with at least one named entry, every named entry before the first generic
/// being in [`DEVICE_ABSENT`], resolving to the first generic keyword, which must be `sans-serif`
/// or `monospace`. Everything else refuses: a bare generic (existing handling unchanged), a name
/// outside the absent set (undecidable), `serif` or an unmapped generic as the first honorable
/// entry, a list with no generic, and anything outside the strict list grammar (`var()`,
/// escapes, CSS-wide keywords, >16 entries, >256 bytes).
///
/// # Errors
/// A [`Refusal`] naming why the list is not admitted.
pub fn device_generic(value: &str) -> Result<Generic, Refusal> {
    let entries = parse_family_list(value)?;
    if !entries.iter().any(|e| matches!(e, Entry::Named(_))) {
        return Err(Refusal::Undecidable);
    }
    for entry in entries {
        match entry {
            Entry::Named(name) if DEVICE_ABSENT.contains(&name.as_str()) => {}
            Entry::Named(_) => return Err(Refusal::Undecidable),
            Entry::Generic(g @ (Generic::SansSerif | Generic::Monospace)) => return Ok(g),
            Entry::Generic(Generic::Serif) => return Err(Refusal::Undecidable),
            Entry::UnsupportedGeneric => return Err(Refusal::UnsupportedGeneric),
        }
    }
    Err(Refusal::NothingAvailable)
}

/// A catalog that can also state, per family, whether a face maps a character to a real glyph.
/// `None` = unknown coverage.
pub trait FaceCoverage: FaceCatalog {
    fn covers(&self, family: FamilyId, character: char, bold: bool) -> Option<bool>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunRefusal {
    Family(Refusal),
    /// The first CSS-available face lacks a character of the run. CSS would still use that face for the glyphs it
    /// has, so rendering the whole run in a later face is wrong admission: refuse, never skip.
    Uncovered,
    /// Coverage of the first available face is not provable for some character.
    UnknownCoverage,
}

/// Single-face-per-run selection: resolve the FIRST CSS-available family, then require it to cover EVERY
/// character of `text`. No fallback to a later listed face when the first covers only part of the run.
pub fn select_run_family(value: &str, catalog: &dyn FaceCoverage, text: &str, bold: bool) -> Result<FamilyId, RunRefusal> {
    let id = resolve_family(value, catalog).map_err(RunRefusal::Family)?;
    for ch in text.chars() {
        match catalog.covers(id, ch, bold) {
            Some(true) => {}
            Some(false) => return Err(RunRefusal::Uncovered),
            None => return Err(RunRefusal::UnknownCoverage),
        }
    }
    Ok(id)
}

// ---------------------------------------------------------------------------------------------
// Interned family-list handles (FL50 increment 1, UNWIRED: nothing in the style tree uses these yet).
//
// Ownership model: `FamilyTable` is a single-owner BUILDER and is deliberately NOT `Clone`, so two
// builders can never share a token and then diverge. `freeze()` consumes it into an immutable
// `FamilyView` that clones by sharing read-only data (`Arc`); keys issued by the builder resolve in
// that view and every clone of it, and in nothing else (each builder takes a process-unique token;
// `get` returns `None` for a foreign key). Indices are `u32` via `try_from`; capacity or token
// exhaustion makes `intern` return `None`, which a caller must turn into a refusal. No wrap.
// ---------------------------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

static NEXT_TABLE_TOKEN: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct FamilyKey {
    table: u32,
    index: u32,
}

#[derive(Debug)]
pub struct FamilyTable {
    token: Option<u32>,
    items: Vec<Box<str>>,
    seen: HashMap<Box<str>, u32>,
    /// Test seam: maximum entries before `intern` returns `None` (defaults to the `u32` index range).
    limit: u32,
}

impl Default for FamilyTable {
    fn default() -> Self {
        Self::new()
    }
}

impl FamilyTable {
    pub fn new() -> Self {
        let token = NEXT_TABLE_TOKEN
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |t| t.checked_add(1))
            .ok();
        FamilyTable { token, items: Vec::new(), seen: HashMap::new(), limit: u32::MAX }
    }

    #[cfg(test)]
    pub fn with_limit(limit: u32) -> Self {
        let mut t = Self::new();
        t.limit = limit;
        t
    }

    /// Intern an already-normalised list text. `None` = capacity or token exhausted (must refuse).
    pub fn intern(&mut self, text: &str) -> Option<FamilyKey> {
        let table = self.token?;
        if let Some(&i) = self.seen.get(text) {
            return Some(FamilyKey { table, index: i });
        }
        let index = u32::try_from(self.items.len()).ok()?;
        if index >= self.limit {
            return None;
        }
        let boxed: Box<str> = text.into();
        self.items.push(boxed.clone());
        self.seen.insert(boxed, index);
        Some(FamilyKey { table, index })
    }

    pub fn get(&self, key: FamilyKey) -> Option<&str> {
        if Some(key.table) != self.token {
            return None;
        }
        self.items.get(usize::try_from(key.index).ok()?).map(|b| &**b)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Consume the builder into an immutable, cheaply clonable view.
    pub fn freeze(self) -> FamilyView {
        FamilyView { inner: Arc::new((self.token, self.items)) }
    }
}

/// Immutable after `freeze`; clones share the same read-only entries and token.
#[derive(Clone, Debug)]
pub struct FamilyView {
    inner: Arc<(Option<u32>, Vec<Box<str>>)>,
}

impl FamilyView {
    pub fn get(&self, key: FamilyKey) -> Option<&str> {
        if Some(key.table) != self.inner.0 {
            return None;
        }
        self.inner.1.get(usize::try_from(key.index).ok()?).map(|b| &**b)
    }

    pub fn len(&self) -> usize {
        self.inner.1.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.1.is_empty()
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;

    #[test]
    fn dedups_and_roundtrips() {
        let mut t = FamilyTable::new();
        let a = t.intern("a, b").unwrap();
        let b = t.intern("c").unwrap();
        assert_ne!(a, b);
        assert_eq!(t.intern("a, b"), Some(a));
        assert_eq!(t.len(), 2);
        assert_eq!(t.get(a), Some("a, b"));
        assert_eq!(t.get(b), Some("c"));
    }

    #[test]
    fn keys_do_not_cross_tables_even_at_equal_index() {
        let mut t1 = FamilyTable::new();
        let mut t2 = FamilyTable::new();
        let k1 = t1.intern("x").unwrap();
        let k2 = t2.intern("y").unwrap();
        assert_eq!(k1.index, k2.index);
        assert_eq!(t1.get(k2), None);
        assert_eq!(t2.get(k1), None);
        let (v1, v2) = (t1.freeze(), t2.freeze());
        assert_eq!(v1.get(k2), None);
        assert_eq!(v2.get(k1), None);
        assert_eq!(v1.get(k1), Some("x"));
        assert_eq!(v2.get(k2), Some("y"));
    }

    #[test]
    fn view_clones_share_immutable_data_and_resolve_same_keys() {
        let mut t = FamilyTable::new();
        let k = t.intern("serif").unwrap();
        let v = t.freeze();
        let c = v.clone();
        let d = c.clone();
        assert_eq!(v.get(k), Some("serif"));
        assert_eq!(c.get(k), Some("serif"));
        assert_eq!(d.get(k), Some("serif"));
        assert!(Arc::ptr_eq(&v.inner, &d.inner));
        assert_eq!(FamilyTable::new().freeze().get(k), None);
    }

    // The clone-divergence hazard is closed by construction: the builder has no Clone, so the only
    // way to get two tables is `new()`, which issues distinct tokens. Both creation orders and
    // divergent appends are exercised here.
    #[test]
    fn independent_builders_with_divergent_appends_never_resolve_each_others_keys() {
        for swap in [false, true] {
            let (mut a, mut b) = (FamilyTable::new(), FamilyTable::new());
            let (ka, kb) = if swap {
                let kb = b.intern("second-only").unwrap();
                let ka = a.intern("first-only").unwrap();
                (ka, kb)
            } else {
                let ka = a.intern("first-only").unwrap();
                let kb = b.intern("second-only").unwrap();
                (ka, kb)
            };
            assert_eq!(a.get(kb), None);
            assert_eq!(b.get(ka), None);
            assert_eq!(a.get(ka), Some("first-only"));
            assert_eq!(b.get(kb), Some("second-only"));
        }
    }

    #[test]
    fn overflow_refuses_without_wrapping() {
        let mut t = FamilyTable::with_limit(2);
        assert!(t.intern("a").is_some());
        assert!(t.intern("b").is_some());
        assert_eq!(t.intern("c"), None);
        assert!(t.intern("a").is_some());
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn token_exhaustion_refuses() {
        let mut t = FamilyTable::new();
        t.token = None; // exhausted-token state, as `new()` yields after u32 exhaustion
        assert_eq!(t.intern("a"), None);
        assert!(t.is_empty());
    }
}
