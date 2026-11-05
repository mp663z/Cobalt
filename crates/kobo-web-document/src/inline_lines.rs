//! Restricted normal-white-space line placement for premeasured ASCII text.
//!
//! The font provider owns glyph advances and raster masks. This module does
//! not invent font metrics, ligatures, kerning, bidi, CSS line-height, or
//! Unicode break rules. Callers must reject unsupported inline contexts.

use crate::box_tree::{BoxKind, BoxTree};
use crate::computed_style::{Direction, TextAlign};
use crate::display_list::{DisplayList, Error as DisplayError, Rect, Rgb, Source, MAX_GLYPH_BYTES};

const MAX_LINES: usize = 4096;
type MeasuredLetter = (char, u32, bool, usize);
type MeasuredWord = (Vec<MeasuredLetter>, Option<(bool, usize)>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionedGlyph {
    pub character: char,
    pub x: u32,
    pub line: u32,
    /// The advance measured when the line was laid out. Advances are
    /// unsigned and RTL/negative shaping is refused upstream, so the
    /// layout cell x..x+advance cannot wrap or run backwards.
    pub advance: u32,
    /// True when this glyph was measured and must be rastered with the bold
    /// face. The same face feeds both advance and bitmap; a provider that
    /// lacks the face fails rather than substituting regular geometry.
    pub bold: bool,
    /// Index into DirectTextLines.leaves; unsourced helpers use only key zero.
    pub leaf: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Lines {
    pub glyphs: Vec<PositionedGlyph>,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineError {
    UnsupportedText,
    InvalidMetrics,
    UnbreakableWord,
    TooManyGlyphs,
    TooManyLines,
    UnsupportedTree,
    FractionalAlignment,
}

/// Place a single block's normal-white-space ASCII text at given integer
/// advances. ASCII whitespace collapses to one breakable space; leading and
/// line-end spaces are omitted. U+00A0 remains a measured nonbreaking glyph
/// inside a word, rather than a whitespace wrap opportunity. A word that cannot fit fails rather than
/// silently overflowing or splitting at an unsupported break opportunity.
/// The caller supplies the exact font advances and owns line-height and paint.
///
/// # Errors
/// Fails on unsupported Unicode/control text, invalid metrics, unbreakable
/// words or bounded storage limits.
pub fn place_ascii_normal(
    text: &str,
    max_width: u32,
    mut advance: impl FnMut(char) -> Option<u32>,
) -> Result<Lines, LineError> {
    place_runs_normal(&[(text, false)], max_width, |ch, _bold| advance(ch))
}

/// Place normal-white-space ASCII inline runs sharing one block's line
/// layout. Each run is (text, bold); words may span run boundaries and
/// ASCII whitespace collapses across them exactly as within one text node:
/// a boundary whitespace run contributes a single measured space, flagged
/// with the face of the run that carried its first whitespace character.
/// Every glyph's advance comes from its own run's face.
///
/// # Errors
/// Same domain and limits as [`place_ascii_normal`]; empty runs are skipped.
/// This unsourced helper uses synthetic leaf key zero for every run. It cannot
/// recover multi-leaf paint identity; use `place_runs_text` for a sourced tree.
#[allow(clippy::too_many_lines)] // Tokenization and wrapping share one bounded glyph budget.
pub fn place_runs_normal(
    runs: &[(&str, bool)],
    max_width: u32,
    mut advance: impl FnMut(char, bool) -> Option<u32>,
) -> Result<Lines, LineError> {
    place_sourced_runs_normal(
        runs.iter().map(|&(text, bold)| (text, bold, 0)),
        max_width,
        &mut advance,
    )
}

#[allow(clippy::too_many_lines)] // Tokenization retains source with each face and advance.
fn place_sourced_runs_normal<'a>(
    runs: impl Iterator<Item = (&'a str, bool, usize)> + Clone,
    max_width: u32,
    mut advance: impl FnMut(char, bool) -> Option<u32>,
) -> Result<Lines, LineError> {
    if max_width == 0 {
        return Err(LineError::InvalidMetrics);
    }
    let mut words: Vec<MeasuredWord> = Vec::new();
    let mut word: Vec<MeasuredLetter> = Vec::new();
    let mut letters = 0_usize;
    // The current placer only has whitespace wrap opportunities. A word
    // containing ASCII hyphen or break punctuation needs CSS/Unicode line
    // breaking semantics when it wraps. An entirely fitting single line has
    // no break decision and can retain its punctuation. A narrow exception
    // allows alphanumeric words with one terminal sentence mark: those
    // marks stay with the word and the next whitespace is the break.
    let needs_break_rules = runs
        .clone()
        .any(|(text, _, _)| text.chars().any(needs_extra_break_rules));
    let mut pending_space: Option<(bool, usize)> = None;
    for (text, bold, leaf) in runs {
        for ch in text.chars() {
            if ch.is_ascii_whitespace() {
                if !word.is_empty() {
                    words.push((std::mem::take(&mut word), pending_space.take()));
                } else if !words.is_empty() && pending_space.is_none() {
                    // Collapsed boundary whitespace: the first run's face
                    // measures the single surviving space.
                    pending_space = Some((bold, leaf));
                }
                if pending_space.is_none() {
                    pending_space = Some((bold, leaf));
                }
            } else {
                if !ch.is_ascii_graphic() && ch != '\u{a0}' {
                    return Err(LineError::UnsupportedText);
                }
                let width = advance(ch, bold)
                    .filter(|&n| n > 0)
                    .ok_or(LineError::InvalidMetrics)?;
                if letters >= MAX_GLYPH_BYTES {
                    return Err(LineError::TooManyGlyphs);
                }
                word.push((ch, width, bold, leaf));
                letters += 1;
            }
        }
    }
    if !word.is_empty() {
        words.push((word, pending_space.take()));
    }
    // One measured space per word boundary, flagged with its own face.
    let mut spaces: Vec<Option<(u32, bool, usize)>> = vec![None; words.len()];
    for index in 1..words.len() {
        let (flag, leaf) = words[index].1.ok_or(LineError::InvalidMetrics)?;
        spaces[index] = Some((
            advance(' ', flag)
                .filter(|&n| n > 0)
                .ok_or(LineError::InvalidMetrics)?,
            flag,
            leaf,
        ));
    }
    if needs_break_rules
        && !words.iter().all(|(word, _)| sentence_word(word))
        && !fits_single_line(&words, &spaces, max_width)
    {
        return Err(LineError::UnsupportedText);
    }
    let mut glyphs = Vec::new();
    let mut x = 0_u32;
    let mut line = 0_u32;
    for (index, (word, _)) in words.into_iter().enumerate() {
        let mut width = 0_u32;
        for &(_, value, _, _) in &word {
            width = width.checked_add(value).ok_or(LineError::UnbreakableWord)?;
        }
        if width > max_width {
            return Err(LineError::UnbreakableWord);
        }
        if x > 0 {
            let (space, space_bold, space_leaf) = spaces[index].ok_or(LineError::InvalidMetrics)?;
            let next = x.checked_add(space).and_then(|v| v.checked_add(width));
            if next.is_none_or(|n| n > max_width) {
                line = line.checked_add(1).ok_or(LineError::TooManyLines)?;
                if line as usize >= MAX_LINES {
                    return Err(LineError::TooManyLines);
                }
                x = 0;
            } else {
                if glyphs.len() >= MAX_GLYPH_BYTES {
                    return Err(LineError::TooManyGlyphs);
                }
                glyphs.push(PositionedGlyph {
                    character: ' ',
                    x,
                    line,
                    advance: space,
                    bold: space_bold,
                    leaf: space_leaf,
                });
                x += space;
            }
        }
        for (character, value, bold, leaf) in word {
            glyphs.push(PositionedGlyph {
                character,
                x,
                line,
                advance: value,
                bold,
                leaf,
            });
            x += value; // checked by the word and line fit above
        }
    }
    Ok(Lines {
        count: if glyphs.is_empty() { 0 } else { line + 1 },
        glyphs,
    })
}

// ---- PROTOTYPE (host research; not wired to FontProvider or any painter) ----
/// One shaped glyph cluster. Indices are Unicode scalar (char) indices into the
/// run text, NOT HarfBuzz byte offsets: the provider boundary converts. The
/// prototype admits only 1:1 clusters (`char_len == 1`) because the current
/// raster is per char and cannot draw a ligature. `units` is in the rule's sub-pixel unit (tests use 64; the recommended minimum is 1024 per px).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShapedCluster {
    pub first_char: u32,
    pub char_len: u32,
    pub units: u32,
}

/// Optional context-aware shaper for one run of identical resolved font
/// parameters (size, bold, face). `None` means "not eligible": the caller
/// uses the unchanged integer path for the whole block.
pub trait RunShaper {
    fn shape_run(&self, text: &str, size: u32, bold: bool) -> Option<Vec<ShapedCluster>>;
}

/// Which path produced the lines. `Shaped` x/advance are in 1/64 px and
/// `max_width` was compared in 1/64 px; `Legacy` is byte-identical to
/// `place_runs_normal`.
#[derive(Debug, Eq, PartialEq)]
pub enum Placed {
    Legacy(Lines),
    Shaped(Lines),
}

/// Explicit comparison rule. `unit` is sub-pixel units per px used by the
/// provider's advances (64 = 1/64 px). `slack == 0` is strict; a nonzero
/// value (in the same units) is a fitted tolerance, never a Blink rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FitRule {
    pub unit: u32,
    pub slack: u32,
}

/// Prototype: shaped placement with whole-block legacy fallback. Any
/// ineligibility (non-ASCII text, shaper `None`, malformed clusters, any
/// internal inconsistency or placer error on the shaped attempt) returns the
/// unchanged legacy result for the same inputs. The shaped attempt reuses
/// `place_sourced_runs_normal` itself (same tokenizer, collapse and break
/// rules) with per-occurrence context advances replayed in the order that
/// function requests them, so word attribution cannot diverge from it.
///
/// # Errors
/// Exactly the legacy placer's errors when the legacy path is taken.
pub fn place_runs_shaped(
    runs: &[(&str, bool)],
    max_width: u32,
    size: u32,
    shaper: &dyn RunShaper,
    fit: FitRule,
    mut legacy: impl FnMut(char, bool) -> Option<u32>,
) -> Result<Placed, LineError> {
    let sourced: Vec<(&str, bool, usize)> = runs.iter().map(|&(t, b)| (t, b, 0)).collect();
    if let Some(lines) = shaped_attempt(&sourced, max_width, size, shaper, fit) {
        return Ok(Placed::Shaped(lines));
    }
    place_sourced_runs_normal(
        runs.iter().map(|&(text, bold)| (text, bold, 0)),
        max_width,
        &mut legacy,
    )
    .map(Placed::Legacy)
}

fn shaped_attempt(
    runs: &[(&str, bool, usize)],
    max_width: u32,
    size: u32,
    shaper: &dyn RunShaper,
    fit: FitRule,
) -> Option<Lines> {
    if max_width == 0 || size == 0 {
        return None;
    }
    // ASCII-only block (graphic + ASCII whitespace); U+00A0 and everything
    // else defers to the legacy path.
    if runs
        .iter()
        .any(|(t, _, _)| t.chars().any(|c| !(c.is_ascii_graphic() || c.is_ascii_whitespace())))
    {
        return None;
    }
    // Mirror of place_sourced_runs_normal's tokenization: words with the
    // flag of the space preceding each.
    let mut words: Vec<(Vec<(char, bool)>, Option<bool>)> = Vec::new();
    let mut word: Vec<(char, bool)> = Vec::new();
    let mut pending: Option<bool> = None;
    for &(text, bold, _) in runs {
        for ch in text.chars() {
            if ch.is_ascii_whitespace() {
                if !word.is_empty() {
                    words.push((std::mem::take(&mut word), pending.take()));
                } else if !words.is_empty() && pending.is_none() {
                    pending = Some(bold);
                }
                if pending.is_none() {
                    pending = Some(bold);
                }
            } else {
                word.push((ch, bold));
            }
        }
    }
    if !word.is_empty() {
        words.push((word, pending.take()));
    }
    // Collapsed paragraph with each char's face flag.
    let mut seq: Vec<(char, bool)> = Vec::new();
    for (index, (letters, space)) in words.iter().enumerate() {
        if index > 0 {
            seq.push((' ', (*space)?));
        }
        seq.extend(letters.iter().copied());
    }
    if seq.is_empty() {
        return None; // nothing to shape: legacy result (empty page) is authoritative
    }
    // Shape each maximal same-face run once; require exact 1:1 coverage.
    let mut advances: Vec<u32> = Vec::with_capacity(seq.len());
    let mut start = 0;
    while start < seq.len() {
        let bold = seq[start].1;
        let mut end = start;
        while end < seq.len() && seq[end].1 == bold {
            end += 1;
        }
        let text: String = seq[start..end].iter().map(|&(c, _)| c).collect();
        let clusters = shaper.shape_run(&text, size, bold)?;
        if clusters.len() != end - start {
            return None;
        }
        for (i, cluster) in clusters.iter().enumerate() {
            if usize::try_from(cluster.first_char).ok()? != i
                || cluster.char_len != 1
                || cluster.units == 0
            {
                return None;
            }
            advances.push(cluster.units);
        }
        start = end;
    }
    // Replay queues in the order the placer asks: all letters (in order),
    // then one space per boundary (in order).
    let mut letters = std::collections::VecDeque::new();
    let mut spaces = std::collections::VecDeque::new();
    for (&(ch, bold), &adv) in seq.iter().zip(&advances) {
        if ch == ' ' {
            spaces.push_back((bold, adv));
        } else {
            letters.push_back((ch, bold, adv));
        }
    }
    let limit = max_width.checked_mul(fit.unit)?.checked_add(fit.slack)?;
    let lines = place_sourced_runs_normal(
        runs.iter().copied(),
        limit,
        |ch, bold| {
            if ch == ' ' {
                let (flag, adv) = spaces.pop_front()?;
                (flag == bold).then_some(adv)
            } else {
                let (expected, flag, adv) = letters.pop_front()?;
                (expected == ch && flag == bold).then_some(adv)
            }
        },
    )
    .ok()?;
    (letters.is_empty() && spaces.is_empty()).then_some(lines)
}

/// Sub-pixel units per px for shaped layout (>= 1024: 1/64 measurably loses accuracy).
pub const SHAPED_UNIT: u32 = 1024;

/// Opt-in shaped placement parameters carried through the height passes and
/// the painter. `None` everywhere means the unchanged legacy path.
#[derive(Clone, Copy)]
pub struct TextShaping<'a> {
    pub shaper: &'a dyn RunShaper,
    /// Strict when `slack == 0`; fitted otherwise. Not a Blink-derived rule.
    pub fit: FitRule,
}

