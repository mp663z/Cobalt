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

use std::collections::{BTreeMap, BTreeSet};

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
    font_sans: Option<&'a F>,
    font_mono: Option<&'a F>,
    widths: WidthPass,
    stats: PartialStats,
    visits: usize,
    viewport_w: i64,
}

impl<'a, F: FontProvider> Painter<'a, F> {
    /// Family choice for a block's text (BASIC lane): generic families
    /// map to the supplied sans/mono faces, everything else stays on the
    /// default serif face. Inline-level family mixing inside one block
    /// flattens to the block family (disclosed approximation).
    fn face_for(&mut self, index: usize) -> &'a F {
        match self.tree.boxes[index].style.font_family {
            Some(crate::computed_style::GenericFamily::SansSerif) => {
                if let Some(f) = self.font_sans {
                    self.stats.approx("font-family-sans");
                    return f;
                }
                self.font
            }
            Some(crate::computed_style::GenericFamily::Monospace) => {
                if let Some(f) = self.font_mono {
                    self.stats.approx("font-family-mono");
                    return f;
                }
                self.font
            }
            _ => self.font,
        }
    }
}

impl<'a, F: FontProvider> Painter<'a, F> {
    #[allow(clippy::wrong_self_convention)]
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
    /// Collect runs from a pure-inline child list (Text | Inline only).
    /// `block_color` is the fallback color of the owning block.
    /// Runs as (text, bold, color, font_size). A text box's own size,
    /// falling back to its ancestors; 0 is impossible after the fallback.
    fn collect_runs_of(&mut self, children: &[usize], block_color: u32) -> Result<(Vec<(String, bool, u32, u32)>, bool), &'static str> {
        if children.is_empty() {
            return Err("empty");
        }
        let mut mixed_color = false;
        let mut runs: Vec<(String, bool, u32, u32)> = Vec::new();
        let mut stack: Vec<usize> = children.iter().rev().copied().collect();
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
                    let run_size = self.font_size(index);
                    runs.push((text, node.style.font_weight == 700, node.style.color, run_size));
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

    /// True when every child is Text or Inline (a pure-inline run group).
    /// Shallow: an Inline child with block descendants is still Inline
    /// here; use `deep_pure_inline` where recursion matters.
    fn pure_inline_children(&self, children: &[usize]) -> bool {
        !children.is_empty()
            && children
                .iter()
                .all(|&c| matches!(self.tree.boxes[c].kind, BoxKind::Text | BoxKind::Inline))
    }

    /// True when the subtree is entirely Text/Inline content.
    fn deep_pure_inline(&self, children: &[usize]) -> bool {
        !children.is_empty()
            && children.iter().all(|&c| {
                let node = &self.tree.boxes[c];
                match node.kind {
                    BoxKind::Text => true,
                    BoxKind::Inline => self.deep_pure_inline(&node.children),
                    _ => false,
                }
            })
    }

