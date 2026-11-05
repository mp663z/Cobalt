//! CSS 2.1 display box generation before used values and line layout.
//!
//! This is a separate, bounded path; the shipping reader remains the fallback.
//! It preserves DOM order and text. Anonymous block wrappers follow CSS 2.1
//! §9.2.1.1 for mixed block and inline children. It does not claim to layout
//! those boxes, and calls out block-in-inline splitting as unsupported.

use crate::computed_style::{
    AlignItems, BoxSizing, Computed, Direction, Display, JustifyContent, Length, Margin, Position,
};
use crate::inline_lines::{place_runs_text, shaped_block_lines, DirectTextLines, TextShaping};
use crate::style_tree::StyleTree;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoxKind {
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Flex,
    Text,
    AnonymousBlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CssBox {
    /// Original element is a non-replaced phrase, not merely display:inline.
    pub inline_phrase: bool,
    pub inline_attributes: bool,
    pub kind: BoxKind,
    // Note: `style.padding_*` is the padding plus border edge, see `from_style`.
    /// Index into the styled arena; anonymous boxes have no DOM source.
    pub source: Option<usize>,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub style: Computed,
    /// Original DOM text. White-space processing belongs to line layout.
    pub text: Option<String>,
}

#[derive(Default)]
pub struct BoxTree {
    pub boxes: Vec<CssBox>,
    pub roots: Vec<usize>,
    pub truncated: bool,
    pub unsupported: bool,
    /// Quirks is a visual paint gate, not a block geometry diagnostic gate.
    pub quirks: bool,
}

/// CSS 2.2 §10.3.3, restricted to zero margin/padding/border, LTR,
/// non-replaced block boxes in normal flow. These are content widths only.
/// A future full algorithm must include box edges and direction rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsedBlockWidth {
    pub content: u32,
    pub margin_left: i64,
    pub margin_right: i64,
}

/// Resolve a restricted CSS 2.2 §10.3.3 non-replaced normal-flow block.
/// Border and padding have been combined into nonnegative pixel edges; signed
/// margins are `None` for auto. The containing block's direction determines
/// which margin absorbs an over-constrained width. Callers must separately
/// reject floating, positioned, replaced, min/max-width, and box-sizing cases.
#[must_use]
pub fn used_block_width_edges(
    width: Length,
    containing: u32,
    left_edge: u32,
    right_edge: u32,
    left_margin: Option<i64>,
    right_margin: Option<i64>,
    rtl: bool,
) -> UsedBlockWidth {
    let specified = match width {
        Length::Auto => None,
        Length::Px(px) => Some(px),
        Length::Percent(hundredths) => Some(
            u32::try_from(u64::from(containing) * u64::from(hundredths) / 10_000)
                .unwrap_or(u32::MAX),
        ),
    };
    let edges = i64::from(left_edge) + i64::from(right_edge);
    if specified.is_none() {
        let left = left_margin.unwrap_or(0);
        let right = right_margin.unwrap_or(0);
        let available = i64::from(containing) - edges - left - right;
        let content = u32::try_from(available.max(0)).unwrap_or(u32::MAX);
        let remaining = i64::from(containing) - edges - i64::from(content);
        return if rtl {
            UsedBlockWidth {
                content,
                margin_left: remaining - right,
                margin_right: right,
            }
        } else {
            UsedBlockWidth {
                content,
                margin_left: left,
                margin_right: remaining - left,
            }
        };
    }
    let content = specified.unwrap_or(0);
    let mut left = left_margin;
    let mut right = right_margin;
    let minimum = edges + i64::from(content) + left.unwrap_or(0) + right.unwrap_or(0);
    if minimum > i64::from(containing) {
        left.get_or_insert(0);
        right.get_or_insert(0);
    }
    let free = i64::from(containing) - edges - i64::from(content);
    let (margin_left, margin_right) = match (left, right) {
        (None, None) => (free / 2, free - free / 2),
        (None, Some(right)) => (free - right, right),
        (Some(_), Some(right)) if rtl => (free - right, right),
        (Some(left), _) => (left, free - left),
    };
    UsedBlockWidth {
        content,
        margin_left,
        margin_right,
    }
}

/// The same restricted equation, converting a declared border-box width to
/// a nonnegative content width after the given border/padding pixel edges.
#[must_use]
pub fn used_block_width_sized(
    width: Length,
    containing: u32,
    [left_edge, right_edge]: [u32; 2],
    margins: [Option<i64>; 2],
    rtl: bool,
    sizing: BoxSizing,
) -> UsedBlockWidth {
    let edges = i64::from(left_edge) + i64::from(right_edge);
    let declared = match width {
        Length::Auto => None,
        Length::Px(px) => Some(px),
        Length::Percent(hundredths) => Some(
            u32::try_from(u64::from(containing) * u64::from(hundredths) / 10_000)
                .unwrap_or(u32::MAX),
        ),
    };
    let content = declared.map(|px| match sizing {
        BoxSizing::ContentBox => px,
        BoxSizing::BorderBox => u32::try_from((i64::from(px) - edges).max(0)).unwrap_or(u32::MAX),
    });
    used_block_width_edges(
        content.map_or(Length::Auto, Length::Px),
        containing,
        left_edge,
        right_edge,
        margins[0],
        margins[1],
        rtl,
    )
}

/// Resolve a signed horizontal margin against containing-block content width.
/// Auto remains unknown until the block-width equation is solved.
#[must_use]
pub fn resolve_margin(margin: Margin, containing: u32) -> Option<i64> {
    match margin {
        // Unresolved (subpixel or overflowing) em margins carry no proven
        // geometry: refuse them like an unprovable auto.
        Margin::Auto | Margin::Em(_) => None,
        Margin::Px(px) => Some(i64::from(px)),
        Margin::Percent(hundredths) => Some(i64::from(containing) * i64::from(hundredths) / 10_000),
    }
}

/// Fast path for LTR blocks with zero border, padding, and fixed zero margins.
/// It does not center a fixed-width block: spare width goes to the right.
#[must_use]
pub fn used_block_width(width: Length, containing: u32) -> UsedBlockWidth {
    used_block_width_edges(width, containing, 0, 0, Some(0), Some(0), false)
}

/// A definite specified height only; `auto` needs child layout, and a
/// percentage needs a definite containing-block content height. This is not
/// the used height of content that could overflow the specified height.
#[must_use]
pub fn specified_block_height(height: Length, containing: Option<u32>) -> Option<u32> {
    match height {
        Length::Auto => None,
        Length::Px(px) => Some(px),
        Length::Percent(hundredths) => containing.map(|base| {
            u32::try_from(u64::from(base) * u64::from(hundredths) / 10_000).unwrap_or(u32::MAX)
        }),
    }
}

/// Collapse a set of margins proven adjoining by the caller. This numeric
/// primitive does not decide whether parent/child, sibling, or empty-block
/// margins actually adjoin in the formatting context.
#[must_use]
pub fn collapse_adjoining_margins(margins: &[i64]) -> i64 {
    let positive = margins
        .iter()
        .copied()
        .filter(|&m| m > 0)
        .max()
        .unwrap_or(0);
    let negative = margins
        .iter()
        .copied()
        .filter(|&m| m < 0)
        .min()
        .unwrap_or(0);
    positive.saturating_add(negative)
}

/// Place a restricted run of sibling block border boxes under a parent content
/// origin. Inputs must be normal-flow siblings with known heights and margins,
/// no parent-child margin collapse, clearance, line boxes, or vertical edges.
/// A missing dimension stops placement of this and all following siblings.
#[must_use]
pub fn stack_definite_siblings(
    origin: i64,
    siblings: &[(Option<u32>, Option<i64>, Option<i64>)],
) -> Vec<Option<i64>> {
    let mut positions = Vec::with_capacity(siblings.len());
    let mut bottom = Some(origin);
    let mut previous_bottom_margin = None;
    for (index, &(height, top_margin, bottom_margin)) in siblings.iter().enumerate() {
        let top = bottom.and_then(|last_bottom| {
            let gap = if index == 0 {
                top_margin?
            } else {
                collapse_adjoining_margins(&[previous_bottom_margin?, top_margin?])
            };
            last_bottom.checked_add(gap)
        });
        positions.push(top);
        bottom = top.and_then(|y| y.checked_add(i64::from(height?)));
        previous_bottom_margin = bottom_margin;
    }
    positions
}

/// Vertical margins resolve percentage against the containing block's
/// content width, not its height. Auto computes to zero in normal flow.
#[must_use]
pub fn resolve_vertical_margin(margin: Margin, containing_width: u32) -> i64 {
    resolve_margin(margin, containing_width).unwrap_or(0)
}

impl BoxTree {
    /// Flex container, or an in-flow item of one: both establish an
    /// independent formatting context, so child margins never collapse out.
    fn is_flex_root(&self, index: usize) -> bool {
        self.boxes[index].kind == BoxKind::Flex
            || self.boxes[index]
                .parent
                .is_some_and(|p| self.boxes[p].kind == BoxKind::Flex)
    }

    /// Scratch flex slice (single-line row, flex-start, LTR). Returns per item
    /// (index, used width, x of item content relative to the container content
    /// x). Items are static display:block boxes with px margins, content-box
    /// sizing, height auto|px, and a px flex base size (flex-basis px, else
    /// px width). Sizes come from the CSS Flexbox 1 sec 9.7 resolution in
    /// `flex_targets`; anything unprovable is refused.
    fn flex_row_items(&self, container: usize, content_width: u32) -> Option<Vec<(usize, UsedBlockWidth, i64)>> {
        let node = &self.boxes[container];
        if node.kind != BoxKind::Flex
            || node.style.direction != Direction::Ltr
            || node.style.positioned.position != Position::Static
            || node.children.is_empty()
        {
            return None;
        }
        for &child in &node.children {
            let item = &self.boxes[child];
            // Margin::Px is signed; negative item margins are unproven here: refuse.
            let (Margin::Px(0..), Margin::Px(0..), Margin::Px(0..), Margin::Px(0..)) = (
                item.style.margin_left,
                item.style.margin_right,
                item.style.margin_top,
                item.style.margin_bottom,
            ) else {
                return None;
            };
            if item.kind != BoxKind::Block
                || item.style.positioned.position != Position::Static
                || item.style.box_sizing != BoxSizing::ContentBox
                || item.style.direction != Direction::Ltr
                || !matches!(item.style.height, Length::Auto | Length::Px(_))
                || !matches!(item.style.width, Length::Auto | Length::Px(_))
                || !matches!(item.style.flex_basis, Length::Auto | Length::Px(_))
            {
                return None;
            }
        }
        let targets = self.flex_targets(container, content_width)?;
        // justify-content: distribute the remaining main-axis free space.
        // Exact integers only (a half or fractional pixel refuses, never rounds);
        // negative free space is only allowed for flex-start (no overflow
        // alignment modes are modelled).
        let used: i64 = node
            .children
            .iter()
            .zip(&targets)
            .map(|(&c, &t)| {
                let s = &self.boxes[c].style;
                let (ml, mr) = match (s.margin_left, s.margin_right) {
                    (Margin::Px(a), Margin::Px(b)) => (i64::from(a), i64::from(b)),
                    _ => (0, 0),
                };
                t + ml + mr + i64::from(s.padding_left) + i64::from(s.padding_right)
            })
            .sum();
        let free = i64::from(content_width) - used;
        let count = i64::try_from(node.children.len()).ok()?;
        let (lead, gap) = match node.style.justify_content {
            JustifyContent::FlexStart => (0, 0),
            _ if free < 0 => return None,
            JustifyContent::FlexEnd => (free, 0),
            JustifyContent::Center => {
                if free % 2 != 0 {
                    return None;
                }
                (free / 2, 0)
            }
            JustifyContent::SpaceBetween => {
                if count == 1 {
                    (0, 0)
                } else if free % (count - 1) != 0 {
                    return None;
                } else {
                    (0, free / (count - 1))
                }
            }
            JustifyContent::SpaceAround => {
                if free % (2 * count) != 0 {
                    return None;
                }
                (free / (2 * count), free / count)
            }
            JustifyContent::SpaceEvenly => {
                if free % (count + 1) != 0 {
                    return None;
                }
                (free / (count + 1), free / (count + 1))
            }
        };
        let mut out = Vec::new();
        let mut cursor = lead;
        for (position, &child) in node.children.iter().enumerate() {
            if position > 0 {
                cursor += gap;
            }
            let item = &self.boxes[child];
            let (Margin::Px(ml), Margin::Px(mr)) = (item.style.margin_left, item.style.margin_right) else {
                return None;
            };
            let width = u32::try_from(targets[position]).ok()?;
            let left = cursor + i64::from(ml);
            let x = left + i64::from(item.style.padding_left);
            cursor = left
                + i64::from(item.style.padding_left)
                + i64::from(width)
                + i64::from(item.style.padding_right)
                + i64::from(mr);
            out.push((
                child,
                UsedBlockWidth { content: width, margin_left: i64::from(ml), margin_right: i64::from(mr) },
                x,
            ));
        }
        Some(out)
    }