/// THE single named unit-to-pixel conversion: round half up, applied once to
/// a cumulative sub-pixel position (never to a sum of pre-rounded advances).
#[must_use]
pub fn px_round_half_up(units: u32, unit: u32) -> Option<u32> {
    units.checked_add(unit / 2)?.checked_div(unit)
}

/// Prototype: shaped lines for one block as an ordinary integer-px
/// `DirectTextLines`, or `None` when the block is not eligible (the caller
/// then uses the legacy path for the WHOLE page). Eligibility: same block
/// shape as `place_runs_text`, left/initial alignment only (other
/// alignments stay legacy until Chrome truth exists), ASCII text, valid
/// 1:1 clusters, and a px conversion with no zero-width cell and every glyph
/// origin inside the content box.
pub fn shaped_block_lines(
    tree: &BoxTree,
    parent: usize,
    max_width: u32,
    shaping: &TextShaping,
    mut line_height: impl FnMut(u32) -> Option<u32>,
) -> Option<DirectTextLines> {
    if tree.truncated || tree.unsupported {
        return None;
    }
    let node = tree.boxes.get(parent)?;
    if node.kind != BoxKind::Block
        || node.style.direction != Direction::Ltr
        || node.children.is_empty()
        || node.text.is_some()
        || !matches!(node.style.text_align, TextAlign::Initial | TextAlign::Left)
    {
        return None;
    }
    let size = node.style.font_size;
    let (runs, leaves) = collect_inline_runs(tree, parent).ok()?;
    let units = shaped_attempt(&runs, max_width, size, shaping.shaper, shaping.fit)?;
    let unit = shaping.fit.unit;
    let mut glyphs = Vec::with_capacity(units.glyphs.len());
    for g in &units.glyphs {
        let x = px_round_half_up(g.x, unit)?;
        let end = px_round_half_up(g.x.checked_add(g.advance)?, unit)?;
        let advance = end.checked_sub(x).filter(|&a| a > 0)?;
        if x >= max_width {
            return None;
        }
        glyphs.push(PositionedGlyph {
            character: g.character,
            x,
            line: g.line,
            advance,
            bold: g.bold,
            leaf: g.leaf,
        });
    }
    let lines = Lines { glyphs, count: units.count };
    let natural = line_height(size)?;
    let height = node.style.used_line_height(natural).filter(|&v| v > 0)?;
    let content_height = lines.count.checked_mul(height)?;
    Some(DirectTextLines {
        source: *leaves.first()?,
        leaves,
        lines,
        content_height,
    })
}
// ---- END PROTOTYPE ----

