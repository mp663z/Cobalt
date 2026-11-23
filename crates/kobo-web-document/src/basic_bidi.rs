//! Per-line Unicode bidi reordering of already shaped logical clusters.
use crate::computed_style::{Direction, UnicodeBidi};
use crate::inline_lines::Lines;
use unicode_bidi::{BidiInfo, Level};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Scope {
    pub id: usize,
    pub mode: UnicodeBidi,
    pub direction: Direction,
}

#[cfg(test)]
pub(crate) fn reorder(lines: &mut Lines, runs: &[(&str, bool)], direction: Direction) {
    let modes = vec![(UnicodeBidi::Normal, direction); runs.len()];
    let _ = reorder_styled(lines, runs, direction, &modes);
}

#[cfg(test)]
fn reorder_styled(
    lines: &mut Lines,
    runs: &[(&str, bool)],
    direction: Direction,
    modes: &[(UnicodeBidi, Direction)],
) -> Vec<bool> {
    let scopes: Vec<Vec<Scope>> = modes
        .iter()
        .enumerate()
        .map(|(id, (mode, dir))| {
            if *mode == UnicodeBidi::Normal {
                Vec::new()
            } else {
                vec![Scope {
                    id,
                    mode: *mode,
                    direction: *dir,
                }]
            }
        })
        .collect();
    reorder_scoped(
        lines,
        runs,
        direction,
        &scopes.iter().map(Vec::as_slice).collect::<Vec<_>>(),
    )
}

fn controls(scope: Scope) -> (&'static str, &'static str) {
    let rtl = scope.direction == Direction::Rtl;
    match scope.mode {
        UnicodeBidi::Normal | UnicodeBidi::Plaintext => ("", ""),
        UnicodeBidi::Embed => (if rtl { "\u{202b}" } else { "\u{202a}" }, "\u{202c}"),
        UnicodeBidi::Override => (if rtl { "\u{202e}" } else { "\u{202d}" }, "\u{202c}"),
        UnicodeBidi::Isolate => (if rtl { "\u{2067}" } else { "\u{2066}" }, "\u{2069}"),
        UnicodeBidi::IsolateOverride => (
            if rtl {
                "\u{2067}\u{202e}"
            } else {
                "\u{2066}\u{202d}"
            },
            "\u{202c}\u{2069}",
        ),
    }
}