    /// Content-box min-content width of a text-free block whose children are
    /// all static blocks with px (or auto, recursive) widths and non-negative
    /// px margins. Anything else (text, inline, flex, percent, auto margins,
    /// border-box, positioned) is unprovable: None.
    fn flex_min_content(&self, index: usize, depth: usize) -> Option<i64> {
        if depth > 32 {
            return None;
        }
        let mut widest = 0_i64;
        for &child in &self.boxes[index].children {
            let c = &self.boxes[child];
            // Signed type: a negative margin makes the contribution unprovable here.
            let (Margin::Px(ml @ 0..), Margin::Px(mr @ 0..)) = (c.style.margin_left, c.style.margin_right) else {
                return None;
            };
            if c.kind != BoxKind::Block
                || c.style.positioned.position != Position::Static
                || c.style.box_sizing != BoxSizing::ContentBox
                || c.style.direction != Direction::Ltr
            {
                return None;
            }
            let inner = match c.style.width {
                Length::Px(w) => i64::from(w),
                Length::Auto => self.flex_min_content(child, depth + 1)?,
                Length::Percent(_) => return None,
            };
            let outer = inner
                + i64::from(c.style.padding_left)
                + i64::from(c.style.padding_right)
                + i64::from(ml)
                + i64::from(mr);
            widest = widest.max(outer);
        }
        Some(widest)
    }

    /// CSS Flexbox 1 sec 9.7 "Resolving Flexible Lengths", exact integers only
    /// (a non-integer share refuses the whole container, never rounds).
    /// No max-width/height exists, so only min violations occur. Hypothetical
    /// size = max(base, min); min = min(content size suggestion, specified size
    /// suggestion) where the specified suggestion exists only for a px width.
    fn flex_targets(&self, container: usize, content_width: u32) -> Option<Vec<i64>> {
        // art76 scratch (UNADOPTED): known-min rows resolve through flex_units_v2 ExactPx; everything else stays legacy.
        if let Some(units) = self.flex_targets_units(container, content_width) {
            use std::sync::atomic::Ordering::Relaxed;
            if units.is_some() { SHADOW_AA.fetch_add(1, Relaxed); } else { SHADOW_AR.fetch_add(1, Relaxed); }
            return units;
        }
        SHADOW_SKIP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if self.boxes[container].children.iter().any(|&i| self.boxes[i].style.min_width != Length::Auto || self.boxes[i].style.max_width != Length::Auto) {
            return None; // legacy loop does not model min/max-width
        }
        self.flex_targets_legacy(container, content_width)
    }

    /// Some(result) only when every input is in the modelled family (known min-content, non-negative px margins, px basis); result None = refuse.
    fn flex_targets_units(&self, container: usize, content_width: u32) -> Option<Option<Vec<i64>>> {
        use crate::flex_units_v2::{resolve, Item, Mode};
        let mut items = Vec::new();
        let mut pads = Vec::new();
        for &i in &self.boxes[container].children {
            let s = &self.boxes[i].style;
            let (Margin::Px(ml), Margin::Px(mr)) = (s.margin_left, s.margin_right) else { return None };
            if ml < 0 || mr < 0 { return None }
            let b = match (s.flex_basis, s.width) {
                (Length::Px(p), _) | (Length::Auto, Length::Px(p)) => i64::from(p),
                _ => return None,
            };
            let maxw = match s.max_width { Length::Px(p) => Some(i64::from(p)), _ => None };
            let m = match s.min_width {
                Length::Px(p) => i64::from(p),
                _ => {
                    let c = self.flex_min_content(i, 0)?;
                    let c = match s.width { Length::Px(w) => c.min(i64::from(w)), _ => c };
                    maxw.map_or(c, |mx| c.min(mx))
                }
            };
            let padb = i64::from(s.padding_left) + i64::from(s.padding_right);
            pads.push(padb);
            items.push(Item { base: b * 64, min: m * 64, max: maxw.map(|x| x * 64), grow: s.flex_grow as u32, shrink: s.flex_shrink as u32, padb: padb * 64, ml: i64::from(ml) * 64, mr: i64::from(mr) * 64 });
        }
        let r = resolve(Mode::ExactPx, i64::from(content_width) * 64, &items);
        Some(r.map(|r| r.iter().zip(&pads).map(|(&(_, bw), &p)| bw / 64 - p).collect()))
    }

    /// art68 scratch: test-only shadow check of the 1/64 ExactPx resolver against the legacy loop; never affects the result.
    fn flex_units_shadow(&self, container: usize, content_width: u32, legacy: &Option<Vec<i64>>) {
        use crate::flex_units_v3::{resolve, Item, Mode};
        let mut items = Vec::new();
        let mut pads = Vec::new();
        for &i in &self.boxes[container].children {
            let s = &self.boxes[i].style;
            let (Margin::Px(ml), Margin::Px(mr)) = (s.margin_left, s.margin_right) else { SHADOW_SKIP.fetch_add(1, std::sync::atomic::Ordering::Relaxed); return };
            if ml < 0 || mr < 0 { SHADOW_SKIP.fetch_add(1, std::sync::atomic::Ordering::Relaxed); return }
            let b = match (s.flex_basis, s.width) {
                (Length::Px(p), _) | (Length::Auto, Length::Px(p)) => i64::from(p),
                _ => return,
            };
            let Some(c) = self.flex_min_content(i, 0) else { return };
            let m = match s.width { Length::Px(w) => c.min(i64::from(w)), _ => c };
            let padb = i64::from(s.padding_left) + i64::from(s.padding_right);
            pads.push(padb);
            items.push(Item { base: b * 64, min: m * 64, max: None, grow: s.flex_grow as u32, shrink: s.flex_shrink as u32, padb: padb * 64, ml: i64::from(ml) * 64, mr: i64::from(mr) * 64 });
        }
        let units = resolve(Mode::ExactPx, i64::from(content_width) * 64, &items);
        let units_t: Option<Vec<i64>> = units.map(|r| r.iter().zip(&pads).map(|(&(_, bw), &p)| bw / 64 - p).collect());
        use std::sync::atomic::Ordering::Relaxed;
        match (legacy, &units_t) {
            (Some(a), Some(b)) if a == b => { SHADOW_AA.fetch_add(1, Relaxed); }
            (None, None) => { SHADOW_AR.fetch_add(1, Relaxed); }
            (None, Some(_)) => { SHADOW_LR.fetch_add(1, Relaxed); }
            _ => { SHADOW_DIS.fetch_add(1, Relaxed); eprintln!("SHADOW DISAGREE legacy {legacy:?} units {units_t:?}"); }
        }
    }

    fn flex_targets_legacy(&self, container: usize, content_width: u32) -> Option<Vec<i64>> {
        let items = &self.boxes[container].children;
        let n = items.len();
        let (mut base, mut extra, mut min) = (Vec::new(), Vec::new(), Vec::new());
        let (mut grow, mut shrink) = (Vec::new(), Vec::new());
        let mut unknown_min = false;
        let mut unknown_items: Vec<usize> = Vec::new();
        for &i in items {
            let s = &self.boxes[i].style;
            let (Margin::Px(ml), Margin::Px(mr)) = (s.margin_left, s.margin_right) else {
                return None;
            };
            let b = match (s.flex_basis, s.width) {
                (Length::Px(p), _) | (Length::Auto, Length::Px(p)) => i64::from(p),
                _ => return None, // content-sized basis needs max-content
            };
            // Unprovable min-content is tolerated only where it cannot matter:
            // a px width with basis auto has min <= width = base, so the
            // item can only violate a min while shrinking; that is re-checked
            // below and refuses.
            let content_min = self.flex_min_content(i, 0);
            let m = match (content_min, s.width, s.flex_basis) {
                (Some(c), Length::Px(w), _) => c.min(i64::from(w)),
                (Some(c), _, _) => c,
                (None, Length::Px(_), Length::Auto) => {
                    unknown_min = true;
                    0
                }
                (None, _, _) => return None,
            };
            if content_min.is_none() {
                unknown_items.push(items.iter().position(|&x| x == i)?);
            }
            base.push(b);
            min.push(m);
            extra.push(i64::from(ml) + i64::from(mr) + i64::from(s.padding_left) + i64::from(s.padding_right));
            grow.push(i64::from(s.flex_grow));
            shrink.push(i64::from(s.flex_shrink));
        }
        let hyp: Vec<i64> = (0..n).map(|k| base[k].max(min[k])).collect();
        let hyp_sum: i64 = (0..n).map(|k| hyp[k] + extra[k]).sum();
        let growing = hyp_sum < i64::from(content_width);
        if !growing && (unknown_min || !unknown_items.is_empty()) {
            return None; // shrinking needs every min-content exactly
        }
        let factor = if growing { &grow } else { &shrink };
        let mut target = base.clone();
        let mut frozen = vec![false; n];
        for k in 0..n {
            // Step 3: inflexible items freeze at their hypothetical size.
            if factor[k] == 0 || (!growing && base[k] < hyp[k]) {
                target[k] = hyp[k];
                frozen[k] = true;
            }
        }
        for _ in 0..=n {
            if frozen.iter().all(|&f| f) {
                break;
            }
            let used: i64 = (0..n).map(|k| if frozen[k] { target[k] } else { base[k] } + extra[k]).sum();
            let free = i64::from(content_width) - used;
            let mut proposed = target.clone();
            if free != 0 {
                if (growing && free < 0) || (!growing && free > 0) {
                    return None; // sign disagrees with the mode: unmodelled
                }
                let weight = |k: usize| if growing { factor[k] } else { factor[k] * base[k] };
                let total: i64 = (0..n).filter(|&k| !frozen[k]).map(weight).sum();
                if total == 0 {
                    return None;
                }
                for k in (0..n).filter(|&k| !frozen[k]) {
                    let num = free.abs().checked_mul(weight(k))?;
                    if num % total != 0 {
                        return None;
                    }
                    proposed[k] = if growing { base[k] + num / total } else { base[k] - num / total };
                }
            } else {
                for k in (0..n).filter(|&k| !frozen[k]) {
                    proposed[k] = base[k];
                }
            }
            let violation: i64 = (0..n)
                .filter(|&k| !frozen[k])
                .map(|k| (min[k] - proposed[k]).max(0))
                .sum();
            for k in 0..n {
                if frozen[k] {
                    continue;
                }
                if violation == 0 {
                    target[k] = proposed[k];
                    frozen[k] = true;
                } else if proposed[k] < min[k] {
                    target[k] = min[k];
                    frozen[k] = true;
                }
            }
        }
        frozen.iter().all(|&f| f).then_some(target)
    }