fn needs_extra_break_rules(ch: char) -> bool {
    matches!(
        ch,
        '\'' | '-'
            | '/'
            | '\u{ad}'
            | '!'
            | ','
            | '.'
            | ':'
            | ';'
            | '?'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
    )
}

// Deliberately excludes other internal punctuation, unmatched brackets, surrounding
// quotes, runs of punctuation, hyphens and slashes. One ASCII apostrophe
// between alphabetic stems stays inside a contraction or possessive.
fn sentence_word(word: &[MeasuredLetter]) -> bool {
    word.split(|&(ch, _, _, _)| ch == '\u{a0}')
        .all(sentence_piece)
}

fn sentence_piece(word: &[MeasuredLetter]) -> bool {
    let Some(&(last, _, _, _)) = word.last() else {
        return false;
    };
    let stem = if matches!(last, '.' | ',' | '!' | '?' | ':' | ';') {
        &word[..word.len() - 1]
    } else {
        word
    };
    let stem = match (stem.first(), stem.last()) {
        (Some(&(open, _, _, _)), Some(&(close, _, _, _)))
            if matches!((open, close), ('(', ')') | ('[', ']') | ('{', '}')) =>
        {
            &stem[1..stem.len() - 1]
        }
        _ => stem,
    };
    if !stem.is_empty() && stem.iter().all(|&(ch, _, _, _)| ch.is_ascii_alphanumeric()) {
        return true;
    }
    let mut pieces = stem.split(|&(ch, _, _, _)| ch == '\'');
    let alphabetic = |part: &[MeasuredLetter]| {
        !part.is_empty() && part.iter().all(|&(ch, _, _, _)| ch.is_ascii_alphabetic())
    };
    pieces.next().is_some_and(alphabetic)
        && pieces.next().is_some_and(alphabetic)
        && pieces.next().is_none()
}

fn fits_single_line(
    words: &[MeasuredWord],
    spaces: &[Option<(u32, bool, usize)>],
    max_width: u32,
) -> bool {
    let mut full_width = 0_u32;
    for (index, (word, _)) in words.iter().enumerate() {
        if index > 0 {
            let Some(Some((space, _, _))) = spaces.get(index) else {
                return false;
            };
            let Some(next) = full_width.checked_add(*space) else {
                return false;
            };
            full_width = next;
        }
        for &(_, width, _, _) in word {
            let Some(next) = full_width.checked_add(width) else {
                return false;
            };
            full_width = next;
        }
    }
    full_width <= max_width
}

/// Measured lines for one directly contained text node. Multiple inline
/// children, nested spans, anonymous blocks and other inline contexts need a
/// shared formatting context, so they cannot use this narrow entry point.
#[derive(Debug, Eq, PartialEq)]
pub struct DirectTextLines {
    /// First leaf source retained for compatibility; paint uses leaves exclusively.
    pub source: usize,
    /// Exact styled text-leaf nodes, indexed by each positioned glyph.
    /// No first-leaf fallback is allowed for a missing or invalid key.
    pub leaves: Vec<usize>,
    pub lines: Lines,
    pub content_height: u32,
}

/// Measure a single direct text child with an external, exact font provider.
/// The parent must be an LTR normal-flow block with one text child. List
/// markers and RTL line alignment are unsupported, including ASCII RTL.
/// Font size comes from the text node's inherited computed style. This result
/// is geometry, not a glyph raster or a license to paint a partial document.
///
/// # Errors
/// Fails on unsupported tree shape, unavailable metrics, width/height
/// overflow, or text beyond this module's deliberately narrow ASCII domain.
pub fn place_direct_text(
    tree: &BoxTree,
    parent: usize,
    max_width: u32,
    mut advance: impl FnMut(char, u32) -> Option<u32>,
    mut line_height: impl FnMut(u32) -> Option<u32>,
) -> Result<DirectTextLines, LineError> {
    let node = tree.boxes.get(parent).ok_or(LineError::UnsupportedTree)?;
    if node.children.len() != 1
        || !matches!(
            tree.boxes.get(node.children[0]).map(|child| child.kind),
            Some(BoxKind::Text)
        )
    {
        return Err(LineError::UnsupportedTree);
    }
    // The weight-ignoring provider measures regular geometry only; refuse a
    // bold run rather than laying it out with the wrong face's advances.
    let lines = place_runs_text(
        tree,
        parent,
        max_width,
        |ch, size, _bold| advance(ch, size),
        &mut line_height,
    )?;
    if lines.lines.glyphs.iter().any(|glyph| glyph.bold) {
        return Err(LineError::UnsupportedTree);
    }
    Ok(lines)
}

/// Measure a block's inline runs with an external, exact font provider.
/// The parent must be an LTR normal-flow block whose children are all text
/// nodes or recursively bare inline elements: one shared inline formatting
/// context. Every run must keep the block's font size, colour, style and
/// line-height; only weight 400/700 may vary, and the bold run is measured
/// with the provider's bold face. Inline boxes with any margin, padding,
/// border or background, italic/oblique runs, other weights
/// and RTL all refuse rather than paint a partial formatting context. List
/// markers stay unsupported. LTR left/right/center alignment uses measured
/// per-line advances; fractional center offsets refuse rather than round.
/// This result is geometry, not a glyph raster
/// or a license to paint a partial document.
///
/// # Errors
/// Fails on unsupported tree shape, unavailable metrics, width/height
/// overflow, or text beyond this module's deliberately narrow ASCII domain.
#[allow(clippy::too_many_lines)] // Existing admission checks plus exact source collection.
pub fn place_runs_text(
    tree: &BoxTree,
    parent: usize,
    max_width: u32,
    mut advance: impl FnMut(char, u32, bool) -> Option<u32>,
    mut line_height: impl FnMut(u32) -> Option<u32>,
) -> Result<DirectTextLines, LineError> {
    if tree.truncated || tree.unsupported {
        return Err(LineError::UnsupportedTree);
    }
    let node = tree.boxes.get(parent).ok_or(LineError::UnsupportedTree)?;
    // This placer emits LTR glyphs with proven per-line alignment, no list marker.
    // Direction also controls the initial inline alignment, even for ASCII.
    if node.kind != BoxKind::Block
        || node.style.direction != Direction::Ltr
        || node.children.is_empty()
        || node.text.is_some()
    {
        return Err(LineError::UnsupportedTree);
    }
    let size = node.style.font_size;
    if size == 0 || max_width == 0 {
        return Err(LineError::InvalidMetrics);
    }
    let (runs, leaves) = collect_inline_runs(tree, parent)?;
    let mut lines = place_sourced_runs_normal(runs.iter().copied(), max_width, |ch, bold| {
        advance(ch, size, bold)
    })?;
    align_lines(&mut lines, max_width, node.style.text_align)?;
    let natural = line_height(size).ok_or(LineError::InvalidMetrics)?;
    let height = node
        .style
        .used_line_height(natural)
        .filter(|&value| value > 0)
        .ok_or(LineError::InvalidMetrics)?;
    let content_height = lines
        .count
        .checked_mul(height)
        .ok_or(LineError::InvalidMetrics)?;
    Ok(DirectTextLines {
        source: *leaves.first().ok_or(LineError::UnsupportedTree)?,
        leaves,
        lines,
        content_height,
    })
}

/// Validate an entire bare inline subtree before asking any font callbacks.
/// Owner depth is zero; the first inline element is depth one.
type SourcedRuns<'a> = (Vec<(&'a str, bool, usize)>, Vec<usize>);

pub(crate) fn collect_inline_runs(
    tree: &BoxTree,
    parent: usize,
) -> Result<SourcedRuns<'_>, LineError> {
    collect_inline_runs_bounded(tree, parent, 96, 4096, 4096, 4096)
}

