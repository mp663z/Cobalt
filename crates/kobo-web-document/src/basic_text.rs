//! One bounded, source-preserving scalar line breaker for Page view.
//!
//! This is not a shaping engine. Characters and their run indices survive
//! placement, including zero-advance combining marks supplied by a font.

use crate::inline_lines::{LineError, Lines, PositionedGlyph};

const MAX_GLYPHS: usize = 262_144;
const MAX_LINES: u32 = 65_536;

/// Place Unicode runs using spaces, hyphens and slashes as opportunities.
/// Normal whitespace collapses across run boundaries; NBSP never collapses.
/// With `pre`, newlines and spaces are retained, tabs advance to 8-space stops,
/// and soft wrapping is disabled. `leaf` identifies the input run, not a node.
///
/// # Errors
/// Rejects zero width, missing advances, overflow and bounded-output limits.
#[allow(clippy::too_many_lines)] // Whitespace normalization and placement share a bounded stream.
pub fn place(
    runs: &[(&str, bool)],
    width: u32,
    pre: bool,
    mut advance: impl FnMut(char, usize) -> Option<u32>,
) -> Result<Lines, LineError> {
    if width == 0 {
        return Err(LineError::InvalidMetrics);
    }
    let mut measured = Vec::new();
    let mut space = None;
    let mut after_cr = false;
    for (leaf, &(text, bold)) in runs.iter().enumerate() {
        for ch in text.chars() {
            if measured.len() >= MAX_GLYPHS {
                return Err(LineError::TooManyGlyphs);
            }
            if pre {
                if ch == '\n' && after_cr {
                    after_cr = false;
                    continue;
                }
                after_cr = ch == '\r';
                let character = if after_cr { '\n' } else { ch };
                let w = if character == '\n' {
                    0
                } else {
                    advance(if ch == '\t' { ' ' } else { ch }, leaf)
                        .ok_or(LineError::InvalidMetrics)?
                };
                measured.push(PositionedGlyph {
                    character,
                    x: 0,
                    line: 0,
                    advance: w,
                    bold,
                    leaf,
                });
            } else if ch.is_ascii_whitespace() {
                if !measured.is_empty() && space.is_none() {
                    space = Some((bold, leaf));
                }
            } else {
                if let Some((bold, leaf)) = space.take() {
                    let w = advance(' ', leaf).ok_or(LineError::InvalidMetrics)?;
                    measured.push(PositionedGlyph {
                        character: ' ',
                        x: 0,
                        line: 0,
                        advance: w,
                        bold,
                        leaf,
                    });
                }
                let w = advance(ch, leaf).ok_or(LineError::InvalidMetrics)?;
                measured.push(PositionedGlyph {
                    character: ch,
                    x: 0,
                    line: 0,
                    advance: w,
                    bold,
                    leaf,
                });
            }
        }
    }
    let mut glyphs = Vec::with_capacity(measured.len());
    let mut x = 0_u32;
    let mut line = 0_u32;
    let mut pos = 0;
    while pos < measured.len() {
        if pre {
            let mut g = measured[pos];
            if g.character == '\n' {
                new_line(&mut x, &mut line)?;
            } else {
                if g.character == '\t' {
                    let tab = g
                        .advance
                        .max(1)
                        .checked_mul(8)
                        .ok_or(LineError::InvalidMetrics)?;
                    g.advance = tab - x % tab;
                    g.character = ' ';
                }
                g.x = x;
                g.line = line;
                x = x.checked_add(g.advance).ok_or(LineError::InvalidMetrics)?;
                glyphs.push(g);
            }
            pos += 1;
            continue;
        }
        let gap = if measured[pos].character == ' ' {
            let gap = Some(measured[pos]);
            pos += 1;
            gap
        } else {
            None
        };
        if pos == measured.len() {
            break;
        }
        let start = pos;
        while pos < measured.len() && measured[pos].character != ' ' {
            let ch = measured[pos].character;
            pos += 1;
            if matches!(ch, '-' | '/' | '\u{2010}' | '\u{2013}') {
                break;
            }
        }
        let segment = &measured[start..pos];
        let segment_width = segment
            .iter()
            .try_fold(0_u32, |sum, g| sum.checked_add(g.advance))
            .ok_or(LineError::InvalidMetrics)?;
        let gap_width = gap.map_or(0, |g| g.advance);
        if x > 0 && x.saturating_add(gap_width).saturating_add(segment_width) > width {
            new_line(&mut x, &mut line)?;
        }
        if x > 0 {
            if let Some(mut g) = gap {
                g.x = x;
                g.line = line;
                x = x.checked_add(g.advance).ok_or(LineError::InvalidMetrics)?;
                glyphs.push(g);
            }
        }
        // Hard splitting is reserved for a segment wider than an empty line.
        for &g in segment {
            if segment_width > width
                && g.advance > 0
                && x > 0
                && x.saturating_add(g.advance) > width
            {
                new_line(&mut x, &mut line)?;
            }
            let mut g = g;
            g.x = x;
            g.line = line;
            x = x.checked_add(g.advance).ok_or(LineError::InvalidMetrics)?;
            glyphs.push(g);
        }
    }
    Ok(Lines {
        count: if measured.is_empty() { 0 } else { line + 1 },
        glyphs,
    })
}