    /// Exact empty out-of-flow auto container, not general shrink-to-fit.
    /// Its absolute children contribute neither intrinsic width nor height.
    pub(crate) fn zero_positioned_container(&self, index: usize) -> bool {
        let node = &self.boxes[index];
        node.kind == BoxKind::Block
            && node.style.positioned.position == Position::Absolute
            && node.style.width == Length::Auto
            && node.style.height == Length::Auto
            && node.style.positioned.offsets == [Margin::Auto; 4]
            && node.style.direction == Direction::Ltr
            && [
                node.style.margin_top,
                node.style.margin_right,
                node.style.margin_bottom,
                node.style.margin_left,
            ] == [Margin::Px(0); 4]
            && [
                node.style.padding_top,
                node.style.padding_right,
                node.style.padding_bottom,
                node.style.padding_left,
            ] == [0; 4]
            && node.style.decor.used_border() == [0; 4]
            && node.style.used_background_color().is_none()
            && node.parent.is_some_and(|p| {
                self.boxes[p].kind == BoxKind::Block
                    && self.boxes[p].style.positioned.position == Position::Static
            })
            && !node.children.is_empty()
            && node.children.iter().all(|&child| {
                let child = &self.boxes[child];
                child.kind == BoxKind::Block
                    && child.children.is_empty()
                    && child.style.positioned.position == Position::Absolute
                    && matches!(child.style.width, Length::Px(_))
                    && matches!(child.style.height, Length::Px(_))
            })
    }

    fn zero_positioned_width(&self, index: usize) -> Option<UsedBlockWidth> {
        self.zero_positioned_container(index)
            .then_some(UsedBlockWidth {
                content: 0,
                margin_left: 0,
                margin_right: 0,
            })
    }

    /// The vertical margin `index` presents to its neighbours once margins
    /// that touch collapse (CSS 2.2 8.3.1): a box with no padding or border on
    /// that edge shares its margin with its first (top) or last (bottom)
    /// in-flow block child. The root element never collapses with its
    /// children, and a bottom margin only escapes an auto-height box.
    fn collapsed_margin(
        &self,
        index: usize,
        top: bool,
        widths: &WidthPass,
        viewport_width: u32,
    ) -> i64 {
        let mut margins = Vec::new();
        let mut current = index;
        loop {
            let node = &self.boxes[current];
            let containing = node
                .parent
                .and_then(|parent| widths.widths[parent])
                .map_or(viewport_width, |w| w.content);
            margins.push(resolve_vertical_margin(
                if top {
                    node.style.margin_top
                } else {
                    node.style.margin_bottom
                },
                containing,
            ));
            let open = !self.is_flex_root(current)
                && node.parent.is_some()
                && if top {
                    node.style.padding_top == 0
                } else {
                    node.style.padding_bottom == 0 && node.style.height == Length::Auto
                };
            let next = if top {
                node.children
                    .iter()
                    .find(|&&c| self.boxes[c].style.positioned.position == Position::Static)
            } else {
                node.children
                    .iter()
                    .rev()
                    .find(|&&c| self.boxes[c].style.positioned.position == Position::Static)
            };
            match next {
                Some(&child)
                    if open
                        && matches!(self.boxes[child].kind, BoxKind::Block | BoxKind::ListItem | BoxKind::Flex) =>
                {
                    current = child;
                }
                _ => break,
            }
        }
        collapse_adjoining_margins(&margins)
    }
}

/// Top-down diagnostic of definite block heights. Missing heights remain
/// unknown for descendant percentage heights. No y position is inferred.
pub struct HeightPass {
    pub heights: Vec<Option<u32>>,
    pub unsupported: bool,
    pub truncated: bool,
}

impl HeightPass {
    #[must_use]
    pub fn from_boxes(tree: &BoxTree, viewport_height: u32) -> Self {
        let mut result = Self {
            heights: vec![None; tree.boxes.len()],
            unsupported: tree.unsupported || tree.stray_min_max_width(),
            truncated: tree.truncated,
        };
        let mut stack: Vec<_> = tree
            .roots
            .iter()
            .rev()
            .map(|&root| (root, Some(viewport_height)))
            .collect();
        let mut visited = 0_usize;
        while let Some((index, containing)) = stack.pop() {
            visited += 1;
            if visited > tree.boxes.len() {
                result.truncated = true;
                break;
            }
            let node = &tree.boxes[index];
            let height = match node.kind {
                BoxKind::Block | BoxKind::ListItem | BoxKind::Flex => {
                    specified_block_height(node.style.height, containing).map(|declared| {
                        if node.style.box_sizing == BoxSizing::BorderBox {
                            declared.saturating_sub(
                                node.style
                                    .padding_top
                                    .saturating_add(node.style.padding_bottom),
                            )
                        } else {
                            declared
                        }
                    })
                }
                BoxKind::AnonymousBlock => None,
                BoxKind::InlineBlock | BoxKind::Inline | BoxKind::Text => {
                    if node.kind == BoxKind::InlineBlock {
                        result.unsupported = true;
                    }
                    None
                }
            };
            let height = if tree.zero_positioned_container(index) {
                Some(0)
            } else {
                height
            };
            result.heights[index] = height;
            for &child in node.children.iter().rev() {
                stack.push((child, height));
            }
        }
        result
    }
}

/// Used content heights for a restricted normal-flow block subtree. Explicit
/// percentage heights resolve only against *specified definite* ancestors;
/// computing an auto parent's height does not make its descendants' percentage
/// heights definite. Auto height includes child border/padding boxes and
/// collapsed sibling gaps. A first or last child's edge margin escapes
/// through the collapse chain instead of extending the height; a box that
/// could collapse through itself stays unsupported.
pub struct UsedHeightPass {
    pub heights: Vec<Option<u32>>,
    pub unsupported: bool,
    pub truncated: bool,
}

impl UsedHeightPass {
    #[must_use]
    pub fn from_boxes(tree: &BoxTree, viewport_width: u32, viewport_height: u32) -> Self {
        Self::calculate(tree, viewport_width, viewport_height, None, None, None)
    }

    /// Resolve auto block heights for the narrow direct-text case using
    /// caller-supplied exact font advances and line heights. This is a
    /// geometry pass only: a separate painter must use the same font and
    /// reject any unsupported box before returning a page image.
    #[must_use]
    pub fn from_boxes_with_direct_text(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        mut advance: impl FnMut(char, u32, bool) -> Option<u32>,
        mut line_height: impl FnMut(u32) -> Option<u32>,
    ) -> Self {
        Self::from_boxes_with_line_policy(
            tree,
            viewport_width,
            viewport_height,
            &mut advance,
            |size, _normal| line_height(size),
        )
    }

    /// Normal-aware height provider. Explicit CSS line-height keeps its legacy strut.
    #[must_use]
    pub fn from_boxes_with_line_policy(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        mut advance: impl FnMut(char, u32, bool) -> Option<u32>,
        mut line_height: impl FnMut(u32, bool) -> Option<u32>,
    ) -> Self {
        Self::calculate(
            tree,
            viewport_width,
            viewport_height,
            Some(&mut advance),
            Some(&mut line_height),
            None,
        )
    }

    /// Heights from a page-level shaped text layout decided once (see
    /// [`PageTextLayout`]); the same layout feeds the vertical pass and the painter.
    #[must_use]
    pub fn from_boxes_with_page_text(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        layout: &PageTextLayout,
    ) -> Self {
        Self::calculate(tree, viewport_width, viewport_height, None, None, Some(layout))
    }

    #[allow(clippy::too_many_lines)] // Bounded bottom-up height walk and guarded direct-text case.
    fn calculate(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        mut advance: Option<&mut dyn FnMut(char, u32, bool) -> Option<u32>>,
        mut line_height: Option<&mut dyn FnMut(u32, bool) -> Option<u32>>,
        layout: Option<&PageTextLayout>,
    ) -> Self {
        let specified = HeightPass::from_boxes(tree, viewport_height);
        let widths = WidthPass::from_boxes(tree, viewport_width);
        let mut result = Self {
            heights: specified.heights,
            unsupported: specified.unsupported || widths.unsupported,
            truncated: specified.truncated || widths.truncated,
        };
        if result.unsupported || result.truncated {
            return result;
        }
        // A valid box tree is acyclic; reverse traversal puts descendants
        // before parents. Invalid trees are rejected by the paint bridge.
        for (index, node) in tree.boxes.iter().enumerate().rev() {
            if node.kind == BoxKind::Text {
                if ((advance.is_none() || line_height.is_none()) && layout.is_none())
                    || inline_owner(tree, index).is_none()
                {
                    result.unsupported = true;
                }
                continue;
            }
            if node.kind == BoxKind::Inline {
                if ((advance.is_none() || line_height.is_none()) && layout.is_none())
                    || inline_owner(tree, index).is_none()
                {
                    result.unsupported = true;
                }
                continue;
            }
            if node.kind == BoxKind::Flex {
                // Single-line row: line cross size is the container's definite
                // height, else the tallest item margin box; auto-height items
                // stretch to it (align-self:normal behaves as stretch).
                let mut natural = 0_i64;
                let mut ok = !node.children.is_empty();
                for &child in &node.children {
                    let item = &tree.boxes[child];
                    let (Margin::Px(mt), Margin::Px(mb)) = (item.style.margin_top, item.style.margin_bottom) else {
                        ok = false;
                        break;
                    };
                    let Some(h) = result.heights[child] else {
                        ok = false;
                        break;
                    };
                    natural = natural.max(
                        i64::from(h)
                            + i64::from(item.style.padding_top)
                            + i64::from(item.style.padding_bottom)
                            + i64::from(mt)
                            + i64::from(mb),
                    );
                }
                let line = match result.heights[index] {
                    Some(definite) if i64::from(definite) >= natural => Some(i64::from(definite)),
                    Some(_) => None, // overflow past a definite height is not modelled
                    None => Some(natural),
                };
                let (Some(line), true) = (line, ok) else {
                    result.unsupported = true;
                    continue;
                };
                for &child in &node.children {
                    let item = &tree.boxes[child];
                    if item.style.height == Length::Auto && node.style.align_items == AlignItems::Stretch {
                        let (Margin::Px(mt), Margin::Px(mb)) = (item.style.margin_top, item.style.margin_bottom) else {
                            continue;
                        };
                        let stretched = line
                            - i64::from(item.style.padding_top)
                            - i64::from(item.style.padding_bottom)
                            - i64::from(mt)
                            - i64::from(mb);
                        result.heights[child] = u32::try_from(stretched).ok();
                    }
                }
                result.heights[index] = u32::try_from(line).ok().filter(|&l| l > 0);
                if result.heights[index].is_none() {
                    result.unsupported = true;
                }
                continue;
            }
            if !matches!(node.kind, BoxKind::Block | BoxKind::ListItem) {
                result.unsupported = true;
                continue;
            }
            if result.heights[index].is_some() {
                continue;
            }
            if node.style.height != Length::Auto {
                result.unsupported = true; // indefinite percentage
                continue;
            }
            let Some(width) = widths.widths[index].map(|w| w.content) else {
                result.unsupported = true;
                continue;
            };
            if !node.children.is_empty()
                && node
                    .children
                    .iter()
                    .all(|&child| matches!(tree.boxes[child].kind, BoxKind::Text | BoxKind::Inline))
            {
                let measured = if let Some(layout) = layout {
                    layout.blocks.get(index).and_then(Option::as_ref).map(|d| d.content_height)
                } else {
                    advance
                    .as_deref_mut()
                    .zip(line_height.as_deref_mut())
                    .and_then(|(advance, line_height)| {
                        place_runs_text(tree, index, width, advance, |size| {
                            line_height(
                                size,
                                node.style.line_height.is_none()
                                    && node.style.line_height_number.is_none(),
                            )
                        })
                        .ok()
                    })
                    .map(|lines| lines.content_height)
                };
                result.heights[index] = measured;
                if result.heights[index].is_none_or(|height| height == 0) {
                    result.unsupported = true;
                }
                continue;
            }
            let mut content = 0_i64;
            let mut previous: Option<usize> = None;
            let mut valid = true;
            for &child in &node.children {
                if tree.boxes[child].style.positioned.position == Position::Absolute {
                    continue;
                }
                let Some(next) = tree.boxes.get(child) else {
                    valid = false;
                    break;
                };
                if !matches!(next.kind, BoxKind::Block | BoxKind::ListItem | BoxKind::Flex) {
                    valid = false;
                    break;
                }
                let Some(height) = result.heights[child] else {
                    valid = false;
                    break;
                };
                if height == 0 && next.style.padding_top == 0 && next.style.padding_bottom == 0 {
                    valid = false; // empty child's margins may collapse through
                    break;
                }
                let top = tree.collapsed_margin(child, true, &widths, viewport_width);
                if let Some(prev) = previous {
                    content = content.saturating_add(collapse_adjoining_margins(&[
                        tree.collapsed_margin(prev, false, &widths, viewport_width),
                        top,
                    ]));
                } else if node.style.padding_top != 0 || node.parent.is_none() || tree.is_flex_root(index) {
                    // Nothing to collapse with: the margin sits inside.
                    content = content.saturating_add(top);
                }
                content = content
                    .saturating_add(i64::from(height))
                    .saturating_add(i64::from(next.style.padding_top))
                    .saturating_add(i64::from(next.style.padding_bottom));
                previous = Some(child);
            }
            if let Some(last) = previous {
                if node.style.padding_bottom != 0 || node.parent.is_none() || tree.is_flex_root(index) {
                    content = content.saturating_add(tree.collapsed_margin(
                        last,
                        false,
                        &widths,
                        viewport_width,
                    ));
                }
            } else if node.style.padding_top == 0 && node.style.padding_bottom == 0 {
                valid = false; // margin-through empty block
            }
            if valid {
                result.heights[index] = u32::try_from(content.max(0)).ok();
            }
            if result.heights[index].is_none() {
                result.unsupported = true;
            }
        }
        result
    }
}

