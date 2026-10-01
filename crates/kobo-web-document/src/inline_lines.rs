//! Restricted normal-white-space line placement for premeasured ASCII text.
//!
//! The font provider owns glyph advances and raster masks. This module does
//! not invent font metrics, ligatures, kerning, bidi, CSS line-height, or
//! Unicode break rules. Callers must reject unsupported inline contexts.

use crate::box_tree::{BoxKind, BoxTree};
use crate::computed_style::Direction;
use crate::display_list::{DisplayList, Error as DisplayError, Rect, Rgb, Source, MAX_GLYPH_BYTES};

const MAX_LINES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionedGlyph {
    pub character: char,
    pub x: u32,
    pub line: u32,
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
    if max_width == 0 {
        return Err(LineError::InvalidMetrics);
    }
    let mut words: Vec<Vec<(char, u32)>> = Vec::new();
    let mut word = Vec::new();
    let mut letters = 0_usize;
    // The current placer only has whitespace wrap opportunities. A word
    // containing ASCII hyphen or break punctuation needs CSS/Unicode line
    // breaking semantics when it wraps. An entirely fitting single line has
    // no break decision and can retain its punctuation. A narrow exception
    // allows alphanumeric words with one terminal sentence mark: those
    // marks stay with the word and the next whitespace is the break.
    let needs_break_rules = text.chars().any(needs_extra_break_rules);
    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            if !ch.is_ascii_graphic() && ch != '\u{a0}' {
                return Err(LineError::UnsupportedText);
            }
            let width = advance(ch)
                .filter(|&n| n > 0)
                .ok_or(LineError::InvalidMetrics)?;
            if letters >= MAX_GLYPH_BYTES {
                return Err(LineError::TooManyGlyphs);
            }
            word.push((ch, width));
            letters += 1;
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    let space = if words.len() > 1 {
        advance(' ')
            .filter(|&n| n > 0)
            .ok_or(LineError::InvalidMetrics)?
    } else {
        0
    };
    if needs_break_rules
        && !words.iter().all(|word| sentence_word(word))
        && !fits_single_line(&words, space, max_width)
    {
        return Err(LineError::UnsupportedText);
    }
    let mut glyphs = Vec::new();
    let mut x = 0_u32;
    let mut line = 0_u32;
    for word in words {
        let mut width = 0_u32;
        for &(_, value) in &word {
            width = width.checked_add(value).ok_or(LineError::UnbreakableWord)?;
        }
        if width > max_width {
            return Err(LineError::UnbreakableWord);
        }
        if x > 0 {
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
                });
                x += space;
            }
        }
        for (character, value) in word {
            glyphs.push(PositionedGlyph { character, x, line });
            x += value; // checked by the word and line fit above
        }
    }
    Ok(Lines {
        count: if glyphs.is_empty() { 0 } else { line + 1 },
        glyphs,
    })
}

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

// Deliberately excludes other internal punctuation, parentheses, surrounding
// quotes, runs of punctuation, hyphens and slashes. One ASCII apostrophe
// between alphabetic stems stays inside a contraction or possessive.
fn sentence_word(word: &[(char, u32)]) -> bool {
    word.split(|&(ch, _)| ch == '\u{a0}').all(sentence_piece)
}

fn sentence_piece(word: &[(char, u32)]) -> bool {
    let Some(&(last, _)) = word.last() else {
        return false;
    };
    let stem = if matches!(last, '.' | ',' | '!' | '?' | ':' | ';') {
        &word[..word.len() - 1]
    } else {
        word
    };
    if !stem.is_empty() && stem.iter().all(|&(ch, _)| ch.is_ascii_alphanumeric()) {
        return true;
    }
    let mut pieces = stem.split(|&(ch, _)| ch == '\'');
    let alphabetic = |part: &[(char, u32)]| {
        !part.is_empty() && part.iter().all(|&(ch, _)| ch.is_ascii_alphabetic())
    };
    pieces.next().is_some_and(alphabetic)
        && pieces.next().is_some_and(alphabetic)
        && pieces.next().is_none()
}

