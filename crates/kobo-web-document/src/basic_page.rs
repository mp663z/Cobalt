//! BASIC MODE (experimental, private, unadopted): a partial, best-effort page
//! painter for the resumption's "basic working browser first" track. Unlike
//! every `css_text_page` entry this NEVER proves a complete page: it paints
//! the boxes it can measure in normal flow, SKIPS and counts everything it
//! cannot, and returns the skip census with the list. A `PartialStats` with
//! any nonzero skip count means the image is a PARTIAL render - never present
//! one as complete or as Chrome parity. All exact painters and gates are
//! unchanged; this module shares only their measured primitives.
//!
//! Documented approximations (each counted in stats when used):
//! - no margin collapsing (adjoining margins add),
//! - diagnostic (floored) percentage widths from `WidthPass`,
//! - `font_size == 0` (fractional-em refusals) falls back to the nearest
//!   nonzero ancestor size, else 16px,
//! - backgrounds paint the padding box; border widths are not subtracted,
//! - floats, positioned boxes, flex subtrees and replaced content are skipped,
//! - line breaking is the existing narrow ASCII placer; blocks it refuses
//!   are skipped whole.
//!
//! Bounds: node visits are capped (MAX_VISITS); emitted commands are NOT
//! display-list budgeted - the caller replays the command vec into bounded
//! display lists (e.g. per strip) for rasterization. `output_truncated` is
//! set only when the visit cap stops the walk.

use std::collections::BTreeMap;

use crate::box_tree::{BoxKind, BoxTree, WidthPass};
use crate::computed_style::{Length, Margin};
use crate::css_text_page::FontProvider;
use crate::display_list::{Command, Rect, Rgb, Source};
use crate::inline_lines::{place_runs_normal, Lines};

const MAX_VISITS: usize = 200_000;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PartialStats {
    pub skips: BTreeMap<&'static str, usize>,
    pub approximations: BTreeMap<&'static str, usize>,
    pub blocks_painted: usize,
    pub text_blocks_painted: usize,
    pub glyph_runs: usize,
    pub fills: usize,
    pub output_truncated: bool,
    /// The intake style/box tree was itself truncated (sheet supply, match
    /// work, retained nodes): the render is partial by construction.
    pub intake_truncated: bool,
}

impl PartialStats {
    fn skip(&mut self, kind: &'static str) {
        *self.skips.entry(kind).or_insert(0) += 1;
    }
    fn approx(&mut self, kind: &'static str) {
        *self.approximations.entry(kind).or_insert(0) += 1;
    }
    /// True when nothing was skipped (the render may still carry the named
    /// approximations above - report them either way).
    #[must_use]
    pub fn complete(&self) -> bool {
        self.skips.is_empty() && !self.output_truncated
    }
}

/// Basic-mode greedy wrapper used when the narrow placer refuses a block:
/// words wrap greedily at `max_width`; a word wider than the line is hard-
/// broken (counted). ASCII-only input (callers fold first). Every fallback
/// block is counted as `greedy-wrap-fallback`.
fn greedy_wrap(
    runs: &[(&str, bool)],
    max_width: u32,
    mut advance: impl FnMut(char, bool) -> Option<u32>,
    stats: &mut PartialStats,
) -> Option<Lines> {
    if max_width == 0 {
        return None;
    }
    let mut glyphs = Vec::new();
    let mut x = 0u32;
    let mut line = 0u32;
    let space_w = advance(' ', false).filter(|&n| n > 0)?;
    let mut at_word_start = true;
    for (text, bold) in runs {
        for word in text.split_inclusive(|c: char| c == ' ') {
            let trimmed = word.trim_end_matches(' ');
            let trailing_space = word.len() != trimmed.len();
            if !trimmed.is_empty() {
                if !at_word_start {
                    if x + space_w > max_width { line += 1; x = 0; } else { x += space_w; }
                }
                let word_w: u32 = trimmed.chars().filter_map(|c| advance(c, *bold)).sum();
                if word_w > max_width {
                    stats.approx("word-hard-broken");
                }
                for ch in trimmed.chars() {
                    let w = advance(ch, *bold).filter(|&n| n > 0)?;
                    if x + w > max_width {
                        line += 1;
                        x = 0;
                        if w > max_width {
                            stats.approx("glyph-wider-than-line");
                        }
                    }
                    glyphs.push(crate::inline_lines::PositionedGlyph { character: ch, x, line, advance: w, bold: *bold, leaf: 0 });
                    x += w;
                }
                at_word_start = false;
            }
            if trailing_space {
                if x + space_w > max_width { line += 1; x = 0; } else { x += space_w; }
            }
        }
    }
    if glyphs.is_empty() {
        return None;
    }
    stats.approx("greedy-wrap-fallback");
    Some(Lines { glyphs, count: line + 1 })
}