    /// Paint one pure-inline text block (or an anonymous inline run group
    /// of a mixed-content block) at (content_x, top) with `width`.
    /// `index` supplies the block style (font size, line height, color);
    /// `children` is the run source. Returns the painted height in px.
    fn paint_text_block(
        &mut self,
        index: usize,
        children: &[usize],
        content_x: i64,
        top: i64,
        width: u32,
        list: &mut Vec<Command>,
    ) -> Result<i64, &'static str> {
        let face = self.face_for(index);
        let node = &self.tree.boxes[index];
        let mut size = node.style.font_size;
        if size == 0 {
            size = self.font_size(index);
            self.stats.approx("font-size-fallback");
        }
        let (mut runs, mixed_color) = self.collect_runs_of(children, self.tree.boxes[index].style.color)?;
        // Per-run font sizes: when every run resolves to one size that
        // differs from the block's, the block is a same-size inline group
        // (e.g. a 7pt subtext span inside a 10pt cell) — paint at the run
        // size. Mixed sizes stay at the block size (baseline sharing is
        // not modelled); both are counted.
        {
            let mut sizes = runs.iter().map(|r| r.3);
            if let Some(first_size) = sizes.next() {
                if sizes.all(|s| s == first_size) && first_size != size {
                    size = first_size;
                    self.stats.approx("run-size-uniform-override");
                } else if runs.iter().any(|r| r.3 != size) {
                    self.stats.approx("mixed-font-size-flattened");
                }
            }
        }
        // Unordered-list marker approximation: a literal bullet glyph
        // prefixed to the item's first run (list-style-type is not read).
        if self.tree.boxes[index].kind == BoxKind::ListItem {
            if let Some(first) = runs.first_mut() {
                first.0.insert_str(0, "* ");
                self.stats.approx("list-marker-bullet");
            }
        }
        if mixed_color {
            self.stats.approx("inline-color-flattened");
        }
        let mut folded_any = false;
        let mut folded_runs: Vec<String> = Vec::with_capacity(runs.len());
        for (text, _, _, _) in &runs {
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
            .map(|(t, (_, b, _, _))| (t.as_str(), *b))
            .collect();
        let lines: Lines = match place_runs_normal(&run_refs, width, |ch, bold| {
            face.advance(ch, size, bold)
        }) {
            Ok(l) => l,
            Err(_) => greedy_wrap(&run_refs, width, |ch, bold| face.advance(ch, size, bold), &mut self.stats)
                .ok_or("place-and-greedy-refused")?,
        };
        // Map each glyph back to its run's color by walking the folded char
        // stream in glyph order; whitespace dropped at line breaks is
        // skipped over. Only engaged when a block mixes colors (links).
        let glyph_colors: Vec<u32> = if mixed_color {
            self.stats.approx("link-color-by-char-map");
            let mut stream: Vec<(char, u32)> = Vec::new();
            for ((f, (_, _, c, _))) in folded_runs.iter().zip(runs.iter()) {
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
            .or_else(|| face.line_height(size, false))
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
            .or_else(|| face.baseline_offset(size, false))
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
                if flush(from, i, glyphs, color_of(from), list, &mut self.stats, face).is_err() {
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
        abs_origin: (i64, i64),
    ) -> i64 {
        self.visits += 1;
        if self.visits > MAX_VISITS {
            self.stats.output_truncated = true;
            return 0;
        }
        // Commands emitted for this subtree start here; the box's own
        // background is INSERTED at this position once its height is known,
        // so it paints behind all descendants (correct stacking order).
        let mut bg_insert_at = list.len();
        let entry_len = list.len();
        let node_kind = self.tree.boxes[index].kind;
        let style = self.tree.boxes[index].style;
        // visibility:hidden (FL38 ext, inherited): layout runs and flow
        // space is kept, but the subtree emits no paint commands.
        let hidden = style.positioned.hidden;
        if hidden {
            self.stats.skip("visibility-hidden");
        }
        let mut flex_row = false;
        if matches!(node_kind, BoxKind::InlineBlock) {
            self.stats.approx("inline-block-as-block");
        }
        // Flex-row slot widths, computed up front so a container whose
        // columns would come out too narrow to set text in falls back to
        // the v1 vertical stack instead of refusing every line.
        let mut flex_slots: Option<(Vec<i64>, i64)> = None;
        if matches!(node_kind, BoxKind::Flex) {
            let kids = &self.tree.boxes[index].children;
            let row_eligible = !kids.is_empty()
                && !self.deep_pure_inline(kids)
                && kids.iter().all(|&c| {
                    let ck = self.tree.boxes[c].kind;
                    !(matches!(ck, BoxKind::Text)
                        || (matches!(ck, BoxKind::Inline)
                            && self.pure_inline_children(&self.tree.boxes[c].children)))
                });
            // Children with no text, no image marker, no background and no
            // border paint nothing; giving them a column squeezes the
            // visible siblings, so they are skipped (counted).
            let visible = |c: usize| -> bool {
                let cs = self.tree.boxes[c].style;
                if cs.used_background_color().is_some() || cs.decor.has_painted_border() {
                    return true;
                }
                let mut stack = vec![c];
                while let Some(n) = stack.pop() {
                    let b = &self.tree.boxes[n];
                    if matches!(b.kind, BoxKind::Text) {
                        return true;
                    }
                    stack.extend_from_slice(&b.children);
                }
                false
            };
            if row_eligible {
                let pad_or_zero = |v: u32| -> i64 {
                    if v == crate::computed_style::PADDING_UNREPRESENTABLE {
                        0
                    } else {
                        i64::from(v)
                    }
                };
                let avail = (i64::from(width)
                    - pad_or_zero(style.padding_left)
                    - pad_or_zero(style.padding_right))
                .max(0);
                let mut slots: Vec<i64> = Vec::with_capacity(kids.len());
                let mut fixed_total = 0i64;
                let mut flex_count = 0i64;
                for &c in kids {
                    if !visible(c) {
                        slots.push(i64::MIN);
                        continue;
                    }
                    let cs = self.tree.boxes[c].style;
                    let ml = self.margin_px(cs.margin_left, width);
                    let mr = self.margin_px(cs.margin_right, width);
                    match self.widths.widths.get(c).copied().flatten() {
                        Some(w) => {
                            let v = i64::from(w.content) + ml + mr;
                            slots.push(v);
                            fixed_total += v;
                        }
                        None => {
                            slots.push(-1);
                            flex_count += 1;
                        }
                    }
                }
                if slots.iter().any(|&v| v == i64::MIN) {
                    self.stats.approx("flex-row-empty-child-skipped");
                }
                let share = if flex_count > 0 {
                    ((avail - fixed_total) / flex_count).max(0)
                } else {
                    0
                };
                let min_cw = slots
                    .iter()
                    .filter(|&&v| v != i64::MIN)
                    .map(|&v| if v >= 0 { v } else { share })
                    .fold(i64::MAX, i64::min);
                if min_cw >= 64 && min_cw != i64::MAX {
                    flex_row = true;
                    flex_slots = Some((slots, share));
                    self.stats.approx("flex-row-basic");
                } else {
                    self.stats.approx("flex-row-refused-narrow");
                    self.stats.approx("flex-as-block");
                }
            } else {
                self.stats.approx("flex-as-block");
            }
        }
        let (mut content_x, mut top) = (content_x, top);
        let mut out_of_flow = false;
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
                // BASIC lane: paint positioned boxes instead of skipping.
                // They take no flow space (the caller adds the returned
                // height, which is forced to zero below). Absolute boxes
                // resolve offsets against the nearest positioned
                // ancestor's origin (threaded as abs_origin); fixed boxes
                // resolve against the page top at scroll 0
                // (fixed-as-absolute). Offsets that resolve to all zero
                // land on the containing origin, not the true static
                // position (abs-auto-to-origin). Width is the containing
                // width, not shrink-to-fit (abs-width-containing).
                let t = self.margin_px(style.positioned.offsets[0], width);
                let r = self.margin_px(style.positioned.offsets[1], width);
                let b = self.margin_px(style.positioned.offsets[2], width);
                let l = self.margin_px(style.positioned.offsets[3], width);
                if matches!(style.positioned.position, crate::computed_style::Position::Fixed) {
                    self.stats.approx("fixed-as-absolute");
                    content_x = l - r;
                    top = t - b;
                } else {
                    content_x = abs_origin.0 + l - r;
                    top = abs_origin.1 + t - b;
                }
                if t == 0 && r == 0 && b == 0 && l == 0 {
                    self.stats.approx("abs-auto-to-origin");
                }
                self.stats.approx("positioned-basic");
                out_of_flow = true;
                if hidden {
                    return 0;
                }
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
        // Origin this box hands to absolutely-positioned descendants:
        // itself when positioned, else the inherited one.
        let child_abs_origin = if matches!(
            style.positioned.position,
            crate::computed_style::Position::Static
        ) {
            abs_origin
        } else {
            (content_x, top)
        };
        let node = &self.tree.boxes[index];
        let pure_inline = self.deep_pure_inline(&self.tree.boxes[index].children);
        let mut content_height: i64 = 0;
        if pure_inline {
            let tree = self.tree;
            match self.paint_text_block(index, &tree.boxes[index].children, content_x + pad_left, content_top, (i64::from(width) - pad_left - pad_right).max(0) as u32, list) {
                Ok(h) => content_height = h,
                Err(reason) => self.stats.skip(reason),
            }
        } else if flex_row {
            let (slots, share) = flex_slots.take().unwrap();
            let kids = &self.tree.boxes[index].children;
            let mut x = content_x + pad_left;
            let mut row_h = 0i64;
            for (k, &c) in kids.iter().enumerate().filter(|(k, _)| slots[*k] != i64::MIN) {
                let cs = self.tree.boxes[c].style;
                let ml = self.margin_px(cs.margin_left, width);
                let mr = self.margin_px(cs.margin_right, width);
                let slot = if slots[k] >= 0 { slots[k] } else { share };
                let cw = (slot - ml - mr).max(0) as u32;
                x += ml;
                let h = self.layout_block(c, x, content_top, cw, list, child_abs_origin);
                row_h = row_h.max(h);
                x += i64::from(cw) + mr;
            }
            content_height = row_h;
        } else {
            let mut cursor = content_top;
            let mut prev_mb = 0i64;
            let child_width = (i64::from(width) - pad_left - pad_right).max(0) as u32;
            let mut anon: Vec<usize> = Vec::new();
            macro_rules! flush_anon {
                ($cursor:ident) => {
                    if !anon.is_empty() {
                        match self.paint_text_block(index, &anon, content_x + pad_left, $cursor, child_width, list) {
                            Ok(h) => $cursor += h,
                            Err(reason) => self.stats.skip(reason),
                        }
                        anon.clear();
                    }
                };
            }
            for &child in &self.tree.boxes[index].children {
                let ckind = self.tree.boxes[child].kind;
                // Inline runs between block children form an anonymous
                // block (CSS 2.2 9.2.1.1); inline boxes with only inline
                // content join the run, text is always run content.
                if matches!(ckind, BoxKind::Text)
                    || (matches!(ckind, BoxKind::Inline) && self.pure_inline_children(&self.tree.boxes[child].children))
                {
                    anon.push(child);
                    continue;
                }
                flush_anon!(cursor);
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
                // Adjacent vertical margins collapse to the larger one.
                if prev_mb != 0 && mt != 0 {
                    self.stats.approx("margin-collapse-adjacent");
                }
                cursor += if prev_mb == 0 { mt } else { prev_mb.max(mt) };
                let child_x = content_x + pad_left + ml;
                let h = self.layout_block(child, child_x, cursor, cw, list, child_abs_origin);
                cursor += h;
                prev_mb = mb;
            }
            flush_anon!(cursor);
            cursor += prev_mb;
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
        // Borders (BASIC lane): four thin fills straddling the padding
        // box, painted outward since this lane does not reserve border
        // space in layout (approximation, counted per side). Dashed and
        // dotted paint solid, counted. Block-level boxes only.
        {
            let used = style.decor.used_border();
            if used.iter().any(|w| *w > 0) {
                let pb_w = pad_left + i64::from(width) + pad_right;
                let pb_h = pad_top + content_height + pad_bot;
                let (bt, br, bb, bl) = (used[0], used[1], used[2], used[3]);
                let color_for = |side: usize| -> Option<u32> {
                    match style.decor.border_color[side] {
                        crate::computed_style::BorderColor::Transparent => None,
                        crate::computed_style::BorderColor::CurrentColor => Some(style.color),
                        crate::computed_style::BorderColor::Opaque(v) => Some(v),
                    }
                };
                if style.decor.border_style.iter().any(|bs| {
                    matches!(bs, crate::computed_style::BorderStyle::Dashed
                        | crate::computed_style::BorderStyle::Dotted)
                }) {
                    self.stats.approx("border-style-solid-approx");
                }
                let mut emit = |x: i64, y: i64, w: i64, h: i64, side: usize| {
                    if w <= 0 || h <= 0 {
                        return;
                    }
                    let Some(col) = color_for(side) else { return };
                    if let (Ok(x), Ok(y), Ok(w), Ok(h)) = (
                        i32::try_from(x),
                        i32::try_from(y),
                        u32::try_from(w.min(512)),
                        u32::try_from(h),
                    ) {
                        list.insert(
                            bg_insert_at,
                            Command::Fill {
                                rect: Rect { x, y, width: w, height: h },
                                color: rgb(col),
                                source: Source { node: self.tree.boxes[index].source, action: None },
                            },
                        );
                        bg_insert_at += 1;
                        self.stats.fills += 1;
                        self.stats.approx("border-side");
                    }
                };
                let (bt, br, bb, bl) = (i64::from(bt), i64::from(br), i64::from(bb), i64::from(bl));
                emit(content_x - bl, top - bt, pb_w + bl + br, bt, 0);
                emit(content_x + pb_w, top, br, pb_h, 1);
                emit(content_x - bl, top + pb_h, pb_w + bl + br, bb, 2);
                emit(content_x - bl, top, bl, pb_h, 3);
            }
        }
        if hidden {
            list.truncate(entry_len);
        } else if out_of_flow {
            // Fully off-canvas positioned boxes emit nothing (slide-out
            // panels parked just outside the viewport otherwise smear
            // clipped slivers along the edge).
            let pb_w = pad_left + i64::from(width) + pad_right;
            if content_x + pb_w <= 0 || content_x >= self.viewport_w {
                list.truncate(entry_len);
                self.stats.skip("positioned-offscreen");
            }
        }
        self.stats.blocks_painted += 1;
        if out_of_flow {
            return 0;
        }
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

/// BASIC-mode intake normalization: rewrite table-structure tags
/// (`table`, `thead`, `tbody`, `tfoot`, `tr`, `td`, `th`, `caption`,
/// `colgroup`, `col`) to `div` so their content flows as blocks,
/// `<img>` tags to their `alt` text (empty when no alt), and
/// presentational `bgcolor` attributes to an inline style background. The engine has no
/// table display and no replaced content; without this the content
/// vanishes. Cells stack vertically (no column geometry) and images show
/// as their alt text - approximations that MUST be disclosed with any
/// render made from normalized input.
fn normalize_tables_and_presentational(input: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>, usize, usize, usize) {
    const NAMES: [&[u8]; 10] = [
        b"table", b"thead", b"tbody", b"tfoot", b"tr", b"td", b"th", b"caption", b"colgroup", b"col",
    ];
    let mut out = Vec::with_capacity(input.len());
    let mut images: Vec<Vec<u8>> = Vec::new();
    // Positions in `out` of currently-open <picture> tags. A marker div
    // emitted for an <img> inside <picture> inside <a> is dropped by the
    // box tree, so the marker replaces the whole picture element instead
    // (a block child of the anchor paints). Counted as pictures_hoisted.
    let mut picture_open: Vec<usize> = Vec::new();
    let mut pictures_hoisted = 0usize;
    let mut table_rows_flexed = 0usize;
    let mut form_chrome = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        if input[i] == b'<' && i + 1 < input.len() && input[i + 1] != b'!' {
            // Find the tag end (quote-aware).
            let mut j = i + 1;
            let mut quote: Option<u8> = None;
            while j < input.len() {
                match (quote, input[j]) {
                    (None, b'"') => quote = Some(b'"'),
                    (None, b'\'') => quote = Some(b'\''),
                    (Some(q), c) if c == q => quote = None,
                    (None, b'>') => break,
                    _ => {}
                }
                j += 1;
            }
            let tag_end = j.min(input.len());
            let tag = &input[i..tag_end];
            // Form controls -> static text approximations (the engine has
            // no form-control boxes; same philosophy as img->alt).
            // <input>: hidden/checkbox/radio vanish; submit/button/reset
            // show their value; text-like inputs show value or
            // placeholder. <textarea>: its text content. <select>: the
            // selected (else first) option's text. All escaped.
            if tag.len() >= 6
                && tag[1..].starts_with(b"input")
                && matches!(tag.get(6), Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>'))
            {
                let ty = extract_attr(tag, b"type").unwrap_or(b"text");
                let is_button = ty.eq_ignore_ascii_case(b"submit")
                    || ty.eq_ignore_ascii_case(b"button")
                    || ty.eq_ignore_ascii_case(b"reset");
                let emit = if ty.eq_ignore_ascii_case(b"hidden")
                    || ty.eq_ignore_ascii_case(b"checkbox")
                    || ty.eq_ignore_ascii_case(b"radio")
                {
                    None
                } else if is_button {
                    Some(extract_attr(tag, b"value").unwrap_or(b"Submit"))
                } else {
                    // Text-like inputs always get a chrome box, even when
                    // empty: an undrawn text field reads as a hole.
                    Some(
                        extract_attr(tag, b"value")
                            .or_else(|| extract_attr(tag, b"placeholder"))
                            .unwrap_or(b""),
                    )
                };
                if let Some(text) = emit {
                    // Basic form chrome: a bordered block box (the class
                    // attribute is preserved so page styling still
                    // applies; the chrome border is only a fallback).
                    out.extend_from_slice(b"<div");
                    if let Some(cls) = extract_attr(tag, b"class") {
                        out.extend_from_slice(b" class=\"");
                        out.extend_from_slice(cls);
                        out.extend_from_slice(b"\"");
                    }
                    if is_button {
                        out.extend_from_slice(b" style=\"border:1px solid #808080;padding:2px 6px;margin:2px 0\">");
                    } else {
                        out.extend_from_slice(b" style=\"border:1px solid #a6a6a6;padding:3px;margin:2px 0\">");
                    }
                    push_escaped(&mut out, &decode_attr_entities(text));
                    out.extend_from_slice(b"</div>");
                    form_chrome += 1;
                }
                i = (tag_end + 1).min(input.len());
                continue;
            }
            if tag.len() >= 9
                && tag[1..].starts_with(b"textarea")
                && matches!(tag.get(9), Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>'))
            {
                let close = find_close(input, tag_end, b"textarea");
                let inner = &input[tag_end + 1..close];
                out.extend_from_slice(b"<div style=\"border:1px solid #a6a6a6;padding:3px;margin:2px 0\">");
                push_escaped(&mut out, inner);
                out.extend_from_slice(b"</div>");
                form_chrome += 1;
                i = skip_close(input, close);
                continue;
            }
            if tag.len() >= 7
                && tag[1..].starts_with(b"select")
                && matches!(tag.get(7), Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>'))
            {
                let close = find_close(input, tag_end, b"select");
                let inner = &input[tag_end + 1..close];
                if let Some(text) = selected_option_text(inner) {
                    out.extend_from_slice(b"<div style=\"border:1px solid #a6a6a6;padding:3px;margin:2px 0\">");
                    push_escaped(&mut out, &text);
                    out.extend_from_slice(b"</div>");
                    form_chrome += 1;
                }
                i = skip_close(input, close);
                continue;
            }
            // <img ...>: with a parseable data-URI src, a marker div
            // reserves vertical space (padding-top = intrinsic height,
            // scaled down to the 800px content width when wider) behind a
            // sentinel background the harness swaps for the real pixels;
            // the alt text stays inside as the pre-blit paint. Without a
            // data URI, escaped alt text as before.
            if tag.len() >= 4
                && tag[1..].starts_with(b"img")
                && matches!(tag.get(4), Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>'))
            {
                let src = extract_attr(tag, b"src");
                let data_uri = src.filter(|s| s.starts_with(b"data:image/"));
                let attr_dims = match (
                    extract_attr(tag, b"width").and_then(attr_num),
                    extract_attr(tag, b"height").and_then(attr_num),
                ) {
                    (Some(w), Some(h)) if w > 0 && h > 0 => Some((w, h)),
                    _ => None,
                };
                let parsed = attr_dims.or_else(|| data_uri.and_then(|s| parse_data_image(s)));
                match (data_uri, parsed) {
                    (Some(uri), Some((iw, ih))) if iw > 0 && ih > 0 && images.len() < 4096 => {
                        let idx = images.len();
                        images.push(uri.to_vec());
                        let hoisted_now = if let Some(pic_pos) = picture_open.pop() {
                            // Replace the whole <picture>: drop its already-
                            // emitted open tag and skip to its close tag.
                            out.truncate(pic_pos);
                            pictures_hoisted += 1;
                            true
                        } else {
                            false
                        };
                        let (r, g, b) = image_sentinel(idx);
                        let width = 800u32;
                        let sh = if iw > width { ih * width / iw } else { ih };
                        let sh = sh.min(4096);
                        out.extend_from_slice(
                            format!("<div style=\"padding-top:{sh}px;background-color:rgb({r},{g},{b})\">").as_bytes(),
                        );
                        if let Some(alt) = extract_attr(tag, b"alt") {
                            push_escaped(&mut out, alt);
                        }
                        out.extend_from_slice(b"</div>");
                        if hoisted_now {
                            let close = find_close(input, i, b"picture");
                            i = skip_close(input, close);
                            continue;
                        }
                    }
                    _ => {
                        if let Some(alt) = extract_attr(tag, b"alt") {
                            push_escaped(&mut out, alt);
                        }
                    }
                }
                i = (tag_end + 1).min(input.len());
                continue;
            }
            // Tag name span (after optional '/').
            let mut k = i + 1;
            if k < tag_end && input[k] == b'/' {
                k += 1;
            }
            let name_start = k;
            while k < tag_end && input[k].is_ascii_alphabetic() {
                k += 1;
            }
            let name = &input[name_start..k];
            let bounded = k >= tag_end
                || matches!(input[k], b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/');
            let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            // <form>/<button>/<fieldset>/<legend> keep their children;
            // the tags themselves become neutral containers/phrases.
            let is_form_tag = bounded && matches!(lower.as_slice(), b"form" | b"fieldset" | b"legend")
                && !name.is_empty();
            let is_button = bounded && lower == b"button" && !name.is_empty();
            let is_table_tag = bounded && !name.is_empty() && NAMES.contains(&lower.as_slice());
            // Presentational bgcolor -> style="background-color:..." (only
            // #hex and plain color names, and only when the tag has no
            // style attribute already).
            let bg = extract_attr(tag, b"bgcolor").filter(|v| {
                !v.is_empty()
                    && v.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'#' || *b == b'%')
                    && extract_attr(tag, b"style").is_none()
            });
            if is_table_tag || is_form_tag || is_button || bg.is_some() {
                out.extend_from_slice(&input[i..name_start]);
                if is_table_tag || is_form_tag {
                    out.extend_from_slice(b"div");
                } else if is_button {
                    out.extend_from_slice(b"span");
                } else {
                    out.extend_from_slice(name);
                }
                out.extend_from_slice(&input[k..tag_end]);
                if let Some(v) = bg {
                    out.extend_from_slice(b" style=\"background-color:");
                    out.extend_from_slice(v);
                    out.extend_from_slice(b"\"");
                }
                // Table rows become flex rows: the basic painter lays
                // flex containers with all-block children out
                // left-to-right (equal split of what definite px widths
                // leave). Rows carrying their own style attribute keep
                // it and stay block stacks.
                if is_table_tag
                    && input[i + 1] != b'/'
                    && lower == b"tr"
                    && extract_attr(tag, b"style").is_none()
                {
                    out.extend_from_slice(b" style=\"display:flex\"");
                    table_rows_flexed += 1;
                }
                i = tag_end;
                continue;
            }
            // <picture> open/close tracking for the marker hoist.
            if tag.len() >= 8 && tag[1..].starts_with(b"picture") {
                picture_open.push(out.len());
            } else if tag.len() >= 9 && tag[1] == b'/' && tag[2..].starts_with(b"picture") {
                picture_open.pop();
            }
            out.extend_from_slice(tag);
            i = tag_end;
            continue;
        }
        out.push(input[i]);
        i += 1;
    }
    (out, images, pictures_hoisted, table_rows_flexed, form_chrome)
}

/// Full basic-mode intake normalization: table/presentational rewrite,
/// then CSS folds inside <style> regions (`:link`/`:visited` fold for the
/// state-free matcher; `pt` to whole `px`, 1pt = 4/3px rounded, since the
/// engine parses only px/%/rem/em), then the data-sheet rule prefilter.
/// All are BASIC-mode intake approximations.
pub fn normalize_for_basic(input: &[u8]) -> Vec<u8> {
    normalize_for_basic_stats(input).0
}

/// [`normalize_for_basic`] with the prefilter counts exposed for the
/// harness census line.
pub fn normalize_for_basic_stats(input: &[u8]) -> (Vec<u8>, PrefilterStats) {
    normalize_for_basic_full(input).0
}

/// Full intake: normalized html, prefilter counts, and the data-URI image
/// payloads (in document order) that the marker-div lane reserves space
/// for. The harness pairs these with sentinel rectangles in the raster.
pub fn normalize_for_basic_full(input: &[u8]) -> ((Vec<u8>, PrefilterStats), Vec<Vec<u8>>, usize, usize, usize) {
    let (out, images, pictures_hoisted, table_rows_flexed, form_chrome) = normalize_tables_and_presentational(input);
    let (folded, _, _) = normalize_css_in_style_tags(&out);
    let (filtered, pf) = prefilter_data_sheets_for_basic(&folded);
    ((filtered, pf), images, pictures_hoisted, table_rows_flexed, form_chrome)
}

/// Sentinel background colour for image marker div number `idx`
/// (document order). rgb(1, hi, lo) is astronomically unlikely as author
/// content and survives the engine's integer colour path.
pub fn image_sentinel(idx: usize) -> (u8, u8, u8) {
    (1, (idx >> 8) as u8, (idx & 0xff) as u8)
}

/// Counts from [`prefilter_data_sheets_for_basic`].
#[derive(Clone, Copy, Debug, Default)]
pub struct PrefilterStats {
    pub sheets_touched: usize,
    pub rules_kept: usize,
    pub rules_dropped: usize,
    /// Positioned rules carrying a fully off-canvas transform (e.g.
    /// `transform: translateX(calc(-100% - 10px))` on a fixed drawer):
    /// the engine has no transform support, so the element would paint
    /// on top of the page. The declaration is demoted to
    /// `position:static`, keeping the content in flow instead.
    pub offcanvas_static: usize,
}

/// Document key census for the prefilter: tag names (lowercased), class
/// tokens and ids exactly as written. The scan only ever ADDS keys
/// (script/style text included), so a census error biases toward keeping
/// a rule, never toward a wrong drop.
struct Census {
    tags: BTreeSet<Vec<u8>>,
    classes: BTreeSet<Vec<u8>>,
    ids: BTreeSet<Vec<u8>>,
}

fn census_of(html: &[u8]) -> Census {
    let mut c = Census { tags: BTreeSet::new(), classes: BTreeSet::new(), ids: BTreeSet::new() };
    let mut i = 0usize;
    while i < html.len() {
        if html[i] == b'<' && i + 1 < html.len() {
            if html[i + 1..].starts_with(b"!--") {
                // comment: skip to -->
                let mut j = i + 4;
                while j + 2 < html.len() && !html[j..].starts_with(b"-->") {
                    j += 1;
                }
                i = (j + 3).min(html.len());
                continue;
            }
            if html[i + 1].is_ascii_alphabetic() {
                let mut j = i + 1;
                while j < html.len() && html[j].is_ascii_alphanumeric() {
                    j += 1;
                }
                let tag: Vec<u8> = html[i + 1..j].iter().map(|b| b.to_ascii_lowercase()).collect();
                if !tag.is_empty() {
                    c.tags.insert(tag);
                }
                // attributes until '>' quote-aware
                let mut k = j;
                let mut quote: Option<u8> = None;
                while k < html.len() {
                    match (quote, html[k]) {
                        (None, b'"') => quote = Some(b'"'),
                        (None, b'\'') => quote = Some(b'\''),
                        (Some(q), ch) if ch == q => quote = None,
                        (None, b'>') => break,
                        _ => {}
                    }
                    k += 1;
                }
                let span = &html[j..k.min(html.len())];
                collect_attr_tokens(span, b"class", &mut c.classes);
                collect_attr_tokens(span, b"id", &mut c.ids);
                i = k;
            }
        }
        i += 1;
    }
    c
}

/// Collect quoted attribute values (class splits on whitespace, id whole).
fn collect_attr_tokens(span: &[u8], attr: &[u8], set: &mut BTreeSet<Vec<u8>>) {
    let n = attr.len();
    let mut i = 0usize;
    while i + n + 2 < span.len() {
        let boundary = i == 0 || matches!(span[i - 1], b' ' | b'\t' | b'\n' | b'\r' | b'/');
        if boundary
            && span[i..].starts_with(attr)
            && span[i + n] == b'='
            && matches!(span[i + n + 1], b'"' | b'\'')
        {
            let q = span[i + n + 1];
            let mut j = i + n + 2;
            while j < span.len() && span[j] != q {
                j += 1;
            }
            let value = &span[i + n + 2..j];
            if attr == b"class" {
                for tok in value.split(|b| b.is_ascii_whitespace()) {
                    if !tok.is_empty() {
                        set.insert(tok.to_vec());
                    }
                }
            } else if !value.is_empty() {
                set.insert(value.to_vec());
            }
            i = j;
        }
        i += 1;
    }
}

/// Prefilter `data:text/css;base64,` sheets embedded in the document:
/// decode, drop every rule whose comma-list selectors ALL carry a
/// rightmost key (tag/class/id) provably absent from the document census,
/// re-encode and splice back. Conservative: any selector part whose key
/// cannot be extracted cleanly keeps the whole rule. Cuts the pinned
/// match-work budget's rule list without touching the engine.
/// BASIC-mode intake approximation; drops are counted.
pub fn prefilter_data_sheets_for_basic(input: &[u8]) -> (Vec<u8>, PrefilterStats) {
    let census = census_of(input);
    let mut out = Vec::with_capacity(input.len());
    let mut stats = PrefilterStats::default();
    let mut i = 0usize;
    const MARK: &[u8] = b"data:text/css";
    while i < input.len() {
        if input[i] == b'd'
            && input[i..].len() >= MARK.len()
            && input[i..i + MARK.len()].eq_ignore_ascii_case(MARK)
        {
            // header until ',' (bounded: no quote allowed inside)
            let mut j = i + MARK.len();
            while j < input.len() && input[j] != b',' && input[j] != b'"' && input[j] != b'\'' {
                j += 1;
            }
            if j < input.len()
                && input[j] == b','
                && input[i + MARK.len()..j]
                    .windows(6)
                    .any(|w| w.eq_ignore_ascii_case(b"base64"))
            {
                let mut k = j + 1;
                while k < input.len()
                    && matches!(input[k], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=')
                {
                    k += 1;
                }
                let payload = &input[j + 1..k];
                if let Some(css) = b64_decode(payload) {
                    let (filtered, kept, dropped, off) = prefilter_css(&css, &census);
                    stats.rules_kept += kept;
                    stats.rules_dropped += dropped;
                    stats.offcanvas_static += off;
                    if dropped > 0 || off > 0 {
                        stats.sheets_touched += 1;
                        out.extend_from_slice(&input[i..j + 1]);
                        out.extend_from_slice(&b64_encode(&filtered));
                        i = k;
                        continue;
                    }
                }
            }
        }
        out.push(input[i]);
        i += 1;
    }
    (out, stats)
}

/// Filter one stylesheet: drop provably unmatchable plain rules, recurse
/// into @media/@supports/@layer, copy everything else verbatim
/// (comments, @keyframes, @font-face, strings all preserved).
fn prefilter_css(css: &[u8], census: &Census) -> (Vec<u8>, usize, usize, usize) {
    let mut out = Vec::with_capacity(css.len());
    let (mut kept, mut dropped, mut offcanvas) = (0usize, 0usize, 0usize);
    let mut i = 0usize;
    while i < css.len() {
        // comments and strings pass through
        if css[i..].starts_with(b"/*") {
            let mut j = i + 2;
            while j + 1 < css.len() && !css[j..].starts_with(b"*/") {
                j += 1;
            }
            let end = (j + 2).min(css.len());
            out.extend_from_slice(&css[i..end]);
            i = end;
            continue;
        }
        if css[i] == b'"' || css[i] == b'\'' {
            let q = css[i];
            let mut j = i + 1;
            while j < css.len() && css[j] != q && css[j] != b'\n' {
                if css[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + 1).min(css.len());
            out.extend_from_slice(&css[i..end]);
            i = end;
            continue;
        }
        // read a prelude to the next top-level '{' (tracking strings/comments/parens)
        let mut j = i;
        let mut parens = 0i32;
        let mut found = false;
        while j < css.len() {
            if css[j..].starts_with(b"/*") {
                let mut m = j + 2;
                while m + 1 < css.len() && !css[m..].starts_with(b"*/") {
                    m += 1;
                }
                j = (m + 2).min(css.len());
                continue;
            }
            match css[j] {
                b'"' | b'\'' => {
                    let q = css[j];
                    j += 1;
                    while j < css.len() && css[j] != q && css[j] != b'\n' {
                        if css[j] == b'\\' {
                            j += 1;
                        }
                        j += 1;
                    }
                }
                b'(' => parens += 1,
                b')' => parens -= 1,
                b'{' if parens == 0 => {
                    found = true;
                    break;
                }
                b'}' if parens == 0 => {
                    // stray close: copy through it
                    j += 1;
                    break;
                }
                _ => {}
            }
            j += 1;
        }
        if !found {
            out.extend_from_slice(&css[i..]);
            break;
        }
        let prelude = &css[i..j];
        // find the block end (matching '}')
        let mut depth = 1i32;
        let mut e = j + 1;
        while e < css.len() && depth > 0 {
            if css[e..].starts_with(b"/*") {
                let mut m = e + 2;
                while m + 1 < css.len() && !css[m..].starts_with(b"*/") {
                    m += 1;
                }
                e = (m + 2).min(css.len());
                continue;
            }
            match css[e] {
                b'"' | b'\'' => {
                    let q = css[e];
                    e += 1;
                    while e < css.len() && css[e] != q && css[e] != b'\n' {
                        if css[e] == b'\\' {
                            e += 1;
                        }
                        e += 1;
                    }
                }
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            e += 1;
        }
        let block = &css[j + 1..e.saturating_sub(1).max(j + 1)];
        let head: Vec<u8> = prelude
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .collect();
        let head_trim: Vec<u8> = head.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
        let recurse = [b"@media".as_slice(), b"@supports".as_slice(), b"@layer".as_slice()]
            .iter()
            .any(|p| head_trim.starts_with(p));
        if head_trim.starts_with(b"@") && recurse {
            let (inner, k2, d2, o2) = prefilter_css(block, census);
            kept += k2;
            dropped += d2;
            offcanvas += o2;
            out.extend_from_slice(prelude);
            out.push(b'{');
            out.extend_from_slice(&inner);
            out.push(b'}');
        } else if head_trim.starts_with(b"@") {
            out.extend_from_slice(&css[i..e]);
        } else if rule_droppable(prelude, census) {
            dropped += 1;
        } else {
            kept += 1;
            if has_offcanvas_transform(block) {
                out.extend_from_slice(prelude);
                out.push(b'{');
                out.extend_from_slice(block);
                out.extend_from_slice(b";position:static}");
                offcanvas += 1;
            } else {
                out.extend_from_slice(&css[i..e]);
            }
        }
        i = e;
    }
    (out, kept, dropped, offcanvas)
}

/// True when a rule both positions itself (fixed or absolute) and
/// transforms itself fully off-canvas on one axis: a translate whose
/// first percentage argument is -100% or beyond. These are JS-revealed
/// drawers/menus; with no transform support the element would paint
/// over the page, so the caller demotes it to static flow.
fn has_offcanvas_transform(block: &[u8]) -> bool {
    let lower: Vec<u8> = block
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let positioned = lower
        .windows(b"position:fixed".len())
        .any(|w| w == b"position:fixed".as_slice())
        || lower
            .windows(b"position:absolute".len())
            .any(|w| w == b"position:absolute".as_slice());
    if !positioned {
        return false;
    }
    const PATTERNS: &[&[u8]] = &[
        b"translatex(calc(-100%",
        b"translatex(-100%",
        b"translate(calc(-100%",
        b"translate(-100%",
        b"translatey(calc(-100%",
        b"translatey(-100%",
        b"translate3d(calc(-100%",
        b"translate3d(-100%",
        b"translatex(calc(100%",
        b"translatex(100%",
        b"translate3d(calc(100%",
        b"translate3d(100%",
    ];
    PATTERNS
        .iter()
        .any(|p| lower.windows(p.len()).any(|w| w == *p))
}

/// True when every comma-part of the selector list has a rightmost key
/// provably absent from the census. Any doubt keeps the rule.
fn rule_droppable(prelude: &[u8], census: &Census) -> bool {
    // split on top-level commas (outside [] and ())
    let mut parts: Vec<&[u8]> = Vec::new();
    let mut start = 0usize;
    let (mut br, mut pa) = (0i32, 0i32);
    for (idx, &b) in prelude.iter().enumerate() {
        match b {
            b'[' => br += 1,
            b']' => br -= 1,
            b'(' => pa += 1,
            b')' => pa -= 1,
            b',' if br == 0 && pa == 0 => {
                parts.push(&prelude[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(&prelude[start..]);
    if parts.iter().any(|p| p.iter().all(|b| b.is_ascii_whitespace())) {
        return false; // an empty part: malformed, keep
    }
    parts.iter().all(|p| match rightmost_key(p) {
        Some(Key::Tag(t)) => !census.tags.contains(&t),
        Some(Key::Class(c)) => !census.classes.contains(&c),
        Some(Key::Id(i)) => !census.ids.contains(&i),
        None => false,
    })
}

enum Key {
    Tag(Vec<u8>),
    Class(Vec<u8>),
    Id(Vec<u8>),
}

/// Extract a sufficient-condition key from the rightmost compound of one
/// selector part. None means unknown (keep the rule). Pseudos with parens
/// (:not/:is/:where/:has), namespaces, escapes and combinators inside the
/// compound all yield None.
fn rightmost_key(part: &[u8]) -> Option<Key> {
    let mut part = part;
    while let Some((&b, rest)) = part.split_first() {
        if b.is_ascii_whitespace() { part = rest; } else { break; }
    }
    while let Some((&b, rest)) = part.split_last() {
        if b.is_ascii_whitespace() { part = rest; } else { break; }
    }
    if part.is_empty() {
        return None;
    }
    // rightmost compound: after the last top-level combinator
    let (mut br, mut pa) = (0i32, 0i32);
    let mut last_break = 0usize;
    let mut prev: Option<u8> = None;
    for (idx, &b) in part.iter().enumerate() {
        match b {
            b'[' => br += 1,
            b']' => br -= 1,
            b'(' => pa += 1,
            b')' => pa -= 1,
            b'>' | b'+' | b'~' if br == 0 && pa == 0 => last_break = idx + 1,
            b' ' | b'\t' | b'\n' | b'\r' if br == 0 && pa == 0 => {
                if prev.is_some_and(|p| !matches!(p, b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'+' | b'~')) {
                    last_break = idx;
                }
            }
            _ => {}
        }
        prev = Some(b);
    }
    let compound = &part[last_break..];
    // strip attribute selectors and simple pseudos; bail on parens or escapes
    let mut clean: Vec<u8> = Vec::with_capacity(compound.len());
    let mut i = 0usize;
    while i < compound.len() {
        match compound[i] {
            b'[' => {
                let mut d = 1i32;
                i += 1;
                while i < compound.len() && d > 0 {
                    match compound[i] {
                        b'[' => d += 1,
                        b']' => d -= 1,
                        _ => {}
                    }
                    i += 1;
                }
            }
            b':' => {
                i += 1;
                if i < compound.len() && compound[i] == b':' {
                    i += 1;
                }
                while i < compound.len() && (compound[i].is_ascii_alphanumeric() || compound[i] == b'-') {
                    i += 1;
                }
                if i < compound.len() && compound[i] == b'(' {
                    // Functional pseudo: its arguments impose their own
                    // requirements, but the outer compound's own key is
                    // still a sufficient condition on the element, so skip
                    // the balanced group rather than bailing out.
                    let mut d = 1i32;
                    i += 1;
                    while i < compound.len() && d > 0 {
                        match compound[i] {
                            b'(' => d += 1,
                            b')' => d -= 1,
                            b'\\' => return None,
                            _ => {}
                        }
                        i += 1;
                    }
                }
            }
            b'(' | b'\\' | b'|' => return None,
            b => {
                clean.push(b);
                i += 1;
            }
        }
    }
    // scan the cleaned compound: leading tag or *, then .class / #id chunks
    let clean: Vec<u8> = clean
        .into_iter()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if clean.is_empty() {
        return None;
    }
    let mut idx = 0usize;
    let mut tag: Option<Vec<u8>> = None;
    if clean[0] != b'.' && clean[0] != b'#' && clean[0] != b'*' {
        let s = idx;
        while idx < clean.len() && (clean[idx].is_ascii_alphanumeric() || clean[idx] == b'-') {
            idx += 1;
        }
        tag = Some(clean[s..idx].iter().map(|b| b.to_ascii_lowercase()).collect());
    } else if clean[0] == b'*' {
        idx += 1;
    }
    let mut class: Option<Vec<u8>> = None;
    let mut id: Option<Vec<u8>> = None;
    while idx < clean.len() {
        let mark = clean[idx];
        if mark != b'.' && mark != b'#' {
            return None; // unexpected residue: keep
        }
        idx += 1;
        let s = idx;
        while idx < clean.len() && (clean[idx].is_ascii_alphanumeric() || matches!(clean[idx], b'-' | b'_')) {
            idx += 1;
        }
        if s == idx {
            return None;
        }
        let v = clean[s..idx].to_vec();
        if mark == b'.' {
            class = Some(v);
        } else {
            id = Some(v);
        }
    }
    if let Some(i) = id {
        return Some(Key::Id(i));
    }
    if let Some(c) = class {
        return Some(Key::Class(c));
    }
    if let Some(t) = tag {
        if !t.is_empty() {
            return Some(Key::Tag(t));
        }
    }
    None
}

fn b64_decode(input: &[u8]) -> Option<Vec<u8>> {
    fn sextet(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut chunks = input.chunks(4);
    for chunk in &mut chunks {
        if chunk.len() < 4 {
            if chunk.iter().all(|&b| b == b'=') || chunk.is_empty() {
                break;
            }
            return None;
        }
        let pad = chunk.iter().filter(|&&b| b == b'=').count();
        if pad > 2 || (pad > 0 && !chunk[4 - pad..].iter().all(|&b| b == b'=')) {
            return None;
        }
        let mut acc: u32 = 0;
        for (pos, &b) in chunk.iter().enumerate() {
            let v = if b == b'=' { 0 } else { sextet(b)? };
            acc |= u32::from(v) << (18 - 6 * pos);
        }
        out.push((acc >> 16) as u8);
        if pad < 2 {
            out.push((acc >> 8) as u8);
        }
        if pad < 1 {
            out.push(acc as u8);
        }
    }
    Some(out)
}

fn b64_encode(input: &[u8]) -> Vec<u8> {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let acc = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHA[(acc >> 18) as usize & 63]);
        out.push(ALPHA[(acc >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 { ALPHA[(acc >> 6) as usize & 63] } else { b'=' });
        out.push(if chunk.len() > 2 { ALPHA[acc as usize & 63] } else { b'=' });
    }
    out
}

/// Intake CSS normalization inside <style>…</style> regions: folds
/// `:link`/`:visited` away (state-free matcher) and rewrites `pt` lengths
/// to whole `px` (1pt = 4/3px, rounded to nearest). Returns the rewritten
/// bytes and (state-pseudo folds, pt folds).
pub fn normalize_css_in_style_tags(input: &[u8]) -> (Vec<u8>, usize, usize) {
    let mut out = Vec::with_capacity(input.len());
    let mut pseudo_folds = 0usize;
    let mut pt_folds = 0usize;
    let mut i = 0usize;
    let mut in_style = false;
    while i < input.len() {
        if !in_style && input[i] == b'<' && input[i + 1..].starts_with(b"style") {
            in_style = true;
        } else if in_style && input[i] == b'<' && input[i + 1..].starts_with(b"/style") {
            in_style = false;
        }
        if in_style {
            if input[i] == b':' && input[i + 1..].starts_with(b"link") {
                pseudo_folds += 1;
                i += 5;
                continue;
            }
            if input[i] == b':' && input[i + 1..].starts_with(b"visited") {
                pseudo_folds += 1;
                i += 8;
                continue;
            }
            if let Some((px, len)) = pt_length(&input[i..]) {
                out.extend_from_slice(px.as_bytes());
                pt_folds += 1;
                i += len;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    (out, pseudo_folds, pt_folds)
}

/// Same intake CSS normalization for a whole host-supplied stylesheet.
pub fn normalize_css_for_basic(input: &[u8]) -> (Vec<u8>, usize, usize) {
    let mut out = Vec::with_capacity(input.len());
    let mut pseudo_folds = 0usize;
    let mut pt_folds = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        if input[i] == b':' && input[i + 1..].starts_with(b"link") {
            pseudo_folds += 1;
            i += 5;
            continue;
        }
        if input[i] == b':' && input[i + 1..].starts_with(b"visited") {
            pseudo_folds += 1;
            i += 8;
            continue;
        }
        if let Some((px, len)) = pt_length(&input[i..]) {
            out.extend_from_slice(px.as_bytes());
            pt_folds += 1;
            i += len;
            continue;
        }
        out.push(input[i]);
        i += 1;
    }
    (out, pseudo_folds, pt_folds)
}

/// At a digit run immediately followed by `pt` (not `ptx` etc.), returns
/// the whole-pixel `px` string (1pt = 4/3px, rounded) and the byte length
/// consumed. Decimal points are left alone (a fractional pt folds only
/// its integer part, which would be wrong — refuse instead).
fn pt_length(input: &[u8]) -> Option<(String, usize)> {
    let mut n = 0usize;
    while n < input.len() && input[n].is_ascii_digit() {
        n += 1;
    }
    if n == 0 || !input[n..].starts_with(b"pt") {
        return None;
    }
    let next = input.get(n + 2);
    if next.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'.') {
        return None;
    }
    let value: u64 = std::str::from_utf8(&input[..n]).ok()?.parse().ok()?;
    let px = (value * 4 + 1) / 3;
    Some((format!("{px}px"), n + 2))
}

fn push_escaped(out: &mut Vec<u8>, text: &[u8]) {
    for &b in text {
        match b {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            _ => out.push(b),
        }
    }
}

/// Intrinsic (width, height) of a data-URI image, from the first decoded
/// bytes. PNG (IHDR), GIF (header), BMP and JPEG (SOF scan in the first
/// 64KB of payload). Anything else is None (caller falls back to alt).
fn parse_data_image(uri: &[u8]) -> Option<(u32, u32)> {
    let comma = uri.iter().position(|&b| b == b',')?;
    let head = &uri[..comma];
    if !head.windows(6).any(|w| w.eq_ignore_ascii_case(b"base64")) {
        return None;
    }
    let payload = &uri[comma + 1..];
    let take = payload.len().min(1 << 16);
    let raw = b64_decode(&payload[..take])?;
    if raw.len() >= 26 && raw.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([raw[16], raw[17], raw[18], raw[19]]);
        let h = u32::from_be_bytes([raw[20], raw[21], raw[22], raw[23]]);
        return Some((w, h));
    }
    if raw.len() >= 10 && raw.starts_with(b"GIF8") {
        let w = u32::from(u16::from_le_bytes([raw[6], raw[7]]));
        let h = u32::from(u16::from_le_bytes([raw[8], raw[9]]));
        return Some((w, h));
    }
    if raw.len() >= 26 && raw.starts_with(b"BM") {
        let w = u32::from_le_bytes([raw[18], raw[19], raw[20], raw[21]]);
        let h = u32::from_le_bytes([raw[22], raw[23], raw[24], raw[25]]) & 0x7fff_ffff;
        return Some((w, h));
    }
    // SVG: decoded text; intrinsic size from width/height attributes,
    // else the viewBox. Wiki icons ship as base64 SVG.
    if raw.starts_with(b"<svg") || raw.starts_with(b"<?xml") || raw.starts_with(b"<SVG") {
        if let Some(at) = raw.windows(4).position(|w| w.eq_ignore_ascii_case(b"<svg")) {
            let tag_end = raw[at..]
                .iter()
                .position(|c| *c == b'>')
                .map(|e| at + e)
                .unwrap_or(raw.len());
            let tag = &raw[at..tag_end];
            let w = extract_attr(tag, b"width").and_then(attr_num);
            let h = extract_attr(tag, b"height").and_then(attr_num);
            if let (Some(w), Some(h)) = (w, h) {
                if w > 0 && h > 0 {
                    return Some((w, h));
                }
            }
            if let Some(vb) = extract_attr(tag, b"viewBox") {
                let nums: Vec<u32> = vb
                    .split(|c: &u8| !c.is_ascii_digit())
                    .filter(|t| !t.is_empty())
                    .filter_map(|t| std::str::from_utf8(t).ok()?.parse::<u32>().ok())
                    .collect();
                if nums.len() >= 4 && nums[2] > 0 && nums[3] > 0 {
                    return Some((nums[2], nums[3]));
                }
            }
        }
        return None;
    }
    // WebP RIFF: VP8 (lossy) / VP8L (lossless) / VP8X (extended canvas).
    if raw.len() >= 30 && &raw[..4] == b"RIFF" && &raw[8..12] == b"WEBP" {
        let kind = &raw[12..16];
        if kind == b"VP8X" {
            let w = 1 + u32::from_le_bytes([raw[24], raw[25], raw[26], 0]);
            let h = 1 + u32::from_le_bytes([raw[27], raw[28], raw[29], 0]);
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        } else if kind == b"VP8 " {
            if raw[23] == 0x9D && raw[24] == 0x01 && raw[25] == 0x2A {
                let w = (u16::from_le_bytes([raw[26], raw[27]]) & 0x3FFF) as u32;
                let h = (u16::from_le_bytes([raw[28], raw[29]]) & 0x3FFF) as u32;
                if w > 0 && h > 0 {
                    return Some((w, h));
                }
            }
        } else if kind == b"VP8L" && raw.len() >= 25 && raw[20] == 0x2F {
            let bits = u32::from_le_bytes([raw[21], raw[22], raw[23], raw[24]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        }
        return None;
    }
    if raw.len() >= 4 && raw[0] == 0xff && raw[1] == 0xd8 {
        let mut i = 2usize;
        while i + 9 < raw.len() {
            if raw[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = raw[i + 1];
            if (0xc0..=0xc3).contains(&marker) || (0xc5..=0xc7).contains(&marker) {
                let h = u32::from(u16::from_be_bytes([raw[i + 5], raw[i + 6]]));
                let w = u32::from(u16::from_be_bytes([raw[i + 7], raw[i + 8]]));
                return Some((w, h));
            }
            let seg = u16::from_be_bytes([raw[i + 2], raw[i + 3]]) as usize;
            if seg < 2 {
                return None;
            }
            i += 2 + seg;
        }
    }
    None
}

/// Byte offset of the `</name` close tag at or after `from`, or the
/// input length when unclosed.
fn find_close(input: &[u8], from: usize, name: &[u8]) -> usize {
    let mut i = from;
    while i + name.len() + 2 < input.len() {
        if input[i] == b'<'
            && input[i + 1] == b'/'
            && input[i + 2..].starts_with(name)
            && matches!(input.get(i + 2 + name.len()), Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/'))
        {
            return i;
        }
        i += 1;
    }
    input.len()
}

/// First index past the close tag starting at `close` (or input length).
fn skip_close(input: &[u8], close: usize) -> usize {
    if close >= input.len() {
        return input.len();
    }
    let mut i = close;
    while i < input.len() && input[i] != b'>' {
        i += 1;
    }
    (i + 1).min(input.len())
}

/// Text of the selected (else first) <option> in a <select> body's
/// bytes. Option text runs to the next <option/<optgroup/</select.
fn selected_option_text(inner: &[u8]) -> Option<Vec<u8>> {
    let mut options: Vec<(bool, Vec<u8>)> = Vec::new();
    let mut i = 0usize;
    while i < inner.len() {
        if inner[i] == b'<'
            && inner[i + 1..].starts_with(b"option")
            && matches!(inner.get(i + 7), Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/'))
        {
            let mut j = i + 1;
            let mut quote: Option<u8> = None;
            while j < inner.len() {
                match (quote, inner[j]) {
                    (None, b'"') => quote = Some(b'"'),
                    (None, b'\'') => quote = Some(b'\''),
                    (Some(q), c) if c == q => quote = None,
                    (None, b'>') => break,
                    _ => {}
                }
                j += 1;
            }
            let tag = &inner[i..j.min(inner.len())];
            let selected = extract_attr(tag, b"selected").is_some();
            // text until next <option, <optgroup or end
            let mut k = j + 1;
            while k < inner.len() {
                if inner[k] == b'<'
                    && (inner[k + 1..].starts_with(b"option")
                        || inner[k + 1..].starts_with(b"optgroup")
                        || (inner[k + 1..].starts_with(b"/")
                            && inner[k + 2..].starts_with(b"option")))
                {
                    break;
                }
                k += 1;
            }
            let mut text: Vec<u8> = inner[j + 1..k.min(inner.len())].to_vec();
            while let Some((&b, rest)) = text.split_first() {
                if b.is_ascii_whitespace() { text = rest.to_vec(); } else { break; }
            }
            while let Some((&b, rest)) = text.split_last() {
                if b.is_ascii_whitespace() { text = rest.to_vec(); } else { break; }
            }
            options.push((selected, text));
            i = k;
            continue;
        }
        i += 1;
    }
    options
        .iter()
        .find(|(s, _)| *s)
        .or_else(|| options.first())
        .map(|(_, t)| t.clone())
        .filter(|t| !t.is_empty())
}

/// Raw value of the first occurrence of an attribute in a tag's bytes,
/// if present and non-empty. Case-sensitive on the lowercase attribute
/// name; callers lowercase the tag first if they need insensitivity.
/// Parse a numeric attribute value ("120", "120px") as pixels.
fn attr_num(v: &[u8]) -> Option<u32> {
    let end = v
        .iter()
        .position(|c| !c.is_ascii_digit())
        .unwrap_or(v.len());
    if end == 0 {
        return None;
    }
    std::str::from_utf8(&v[..end]).ok()?.parse().ok()
}

/// Decode the small HTML entity set that matters inside attribute
/// values before the text is re-escaped for emission. Numeric decimal
/// references in the ASCII range are handled too; anything unknown is
/// left as-is.
fn decode_attr_entities(v: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len());
    let mut i = 0usize;
    while i < v.len() {
        if v[i] == b'&' {
            if let Some(semi) = v[i..].iter().position(|&c| c == b';').map(|p| i + p) {
                if semi - i <= 10 {
                    let ent = &v[i + 1..semi];
                    let decoded: Option<u8> = match ent {
                        b"amp" => Some(b'&'),
                        b"lt" => Some(b'<'),
                        b"gt" => Some(b'>'),
                        b"quot" => Some(b'"'),
                        b"apos" => Some(b'\''),
                        b"nbsp" => Some(b' '),
                        _ if ent.first() == Some(&b'#') => {
                            std::str::from_utf8(&ent[1..]).ok().and_then(|d| d.parse::<u32>().ok()).and_then(|n| u8::try_from(n).ok()).filter(|&n| n >= 0x20)
                        }
                        _ => None,
                    };
                    if let Some(c) = decoded {
                        out.push(c);
                        i = semi + 1;
                        continue;
                    }
                }
            }
        }
        out.push(v[i]);
        i += 1;
    }
    out
}

fn extract_attr<'a>(tag: &'a [u8], attr: &[u8]) -> Option<&'a [u8]> {
    let n = attr.len();
    let mut i = 0usize;
    while i + n + 1 < tag.len() {
        if matches!(tag[i], b' ' | b'\t' | b'\n' | b'\r')
            && tag[i + 1..].starts_with(attr)
            && tag.get(i + 1 + n) == Some(&b'=')
        {
            let mut j = i + 2 + n;
            if j >= tag.len() {
                return None;
            }
            let q = tag[j];
            if q != b'"' && q != b'\'' {
                i += 1;
                continue;
            }
            j += 1;
            let val_start = j;
            while j < tag.len() && tag[j] != q {
                j += 1;
            }
            let v = &tag[val_start..j];
            if !v.is_empty() {
                return Some(v);
            }
            return None;
        }
        i += 1;
    }
    None
}

/// Paint a best-effort partial page. The returned stats MUST travel with any
/// image produced from the list: a nonzero skip census or `output_truncated`
/// marks the render partial.
///
/// # Errors
/// Fails only on invalid viewport or an unpaintable root (no boxes); partial
/// results always return Ok with their census.
/// BASIC page paint with generic font families: sans-serif and monospace
/// blocks use the supplied faces, everything else uses `serif`. Inline
/// family mixing flattens to the block family (counted approximation).
pub fn paint_basic_page_families<F: FontProvider>(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    serif: &F,
    sans: &F,
    mono: &F,
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
        font: serif,
        font_sans: Some(sans),
        font_mono: Some(mono),
        widths,
        stats: PartialStats::default(),
        visits: 0,
        viewport_w: i64::from(viewport_width),
    };
    painter.stats.intake_truncated = tree.truncated;
    let mut list: Vec<Command> = Vec::new();
    let root = tree.roots[0];
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
    }
    let content_h = painter.layout_block(root, 0, 0, viewport_width, &mut list, (0, 0));
    let h = if content_h > 0 { content_h } else { i64::from(viewport_height) };
    Ok((list, painter.stats, h))
}

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
        font_sans: None,
        font_mono: None,
        widths,
        stats: PartialStats::default(),
        visits: 0,
        viewport_w: i64::from(viewport_width),
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
        cursor += painter.layout_block(top_index, 0, cursor, root_width, &mut list, (0, 0));
    }
    Ok((list, painter.stats, cursor))
}