fn fits_single_line(words: &[Vec<(char, u32)>], space: u32, max_width: u32) -> bool {
    let mut full_width = 0_u32;
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            let Some(next) = full_width.checked_add(space) else {
                return false;
            };
            full_width = next;
        }
        for &(_, width) in word {
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
    pub source: usize,
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
    if tree.truncated || tree.unsupported {
        return Err(LineError::UnsupportedTree);
    }
    let node = tree.boxes.get(parent).ok_or(LineError::UnsupportedTree)?;
    // This placer only emits left-origin LTR glyphs and no list marker.
    // Direction also controls the initial inline alignment, even for ASCII.
    if node.kind != BoxKind::Block
        || node.style.direction != Direction::Ltr
        || node.children.len() != 1
        || node.text.is_some()
    {
        return Err(LineError::UnsupportedTree);
    }
    let child = tree
        .boxes
        .get(node.children[0])
        .ok_or(LineError::UnsupportedTree)?;
    if child.parent != Some(parent)
        || child.kind != BoxKind::Text
        || child.style.direction != Direction::Ltr
    {
        return Err(LineError::UnsupportedTree);
    }
    let source = child.source.ok_or(LineError::UnsupportedTree)?;
    let text = child.text.as_deref().ok_or(LineError::UnsupportedTree)?;
    let size = child.style.font_size;
    if size == 0 || max_width == 0 {
        return Err(LineError::InvalidMetrics);
    }
    let lines = place_ascii_normal(text, max_width, |ch| advance(ch, size))?;
    let natural = line_height(size).ok_or(LineError::InvalidMetrics)?;
    let height = child
        .style
        .used_line_height(natural)
        .filter(|&value| value > 0)
        .ok_or(LineError::InvalidMetrics)?;
    let content_height = lines
        .count
        .checked_mul(height)
        .ok_or(LineError::InvalidMetrics)?;
    Ok(DirectTextLines {
        source,
        lines,
        content_height,
    })
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
    mut raster: impl FnMut(char, u32) -> Option<GlyphBitmap>,
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
    let mut list = DisplayList::default();
    for glyph in &text.lines.glyphs {
        if glyph.line >= text.lines.count || glyph.x >= content_width {
            return Err(GlyphPaintError::OutsideLine);
        }
        let bitmap = raster(glyph.character, size).ok_or(GlyphPaintError::InvalidBitmap)?;
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
        list.glyph_run(
            Rect {
                x: i32::try_from(x).map_err(|_| GlyphPaintError::InvalidMetrics)?,
                y: i32::try_from(y).map_err(|_| GlyphPaintError::InvalidMetrics)?,
                width: bitmap.width,
                height: bitmap.height,
            },
            bitmap.coverage,
            color,
            Source {
                node: Some(text.source),
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
                line: 0
            }
        );
        assert_eq!(
            lines.glyphs[5],
            PositionedGlyph {
                character: 'w',
                x: 0,
                line: 1
            }
        );
        assert_eq!(
            lines.glyphs[10],
            PositionedGlyph {
                character: 'a',
                x: 0,
                line: 2
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
            content_height: 12,
            lines: Lines {
                count: 1,
                glyphs: vec![PositionedGlyph {
                    character: 'A',
                    x: 2,
                    line: 0,
                }],
            },
        };
        let raster = |_, _| {
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
        let overflow = paint_direct_glyphs(&text, 3, 4, 20, 16, 12, 6, Rgb(0, 0, 0), |_, _| {
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
            paint_direct_glyphs(&text, 3, 4, 20, 16, 12, 6, Rgb(0, 0, 0), |_, _| Some(
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
                Err(LineError::UnsupportedText)
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
            "a-b cd.", "a/b cd.", "a.b cd.", "a,b cd.", "(ab) cd.", "ab!! cd.", ". ab.", "ab. 'cd'",
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