pub(crate) fn reorder_scoped(
    lines: &mut Lines,
    runs: &[(&str, bool)],
    direction: Direction,
    scopes: &[&[Scope]],
) -> Vec<bool> {
    let mut mirrored = vec![false; lines.glyphs.len()];
    let mut text = String::new();
    let mut bases = Vec::with_capacity(runs.len());
    let mut active: &[Scope] = &[];
    for (i, run) in runs.iter().enumerate() {
        let next = scopes.get(i).copied().unwrap_or(&[]);
        let common = active.iter().zip(next).take_while(|(a, b)| a == b).count();
        for scope in active[common..].iter().rev() {
            text.push_str(controls(*scope).1);
        }
        for scope in &next[common..] {
            text.push_str(controls(*scope).0);
        }
        bases.push(text.len());
        text.push_str(run.0);
        active = next;
    }
    for scope in active.iter().rev() {
        text.push_str(controls(*scope).1);
    }
    let base = if direction == Direction::Rtl {
        Level::rtl()
    } else {
        Level::ltr()
    };
    let auto = scopes
        .iter()
        .flat_map(|s| s.iter())
        .any(|s| s.mode == UnicodeBidi::Plaintext);
    let info = BidiInfo::new(&text, if auto { None } else { Some(base) });
    if !info.levels.iter().any(Level::is_rtl) {
        return mirrored;
    }
    let mut cursors = vec![0; runs.len()];
    let bytes: Vec<_> = lines
        .glyphs
        .iter()
        .map(|g| {
            let cursor = &mut cursors[g.leaf];
            let source = runs[g.leaf].0;
            let byte = source[*cursor..]
                .char_indices()
                .find(|(_, c)| *c == g.character)
                .map_or(*cursor, |(i, _)| *cursor + i);
            *cursor = (byte + g.character.len_utf8()).min(source.len());
            while !source.is_char_boundary(*cursor) {
                *cursor -= 1;
            }
            bases[g.leaf] + byte
        })
        .collect();
    let mut start = 0;
    while start < lines.glyphs.len() {
        let line = lines.glyphs[start].line;
        let mut end = start + 1;
        while end < lines.glyphs.len() && lines.glyphs[end].line == line {
            end += 1;
        }
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for i in start..end {
            if lines.glyphs[i].advance == 0 && !groups.is_empty() {
                groups.last_mut().unwrap().1 = i + 1;
            } else {
                groups.push((i, i + 1));
            }
        }
        let range_start = bytes[start].min(text.len());
        let range_end =
            (bytes[end - 1] + lines.glyphs[end - 1].character.len_utf8()).min(text.len());
        let para = info
            .paragraphs
            .iter()
            .find(|p| p.range.start <= range_start && p.range.end >= range_end);
        let levels = para.map_or_else(
            || info.levels.clone(),
            |p| info.reordered_levels(p, range_start..range_end),
        );
        let group_levels: Vec<_> = groups
            .iter()
            .map(|(i, _)| levels.get(bytes[*i]).copied().unwrap_or(base))
            .collect();
        for ((a, b), level) in groups.iter().zip(&group_levels) {
            if level.is_rtl() {
                for flag in &mut mirrored[*a..*b] {
                    *flag = true;
                }
            }
        }
        let order = BidiInfo::reorder_visual(&group_levels);
        let mut x = 0_u32;
        for visual in order {
            let (a, b) = groups[visual];
            let old = lines.glyphs[a].x;
            let advance = lines.glyphs[a].advance;
            for g in &mut lines.glyphs[a..b] {
                g.x = x.saturating_add(g.x.saturating_sub(old));
            }
            x = x.saturating_add(advance);
        }
        start = end;
    }
    mirrored
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rtl_reorders_hebrew_but_preserves_latin_and_digits() {
        let runs = [("אבג abc 123", false)];
        let mut lines = crate::basic_text::place(&runs, 300, false, |_, _| Some(1)).unwrap();
        reorder(&mut lines, &runs, Direction::Rtl);
        let mut glyphs = lines.glyphs;
        glyphs.sort_by_key(|g| g.x);
        assert_eq!(
            glyphs.iter().map(|g| g.character).collect::<String>(),
            "abc 123 גבא"
        );
    }
    #[test]
    fn source_leaf_identity_survives_visual_reordering() {
        let runs = [("אב", false), ("ג abc", true)];
        let mut lines = crate::basic_text::place(&runs, 300, false, |_, _| Some(1)).unwrap();
        reorder(&mut lines, &runs, Direction::Rtl);
        let aleph = lines.glyphs.iter().find(|g| g.character == 'א').unwrap();
        let gimel = lines.glyphs.iter().find(|g| g.character == 'ג').unwrap();
        assert_eq!((aleph.leaf, gimel.leaf), (0, 1));
        assert!(aleph.x > gimel.x);
    }

    #[test]
    fn css_override_reverses_latin_without_reversing_source() {
        let runs = [("abc 123", false)];
        let mut lines = crate::basic_text::place(&runs, 100, false, |_, _| Some(1)).unwrap();
        reorder_styled(
            &mut lines,
            &runs,
            Direction::Ltr,
            &[(UnicodeBidi::Override, Direction::Rtl)],
        );
        let mut glyphs = lines.glyphs;
        glyphs.sort_by_key(|g| g.x);
        assert_eq!(
            glyphs.iter().map(|g| g.character).collect::<String>(),
            "321 cba"
        );
    }
    #[test]
    fn isolate_keeps_digits_with_its_inline_direction() {
        let runs = [("start ", false), ("אבג 123", false), (" end", false)];
        let mut lines = crate::basic_text::place(&runs, 100, false, |_, _| Some(1)).unwrap();
        reorder_styled(
            &mut lines,
            &runs,
            Direction::Ltr,
            &[
                (UnicodeBidi::Normal, Direction::Ltr),
                (UnicodeBidi::Isolate, Direction::Rtl),
                (UnicodeBidi::Normal, Direction::Ltr),
            ],
        );
        let mut glyphs = lines.glyphs;
        glyphs.sort_by_key(|g| g.x);
        assert_eq!(
            glyphs.iter().map(|g| g.character).collect::<String>(),
            "start 123 גבא end"
        );
    }

    #[test]
    fn paired_neutrals_resolve_rtl_for_mirrored_paint() {
        let runs = [("אבג (דהו) [123]", false)];
        let mut lines = crate::basic_text::place(&runs, 100, false, |_, _| Some(1)).unwrap();
        let flags = reorder_styled(
            &mut lines,
            &runs,
            Direction::Rtl,
            &[(UnicodeBidi::Normal, Direction::Rtl)],
        );
        for (g, flag) in lines.glyphs.iter().zip(flags) {
            if matches!(g.character, '(' | ')' | '[' | ']') {
                assert!(flag);
            }
        }
    }

    #[test]
    fn ancestor_override_stays_open_across_style_leaves() {
        let runs = [("abc ", false), ("def", true)];
        let scope = Scope {
            id: 42,
            mode: UnicodeBidi::Override,
            direction: Direction::Rtl,
        };
        let mut lines = crate::basic_text::place(&runs, 100, false, |_, _| Some(1)).unwrap();
        reorder_scoped(&mut lines, &runs, Direction::Ltr, &[&[scope], &[scope]]);
        let mut glyphs = lines.glyphs;
        glyphs.sort_by_key(|g| g.x);
        assert_eq!(
            glyphs.iter().map(|g| g.character).collect::<String>(),
            "fed cba"
        );
    }
    #[test]
    fn nested_isolate_closes_before_surrounding_text() {
        let runs = [
            ("A ", false),
            ("אב ", false),
            ("abc", false),
            (" גד", false),
            (" Z", false),
        ];
        let outer = Scope {
            id: 1,
            mode: UnicodeBidi::Isolate,
            direction: Direction::Rtl,
        };
        let inner = Scope {
            id: 2,
            mode: UnicodeBidi::Override,
            direction: Direction::Rtl,
        };
        let mut lines = crate::basic_text::place(&runs, 100, false, |_, _| Some(1)).unwrap();
        reorder_scoped(
            &mut lines,
            &runs,
            Direction::Ltr,
            &[&[], &[outer], &[outer, inner], &[outer], &[]],
        );
        let mut glyphs = lines.glyphs;
        glyphs.sort_by_key(|g| g.x);
        assert_eq!(
            glyphs.iter().map(|g| g.character).collect::<String>(),
            "A דג cba בא Z"
        );
    }

    #[test]
    fn wrapped_lines_are_reordered_independently() {
        let runs = [("אבג דהו זחט", false)];
        let mut lines = crate::basic_text::place(&runs, 4, false, |_, _| Some(1)).unwrap();
        reorder(&mut lines, &runs, Direction::Rtl);
        for n in 0..3 {
            let mut glyphs: Vec<_> = lines.glyphs.iter().filter(|g| g.line == n).collect();
            glyphs.sort_by_key(|g| g.x);
            assert_eq!(
                glyphs.iter().map(|g| g.character).collect::<String>(),
                ["גבא", "והד", "טחז"][n as usize]
            );
        }
    }
}
