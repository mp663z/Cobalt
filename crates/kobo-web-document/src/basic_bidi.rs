//! Per-line Unicode bidi reordering of already shaped logical clusters.
use crate::computed_style::Direction;
use crate::inline_lines::Lines;
use unicode_bidi::{BidiInfo, Level};

pub(crate) fn reorder(lines: &mut Lines, runs: &[(&str, bool)], direction: Direction) {
    let text: String = runs.iter().map(|r| r.0).collect();
    let base = if direction == Direction::Rtl {
        Level::rtl()
    } else {
        Level::ltr()
    };
    let info = BidiInfo::new(&text, Some(base));
    if !info.levels.iter().any(Level::is_rtl) {
        return;
    }
    let mut bases = Vec::with_capacity(runs.len());
    let mut total = 0;
    for r in runs {
        bases.push(total);
        total += r.0.len();
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