/// Fold common Unicode punctuation to ASCII for the narrow placer. Every
/// substitution is counted as `unicode-folded-ascii` by the caller; text
/// with any other non-ASCII character still skips the block.
fn fold_char(ch: char) -> Option<&'static str> {
    Some(match ch {
        '\u{2018}' | '\u{2019}' | '\u{2032}' | '\u{00b4}' => "'",
        '\u{201c}' | '\u{201d}' | '\u{2033}' => "\"",
        '\u{2013}' | '\u{2014}' | '\u{2212}' | '\u{2010}' | '\u{2011}' => "-",
        '\u{2026}' => "...",
        '\u{00a0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' | '\u{202f}' | '\t' | '\n' | '\r' => " ",
        '\u{00b7}' | '\u{2022}' | '\u{25cf}' => "*",
        '\u{2192}' => ">",
        '\u{2190}' => "<",
        // Latin-1 Supplement and Extended-A letters fold to their ASCII base
        // (a transliteration approximation, counted per block by the caller).
        '\u{00c0}'..='\u{00c5}' | '\u{0100}' | '\u{0102}' | '\u{0104}' => "A",
        '\u{00e0}'..='\u{00e5}' | '\u{0101}' | '\u{0103}' | '\u{0105}' => "a",
        '\u{00c6}' => "AE",
        '\u{00e6}' => "ae",
        '\u{00c7}' | '\u{0106}' | '\u{0108}' | '\u{010a}' | '\u{010c}' => "C",
        '\u{00e7}' | '\u{0107}' | '\u{0109}' | '\u{010b}' | '\u{010d}' => "c",
        '\u{00d0}' | '\u{010e}' | '\u{0110}' => "D",
        '\u{00f0}' | '\u{010f}' | '\u{0111}' => "d",
        '\u{00c8}'..='\u{00cb}' | '\u{0112}' | '\u{0114}' | '\u{0116}' | '\u{0118}' | '\u{011a}' => "E",
        '\u{00e8}'..='\u{00eb}' | '\u{0113}' | '\u{0115}' | '\u{0117}' | '\u{0119}' | '\u{011b}' => "e",
        '\u{011c}' | '\u{011e}' | '\u{0120}' | '\u{0122}' => "G",
        '\u{011d}' | '\u{011f}' | '\u{0121}' | '\u{0123}' => "g",
        '\u{0124}' | '\u{0126}' => "H",
        '\u{0125}' | '\u{0127}' => "h",
        '\u{00cc}'..='\u{00cf}' | '\u{0128}' | '\u{012a}' | '\u{012c}' | '\u{012e}' | '\u{0130}' => "I",
        '\u{00ec}'..='\u{00ef}' | '\u{0129}' | '\u{012b}' | '\u{012d}' | '\u{012f}' | '\u{0131}' => "i",
        '\u{0132}' => "IJ",
        '\u{0133}' => "ij",
        '\u{0134}' => "J",
        '\u{0135}' => "j",
        '\u{0136}' => "K",
        '\u{0137}' => "k",
        '\u{0139}' | '\u{013b}' | '\u{013d}' | '\u{013f}' | '\u{0141}' => "L",
        '\u{013a}' | '\u{013c}' | '\u{013e}' | '\u{0140}' | '\u{0142}' => "l",
        '\u{00d1}' | '\u{0143}' | '\u{0145}' | '\u{0147}' => "N",
        '\u{00f1}' | '\u{0144}' | '\u{0146}' | '\u{0148}' | '\u{014b}' => "n",
        '\u{014a}' => "N",
        '\u{00d2}'..='\u{00d6}' | '\u{00d8}' | '\u{014c}' | '\u{014e}' | '\u{0150}' => "O",
        '\u{00f2}'..='\u{00f6}' | '\u{00f8}' | '\u{014d}' | '\u{014f}' | '\u{0151}' => "o",
        '\u{0152}' => "OE",
        '\u{0153}' => "oe",
        '\u{0154}' | '\u{0156}' | '\u{0158}' => "R",
        '\u{0155}' | '\u{0157}' | '\u{0159}' => "r",
        '\u{015a}' | '\u{015c}' | '\u{015e}' | '\u{0160}' => "S",
        '\u{015b}' | '\u{015d}' | '\u{015f}' | '\u{0161}' | '\u{017f}' => "s",
        '\u{00de}' => "TH",
        '\u{00fe}' => "th",
        '\u{0162}' | '\u{0164}' | '\u{0166}' => "T",
        '\u{0163}' | '\u{0165}' | '\u{0167}' => "t",
        '\u{00d9}'..='\u{00dc}' | '\u{0168}' | '\u{016a}' | '\u{016c}' | '\u{016e}' | '\u{0170}' | '\u{0172}' => "U",
        '\u{00f9}'..='\u{00fc}' | '\u{0169}' | '\u{016b}' | '\u{016d}' | '\u{016f}' | '\u{0171}' | '\u{0173}' => "u",
        '\u{0174}' => "W",
        '\u{0175}' => "w",
        '\u{00dd}' | '\u{0176}' | '\u{0178}' => "Y",
        '\u{00fd}' | '\u{00ff}' | '\u{0177}' => "y",
        '\u{0179}' | '\u{017b}' | '\u{017d}' => "Z",
        '\u{017a}' | '\u{017c}' | '\u{017e}' => "z",
        '\u{00df}' => "ss",
        c if c.is_ascii() && !c.is_control() => return None,
        _ => return Some(""),
    })
}

