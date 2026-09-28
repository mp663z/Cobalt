//! Restricted normal-white-space line placement for premeasured ASCII text.
//!
//! The font provider owns glyph advances and raster masks. This module does
//! not invent font metrics, ligatures, kerning, bidi, CSS line-height, or
//! Unicode break rules. Callers must reject unsupported inline contexts.

use crate::display_list::MAX_GLYPH_BYTES;

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
}

/// Place a single block's normal-white-space ASCII text at given integer
/// advances. ASCII whitespace collapses to one breakable space; leading and
/// line-end spaces are omitted. A word that cannot fit fails rather than
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
    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            if !ch.is_ascii_graphic() {
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
    fn refuses_unsupported_text_and_unproven_measurements() {
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
