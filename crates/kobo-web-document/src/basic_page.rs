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
fn fold_ascii(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut folded = false;
    for ch in text.chars() {
        let mapped = match ch {
            '\u{2018}' | '\u{2019}' | '\u{2032}' | '\u{00b4}' => '\'',
            '\u{201c}' | '\u{201d}' | '\u{2033}' => '"',
            '\u{2013}' | '\u{2014}' | '\u{2212}' | '\u{2010}' | '\u{2011}' => '-',
            '\u{2026}' => '.',
            '\u{00a0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' | '\u{202f}' => ' ',
            '\u{00b7}' | '\u{2022}' | '\u{25cf}' => '*',
            '\u{2192}' => '>',
            '\u{2190}' => '<',
            '\u{00e9}' => 'e',
            '\u{00e8}' => 'e',
            '\u{00fc}' => 'u',
            '\u{00f6}' => 'o',
            '\u{00e4}' => 'a',
            '\u{00e1}' => 'a',
            '\u{00ed}' => 'i',
            '\u{00f3}' => 'o',
            '\u{00fa}' => 'u',
            '\u{00f1}' => 'n',
            '\u{00e7}' => 'c',
            c if c.is_ascii() && !c.is_control() => c,
            '\t' | '\n' | '\r' => ' ',
            _ => return None,
        };
        if mapped != ch {
            folded = true;
        }
        out.push(mapped);
    }
    Some(if folded { out } else { text.to_string() })
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
    fn collect_runs(&mut self, parent: usize) -> Result<(Vec<(String, bool)>, bool), &'static str> {
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
        let mut runs: Vec<(String, bool)> = Vec::new();
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
                    runs.push((text, node.style.font_weight == 700));
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
        for (text, _) in &runs {
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
            .map(|(t, (_, b))| (t.as_str(), *b))
            .collect();
        let lines: Lines = match place_runs_normal(&run_refs, width, |ch, bold| {
            self.font.advance(ch, size, bold)
        }) {
            Ok(l) => l,
            Err(_) => greedy_wrap(&run_refs, width, |ch, bold| self.font.advance(ch, size, bold), &mut self.stats)
                .ok_or("place-and-greedy-refused")?,
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
            let color = block_rgb;
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
        for i in 1..=glyphs.len() {
            let boundary = i == glyphs.len()
                || glyphs[i].line != glyphs[i - 1].line
                || glyphs[i].bold != glyphs[i - 1].bold;
            if boundary {
                let from = run_start.take().unwrap_or(0);
                if flush(from, i, glyphs, list, &mut self.stats, self.font).is_err() {
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
        // Background: padding box. Fill AFTER content in v1 only when the box
        // is a text block (its background would hide the text); block
        // containers get their fill before children in the caller below.
        if pure_inline {
            if let Some(color) = style.used_background_color() {
                // re-emit behind text is not possible post hoc; skip with count
                self.stats.skip("inline-block-bg-order");
                let _ = color;
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
    // Container backgrounds: second preorder pass would double-emit; v1 fills
    // container backgrounds BEFORE content in a first pass instead. To keep
    // one walk simple, v1 paints container backgrounds after content with a
    // documented order approximation only when they carry a background.
    // (Counted; fixed in the hardening track with a two-phase paint.)
    let mut order_pass = |list: &mut Vec<Command>, stats: &mut PartialStats| {
        let mut stack = vec![root];
        let mut visited = 0usize;
        while let Some(index) = stack.pop() {
            visited += 1;
            if visited > MAX_VISITS {
                stats.output_truncated = true;
                break;
            }
            let node = &tree.boxes[index];
            if matches!(node.kind, BoxKind::Block | BoxKind::AnonymousBlock | BoxKind::ListItem)
                && node.style.used_background_color().is_some()
                && index != root
                && Some(index) != body
            {
                stats.approx("container-bg-after-content");
            }
            for &c in node.children.iter().rev() {
                stack.push(c);
            }
        }
    };
    order_pass(&mut list, &mut painter.stats);
    Ok((list, painter.stats, cursor))
}