/// Diagnostic vertical coordinates only for a narrow, fully resolved block
/// subtree. Parent/child margin collapse resolves through `collapsed_margin`:
/// an escaped first-child margin is carried in the parent's placement gap.
/// Unpadded zero-height boxes, inline content, and additional roots
/// are rejected rather than assigned speculative positions.
pub struct VerticalPass {
    pub content_y: Vec<Option<i64>>,
    pub unsupported: bool,
    pub truncated: bool,
}

impl VerticalPass {
    #[must_use]
    pub fn from_boxes(tree: &BoxTree, viewport_width: u32, viewport_height: u32) -> Self {
        let heights = UsedHeightPass::from_boxes(tree, viewport_width, viewport_height);
        Self::calculate(tree, viewport_width, viewport_height, &heights)
    }

    /// Coordinates from the same page-level shaped layout as the heights.
    #[must_use]
    pub fn from_boxes_with_page_text(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        layout: &PageTextLayout,
    ) -> Self {
        let heights =
            UsedHeightPass::from_boxes_with_page_text(tree, viewport_width, viewport_height, layout);
        Self::calculate(tree, viewport_width, viewport_height, &heights)
    }

    /// Diagnostic coordinates for direct-text blocks measured with one font
    /// provider. A separate full-page paint gate is still required: the
    /// background-only bridge intentionally refuses text nodes.
    #[must_use]
    pub fn from_boxes_with_direct_text(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        mut advance: impl FnMut(char, u32, bool) -> Option<u32>,
        mut line_height: impl FnMut(u32) -> Option<u32>,
    ) -> Self {
        Self::from_boxes_with_line_policy(
            tree,
            viewport_width,
            viewport_height,
            &mut advance,
            |size, _normal| line_height(size),
        )
    }

    /// Coordinates using the same normal-aware height policy as paint.
    #[must_use]
    pub fn from_boxes_with_line_policy(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        mut advance: impl FnMut(char, u32, bool) -> Option<u32>,
        mut line_height: impl FnMut(u32, bool) -> Option<u32>,
    ) -> Self {
        let heights = UsedHeightPass::from_boxes_with_line_policy(
            tree,
            viewport_width,
            viewport_height,
            &mut advance,
            &mut line_height,
        );
        Self::calculate(tree, viewport_width, viewport_height, &heights)
    }