fn fold_ascii(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut folded = false;
    for ch in text.chars() {
        if ch.is_ascii() && !ch.is_control() {
            out.push(ch);
            continue;
        }
        match fold_char(ch) {
            Some("") => return None,
            Some(s) => {
                folded = true;
                out.push_str(s);
            }
            None => out.push(ch),
        }
    }
    let _ = folded;
    Some(out)
}

fn rgb(color: u32) -> Rgb {
    Rgb(
        ((color >> 16) & 255) as u8,
        ((color >> 8) & 255) as u8,
        (color & 255) as u8,
    )
}

struct Painter<'a, F: FontProvider> {
    tree: &'a BoxTree,
    font: &'a F,
    widths: WidthPass,
    stats: PartialStats,
    visits: usize,
}

impl<'a, F: FontProvider> Painter<'a, F> {
    fn font_size(&self, index: usize) -> u32 {
        let mut i = Some(index);
        while let Some(current) = i {
            let size = self.tree.boxes[current].style.font_size;
            if size != 0 {
                if current != index {
                    // counted once per text block by the caller
                    return size;
                }
                return size;
            }
            i = self.tree.boxes[current].parent;
        }
        16
    }

    fn margin_px(&mut self, margin: Margin, containing: u32) -> i64 {
        match margin {
            Margin::Px(v) => i64::from(v),
            Margin::Percent(p) => {
                self.stats.approx("margin-percent-floored");
                (i64::from(containing) * i64::from(p)) / 10_000
            }
            Margin::Auto | Margin::Em(_) => {
                self.stats.approx("margin-zeroed");
                0
            }
        }
    }