fn new_line(x: &mut u32, line: &mut u32) -> Result<(), LineError> {
    *line += 1;
    *x = 0;
    if *line >= MAX_LINES {
        return Err(LineError::TooManyLines);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::place;

    fn lines(text: &str, width: u32) -> Vec<String> {
        let placed = place(&[(text, false)], width, false, |_, _| Some(1)).unwrap();
        let mut lines = vec![String::new(); placed.count as usize];
        for g in placed.glyphs {
            lines[g.line as usize].push(g.character);
        }
        lines
    }

    #[test]
    fn words_move_whole_and_spaces_are_not_doubled() {
        assert_eq!(lines("four five six", 8), ["four", "five six"]);
        assert_eq!(lines("  four   five  ", 20), ["four five"]);
        assert_eq!(lines("alpha-beta/gamma", 7), ["alpha-", "beta/", "gamma"]);
    }

    #[test]
    fn only_overwide_words_break_inside() {
        assert_eq!(lines("a abcdef z", 4), ["a", "abcd", "ef z"]);
        assert_eq!(lines("a abcd z", 4), ["a", "abcd", "z"]);
    }

    #[test]
    fn unicode_and_source_run_identity_survive() {
        let runs = [("Рус", false), ("ский हिन्दी 日本語 é ©", true)];
        let result = place(&runs, 300, false, |_, _| Some(1)).unwrap();
        let text: String = result.glyphs.iter().map(|g| g.character).collect();
        assert_eq!(text, "Русский हिन्दी 日本語 é ©");
        assert_eq!(result.glyphs[2].leaf, 0);
        assert_eq!(result.glyphs[3].leaf, 1);
        assert!(result.glyphs[3].bold);
    }

    #[test]
    fn whitespace_collapses_across_style_boundaries() {
        let result = place(&[("a  ", false), ("  b", true)], 20, false, |_, _| Some(1)).unwrap();
        assert_eq!(result.glyphs.len(), 3);
        assert_eq!(result.glyphs[2].x, 2);
        assert_eq!(result.glyphs[2].leaf, 1);
        assert_eq!(lines("a\u{a0}b c", 3), ["a\u{a0}b", "c"]);
    }

    #[test]
    fn pre_retains_blank_lines_indentation_and_tabs() {
        let result = place(&[("  a\r\n\nb\tc", false)], 2, true, |_, _| Some(1)).unwrap();
        assert_eq!(result.count, 3);
        let c = result.glyphs.last().unwrap();
        assert_eq!((c.character, c.x, c.line), ('c', 8, 2));
        assert_eq!(result.glyphs[2].x, 2);
    }

    #[test]
    fn an_overwide_cluster_keeps_its_zero_advance_continuation() {
        let result = place(&[("a\u{301}b", false)], 2, false, |ch, _| {
            Some(match ch {
                'a' => 3,
                '\u{301}' => 0,
                _ => 1,
            })
        })
        .unwrap();
        assert_eq!(result.glyphs[0].line, result.glyphs[1].line);
        assert_eq!(result.glyphs[2].line, result.glyphs[1].line + 1);
    }

    #[test]
    fn zero_advance_marks_do_not_disappear() {
        let result = place(&[("e\u{301}", false)], 2, false, |ch, _| {
            Some(u32::from(ch != '\u{301}'))
        })
        .unwrap();
        assert_eq!(result.glyphs.len(), 2);
        assert_eq!(result.glyphs[1].advance, 0);
    }
}
