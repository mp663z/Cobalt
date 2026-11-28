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
//! - Page view uses a source-preserving scalar line breaker; complex-script
//!   shaping and bidi are not implemented. Missing glyphs draw placeholders.
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

#[path = "basic_table.rs"]
mod table;

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
    table_stack_depth: usize,
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

    /// Margin resolution that knows the owning element: em margins
    /// resolve against its computed font size, but only on an exact
    /// integer pixel product (the `Margin::Em` contract); anything else
    /// stays zeroed and counted. Auto margins are the caller's problem
    /// (they need the used width), so they stay zeroed here.
    fn margin_px_el(&mut self, el: usize, margin: Margin, containing: u32) -> i64 {
        if let Margin::Em(hundredths) = margin {
            let fs = i64::from(self.font_size(el));
            let product = i64::from(hundredths) * fs;
            if product % 100 == 0 {
                return product / 100;
            }
        }
        self.margin_px(margin, containing)
    }

    /// Horizontal geometry of a normal-flow block child per the engine's
    /// restricted CSS 2.2 §10.3.3 resolver, with the one extension the
    /// resolver refuses: a `max-width` cap, applied by re-running with
    /// the cap as the used width so auto margins split the leftover
    /// (`.mw7.center` and friends). Returns (margin_left, layout_width,
    /// margin_right) where layout_width is the border-box width the
    /// painter expects (content + padding + border).
    fn block_child_hgeom(&mut self, child: usize, containing: u32) -> (i64, u32, i64) {
        let cstyle = self.tree.boxes[child].style;
        let border = cstyle.decor.used_border();
        let pad_or_zero = |v: u32| -> u32 {
            if v == crate::computed_style::PADDING_UNREPRESENTABLE {
                0
            } else {
                v
            }
        };
        let left_edge = pad_or_zero(cstyle.padding_left).saturating_add(border[3]);
        let right_edge = pad_or_zero(cstyle.padding_right).saturating_add(border[1]);
        let mut margin = |s: &mut Self, m: Margin| -> Option<i64> {
            match m {
                Margin::Auto => None,
                other => Some(s.margin_px_el(child, other, containing)),
            }
        };
        let ml = margin(self, cstyle.margin_left);
        let mr = margin(self, cstyle.margin_right);
        let (geom, capped) = capped_block_width(
            cstyle.width,
            cstyle.max_width,
            containing,
            left_edge,
            right_edge,
            ml,
            mr,
        );
        if capped {
            self.stats.approx("max-width-capped");
        }
        if matches!(cstyle.margin_left, Margin::Auto) || matches!(cstyle.margin_right, Margin::Auto)
        {
            if geom.margin_left != 0 || geom.margin_right != 0 {
                self.stats.approx("margin-auto-resolved");
            }
        }
        (
            geom.margin_left,
            geom.content + left_edge + right_edge,
            geom.margin_right,
        )
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
    fn collect_runs_of(
        &mut self,
        children: &[usize],
        block_color: u32,
    ) -> Result<(Vec<crate::basic_text_paint::Run>, bool), &'static str> {
        if children.is_empty() {
            return Err("empty");
        }
        let mut mixed_color = false;
        let mut runs: Vec<crate::basic_text_paint::Run> = Vec::new();
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
                    let mut underline = node.style.text_decoration
                        == crate::computed_style::TextDecoration::Underline;
                    let mut scopes=Vec::new();
                    let mut parent = node.parent;
                    while let Some(p) = parent {
                        let style=self.tree.boxes[p].style;
                        if style.unicode_bidi != crate::computed_style::UnicodeBidi::Normal { scopes.push(crate::basic_bidi::Scope { id:p, mode:style.unicode_bidi, direction:style.direction }); }
                        underline |= self.tree.boxes[p].style.text_decoration
                            == crate::computed_style::TextDecoration::Underline;
                        parent = self.tree.boxes[p].parent;
                    }
                    scopes.reverse();
                    runs.push((
                        text,
                        node.style.font_weight >= 600,
                        node.style.color,
                        run_size,
                        node.style.font_style != crate::computed_style::FontStyle::Normal,
                        underline,
                        node.source,
                        scopes,
                    ));
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
    /// Unwrapped width of a pure-inline subtree's text at its resolved
    /// font size, or None when it has no measurable text. Used by the
    /// flex-row approximation to size link columns by content.
    fn natural_inline_width(&mut self, index: usize) -> Option<i64> {
        let face = self.face_for(index);
        let mut size = self.tree.boxes[index].style.font_size;
        if size == 0 {
            size = self.font_size(index);
        }
        if size == 0 {
            return None;
        }
        let color = self.tree.boxes[index].style.color;
        let (runs, _) = self
            .collect_runs_of(&self.tree.boxes[index].children, color)
            .ok()?;
        let mut total = 0i64;
        for (text, bold, _, rsize, _, _, _, _) in &runs {
            let sz = if *rsize == 0 { size } else { *rsize };
            // Estimate only: non-folding text (wiki LRM marks, emoji)
            // and glyphs without an advance charge half an em per char
            // instead of failing the whole measurement.
            for ch in text.chars() {
                total += i64::from(face.advance(ch, sz, *bold).unwrap_or((sz / 2).max(1)));
            }
        }
        if total == 0 {
            None
        } else {
            Some(total + 8)
        }
    }

    /// Deep single-line advance estimate of in-flow text descendants,
    /// used to size nested flex children whose structure is not pure
    /// inline. Absolutely/fixed-positioned subtrees (hover dropdowns)
    /// take no inline space and are skipped. Non-folding text and
    /// missing advances charge half an em.
    fn estimate_text_width(&mut self, index: usize, inherited_size: u32) -> i64 {
        let (kind, size, text, children) = {
            let b = &self.tree.boxes[index];
            if matches!(
                b.style.positioned.position,
                crate::computed_style::Position::Absolute | crate::computed_style::Position::Fixed
            ) {
                return 0;
            }
            let size = if b.style.font_size != 0 {
                b.style.font_size
            } else {
                inherited_size
            };
            (b.kind, size, b.text.clone(), b.children.clone())
        };
        if matches!(kind, BoxKind::Text) {
            let Some(text) = text else { return 0 };
            let size = if size == 0 { 16 } else { size };
            let face = self.face_for(index);
            return text
                .chars()
                .map(|ch| i64::from(face.advance(ch, size, false).unwrap_or((size / 2).max(1))))
                .sum();
        }
        let mut total = 0i64;
        for c in children {
            total += self.estimate_text_width(c, size);
        }
        total
    }

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
        let size = self.font_size(index).clamp(1, 256);
        let mut ancestor = Some(index);
        let mut background = 0xffffff;
        while let Some(i) = ancestor {
            if let Some(color) = self.tree.boxes[i].style.used_background_color() {
                background = color;
                break;
            }
            ancestor = self.tree.boxes[i].parent;
        }
        let style = crate::basic_text_paint::Style {
            size,
            line_height: node.style.used_line_height(size + 4).unwrap_or(size + 4),
            align: node.style.text_align,
            direction: node.style.direction,
            pre: node.style.white_space_pre,
            background,
        };
        let (mut runs, _) = self.collect_runs_of(children, node.style.color)?;
        if self.tree.boxes[index].kind == BoxKind::ListItem
            && !self.tree.boxes[index].style.list_style_none
        {
            if let Some(first) = runs.first_mut() {
                first.0.insert_str(
                    0,
                    self.tree
                        .page_list_markers
                        .get(&index)
                        .map_or("• ", String::as_str),
                );
            }
        }
        let (commands, height, missing) =
            crate::basic_text_paint::paint(&runs, face, &style, (content_x, top), width)?;
        if missing > 0 {
            *self
                .stats
                .approximations
                .entry("missing-glyph-placeholder")
                .or_insert(0) += missing;
        }
        self.stats.glyph_runs += commands.len();
        self.stats.text_blocks_painted += usize::from(!commands.is_empty());
        list.extend(commands);
        Ok(height)
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
        let node_kind = self.tree.boxes[index].kind;
        if node_kind == BoxKind::Table {
            let entry = list.len();
            let before = self.stats.clone();
            return match self.layout_table(index, content_x, top, width, list, abs_origin) {
                Ok(height) => height,
                Err(reason) => {
                    list.truncate(entry);
                    self.stats = before;
                    self.stats.approx("table-stacked-fallback");
                    self.stats.approx(reason);
                    self.table_stack_depth += 1;
                    let height =
                        self.layout_block_contents(index, content_x, top, width, list, abs_origin);
                    self.table_stack_depth -= 1;
                    height
                }
            };
        }
        self.layout_block_contents(index, content_x, top, width, list, abs_origin)
    }

    fn layout_block_contents(
        &mut self,
        index: usize,
        content_x: i64,
        top: i64,
        width: u32,
        list: &mut Vec<Command>,
        abs_origin: (i64, i64),
    ) -> i64 {
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
            // Whitespace-only text children of a flex container wrap into
            // anonymous flex items that are not rendered (CSS Flexbox 1
            // sec 4), so they neither disqualify the row nor take a slot.
            let ws_text = |c: usize| -> bool {
                let b = &self.tree.boxes[c];
                matches!(b.kind, BoxKind::Text)
                    && b.text.as_deref().is_some_and(|t| t.trim().is_empty())
            };
            // Pure-inline inline children (a nav-logo link, an empty
            // wrapper span) blockify as flex items in a real engine, so
            // they are row items measured by content below, not a reason
            // to stack. A bare non-whitespace Text child would need an
            // anonymous item this walker cannot paint, so it still stacks.
            let row_eligible = !kids.is_empty()
                && !self.deep_pure_inline(kids)
                && kids.iter().all(|&c| {
                    let ck = self.tree.boxes[c].kind;
                    !matches!(ck, BoxKind::Text) || ws_text(c)
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
                    if matches!(b.kind, BoxKind::Text)
                        && b.text.as_deref().is_some_and(|t| !t.trim().is_empty())
                    {
                        return true;
                    }
                    stack.extend_from_slice(&b.children);
                }
                false
            };
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
            // A nested flex child is a real row item, not a reason to
            // stack: its measured content width is its basis (CSS flex
            // basis auto), a growing child (flex:auto / flex-grow) takes
            // a share of what is left, and a shrinkable child wider than
            // the container shrinks into its share and runs its own row
            // admission at that width. Only an UNSHRINKABLE nested row
            // wider than the container still stacks, because painting it
            // would spill past the border box. Never-painted sub items
            // (whitespace anonymous items, empty wrappers) and out-of-flow
            // sub items (the search icon's absolute svg) take no row
            // space, so they stay out of the measurement.
            let mut refuse_nested = false;
            let mut nested_basis: Vec<(usize, i64)> = Vec::new();
            if row_eligible {
                for &c in kids {
                    if !matches!(self.tree.boxes[c].kind, BoxKind::Flex) {
                        continue;
                    }
                    let sub: Vec<usize> = self.tree.boxes[c].children.clone();
                    let mut sub_total = 0i64;
                    let mut measurable = true;
                    for &k in &sub {
                        // Whitespace anonymous items are not rendered;
                        // the parent's own border/background does not
                        // make them visible (text boxes carry the
                        // element's computed style here).
                        if ws_text(k) || !visible(k) {
                            continue;
                        }
                        if matches!(
                            self.tree.boxes[k].style.positioned.position,
                            crate::computed_style::Position::Absolute
                                | crate::computed_style::Position::Fixed
                        ) {
                            continue;
                        }
                        let ks = self.tree.boxes[k].style;
                        let nw = match self.natural_inline_width(k) {
                            Some(nw) => nw,
                            None => {
                                let est = self.estimate_text_width(k, 0);
                                // Icon-only kids (hamburger, search): a
                                // small fixed estimate keeps the row.
                                if est == 0 {
                                    48
                                } else {
                                    est + 8
                                }
                            }
                        };
                        sub_total += nw
                            + pad_or_zero(ks.padding_left)
                            + pad_or_zero(ks.padding_right)
                            + self.margin_px(ks.margin_left, width)
                            + self.margin_px(ks.margin_right, width);
                    }
                    nested_basis.push((c, sub_total));
                    if !measurable
                        || (sub_total > avail && self.tree.boxes[c].style.flex_shrink == 0)
                    {
                        refuse_nested = true;
                    }
                }
            }
            if refuse_nested {
                self.stats.approx("flex-row-refused-nested-flex");
            }
            if row_eligible && !refuse_nested {
                let mut slots: Vec<i64> = Vec::with_capacity(kids.len());
                let mut fixed_total = 0i64;
                let mut flex_count = 0i64;
                for &c in kids {
                    if ws_text(c) {
                        slots.push(i64::MIN);
                        continue;
                    }
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
                            // Pure-inline children (nav links etc.):
                            // measure their natural single-line width and
                            // treat it as fixed so equal-share columns do
                            // not wrap single words ("Govern/ance").
                            let nat = if self.deep_pure_inline(&self.tree.boxes[c].children) {
                                self.natural_inline_width(c)
                            } else {
                                None
                            };
                            match nat {
                                Some(nw) => {
                                    // The child keeps its own horizontal
                                    // padding inside the slot (blog nav
                                    // li.ph4-ns = 32px/side), so the
                                    // measured text width must come on
                                    // top of it, not instead of it.
                                    let v = nw
                                        + pad_or_zero(cs.padding_left)
                                        + pad_or_zero(cs.padding_right)
                                        + ml
                                        + mr;
                                    slots.push(v);
                                    fixed_total += v;
                                    self.stats.approx("flex-row-natural-width");
                                }
                                None => {
                                    // A nested flex child keeps its
                                    // measured content width as a fixed
                                    // slot when it does not grow and that
                                    // width fits; a growing or too-wide
                                    // (but shrinkable) child takes a
                                    // share and runs its own admission at
                                    // that width below.
                                    let basis =
                                        nested_basis.iter().find(|(b, _)| *b == c).map(|(_, n)| *n);
                                    match basis {
                                        Some(b) if cs.flex_grow == 0 && b <= avail => {
                                            let v = b + ml + mr;
                                            slots.push(v);
                                            fixed_total += v;
                                            self.stats.approx("flex-row-nested-natural");
                                        }
                                        Some(_) => {
                                            slots.push(-1);
                                            flex_count += 1;
                                            self.stats.approx("flex-row-nested-share");
                                        }
                                        None => {
                                            slots.push(-1);
                                            flex_count += 1;
                                        }
                                    }
                                }
                            }
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
                // The 64px narrow-column refusal applies only to
                // equal-share columns; content-measured and fixed slots
                // are exempt (a 40px "Talk" tab is fine). A row whose
                // measured widths exceed the container would paint off
                // the right edge, so it refuses too.
                let share_narrow = flex_count > 0 && share < 64;
                // Slots lay out starting at the left padding edge, so a
                // row may reach into the container's right padding (which
                // paints nothing) but never past the border box.
                let full = (i64::from(width) - pad_or_zero(style.padding_left)).max(0);
                let overflow = fixed_total > full;
                // Measured items slightly wider than the container shrink
                // to fit (CSS Flexbox 1 sec 9.7): each shrinkable fixed
                // slot (flex-shrink > 0, the initial value) gives up space
                // in proportion to its scaled shrink factor (base size x
                // flex-shrink), never below zero. flex-shrink:0 items keep
                // their size; when even shrinking every shrinkable slot to
                // zero cannot fit, the row keeps the overflow fallback
                // below. Only attempted when no child waits on a grow
                // share: negative free space grows nothing.
                let shrunk_slots: Option<Vec<i64>> = if overflow && flex_count == 0 {
                    let weights: Vec<i64> = slots
                        .iter()
                        .enumerate()
                        .map(|(k, &v)| {
                            if v > 0 {
                                v * i64::from(self.tree.boxes[kids[k]].style.flex_shrink)
                            } else {
                                0
                            }
                        })
                        .collect();
                    let weight_sum: i64 = weights.iter().sum();
                    let shrinkable_total: i64 = slots
                        .iter()
                        .enumerate()
                        .filter(|(k, _)| weights[*k] > 0)
                        .map(|(_, &v)| v)
                        .sum();
                    let excess = fixed_total - full;
                    if weight_sum > 0 && excess <= shrinkable_total {
                        let mut next = slots.clone();
                        let mut order: Vec<usize> =
                            (0..slots.len()).filter(|&k| weights[k] > 0).collect();
                        order.sort_by(|&a, &b| weights[b].cmp(&weights[a]).then(a.cmp(&b)));
                        let mut remaining = excess;
                        for (pos, &k) in order.iter().enumerate() {
                            let cut = if pos + 1 == order.len() {
                                remaining.min(next[k])
                            } else {
                                (excess * weights[k] / weight_sum).min(next[k])
                            };
                            next[k] -= cut;
                            remaining -= cut;
                        }
                        (remaining == 0).then_some(next)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if !share_narrow && (!overflow || shrunk_slots.is_some()) {
                    flex_row = true;
                    if shrunk_slots.is_some() {
                        self.stats.approx("flex-row-shrunk");
                    }
                    flex_slots = Some((shrunk_slots.unwrap_or(slots), share));
                    self.stats.approx("flex-row-basic");
                } else {
                    // Content widths exceed the container, or nothing is
                    // left for an unmeasured child: retry with plain
                    // equal shares (the v1 behavior) so the row stays
                    // horizontal when equal shares still fit text.
                    let visible_count = slots.iter().filter(|&&v| v != i64::MIN).count() as i64;
                    let eq = if visible_count > 0 {
                        avail / visible_count
                    } else {
                        0
                    };
                    if eq >= 64 {
                        flex_row = true;
                        flex_slots = Some((
                            slots
                                .iter()
                                .map(|&v| if v == i64::MIN { i64::MIN } else { -1 })
                                .collect(),
                            eq,
                        ));
                        self.stats.approx("flex-row-equal-fallback");
                        self.stats.approx("flex-row-basic");
                    } else {
                        if overflow {
                            self.stats.approx("flex-row-refused-overflow");
                        } else {
                            self.stats.approx("flex-row-refused-narrow");
                        }
                        self.stats.approx("flex-as-block");
                    }
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
                if matches!(
                    style.positioned.position,
                    crate::computed_style::Position::Fixed
                ) {
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
            match self.paint_text_block(
                index,
                &tree.boxes[index].children,
                content_x + pad_left,
                content_top,
                (i64::from(width) - pad_left - pad_right).max(0) as u32,
                list,
            ) {
                Ok(h) => content_height = h,
                Err(reason) => self.stats.skip(reason),
            }
        } else if flex_row {
            let (slots, share) = flex_slots.take().unwrap();
            let kids = &self.tree.boxes[index].children;
            let mut x = content_x + pad_left;
            let mut row_h = 0i64;
            for (k, &c) in kids
                .iter()
                .enumerate()
                .filter(|(k, _)| slots[*k] != i64::MIN)
            {
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
                        match self.paint_text_block(
                            index,
                            &anon,
                            content_x + pad_left,
                            $cursor,
                            child_width,
                            list,
                        ) {
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
                    || (matches!(ckind, BoxKind::Inline)
                        && self.pure_inline_children(&self.tree.boxes[child].children))
                {
                    anon.push(child);
                    continue;
                }
                flush_anon!(cursor);
                let cstyle = self.tree.boxes[child].style;
                let containing = width;
                let mt = self.margin_px(cstyle.margin_top, containing);
                let mb = self.margin_px(cstyle.margin_bottom, containing);
                let (ml, cw, _mr) = if self.table_stack_depth > 0 {
                    // The fallback is an intentional block stack. Folded box
                    // edges are already included, and descendants use its
                    // actual inner width rather than the pre-grid width pass.
                    let ml = self.margin_px(cstyle.margin_left, child_width);
                    let mr = self.margin_px(cstyle.margin_right, child_width);
                    let cw = match cstyle.width {
                        Length::Px(value) => value,
                        _ => u32::try_from((i64::from(child_width) - ml - mr).max(0))
                            .unwrap_or(0),
                    };
                    (ml, cw, mr)
                } else if ckind == BoxKind::Table {
                    match cstyle.width {
                        Length::Px(value) => {
                            (self.margin_px(cstyle.margin_left, child_width), value, 0)
                        }
                        _ => self.block_child_hgeom(child, child_width),
                    }
                } else {
                    match self.widths.widths.get(child).copied().flatten() {
                        Some(w) => {
                            // Engine width pass already resolved geometry;
                            // honor its margins (auto splits included). Its
                            // content width is content-box; the painter wants
                            // the border box.
                            let b = cstyle.decor.used_border();
                            let edge_or_zero = |v: u32| -> u32 {
                                if v == crate::computed_style::PADDING_UNREPRESENTABLE {
                                    0
                                } else {
                                    v
                                }
                            };
                            (
                                w.margin_left,
                                w.content
                                    + edge_or_zero(cstyle.padding_left)
                                    + edge_or_zero(cstyle.padding_right)
                                    + b[3]
                                    + b[1],
                                w.margin_right,
                            )
                        }
                        None => self.block_child_hgeom(child, child_width),
                    }
                };
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
                let bw = if self.table_stack_depth > 0 {
                    i64::from(width)
                } else {
                    pad_left + i64::from(width) + pad_right
                };
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
                                rect: Rect {
                                    x,
                                    y,
                                    width: w,
                                    height: h,
                                },
                                color: rgb(color),
                                source: Source {
                                    node: self.tree.boxes[index].source,
                                    action: None,
                                    text_range: None,
                                },
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
                let pb_w = if self.table_stack_depth > 0 {
                    i64::from(width)
                } else {
                    pad_left + i64::from(width) + pad_right
                };
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
                    matches!(
                        bs,
                        crate::computed_style::BorderStyle::Dashed
                            | crate::computed_style::BorderStyle::Dotted
                    )
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
                        u32::try_from(w),
                        u32::try_from(h),
                    ) {
                        let insert_at = if self.table_stack_depth > 0 {
                            list.len()
                        } else {
                            bg_insert_at
                        };
                        list.insert(
                            insert_at,
                            Command::Fill {
                                rect: Rect {
                                    x,
                                    y,
                                    width: w,
                                    height: h,
                                },
                                color: rgb(col),
                                source: Source {
                                    node: self.tree.boxes[index].source,
                                    action: None,
                                    text_range: None,
                                },
                            },
                        );
                        bg_insert_at += 1;
                        self.stats.fills += 1;
                        self.stats.approx("border-side");
                    }
                };
                let (bt, br, bb, bl) = (i64::from(bt), i64::from(br), i64::from(bb), i64::from(bl));
                if self.table_stack_depth > 0 {
                    emit(content_x, top, pb_w, bt, 0);
                    emit(content_x + pb_w - br, top, br, pb_h, 1);
                    emit(content_x, top + pb_h - bb, pb_w, bb, 2);
                    emit(content_x, top, bl, pb_h, 3);
                } else {
                    emit(content_x - bl, top - bt, pb_w + bl + br, bt, 0);
                    emit(content_x + pb_w, top, br, pb_h, 1);
                    emit(content_x - bl, top + pb_h, pb_w + bl + br, bb, 2);
                    emit(content_x - bl, top, bl, pb_h, 3);
                }
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

/// BASIC intake retains HTML tables for Page-view grid layout. Unsupported
/// grids use the counted stacked fallback; presentational bgcolor and form
/// control rewriting remain intake approximations.
fn normalize_tables_and_presentational(input: &[u8]) -> (Vec<u8>, usize, usize) {
    let mut out = Vec::with_capacity(input.len());
    let table_rows_flexed = 0usize;
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
                        out.extend_from_slice(
                            b" style=\"border:1px solid #808080;padding:2px 6px;margin:2px 0\">",
                        );
                    } else {
                        out.extend_from_slice(
                            b" style=\"border:1px solid #a6a6a6;padding:3px;margin:2px 0\">",
                        );
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
                out.extend_from_slice(
                    b"<div style=\"border:1px solid #a6a6a6;padding:3px;margin:2px 0\">",
                );
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
                    out.extend_from_slice(
                        b"<div style=\"border:1px solid #a6a6a6;padding:3px;margin:2px 0\">",
                    );
                    push_escaped(&mut out, &text);
                    out.extend_from_slice(b"</div>");
                    form_chrome += 1;
                }
                i = skip_close(input, close);
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
            let bounded =
                k >= tag_end || matches!(input[k], b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/');
            let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();
            // <form>/<button>/<fieldset>/<legend> keep their children;
            // the tags themselves become neutral containers/phrases.
            let is_form_tag = bounded
                && matches!(lower.as_slice(), b"form" | b"fieldset" | b"legend")
                && !name.is_empty();
            let is_button = bounded && lower == b"button" && !name.is_empty();
            // Presentational bgcolor -> style="background-color:..." (only
            // #hex and plain color names, and only when the tag has no
            // style attribute already).
            let bg = extract_attr(tag, b"bgcolor").filter(|v| {
                !v.is_empty()
                    && v.iter()
                        .all(|b| b.is_ascii_alphanumeric() || *b == b'#' || *b == b'%')
                    && extract_attr(tag, b"style").is_none()
            });
            if is_form_tag || is_button || bg.is_some() {
                out.extend_from_slice(&input[i..name_start]);
                if is_form_tag {
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
                i = tag_end;
                continue;
            }
            out.extend_from_slice(tag);
            i = tag_end;
            continue;
        }
        out.push(input[i]);
        i += 1;
    }
    (out, table_rows_flexed, form_chrome)
}

/// Full basic-mode intake normalization: presentational/form rewrite,
/// then the data-sheet rule prefilter. Both are BASIC-mode intake
/// approximations. CSS is NO LONGER byte-folded here: `:link`/`:visited`
/// are handled by the state-aware parse entry (`:link` matches an
/// href-present hyperlink under the caller's FreshNoHistory guarantee,
/// `:visited` never matches), and `pt` lengths are parsed by the length
/// parser (1pt = 4/3px in the existing fixed-point contract).
pub fn normalize_for_basic(input: &[u8]) -> Vec<u8> {
    normalize_for_basic_stats(input).0
}

/// [`normalize_for_basic`] with the prefilter counts exposed for the
/// harness census line.
pub fn normalize_for_basic_stats(input: &[u8]) -> (Vec<u8>, PrefilterStats) {
    normalize_for_basic_full(input).0
}

/// Full intake: normalized html, prefilter counts, the reserved legacy
/// table-row counter (now zero), and the form-chrome rewrite counter. Image payloads travel with the box tree
/// now (`BoxTree::images_in_order`); the harness pairs them with the
/// sentinel rectangles in the raster.
pub fn normalize_for_basic_full(input: &[u8]) -> ((Vec<u8>, PrefilterStats), usize, usize) {
    let (out, table_rows_flexed, form_chrome) = normalize_tables_and_presentational(input);
    let (filtered, pf) = prefilter_data_sheets_for_basic(&out);
    ((filtered, pf), table_rows_flexed, form_chrome)
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
    /// on top of the page. Folded to display:none, matching the
    /// JS-unrevealed static state.
    pub offcanvas_static: usize,
    /// Rules matching the classic screen-reader-only pattern
    /// (position:absolute, 1px box, overflow:hidden). The engine does
    /// not clip overflow, so the text paints as a sliver column; folded
    /// to display:none, which matches the sighted rendering.
    pub visually_hidden_folded: usize,
    /// @media blocks whose width conditions evaluate false at the
    /// basic viewport width and were dropped whole. Unconditionally
    /// applying them was the source of mobile-chrome-over-desktop
    /// overlap defects.
    pub media_blocks_dropped: usize,
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
    let mut c = Census {
        tags: BTreeSet::new(),
        classes: BTreeSet::new(),
        ids: BTreeSet::new(),
    };
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
                let tag: Vec<u8> = html[i + 1..j]
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect();
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

/// Collect attribute values (class splits on whitespace, id whole). Attribute names match case-insensitively,
/// whitespace is allowed around `=`, and quoted and unquoted values both count: missing a key here would let the
/// prefilter drop a rule that does match, so every form HTML allows is read.
fn collect_attr_tokens(span: &[u8], attr: &[u8], set: &mut BTreeSet<Vec<u8>>) {
    let n = attr.len();
    let mut i = 0usize;
    while i + n < span.len() {
        let boundary = i == 0 || matches!(span[i - 1], b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'"' | b'\'');
        if boundary && span[i..i + n].eq_ignore_ascii_case(attr) {
            let mut j = i + n;
            while j < span.len() && span[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < span.len() && span[j] == b'=' {
                j += 1;
                while j < span.len() && span[j].is_ascii_whitespace() {
                    j += 1;
                }
                let (start, end) = match span.get(j) {
                    Some(&q) if q == b'"' || q == b'\'' => {
                        let mut e = j + 1;
                        while e < span.len() && span[e] != q {
                            e += 1;
                        }
                        (j + 1, e)
                    }
                    Some(_) => {
                        let mut e = j;
                        while e < span.len() && !span[e].is_ascii_whitespace() {
                            e += 1;
                        }
                        (j, e)
                    }
                    None => (j, j),
                };
                let value = &span[start..end];
                if attr == b"class" {
                    for tok in value.split(|b| b.is_ascii_whitespace()) {
                        if !tok.is_empty() {
                            set.insert(tok.to_vec());
                        }
                    }
                } else if !value.is_empty() {
                    set.insert(value.to_vec());
                }
                i = end.max(i + 1);
                continue;
            }
        }
        i += 1;
    }
}

/// The same static-state prefilter for one externally linked stylesheet: `html` is the (normalised) document
/// whose tag/class/id census decides which rules are provably unmatchable. Folds off-canvas drawers and
/// screen-reader-only text to `display:none` and drops false `@media` blocks, exactly as the data-sheet path does.
#[must_use]
pub fn prefilter_linked_sheet_for_basic(html: &[u8], css: &[u8]) -> (Vec<u8>, PrefilterStats) {
    let census = census_of(html);
    let (out, kept, dropped, offcanvas, vishidden, media_dropped) = prefilter_css(css, &census);
    let stats = PrefilterStats {
        sheets_touched: 1,
        rules_kept: kept,
        rules_dropped: dropped,
        offcanvas_static: offcanvas,
        visually_hidden_folded: vishidden,
        media_blocks_dropped: media_dropped,
    };
    (out, stats)
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
                    let (filtered, kept, dropped, off, vh, md) = prefilter_css(&css, &census);
                    stats.rules_kept += kept;
                    stats.rules_dropped += dropped;
                    stats.offcanvas_static += off;
                    stats.visually_hidden_folded += vh;
                    stats.media_blocks_dropped += md;
                    if dropped > 0 || off > 0 || vh > 0 || md > 0 {
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
fn prefilter_css(css: &[u8], census: &Census) -> (Vec<u8>, usize, usize, usize, usize, usize) {
    let mut out = Vec::with_capacity(css.len());
    let (mut kept, mut dropped, mut offcanvas, mut vishidden, mut media_dropped) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
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
        let head: Vec<u8> = prelude.iter().map(|b| b.to_ascii_lowercase()).collect();
        let head_trim: Vec<u8> = head
            .iter()
            .copied()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        let recurse = [
            b"@media".as_slice(),
            b"@supports".as_slice(),
            b"@layer".as_slice(),
        ]
        .iter()
        .any(|p| head_trim.starts_with(p));
        if head_trim.starts_with(b"@media") {
            match eval_media_query_list(prelude) {
                Some(false) => {
                    // The width conditions can never hold at the basic
                    // viewport: drop the whole block instead of letting
                    // its rules apply unconditionally.
                    media_dropped += 1;
                }
                Some(true) | None => {
                    // Holds at the viewport, or not evaluable (unknown
                    // features, `not`, range syntax): keep the block and
                    // recurse so inner rules still get filtered.
                    let (inner, k2, d2, o2, v2, m2) = prefilter_css(block, census);
                    kept += k2;
                    dropped += d2;
                    offcanvas += o2;
                    vishidden += v2;
                    media_dropped += m2;
                    out.extend_from_slice(prelude);
                    out.push(b'{');
                    out.extend_from_slice(&inner);
                    out.push(b'}');
                }
            }
        } else if head_trim.starts_with(b"@") && recurse {
            let (inner, k2, d2, o2, v2, m2) = prefilter_css(block, census);
            kept += k2;
            dropped += d2;
            offcanvas += o2;
            vishidden += v2;
            media_dropped += m2;
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
                // JS-revealed drawers are hidden in the page's static
                // initial state at any viewport width, so display:none
                // matches the sighted rendering (in-flow demotion
                // dumped the docs drawer over the page top).
                out.extend_from_slice(prelude);
                out.push(b'{');
                out.extend_from_slice(block);
                out.extend_from_slice(b";display:none}");
                offcanvas += 1;
            } else if has_visually_hidden_pattern(block) {
                out.extend_from_slice(prelude);
                out.push(b'{');
                out.extend_from_slice(block);
                out.extend_from_slice(b";display:none}");
                vishidden += 1;
            } else {
                out.extend_from_slice(&css[i..e]);
            }
        }
        i = e;
    }
    (out, kept, dropped, offcanvas, vishidden, media_dropped)
}

/// Viewport width the basic lane lays out at; @media width
/// conditions are evaluated against it.
pub const BASIC_VIEWPORT_WIDTH_PX: u32 = 800;

/// Evaluate a `@media` prelude against the basic viewport. Returns
/// Some(verdict) only when every query in the list is decidable from
/// width alone; any unknown feature, `not`, or range syntax yields
/// None and the caller keeps the block (fail-closed).
fn eval_media_query_list(prelude: &[u8]) -> Option<bool> {
    let lower: Vec<u8> = prelude.iter().map(|b| b.to_ascii_lowercase()).collect();
    let text = String::from_utf8_lossy(&lower);
    let rest = text.trim_start().strip_prefix("@media")?.trim();
    let mut saw_query = false;
    // split on top-level commas
    let (mut start, mut parens) = (0usize, 0i32);
    let bytes = rest.as_bytes();
    let mut verdict = false;
    for idx in 0..=bytes.len() {
        let at_end = idx == bytes.len();
        if !at_end {
            match bytes[idx] {
                b'(' => parens += 1,
                b')' => parens -= 1,
                _ => {}
            }
        }
        if at_end || (bytes[idx] == b',' && parens == 0) {
            let q = rest[start..idx].trim();
            if !q.is_empty() {
                saw_query = true;
                match eval_single_media_query(q) {
                    Some(true) => verdict = true,
                    Some(false) => {}
                    None => return None,
                }
            }
            start = idx + 1;
        }
    }
    if saw_query {
        Some(verdict)
    } else {
        None
    }
}

fn eval_single_media_query(q: &str) -> Option<bool> {
    let mut t = q.trim();
    if t.starts_with("not ") || t.starts_with("not(") {
        return None;
    }
    if let Some(r) = t.strip_prefix("only ") {
        t = r.trim();
    }
    // optional media type followed by `and`, or bare conditions
    if !t.starts_with('(') {
        let split = t.find(|c: char| c.is_ascii_whitespace() || c == '(');
        let (ty, rest) = match split {
            Some(p) => (t[..p].trim(), t[p..].trim()),
            None => (t, ""),
        };
        match ty {
            "print" | "speech" => return Some(false),
            "screen" | "all" | "" => {}
            _ => return None,
        }
        t = rest;
        if !t.is_empty() {
            t = t.strip_prefix("and")?.trim();
        }
    }
    let mut saw_cond = false;
    while !t.is_empty() {
        if !t.starts_with('(') {
            return None;
        }
        let close = t.find(')')?;
        let inner = t[1..close].trim();
        saw_cond = true;
        match eval_width_condition(inner) {
            Some(true) => {}
            Some(false) => return Some(false),
            None => return None,
        }
        t = t[close + 1..].trim();
        if !t.is_empty() {
            t = t.strip_prefix("and")?.trim();
        }
    }
    let _ = saw_cond;
    Some(true)
}

/// `(min-width: X)` / `(max-width: X)` / `(width: X)` against the basic
/// viewport. Values in px, or em/rem at 16px. Anything else is
/// undecidable.
fn eval_width_condition(inner: &str) -> Option<bool> {
    let (name, value) = inner.split_once(':')?;
    let px = parse_media_length_px(value.trim())?;
    let vp = BASIC_VIEWPORT_WIDTH_PX;
    match name.trim() {
        "min-width" => Some(vp >= px),
        "max-width" => Some(vp <= px),
        "width" => Some(vp == px),
        _ => None,
    }
}

fn parse_media_length_px(v: &str) -> Option<u32> {
    let (num, mult) = if let Some(n) = v.strip_suffix("px") {
        (n, 1.0f64)
    } else if let Some(n) = v.strip_suffix("rem") {
        (n, 16.0f64)
    } else if let Some(n) = v.strip_suffix("em") {
        (n, 16.0f64)
    } else {
        return None;
    };
    let f: f64 = num.trim().parse().ok()?;
    if f < 0.0 {
        return None;
    }
    Some(f.mul_add(mult, 0.0).round() as u32)
}

/// True for the classic screen-reader-only rule: an absolutely
/// positioned 1px box with overflow:hidden. Without overflow clipping
/// the text paints as a one-character-wide column.
fn has_visually_hidden_pattern(block: &[u8]) -> bool {
    let lower: Vec<u8> = block
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let has = |needle: &[u8]| lower.windows(needle.len()).any(|w| w == needle);
    has(b"position:absolute") && has(b"width:1px") && has(b"height:1px") && has(b"overflow:hidden")
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
    if parts
        .iter()
        .any(|p| p.iter().all(|b| b.is_ascii_whitespace()))
    {
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
        if b.is_ascii_whitespace() {
            part = rest;
        } else {
            break;
        }
    }
    while let Some((&b, rest)) = part.split_last() {
        if b.is_ascii_whitespace() {
            part = rest;
        } else {
            break;
        }
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
                if prev.is_some_and(|p| {
                    !matches!(p, b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'+' | b'~')
                }) {
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
                while i < compound.len()
                    && (compound[i].is_ascii_alphanumeric() || compound[i] == b'-')
                {
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
        tag = Some(
            clean[s..idx]
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect(),
        );
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
        while idx < clean.len()
            && (clean[idx].is_ascii_alphanumeric() || matches!(clean[idx], b'-' | b'_'))
        {
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
        out.push(if chunk.len() > 1 {
            ALPHA[(acc >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if chunk.len() > 2 {
            ALPHA[acc as usize & 63]
        } else {
            b'='
        });
    }
    out
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
pub(crate) fn parse_data_image(uri: &[u8]) -> Option<(u32, u32)> {
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
            && matches!(
                input.get(i + 2 + name.len()),
                Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/')
            )
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
            && matches!(
                inner.get(i + 7),
                Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/')
            )
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
                if b.is_ascii_whitespace() {
                    text = rest.to_vec();
                } else {
                    break;
                }
            }
            while let Some((&b, rest)) = text.split_last() {
                if b.is_ascii_whitespace() {
                    text = rest.to_vec();
                } else {
                    break;
                }
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
                        _ if ent.first() == Some(&b'#') => std::str::from_utf8(&ent[1..])
                            .ok()
                            .and_then(|d| d.parse::<u32>().ok())
                            .and_then(|n| u8::try_from(n).ok())
                            .filter(|&n| n >= 0x20),
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
        table_stack_depth: 0,
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
            rect: Rect {
                x: 0,
                y: 0,
                width: viewport_width,
                height: viewport_height,
            },
            color: rgb(color),
            source: Source {
                node: tree.boxes[root].source,
                action: None,
                text_range: None,
            },
        });
    }
    let content_h = painter.layout_block(root, 0, 0, viewport_width, &mut list, (0, 0));
    let h = if content_h > 0 {
        content_h
    } else {
        i64::from(viewport_height)
    };
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
        table_stack_depth: 0,
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
            rect: Rect {
                x: 0,
                y: 0,
                width: viewport_width,
                height: viewport_height,
            },
            color: rgb(color),
            source: Source {
                node: tree.boxes[root].source,
                action: None,
                text_range: None,
            },
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

/// Restricted CSS 2.2 §10.3.3 via the engine resolver, plus the one
/// extension it refuses: a `max-width` cap, applied by re-running with
/// the cap as the used width so auto margins split the leftover
/// (`.mw7.center` and friends). Returns the geometry and whether the
/// cap fired. LTR only, matching the resolver's own restriction.
fn capped_block_width(
    width: crate::computed_style::Length,
    max_width: crate::computed_style::Length,
    containing: u32,
    left_edge: u32,
    right_edge: u32,
    margin_left: Option<i64>,
    margin_right: Option<i64>,
) -> (crate::box_tree::UsedBlockWidth, bool) {
    let geom = crate::box_tree::used_block_width_edges(
        width,
        containing,
        left_edge,
        right_edge,
        margin_left,
        margin_right,
        false,
    );
    if let crate::computed_style::Length::Px(cap) = max_width {
        if geom.content > cap {
            return (
                crate::box_tree::used_block_width_edges(
                    crate::computed_style::Length::Px(cap),
                    containing,
                    left_edge,
                    right_edge,
                    margin_left,
                    margin_right,
                    false,
                ),
                true,
            );
        }
    }
    (geom, false)
}

#[cfg(test)]
#[test]
fn basic_breaker_collapses_inter_word_space() {
    let lines = crate::basic_text::place(&[("four five six", false)], 100, false, |ch, _| {
        Some(if ch == ' ' { 5 } else { 10 })
    })
    .unwrap();
    assert_eq!(lines.glyphs[5].character, 'f');
    assert_eq!(lines.glyphs[5].x, 45);
}

#[cfg(test)]
mod hgeom_tests {
    use super::capped_block_width;
    use crate::computed_style::Length;

    #[test]
    fn auto_margins_split_free_space() {
        // 700px block in an 800px container, both margins auto.
        let (geom, capped) =
            capped_block_width(Length::Px(700), Length::Auto, 800, 0, 0, None, None);
        assert!(!capped);
        assert_eq!(geom.content, 700);
        assert_eq!(geom.margin_left, 50);
        assert_eq!(geom.margin_right, 50);
    }

    #[test]
    fn max_width_cap_recenters() {
        // width:auto fills 800, capped to 768: the re-run splits the
        // 32px leftover across the two auto margins.
        let (geom, capped) =
            capped_block_width(Length::Auto, Length::Px(768), 800, 0, 0, None, None);
        assert!(capped);
        assert_eq!(geom.content, 768);
        assert_eq!(geom.margin_left, 16);
        assert_eq!(geom.margin_right, 16);
    }

    #[test]
    fn cap_below_content_does_not_fire_and_one_sided_auto_absorbs() {
        let (geom, capped) =
            capped_block_width(Length::Px(500), Length::Px(768), 800, 0, 0, Some(10), None);
        assert!(!capped);
        assert_eq!(geom.margin_left, 10);
        assert_eq!(geom.margin_right, 290);
    }

    #[test]
    fn over_wide_content_overruns_the_right_margin() {
        // CSS 2.2 §10.3.3 over-constrained LTR: the right margin absorbs
        // the overflow (goes negative); content keeps its used width.
        let (geom, _) = capped_block_width(Length::Px(900), Length::Auto, 800, 0, 0, None, None);
        assert_eq!(geom.content, 900);
        assert_eq!(geom.margin_left, 0);
        assert_eq!(geom.margin_right, -100);
    }
}

#[cfg(test)]
mod media_eval_tests {
    use super::*;

    fn eval(p: &str) -> Option<bool> {
        eval_media_query_list(p.as_bytes())
    }

    #[test]
    fn media_width_verdicts_at_basic_viewport() {
        assert_eq!(
            eval("@media only screen and (max-width: 700px)"),
            Some(false)
        );
        assert_eq!(
            eval("@media only screen and (max-width: 900px)"),
            Some(true)
        );
        assert_eq!(eval("@media screen and (min-width: 45em)"), Some(true)); // 720px
        assert_eq!(eval("@media screen and (min-width: 60em)"), Some(false));
        assert_eq!(
            eval("@media (min-width: 700px) and (max-width: 900px)"),
            Some(true)
        );
        assert_eq!(eval("@media print"), Some(false));
        assert_eq!(eval("@media print, (max-width: 900px)"), Some(true));
        assert_eq!(eval("@media screen"), Some(true));
        // undecidable cases keep the block
        assert_eq!(eval("@media (hover: hover)"), None);
        assert_eq!(eval("@media not screen and (max-width: 700px)"), None);
        assert_eq!(eval("@media (400px <= width <= 900px)"), None);
        assert_eq!(eval("@media screen and (orientation: landscape)"), None);
        assert_eq!(eval("@media (prefers-color-scheme: dark)"), None);
    }

    #[test]
    fn media_prefilter_drops_false_blocks_and_keeps_true_ones() {
        let census = census_of(b"<div class=\"a b\"></div>");
        let css = b"@media only screen and (max-width: 700px) { .a { color: red; } }\n@media (max-width: 900px) { .b { color: blue; } }\n@media (hover: hover) { .a { color: green; } }";
        let (out, kept, _dropped, _off, _vh, media_dropped) = prefilter_css(css, &census);
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("color: red"), "false block must go: {text}");
        assert!(text.contains("color: blue"), "true block stays: {text}");
        assert!(
            text.contains("color: green"),
            "undecidable block stays: {text}"
        );
        assert_eq!(media_dropped, 1);
        assert!(kept >= 2);
    }
}