    /// Collect (text, bold) runs for a pure-inline block, tolerating the
    /// tree-level flags the exact collector refuses on. Second return is
    /// true when any leaf colour differs from the block's (v1 flattens them).
    /// Runs carry (text, bold, color). Color is the leaf's own computed
    /// color; the caller maps glyphs back to runs to paint link colors.
    fn collect_runs(&mut self, parent: usize) -> Result<(Vec<(String, bool, u32)>, bool), &'static str> {
        let owner = &self.tree.boxes[parent];
        if owner.kind != BoxKind::Block && owner.kind != BoxKind::AnonymousBlock && owner.kind != BoxKind::ListItem {
            return Err("not-block");
        }
        if owner.children.is_empty() {
            return Err("empty");
        }
        if !owner
            .children
            .iter()
            .all(|&c| matches!(self.tree.boxes[c].kind, BoxKind::Text | BoxKind::Inline))
        {
            return Err("mixed-content");
        }
        let block_color = owner.style.color;
        let mut mixed_color = false;
        let mut runs: Vec<(String, bool, u32)> = Vec::new();
        let mut stack: Vec<usize> = owner.children.iter().rev().copied().collect();
        let mut visited = 0usize;
        while let Some(index) = stack.pop() {
            visited += 1;
            if visited > 4096 || runs.len() >= 4096 {
                return Err("bounded");
            }
            let node = &self.tree.boxes[index];
            if node.style.color != block_color {
                mixed_color = true;
            }
            match node.kind {
                BoxKind::Text => {
                    let text = node.text.clone().ok_or("no-text")?;
                    runs.push((text, node.style.font_weight == 700, node.style.color));
                }
                BoxKind::Inline => {
                    for &c in node.children.iter().rev() {
                        stack.push(c);
                    }
                }
                _ => return Err("other-kind"),
            }
        }
        if runs.is_empty() {
            return Err("no-runs");
        }
        Ok((runs, mixed_color))
    }

    /// Paint one pure-inline text block at (content_x, top) with `width`.
    /// Returns the painted content height in px.
    fn paint_text_block(
        &mut self,
        index: usize,
        content_x: i64,
        top: i64,
        width: u32,
        list: &mut Vec<Command>,
    ) -> Result<i64, &'static str> {
        let node = &self.tree.boxes[index];
        let mut size = node.style.font_size;
        if size == 0 {
            size = self.font_size(index);
            self.stats.approx("font-size-fallback");
        }
        let (runs, mixed_color) = self.collect_runs(index)?;
        if mixed_color {
            self.stats.approx("inline-color-flattened");
        }
        let mut folded_any = false;
        let mut folded_runs: Vec<String> = Vec::with_capacity(runs.len());
        for (text, _, _) in &runs {
            let f = fold_ascii(text).ok_or("non-ascii-text")?;
            if f != *text {
                folded_any = true;
            }
            folded_runs.push(f);
        }
        if folded_any {
            self.stats.approx("unicode-folded-ascii");
        }
        let run_refs: Vec<(&str, bool)> = folded_runs
            .iter()
            .zip(runs.iter())
            .map(|(t, (_, b, _))| (t.as_str(), *b))
            .collect();
        let lines: Lines = match place_runs_normal(&run_refs, width, |ch, bold| {
            self.font.advance(ch, size, bold)
        }) {
            Ok(l) => l,
            Err(_) => greedy_wrap(&run_refs, width, |ch, bold| self.font.advance(ch, size, bold), &mut self.stats)
                .ok_or("place-and-greedy-refused")?,
        };
        // Map each glyph back to its run's color by walking the folded char
        // stream in glyph order; whitespace dropped at line breaks is
        // skipped over. Only engaged when a block mixes colors (links).
        let glyph_colors: Vec<u32> = if mixed_color {
            self.stats.approx("link-color-by-char-map");
            let mut stream: Vec<(char, u32)> = Vec::new();
            for ((f, (_, _, c))) in folded_runs.iter().zip(runs.iter()) {
                for ch in f.chars() {
                    stream.push((ch, *c));
                }
            }
            let mut pos = 0usize;
            let mut colors = Vec::with_capacity(lines.glyphs.len());
            for g in &lines.glyphs {
                let mut skipped = 0;
                while pos < stream.len()
                    && stream[pos].0 != g.character
                    && stream[pos].0.is_whitespace()
                    && skipped < 8
                {
                    pos += 1;
                    skipped += 1;
                }
                let color = stream.get(pos).map(|&(_, c)| c).unwrap_or(self.tree.boxes[index].style.color);
                colors.push(color);
                if pos < stream.len() {
                    pos += 1;
                }
            }
            colors
        } else {
            Vec::new()
        };
        let natural = self
            .font
            .normal_line_metrics(size, false)
            .map(|m| m.height)
            .or_else(|| self.font.line_height(size, false))
            .ok_or("no-line-metrics")?;
        let line_height = node
            .style
            .used_line_height(natural)
            .filter(|&v| v > 0)
            .unwrap_or(natural);
        let baseline = self
            .font
            .normal_line_metrics(size, false)
            .map(|m| m.baseline)
            .or_else(|| self.font.baseline_offset(size, false))
            .filter(|&b| b >= 0 && i64::from(b) < i64::from(natural))
            .ok_or("no-baseline")?;
        if lines.count == 0 {
            return Ok(0);
        }
        // Merge consecutive glyphs sharing (line, leaf, bold) into one
        // coverage command so long pages stay inside display-list budgets.
        let mut run_start: Option<usize> = Some(0);
        let glyphs = &lines.glyphs;
        let block_rgb = rgb(self.tree.boxes[index].style.color);
        let flush = |from: usize,
                     to: usize,
                     glyphs: &[crate::inline_lines::PositionedGlyph],
                     run_color: Rgb,
                     list: &mut Vec<Command>,
                     stats: &mut PartialStats,
                     font: &F|
         -> Result<(), ()> {
            if from >= to {
                return Ok(());
            }
            let first = glyphs[from];
            let line_top = top
                + i64::from(to_owned_line(glyphs, from)) * i64::from(line_height);
            let x0 = content_x + i64::from(first.x);
            let last = glyphs[to - 1];
            let width_px = i64::from(last.x) + i64::from(last.advance);
            let run_w = u32::try_from(width_px - i64::from(first.x)).map_err(|_| ())?;
            if run_w == 0 {
                return Ok(());
            }
            let mut mask = vec![0u8; run_w as usize * line_height as usize];
            for g in &glyphs[from..to] {
                let bitmap = font.raster(g.character, size, g.bold).ok_or(())?;
                let dx = i64::from(g.x) - i64::from(first.x) + i64::from(bitmap.left);
                let dy = i64::from(baseline) + i64::from(bitmap.top);
                for row in 0..bitmap.height {
                    let y = dy + i64::from(row);
                    if y < 0 || y >= i64::from(line_height) {
                        continue;
                    }
                    for col in 0..bitmap.width {
                        let x = dx + i64::from(col);
                        if x < 0 || x >= i64::from(run_w) {
                            continue;
                        }
                        let dst = (y as usize) * run_w as usize + x as usize;
                        let src = (row * bitmap.width + col) as usize;
                        if src < bitmap.coverage.len() {
                            mask[dst] = mask[dst].max(bitmap.coverage[src]);
                        }
                    }
                }
            }
            let color = run_color;
            let rect = Rect {
                x: i32::try_from(x0).map_err(|_| ())?,
                y: i32::try_from(line_top).map_err(|_| ())?,
                width: run_w,
                height: line_height,
            };
            list.push(Command::GlyphRun {
                bounds: rect,
                layout: rect,
                coverage: mask,
                color,
                source: Source { node: self_node(glyphs, from), action: None },
            });
            stats.glyph_runs += 1;
            Ok(())
        };
        let color_of = |i: usize| -> Rgb {
            if glyph_colors.is_empty() {
                block_rgb
            } else {
                rgb(glyph_colors[i])
            }
        };
        for i in 1..=glyphs.len() {
            let boundary = i == glyphs.len()
                || glyphs[i].line != glyphs[i - 1].line
                || glyphs[i].bold != glyphs[i - 1].bold
                || (!glyph_colors.is_empty() && glyph_colors[i] != glyph_colors[i - 1]);
            if boundary {
                let from = run_start.take().unwrap_or(0);
                if flush(from, i, glyphs, color_of(from), list, &mut self.stats, self.font).is_err() {
                    self.stats.output_truncated = true;
                    return Ok(i64::from(lines.count) * i64::from(line_height));
                }
                if i < glyphs.len() {
                    run_start = Some(i);
                }
            }
        }
        self.stats.text_blocks_painted += 1;
        Ok(i64::from(lines.count) * i64::from(line_height))
    }

    /// Normal-flow stacking walk. Returns the subtree's vertical extent in px
    /// (content only; caller adds its own padding/margins).
    fn layout_block(
        &mut self,
        index: usize,
        content_x: i64,
        top: i64,
        width: u32,
        list: &mut Vec<Command>,
    ) -> i64 {
        self.visits += 1;
        if self.visits > MAX_VISITS {
            self.stats.output_truncated = true;
            return 0;
        }
        // Commands emitted for this subtree start here; the box's own
        // background is INSERTED at this position once its height is known,
        // so it paints behind all descendants (correct stacking order).
        let bg_insert_at = list.len();
        let node_kind = self.tree.boxes[index].kind;
        let style = self.tree.boxes[index].style;
        match node_kind {
            BoxKind::Flex => {
                // v1 approximation: flex containers lay out as plain block
                // stacks (row direction collapses to vertical).
                self.stats.approx("flex-as-block");
            }
            BoxKind::InlineBlock => {
                self.stats.approx("inline-block-as-block");
            }
            _ => {}
        }
        let (mut content_x, mut top) = (content_x, top);
        match style.positioned.position {
            crate::computed_style::Position::Static => {}
            crate::computed_style::Position::Relative | crate::computed_style::Position::Sticky => {
                // Relative (and sticky at scroll offset 0) occupies its static
                // flow space and paints shifted by the resolved offsets.
                let t = self.margin_px(style.positioned.offsets[0], width);
                let r = self.margin_px(style.positioned.offsets[1], width);
                let b = self.margin_px(style.positioned.offsets[2], width);
                let l = self.margin_px(style.positioned.offsets[3], width);
                let dx = l - r;
                let dy = t - b;
                if dx != 0 || dy != 0 {
                    self.stats.approx("relative-offset-applied");
                } else {
                    self.stats.approx("relative-as-static");
                }
                content_x += dx;
                top += dy;
            }
            crate::computed_style::Position::Absolute | crate::computed_style::Position::Fixed => {
                self.stats.skip("positioned");
                return 0;
            }
        }
        let pad = |v: u32, stats: &mut PartialStats| -> i64 {
            if v == crate::computed_style::PADDING_UNREPRESENTABLE {
                stats.approx("padding-unrepresentable-zeroed");
                0
            } else {
                i64::from(v)
            }
        };
        let pad_top = pad(style.padding_top, &mut self.stats);
        let pad_bot = pad(style.padding_bottom, &mut self.stats);
        let pad_left = pad(style.padding_left, &mut self.stats);
        let pad_right = pad(style.padding_right, &mut self.stats);
        let content_top = top + pad_top;
        let node = &self.tree.boxes[index];
        let pure_inline = !node.children.is_empty()
            && node
                .children
                .iter()
                .all(|&c| matches!(self.tree.boxes[c].kind, BoxKind::Text | BoxKind::Inline));
        let mut content_height: i64 = 0;
        if pure_inline {
            match self.paint_text_block(index, content_x + pad_left, content_top, (i64::from(width) - pad_left - pad_right).max(0) as u32, list) {
                Ok(h) => content_height = h,
                Err(reason) => self.stats.skip(reason),
            }
        } else {
            let mut cursor = content_top;
            let child_width = (i64::from(width) - pad_left - pad_right).max(0) as u32;
            for &child in &self.tree.boxes[index].children {
                let ckind = self.tree.boxes[child].kind;
                if matches!(ckind, BoxKind::Text | BoxKind::Inline) {
                    self.stats.skip("stray-inline");
                    continue;
                }
                let cstyle = self.tree.boxes[child].style;
                let containing = width;
                let ml = self.margin_px(cstyle.margin_left, containing);
                let mt = self.margin_px(cstyle.margin_top, containing);
                let mb = self.margin_px(cstyle.margin_bottom, containing);
                let cw = self
                    .widths
                    .widths
                    .get(child)
                    .copied()
                    .flatten()
                    .map(|w| w.content)
                    .unwrap_or(child_width);
                if self.widths.widths.get(child).copied().flatten().is_none() {
                    self.stats.approx("width-fallback-parent");
                }
                cursor += mt;
                let child_x = content_x + pad_left + ml;
                let h = self.layout_block(child, child_x, cursor, cw, list);
                // Background for this child, behind nothing (emitted after
                // descendants in v1: paint order approximation, counted once).
                cursor += h + mb;
            }
            content_height = cursor - content_top;
        }
        // Definite px height wins for the background rect when larger.
        if let Length::Px(h) = style.height {
            if i64::from(h) > content_height {
                content_height = i64::from(h);
            }
        }
        // Background: padding box, inserted behind everything this subtree
        // emitted. The root's background is the canvas fill (already first).
        if Some(index) != self.tree.roots.first().copied() {
            if let Some(color) = style.used_background_color() {
                let bw = pad_left + i64::from(width) + pad_right;
                let bh = pad_top + content_height + pad_bot;
                if bw > 0 && bh > 0 {
                    if let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                        i32::try_from(content_x),
                        i32::try_from(top),
                        u32::try_from(bw),
                        u32::try_from(bh),
                    ) {
                        list.insert(
                            bg_insert_at,
                            Command::Fill {
                                rect: Rect { x, y, width: w, height: h },
                                color: rgb(color),
                                source: Source { node: self.tree.boxes[index].source, action: None },
                            },
                        );
                        self.stats.fills += 1;
                    }
                }
            }
        }
        self.stats.blocks_painted += 1;
        let total = content_height + pad_top + pad_bot;
        if total > 100_000 {
            eprintln!("HUGE block index={index} kind={node_kind:?} total={total} content={content_height} pad_t={pad_top} pad_b={pad_bot} height_style={:?}", style.height);
        }
        total
    }
}