#[allow(clippy::too_many_lines)] // Full validation and bounded source collection are atomic.
fn collect_inline_runs_bounded(
    tree: &BoxTree,
    parent: usize,
    max_depth: usize,
    max_visited: usize,
    max_edges: usize,
    max_leaves: usize,
) -> Result<SourcedRuns<'_>, LineError> {
    let owner = tree.boxes.get(parent).ok_or(LineError::UnsupportedTree)?;
    if tree.unsupported
        || tree.truncated
        || owner.kind != BoxKind::Block
        || owner.children.is_empty()
    {
        return Err(LineError::UnsupportedTree);
    }
    let mut runs = Vec::new();
    let mut leaves = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(parent);
    if owner.children.len() > max_edges {
        return Err(LineError::UnsupportedTree);
    }
    let mut stack = Vec::new();
    for &child in owner.children.iter().rev() {
        stack.push((child, parent, 0_usize));
    }
    let mut visited = 0_usize;
    let mut edges = owner.children.len();
    while let Some((index, expected, depth)) = stack.pop() {
        visited += 1;
        if visited > max_visited || edges > max_edges || !seen.insert(index) {
            return Err(LineError::UnsupportedTree);
        }
        let n = tree.boxes.get(index).ok_or(LineError::UnsupportedTree)?;
        if n.parent != Some(expected) || n.source.is_none() {
            return Err(LineError::UnsupportedTree);
        }
        let st = n.style;
        if st.direction != Direction::Ltr
            || st.font_size != owner.style.font_size
            || st.font_family.is_some()
            || st.font_style != crate::computed_style::FontStyle::Normal
            || st.color != owner.style.color
            || !matches!(st.font_weight, 400 | 700)
            || st.line_height != owner.style.line_height
            || st.line_height_number != owner.style.line_height_number
        {
            return Err(LineError::UnsupportedTree);
        }
        match n.kind {
            BoxKind::Text => {
                if !n.children.is_empty() || leaves.len() >= max_leaves {
                    return Err(LineError::UnsupportedTree);
                }
                let text = n.text.as_deref().ok_or(LineError::UnsupportedTree)?;
                runs.push((text, st.font_weight == 700, leaves.len()));
                leaves.push(n.source.ok_or(LineError::UnsupportedTree)?);
            }
            BoxKind::Inline => {
                let depth = depth + 1;
                // Preserve the old topology: an owner-direct inline wrapping
                // exactly one Text leaf did not require a particular tag name.
                // Only newly admitted nested/empty shapes require phrase identity.
                let legacy_direct = expected == parent
                    && n.children.len() == 1
                    && tree
                        .boxes
                        .get(n.children[0])
                        .is_some_and(|child| child.kind == BoxKind::Text);
                if depth > max_depth
                    || !n.inline_attributes
                    || (!legacy_direct && !n.inline_phrase)
                    || n.text.is_some()
                    || st.positioned.position != crate::computed_style::Position::Static
                    || [
                        st.margin_left,
                        st.margin_right,
                        st.margin_top,
                        st.margin_bottom,
                    ]
                    .iter()
                    .any(|m| *m != crate::computed_style::Margin::Px(0))
                    || [
                        st.padding_left,
                        st.padding_right,
                        st.padding_top,
                        st.padding_bottom,
                    ]
                    .iter()
                    .any(|&p| p != 0)
                    || st.background_color.is_some()
                    || st.background_current_color
                    || st.decor.has_painted_border()
                    || st.decor.layers != 1
                {
                    return Err(LineError::UnsupportedTree);
                }
                edges = edges
                    .checked_add(n.children.len())
                    .ok_or(LineError::UnsupportedTree)?;
                if edges > max_edges {
                    return Err(LineError::UnsupportedTree);
                }
                for &child in n.children.iter().rev() {
                    stack.push((child, index, depth));
                }
            }
            _ => return Err(LineError::UnsupportedTree),
        }
    }
    if leaves.is_empty() {
        return Err(LineError::UnsupportedTree);
    }
    Ok((runs, leaves))
}

// Normal whitespace has already removed leading/trailing spaces. Align each
// line by advance extent, not ink bounds, preserving the same wrapping/face.
fn align_lines(lines: &mut Lines, width: u32, align: TextAlign) -> Result<(), LineError> {
    if matches!(align, TextAlign::Initial | TextAlign::Left) {
        return Ok(());
    }
    let mut start = 0;
    while start < lines.glyphs.len() {
        let line = lines.glyphs[start].line;
        let mut end = start + 1;
        while end < lines.glyphs.len() && lines.glyphs[end].line == line {
            end += 1;
        }
        let last = &lines.glyphs[end - 1];
        let extent = last
            .x
            .checked_add(last.advance)
            .ok_or(LineError::InvalidMetrics)?;
        let remaining = width.checked_sub(extent).ok_or(LineError::InvalidMetrics)?;
        let shift = if align == TextAlign::Center {
            if remaining % 2 != 0 {
                return Err(LineError::FractionalAlignment);
            }
            remaining / 2
        } else {
            remaining
        };
        for glyph in &mut lines.glyphs[start..end] {
            glyph.x = glyph
                .x
                .checked_add(shift)
                .ok_or(LineError::InvalidMetrics)?;
        }
        start = end;
    }
    Ok(())
}

