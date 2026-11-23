//! Best-effort text paint with sourced runs and a shared line baseline.

use crate::computed_style::{Direction, TextAlign};
use crate::css_text_page::FontProvider;
use crate::display_list::{Command, Rect, Rgb, Source};
use crate::inline_lines::GlyphBitmap;

/// Source text, weight, colour and CSS size.
pub(crate) type Run = (String, bool, u32, u32, bool, bool, Option<usize>);

pub(crate) struct Style {
    pub size: u32,
    pub line_height: u32,
    pub align: TextAlign,
    pub direction: Direction,
    pub pre: bool,
    pub background: u32,
}

/// Darken low-contrast foregrounds against a light e-ink background.
fn contrast_floor(color: u32, background: u32) -> u32 {
    let luminance = |c: u32| -> u32 {
        (((c >> 16) & 255) * 299 + ((c >> 8) & 255) * 587 + (c & 255) * 114) / 1000
    };
    if luminance(background) > 160 && luminance(color) > 128 {
        0x0040_4040
    } else {
        color
    }
}

fn placeholder(size: u32) -> GlyphBitmap {
    let width = (size / 2).max(3);
    let height = size.max(4);
    let mut coverage = vec![0; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            if x == 0 || x + 1 == width || y == 0 || y + 1 == height {
                coverage[(y * width + x) as usize] = 255;
            }
        }
    }
    GlyphBitmap {
        left: 0,
        top: -i32::try_from(height).unwrap_or(256),
        width,
        height,
        coverage,
    }
}

/// Byte offset of the next occurrence of `ch` at or after `*offset`,
/// advancing the cursor past it. The placer may re-request a character
/// (e.g. measuring both sides of a line-break decision), so a forward-only
/// scan can miss an occurrence that lies BEHIND the cursor; fall back to
/// scanning the prefix so the cursor always lands on a real occurrence. A
/// byte offset inside a multi-byte character would panic the next slice.
fn next_occurrence(text: &str, offset: &mut usize, ch: char) -> usize {
    *offset = (*offset).min(text.len());
    while !text.is_char_boundary(*offset) {
        *offset -= 1;
    }
    let found = text[*offset..]
        .char_indices()
        .find(|(_, c)| *c == ch)
        .map(|(i, _)| *offset + i)
        .or_else(|| {
            text[..*offset]
                .char_indices()
                .find(|(_, c)| *c == ch)
                .map(|(i, _)| i)
        });
    let Some(byte) = found else {
        return *offset;
    };
    *offset = byte + ch.len_utf8();
    byte
}