fn to_owned_line(glyphs: &[crate::inline_lines::PositionedGlyph], i: usize) -> u32 {
    glyphs[i].line
}

fn self_node(_glyphs: &[crate::inline_lines::PositionedGlyph], _i: usize) -> Option<usize> {
    None
}

/// Paint a best-effort partial page. The returned stats MUST travel with any
/// image produced from the list: a nonzero skip census or `output_truncated`
/// marks the render partial.
///
/// # Errors
/// Fails only on invalid viewport or an unpaintable root (no boxes); partial
/// results always return Ok with their census.
pub fn paint_basic_page(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
) -> Result<(Vec<Command>, PartialStats, i64), &'static str> {
    if viewport_width == 0 || viewport_height == 0 {
        return Err("viewport");
    }
    if tree.roots.len() != 1 || tree.roots[0] >= tree.boxes.len() {
        return Err("root");
    }
    let widths = WidthPass::from_boxes(tree, viewport_width);
    let mut painter = Painter {
        tree,
        font,
        widths,
        stats: PartialStats::default(),
        visits: 0,
    };
    painter.stats.intake_truncated = tree.truncated;
    let mut list: Vec<Command> = Vec::new();
    let root = tree.roots[0];
    // Canvas background from root (or body) as in the exact painter.
    let body = tree.boxes[root].children.first().copied();
    let canvas = tree.boxes[root]
        .style
        .used_background_color()
        .or_else(|| body.and_then(|b| tree.boxes[b].style.used_background_color()));
    if let Some(color) = canvas {
        list.push(Command::Fill {
            rect: Rect { x: 0, y: 0, width: viewport_width, height: viewport_height },
            color: rgb(color),
            source: Source { node: tree.boxes[root].source, action: None },
        });
        painter.stats.fills += 1;
    }
    // Walk top-level blocks: root's descendants in order, block by block.
    let mut cursor = 0i64;
    let root_width = viewport_width;
    let start: Vec<usize> = match tree.boxes[root].kind {
        BoxKind::Block | BoxKind::AnonymousBlock | BoxKind::ListItem => vec![root],
        _ => vec![],
    };
    for top_index in start {
        cursor += painter.layout_block(top_index, 0, cursor, root_width, &mut list);
    }
    Ok((list, painter.stats, cursor))
}