/// A measured glyph's own bitmap and offsets relative to its baseline.
/// The provider must return masks for the same face and size used to measure
/// advances; this function cannot verify a provider's font identity.
pub struct GlyphBitmap {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlyphPaintError {
    InvalidMetrics,
    InvalidBitmap,
    OutsideLine,
    TooManyCommands,
    TooManyGlyphBytes,
    Allocation,
}

/// Convert already measured direct text into a separate bounded glyph list.
/// `line_top` is the first line's content-box y; `baseline_offset` and
/// `line_height` come from the actual font provider. Advances must fit the
/// content width; horizontal glyph bearings may extend outside it, as CSS
/// visible overflow does not clip ink to the content box or line box. The
/// baseline may lie outside a short CSS line after negative half-leading.
/// The caller validates the natural font baseline and combines this list
/// with backgrounds only after the *whole page* is proven paintable.
///
/// # Errors
/// Refuses missing/invalid bitmaps, coordinate overflow, invalid placement, and command or
/// glyph byte budgets. It never returns a partially painted list.
#[allow(clippy::too_many_arguments)]
pub fn paint_direct_glyphs(
    text: &DirectTextLines,
    content_x: i32,
    line_top: i32,
    content_width: u32,
    size: u32,
    line_height: u32,
    baseline_offset: i32,
    color: Rgb,
    mut raster: impl FnMut(char, u32, bool) -> Option<GlyphBitmap>,
) -> Result<DisplayList, GlyphPaintError> {
    if size == 0
        || line_height == 0
        || content_width == 0
        || text.content_height
            != text
                .lines
                .count
                .checked_mul(line_height)
                .ok_or(GlyphPaintError::InvalidMetrics)?
    {
        return Err(GlyphPaintError::InvalidMetrics);
    }
    if text.lines.glyphs.len() > crate::display_list::MAX_COMMANDS {
        return Err(GlyphPaintError::TooManyCommands);
    }
    if text.lines.glyphs.is_empty() != (text.lines.count == 0) {
        return Err(GlyphPaintError::InvalidMetrics);
    }
    if text.leaves.first().copied() != Some(text.source)
        || text
            .lines
            .glyphs
            .iter()
            .any(|g| g.leaf >= text.leaves.len())
    {
        return Err(GlyphPaintError::InvalidMetrics);
    }
    let mut list = DisplayList::default();
    for glyph in &text.lines.glyphs {
        if glyph.line >= text.lines.count || glyph.x >= content_width {
            return Err(GlyphPaintError::OutsideLine);
        }
        let bitmap =
            raster(glyph.character, size, glyph.bold).ok_or(GlyphPaintError::InvalidBitmap)?;
        let bytes = usize::try_from(bitmap.width)
            .ok()
            .and_then(|w| {
                usize::try_from(bitmap.height)
                    .ok()
                    .and_then(|h| w.checked_mul(h))
            })
            .ok_or(GlyphPaintError::InvalidBitmap)?;
        if bytes != bitmap.coverage.len() {
            return Err(GlyphPaintError::InvalidBitmap);
        }
        if bytes == 0 {
            continue; // e.g. a measured space has no ink
        }
        let x = i64::from(content_x) + i64::from(glyph.x) + i64::from(bitmap.left);
        let line_y = i64::from(line_top) + i64::from(glyph.line) * i64::from(line_height);
        let y = line_y + i64::from(baseline_offset) + i64::from(bitmap.top);
        // Proven layout extent: the glyph's own cell from its measured pen
        // position, one line-box slice tall. Ink bounds may overhang it via
        // font bearings; seams that refuse off-page layout test this rect.
        let layout = Rect {
            x: i32::try_from(i64::from(content_x) + i64::from(glyph.x))
                .map_err(|_| GlyphPaintError::InvalidMetrics)?,
            y: i32::try_from(line_y).map_err(|_| GlyphPaintError::InvalidMetrics)?,
            width: glyph.advance,
            height: line_height,
        };
        list.glyph_run(
            Rect {
                x: i32::try_from(x).map_err(|_| GlyphPaintError::InvalidMetrics)?,
                y: i32::try_from(y).map_err(|_| GlyphPaintError::InvalidMetrics)?,
                width: bitmap.width,
                height: bitmap.height,
            },
            layout,
            bitmap.coverage,
            color,
            Source {
                node: Some(
                    *text
                        .leaves
                        .get(glyph.leaf)
                        .ok_or(GlyphPaintError::InvalidMetrics)?,
                ),
                action: None,
            },
        )
        .map_err(|error| match error {
            DisplayError::TooManyCommands => GlyphPaintError::TooManyCommands,
            DisplayError::GlyphBudget => GlyphPaintError::TooManyGlyphBytes,
            DisplayError::Allocation => GlyphPaintError::Allocation,
            _ => GlyphPaintError::InvalidBitmap,
        })?;
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_whitespace_and_wraps_on_word_boundary() {
        let lines = place_ascii_normal("  Hello\t world  \n again ", 35, |ch| {
            Some(if ch == ' ' { 3 } else { 6 })
        })
        .unwrap();
        assert_eq!(lines.count, 3);
        assert_eq!(
            lines.glyphs.iter().filter(|g| g.character == ' ').count(),
            0
        );
        assert_eq!(
            lines.glyphs[0],
            PositionedGlyph {
                character: 'H',
                x: 0,
                line: 0,
                advance: 6,
                bold: false,
                leaf: 0
            }
        );
        assert_eq!(
            lines.glyphs[5],
            PositionedGlyph {
                character: 'w',
                x: 0,
                line: 1,
                advance: 6,
                bold: false,
                leaf: 0
            }
        );
        assert_eq!(
            lines.glyphs[10],
            PositionedGlyph {
                character: 'a',
                x: 0,
                line: 2,
                advance: 6,
                bold: false,
                leaf: 0
            }
        );
    }

    #[test]
    fn keeps_collapsed_space_inside_line() {
        let lines = place_ascii_normal(" a   b ", 11, |_| Some(3)).unwrap();
        assert_eq!(lines.count, 1);
        assert_eq!(
            lines
                .glyphs
                .iter()
                .map(|g| (g.character, g.x))
                .collect::<Vec<_>>(),
            [('a', 0), (' ', 3), ('b', 6)]
        );
    }

    #[test]
    fn paints_only_supplied_mask_with_text_source_and_no_partial_result() {
        let text = DirectTextLines {
            source: 7,
            leaves: vec![7],
            content_height: 12,
            lines: Lines {
                count: 1,
                glyphs: vec![PositionedGlyph {
                    character: 'A',
                    x: 2,
                    line: 0,
                    advance: 3,
                    bold: false,
                    leaf: 0,
                }],
            },
        };
        let raster = |_, _, _| {
            Some(GlyphBitmap {
                left: 1,
                top: -3,
                width: 2,
                height: 2,
                coverage: vec![255, 128, 64, 0],
            })
        };
        let list = paint_direct_glyphs(&text, 3, 4, 20, 16, 12, 6, Rgb(0, 0, 0), raster).unwrap();
        assert!(matches!(
            &list.commands()[0],
            crate::display_list::Command::GlyphRun {
                bounds: Rect {
                    x: 6,
                    y: 7,
                    width: 2,
                    height: 2
                },
                source: Source {
                    node: Some(7),
                    action: None
                },
                ..
            }
        ));
        let pixels = list.rasterize(20, 20, Rgb(255, 255, 255)).unwrap();
        let at = |x: usize, y: usize| &pixels[((y * 20 + x) * 4)..((y * 20 + x) * 4 + 4)];
        assert_eq!(at(6, 7), &[0, 0, 0, 255]);
        assert_eq!(at(7, 7), &[127, 127, 127, 255]);
        assert_eq!(at(5, 7), &[255, 255, 255, 255]);
        let overflow = paint_direct_glyphs(&text, 3, 4, 20, 16, 12, 6, Rgb(0, 0, 0), |_, _, _| {
            Some(GlyphBitmap {
                left: 1,
                top: -7,
                width: 2,
                height: 2,
                coverage: vec![255, 128, 64, 0],
            })
        })
        .unwrap();
        assert!(
            matches!(&overflow.commands()[0], crate::display_list::Command::GlyphRun { bounds, .. } if bounds.y == 3)
        );
        assert_eq!(
            paint_direct_glyphs(&text, 3, 4, 20, 16, 12, 6, Rgb(0, 0, 0), |_, _, _| Some(
                GlyphBitmap {
                    left: 1,
                    top: -3,
                    width: 2,
                    height: 2,
                    coverage: vec![255]
                }
            ))
            .err(),
            Some(GlyphPaintError::InvalidBitmap)
        );
    }

    #[test]
    fn direct_text_uses_inherited_size_and_rejects_other_inline_contexts() {
        use crate::{box_tree::BoxTree, parse_style_tree, Limits};
        let tree = |html: &str| {
            BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
        };
        let single = tree("<html><body><p style='font-size:20px'>ab cd</p></body></html>");
        let p = single
            .boxes
            .iter()
            .position(|node| node.style.font_size == 20 && node.kind == BoxKind::Block)
            .unwrap();
        let laid_out = place_direct_text(
            &single,
            p,
            25,
            |_, size| Some(size / 2),
            |size| Some(size + 4),
        )
        .unwrap();
        assert_eq!(laid_out.content_height, 48);
        assert_eq!(laid_out.lines.count, 2);
        assert_eq!(laid_out.lines.glyphs[2].line, 1);
        assert_eq!(
            laid_out.source,
            single.boxes[single.boxes[p].children[0]].source.unwrap()
        );
        let mixed = tree("<html><body><p>one <em>two</em></p></body></html>");
        let p = mixed
            .boxes
            .iter()
            .position(|node| node.kind == BoxKind::Block && node.children.len() > 1)
            .unwrap();
        assert_eq!(
            place_direct_text(&mixed, p, 100, |_, _| Some(5), |_| Some(16)),
            Err(LineError::UnsupportedTree)
        );
    }

    #[test]
    fn refuses_unsupported_text_and_unproven_measurements() {
        for text in ["one-two", "one/two", "one\u{ad}two", "one,two", "one.two"] {
            assert_eq!(
                place_ascii_normal(text, 10, |_| Some(5)),
                Err(LineError::UnsupportedText)
            );
        }
        assert_eq!(
            place_ascii_normal("a🙂", 20, |_| Some(5)),
            Err(LineError::UnsupportedText)
        );
        assert_eq!(
            place_ascii_normal("\u{1b}", 20, |_| Some(5)),
            Err(LineError::UnsupportedText)
        );
        assert_eq!(
            place_ascii_normal("word", 10, |_| Some(5)),
            Err(LineError::UnbreakableWord)
        );
        assert_eq!(
            place_ascii_normal("x", 0, |_| Some(5)),
            Err(LineError::InvalidMetrics)
        );
        assert_eq!(
            place_ascii_normal("x", 20, |_| None),
            Err(LineError::InvalidMetrics)
        );
        assert_eq!(
            place_ascii_normal("x", 20, |_| Some(0)),
            Err(LineError::InvalidMetrics)
        );
        assert_eq!(place_ascii_normal("   ", 20, |_| None).unwrap().count, 0);
    }
}

#[cfg(test)]
mod punctuation_tests {
    use super::*;

    #[test]
    fn punctuation_that_fits_needs_no_line_break_guess() {
        for text in ["a-b", "a/b", "a,b", "a.b", "(ab)!", "[ab]?"] {
            let lines = place_ascii_normal(text, 100, |_| Some(3)).unwrap();
            assert_eq!(lines.count, 1);
            assert_eq!(
                lines.glyphs.iter().map(|g| g.character).collect::<String>(),
                text
            );
            assert_eq!(
                place_ascii_normal(text, 3, |_| Some(3)),
                Err(if text.starts_with('(') || text.starts_with('[') {
                    LineError::UnbreakableWord
                } else {
                    LineError::UnsupportedText
                })
            );
        }
        let lines = place_ascii_normal("  a,b \t c.d  ", 21, |_| Some(3)).unwrap();
        assert_eq!(lines.count, 1);
        assert_eq!(
            lines.glyphs.iter().map(|g| g.character).collect::<String>(),
            "a,b c.d"
        );
        assert_eq!(
            place_ascii_normal("a,b c.d", 20, |_| Some(3)),
            Err(LineError::UnsupportedText)
        );
        assert_eq!(
            place_ascii_normal("a\u{ad}b", 100, |_| Some(3)),
            Err(LineError::UnsupportedText)
        );
    }
}

#[cfg(test)]
mod sentence_wrap_tests {
    use super::*;
    #[test]
    fn terminal_marks_stay_with_words_at_whitespace_wraps() {
        for mark in ['.', ',', '!', '?', ':', ';'] {
            let text = format!("  ab{mark}  cd{mark} ");
            let lines = place_ascii_normal(&text, 9, |_| Some(3)).unwrap();
            assert_eq!(lines.count, 2);
            assert_eq!(lines.glyphs[2].character, mark);
            assert_eq!(lines.glyphs[2].line, 0);
            assert_eq!(lines.glyphs[3].x, 0);
            assert_eq!(lines.glyphs[3].line, 1);
            assert_eq!(lines.glyphs[5].line, 1);
        }
    }
    #[test]
    fn exact_single_line_preserves_collapsed_space() {
        let lines = place_ascii_normal("ab. cd!", 21, |_| Some(3)).unwrap();
        assert_eq!(lines.count, 1);
        assert_eq!(lines.glyphs[3].character, ' ');
        assert_eq!(
            place_ascii_normal("abcd.", 9, |_| Some(3)),
            Err(LineError::UnbreakableWord)
        );
    }
    #[test]
    fn broader_punctuation_wraps_still_refuse() {
        for text in [
            "a-b cd.", "a/b cd.", "a.b cd.", "a,b cd.", "(ab cd.", "ab!! cd.", ". ab.", "ab. 'cd'",
        ] {
            assert_eq!(
                place_ascii_normal(text, 9, |_| Some(3)),
                Err(LineError::UnsupportedText),
                "{text}"
            );
        }
    }
}

#[cfg(test)]
mod apostrophe_wrap_tests {
    use super::*;
    #[test]
    fn internal_ascii_apostrophes_stay_in_sentence_words() {
        for text in ["don't stop.", "Alice's book!", "I'm here,", "isn't it?"] {
            let lines = place_ascii_normal(text, 21, |_| Some(3)).unwrap();
            assert_eq!(lines.count, 2, "{text}");
            let apostrophe = lines
                .glyphs
                .iter()
                .position(|g| g.character == '\'')
                .unwrap();
            assert_eq!(
                lines.glyphs[apostrophe - 1].line,
                lines.glyphs[apostrophe].line
            );
            assert_eq!(
                lines.glyphs[apostrophe + 1].line,
                lines.glyphs[apostrophe].line
            );
        }
    }
    #[test]
    fn ambiguous_quotes_and_multiple_apostrophes_still_refuse() {
        for text in [
            "'ab' cd.", "ab' cd.", "'ab cd.", "a'b'c d.", "a'b'c d", "1'2 cd.", "a'1 cd.",
        ] {
            assert_eq!(
                place_ascii_normal(text, 12, |_| Some(3)),
                Err(LineError::UnsupportedText),
                "{text}"
            );
        }
    }
}

#[cfg(test)]
mod nonbreaking_space_tests {
    use super::*;
    #[test]
    fn nonbreaking_space_uses_its_own_advance_and_stays_in_word() {
        let lines = place_ascii_normal("a\u{a0}b cd.", 15, |ch| {
            Some(if ch == '\u{a0}' { 7 } else { 3 })
        })
        .unwrap();
        assert_eq!(lines.count, 2);
        assert_eq!(lines.glyphs[1].character, '\u{a0}');
        assert_eq!(lines.glyphs[2].x, 10);
        assert_eq!(lines.glyphs[2].line, 0);
        assert_eq!(lines.glyphs[3].line, 1);
        assert_eq!(
            place_ascii_normal("a\u{a0}b", 12, |ch| Some(if ch == '\u{a0}' {
                7
            } else {
                3
            })),
            Err(LineError::UnbreakableWord)
        );
    }
    #[test]
    fn nonbreaking_space_is_not_trimmed_or_collapsed() {
        let lines = place_ascii_normal("\u{a0}a\u{a0}\u{a0}", 30, |_| Some(3)).unwrap();
        assert_eq!(lines.glyphs.len(), 4);
        assert_eq!(lines.glyphs[3].x, 9);
        assert_eq!(
            place_ascii_normal("a\u{a0}b", 30, |ch| (ch != '\u{a0}').then_some(3)),
            Err(LineError::InvalidMetrics)
        );
    }
}

#[cfg(test)]
mod bracket_word_tests {
    use super::*;
    #[test]
    fn single_paired_brackets_stay_on_their_word_line() {
        for text in [
            "(ab) cd.",
            "[ab] cd.",
            "{ab} cd.",
            "(ab)! cd.",
            "[I'm] here.",
        ] {
            let lines = place_ascii_normal(text, 18, |_| Some(3)).unwrap();
            assert_eq!(lines.count, 2, "{text}");
            let close = lines
                .glyphs
                .iter()
                .find(|g| matches!(g.character, ')' | ']' | '}'))
                .unwrap();
            assert_eq!(close.line, 0);
        }
    }
    #[test]
    fn unmatched_nested_and_multiword_brackets_still_refuse() {
        for text in [
            "(ab cd.",
            "ab) cd.",
            "[ab) cd.",
            "((ab)) cd.",
            "(ab cd) ef.",
            "() abc.",
        ] {
            assert_eq!(
                place_ascii_normal(text, 18, |_| Some(3)),
                Err(LineError::UnsupportedText),
                "{text}"
            );
        }
        assert_eq!(
            place_ascii_normal("(abcd)", 12, |_| Some(3)),
            Err(LineError::UnbreakableWord)
        );
    }
    #[test]
    fn runs_normal_tracks_per_face_advances_positions_and_flags() {
        // "Test is " regular, "no red" bold, "." regular: words, spaces and
        // bold flags follow the face that carried each glyph; the word
        // "red." spans the bold->regular boundary without a break.
        let lines = place_runs_normal(
            &[("Test is ", false), ("no red", true), (".", false)],
            800,
            |ch, bold| {
                Some(if ch == ' ' {
                    if bold {
                        3
                    } else {
                        2
                    }
                } else if bold {
                    4
                } else {
                    2
                })
            },
        )
        .unwrap();
        assert_eq!(lines.count, 1);
        assert_eq!(lines.glyphs.len(), 15);
        let at = |index: usize| {
            let glyph = lines.glyphs[index];
            (glyph.character, glyph.x, glyph.advance, glyph.bold)
        };
        assert_eq!(at(0), ('T', 0, 2, false));
        assert_eq!(at(4), (' ', 8, 2, false)); // boundary space from "is "
        assert_eq!(at(5), ('i', 10, 2, false));
        assert_eq!(at(7), (' ', 14, 2, false)); // " " before no, regular run
        assert_eq!(at(8), ('n', 16, 4, true));
        assert_eq!(at(9), ('o', 20, 4, true));
        assert_eq!(at(10), (' ', 24, 3, true)); // bold run's own space
        assert_eq!(at(11), ('r', 27, 4, true));
        assert_eq!(at(13), ('d', 35, 4, true));
        assert_eq!(at(14), ('.', 39, 2, false));
    }

    #[test]
    fn runs_normal_boundary_space_belongs_to_the_run_that_carries_it() {
        // The whitespace lives in the second (bold) run, so the collapsed
        // space is measured with the bold face.
        let lines = place_runs_normal(&[("is", false), (" no", true)], 800, |ch, bold| {
            Some(if ch == ' ' {
                if bold {
                    5
                } else {
                    2
                }
            } else if bold {
                4
            } else {
                2
            })
        })
        .unwrap();
        let at = |index: usize| {
            let glyph = lines.glyphs[index];
            (glyph.character, glyph.x, glyph.advance, glyph.bold)
        };
        assert_eq!(lines.glyphs.len(), 5);
        assert_eq!(at(1), ('s', 2, 2, false));
        assert_eq!(at(2), (' ', 4, 5, true));
        assert_eq!(at(3), ('n', 9, 4, true));
    }

    #[test]
    fn duplicated_boundary_whitespace_collapses_once_and_trims_edges() {
        let lines = place_runs_normal(
            &[("  a ", false), ("  ", true), (" b  ", true)],
            100,
            |ch, bold| {
                Some(if ch == ' ' {
                    if bold {
                        7
                    } else {
                        2
                    }
                } else {
                    3
                })
            },
        )
        .unwrap();
        assert_eq!(lines.glyphs.len(), 3);
        assert_eq!(
            (
                lines.glyphs[1].character,
                lines.glyphs[1].x,
                lines.glyphs[1].advance,
                lines.glyphs[1].bold
            ),
            (' ', 3, 2, false)
        );
        assert_eq!(lines.glyphs[2].x, 5);
    }

    #[test]
    fn runs_normal_wraps_across_runs_on_word_boundaries() {
        let lines = place_runs_normal(&[("aa ", false), ("bb cc", true)], 9, |ch, bold| {
            Some(if ch == ' ' {
                3
            } else if bold {
                4
            } else {
                2
            })
        })
        .unwrap();
        assert_eq!(lines.count, 3);
        let bold_lines: Vec<u32> = lines
            .glyphs
            .iter()
            .filter(|glyph| glyph.bold)
            .map(|glyph| glyph.line)
            .collect();
        assert_eq!(bold_lines, vec![1, 1, 2, 2]);
    }

    #[test]
    fn runs_text_places_strong_with_bold_face_and_flags() {
        use crate::{box_tree::BoxTree, parse_style_tree, Limits};
        let tree = BoxTree::from_style(&parse_style_tree(
            b"<html><body><p>one <strong>two</strong> three</p></body></html>",
            &[],
            &Limits::DEFAULT,
        ));
        let p = tree
            .boxes
            .iter()
            .position(|node| node.kind == BoxKind::Block && node.children.len() == 3)
            .unwrap();
        let laid_out = place_runs_text(
            &tree,
            p,
            800,
            |_, _, bold| Some(if bold { 8 } else { 4 }),
            |_| Some(16),
        )
        .unwrap();
        assert_eq!(laid_out.lines.count, 1);
        let words: Vec<(char, bool)> = laid_out
            .lines
            .glyphs
            .iter()
            .filter(|glyph| glyph.character != ' ')
            .map(|glyph| (glyph.character, glyph.bold))
            .collect();
        // one two three: only t,w,o are bold.
        let bold: Vec<char> = words
            .iter()
            .filter(|(_, bold)| *bold)
            .map(|(ch, _)| *ch)
            .collect();
        assert_eq!(bold, vec!['t', 'w', 'o']);
        // The bold run used the bold advance: 't' of two sits after
        // "one " at 4+4+4+4 (space measured at regular advance 4).
        let t = laid_out
            .lines
            .glyphs
            .iter()
            .find(|glyph| glyph.bold)
            .unwrap();
        assert_eq!(t.x, 16);
        assert_eq!(t.advance, 8);
    }

    #[test]
    fn runs_text_refuses_unproven_inline_contexts() {
        use crate::{box_tree::BoxTree, parse_style_tree, Limits};
        let tree = |html: &str| {
            BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
        };
        let p_of = |tree: &BoxTree| {
            tree.boxes
                .iter()
                .position(|node| {
                    node.kind == BoxKind::Block
                        && node
                            .children
                            .iter()
                            .any(|&child| tree.boxes[child].kind != BoxKind::Block)
                })
                .unwrap()
        };
        let advance = |_, _, _| Some(4);
        let line_height = |_| Some(16);
        for html in [
            "<html><body><p>one <em>two</em></p></body></html>", // italic UA style
            "<html><body><p>one <strong><em>two</em></strong></p></body></html>", // nested inline
            "<html><body><p>one <strong style='font-size:20px'>two</strong></p></body></html>", // mixed size
            "<html><body><p>one <span style='font-weight:600'>two</span></p></body></html>", // no 600 face
            "<html><body><p>one <span style='padding:2px'>two</span></p></body></html>", // inline padding
            "<html><body><p>one <span style='margin-left:2px'>two</span></p></body></html>", // inline margin
            "<html><body><p>one <span style='background-color:red'>two</span></p></body></html>", // inline bg
            "<html><body><p>one <span style='color:#123456'>two</span></p></body></html>", // run colour
            "<html><body><p style='direction:rtl'>one two</p></body></html>", // RTL block
            "<html><body><p>one <span style='font-style:oblique'>two</span></p></body></html>",
            "<html><body><p>one <span style='display:inline-block'>two</span></p></body></html>",
            "<html><body><p>one <span style='border:2px solid transparent'>two</span></p></body></html>",
            "<html><body><p>one <span style='unicode-bidi:bidi-override'>two</span></p></body></html>",
        ] {
            let tree = tree(html);
            let p = p_of(&tree);
            assert_eq!(
                place_runs_text(&tree, p, 800, advance, line_height).err(),
                Some(LineError::UnsupportedTree),
                "{html}"
            );
        }
    }

    #[test]
    fn direct_text_refuses_a_bold_run_without_a_bold_face_key() {
        use crate::{box_tree::BoxTree, parse_style_tree, Limits};
        let tree = BoxTree::from_style(&parse_style_tree(
            b"<html><body><p style='font-weight:bold'>ab</p></body></html>",
            &[],
            &Limits::DEFAULT,
        ));
        let p = tree
            .boxes
            .iter()
            .position(|node| {
                node.kind == BoxKind::Block
                    && !node.children.is_empty()
                    && node
                        .children
                        .iter()
                        .all(|&child| tree.boxes[child].kind == BoxKind::Text)
            })
            .unwrap();
        assert_eq!(
            place_direct_text(&tree, p, 800, |_, _| Some(4), |_| Some(16)).err(),
            Some(LineError::UnsupportedTree)
        );
        let laid_out = place_runs_text(&tree, p, 800, |_, _, _| Some(4), |_| Some(16)).unwrap();
        assert!(laid_out.lines.glyphs.iter().all(|glyph| glyph.bold));
    }
}

#[cfg(test)]
mod leaf_source_tests {
    use super::*;
    use crate::{parse_style_tree, Limits};
    use std::cell::Cell;

    fn measured(body: &str, width: u32, align: &str) -> DirectTextLines {
        let html = format!("<!doctype html><p style='text-align:{align}'>{body}</p>");
        let tree = BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT));
        let p = tree
            .boxes
            .iter()
            .position(|n| {
                n.kind == BoxKind::Block
                    && n.children
                        .iter()
                        .any(|&c| tree.boxes[c].kind == BoxKind::Text)
            })
            .unwrap();
        place_runs_text(&tree, p, width, |_, _, _| Some(5), |_| Some(20)).unwrap()
    }
    fn paint(text: &DirectTextLines, width: u32) -> DisplayList {
        paint_direct_glyphs(text, 0, 0, width, 16, 20, 15, Rgb(0, 0, 0), |_, _, _| {
            Some(GlyphBitmap {
                left: 0,
                top: -1,
                width: 1,
                height: 1,
                coverage: vec![255],
            })
        })
        .unwrap()
    }
    #[test]
    fn actual_leaf_sources_replace_first_leaf_stamping_without_pixel_change() {
        let text = measured("aa <strong>bb</strong> cc", 100, "left");
        assert_eq!(text.leaves, [4, 6, 7]);
        let actual = paint(&text, 100);
        let mut legacy = text;
        for glyph in &mut legacy.lines.glyphs {
            glyph.leaf = 0;
        }
        let old = paint(&legacy, 100);
        let sources: Vec<_> = actual
            .commands()
            .iter()
            .map(|c| match c {
                crate::display_list::Command::GlyphRun { source, .. } => source.node.unwrap(),
                _ => panic!("glyph only"),
            })
            .collect();
        assert_eq!(sources, [4, 4, 4, 6, 6, 7, 7, 7]);
        let normalized: Vec<_> = actual
            .commands()
            .iter()
            .cloned()
            .map(|mut c| {
                if let crate::display_list::Command::GlyphRun { source, .. } = &mut c {
                    source.node = Some(4);
                }
                c
            })
            .collect();
        assert_eq!(normalized, old.commands());
        assert_eq!(
            actual.rasterize(100, 100, Rgb(255, 255, 255)),
            old.rasterize(100, 100, Rgb(255, 255, 255))
        );
    }
    #[test]
    fn words_spaces_wrap_alignment_and_face_keep_exact_source() {
        for align in ["left", "right", "center"] {
            let text = measured(
                "a<strong>a</strong>a <span> </span><strong>bb</strong> cc",
                85,
                align,
            );
            let mapped: Vec<_> = text
                .lines
                .glyphs
                .iter()
                .map(|g| (g.character, g.bold, text.leaves[g.leaf]))
                .collect();
            assert_eq!(mapped.iter().map(|x| x.0).collect::<String>(), "aaa bb cc");
            assert_ne!(mapped[0].2, mapped[1].2);
            assert_ne!(mapped[1].2, mapped[2].2);
            assert_eq!(mapped[3].2, mapped[2].2);
            assert!(!mapped[3].1);
            assert!(mapped[4].1 && mapped[5].1);
            assert!(!mapped[6].1);
            let wrapped = measured("aa <strong>bb</strong> cc", 20, align);
            assert_eq!(wrapped.lines.count, 3);
            assert!(wrapped.lines.glyphs.iter().all(|g| g.character != ' '));
            assert_eq!(
                wrapped
                    .lines
                    .glyphs
                    .iter()
                    .map(|g| wrapped.leaves[g.leaf])
                    .collect::<Vec<_>>(),
                [4, 4, 6, 6, 7, 7]
            );
        }
    }
    #[test]
    fn sourced_tokenizer_matches_compatibility_geometry_and_provider_calls() {
        let source_runs = [
            ("  a", false, 0),
            ("", true, 1),
            ("a\u{a0}", true, 2),
            ("b \t", false, 3),
            (" c ", true, 4),
        ];
        let plain: Vec<_> = source_runs.iter().map(|&(s, b, _)| (s, b)).collect();
        let mut actual_calls = Vec::new();
        let mut old_calls = Vec::new();
        let mut actual = place_sourced_runs_normal(source_runs.iter().copied(), 60, |c, b| {
            actual_calls.push((c, b));
            Some(5)
        })
        .unwrap();
        let old = place_runs_normal(&plain, 60, |c, b| {
            old_calls.push((c, b));
            Some(5)
        })
        .unwrap();
        assert_eq!(actual_calls, old_calls);
        assert_eq!(
            actual.glyphs.iter().map(|g| g.leaf).collect::<Vec<_>>(),
            [0, 2, 2, 3, 3, 4]
        );
        for glyph in &mut actual.glyphs {
            glyph.leaf = 0;
        }
        assert_eq!(actual, old);
    }
    #[test]
    fn invalid_or_missing_mapping_refuses_before_raster_and_never_guesses() {
        for mode in 0..3 {
            let mut text = measured("a<strong>b</strong>", 100, "left");
            match mode {
                0 => text.leaves.clear(),
                1 => text.lines.glyphs[1].leaf = usize::MAX,
                _ => text.source = usize::MAX,
            }
            let calls = Cell::new(0);
            let result =
                paint_direct_glyphs(&text, 0, 0, 100, 16, 20, 15, Rgb(0, 0, 0), |_, _, _| {
                    calls.set(calls.get() + 1);
                    None
                });
            assert!(matches!(result, Err(GlyphPaintError::InvalidMetrics)));
            assert_eq!(calls.get(), 0);
        }
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod nested_inline_tests {
    use super::*;
    use crate::{parse_style_tree, Limits};
    fn tree(body: &str) -> BoxTree {
        BoxTree::from_style(&parse_style_tree(format!("<!doctype html><style>html,body,p{{margin:0}}p{{width:200px;line-height:20px}}</style><p>{body}</p>").as_bytes(),&[],&Limits::DEFAULT))
    }
    fn owner(t: &BoxTree) -> usize {
        t.boxes
            .iter()
            .position(|n| {
                n.kind == BoxKind::Block && n.style.width == crate::computed_style::Length::Px(200)
            })
            .unwrap()
    }
    #[test]
    fn nested_sources_effective_weights_and_first_space_survive() {
        for (html, bold_space) in [
            ("aa <span><strong> </strong>bb</span> cc", false),
            ("aa<strong> <span> </span></strong>bb cc", true),
        ] {
            let t = tree(html);
            let p = owner(&t);
            let l = place_runs_text(&t, p, 200, |_, _, _| Some(5), |_| Some(20)).unwrap();
            let g = l.lines.glyphs.iter().find(|g| g.character == ' ').unwrap();
            assert_eq!(g.bold, bold_space);
            let source = l.leaves[g.leaf];
            let n = t.boxes.iter().find(|n| n.source == Some(source)).unwrap();
            assert!(n.text.as_ref().unwrap().contains(' '));
            assert_eq!(n.style.font_weight == 700, bold_space);
        }
        let t = tree(
            "a<strong><span>b<span style='font-weight:400'><span>c</span></span>d</span></strong>e",
        );
        let l = place_runs_text(&t, owner(&t), 200, |_, _, _| Some(5), |_| Some(20)).unwrap();
        assert_eq!(
            l.lines.glyphs.iter().map(|g| g.bold).collect::<Vec<_>>(),
            [false, true, false, true, false]
        );
        let sources: Vec<_> = l.lines.glyphs.iter().map(|g| l.leaves[g.leaf]).collect();
        assert_eq!(
            sources
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            5
        );
    }
    #[test]
    fn invalid_full_subtree_never_asks_metrics() {
        for html in [
            "a<span><span style='padding:4px'></span></span>b",
            "a<span style='margin:1px'></span>b",
            "a<span style='background:red'></span>b",
            "a<span style='border:1px solid red'></span>b",
            "a<span style='position:absolute'></span>b",
            "a<span style='font-weight:900'></span>b",
            "a<span style='text-transform:uppercase'>b</span>",
            "a<span style='letter-spacing:1px'>b</span>",
            "a<span style='word-spacing:1px'>b</span>",
            "a<span style='visibility:hidden'>b</span>",
            "a<span><img></span>b",
            "a<span onclick='void(0)'><span>b</span></span>",
            "a<span><br></span>b",
        ] {
            let t = tree(html);
            let calls = std::cell::Cell::new(0);
            let result = place_runs_text(
                &t,
                owner(&t),
                200,
                |_, _, _| {
                    calls.set(calls.get() + 1);
                    Some(5)
                },
                |_| {
                    calls.set(calls.get() + 1);
                    Some(20)
                },
            );
            assert!(result.is_err(), "{html}");
            assert_eq!(calls.get(), 0, "{html}");
        }
        for mode in 0..3 {
            let mut t = tree("a<span><span>b</span></span>c");
            let p = owner(&t);
            let i = t.boxes[p].children[1];
            match mode {
                0 => t.boxes[i].children.push(i),
                1 => t.boxes[i].parent = Some(i),
                _ => t.boxes[p].children.push(i),
            };
            let calls = std::cell::Cell::new(0);
            assert!(place_runs_text(
                &t,
                p,
                200,
                |_, _, _| {
                    calls.set(calls.get() + 1);
                    Some(5)
                },
                |_| Some(20)
            )
            .is_err());
            assert_eq!(calls.get(), 0);
        }
    }
    #[test]
    fn caps_are_independent_and_depth_owner_zero() {
        for depth in [96, 97] {
            let t = tree(&format!(
                "{}a{}",
                "<span>".repeat(depth),
                "</span>".repeat(depth)
            ));
            assert_eq!(collect_inline_runs(&t, owner(&t)).is_ok(), depth == 96);
        }
        let t = tree("a<span>b</span>c");
        let p = owner(&t);
        // Four visited descendants/edges; three actual leaves.
        assert!(collect_inline_runs_bounded(&t, p, 96, 4, 4, 3).is_ok());
        assert!(collect_inline_runs_bounded(&t, p, 96, 3, 4, 3).is_err());
        assert!(collect_inline_runs_bounded(&t, p, 96, 4, 3, 3).is_err());
        assert!(collect_inline_runs_bounded(&t, p, 96, 4, 4, 2).is_err());
        for count in [4096, 4097] {
            let mut t = tree("a");
            let p = owner(&t);
            let template = t.boxes[t.boxes[p].children[0]].clone();
            t.boxes[p].children.clear();
            for n in 0..count {
                let mut node = template.clone();
                node.source = Some(100 + n);
                let id = t.boxes.len();
                t.boxes.push(node);
                t.boxes[p].children.push(id);
            }
            assert_eq!(collect_inline_runs(&t, p).is_ok(), count == 4096);
            // Isolate each production limit so an earlier tied cap cannot mask it.
            assert_eq!(
                collect_inline_runs_bounded(&t, p, 96, 4096, usize::MAX, usize::MAX).is_ok(),
                count == 4096
            );
            assert_eq!(
                collect_inline_runs_bounded(&t, p, 96, usize::MAX, 4096, usize::MAX).is_ok(),
                count == 4096
            );
            assert_eq!(
                collect_inline_runs_bounded(&t, p, 96, usize::MAX, usize::MAX, 4096).is_ok(),
                count == 4096
            );
        }
    }
    #[test]
    fn empty_bare_wrappers_do_not_reset_pending_whitespace_or_nbsp() {
        let t=tree("aa<strong> <span></span></strong><span><span></span> </span>bb<span>&nbsp;<span>cc</span></span>");
        let l = place_runs_text(&t, owner(&t), 200, |_, _, _| Some(5), |_| Some(20)).unwrap();
        assert_eq!(
            l.lines
                .glyphs
                .iter()
                .map(|g| g.character)
                .collect::<String>(),
            "aa bb\u{a0}cc"
        );
        assert!(l.lines.glyphs[2].bold);
    }
}