#[allow(clippy::too_many_lines)] // Measurement, baseline and mask placement share the same run data.
pub(crate) fn paint(
    runs: &[Run],
    font: &impl FontProvider,
    style: &Style,
    origin: (i64, i64),
    width: u32,
) -> Result<(Vec<Command>, i64, usize), &'static str> {
    let refs: Vec<_> = runs.iter().map(|r| (r.0.as_str(), r.1)).collect();
    let size_of = |leaf: usize| runs[leaf].3.clamp(1, 256);
    // Shape compatible font spans across DOM/style boundaries. Colour,
    // decoration and source identity do not break OpenType context.
    let mut shaped: Vec<Option<Vec<crate::css_text_page::PageCluster>>> =
        (0..runs.len()).map(|_| None).collect();
    let mut group_start = 0;
    while group_start < runs.len() {
        let mut group_end = group_start + 1;
        while group_end < runs.len()
            && runs[group_end].1 == runs[group_start].1
            && runs[group_end].3 == runs[group_start].3
            && runs[group_end].4 == runs[group_start].4
        {
            group_end += 1;
        }
        let text: String = runs[group_start..group_end]
            .iter()
            .map(|r| r.0.as_str())
            .collect();
        if let Some(clusters) = font.page_shape(
            &text,
            runs[group_start].3.clamp(1, 256),
            runs[group_start].1,
            runs[group_start].4,
        ) {
            let mut base = 0;
            for leaf in group_start..group_end {
                let end = base + runs[leaf].0.len();
                let mut local = Vec::new();
                for c in &clusters {
                    if c.start >= end || c.end <= base {
                        continue;
                    }
                    let origin = c.start >= base;
                    local.push(crate::css_text_page::PageCluster {
                        start: c.start.saturating_sub(base),
                        end: c.end.min(end) - base,
                        advance: if origin { c.advance } else { 0 },
                        bitmap: if origin {
                            GlyphBitmap {
                                left: c.bitmap.left,
                                top: c.bitmap.top,
                                width: c.bitmap.width,
                                height: c.bitmap.height,
                                coverage: c.bitmap.coverage.clone(),
                            }
                        } else {
                            GlyphBitmap {
                                left: 0,
                                top: 0,
                                width: 0,
                                height: 0,
                                coverage: Vec::new(),
                            }
                        },
                    });
                }
                shaped[leaf] = Some(local);
                base = end;
            }
        }
        group_start = group_end;
    }
    let mut measure_offsets = vec![0_usize; runs.len()];
    let glyph_width = |ch: char, leaf: usize| {
        let size = size_of(leaf);
        let byte = next_occurrence(&runs[leaf].0, &mut measure_offsets[leaf], ch);
        if let Some(clusters) = &shaped[leaf] {
            if let Some(cluster) = clusters
                .get(clusters.partition_point(|c| c.end <= byte))
                .filter(|c| c.start <= byte && c.end > byte)
            {
                return Some(if cluster.start == byte {
                    cluster.advance
                } else {
                    0
                });
            }
        }
        if font.has_glyph(ch, runs[leaf].1) == Some(false) {
            return Some((size / 2).max(3));
        }
        Some(
            font.page_advance(ch, size, runs[leaf].1, runs[leaf].4)
                .unwrap_or((size / 2).max(3)),
        )
    };
    let lines = crate::basic_text::place(&refs, width, style.pre, glyph_width)
        .map_err(|_| "text-layout-bounded")?;
    if lines.glyphs.is_empty() {
        return Ok((Vec::new(), 0, 0));
    }
    let mut struts = vec![(0_u32, 0_u32); lines.count as usize];
    for g in &lines.glyphs {
        let size = size_of(g.leaf);
        let (height, baseline) = font
            .page_character_metrics(g.character, size, g.bold, runs[g.leaf].4)
            .map(|m| (m.height, m.baseline))
            .unwrap_or((size + 4, i32::try_from(size).unwrap_or(16)));
        let baseline = u32::try_from(baseline).unwrap_or(size).min(height);
        let strut = &mut struts[g.line as usize];
        strut.0 = strut.0.max(baseline);
        strut.1 = strut.1.max(height.saturating_sub(baseline));
    }
    let mut line_tops = Vec::with_capacity(struts.len());
    let mut total_height = 0_i64;
    for strut in &mut struts {
        let natural = strut.0.saturating_add(strut.1).max(style.size + 4);
        let height = style.line_height.max(natural);
        strut.0 += (height - strut.0 - strut.1) / 2;
        line_tops.push(total_height);
        total_height += i64::from(height);
    }
    let mut line_widths = vec![0_u32; struts.len()];
    for g in &lines.glyphs {
        line_widths[g.line as usize] =
            line_widths[g.line as usize].max(g.x.saturating_add(g.advance));
    }
    let mut commands = Vec::new();
    let mut missing = 0;
    let mut paint_offsets = vec![0_usize; runs.len()];
    let mut from = 0;
    while from < lines.glyphs.len() {
        let first = lines.glyphs[from];
        let mut to = from + 1;
        while to < lines.glyphs.len()
            && lines.glyphs[to].line == first.line
            && lines.glyphs[to].leaf == first.leaf
        {
            to += 1;
        }
        let glyphs = &lines.glyphs[from..to];
        let size = size_of(first.leaf);
        let available = width.saturating_sub(line_widths[first.line as usize]);
        let offset = match style.align {
            TextAlign::Center => available / 2,
            TextAlign::Right => available,
            TextAlign::Initial if style.direction == Direction::Rtl => available,
            _ => 0,
        };
        let bitmaps: Vec<_> = glyphs
            .iter()
            .map(|g| {
                let byte =
                    next_occurrence(&runs[g.leaf].0, &mut paint_offsets[g.leaf], g.character);
                if let Some(clusters) = &shaped[g.leaf] {
                    if let Some(cluster) = clusters
                        .get(clusters.partition_point(|c| c.end <= byte))
                        .filter(|c| c.start <= byte && c.end > byte)
                    {
                        if cluster.start == byte {
                            return GlyphBitmap {
                                left: cluster.bitmap.left,
                                top: cluster.bitmap.top,
                                width: cluster.bitmap.width,
                                height: cluster.bitmap.height,
                                coverage: cluster.bitmap.coverage.clone(),
                            };
                        }
                        return GlyphBitmap {
                            left: 0,
                            top: 0,
                            width: 0,
                            height: 0,
                            coverage: Vec::new(),
                        };
                    }
                }
                if font.has_glyph(g.character, g.bold) != Some(false) {
                    if let Some(bitmap) =
                        font.page_raster(g.character, size, g.bold, runs[g.leaf].4)
                    {
                        return bitmap;
                    }
                }
                missing += 1;
                placeholder(size)
            })
            .collect();
        let baseline = i64::from(struts[first.line as usize].0);
        let left = glyphs
            .iter()
            .zip(&bitmaps)
            .map(|(g, b)| i64::from(g.x) + i64::from(b.left))
            .min()
            .unwrap_or(0);
        let right = glyphs
            .iter()
            .zip(&bitmaps)
            .map(|(g, b)| i64::from(g.x) + i64::from(b.left) + i64::from(b.width))
            .max()
            .unwrap_or(left);
        let ink_top = bitmaps
            .iter()
            .map(|b| baseline + i64::from(b.top))
            .min()
            .unwrap_or(0);
        let ink_bottom = bitmaps
            .iter()
            .map(|b| baseline + i64::from(b.top) + i64::from(b.height))
            .max()
            .unwrap_or(ink_top);
        let w = u32::try_from(right - left).map_err(|_| "glyph-width")?;
        let h = u32::try_from(ink_bottom - ink_top).map_err(|_| "glyph-height")?;
        if w > 0 && h > 0 {
            let count = usize::try_from(w)
                .ok()
                .and_then(|w| w.checked_mul(h as usize))
                .filter(|&n| n <= 4_194_304)
                .ok_or("glyph-mask-budget")?;
            let mut mask = vec![0_u8; count];
            for (g, bitmap) in glyphs.iter().zip(&bitmaps) {
                let dx = i64::from(g.x) + i64::from(bitmap.left) - left;
                let dy = baseline + i64::from(bitmap.top) - ink_top;
                for y in 0..bitmap.height {
                    for x in 0..bitmap.width {
                        let dst =
                            usize::try_from((dy + i64::from(y)) * i64::from(w) + dx + i64::from(x))
                                .map_err(|_| "glyph-index")?;
                        let src = (y * bitmap.width + x) as usize;
                        if dst < mask.len() && src < bitmap.coverage.len() {
                            mask[dst] = mask[dst].max(bitmap.coverage[src]);
                        }
                    }
                }
            }
            let rect = Rect {
                x: i32::try_from(origin.0 + left + i64::from(offset)).map_err(|_| "text-x")?,
                y: i32::try_from(origin.1 + line_tops[first.line as usize] + ink_top)
                    .map_err(|_| "text-y")?,
                width: w,
                height: h,
            };
            let color = contrast_floor(runs[first.leaf].2, style.background);
            commands.push(Command::GlyphRun {
                bounds: rect,
                layout: rect,
                coverage: mask,
                color: Rgb(
                    ((color >> 16) & 255) as u8,
                    ((color >> 8) & 255) as u8,
                    (color & 255) as u8,
                ),
                source: Source {
                    node: runs[first.leaf].6,
                    action: None,
                },
            });
        }
        if runs[first.leaf].5 {
            let extent = glyphs
                .last()
                .map_or(first.x, |g| g.x.saturating_add(g.advance))
                .saturating_sub(first.x);
            let color = contrast_floor(runs[first.leaf].2, style.background);
            commands.push(Command::Fill {
                rect: Rect {
                    x: i32::try_from(origin.0 + i64::from(first.x) + i64::from(offset))
                        .map_err(|_| "underline-x")?,
                    y: i32::try_from(origin.1 + line_tops[first.line as usize] + baseline + 1)
                        .map_err(|_| "underline-y")?,
                    width: extent,
                    height: (size / 16).max(1),
                },
                color: Rgb(
                    ((color >> 16) & 255) as u8,
                    ((color >> 8) & 255) as u8,
                    (color & 255) as u8,
                ),
                source: Source {
                    node: runs[first.leaf].6,
                    action: None,
                },
            });
        }
        from = to;
    }
    Ok((commands, total_height, missing))
}

#[cfg(test)]
mod tests {
    use super::contrast_floor;
    #[test]
    fn cursor_remeasurement_and_absent_collapsed_space_stay_on_boundaries() {
        let text = "a’ब";
        let mut cursor = 0;
        assert_eq!(super::next_occurrence(text, &mut cursor, 'a'), 0);
        assert_eq!(super::next_occurrence(text, &mut cursor, '’'), 1);
        assert_eq!(super::next_occurrence(text, &mut cursor, 'a'), 0);
        assert!(text.is_char_boundary(cursor));
        // A collapsed space can have originated from a tab or newline.
        assert_eq!(super::next_occurrence(text, &mut cursor, ' '), 1);
        assert_eq!(cursor, 1);
        assert_eq!(super::next_occurrence(text, &mut cursor, 'ब'), 4);
        assert_eq!(cursor, text.len());
    }

    #[test]
    fn light_text_on_light_paper_is_darkened() {
        assert!(contrast_floor(0xaaaaaa, 0xffffff) <= 0x007f_7f7f);
        assert_eq!(contrast_floor(0x112233, 0xffffff), 0x112233);
        assert_eq!(contrast_floor(0xffffff, 0x111111), 0xffffff);
    }
}