    #[allow(clippy::too_many_lines)] // Validation and placement share one bounded tree walk.
    fn calculate(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        heights: &UsedHeightPass,
    ) -> Self {
        let count = tree.boxes.len();
        let mut result = Self {
            content_y: vec![None; count],
            unsupported: tree.unsupported || tree.stray_min_max_width(),
            truncated: tree.truncated,
        };
        if result.unsupported || result.truncated {
            return result;
        }
        if tree.roots.len() != 1 || tree.roots[0] >= count {
            result.unsupported = true;
            return result;
        }
        let widths = WidthPass::from_boxes(tree, viewport_width);
        if heights.truncated || widths.truncated {
            result.truncated = true;
            return result;
        }
        if heights.unsupported || widths.unsupported {
            result.unsupported = true;
            return result;
        }
        let root = tree.roots[0];
        let mut seen = vec![false; count];
        let mut stack = vec![(root, None)];
        while let Some((index, parent)) = stack.pop() {
            let Some(node) = tree.boxes.get(index) else {
                result.unsupported = true;
                break;
            };
            let inline_member = matches!(node.kind, BoxKind::Text | BoxKind::Inline)
                && inline_owner(tree, index).is_some_and(|owner| heights.heights[owner].is_some());
            if seen[index]
                || node.parent != parent
                || !(matches!(node.kind, BoxKind::Block | BoxKind::ListItem | BoxKind::Flex) || inline_member)
                || (!matches!(node.kind, BoxKind::Text | BoxKind::Inline)
                    && heights.heights[index].is_none_or(|h| {
                        h == 0
                            && node.style.padding_top == 0
                            && node.style.padding_bottom == 0
                            && !tree.zero_positioned_container(index)
                    }))
                || (!matches!(node.kind, BoxKind::Text | BoxKind::Inline)
                    && widths.widths[index].is_none())
            {
                result.unsupported = true;
                break;
            }
            seen[index] = true;
            if node.kind == BoxKind::Text {
                continue;
            }
            for &child in node.children.iter().rev() {
                stack.push((child, Some(index)));
            }
        }
        if result.unsupported || seen.iter().any(|&visited| !visited) {
            result.unsupported = true;
            return result;
        }
        result.content_y[root] = Some(
            resolve_vertical_margin(tree.boxes[root].style.margin_top, viewport_width)
                .saturating_add(i64::from(tree.boxes[root].style.padding_top)),
        );
        let mut stack = vec![root];
        while let Some(index) = stack.pop() {
            let node = &tree.boxes[index];
            if node.kind == BoxKind::Text {
                continue;
            }
            if node.kind == BoxKind::Flex {
                // Row items share one line: top-aligned at margin-top, no collapse.
                for &child in &node.children {
                    let item = &tree.boxes[child];
                    // Cross-axis offset of the item's margin box inside the line.
                    let offset = match (node.style.align_items, heights.heights[index], heights.heights[child], item.style.margin_top, item.style.margin_bottom) {
                        (AlignItems::Stretch | AlignItems::FlexStart, ..) => Some(0),
                        (mode, Some(line), Some(h), Margin::Px(mt), Margin::Px(mb)) => {
                            let outer = i64::from(h)
                                + i64::from(item.style.padding_top)
                                + i64::from(item.style.padding_bottom)
                                + i64::from(mt)
                                + i64::from(mb);
                            let spare = i64::from(line) - outer;
                            match mode {
                                AlignItems::FlexEnd => Some(spare),
                                // Odd spare would be a half pixel: refuse, never round.
                                _ if spare % 2 == 0 => Some(spare / 2),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    result.content_y[child] = match (item.style.margin_top, offset) {
                        (Margin::Px(mt), Some(offset)) => result.content_y[index].and_then(|y| {
                            y.checked_add(offset)?
                                .checked_add(i64::from(mt))?
                                .checked_add(i64::from(item.style.padding_top))
                        }),
                        _ => None,
                    };
                }
                for &child in node.children.iter().rev() {
                    stack.push(child);
                }
                continue;
            }
            let mut previous = None::<usize>;
            for &child in &node.children {
                if matches!(tree.boxes[child].kind, BoxKind::Text | BoxKind::Inline) {
                    // Inline members share the block's line layout; their
                    // glyphs anchor at the block's content origin.
                    result.content_y[child] = result.content_y[index];
                    continue;
                }
                if tree.boxes[child].style.positioned.position == Position::Absolute {
                    let style = tree.boxes[child].style;
                    let static_y = previous.map_or(result.content_y[index], |prev| {
                        result.content_y[prev].and_then(|y| {
                            heights.heights[prev].map(|h| {
                                y + i64::from(h)
                                    + i64::from(tree.boxes[prev].style.padding_bottom)
                                    + tree.collapsed_margin(prev, false, &widths, viewport_width)
                            })
                        })
                    });
                    let outer = heights.heights[child].map(|h| {
                        i64::from(h)
                            + i64::from(style.padding_top)
                            + i64::from(style.padding_bottom)
                    });
                    let nested = node.style.positioned.position == Position::Absolute;
                    let cb_height = if nested {
                        let border = node.style.decor.used_border();
                        heights.heights[index].and_then(|height| {
                            height
                                .checked_add(node.style.padding_top.checked_sub(border[0])?)?
                                .checked_add(node.style.padding_bottom.checked_sub(border[2])?)
                        })
                    } else {
                        Some(viewport_height)
                    };
                    let cb_y = if nested {
                        let border = node.style.decor.used_border();
                        result.content_y[index].and_then(|y| {
                            y.checked_sub(i64::from(node.style.padding_top.checked_sub(border[0])?))
                        })
                    } else {
                        Some(0)
                    };
                    let [top, _, bottom, _] = style.positioned.offsets;
                    let border_y = match (top, bottom) {
                        (Margin::Px(top), _) => cb_y.and_then(|y| y.checked_add(i64::from(top))),
                        (Margin::Auto, Margin::Px(bottom)) => outer.and_then(|h| {
                            cb_y?
                                .checked_add(i64::from(cb_height?))?
                                .checked_sub(i64::from(bottom))?
                                .checked_sub(h)
                        }),
                        (Margin::Auto, Margin::Auto) => static_y,
                        _ => None,
                    };
                    result.content_y[child] =
                        border_y.and_then(|y| y.checked_add(i64::from(style.padding_top)));
                    continue;
                }
                let top = tree.collapsed_margin(child, true, &widths, viewport_width);
                let gap = previous.map_or_else(
                    || {
                        // The first child's margin sits inside a box that
                        // cannot share it: the root, or one with top padding.
                        if node.parent.is_none() || node.style.padding_top != 0 || tree.is_flex_root(index) {
                            top
                        } else {
                            0
                        }
                    },
                    |prev| {
                        collapse_adjoining_margins(&[
                            tree.collapsed_margin(prev, false, &widths, viewport_width),
                            top,
                        ])
                    },
                );
                let baseline = previous.map_or(result.content_y[index], |prev| {
                    result.content_y[prev].and_then(|y| {
                        heights.heights[prev].and_then(|h| {
                            y.checked_add(i64::from(h)).and_then(|bottom| {
                                bottom.checked_add(i64::from(tree.boxes[prev].style.padding_bottom))
                            })
                        })
                    })
                });
                result.content_y[child] = baseline.and_then(|y| {
                    y.checked_add(gap).and_then(|top| {
                        top.checked_add(i64::from(tree.boxes[child].style.padding_top))
                    })
                });
                previous = Some(child);
            }
            for &child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        if result.content_y.iter().any(Option::is_none) {
            result.content_y.fill(None);
            result.unsupported = true;
        }
        result
    }
}

/// A block whose children form one pure inline formatting context: every
/// child is a text node or an inline element. Mixed inline/block content is
/// wrapped in anonymous blocks during tree construction, which the used
/// passes still refuse, so this shape is the only direct-text block form.
fn pure_inline_children(tree: &BoxTree, owner: usize) -> bool {
    tree.boxes.get(owner).is_some_and(|node| {
        matches!(node.kind, BoxKind::Block | BoxKind::ListItem)
            && !node.children.is_empty()
            && node
                .children
                .iter()
                .all(|&child| matches!(tree.boxes[child].kind, BoxKind::Text | BoxKind::Inline))
    })
}

/// Nearest block owner of bare inline ancestry. Full styles are validated by the collector.
pub(crate) fn inline_owner(tree: &BoxTree, index: usize) -> Option<usize> {
    let mut at = tree.boxes.get(index)?.parent;
    let mut depth = 0;
    while let Some(i) = at {
        depth += 1;
        if depth > 97 {
            return None;
        }
        let n = tree.boxes.get(i)?;
        if pure_inline_children(tree, i) {
            return Some(i);
        }
        if n.kind != BoxKind::Inline {
            return None;
        }
        at = n.parent;
    }
    None
}

/// A width-only pass. `None` means an unknown width, not a paintable box.
/// Height, y position, and line widths are still missing. The reader remains
/// the only user-facing renderer.
pub struct WidthPass {
    pub widths: Vec<Option<UsedBlockWidth>>,
    /// Content-box x in viewport pixels, only on verified block paths.
    pub content_x: Vec<Option<i64>>,
    pub unsupported: bool,
    pub truncated: bool,
}

impl WidthPass {
    /// A nested absolute leaf is contained by its direct positioned parent's
    /// padding box. Outer leaves retain the initial viewport containing block.
    fn positioned_containing_width(
        &self,
        tree: &BoxTree,
        index: usize,
        viewport: u32,
    ) -> (Option<u32>, Option<i64>) {
        let parent = tree.boxes[index]
            .parent
            .filter(|&parent| tree.boxes[parent].style.positioned.position == Position::Absolute);
        let Some(parent) = parent else {
            return (Some(viewport), Some(0));
        };
        let style = tree.boxes[parent].style;
        let border = style.decor.used_border();
        let width = self.widths[parent].and_then(|w| {
            w.content
                .checked_add(style.padding_left.checked_sub(border[3])?)?
                .checked_add(style.padding_right.checked_sub(border[1])?)
        });
        let x = self.content_x[parent]
            .and_then(|x| x.checked_sub(i64::from(style.padding_left.checked_sub(border[3])?)));
        (width, x)
    }

    /// Compute block content widths top-down. Inline formatting does not
    /// supply a known x; an inline-block needs shrink-to-fit and cuts off
    /// its descendants' containing width as well.
    #[must_use]
    pub fn from_boxes(tree: &BoxTree, viewport_width: u32) -> Self {
        let mut result = Self {
            widths: vec![None; tree.boxes.len()],
            content_x: vec![None; tree.boxes.len()],
            unsupported: tree.unsupported || tree.stray_min_max_width(),
            truncated: tree.truncated,
        };
        let mut stack: Vec<_> = tree
            .roots
            .iter()
            .rev()
            .map(|&root| (root, Some(viewport_width), Some(0_i64)))
            .collect();
        let mut visited = 0_usize;
        while let Some((index, containing, containing_x)) = stack.pop() {
            visited += 1;
            if visited > tree.boxes.len() {
                result.truncated = true;
                break;
            }
            let node = &tree.boxes[index];
            if node.kind == BoxKind::Block
                && node.parent.is_some_and(|p| tree.boxes[p].kind == BoxKind::Flex)
            {
                // Flex item: width and x were assigned by its container.
                let (w, x) = (result.widths[index].map(|w| w.content), result.content_x[index]);
                for &child in node.children.iter().rev() {
                    stack.push((child, w, x));
                }
                continue;
            }
            let (content, x) = match node.kind {
                BoxKind::Block | BoxKind::ListItem | BoxKind::AnonymousBlock | BoxKind::Flex => {
                    let width = containing.map(|base| {
                        if node.kind == BoxKind::AnonymousBlock {
                            used_block_width(Length::Auto, base)
                        } else {
                            used_block_width_sized(
                                node.style.width,
                                base,
                                [node.style.padding_left, node.style.padding_right],
                                [
                                    resolve_margin(node.style.margin_left, base),
                                    resolve_margin(node.style.margin_right, base),
                                ],
                                node.parent.is_some_and(|p| {
                                    tree.boxes[p].style.direction == Direction::Rtl
                                }),
                                node.style.box_sizing,
                            )
                        }
                    });
                    let (cb_width, cb_x) =
                        result.positioned_containing_width(tree, index, viewport_width);
                    let width = if node.style.positioned.position == Position::Absolute {
                        cb_width.map(|cb| {
                            used_block_width_sized(
                                node.style.width,
                                cb,
                                [node.style.padding_left, node.style.padding_right],
                                [Some(0), Some(0)],
                                false,
                                node.style.box_sizing,
                            )
                        })
                    } else {
                        width
                    };
                    let width = tree.zero_positioned_width(index).or(width);
                    result.widths[index] = width;
                    let x = containing_x.and_then(|parent_x| {
                        width.and_then(|w| {
                            parent_x.checked_add(w.margin_left).and_then(|left| {
                                left.checked_add(i64::from(node.style.padding_left))
                            })
                        })
                    });
                    let x = if node.style.positioned.position == Position::Absolute {
                        width.and_then(|w| {
                            let [_, right, _, left] = node.style.positioned.offsets;
                            let outer = i64::from(w.content)
                                + i64::from(node.style.padding_left)
                                + i64::from(node.style.padding_right);
                            let border_x = match (left, right) {
                                (Margin::Px(left), _) => cb_x?.checked_add(i64::from(left)),
                                (Margin::Auto, Margin::Px(right)) => cb_x?
                                    .checked_add(i64::from(cb_width?))?
                                    .checked_sub(i64::from(right))?
                                    .checked_sub(outer),
                                (Margin::Auto, Margin::Auto) => containing_x,
                                _ => None,
                            };
                            border_x.and_then(|n| n.checked_add(i64::from(node.style.padding_left)))
                        })
                    } else {
                        x
                    };
                    result.content_x[index] = x;
                    if node.kind == BoxKind::Flex {
                        let items = width
                            .zip(x)
                            .and_then(|(w, x)| tree.flex_row_items(index, w.content).map(|i| (i, x)));
                        if let Some((items, x)) = items {
                            for (item, used, offset) in items {
                                result.widths[item] = Some(used);
                                result.content_x[item] = x.checked_add(offset);
                            }
                        } else {
                            result.unsupported = true;
                        }
                    }
                    (width.map(|w| w.content), x)
                }
                BoxKind::InlineBlock => {
                    result.unsupported = true;
                    (None, None)
                }
                BoxKind::Inline | BoxKind::Text => (containing, None),
            };
            for &child in node.children.iter().rev() {
                stack.push((child, content, x));
            }
        }
        result
    }
}

// Retain the styled DOM; omit only block indentation proven to have no ink.
fn collapsed_block_indentation(styled: &StyleTree) -> Vec<bool> {
    // In normal white-space, a block-only parent's indentation does not
    // generate anonymous line boxes. Keep the styled DOM unchanged and
    // omit only runs proven to contain CSS whitespace and no inline ink.
    // Unknown white-space declarations already mark the tree unsupported.
    let mut collapsed = vec![false; styled.nodes.len()];
    for owner in &styled.nodes {
        if !matches!(owner.style.display, Display::Block | Display::ListItem | Display::Flex) {
            continue;
        }
        let mut has_block = false;
        let mut only_indentation = true;
        for &child in &owner.children {
            let node = &styled.nodes[child];
            if node.text.is_none() && node.style.display == Display::None {
                continue;
            }
            if node.text.is_none()
                && matches!(node.style.display, Display::Block | Display::ListItem | Display::Flex)
            {
                has_block = true;
            } else if !node.text.as_deref().is_some_and(|text| {
                text.chars()
                    .all(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r'))
            }) {
                only_indentation = false;
            }
        }
        if has_block && only_indentation {
            for &child in &owner.children {
                collapsed[child] = styled.nodes[child].text.is_some();
            }
        }
    }
    collapsed
}

impl BoxTree {
    /// Only wrappers generated by mixed-content normalization may lack a DOM
    /// source. Their inherited style, ownership and inline-only body are exact;
    /// this is not permission to paint anonymous text or invent hit targets.
    fn anonymous_wrapper_valid(&self, index: usize) -> bool {
        let node = &self.boxes[index];
        let Some(parent) = node.parent.and_then(|p| self.boxes.get(p)) else {
            return false;
        };
        let expected = Computed {
            display: Display::Block,
            ..Computed::cascade(Some(parent.style), &[])
        };
        node.kind == BoxKind::AnonymousBlock
            && node.source.is_none()
            && matches!(parent.kind, BoxKind::Block | BoxKind::ListItem)
            && !node.inline_phrase
            && !node.inline_attributes
            && node.text.is_none()
            && node.style == expected
            && !node.children.is_empty()
            && node.children.iter().all(|&child| {
                self.boxes.get(child).is_some_and(|child| {
                    matches!(
                        child.kind,
                        BoxKind::Inline | BoxKind::InlineBlock | BoxKind::Text
                    )
                })
            })
    }

    /// Canonicalize the generated arena after appended wrappers reparent earlier
    /// children. Prove a bounded connected tree BEFORE any index or mutation.
    /// Source identities and sibling order remain unchanged. Failure is atomic.
    fn normalize_preorder(&mut self, limit: usize) -> bool {
        let count = self.boxes.len();
        if count == 0 {
            return self.roots.is_empty();
        }
        if count > limit || self.roots.len() != 1 {
            return false;
        }
        let mut edges = 0_usize;
        for node in &self.boxes {
            let Some(total) = edges.checked_add(node.children.len()) else {
                return false;
            };
            edges = total;
            if edges > limit
                || node.parent.is_some_and(|p| p >= count)
                || node.children.iter().any(|&c| c >= count)
            {
                return false;
            }
        }
        let mut order = Vec::with_capacity(count);
        let mut seen = vec![false; count];
        let mut stack = vec![(self.roots[0], None)];
        while let Some((index, parent)) = stack.pop() {
            let Some(node) = self.boxes.get(index) else {
                return false;
            };
            if seen[index] || node.parent != parent {
                return false;
            }
            seen[index] = true;
            order.push(index);
            for &child in node.children.iter().rev() {
                stack.push((child, Some(index)));
            }
        }
        if order.len() != count {
            return false;
        }
        if order.iter().enumerate().all(|(new, &old)| new == old) {
            return true;
        }
        let mut mapping = vec![0; count];
        for (new, &old) in order.iter().enumerate() {
            mapping[old] = new;
        }
        // All fallible graph checks are complete. Move boxes without cloning
        // text/style data; no duplicate, missing or invalid index can occur.
        let mut old: Vec<_> = std::mem::take(&mut self.boxes)
            .into_iter()
            .map(Some)
            .collect();
        self.boxes = order
            .into_iter()
            .map(|index| {
                let mut node = old[index].take().expect("validated unique preorder");
                node.parent = node.parent.map(|p| mapping[p]);
                for child in &mut node.children {
                    *child = mapping[*child];
                }
                node
            })
            .collect();
        self.roots[0] = mapping[self.roots[0]];
        true
    }

    /// Check the retained preorder arena before any diagnostic pass indexes it.
    /// Paint paths accept one connected tree, with bounded edges and sources.
    /// art84 scratch: true when a box carries min/max-width but is not a direct child of a Flex box
    /// (the only position the flex solver models). Used by the pass constructors so direct callers fail closed.
    pub(crate) fn stray_min_max_width(&self) -> bool {
        self.boxes.iter().any(|node| {
            (node.style.min_width != Length::Auto || node.style.max_width != Length::Auto)
                && !node.parent.is_some_and(|p| self.boxes.get(p).is_some_and(|b| b.kind == BoxKind::Flex))
        })
    }

    pub(crate) fn paint_structure_valid(&self, limit: usize) -> bool {
        let count = self.boxes.len();
        if count == 0 || count > limit || self.roots.len() != 1 || self.roots[0] >= count {
            return false;
        }
        let mut edges = 0_usize;
        for (index, node) in self.boxes.iter().enumerate() {
            edges = edges.saturating_add(node.children.len());
            if edges > limit
                || (node.kind == BoxKind::AnonymousBlock && !self.anonymous_wrapper_valid(index))
                || (node.kind != BoxKind::AnonymousBlock && node.source.is_none())
                || node.parent.is_some_and(|parent| parent >= index)
                || ((node.style.min_width != Length::Auto || node.style.max_width != Length::Auto)
                    && !node.parent.is_some_and(|p| self.boxes[p].kind == BoxKind::Flex))
                || node
                    .children
                    .iter()
                    .any(|&child| child <= index || child >= count)
            {
                return false;
            }
        }
        let mut seen = vec![false; count];
        let mut stack = vec![(self.roots[0], None)];
        while let Some((index, parent)) = stack.pop() {
            let node = &self.boxes[index];
            if seen[index] || node.parent != parent {
                return false;
            }
            seen[index] = true;
            for &child in &node.children {
                stack.push((child, Some(index)));
            }
        }
        seen.into_iter().all(|visited| visited)
    }

    /// Convert a bounded styled DOM into boxes without painting or flattening
    /// mixed text. At most two boxes per styled node are allocated.
    #[must_use]
    #[allow(clippy::too_many_lines)] // Bounded box construction and positioned-leaf validation.
    pub fn from_style(styled: &StyleTree) -> Self {
        let mut tree = Self {
            truncated: styled.truncated,
            unsupported: styled.unsupported,
            quirks: styled.quirks,
            ..Self::default()
        };
        let limit = styled.nodes.len().saturating_mul(2);
        let collapsed = collapsed_block_indentation(styled);
        let mut stack: Vec<_> = styled
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.parent.is_none())
            .map(|(index, _)| (index, None))
            .rev()
            .collect();
        while let Some((source, parent)) = stack.pop() {
            if collapsed[source] {
                continue;
            }
            let node = &styled.nodes[source];
            let kind = if node.text.is_some() {
                Some(BoxKind::Text)
            } else {
                match node.style.display {
                    Display::None => None,
                    Display::Block => Some(BoxKind::Block),
                    Display::Inline => Some(BoxKind::Inline),
                    Display::InlineBlock => Some(BoxKind::InlineBlock),
                    Display::ListItem => Some(BoxKind::ListItem),
                    Display::Flex => Some(BoxKind::Flex),
                }
            };
            let Some(kind) = kind else {
                continue; // display:none removes the whole subtree.
            };
            if tree.boxes.len() >= limit {
                tree.truncated = true;
                break;
            }
            let index = tree.boxes.len();
            // Every pass below sees one edge per side: padding plus border.
            // The border widths stay in `style.decor` for painting.
            let mut style = node.style;
            if node.text.is_some() {
                style.positioned = crate::computed_style::Positioned::INITIAL;
            }
            let [top, right, bottom, left] = style.decor.used_border();
            style.padding_top = style.padding_top.saturating_add(top);
            style.padding_right = style.padding_right.saturating_add(right);
            style.padding_bottom = style.padding_bottom.saturating_add(bottom);
            style.padding_left = style.padding_left.saturating_add(left);
            tree.boxes.push(CssBox {
                inline_phrase: node.inline_phrase,
                inline_attributes: node.inline_attributes,
                kind,
                source: Some(source),
                parent,
                children: Vec::new(),
                style,
                text: node.text.clone(),
            });
            if let Some(parent) = parent {
                tree.boxes[parent].children.push(index);
            } else {
                tree.roots.push(index);
            }
            for &child in node.children.iter().rev() {
                stack.push((child, Some(index)));
            }
        }
        // Positioned support has definite sizes, zero margins and LTR.
        // One outer positioned box may contain direct empty positioned leaves;
        // no normal-flow children, third level, shrink-to-fit or stacking context.
        // Inner leaves require explicit offsets on both axes. Overflow remains
        // visible, clipped only at the final viewport; negative integer offsets
        // are allowed. Checked geometry refuses unrepresentable coordinates.
        for (index, node) in tree.boxes.iter().enumerate() {
            if node.style.positioned.position != Position::Absolute {
                continue;
            }
            if node.kind != BoxKind::Block
                || node.children.iter().any(|&child| {
                    tree.boxes[child].style.positioned.position != Position::Absolute
                        || !tree.boxes[child].children.is_empty()
                })
                || node.parent.is_none()
                || (!tree.zero_positioned_container(index)
                    && (!matches!(node.style.width, Length::Px(_))
                        || !matches!(node.style.height, Length::Px(_))))
                || node.style.direction != Direction::Ltr
                || [
                    node.style.margin_top,
                    node.style.margin_right,
                    node.style.margin_bottom,
                    node.style.margin_left,
                ]
                .iter()
                .any(|m| *m != Margin::Px(0))
            {
                tree.unsupported = true;
            }
            let nested = node
                .parent
                .is_some_and(|p| tree.boxes[p].style.positioned.position == Position::Absolute);
            if nested {
                let [top, right, bottom, left] = node.style.positioned.offsets;
                if !node.children.is_empty()
                    || !matches!(
                        (left, right),
                        (Margin::Px(_), _) | (Margin::Auto, Margin::Px(_))
                    )
                    || !matches!(
                        (top, bottom),
                        (Margin::Px(_), _) | (Margin::Auto, Margin::Px(_))
                    )
                {
                    tree.unsupported = true;
                }
            }
            let mut parent = node.parent;
            let mut depth = 0;
            while let Some(p) = parent {
                depth += 1;
                if p >= index
                    || depth > tree.boxes.len()
                    || (tree.boxes[p].style.positioned.position != Position::Static
                        && !(depth == 1 && nested))
                    || tree.boxes[p].style.direction != Direction::Ltr
                {
                    tree.unsupported = true;
                    break;
                }
                parent = tree.boxes[p].parent;
            }
        }
        // Iterate only the original boxes. Anonymous wrappers append later and
        // must not themselves be normalized again.
        let originals = tree.boxes.len();
        for parent in (0..originals).rev() {
            let kind = tree.boxes[parent].kind;
            let children = &tree.boxes[parent].children;
            let has_block = children
                .iter()
                .any(|&child| matches!(tree.boxes[child].kind, BoxKind::Block | BoxKind::ListItem | BoxKind::Flex));
            if kind == BoxKind::Inline && has_block {
                // CSS 2.1 §9.2.1.1 requires splitting inline boxes. Do not
                // mislabel this partial tree as a valid used box tree.
                tree.unsupported = true;
            }
            if !matches!(kind, BoxKind::Block | BoxKind::ListItem) || !has_block {
                continue;
            }
            let old = std::mem::take(&mut tree.boxes[parent].children);
            let mut run = Vec::new();
            for child in old {
                if matches!(tree.boxes[child].kind, BoxKind::Block | BoxKind::ListItem | BoxKind::Flex) {
                    tree.wrap_inline_run(parent, &mut run, limit);
                    tree.boxes[parent].children.push(child);
                } else {
                    run.push(child);
                }
            }
            tree.wrap_inline_run(parent, &mut run, limit);
        }
        if !tree.normalize_preorder(limit) {
            tree.unsupported = true;
        }
        tree
    }

    fn wrap_inline_run(&mut self, parent: usize, run: &mut Vec<usize>, limit: usize) {
        if run.is_empty() {
            return;
        }
        if self.boxes.len() >= limit {
            self.truncated = true;
            // Keep surviving children reachable even when a wrapper won't fit.
            self.boxes[parent].children.append(run);
            return;
        }
        let index = self.boxes.len();
        // Anonymous blocks inherit inherited properties (color, direction),
        // not their parent's non-inherited margins, width, or sizing.
        let style = Computed {
            display: Display::Block,
            ..Computed::cascade(Some(self.boxes[parent].style), &[])
        };
        for &child in run.iter() {
            self.boxes[child].parent = Some(index);
        }
        self.boxes.push(CssBox {
            inline_phrase: false,
            inline_attributes: false,
            kind: BoxKind::AnonymousBlock,
            source: None,
            parent: Some(parent),
            children: std::mem::take(run),
            style,
            text: None,
        });
        self.boxes[parent].children.push(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse_style_tree, Limits};

    /// Parses `html` with the user-agent body margin removed, so geometry
    /// tests state every margin they depend on.
    fn styled(html: &str) -> crate::style_tree::StyleTree {
        let mut tree = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        for node in tree.nodes.iter_mut().filter(|node| node.tag == "body") {
            node.style.margin_top = Margin::Px(0);
            node.style.margin_right = Margin::Px(0);
            node.style.margin_bottom = Margin::Px(0);
            node.style.margin_left = Margin::Px(0);
        }
        tree
    }

    fn boxes(html: &str) -> BoxTree {
        BoxTree::from_style(&styled(html))
    }

    #[test]
    fn definite_height_does_not_guess_auto_or_indefinite_percent() {
        assert_eq!(specified_block_height(Length::Px(80), None), Some(80));
        assert_eq!(specified_block_height(Length::Auto, Some(300)), None);
        assert_eq!(specified_block_height(Length::Percent(2500), None), None);
        assert_eq!(
            specified_block_height(Length::Percent(2500), Some(300)),
            Some(75)
        );
        assert_eq!(
            specified_block_height(Length::Percent(10_000), Some(u32::MAX)),
            Some(u32::MAX)
        );
        let tree = boxes("<html style='height:400px'><body style='height:50%'><main style='height:25%'>a</main><aside><div style='height:20%'>b</div></aside></body></html>");
        let pass = HeightPass::from_boxes(&tree, 600);
        let styled = parse_style_tree(b"<html style='height:400px'><body style='height:50%'><main style='height:25%'>a</main><aside><div style='height:20%'>b</div></aside></body></html>", &[], &Limits::DEFAULT);
        let height = |tag: &str| {
            tree.boxes
                .iter()
                .enumerate()
                .find(|(_, b)| {
                    b.source
                        .is_some_and(|source| styled.nodes[source].tag == tag)
                })
                .and_then(|(index, _)| pass.heights[index])
        };
        assert_eq!(height("html"), Some(400));
        assert_eq!(height("body"), Some(200));
        assert_eq!(height("main"), Some(50));
        assert_eq!(height("aside"), None);
        assert_eq!(height("div"), None);
        assert!(!pass.truncated);
    }

    #[test]
    fn measured_text_height_does_not_turn_background_only_pass_into_text_paint() {
        let tree = boxes(
            "<html style='height:100px'><body><p style='font-size:20px'>ab cd</p></body></html>",
        );
        let measured = UsedHeightPass::from_boxes_with_direct_text(
            &tree,
            100,
            100,
            |_, size, _| Some(size / 2),
            |size| Some(size + 4),
        );
        let paragraph = tree
            .boxes
            .iter()
            .position(|box_| box_.kind == BoxKind::Block && box_.style.font_size == 20)
            .unwrap();
        let body = tree.boxes[paragraph].parent.unwrap();
        assert_eq!(measured.heights[paragraph], Some(24));
        assert_eq!(measured.heights[body], Some(24));
        assert!(!measured.unsupported);
        assert!(UsedHeightPass::from_boxes(&tree, 100, 100).unsupported);
        let mixed = boxes("<html style='height:100px'><body><p>ab <em>cd</em></p></body></html>");
        assert!(
            UsedHeightPass::from_boxes_with_direct_text(
                &mixed,
                100,
                100,
                |_, _, _| Some(8),
                |_| Some(20)
            )
            .unsupported
        );
        let unknown_font = UsedHeightPass::from_boxes_with_direct_text(
            &tree,
            100,
            100,
            |_, _, _| None,
            |_| Some(20),
        );
        assert!(unknown_font.unsupported);
    }

    #[test]
    fn measured_direct_text_receives_content_origin_without_background_permission() {
        let tree = boxes("<html style='height:100px;padding-top:2px'><body style='padding-top:3px'><p style='font-size:20px;padding-top:4px;margin:0'>ab cd</p></body></html>");
        let vertical = VerticalPass::from_boxes_with_direct_text(
            &tree,
            100,
            100,
            |_, size, _| Some(size / 2),
            |size| Some(size + 4),
        );
        let p = tree
            .boxes
            .iter()
            .position(|node| node.kind == BoxKind::Block && node.style.font_size == 20)
            .unwrap();
        let text = tree.boxes[p].children[0];
        assert_eq!(vertical.content_y[p], Some(9));
        assert_eq!(vertical.content_y[text], Some(9));
        assert!(!vertical.unsupported);
        assert!(VerticalPass::from_boxes(&tree, 100, 100).unsupported);
    }

    #[test]
    fn vertical_pass_positions_only_definite_block_subtrees() {
        let html = "<html style='height:600px'><body style='height:400px'><main style='height:100px'><section style='height:10px;margin-bottom:20px'></section><article style='height:30px;margin-top:15px'></article></main></body></html>";
        let styled = styled(html);
        let tree = BoxTree::from_style(&styled);
        let pass = VerticalPass::from_boxes(&tree, 400, 600);
        assert!(!pass.unsupported);
        let y = |tag: &str| {
            tree.boxes
                .iter()
                .enumerate()
                .find(|(_, b)| {
                    b.source
                        .is_some_and(|source| styled.nodes[source].tag == tag)
                })
                .and_then(|(index, _)| pass.content_y[index])
        };
        assert_eq!(y("html"), Some(0));
        assert_eq!(y("body"), Some(0));
        assert_eq!(y("main"), Some(0));
        assert_eq!(y("section"), Some(0));
        assert_eq!(y("article"), Some(30));
    }

    #[test]
    fn vertical_padding_moves_content_and_siblings_without_margin_collapse() {
        let html = "<html style='height:300px;padding-top:7px'><body style='height:200px;padding-top:11px;padding-bottom:13px'><main style='height:30px;padding-top:5px;padding-bottom:3px;margin-bottom:9px'></main><section style='height:20px;padding-top:4px;margin-top:6px'></section></body></html>";
        let styled = styled(html);
        let tree = BoxTree::from_style(&styled);
        let pass = VerticalPass::from_boxes(&tree, 400, 600);
        assert!(!pass.unsupported);
        let y = |tag: &str| {
            tree.boxes
                .iter()
                .enumerate()
                .find(|(_, b)| {
                    b.source
                        .is_some_and(|source| styled.nodes[source].tag == tag)
                })
                .and_then(|(index, _)| pass.content_y[index])
        };
        assert_eq!(y("html"), Some(7));
        assert_eq!(y("body"), Some(18));
        assert_eq!(y("main"), Some(23));
        assert_eq!(y("section"), Some(69)); // 23 + 30 + 3 + max(9, 6) + 4
    }

    #[test]
    fn border_box_height_subtracts_vertical_padding_without_underflow() {
        let html = "<html style='height:100px'><body style='height:50%;box-sizing:border-box;padding-top:12px;padding-bottom:13px'><main style='height:50%'></main></body></html>";
        let styled = styled(html);
        let tree = BoxTree::from_style(&styled);
        let pass = HeightPass::from_boxes(&tree, 100);
        let height = |tag: &str| {
            tree.boxes
                .iter()
                .enumerate()
                .find(|(_, b)| {
                    b.source
                        .is_some_and(|source| styled.nodes[source].tag == tag)
                })
                .and_then(|(index, _)| pass.heights[index])
        };
        assert_eq!(height("body"), Some(25));
        assert_eq!(height("main"), Some(12));
        assert!(!VerticalPass::from_boxes(&tree, 100, 100).unsupported);
    }

    #[test]
    fn root_children_keep_their_top_margin_and_nested_first_margins_collapse() {
        // The root element never collapses with its children.
        let tree = boxes("<html style='height:600px'><body style='height:400px'><main style='height:30px;margin-top:10px'></main></body></html>");
        let pass = VerticalPass::from_boxes(&tree, 400, 600);
        assert!(!pass.unsupported);
        // html, body, main: the body has no padding, so main's margin and
        // the body's (zero here) are shared and both start at y = 10.
        assert_eq!(pass.content_y, vec![Some(0), Some(10), Some(10)]);
        // The larger of two touching margins wins.
        let tree = boxes("<html style='height:600px'><body style='height:400px'><main style='margin-top:10px'><div style='margin-top:25px;height:30px'></div></main></body></html>");
        let pass = VerticalPass::from_boxes(&tree, 400, 600);
        assert!(!pass.unsupported);
        assert_eq!(pass.content_y[2], Some(25));
        assert_eq!(pass.content_y[3], Some(25));
        // With top padding the two margins stay apart.
        let tree = boxes("<html style='height:600px'><body style='height:400px'><main style='margin-top:10px;padding-top:4px'><div style='margin-top:25px;height:30px'></div></main></body></html>");
        let pass = VerticalPass::from_boxes(&tree, 400, 600);
        assert!(!pass.unsupported);
        assert_eq!(pass.content_y[2], Some(14));
        assert_eq!(pass.content_y[3], Some(39));
    }

    #[test]
    fn vertical_pass_rejects_unknown_height_and_parent_child_collapse() {
        for html in [
            "<html style='height:600px'><body style='height:400px'>text</body></html>",
            "<html style='height:600px'><body style='height:400px'><div style='height:0'></div></body></html>",
        ] {
            let tree = boxes(html);
            let pass = VerticalPass::from_boxes(&tree, 400, 600);
            assert!(pass.unsupported, "{html}");
        }
    }

    #[test]
    fn auto_block_height_contains_definite_children_but_not_indefinite_percent() {
        let html = "<html style='height:600px'><body style='padding-top:7px;padding-bottom:3px'><main style='height:20px;padding-top:2px;padding-bottom:3px;margin-bottom:5px'></main><section style='height:10px;margin-top:8px'></section></body></html>";
        let styled = styled(html);
        let tree = BoxTree::from_style(&styled);
        let height = UsedHeightPass::from_boxes(&tree, 400, 600);
        let vertical = VerticalPass::from_boxes(&tree, 400, 600);
        let body = tree
            .boxes
            .iter()
            .enumerate()
            .find(|(_, b)| b.source.is_some_and(|i| styled.nodes[i].tag == "body"))
            .unwrap()
            .0;
        let section = tree
            .boxes
            .iter()
            .enumerate()
            .find(|(_, b)| b.source.is_some_and(|i| styled.nodes[i].tag == "section"))
            .unwrap()
            .0;
        assert_eq!(height.heights[body], Some(43)); // 20 + 2 + 3 + max(5, 8) + 10
        assert_eq!(vertical.content_y[body], Some(7));
        assert_eq!(vertical.content_y[section], Some(40));
        assert!(!height.unsupported && !vertical.unsupported);

        let tree = boxes(
            "<html style='height:600px'><body><main style='height:50%'></main></body></html>",
        );
        assert!(UsedHeightPass::from_boxes(&tree, 400, 600).unsupported);
        assert!(VerticalPass::from_boxes(&tree, 400, 600).unsupported);
    }

    #[test]
    fn vertical_percentage_margin_uses_containing_width_and_auto_zero() {
        assert_eq!(resolve_vertical_margin(Margin::Percent(1000), 300), 30);
        assert_eq!(resolve_vertical_margin(Margin::Percent(-2500), 200), -50);
        assert_eq!(resolve_vertical_margin(Margin::Auto, 300), 0);
    }

    #[test]
    fn adjoining_margin_equation_handles_positive_negative_and_zero() {
        assert_eq!(collapse_adjoining_margins(&[12, 30, 5]), 30);
        assert_eq!(collapse_adjoining_margins(&[-12, -30, -5]), -30);
        assert_eq!(collapse_adjoining_margins(&[20, -8, 10]), 12);
        assert_eq!(collapse_adjoining_margins(&[0, 0]), 0);
        assert_eq!(collapse_adjoining_margins(&[i64::MAX, i64::MIN]), -1);
    }

    #[test]
    fn definite_sibling_stack_stops_at_first_unknown() {
        assert_eq!(
            stack_definite_siblings(
                100,
                &[
                    (Some(40), Some(10), Some(20)),
                    (Some(30), Some(15), Some(-5)),
                    (Some(12), Some(-10), Some(8)),
                ]
            ),
            [Some(110), Some(170), Some(190)]
        );
        assert_eq!(
            stack_definite_siblings(
                0,
                &[
                    (Some(20), Some(0), Some(0)),
                    (None, Some(0), Some(0)),
                    (Some(20), Some(0), Some(0)),
                ]
            ),
            [Some(0), Some(20), None]
        );
        assert_eq!(
            stack_definite_siblings(i64::MAX, &[(Some(1), Some(1), Some(0))]),
            [None]
        );
    }

    #[test]
    fn border_box_width_excludes_nonnegative_edges() {
        assert_eq!(
            used_block_width_sized(
                Length::Px(100),
                300,
                [8, 12],
                [Some(0), Some(0)],
                false,
                BoxSizing::BorderBox
            ),
            UsedBlockWidth {
                content: 80,
                margin_left: 0,
                margin_right: 200
            }
        );
        assert_eq!(
            used_block_width_sized(
                Length::Percent(5000),
                300,
                [8, 12],
                [None, None],
                false,
                BoxSizing::BorderBox
            ),
            UsedBlockWidth {
                content: 130,
                margin_left: 75,
                margin_right: 75
            }
        );
        assert_eq!(
            used_block_width_sized(
                Length::Px(5),
                300,
                [8, 12],
                [Some(0), Some(0)],
                false,
                BoxSizing::BorderBox
            )
            .content,
            0
        );
        assert_eq!(
            used_block_width_sized(
                Length::Auto,
                300,
                [8, 12],
                [Some(0), Some(0)],
                false,
                BoxSizing::BorderBox
            )
            .content,
            280
        );
    }

    #[test]
    fn block_width_edges_auto_margins_and_direction() {
        let result = used_block_width_edges(Length::Auto, 300, 10, 10, None, Some(20), false);
        assert_eq!(
            result,
            UsedBlockWidth {
                content: 260,
                margin_left: 0,
                margin_right: 20
            }
        );
        let centered = used_block_width_edges(Length::Px(100), 301, 10, 10, None, None, false);
        assert_eq!(
            centered,
            UsedBlockWidth {
                content: 100,
                margin_left: 90,
                margin_right: 91
            }
        );
        let rtl = used_block_width_edges(Length::Px(100), 301, 10, 10, Some(20), Some(30), true);
        assert_eq!(
            rtl,
            UsedBlockWidth {
                content: 100,
                margin_left: 151,
                margin_right: 30
            }
        );
        let overflow = used_block_width_edges(Length::Px(350), 300, 0, 0, None, None, false);
        assert_eq!(
            overflow,
            UsedBlockWidth {
                content: 350,
                margin_left: 0,
                margin_right: -50
            }
        );
        let rtl_overflow = used_block_width_edges(Length::Px(350), 300, 0, 0, None, None, true);
        assert_eq!(
            rtl_overflow,
            UsedBlockWidth {
                content: 350,
                margin_left: -50,
                margin_right: 0
            }
        );
        let negative_auto = used_block_width_edges(Length::Auto, 10, 20, 20, Some(5), None, false);
        assert_eq!(
            negative_auto,
            UsedBlockWidth {
                content: 0,
                margin_left: 5,
                margin_right: -35
            }
        );
    }

    #[test]
    fn css_2_2_block_width_with_zero_edges() {
        assert_eq!(
            used_block_width(Length::Auto, 300),
            UsedBlockWidth {
                content: 300,
                margin_left: 0,
                margin_right: 0
            }
        );
        assert_eq!(
            used_block_width(Length::Px(200), 301),
            UsedBlockWidth {
                content: 200,
                margin_left: 0,
                margin_right: 101
            }
        );
        assert_eq!(
            used_block_width(Length::Percent(2500), 400),
            UsedBlockWidth {
                content: 100,
                margin_left: 0,
                margin_right: 300
            }
        );
        assert_eq!(
            used_block_width(Length::Px(360), 300),
            UsedBlockWidth {
                content: 360,
                margin_left: 0,
                margin_right: -60
            }
        );
        assert_eq!(
            used_block_width(Length::Percent(10_000), u32::MAX).content,
            u32::MAX
        );
    }

    #[test]
    fn widths_follow_nearest_block_content_width() {
        let tree = boxes("<style>body{width:50%}section{width:25%}</style><body><section><div>Text</div></section></body>");
        let used = WidthPass::from_boxes(&tree, 400);
        let blocks: Vec<_> = tree
            .boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.kind == BoxKind::Block)
            .map(|(i, _)| used.widths[i].unwrap().content)
            .collect();
        assert_eq!(blocks, [400, 200, 50, 50]); // html, body, section, div
        assert!(!used.unsupported);
        assert!(!used.truncated);
        assert_eq!(used.widths.len(), tree.boxes.len());
    }

    #[test]
    fn padding_changes_content_width_and_x_without_stealing_margin() {
        let tree = boxes("<div style='width:100px;padding-left:10px;padding-right:20px;box-sizing:border-box'><p style='width:50%'></p></div>");
        let widths = WidthPass::from_boxes(&tree, 200);
        let outer = tree
            .boxes
            .iter()
            .position(|b| b.style.padding_left == 10)
            .unwrap();
        let child = tree.boxes[outer].children[0];
        assert_eq!(widths.widths[outer].unwrap().content, 70);
        assert_eq!(widths.content_x[outer], Some(10));
        assert_eq!(widths.widths[child].unwrap().content, 35);
        assert_eq!(widths.content_x[child], Some(10));
    }

    #[test]
    fn horizontal_margins_and_anonymous_blocks_reset_noninherited_values() {
        assert_eq!(resolve_margin(Margin::Auto, 200), None);
        assert_eq!(resolve_margin(Margin::Px(-5), 200), Some(-5));
        assert_eq!(resolve_margin(Margin::Percent(2500), 200), Some(50));
        let html = "<div style='direction:rtl;width:200px;margin-left:20px'>A<p style='width:100px;margin-right:auto'>B</p>C</div>";
        let styled = styled(html);
        let tree = BoxTree::from_style(&styled);
        let pass = WidthPass::from_boxes(&tree, 400);
        let div = tree
            .boxes
            .iter()
            .position(|b| b.source.is_some_and(|i| styled.nodes[i].tag == "div"))
            .unwrap();
        let paragraph = tree
            .boxes
            .iter()
            .position(|b| b.source.is_some_and(|i| styled.nodes[i].tag == "p"))
            .unwrap();
        assert_eq!(pass.widths[div].unwrap().margin_left, 20);
        assert_eq!(pass.content_x[div], Some(20));
        assert_eq!(pass.widths[paragraph].unwrap().margin_right, 100);
        assert_eq!(pass.widths[paragraph].unwrap().margin_left, 0);
        assert_eq!(pass.content_x[paragraph], Some(20));
        for &index in &tree.boxes[div].children {
            let child = &tree.boxes[index];
            if child.kind == BoxKind::AnonymousBlock {
                assert_eq!(child.style.margin_left, Margin::Px(0));
                assert_eq!(child.style.direction, Direction::Rtl);
                assert_eq!(pass.widths[index].unwrap().content, 200);
            }
        }
    }

    #[test]
    fn width_position_is_known_only_on_block_paths() {
        let tree = boxes("<div style='width:200px'><section style='width:50%'>One</section><span><div style='width:20px'>Two</div></span></div>");
        let pass = WidthPass::from_boxes(&tree, 400);
        let styled = parse_style_tree(b"<div style='width:200px'><section style='width:50%'>One</section><span><div style='width:20px'>Two</div></span></div>", &[], &Limits::DEFAULT);
        let find = |tag: &str| {
            tree.boxes
                .iter()
                .enumerate()
                .find(|(_, b)| {
                    b.source
                        .is_some_and(|source| styled.nodes[source].tag == tag)
                })
                .map(|(index, _)| index)
                .unwrap()
        };
        assert_eq!(pass.widths[find("section")].unwrap().content, 100);
        assert_eq!(pass.content_x[find("section")], Some(0));
        assert_eq!(pass.widths[find("div")].unwrap().content, 200);
        let nested = tree
            .boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| {
                b.source
                    .is_some_and(|source| styled.nodes[source].tag == "div")
            })
            .nth(1)
            .unwrap()
            .0;
        assert_eq!(pass.widths[nested].unwrap().content, 20);
        assert_eq!(pass.content_x[nested], None);
    }

    #[test]
    fn inline_block_width_is_not_guessed() {
        let tree = boxes("<div><span style='display:inline-block;width:80px'>A</span></div>");
        let used = WidthPass::from_boxes(&tree, 300);
        let index = tree
            .boxes
            .iter()
            .position(|b| b.kind == BoxKind::InlineBlock)
            .unwrap();
        assert_eq!(used.widths[index], None);
        assert!(used.unsupported);
    }

    #[test]
    fn normalized_wrappers_keep_sources_order_and_width_geometry() {
        let html = "<!doctype html><div style='width:180px;padding:4px'>A <b>B</b><p>C</p> D <span>E</span></div>";
        let tree = boxes(html);
        assert!(tree.paint_structure_valid(4096));
        let text: Vec<_> = tree
            .boxes
            .iter()
            .filter_map(|b| b.text.as_deref())
            .collect();
        assert_eq!(text, ["A ", "B", "C", " D ", "E"]);
        let before = WidthPass::from_boxes(&tree, 400);
        let mut reversed = BoxTree {
            boxes: tree.boxes.clone(),
            roots: tree.roots.clone(),
            ..BoxTree::default()
        };
        let count = reversed.boxes.len();
        reversed.boxes.reverse();
        for node in &mut reversed.boxes {
            node.parent = node.parent.map(|p| count - 1 - p);
            for child in &mut node.children {
                *child = count - 1 - *child;
            }
        }
        for root in &mut reversed.roots {
            *root = count - 1 - *root;
        }
        assert!(reversed.normalize_preorder(4096));
        assert_eq!(reversed.boxes, tree.boxes);
        assert_eq!(reversed.roots, tree.roots);
        let after = WidthPass::from_boxes(&reversed, 400);
        assert_eq!(before.widths, after.widths);
        assert_eq!(before.content_x, after.content_x);
        for (index, node) in tree.boxes.iter().enumerate() {
            assert!(node.parent.is_none_or(|p| p < index));
            assert!(node.children.iter().all(|&child| child > index));
        }
    }

    #[test]
    fn normalization_rejects_bad_graphs_atomically() {
        for damage in 0..9 {
            let mut tree = boxes("<!doctype html><div>A<p>B</p>C</div>");
            let count = tree.boxes.len();
            match damage {
                0 => tree.boxes[0].children.push(count),
                1 => tree.boxes[0].parent = Some(count),
                2 => {
                    let child = tree.boxes[0].children[0];
                    tree.boxes[0].children.push(child);
                }
                3 => tree.boxes[0].children.clear(),
                4 => tree.boxes[1].parent = None,
                5 => tree.roots.push(0),
                6 => tree.boxes[0].children.push(0),
                7 => tree.roots[0] = count,
                _ => {
                    let child = tree.boxes[0].children[0];
                    tree.boxes[child].children.push(child);
                }
            }
            let old = tree.boxes.clone();
            let roots = tree.roots.clone();
            assert!(!tree.normalize_preorder(4096), "damage {damage}");
            assert_eq!(tree.boxes, old);
            assert_eq!(tree.roots, roots);
        }
        let mut tree = boxes("<!doctype html><div>A<p>B</p>C</div>");
        let old = tree.boxes.clone();
        assert!(!tree.normalize_preorder(tree.boxes.len() - 1));
        assert_eq!(tree.boxes, old);
    }

    #[test]
    fn source_exemption_is_only_exact_generated_anonymous_wrapper() {
        for damage in 0..8 {
            let mut tree = boxes("<!doctype html><div>A<p>B</p>C</div>");
            assert!(tree.paint_structure_valid(4096));
            let wrapper = tree
                .boxes
                .iter()
                .position(|b| b.kind == BoxKind::AnonymousBlock)
                .unwrap();
            match damage {
                0 => tree.boxes[wrapper].source = Some(0),
                1 => tree.boxes[wrapper].style.padding_left = 1,
                2 => tree.boxes[wrapper].text = Some("invented".into()),
                3 => tree.boxes[wrapper].inline_phrase = true,
                4 => tree.boxes[wrapper].children.clear(),
                5 => {
                    let child = tree.boxes[wrapper].children[0];
                    tree.boxes[child].kind = BoxKind::Block;
                }
                6 => tree.boxes[0].source = None,
                _ => {
                    let child = tree.boxes[wrapper].children[0];
                    tree.boxes[child].source = None;
                }
            }
            assert!(!tree.paint_structure_valid(4096), "damage {damage}");
        }
    }

    #[test]
    fn mixed_children_get_anonymous_blocks_in_dom_order() {
        let tree = boxes("<div>A <em>B</em><p>C</p> D <i>E</i></div>");
        let div = tree
            .boxes
            .iter()
            .position(|b| b.source.is_some_and(|s| s == 3))
            .unwrap();
        let kinds: Vec<_> = tree.boxes[div]
            .children
            .iter()
            .map(|&i| tree.boxes[i].kind)
            .collect();
        assert_eq!(
            kinds,
            [
                BoxKind::AnonymousBlock,
                BoxKind::Block,
                BoxKind::AnonymousBlock
            ]
        );
        let first = tree.boxes[div].children[0];
        assert_eq!(tree.boxes[first].children.len(), 2);
        assert!(tree.boxes[first]
            .children
            .iter()
            .all(|&i| tree.boxes[i].parent == Some(first)));
        assert!(!tree.truncated);
    }

    #[test]
    fn all_inline_children_stay_in_inline_formatting_context() {
        let tree = boxes("<div>One <em>two</em> three</div>");
        assert!(!tree.boxes.iter().any(|b| b.kind == BoxKind::AnonymousBlock));
        assert!(tree.boxes.iter().any(|b| b.text.as_deref() == Some("One ")));
    }

    #[test]
    fn hidden_subtree_has_no_boxes_or_text() {
        let tree =
            boxes("<div>Shown<span style='display:none'>Secret<b>Nested</b></span> End</div>");
        assert!(!tree
            .boxes
            .iter()
            .any(|b| b.text.as_deref() == Some("Secret") || b.text.as_deref() == Some("Nested")));
        assert!(tree.boxes.iter().any(|b| b.text.as_deref() == Some(" End")));
    }

    #[test]
    fn block_inside_inline_is_explicitly_unsupported() {
        let tree = boxes("<span>before<div>block</div>after</span>");
        assert!(tree.unsupported);
    }
}

/// Shaped text layout for a whole page, decided ONCE and consumed by the
/// height pass, the vertical pass and the painter. All-or-nothing: if any
/// direct-text block is ineligible, `build` returns `None` and the page uses
/// the unchanged legacy path, so a page never mixes two metric systems.
pub struct PageTextLayout {
    /// Per box index; `Some` for every direct-text block.
    pub blocks: Vec<Option<DirectTextLines>>,
}

impl PageTextLayout {
    #[must_use]
    pub fn build(
        tree: &BoxTree,
        widths: &WidthPass,
        shaping: &TextShaping,
        mut line_height: impl FnMut(usize, u32) -> Option<u32>,
    ) -> Option<Self> {
        let mut blocks: Vec<Option<DirectTextLines>> = Vec::new();
        blocks.resize_with(tree.boxes.len(), || None);
        let mut any = false;
        for (index, node) in tree.boxes.iter().enumerate() {
            if node.kind == BoxKind::Block
                && !node.children.is_empty()
                && node
                    .children
                    .iter()
                    .all(|&c| matches!(tree.boxes[c].kind, BoxKind::Text | BoxKind::Inline))
            {
                let width = widths.widths.get(index).copied().flatten()?.content;
                blocks[index] =
                    Some(shaped_block_lines(tree, index, width, shaping, |size| {
                        line_height(index, size)
                    })?);
                any = true;
            }
        }
        any.then_some(Self { blocks })
    }
}

pub static SHADOW_AA: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static SHADOW_AR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static SHADOW_LR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static SHADOW_DIS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static SHADOW_SKIP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub fn shadow_counts() -> [u32; 5] {
    use std::sync::atomic::Ordering::Relaxed;
    [SHADOW_AA.load(Relaxed), SHADOW_AR.load(Relaxed), SHADOW_LR.load(Relaxed), SHADOW_DIS.load(Relaxed), SHADOW_SKIP.load(Relaxed)]
}
