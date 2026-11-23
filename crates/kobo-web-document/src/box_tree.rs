//! CSS 2.1 display box generation before used values and line layout.
//!
//! This is a separate, bounded path; the shipping reader remains the fallback.
//! It preserves DOM order and text. Anonymous block wrappers follow CSS 2.1
//! §9.2.1.1 for mixed block and inline children. It does not claim to layout
//! those boxes, and calls out block-in-inline splitting as unsupported.

use crate::computed_style::{
    AlignItems, BoxSizing, Clear, Computed, Direction, Display, Float, JustifyContent, Length, Margin, Position,
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
    /// Replaced `img` content with ImageFacts and block marker geometry.
    /// The basic painter reserves image space; the offline harness replaces
    /// its sentinel fill with decoded pixels. Not native image decoding.
    Image,
    Table,
    TableRowGroup,
    TableRow,
    TableCell,
    TableCaption,
    TableColumn,
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
    /// Replaced-content facts when `kind == BoxKind::Image`.
    pub image: Option<crate::style_tree::ImageFacts>,
    pub table_span: (u32, u32),
    /// EXPERIMENTAL: a generated `::before`/`::after` box (extended-CSS entry).
    /// Paintable only as the cleared child of the confined float kernel by the
    /// explicit kernel painter.
    pub(crate) generated: bool,
}

/// Intrinsic dimensions of a usable image: both `width` and `height`
/// attributes when positive, else the data-URI payload's own header.
/// Mirrors the deleted intake rewrite exactly: the source must be a
/// `data:image/` URI AND dimensions must resolve one way or the other.
fn image_intrinsic(facts: &crate::style_tree::ImageFacts) -> Option<(u32, u32)> {
    facts.src_data_uri.as_ref()?;
    match (facts.attr_width, facts.attr_height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => Some((w, h)),
        _ => facts
            .src_data_uri
            .as_deref()
            .and_then(|s| crate::basic_page::parse_data_image(s.as_bytes())),
    }
}

/// Which explicit paint consumer is validating a tree. `Default` refuses all
/// generated boxes; `FloatKernel` admits only the confined cleared generated
/// child; `GeneratedBlock` admits only `generated_block_admitted` ordinary
/// generated blocks under a tree-global float/clear refusal.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub(crate) enum PaintMode {
    Default,
    FloatKernel,
    GeneratedBlock,
}

#[derive(Default, Clone)]
pub struct BoxTree {
    pub boxes: Vec<CssBox>,
    pub roots: Vec<usize>,
    pub truncated: bool,
    /// Image boxes created so far; assigns sentinel indexes in document
    /// (box creation) order.
    image_count: usize,
    pub unsupported: bool,
    /// Quirks is a visual paint gate, not a block geometry diagnostic gate.
    pub quirks: bool,
    /// CANDIDATE-local API growth (disclosed): copied from `StyleTree::ext`; stretch and centring of absolute leaves require it.
    pub ext: bool,
    /// Page-view list markers keyed by box index; exact painters ignore these.
    pub page_list_markers: std::collections::BTreeMap<usize, String>,
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

/// EXPERIMENTAL private guard, UNREVIEWED. A percentage of an integer-px basis is
/// admitted only in the integer subset where Chrome's binary32 operation
/// `LayoutUnit(float(basis * P / 100.0f))` (P = float(hundredths/100)) is provably the
/// exact integer, so no host float or soft-float is needed:
///  * `h % 25 == 0`  => P = (h/25)/4 is a binary32-exact multiple of 1/4;
///  * basis <= 2^24 and q = basis*(h/25) <= 2^24 => `basis*P = q/4` is exact in binary32;
///  * `q % 400 == 0` => `(q/4)/100 = q/400` is an exact integer, hence the division and
///    the truncation to 1/64 are exact.
/// Everything else (any fraction, any h not a multiple of 25, large operands) refuses.
/// Conservative, not maximal. Negative percentages are not covered (refused by callers).
#[must_use]
pub(crate) fn percent_exact_px(basis: u32, hundredths: u32) -> Option<u32> {
    const LIMIT: u64 = 1 << 24;
    if hundredths % 25 != 0 || u64::from(basis) > LIMIT {
        return None;
    }
    let q = u64::from(basis) * u64::from(hundredths / 25);
    (q <= LIMIT && q % 400 == 0).then(|| u32::try_from(q / 400).ok()).flatten()
}

/// True when a percent-valued length/margin is absent or admitted by `percent_exact_px`.
fn length_percent_ok(length: Length, basis: Option<u32>) -> bool {
    match (length, basis) {
        (Length::Percent(h), Some(b)) => percent_exact_px(b, h).is_some(),
        (Length::Percent(_), None) => false,
        _ => true,
    }
}

fn margin_percent_ok(margin: Margin, basis: Option<u32>) -> bool {
    match (margin, basis) {
        (Margin::Percent(h), Some(b)) => u32::try_from(h).ok().is_some_and(|h| percent_exact_px(b, h).is_some()),
        (Margin::Percent(_), None) => false,
        _ => true,
    }
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
    /// BOUNDED FLOAT KERNEL (private candidate, UNREVIEWED). A block `p` is a kernel
    /// parent only when it has exactly two block children: one `float:left` or
    /// `float:right` empty
    /// block with a definite height and a width that is a px length or a whole-px
    /// percentage of the parent's px width (resolved width > 0; fractional, 0 and
    /// over-100% refuse), then one empty auto-width block with
    /// height that is EITHER `clear:both` (variant A, starts below the float; the
    /// height may be a nonzero px, `0` or auto, i.e. an empty zero-height clearing
    /// block as an explicit element, NOT a pseudo-element and NOT `display:table`) OR
    /// has no clear (variant B, nonzero px height only; starts at the parent top and
    /// the float overlaps it). Every involved box is static, LTR,
    /// content-box, with zero margins, padding and border. The float must fit in the
    /// parent's definite px width (a 0px parent is refused). Anything else carrying float/clear is refused.
    /// Returns (float child, second child); the second child is cleared in A only.
    pub(crate) fn float_kernel(&self, p: usize) -> Option<(usize, usize)> {
        let zero_edges = |b: &CssBox| {
            let s = &b.style;
            [s.margin_top, s.margin_right, s.margin_bottom, s.margin_left]
                .iter()
                .all(|m| *m == Margin::Px(0))
                && s.padding_top == 0
                && s.padding_right == 0
                && s.padding_bottom == 0
                && s.padding_left == 0
                && s.decor.used_border() == [0; 4]
                && !s.decor.has_painted_border()
        };
        let plain = |b: &CssBox| {
            b.kind == BoxKind::Block
                && b.style.positioned.position == Position::Static
                && b.style.direction == Direction::Ltr
                && b.style.box_sizing == BoxSizing::ContentBox
                && b.style.min_width == Length::Auto
                && b.style.max_width == Length::Auto
                && b.style.display == Display::Block
                && b.children.is_empty()
                && zero_edges(b)
        };
        let parent = self.boxes.get(p)?;
        if parent.children.len() != 2 || parent.kind != BoxKind::Block {
            return None;
        }
        let (f, c) = (parent.children[0], parent.children[1]);
        let (fb, cb) = (self.boxes.get(f)?, self.boxes.get(c)?);
        let Length::Px(pw) = parent.style.width else {
            return None;
        };
        let parent_ok = parent.style.positioned.position == Position::Static
            && parent.style.positioned.float == Float::None
            && parent.style.positioned.clear == Clear::None
            && parent.style.direction == Direction::Ltr
            && parent.style.box_sizing == BoxSizing::ContentBox
            && parent.style.display == Display::Block
            // A px parent shorter than float + cleared block lets the float overflow onto
            // later siblings. ELIGIBILITY CHANGE vs 4fc540a2 (which refused it): layout
            // geometry is admitted again (Chrome-recorded b1: next block y = parent
            // height) but ONLY the explicit float-kernel background painter may draw it.
            && matches!(parent.style.height, Length::Auto | Length::Px(_))
            && parent.style.min_width == Length::Auto
            && parent.style.max_width == Length::Auto
            && zero_edges(parent)
            && parent
                .parent
                .is_none_or(|g| self.boxes.get(g).is_some_and(|g| g.kind == BoxKind::Block));
        let float_ok = plain(fb)
            && matches!(fb.style.positioned.float, Float::Left | Float::Right)
            && fb.style.positioned.clear == Clear::None
            && match fb.style.width {
                Length::Px(w) => w > 0 && w <= pw,
                // Variant D: a percentage that resolves to a whole px count of the
                // parent's definite px width (no fractional LayoutUnit/snapping case).
                // Repaired (UNREVIEWED): exact-rational whole was not sufficient (Chrome's
                // binary32 gives 162.984375 for 250px*65.2%); use the proven integer subset.
                Length::Percent(h) => {
                    h > 0 && percent_exact_px(pw, h).is_some_and(|px| px > 0 && px <= pw)
                }
                Length::Auto => false,
            }
            && matches!(fb.style.height, Length::Px(h) if h > 0);
        // Variant A: the second block is `clear:both` (starts below the float).
        // Variant B: the second block is an ordinary block with no clear; it starts at
        // the parent's top beside/under the float, and the float escapes the parent.
        let clear_ok = plain(cb)
            && cb.style.positioned.float == Float::None
            && matches!(cb.style.positioned.clear, Clear::Both | Clear::None)
            && cb.style.width == Length::Auto
            && match cb.style.height {
                Length::Px(h) => h > 0 || cb.style.positioned.clear == Clear::Both,
                // Variant E1: an EMPTY cleared block of auto height (the clearfix
                // shape, zero height). Never for an uncleared second block.
                Length::Auto => cb.style.positioned.clear == Clear::Both,
                Length::Percent(_) => false,
            };
        (parent_ok && float_ok && clear_ok).then_some((f, c))
    }

    /// True for the float child of a valid kernel (the box painted in the float phase).
    pub(crate) fn is_kernel_float(&self, index: usize) -> bool {
        self.boxes
            .get(index)
            .and_then(|b| b.parent)
            .and_then(|p| self.float_kernel(p))
            .is_some_and(|(f, _)| f == index)
    }

    /// True for the `clear:both` second child of a valid kernel (variant A shape). Its
    /// zero used height is exempt from the generic empty-block refusal (margins are
    /// zero by the kernel gate, so nothing can collapse through it).
    /// EXPERIMENTAL generated-block admission (extended-CSS entry only), the
    /// predicate P of the reviewed kernel-B contract. Mirrors the `plain` /
    /// `zero_edges` expressions in `float_kernel` (above) DELIBERATELY
    /// duplicated, so the accepted kernel source stays byte-unchanged.
    /// Symmetric across `::before`/`::after`; one failure refuses the tree.
    fn generated_block_admitted(&self, index: usize) -> bool {
        let Some(b) = self.boxes.get(index) else {
            return false;
        };
        let s = &b.style;
        let zero_edges = [s.margin_top, s.margin_right, s.margin_bottom, s.margin_left]
            .iter()
            .all(|m| *m == Margin::Px(0))
            && s.padding_top == 0
            && s.padding_right == 0
            && s.padding_bottom == 0
            && s.padding_left == 0
            && s.decor.used_border() == [0; 4]
            && !s.decor.has_painted_border();
        let predicate = b.kind == BoxKind::Block
            && s.positioned.position == Position::Static
            && s.direction == Direction::Ltr
            && s.box_sizing == BoxSizing::ContentBox
            && s.display == Display::Block
            && b.children.is_empty()
            && zero_edges
            && matches!(s.height, Length::Px(h) if h > 0)
            && matches!(s.width, Length::Auto | Length::Px(_))
            // Kernel-B contract pin R2: min/max width must be unconstrained.
            && s.min_width == Length::Auto
            && s.max_width == Length::Auto;
        if !predicate {
            return false;
        }
        // A generated box is never a root; refuse defensively.
        let Some(parent) = b.parent else {
            return false;
        };
        // Text-mixing rule (source-grounded form of the contract's
        // any-retained-text-sibling pin): mixed-content normalization can lift
        // a retained text box (including whitespace-only) into an anonymous
        // wrapper, so a raw box-tree sibling check can miss it. Find the
        // ORIGINATING box (nearest ancestor with a DOM source) and refuse if
        // ANY of its descendants carries text or is not a Block box (this also
        // refuses anonymous wrappers, which only exist under mixed content).
        let mut orig = parent;
        while self.boxes[orig].source.is_none() {
            match self.boxes[orig].parent {
                Some(p) => orig = p,
                None => return false,
            }
        }
        let mut stack = self.boxes[orig].children.clone();
        while let Some(d) = stack.pop() {
            if d == index {
                continue;
            }
            let db = &self.boxes[d];
            if db.text.is_some() || db.kind != BoxKind::Block {
                return false;
            }
            stack.extend_from_slice(&db.children);
        }
        // Full ancestor walk: every ancestor up to the root is a static Block
        // with display Block; non-root ancestors are also LTR content-box.
        // Kernel-B contract pin R1: the root itself is NOT unconditionally exempt -
        // it must still be kind Block + display Block + position Static.
        let mut up = Some(parent);
        while let Some(a) = up {
            let ab = &self.boxes[a];
            let is_root = ab.parent.is_none();
            let ok = ab.kind == BoxKind::Block
                && ab.style.display == Display::Block
                && ab.style.positioned.position == Position::Static
                && (is_root
                    || (ab.style.direction == Direction::Ltr
                        && ab.style.box_sizing == BoxSizing::ContentBox));
            if !ok {
                return false;
            }
            up = ab.parent;
        }
        true
    }

    pub(crate) fn is_kernel_cleared(&self, index: usize) -> bool {
        self.boxes.get(index).is_some_and(|b| {
            b.style.positioned.clear == Clear::Both
                && b.parent.and_then(|p| self.float_kernel(p)).is_some_and(|(_, c)| c == index)
        })
    }

    pub(crate) fn has_float_or_clear(&self) -> bool {
        self.boxes
            .iter()
            .any(|b| b.style.positioned.float != Float::None || b.style.positioned.clear != Clear::None)
    }

    /// True when every float/clear declaration sits in a valid kernel.
    pub(crate) fn float_clear_confined(&self) -> bool {
        // GLOBAL confinement: at most ONE kernel in the whole tree. Floats from
        // different parents share one block formatting context in Chrome (a float
        // escaping a short parent displaces a later float), which is not modelled
        // (review blocker 4:51: second float at x100 in Chrome, x0 here).
        let floats = self.boxes.iter().filter(|b| b.style.positioned.float != Float::None).count();
        let clears = self.boxes.iter().filter(|b| b.style.positioned.clear != Clear::None).count();
        floats <= 1
            && clears <= 1
            && self.boxes.iter().enumerate().all(|(i, b)| {
                (b.style.positioned.float == Float::None && b.style.positioned.clear == Clear::None)
                    || b.parent
                        .and_then(|p| self.float_kernel(p))
                        .is_some_and(|(f, c)| i == f || i == c)
            })
    }
}

impl BoxTree {
    /// Flex container, or an in-flow item of one: both establish an
    /// independent formatting context, so child margins never collapse out.
    fn is_flex_root(&self, index: usize) -> bool {
        self.boxes[index].kind == BoxKind::Flex
            || self.boxes[index].style.overflow_hidden
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
        // FL18: `column-gap` (ext only; Auto = normal = 0) is fixed space between adjacent items, so it is removed from the
        // width the flex resolver and the justify-content free space see. Refuse when the gaps alone exceed the width.
        let col_gap: i64 = match node.style.column_gap {
            Length::Px(g) => i64::from(g),
            Length::Auto => 0,
            Length::Percent(_) => return None,
        };
        let gaps_total = col_gap.checked_mul(i64::try_from(node.children.len().saturating_sub(1)).ok()?)?;
        let avail = i64::from(content_width).checked_sub(gaps_total).filter(|a| *a >= 0)?;
        let avail_u = u32::try_from(avail).ok()?;
        let targets = self.flex_targets(container, avail_u)?;
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
        let free = avail - used;
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
                cursor += gap + col_gap;
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

    /// CANDIDATE (ext, private): a direct, empty, absolute leaf of a positioned Block parent whose width and/or height is
    /// `auto` and resolved by BOTH opposite offsets as `px` (CSS 2.2 10.3.7/10.6.4 stretch). Zero margins and zero border
    /// on the leaf only; padding allowed. box-sizing is NOT checked: with both opposite offsets an auto axis fills the same
    /// box under either value (Chrome st4b/st5, tested). The parent may be Absolute or Relative (tested for both). This
    /// is shared BoxTree geometry, so it is NOT limited to the background painter: the text painter consumes it too (stub
    /// font layer-order control in tests/position_stretch_leaf.rs). Returns (stretch_width, stretch_height).
    pub(crate) fn stretch_axes(&self, index: usize) -> (bool, bool) {
        let n = &self.boxes[index];
        let ok = self.ext
            && n.kind == BoxKind::Block
            && n.style.positioned.position == Position::Absolute
            && n.children.is_empty()
            && n.style.direction == Direction::Ltr
            && [n.style.margin_top, n.style.margin_right, n.style.margin_bottom, n.style.margin_left] == [Margin::Px(0); 4]
            && n.style.decor.used_border() == [0; 4]
            && n.parent.is_some_and(|p| {
                self.boxes[p].kind == BoxKind::Block
                    && matches!(self.boxes[p].style.positioned.position, Position::Absolute | Position::Relative)
            });
        if !ok {
            return (false, false);
        }
        let [top, right, bottom, left] = n.style.positioned.offsets;
        (
            n.style.width == Length::Auto && matches!((left, right), (Margin::Px(_), Margin::Px(_))),
            n.style.height == Length::Auto && matches!((top, bottom), (Margin::Px(_), Margin::Px(_))),
        )
    }

    /// CANDIDATE (ext, private): `margin:auto` centring of an absolute leaf along an axis whose size is a px value and
    /// whose BOTH opposite offsets are px (CSS 2.2 10.3.7/10.6.4, equal auto margins). Per axis: both margins auto =
    /// centred, both Px(0) = ordinary. Used free space must be non-negative and even (odd space gives half-pixel
    /// margins; unproven, refused). Same leaf/parent preconditions as `stretch_axes` except margins.
    pub(crate) fn centered_axes(&self, index: usize) -> (bool, bool) {
        let n = &self.boxes[index];
        let ok = self.ext
            && n.kind == BoxKind::Block
            && n.style.positioned.position == Position::Absolute
            && n.children.is_empty()
            && n.style.direction == Direction::Ltr
            && n.parent.is_some_and(|p| {
                self.boxes[p].kind == BoxKind::Block
                    && matches!(self.boxes[p].style.positioned.position, Position::Absolute | Position::Relative)
            });
        if !ok {
            return (false, false);
        }
        let [top, right, bottom, left] = n.style.positioned.offsets;
        let h = n.style.margin_left == Margin::Auto
            && n.style.margin_right == Margin::Auto
            && matches!(n.style.width, Length::Px(_))
            && matches!((left, right), (Margin::Px(_), Margin::Px(_)));
        let v = n.style.margin_top == Margin::Auto
            && n.style.margin_bottom == Margin::Auto
            && matches!(n.style.height, Length::Px(_))
            && matches!((top, bottom), (Margin::Px(_), Margin::Px(_)));
        (h, v)
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
                    .find(|&&c| matches!(self.boxes[c].style.positioned.position, Position::Static | Position::Relative))
            } else {
                node.children
                    .iter()
                    .rev()
                    .find(|&&c| matches!(self.boxes[c].style.positioned.position, Position::Static | Position::Relative))
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
            unsupported: tree.unsupported || tree.stray_min_max_width() || tree.stray_min_height() || tree.stray_overflow_hidden(),
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
            if matches!(node.style.height, Length::Percent(_)) && containing.is_some() && !length_percent_ok(node.style.height, containing) {
                result.unsupported = true;
            }
            let height = match node.kind {
                BoxKind::Block
                | BoxKind::ListItem
                | BoxKind::Flex
                | BoxKind::Image
                | BoxKind::Table
                | BoxKind::TableRowGroup
                | BoxKind::TableRow
                | BoxKind::TableCell
                | BoxKind::TableCaption
                | BoxKind::TableColumn => specified_block_height(node.style.height, containing)
                    .map(|declared| {
                        if node.style.box_sizing == BoxSizing::BorderBox {
                            declared.saturating_sub(
                                node.style
                                    .padding_top
                                    .saturating_add(node.style.padding_bottom),
                            )
                        } else {
                            declared
                        }
                    }),
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
/// Per-node refusal kinds recorded by [`UsedHeightPass`] (CONTAINMENT v1,
/// reviewer-dispositioned delta5/6, frozen acceptance matrix delta7). The
/// whole-pass `unsupported` flag remains IMPERATIVELY MAINTAINED (set at the
/// same sites as pre-containment) and is byte-identical in value; the
/// equivalence invariant `unsupported == seed (tree-global kinds only) ||
/// any node refusal` is test-pinned, not a derivation the code relies on.
/// Subtree-attributable kinds are recorded per node so a clean subtree's
/// heights compose while a poisoned sibling stays refused; tree-global kinds
/// (DroppedAttributes, MissingStylesheet, CssDeclaration/CssCondition,
/// CssScanBudget, Encoding, Quirks, truncations) never appear here - they
/// remain whole-tree via the seed. InlineBlock/flex/percent stay whole-pass
/// in v1 by decision (SQ3, census-backed).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HeightRefusal(u32);

impl HeightRefusal {
    pub const NONE: Self = Self(0);
    /// Text/Inline node with no font provider and no page layout supplied.
    pub const NO_METRICS_PROVIDER: Self = Self(1);
    /// Text/Inline node with no pure-inline owner block (inline_owner :1788).
    pub const NO_INLINE_OWNER: Self = Self(2);
    /// AnonymousBlock/InlineBlock reaching the block composition arm.
    pub const UNSUPPORTED_KIND: Self = Self(4);
    /// Flex row height composition failed.
    pub const FLEX_UNRESOLVED: Self = Self(8);
    /// Non-auto specified height unresolved (indefinite percentage).
    pub const INDEFINITE_PERCENT: Self = Self(16);
    /// Containing width unknown for a block that needs one.
    pub const NO_WIDTH: Self = Self(32);
    /// Pure-inline block measured None or zero content height.
    pub const EMPTY_OR_UNMEASURED: Self = Self(64);
    /// Kernel float composition failed.
    pub const FLOAT_UNRESOLVED: Self = Self(128);
    /// min-height last-child bottom margin would escape (Chrome 156 rule).
    pub const MIN_HEIGHT_MARGIN_ESCAPE: Self = Self(256);
    /// Block children composition invalid or height unresolved.
    pub const COMPOSITION_UNRESOLVED: Self = Self(512);
    /// Stretch-axis fixup could not resolve a definite height.
    pub const STRETCH_UNRESOLVED: Self = Self(1024);
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    #[must_use]
    pub fn bits(self) -> u32 {
        self.0
    }
}

impl std::ops::BitOr for HeightRefusal {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for HeightRefusal {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

pub struct UsedHeightPass {
    pub heights: Vec<Option<u32>>,
    pub unsupported: bool,
    pub truncated: bool,
    /// Per-node refusal flags; empty bits mean the node did not refuse.
    /// Recording only: no consumer reads this field yet. Invariant
    /// (test-pinned): `unsupported == (seed || any refusal)`.
    pub refusals: Vec<HeightRefusal>,
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
        let seed = specified.unsupported || widths.unsupported;
        let mut result = Self {
            heights: specified.heights,
            unsupported: seed,
            truncated: specified.truncated || widths.truncated,
            refusals: vec![HeightRefusal::NONE; tree.boxes.len()],
        };
        // CONTAINMENT v1: no early-return on the seed. Composition always runs
        // so clean subtrees resolve per-block heights on a seed-poisoned tree;
        // every consumer gates on the whole-pass flags before reading heights
        // (VerticalPass :1560, css_paint_bridge :290, css_text_page :530), so
        // the composed values are invisible wherever the bool refuses.
        if result.truncated {
            return result;
        }
        // A valid box tree is acyclic; reverse traversal puts descendants
        // before parents. Invalid trees are rejected by the paint bridge.
        for (index, node) in tree.boxes.iter().enumerate().rev() {
            if node.kind == BoxKind::Text || node.kind == BoxKind::Inline {
                let no_provider = (advance.is_none() || line_height.is_none()) && layout.is_none();
                let no_owner = inline_owner(tree, index).is_none();
                if no_provider || no_owner {
                    result.unsupported = true;
                    if no_provider {
                        result.refusals[index] |= HeightRefusal::NO_METRICS_PROVIDER;
                    }
                    if no_owner {
                        result.refusals[index] |= HeightRefusal::NO_INLINE_OWNER;
                    }
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
                    result.refusals[index] |= HeightRefusal::FLEX_UNRESOLVED;
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
                    result.refusals[index] |= HeightRefusal::FLEX_UNRESOLVED;
                }
                continue;
            }
            if !matches!(node.kind, BoxKind::Block | BoxKind::ListItem) {
                result.unsupported = true;
                result.refusals[index] |= HeightRefusal::UNSUPPORTED_KIND;
                continue;
            }
            if result.heights[index].is_some() {
                continue;
            }
            if tree.stretch_axes(index).1 {
                continue; // resolved after the parent height is known (fixup below)
            }
            // Variant E1: the kernel's EMPTY cleared block of auto height has used height 0.
            if node.style.height == Length::Auto
                && node.style.positioned.clear == Clear::Both
                && node.parent.is_some_and(|p| tree.float_kernel(p).is_some_and(|(_, c)| c == index))
            {
                result.heights[index] = Some(0);
                continue;
            }
            if node.style.height != Length::Auto {
                result.unsupported = true; // indefinite percentage
                result.refusals[index] |= HeightRefusal::INDEFINITE_PERCENT;
                continue;
            }
            let Some(width) = widths.widths[index].map(|w| w.content) else {
                result.unsupported = true;
                result.refusals[index] |= HeightRefusal::NO_WIDTH;
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
                                    && node.style.line_height_number.is_none() && node.style.line_height_precise.is_none(),
                            )
                        })
                        .ok()
                    })
                    .map(|lines| lines.content_height)
                };
                result.heights[index] = measured;
                if result.heights[index].is_none_or(|height| height == 0) {
                    result.unsupported = true;
                    result.refusals[index] |= HeightRefusal::EMPTY_OR_UNMEASURED;
                }
                continue;
            }
            if let Some((f, c)) = tree.float_kernel(index) {
                // The float leaves normal flow. Cleared (A): the second block starts
                // below it. Uncleared (B): the parent is only as tall as the second
                // block and the float overflows.
                result.heights[index] = if tree.boxes[c].style.positioned.clear == Clear::Both {
                    result.heights[f]
                        .zip(result.heights[c])
                        .and_then(|(fh, ch)| fh.checked_add(ch))
                } else {
                    result.heights[c]
                };
                if result.heights[index].is_none() {
                    result.unsupported = true;
                    result.refusals[index] |= HeightRefusal::FLOAT_UNRESOLVED;
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
                let Some(height) = result.heights[child].map(|h| h.max(tree.min_height_px(child))) else {
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
                // Chrome 156 (private-evidence ul1/mh2, mh3): with a positive min-height the last child's bottom margin is
                // absorbed when content < min and escapes when content >= min, so the result depends on the used heights.
                // Not modelled: refuse whenever that margin is non-zero and would escape.
                if node.style.min_height != Length::Auto
                    && node.style.padding_bottom == 0
                    && node.parent.is_some()
                    && tree.collapsed_margin(last, false, &widths, viewport_width) != 0
                {
                    result.unsupported = true;
                    result.refusals[index] |= HeightRefusal::MIN_HEIGHT_MARGIN_ESCAPE;
                }
                if node.style.padding_bottom != 0 || node.parent.is_none() || tree.is_flex_root(index) {
                    content = content.saturating_add(tree.collapsed_margin(
                        last,
                        false,
                        &widths,
                        viewport_width,
                    ));
                }
            } else if node.style.padding_top == 0 && node.style.padding_bottom == 0 && node.style.min_height == Length::Auto {
                valid = false; // margin-through empty block
            }
            if valid {
                result.heights[index] = u32::try_from(content.max(0)).ok();
            }
            if result.heights[index].is_none() {
                result.unsupported = true;
                result.refusals[index] |= HeightRefusal::COMPOSITION_UNRESOLVED;
            }
        }
        for index in 0..tree.boxes.len() {
            if tree.boxes[index].kind == BoxKind::Block {
                let min = tree.min_height_px(index);
                if min > 0 {
                    result.heights[index] = result.heights[index].map(|h| h.max(min));
                }
            }
        }
        // Stretch fixup: the leaf contributes nothing to its parent, so its height follows the final parent height.
        for index in 0..tree.boxes.len() {
            if !tree.stretch_axes(index).1 {
                continue;
            }
            let leaf = &tree.boxes[index];
            let parent = leaf.parent.map(|p| (p, &tree.boxes[p]));
            let [top, _, bottom, _] = leaf.style.positioned.offsets;
            let used = match (parent, top, bottom) {
                (Some((p, ps)), Margin::Px(t), Margin::Px(b)) => {
                    let border = ps.style.decor.used_border();
                    result.heights[p]
                        .and_then(|h| i64::from(h).checked_add(i64::from(ps.style.padding_top) - i64::from(border[0])))
                        .and_then(|v| v.checked_add(i64::from(ps.style.padding_bottom) - i64::from(border[2])))
                        .and_then(|v| v.checked_sub(i64::from(t)))
                        .and_then(|v| v.checked_sub(i64::from(b)))
                        .and_then(|v| v.checked_sub(i64::from(leaf.style.padding_top)))
                        .and_then(|v| v.checked_sub(i64::from(leaf.style.padding_bottom)))
                        .and_then(|v| u32::try_from(v).ok())
                }
                _ => None,
            };
            result.heights[index] = used;
            if used.is_none() {
                result.unsupported = true;
                result.refusals[index] |= HeightRefusal::STRETCH_UNRESOLVED;
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
/// Per-node refusal kinds recorded by [`VerticalPass`] (CONTAINMENT delta9,
/// frozen outcomes TEST-MATRIX.md Part D, written before this code). Recording
/// only: no consumer reads this field yet. The whole-pass `unsupported` flag
/// remains IMPERATIVELY MAINTAINED and byte-identical in value; the
/// equivalence with pre-containment behavior is pinned by the fill-gate below
/// (any refused node leaves a None content_y, which wipes the whole vector)
/// plus the Part D fixtures. Structural corruption (parent mismatch, missing
/// node, bad roots) stays whole-pass with no per-node attribution.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct VerticalRefusal(u32);

impl VerticalRefusal {
    pub const NONE: Self = Self(0);
    /// Not a block/list-item/flex box and not a measured inline member.
    pub const KIND_NOT_ADMITTED: Self = Self(1);
    /// No usable height (None, or unpadded zero without an allowed reason).
    pub const NO_HEIGHT: Self = Self(2);
    /// No resolved width.
    pub const NO_WIDTH: Self = Self(4);
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    #[must_use]
    pub fn bits(self) -> u32 {
        self.0
    }
}

impl std::ops::BitOr for VerticalRefusal {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for VerticalRefusal {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

pub struct VerticalPass {
    pub content_y: Vec<Option<i64>>,
    pub unsupported: bool,
    pub truncated: bool,
    /// Per-node vertical refusal attribution; empty bits mean admitted.
    pub refusals: Vec<VerticalRefusal>,
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
            unsupported: tree.unsupported || tree.stray_min_max_width() || tree.stray_min_height() || tree.stray_overflow_hidden(),
            truncated: tree.truncated,
            refusals: vec![VerticalRefusal::NONE; count],
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
        // CONTAINMENT delta9: no early-return on the heights/widths flags. The
        // bool stays imperatively maintained (identical value); the validation
        // walk runs under it to attribute refusals per node, and the fill-gate
        // after composition keeps content_y all-None exactly as the
        // early-return did.
        if heights.unsupported || widths.unsupported {
            result.unsupported = true;
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
            if seen[index] || node.parent != parent {
                // Structural corruption: whole-pass, no per-node attribution.
                result.unsupported = true;
                break;
            }
            let is_inline_kind = matches!(node.kind, BoxKind::Text | BoxKind::Inline);
            let refusal = if !(matches!(node.kind, BoxKind::Block | BoxKind::ListItem | BoxKind::Flex)
                || inline_member)
            {
                VerticalRefusal::KIND_NOT_ADMITTED
            } else if !is_inline_kind
                && heights.heights[index].is_none_or(|h| {
                    h == 0
                        && node.style.padding_top == 0
                        && node.style.padding_bottom == 0
                        && !tree.zero_positioned_container(index)
                        && !tree.is_kernel_cleared(index)
                })
            {
                VerticalRefusal::NO_HEIGHT
            } else if !is_inline_kind && widths.widths[index].is_none() {
                VerticalRefusal::NO_WIDTH
            } else {
                VerticalRefusal::NONE
            };
            if !refusal.is_empty() {
                result.unsupported = true;
                result.refusals[index] |= refusal;
                // Keep walking through children so sibling branches and
                // descendants still get attributed (delta9 Part D).
                if node.kind != BoxKind::Text {
                    for &child in node.children.iter().rev() {
                        stack.push((child, Some(index)));
                    }
                }
                continue;
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
            if let Some((f, c)) = tree.float_kernel(index) {
                result.content_y[f] = result.content_y[index];
                result.content_y[c] = if tree.boxes[c].style.positioned.clear == Clear::Both {
                    result.content_y[index]
                        .zip(heights.heights[f])
                        .and_then(|(y, h)| y.checked_add(i64::from(h)))
                } else {
                    result.content_y[index]
                };
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
                    let nested = matches!(node.style.positioned.position, Position::Absolute | Position::Relative);
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
                        (Margin::Px(top), Margin::Px(bottom)) if tree.centered_axes(child).1 => (|| {
                            let free = i64::from(cb_height?) - i64::from(top) - i64::from(bottom) - outer?;
                            if free < 0 || free % 2 != 0 { return None; }
                            cb_y?.checked_add(i64::from(top))?.checked_add(free / 2)
                        })(),
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
            .filter(|&parent| matches!(tree.boxes[parent].style.positioned.position, Position::Absolute | Position::Relative));
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
            unsupported: tree.unsupported || tree.stray_min_max_width() || tree.stray_min_height() || tree.stray_overflow_hidden(),
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
            // Percent guard (UNREVIEWED): every percent width/margin resolves against the
            // containing width only in the proven exact-integer subset.
            if matches!(node.kind, BoxKind::Block | BoxKind::ListItem | BoxKind::AnonymousBlock | BoxKind::Flex)
                && !(length_percent_ok(node.style.width, containing)
                && margin_percent_ok(node.style.margin_left, containing)
                && margin_percent_ok(node.style.margin_right, containing)
                && margin_percent_ok(node.style.margin_top, containing)
                && margin_percent_ok(node.style.margin_bottom, containing))
            {
                result.unsupported = true;
            }
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
                BoxKind::Block
                | BoxKind::ListItem
                | BoxKind::AnonymousBlock
                | BoxKind::Flex
                | BoxKind::Image
                | BoxKind::Table
                | BoxKind::TableRowGroup
                | BoxKind::TableRow
                | BoxKind::TableCell
                | BoxKind::TableCaption
                | BoxKind::TableColumn => {
                    let width = containing.map(|base| {
                        if node.kind == BoxKind::AnonymousBlock {
                            used_block_width(Length::Auto, base)
                        } else {
                            let solve = |w: Length| {
                                used_block_width_sized(
                                    w,
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
                            };
                            let mut used = solve(node.style.width);
                            // CANDIDATE (ext only): CSS 2.1 10.4 min/max-width clamp for an admitted in-flow static block.
                            // `solve` takes a content-box width for ContentBox and a border-box width for BorderBox, and min/max
                            // apply to the same box as `width`, so the clamp compares and re-solves in that box (FL31).
                            if tree.width_clamp_admitted(index) {
                                let edges = node.style.padding_left.saturating_add(node.style.padding_right);
                                let in_box = |content: u32| {
                                    if node.style.box_sizing == BoxSizing::BorderBox { content.saturating_add(edges) } else { content }
                                };
                                if let Length::Px(max) = node.style.max_width {
                                    if in_box(used.content) > max {
                                        used = solve(Length::Px(max));
                                    }
                                }
                                if let Length::Px(min) = node.style.min_width {
                                    if in_box(used.content) < min {
                                        used = solve(Length::Px(min));
                                    }
                                }
                            }
                            used
                        }
                    });
                    let (cb_width, cb_x) =
                        result.positioned_containing_width(tree, index, viewport_width);
                    let (stretch_w, _) = tree.stretch_axes(index);
                    let width = if stretch_w {
                        let [_, right, _, left] = node.style.positioned.offsets;
                        let used = match (cb_width, left, right) {
                            (Some(cb), Margin::Px(l), Margin::Px(r)) => i64::from(cb)
                                .checked_sub(i64::from(l))
                                .and_then(|v| v.checked_sub(i64::from(r)))
                                .and_then(|v| v.checked_sub(i64::from(node.style.padding_left)))
                                .and_then(|v| v.checked_sub(i64::from(node.style.padding_right)))
                                .and_then(|v| u32::try_from(v).ok()),
                            _ => None,
                        };
                        if used.is_none() { result.unsupported = true; }
                        used.map(|content| UsedBlockWidth { content, margin_left: 0, margin_right: 0 })
                    } else if node.style.positioned.position == Position::Absolute {
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
                                (Margin::Px(left), r) if tree.centered_axes(index).0 => {
                                    let Margin::Px(right) = r else { return None };
                                    let free = i64::from(cb_width?) - i64::from(left) - i64::from(right) - outer;
                                    if free < 0 || free % 2 != 0 { return None; }
                                    cb_x?.checked_add(i64::from(left))?.checked_add(free / 2)
                                }
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
                    // Kernel float:right: the float's right margin edge sits on the parent's
                    // content right edge (zero edges, resolved width > 0, fits the parent).
                    let x = if node.style.positioned.float == Float::Right && tree.is_kernel_float(index) {
                        match (containing_x, containing, width) {
                            (Some(px), Some(pw), Some(w)) => px
                                .checked_add(i64::from(pw))
                                .and_then(|r| r.checked_sub(i64::from(w.content))),
                            _ => None,
                        }
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
        if !tree.overflow_hidden_contained(&result.widths, &result.content_x) {
            result.unsupported = true;
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
                && !self.width_clamp_admitted_node(node)
        })
    }

    /// CANDIDATE (ext only, FL29): a plain in-flow static content-box LTR `Block` whose parent is a `Block` (or a root) with
    /// auto/px width and auto/px min/max-width. The width pass applies the CSS 2.1 10.4 clamp to exactly these boxes.
    fn width_clamp_admitted_node(&self, node: &CssBox) -> bool {
        self.ext
            && node.kind == BoxKind::Block
            && node.style.positioned.position == Position::Static
            && node.style.positioned.float == Float::None
            && node.style.positioned.clear == Clear::None
            && node.style.direction == Direction::Ltr
            && node.style.display == Display::Block
            && matches!(node.style.width, Length::Auto | Length::Px(_) | Length::Percent(_))
            && matches!(node.style.min_width, Length::Auto | Length::Px(_))
            && matches!(node.style.max_width, Length::Auto | Length::Px(_))
            && node.parent.is_none_or(|p| self.boxes.get(p).is_some_and(|b| b.kind == BoxKind::Block))
    }

    pub(crate) fn width_clamp_admitted(&self, index: usize) -> bool {
        self.boxes.get(index).is_some_and(|n| self.width_clamp_admitted_node(n))
    }

    /// CANDIDATE (ext only). True when some box carries a POSITIVE `min-height` that the height pass cannot prove: only an
    /// in-flow, non-floated, non-cleared static content-box `Block` of auto height that is not a flex item, not part of a float kernel
    /// and has no positioned child is admitted. Everything else fails closed (flex stretch, abspos offsets and float
    /// kernels read heights this change does not touch).
    pub(crate) fn stray_min_height(&self) -> bool {
        self.boxes.iter().enumerate().any(|(index, node)| {
            node.kind != BoxKind::Text
                && node.style.min_height != Length::Auto
                && !(node.kind == BoxKind::Block
                    && node.style.height == Length::Auto
                    && node.style.box_sizing == BoxSizing::ContentBox
                    && node.style.positioned.position == Position::Static
                    && node.style.positioned.float == Float::None
                    && node.style.positioned.clear == Clear::None
                    && self.float_kernel(index).is_none()
                    && node.parent.is_some_and(|p| {
                        self.boxes.get(p).is_some_and(|b| b.kind != BoxKind::Flex) && self.float_kernel(p).is_none()
                    })
                    && node.children.iter().all(|&c| self.boxes[c].style.positioned.position == Position::Static))
        })
    }

    /// CANDIDATE (ext only, FL22). True when some box carries `overflow:hidden` that this build cannot prove inert. No clip is
    /// painted, so `hidden` is admitted ONLY as a block formatting context whose clip can never matter: the box is a static,
    /// in-flow, content-box, auto-height, non-floated `Block` (not a flex item) with no min-height, at least one child, and the
    /// page has NO float or clear anywhere (a float context changes the hidden box itself); every descendant is a static `Block`
    /// with non-negative px margins and auto height (or an explicit px height with no children) - so no text, inline, positioned
    /// or percentage box can overflow. Horizontal containment is then checked on USED edges in `overflow_hidden_contained`.
    /// Glyph ink is excluded by construction (no text descendants). Everything else fails closed.
    pub(crate) fn stray_overflow_hidden(&self) -> bool {
        // CANDIDATE (FL34, ext only): a non-visible `overflow` on the ROOT element alone propagates to the viewport, so the root
        // box itself stays visible (no clip, and a root is already a block formatting context). It is admitted only when NO other
        // box carries a non-visible overflow: with an overflow on `body` too, `body` would no longer propagate and would clip itself.
        let any_hidden = self
            .boxes
            .iter()
            .any(|b| b.style.overflow_hidden && !(self.ext && b.parent.is_none()));
        if !any_hidden {
            return false;
        }
        if self.has_float_or_clear() {
            return true;
        }
        self.boxes.iter().enumerate().any(|(index, node)| {
            if !node.style.overflow_hidden {
                return false;
            }
            let ok_self = node.kind == BoxKind::Block
                && match node.style.height {
                    Length::Auto => true,
                    // CANDIDATE (FL43, ext only): an explicit px height is admitted only when a conservative UPPER BOUND of the
                    // children's stacked margin-box extent (sibling margin collapse can only shrink it) fits in that height.
                    Length::Px(h) => self.ext && self.stacked_extent_bound(index).is_some_and(|e| e <= u64::from(h)),
                    Length::Percent(_) => false,
                }
                && node.style.min_height == Length::Auto
                && node.style.box_sizing == BoxSizing::ContentBox
                && node.style.positioned.position == Position::Static
                && !node.children.is_empty()
                && node.parent.is_some_and(|p| self.boxes.get(p).is_some_and(|b| b.kind != BoxKind::Flex));
            if !ok_self {
                return true;
            }
            let mut stack: Vec<usize> = node.children.clone();
            while let Some(i) = stack.pop() {
                let d = &self.boxes[i];
                let margins_ok = matches!(
                    (d.style.margin_left, d.style.margin_right, d.style.margin_top, d.style.margin_bottom),
                    (Margin::Px(0..), Margin::Px(0..), Margin::Px(0..), Margin::Px(0..))
                );
                let height_ok = match d.style.height {
                    Length::Auto => true,
                    Length::Px(_) => d.children.is_empty(),
                    Length::Percent(_) => false,
                };
                if d.kind != BoxKind::Block
                    || d.style.positioned.position != Position::Static
                    || !margins_ok
                    || !height_ok
                    || d.style.min_height != Length::Auto
                {
                    return true;
                }
                stack.extend(d.children.iter().copied());
            }
            false
        })
    }

    /// FL43: upper bound (px) of the content height of box `index`: children's margin boxes summed. Only non-negative px margins,
    /// px or auto heights (auto resolved recursively) are measured; anything else gives `None`. Called only on trees whose
    /// descendants already satisfy the FL22 shape rules (checked by the same walk), so this is a bound, not a layout.
    fn stacked_extent_bound(&self, index: usize) -> Option<u64> {
        let mut total = 0_u64;
        for &c in &self.boxes[index].children {
            let d = &self.boxes[c];
            let (Margin::Px(mt), Margin::Px(mb)) = (d.style.margin_top, d.style.margin_bottom) else { return None };
            let own = u64::from(d.style.padding_top) + u64::from(d.style.padding_bottom);
            let inner = match d.style.height {
                Length::Px(h) => u64::from(h),
                Length::Auto => self.stacked_extent_bound(c)?,
                Length::Percent(_) => return None,
            };
            total += u64::from(mt.max(0).unsigned_abs()) + u64::from(mb.max(0).unsigned_abs()) + own + inner;
        }
        Some(total)
    }

    /// FL22: after widths are placed, every descendant margin box of an `overflow:hidden` box must lie inside its padding box.
    fn overflow_hidden_contained(&self, widths: &[Option<UsedBlockWidth>], xs: &[Option<i64>]) -> bool {
        for (h, node) in self.boxes.iter().enumerate() {
            if !node.style.overflow_hidden {
                continue;
            }
            let (Some(hw), Some(hx)) = (widths[h], xs[h]) else { return false };
            // style.padding_* FOLDS border widths in (from_style_inner), so the PADDING box (the clip edge) is the folded
            // padding minus the used border on each side. FL23 fix: FL22 used the border box here and admitted a child in
            // the right border band (reviewer ov7: Chrome clips at the padding edge).
            let hb = node.style.decor.used_border();
            let lo = hx - i64::from(node.style.padding_left.saturating_sub(hb[3]));
            let hi = hx + i64::from(hw.content) + i64::from(node.style.padding_right.saturating_sub(hb[1]));
            let mut stack: Vec<usize> = node.children.clone();
            while let Some(i) = stack.pop() {
                let d = &self.boxes[i];
                let (Some(w), Some(x)) = (widths[i], xs[i]) else { return false };
                // The descendant's folded padding already includes its own border (FL22 counted it twice: fail-closed
                // over-refusal, removed in FL23).
                let left = x - i64::from(d.style.padding_left) - w.margin_left.max(0);
                let right = x + i64::from(w.content) + i64::from(d.style.padding_right) + w.margin_right.max(0);
                // An over-constrained auto-width/px-width box gets a NEGATIVE used margin-right: the border box still
                // overflows, so only non-negative margins count toward the right edge.
                if left < lo || right > hi {
                    return false;
                }
                stack.extend(d.children.iter().copied());
            }
        }
        true
    }

    /// Used minimum height in px (0 when `auto`); only meaningful after `stray_min_height` is false.
    fn min_height_px(&self, index: usize) -> u32 {
        match self.boxes[index].style.min_height {
            Length::Px(n) => n,
            _ => 0,
        }
    }

    pub(crate) fn paint_structure_valid(&self, limit: usize) -> bool {
        self.paint_structure_valid_with(limit, false)
    }

    /// Shared paint gate. `allow_float_kernel` is passed ONLY by the explicit
    /// background float-kernel entry, whose painter orders floats above in-flow
    /// block backgrounds. Every other consumer keeps float/clear fail-closed.
    pub(crate) fn paint_structure_valid_with(&self, limit: usize, allow_float_kernel: bool) -> bool {
        self.paint_structure_valid_layers(limit, allow_float_kernel, false)
    }

    /// `allow_relative` is passed by painters with a positioned-layer phase (decorated background painter, text painter) that paint a relative box as a
    /// positioned layer (App. E). The text painter (css_text_page) and the decorated background painter pass true; every
    /// other consumer (the padding/content rectangle primitives) refuses `position:relative` here (fail-closed).
    pub(crate) fn paint_structure_valid_layers(&self, limit: usize, allow_float_kernel: bool, allow_relative: bool) -> bool {
        let mode = if allow_float_kernel {
            PaintMode::FloatKernel
        } else {
            PaintMode::Default
        };
        self.paint_structure_valid_layers_mode(limit, mode, allow_relative)
    }

    pub(crate) fn paint_structure_valid_layers_mode(
        &self,
        limit: usize,
        mode: PaintMode,
        allow_relative: bool,
    ) -> bool {
        if !allow_relative
            && self
                .boxes
                .iter()
                .any(|b| b.style.positioned.position == Position::Relative)
        {
            return false;
        }
        let count = self.boxes.len();
        if count == 0 || count > limit || self.roots.len() != 1 || self.roots[0] >= count {
            return false;
        }
        // Float/clear have diagnostic layout geometry. Only the explicit background
        // float-kernel painter (`FloatKernel`) orders floats above in-flow
        // block backgrounds (CSS 2.2 App. E); every other paint consumer refuses
        // them here, as does the kernel painter outside a single confined kernel.
        // The generated-block entry refuses float/clear tree-globally instead.
        if self.has_float_or_clear()
            && !(mode == PaintMode::FloatKernel && !self.unsupported && self.float_clear_confined())
        {
            return false;
        }
        // Generated boxes (`::before`/`::after`): the DEFAULT consumer refuses
        // them all; the explicit kernel painter admits only the cleared child of
        // the confined float kernel; the explicit generated-block entry admits
        // only `generated_block_admitted` ordinary blocks (symmetric across both
        // families). Every other placement refuses here.
        if self.boxes.iter().enumerate().any(|(i, b)| {
            b.generated
                && match mode {
                    PaintMode::Default => true,
                    PaintMode::FloatKernel => self.unsupported || !self.is_kernel_cleared(i),
                    PaintMode::GeneratedBlock => {
                        self.unsupported
                            || self.has_float_or_clear()
                            || !self.generated_block_admitted(i)
                    }
                }
        }) {
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
                    && !node.parent.is_some_and(|p| self.boxes[p].kind == BoxKind::Flex)
                    && !self.width_clamp_admitted_node(node))
                || self.stray_min_height()
                || self.stray_overflow_hidden()
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
    /// Data-URI payloads of the image boxes in document order; the harness
    /// pairs them with sentinel rectangles in the raster. Capped at 4096,
    /// the deleted intake rewrite's cap.
    #[must_use]
    pub fn images_in_order(&self) -> Vec<Vec<u8>> {
        self.boxes
            .iter()
            .filter(|b| b.kind == BoxKind::Image)
            .take(4096)
            .filter_map(|b| {
                b.image
                    .as_ref()
                    .and_then(|f| f.src_data_uri.clone().map(String::into_bytes))
            })
            .collect()
    }

    pub fn from_style(styled: &StyleTree) -> Self {
        Self::from_style_inner(styled, false)
    }

    /// EXPERIMENTAL: like [`BoxTree::from_style`] but a non-`none` `text-decoration` does not
    /// by itself mark the tree unsupported. Only a painter that validates and draws the
    /// decoration (`css_text_page::paint_direct_text_blocks_underline`) may use this tree;
    /// every other consumer must go through `from_style`, which refuses it.
    #[must_use]
    pub fn from_style_with_text_decoration(styled: &StyleTree) -> Self {
        Self::from_style_inner(styled, true)
    }

    #[must_use]
    #[allow(clippy::too_many_lines)]
    fn from_style_inner(styled: &StyleTree, allow_text_decoration: bool) -> Self {
        let mut tree = Self {
            truncated: styled.truncated,
            unsupported: styled.unsupported,
            quirks: styled.quirks,
            ext: styled.ext,
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
        let mut page_list_counts = std::collections::BTreeMap::<usize, usize>::new();
        while let Some((source, parent)) = stack.pop() {
            if collapsed[source] {
                continue;
            }
            let node = &styled.nodes[source];
            // <picture> with a qualifying img child: the box tree cannot
            // keep a block-level image inside the inline chain, so the
            // picture element vanishes and its children attach to the
            // picture's parent (the old marker-div hoist's shape).
            if node.tag == "picture"
                && node.children.iter().any(|&c| {
                    styled.nodes[c]
                        .image
                        .as_ref()
                        .is_some_and(|f| image_intrinsic(f).is_some())
                })
            {
                for &child in node.children.iter().rev() {
                    stack.push((child, parent));
                }
                continue;
            }
            let kind = if node.text.is_some() {
                Some(BoxKind::Text)
            } else if let Some(facts) = &node.image {
                match node.style.display {
                    Display::None => None,
                    // Replaced image with a data-URI source and resolvable
                    // dimensions. `display:none` still removes it.
                    _ if image_intrinsic(facts).is_some() => Some(BoxKind::Image),
                    // No usable image: the alt text, exactly the old intake
                    // rewrite's fallback.
                    _ => match &facts.alt {
                        Some(alt) if !alt.is_empty() => Some(BoxKind::Text),
                        _ => None,
                    },
                }
            } else {
                match node.style.display {
                    Display::None => None,
                    Display::Block => Some(BoxKind::Block),
                    Display::Inline => Some(BoxKind::Inline),
                    Display::InlineBlock => Some(BoxKind::InlineBlock),
                    // `list-style-type: none` means no marker STRING; a marker box can still be generated by
                    // `::marker` styling or `list-style-image`, and a list item still increments the implicit
                    // `list-item` counter. Treating it as a plain Block drops both facts, which is only safe
                    // while every way to observe them is refused (`::marker`, list-style-image/position/shorthand,
                    // counter-reset/-increment/-set, counter()/counters() in content; tested). If any of those
                    // is ever accepted this mapping MUST be revisited. Any other list item keeps its
                    // (unmodelled) marker and stays refused.
                    Display::ListItem if node.style.list_style_none => Some(BoxKind::Block),
                    Display::ListItem => Some(BoxKind::ListItem),
                    Display::Flex => Some(BoxKind::Flex),
                    Display::Table => Some(BoxKind::Table),
                    Display::TableRowGroup => Some(BoxKind::TableRowGroup),
                    Display::TableRow => Some(BoxKind::TableRow),
                    Display::TableCell => Some(BoxKind::TableCell),
                    Display::TableCaption => Some(BoxKind::TableCaption),
                    Display::TableColumn => Some(BoxKind::TableColumn),
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
            if matches!(
                kind,
                BoxKind::Table
                    | BoxKind::TableRowGroup
                    | BoxKind::TableRow
                    | BoxKind::TableCell
                    | BoxKind::TableCaption
                    | BoxKind::TableColumn
            ) {
                // Legacy exact painters do not implement table formatting.
                // The separate basic grid painter must own these roles.
                tree.unsupported = true;
            }
            // Every pass below sees one edge per side: padding plus border.
            // The border widths stay in `style.decor` for painting.
            let mut style = node.style;
            // CANDIDATE (FL38, ext only): `visibility:hidden` keeps layout and paints nothing. Admitted only where the ONLY paint a
            // box could make is its background: no text, no visible border, outline or shadow, not the root or body (canvas
            // propagation), and no `visibility:visible` descendant of a hidden box (the cascade copies the parent's value, so a
            // visible child under a hidden parent is exactly that case). The background is dropped here; geometry is untouched.
            if node.style.positioned.hidden {
                let shallow = parent.is_none() || matches!(node.tag.as_str(), "html" | "body");
                let paints_more = node.text.is_some()
                    || node.style.box_shadow.is_some()
                    || node.style.outline_solid
                    || node.style.outline_color.is_some()
                    || node.style.decor.used_border().iter().any(|w| *w > 0);
                if !tree.ext || shallow || paints_more {
                    tree.unsupported = true;
                }
                style.background_color = None;
                style.background_current_color = false;
            } else if parent.is_some_and(|p: usize| tree.boxes[p].style.positioned.hidden) {
                // visible child of a hidden parent
                tree.unsupported = true;
            }
            if node.text.is_some() {
                style.positioned = crate::computed_style::Positioned::INITIAL;
            }
            // CANDIDATE (FL32, ext only): `position:fixed` at scroll offset 0 is laid out and painted as `absolute` against the
            // initial containing block. That equivalence needs NO positioned ancestor (a positioned ancestor would be the absolute
            // containing block, a fixed box ignores it); anything else, or a non-ext tree, refuses.
            // CANDIDATE (FL35, ext only): `position:sticky` at scroll offset 0 with all-auto offsets or exactly `top:0` never moves
            // (its natural y >= 0), so it is laid out and painted as `relative` with all-auto offsets; every other offset refuses.
            if style.positioned.position == Position::Sticky {
                let [top, right, bottom, left] = style.positioned.offsets;
                let only_top_zero = matches!(top, Margin::Auto | Margin::Px(0))
                    && right == Margin::Auto
                    && bottom == Margin::Auto
                    && left == Margin::Auto;
                if tree.ext && only_top_zero {
                    style.positioned.position = Position::Relative;
                    style.positioned.offsets = [Margin::Auto; 4];
                } else {
                    tree.unsupported = true;
                }
            }
            if style.positioned.position == Position::Fixed {
                let mut anc: Option<usize> = parent;
                let mut positioned_ancestor = false;
                while let Some(p) = anc {
                    positioned_ancestor |= tree.boxes[p].style.positioned.position != Position::Static;
                    anc = tree.boxes[p].parent;
                }
                if tree.ext && !positioned_ancestor {
                    style.positioned.position = Position::Absolute;
                } else {
                    tree.unsupported = true;
                }
            }
            let [top, right, bottom, left] = style.decor.used_border();
            style.padding_top = style.padding_top.saturating_add(top);
            style.padding_right = style.padding_right.saturating_add(right);
            style.padding_bottom = style.padding_bottom.saturating_add(bottom);
            style.padding_left = style.padding_left.saturating_add(left);
            // Preserve semantic pre formatting and ordered-list numbering for Page view.
            if style.white_space_pre { tree.unsupported = true; }
            if kind == BoxKind::ListItem {
                let marker = node.parent.and_then(|p| {
                    if styled.nodes[p].tag != "ol" { return None; }
                    let number = page_list_counts.entry(p).or_default();
                    *number += 1;
                    Some(format!("{number}. "))
                }).unwrap_or_else(|| "• ".to_owned());
                tree.page_list_markers.insert(index, marker);
            }
            tree.boxes.push(CssBox {
                inline_phrase: node.inline_phrase,
                inline_attributes: node.inline_attributes,
                kind,
                source: Some(source),
                parent,
                children: Vec::new(),
                style,
                text: if kind == BoxKind::Text && node.image.is_some() {
                    // The alt-text fallback box: the element carries the text.
                    node.image.as_ref().and_then(|f| f.alt.clone())
                } else {
                    node.text.clone()
                },
                image: node.image.clone(),
                table_span: node.table_span,
                // Same generated-box paint confinement for both pseudo families:
                // kernel-cleared only, by the explicit kernel painter.
                generated: node.tag == "::after" || node.tag == "::before",
            });
            if let Some(parent) = parent {
                tree.boxes[parent].children.push(index);
            } else {
                tree.roots.push(index);
            }
            if kind == BoxKind::Image {
                // Marker geometry, moved from the deleted intake rewrite:
                // block box, padding-top = intrinsic height scaled to the
                // 800px content width (capped), sentinel background the
                // harness swaps for decoded pixels, alt text as the
                // pre-blit paint. Image-specific author styles are not
                // applied, matching the marker div.
                let facts = node.image.as_ref().expect("image box facts").clone();
                let (iw, ih) = image_intrinsic(&facts).expect("qualified image box");
                let idx = tree.image_count;
                tree.image_count += 1;
                let (r, g, b) = crate::basic_page::image_sentinel(idx);
                let width = 800u32;
                // Attribute/header dimensions are untrusted u32 values. Widen
                // before multiplying, then clamp before narrowing for layout.
                let sh = if iw > width {
                    (u64::from(ih) * u64::from(width) / u64::from(iw)).min(4096) as u32
                } else {
                    ih.min(4096)
                };
                let style = &mut tree.boxes[index].style;
                style.display = Display::Block;
                style.padding_top = sh;
                style.padding_right = 0;
                style.padding_bottom = 0;
                style.padding_left = 0;
                style.background_color =
                    Some((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
                style.background_current_color = false;
                if let Some(alt) = facts.alt.filter(|a| !a.is_empty()) {
                    let alt_style = Computed {
                        display: Display::Inline,
                        ..Computed::cascade(Some(tree.boxes[index].style), &[])
                    };
                    let alt_index = tree.boxes.len();
                    tree.boxes.push(CssBox {
                        inline_phrase: false,
                        inline_attributes: false,
                        kind: BoxKind::Text,
                        source: Some(source),
                        parent: Some(index),
                        children: Vec::new(),
                        style: alt_style,
                        text: Some(alt),
                        image: None,
                        table_span: (1, 1),
                        generated: false,
                    });
                    tree.boxes[index].children.push(alt_index);
                }
            }
            for &child in node.children.iter().rev() {
                stack.push((child, Some(index)));
            }
        }
        // CANDIDATE (FL42, ext only): `clear` is a no-op when the page has no float at all (clearance only exists against a float
        // in the same block formatting context), so every `clear` is reset to none BEFORE the float/clear and margin passes. Any
        // float anywhere in the tree keeps every clear as written (the existing float-kernel rules decide).
        if tree.ext && tree.boxes.iter().all(|b| b.style.positioned.float == Float::None) {
            for b in &mut tree.boxes {
                b.style.positioned.clear = Clear::None;
            }
        }
        // CANDIDATE (FL39, ext only): a positive px `min-height` on a box with a px `height` is folded into the height
        // (used = max(height, min-height); no max-height exists in this build) and min-height is reset to auto, so every later
        // pass sees one explicit height. Run AFTER the tree is built so it can use the SAME exclusions as `stray_min_height`
        // (parity): in-flow static non-floated non-cleared content-box `Block`, not a float-kernel member or kernel parent,
        // non-flex parent, only static children (an absolutely positioned child keeps refusing, conservatively), and never the
        // root (no parent). Every other shape keeps the old rule and refuses.
        if tree.ext {
            let fold: Vec<usize> = (0..tree.boxes.len())
                .filter(|&i| {
                    let n = &tree.boxes[i];
                    matches!(n.style.height, Length::Px(_))
                        && (matches!(n.style.min_height, Length::Px(m) if m > 0) || matches!(n.style.positioned.max_height, Length::Px(_)))
                        && n.kind == BoxKind::Block
                        && n.style.box_sizing == BoxSizing::ContentBox
                        && n.style.positioned.position == Position::Static
                        && n.style.positioned.float == Float::None
                        && n.style.positioned.clear == Clear::None
                        && tree.float_kernel(i).is_none()
                        && n.parent.is_some_and(|p: usize| {
                            tree.boxes[p].kind != BoxKind::Flex && tree.float_kernel(p).is_none()
                        })
                        && n.children.iter().all(|&c| tree.boxes[c].style.positioned.position == Position::Static)
                })
                .collect();
            for i in fold {
                // FL40: CSS 2 10.7: used = max(min, min(height, max)); max-height:none (Auto) skips the clamp. The fold already
                // refused (stayed unfolded) every shape outside the parity gate, so a surviving non-none max-height refuses below.
                if let Length::Px(h) = tree.boxes[i].style.height {
                    let mut used = h;
                    if let Length::Px(mx) = tree.boxes[i].style.positioned.max_height {
                        used = used.min(mx);
                    }
                    if let Length::Px(m) = tree.boxes[i].style.min_height {
                        used = used.max(m);
                    }
                    tree.boxes[i].style.height = Length::Px(used);
                    tree.boxes[i].style.min_height = Length::Auto;
                    tree.boxes[i].style.positioned.max_height = Length::Auto;
                }
            }
            if tree.boxes.iter().any(|b| b.style.positioned.max_height != Length::Auto) {
                tree.unsupported = true;
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
                    && ((!matches!(node.style.width, Length::Px(_)) && !tree.stretch_axes(index).0)
                        || (!matches!(node.style.height, Length::Px(_)) && !tree.stretch_axes(index).1)))
                || node.style.direction != Direction::Ltr
                || [
                    node.style.margin_top,
                    node.style.margin_right,
                    node.style.margin_bottom,
                    node.style.margin_left,
                ]
                .iter()
                .any(|m| *m != Margin::Px(0))
                    && {
                        let (ch, cv) = tree.centered_axes(index);
                        let zero = |a: Margin, b: Margin| a == Margin::Px(0) && b == Margin::Px(0);
                        !((ch || zero(node.style.margin_left, node.style.margin_right))
                            && (cv || zero(node.style.margin_top, node.style.margin_bottom)))
                    }
            {
                tree.unsupported = true;
            }
            let nested = node.parent.is_some_and(|p| {
                matches!(tree.boxes[p].style.positioned.position, Position::Absolute | Position::Relative)
            });
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
        // CANDIDATE (ext entry): `position:relative` is admitted only with no offset and none of the modelled overlap sources
        // anything: Block/ListItem box, all four offsets auto, and the whole page has no float, clear,
        // absolute box or negative margin (the overlap sources this engine models; overflow, text-glyph overflow and
        // other unmodelled overlap are NOT excluded). An absolute box is admitted only as a direct leaf child of a relative Block (B1); any other absolute box refuses. Its
        // positioned-layer paint order (CSS 2.2 App. E) DOES differ from static order (see the order test); only the modelled overlap
        // sources are refused, so this is a bounded background-bridge admission, not a proof of no overlap. The text painter has a positioned-layer phase;
        // the rectangle-only primitives refuse relative in paint_structure_valid_layers.
        // Anything else, including every nonzero offset, stays refused.
        if tree.boxes.iter().any(|b| b.style.positioned.position == Position::Relative) {
            let overlap_source = tree.boxes.iter().any(|b| {
                (b.style.positioned.position == Position::Absolute
                    && !b.parent.is_some_and(|p| tree.boxes[p].style.positioned.position == Position::Relative))
                    || b.style.positioned.float != Float::None
                    || b.style.positioned.clear != Clear::None
                    || [b.style.margin_top, b.style.margin_right, b.style.margin_bottom, b.style.margin_left]
                        .iter()
                        .any(|m| matches!(m, Margin::Px(v) if *v < 0))
            });
            let bad = tree.boxes.iter().any(|b| {
                b.style.positioned.position == Position::Relative
                    && (!matches!(b.kind, BoxKind::Block | BoxKind::ListItem)
                        || b.style.positioned.offsets != [Margin::Auto; 4]
                        || b.style.direction != Direction::Ltr)
            });
            if overlap_source || bad {
                tree.unsupported = true;
            }
        }
        // Iterate only the original boxes. Anonymous wrappers append later and
        // must not themselves be normalized again.
        let originals = tree.boxes.len();
        for parent in (0..originals).rev() {
            let kind = tree.boxes[parent].kind;
            let children = &tree.boxes[parent].children;
            let has_block = children.iter().any(|&child| {
                matches!(
                    tree.boxes[child].kind,
                    BoxKind::Block
                        | BoxKind::ListItem
                        | BoxKind::Flex
                        | BoxKind::Image
                        | BoxKind::Table
                        | BoxKind::TableRowGroup
                        | BoxKind::TableRow
                        | BoxKind::TableCell
                        | BoxKind::TableCaption
                        | BoxKind::TableColumn
                )
            });
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
                if matches!(
                    tree.boxes[child].kind,
                    BoxKind::Block
                        | BoxKind::ListItem
                        | BoxKind::Flex
                        | BoxKind::Image
                        | BoxKind::Table
                        | BoxKind::TableRowGroup
                        | BoxKind::TableRow
                        | BoxKind::TableCell
                        | BoxKind::TableCaption
                        | BoxKind::TableColumn
                ) {
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
        if !tree.float_clear_confined() {
            tree.unsupported = true;
        }
        if !tree.nowrap_texts_have_no_break_opportunity() {
            tree.unsupported = true;
        }
        if !allow_text_decoration
            && tree
                .boxes
                .iter()
                .any(|b| b.style.text_decoration != crate::computed_style::TextDecoration::None)
        {
            tree.unsupported = true;
        }
        tree
    }

    /// CANDIDATE (ext): `white-space: nowrap` equals normal wrapping when the line formatting context has no break
    /// opportunity at all. For every block container that directly holds nowrap text, ALL its inline content (text of
    /// the container and of nested inline boxes, nowrap or not) is concatenated; after trimming ASCII whitespace at the
    /// container's own edges (a line boundary: safe) it must contain no ASCII whitespace, and the content must hold no
    /// atomic inline (inline-block). Trimming at a nowrap ELEMENT boundary is not done (NW1 failed on that: Chrome
    /// breaks at the space across a span edge). Anything else is refused.
    fn nowrap_texts_have_no_break_opportunity(&self) -> bool {
        let ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}');
        let is_container = |k: BoxKind| matches!(k, BoxKind::Block | BoxKind::AnonymousBlock | BoxKind::ListItem | BoxKind::Flex | BoxKind::InlineBlock);
        let mut checked = vec![false; self.boxes.len()];
        for (index, b) in self.boxes.iter().enumerate() {
            if b.text.is_none() || !b.style.white_space_nowrap {
                continue;
            }
            let mut container = b.parent;
            while let Some(c) = container {
                if is_container(self.boxes[c].kind) {
                    break;
                }
                container = self.boxes[c].parent;
            }
            let Some(container) = container else {
                return false;
            };
            let _ = index;
            if std::mem::replace(&mut checked[container], true) {
                continue;
            }
            let mut text = String::new();
            let mut stack: Vec<usize> = self.boxes[container].children.iter().rev().copied().collect();
            while let Some(i) = stack.pop() {
                let node = &self.boxes[i];
                match node.kind {
                    BoxKind::Text => text.push_str(node.text.as_deref().unwrap_or("")),
                    BoxKind::Inline => stack.extend(node.children.iter().rev()),
                    // Atomic inlines and nested block-level boxes: block children are separate containers;
                    // an inline-block next to nowrap text is refused rather than modelled.
                    BoxKind::InlineBlock => return false,
                    _ => {}
                }
            }
            if text.trim_matches(ws).chars().any(ws) {
                return false;
            }
        }
        true
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
            image: None,
            table_span: (1, 1),
            generated: false,
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
    fn img_becomes_an_image_box_with_its_facts() {
        let tree = boxes(
            "<div><img src=\"data:image/png;base64,AAAA\" alt=\"A\" width=10 height=20></div>",
        );
        let image_boxes: Vec<_> = tree
            .boxes
            .iter()
            .filter(|b| b.kind == BoxKind::Image)
            .collect();
        assert_eq!(image_boxes.len(), 1);
        let facts = image_boxes[0].image.as_ref().expect("image facts");
        assert_eq!(
            facts.src_data_uri.as_deref(),
            Some("data:image/png;base64,AAAA")
        );
        assert_eq!(facts.alt.as_deref(), Some("A"));
        assert_eq!(facts.attr_width, Some(10));
        assert_eq!(facts.attr_height, Some(20));
        // display:none still removes the box entirely.
        let hidden = boxes("<div><img style=\"display:none\" alt=\"A\"></div>");
        assert!(!hidden.boxes.iter().any(|b| b.kind == BoxKind::Image));
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
        // CHANGED by the private percent-guard candidate (UNREVIEWED): 50% of 25px is 12.5,
        // a fractional LayoutUnit the integer pipeline floors to 12. The guard now marks
        // this unsupported (it used to assert `!unsupported`). Not a Chrome-parity claim.
        assert!(HeightPass::from_boxes(&tree, 100).unsupported);
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

#[cfg(test)]
mod float_kernel_tests {
    use super::*;
    use crate::{parse_style_tree, Limits};

    fn tree_for(inner: &str, css: &str) -> BoxTree {
        let html = format!("<!doctype html><style>{css}</style><body>{inner}</body>");
        let mut styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        for node in styled.nodes.iter_mut().filter(|n| n.tag == "body") {
            node.style.margin_top = Margin::Px(0);
            node.style.margin_right = Margin::Px(0);
            node.style.margin_bottom = Margin::Px(0);
            node.style.margin_left = Margin::Px(0);
        }
        BoxTree::from_style(&styled)
    }

    const BASE: &str = ".p{width:400px}.f{float:left;width:100px;height:50px}.c{clear:both;height:10px}.n{width:400px;height:7px}";
    const MARKUP: &str =
        "<div class=p><div class=f></div><div class=c></div></div><div class=n></div>";

    /// Chrome 156 reference (private floatkernel/v2/chrome-v2.txt, a1-kernel):
    /// parent h60, float 100x50 at y0, cleared block y50 h10, next block y60.
    #[test]
    fn kernel_layout_numbers_match_recorded_chrome_xwh_and_y() {
        let tree = tree_for(MARKUP, BASE);
        assert!(!tree.unsupported);
        let h = UsedHeightPass::from_boxes(&tree, 400, 600);
        let v = VerticalPass::from_boxes(&tree, 400, 600);
        assert!(!h.unsupported && !v.unsupported);
        let p = (0..tree.boxes.len()).find(|&i| tree.float_kernel(i).is_some()).unwrap();
        let (f, c) = tree.float_kernel(p).unwrap();
        let n = tree.boxes.len() - 1;
        assert_eq!(h.heights[p], Some(60));
        assert_eq!((v.content_y[p], v.content_y[f]), (Some(0), Some(0)));
        assert_eq!((v.content_y[c], h.heights[c]), (Some(50), Some(10)));
        assert_eq!(v.content_y[n], Some(60));
    }

    /// Review blocker (4:29): a parent shorter than float + cleared block lets the
    /// float overflow onto the next sibling; paint order would draw that sibling's
    /// background over the float (Chrome paints the float above). Layout is now
    /// admitted (see float_kernel); default painters refuse and only the explicit
    /// float-kernel background painter draws it, float last.
    #[test]
    fn short_parent_overflow_is_layout_admitted_but_paint_refused_by_default_gate() {
        for h in [10, 59] {
            let tree = tree_for(MARKUP, &format!("{BASE}.p{{height:{h}px}}"));
            let v = VerticalPass::from_boxes(&tree, 400, 600);
            assert!(!tree.unsupported && !v.unsupported, "height {h} layout");
            // Chrome b1: next block y = parent height (overlaps the float)
            assert_eq!(v.content_y[tree.boxes.len() - 1], Some(i64::from(h)));
            assert!(!tree.paint_structure_valid(4096), "default painters refuse height {h}");
        }
    }

    /// Every paint consumer shares `paint_structure_valid`; admitted float geometry
    /// must never reach a painter (paint-phase overlap, review blocker 4:29).
    #[test]
    fn admitted_float_layout_is_refused_by_the_paint_gate() {
        for css in [BASE.to_string(), format!("{BASE}.p{{height:60px}}"), format!("{BASE}.p{{height:200px}}")] {
            let tree = tree_for(MARKUP, &css);
            assert!(!tree.unsupported, "layout stays diagnostic-admitted: {css}");
            assert!(!tree.paint_structure_valid(4096), "painter would accept: {css}");
        }
        let plain = tree_for(MARKUP, &BASE.replace("float:left;", "").replace("clear:both;", ""));
        assert!(plain.paint_structure_valid(4096), "no-float default path stays paintable");
    }

    /// Review blocker (4:51): two individually valid kernels in one formatting context.
    /// Chrome: second float at x100 beside the escaping first, parent2 h45.
    /// Engine before the repair: second float x0 y10. Now refused globally.
    #[test]
    fn two_kernels_in_one_tree_are_refused() {
        let css = ".p1{width:400px;height:10px}.p2{width:400px}.f1{float:left;width:100px;height:50px}.c1{clear:both;height:10px}.f2{float:left;width:100px;height:5px}.c2{clear:both;height:5px}";
        let two = "<div class=p1><div class=f1></div><div class=c1></div></div><div class=p2><div class=f2></div><div class=c2></div></div>";
        let tree = tree_for(two, css);
        let v = VerticalPass::from_boxes(&tree, 785, 600);
        assert!(tree.unsupported || v.unsupported);
        assert!(!tree.paint_structure_valid_with(4096, true));
        // each alone is still a kernel
        let one = "<div class=p1><div class=f1></div><div class=c1></div></div>";
        assert!(!tree_for(one, css).unsupported);
    }

    #[test]
    fn explicit_parent_height_moves_next_block_to_it() {
        let tree = tree_for(MARKUP, &format!("{BASE}.p{{height:100px}}"));
        let v = VerticalPass::from_boxes(&tree, 400, 600);
        assert!(!v.unsupported);
        assert_eq!(v.content_y[tree.boxes.len() - 1], Some(100));
    }

    #[test]
    fn no_float_declaration_keeps_normal_flow_identity() {
        let tree = tree_for(MARKUP, &BASE.replace("float:left;", ""));
        // clear:both without a float is refused (not a kernel), never silently flowed.
        assert!(tree.unsupported);
        let plain = tree_for(MARKUP, &BASE.replace("float:left;", "").replace("clear:both;", ""));
        let v = VerticalPass::from_boxes(&plain, 400, 600);
        assert!(!plain.unsupported && !v.unsupported);
        // Plain stack: 50 + 10 = 60, the same numbers the float kernel gives.
        assert_eq!(v.content_y[plain.boxes.len() - 1], Some(60));
    }

    #[test]
    fn everything_outside_the_kernel_is_refused() {
        let refused = [
            (MARKUP, format!("{BASE}.f{{float:inline-start}}")),
            (MARKUP, format!("{BASE}.f{{margin:5px}}")),
            (MARKUP, format!("{BASE}.f{{width:25.001%}}")),
(MARKUP, format!("{BASE}.f{{width:101%}}")),
(MARKUP, format!("{BASE}.f{{width:0%}}")),
            (MARKUP, format!("{BASE}.f{{width:500px}}")),
            (MARKUP, format!("{BASE}.f{{padding:1px}}")),
            (MARKUP, format!("{BASE}.f{{border:1px solid #000}}")),
            (MARKUP, format!("{BASE}.f{{position:absolute}}")),
                        (MARKUP, format!("{BASE}.c{{width:50px}}")),
            (MARKUP, format!("{BASE}.p{{width:auto}}")),
            ("<div class=p><div class=f></div></div><div class=n></div>", BASE.to_string()),
            ("<div class=p><div class=c></div></div><div class=n></div>", BASE.to_string()),
            ("<div class=p><div class=f></div><div class=f></div><div class=c></div></div>", BASE.to_string()),
            ("<div class=p><div class=f></div><div class=c>x</div></div>", BASE.to_string()),
            ("<div class=p><div class=f>x</div><div class=c></div></div>", BASE.to_string()),
            ("<div class=p>t<div class=f></div><div class=c></div></div>", BASE.to_string()),
            ("<div class=p><div class=c></div><div class=f></div></div>", BASE.to_string()),
        ];
        for (markup, css) in refused {
            let tree = tree_for(markup, &css);
            let v = VerticalPass::from_boxes(&tree, 400, 600);
            assert!(tree.unsupported || v.unsupported, "admitted: {markup} / {css}");
        }
    }

    #[test]
    fn cleared_block_y_equals_float_height_but_is_degenerate_with_plain_stacking() {
        // HONEST NOTE: in this variant A scope (empty float, nonzero cleared block) the
        // numbers are identical to plain stacking, so this does NOT prove the float
        // mechanism; disabling both kernel branches leaves it passing (mutation 4:31).
        // The branches are exercised by the variant B tests, not by this one.
        // It pins that the cleared y follows the float height (50, then 80).
        let tree = tree_for(MARKUP, BASE);
        let p = (0..tree.boxes.len()).find(|&i| tree.float_kernel(i).is_some()).unwrap();
        let (f, c) = tree.float_kernel(p).unwrap();
        let v = VerticalPass::from_boxes(&tree, 400, 600);
        assert_eq!(v.content_y[c].unwrap() - v.content_y[f].unwrap(), 50);
        let wide = tree_for(MARKUP, &format!("{BASE}.f{{height:80px}}"));
        let w = VerticalPass::from_boxes(&wide, 400, 600);
        let pw = (0..wide.boxes.len()).find(|&i| wide.float_kernel(i).is_some()).unwrap();
        let (_, cw) = wide.float_kernel(pw).unwrap();
        assert_eq!(w.content_y[cw], Some(80));
    }

    const BASE_B: &str = ".p{width:400px}.f{float:left;width:100px;height:50px}.n{height:7px}.m{width:400px;height:20px}";
    const MARKUP_B: &str = "<div class=p><div class=f></div><div class=n></div></div><div class=m></div>";

    /// Variant C: float:right in the kernel. Chrome 156: float x = parent right edge -
    /// float width (parent x 8 + 400 - 100 = 308 at body margin 8). Without the
    /// right-float branch the float would stay at the parent's left edge.
    #[test]
    fn variant_c_right_float_x_is_parent_right_edge() {
        let left = tree_for(MARKUP, BASE);
        let right = tree_for(MARKUP, &BASE.replace("float:left", "float:right"));
        for (tree, want_delta) in [(&left, 0i64), (&right, 300)] {
            assert!(!tree.unsupported);
            let w = WidthPass::from_boxes(tree, 785);
            let p = (0..tree.boxes.len()).find(|&i| tree.float_kernel(i).is_some()).unwrap();
            let (f, _) = tree.float_kernel(p).unwrap();
            let (px, fx) = (w.content_x[p].unwrap(), w.content_x[f].unwrap());
            assert_eq!(fx - px, want_delta);
        }
        // Right floats never leave the kernel: a second right float is refused.
        let two = tree_for(
            "<div class=p><div class=f></div><div class=f></div><div class=c></div></div>",
            &BASE.replace("float:left", "float:right"),
        );
        assert!(!two.float_clear_confined() || two.float_kernel((0..two.boxes.len()).find(|&i| two.boxes[i].children.len() == 3).unwrap_or(0)).is_none());
    }

    /// Variant E1: the clearfix SHAPE with an explicit element (not a pseudo-element):
    /// float + empty zero-height `clear:both` block. Chrome 156 (floatkernel-e/ctx
    /// g1-g7, whole-image 0 diffs): parent height = float height, cleared y = 50, next
    /// block y = 50. DISTINCT from the nonzero cleared kernel: the zero used height
    /// needs the used-height rule and the vertical empty-block exemption below.
    #[test]
    fn variant_e1_zero_height_clear_layout_and_exemptions() {
        for css in [
            ".p{width:400px}.f{float:left;width:100px;height:50px}.c{clear:both}.m{width:400px;height:20px}",
            ".p{width:400px}.f{float:left;width:100px;height:50px}.c{clear:both;height:0}.m{width:400px;height:20px}",
        ] {
            let tree = tree_for(MARKUP_E, css);
            assert!(!tree.unsupported);
            let h = UsedHeightPass::from_boxes(&tree, 785, 600);
            let v = VerticalPass::from_boxes(&tree, 785, 600);
            assert!(!h.unsupported && !v.unsupported, "{css}");
            let p = (0..tree.boxes.len()).find(|&i| tree.float_kernel(i).is_some()).unwrap();
            let (f, c) = tree.float_kernel(p).unwrap();
            assert_eq!((h.heights[p], h.heights[c]), (Some(50), Some(0)));
            assert_eq!((v.content_y[f], v.content_y[c]), (Some(0), Some(50)));
            assert_eq!(v.content_y[tree.boxes.len() - 1], Some(50));
        }
    }

    /// E1 refusals: an uncleared zero-height second block, a cleared block with padding,
    /// margin or content, two clears, and a percentage height stay outside the kernel.
    #[test]
    fn variant_e1_zero_height_negatives_stay_refused() {
        let base = ".p{width:400px}.f{float:left;width:100px;height:50px}.c{clear:both}";
        let kernel = |markup: &str, css: &str| {
            let tree = tree_for(markup, css);
            (0..tree.boxes.len()).any(|i| tree.float_kernel(i).is_some())
        };
        assert!(kernel(MARKUP_E, base));
        for css in [
            format!("{base}.c{{clear:none}}"),
            format!("{base}.c{{padding-top:1px}}"),
            format!("{base}.c{{margin-top:4px}}"),
            format!("{base}.c{{height:50%}}"),
            format!("{base}.c{{width:50px}}"),
        ] {
            assert!(!kernel(MARKUP_E, &css), "{css}");
        }
        assert!(!kernel("<div class=p><div class=f></div><div class=c>x</div></div><div class=m></div>", base));
        assert!(!kernel("<div class=p><div class=f></div><div class=c></div><div class=c></div></div><div class=m></div>", base));
    }

    /// E1 leak check (review 6:12): the zero-height exemptions apply ONLY to a kernel's
    /// cleared child. An ordinary empty zero-height block, even in the same tree as a
    /// kernel or with no kernel at all, stays unsupported.
    #[test]
    fn variant_e1_exemptions_do_not_leak_outside_the_kernel() {
        let css = ".p{width:400px}.f{float:left;width:100px;height:50px}.c{clear:both}.m{width:400px;height:20px}.z{height:0}";
        for markup in [
            "<div class=p><div class=f></div><div class=c></div></div><div class=m></div><div class=z></div>",
            "<div class=m></div><div class=z></div>",
            "<div class=z></div><div class=m></div>",
        ] {
            let tree = tree_for(markup, css);
            let h = UsedHeightPass::from_boxes(&tree, 785, 600);
            let v = VerticalPass::from_boxes(&tree, 785, 600);
            assert!(h.unsupported || v.unsupported, "{markup}");
        }
        // The same z block is not a kernel cleared child.
        let tree = tree_for("<div class=m></div><div class=z></div>", css);
        assert!((0..tree.boxes.len()).all(|i| !tree.is_kernel_cleared(i)));
    }


    const MARKUP_E: &str = "<div class=p><div class=f></div><div class=c></div></div><div class=m></div>";

    /// Variant D: only a percentage that is a whole px count of the px parent forms a
    /// kernel (checked at the kernel gate itself, independent of later painter refusals).
    #[test]
    fn variant_d_kernel_gate_requires_whole_px_percent() {
        let kernel = |w: &str| {
            let tree = tree_for(MARKUP, &BASE.replace("width:100px", &format!("width:{w}")));
            (0..tree.boxes.len()).any(|i| tree.float_kernel(i).is_some())
        };
        for ok in ["25%", "50%", "100%", "33%"] {
            assert!(kernel(ok), "{ok}");
        }
        for bad in ["25.001%", "66.6667%", "12.34%", "0%", "101%", "auto"] {
            assert!(!kernel(bad), "{bad}");
        }
        // Review 6:00: a 0px parent makes every percentage resolve to 0px (not > 0, as
        // for the px arm). Refused, with the px arm unchanged (0px float also refused).
        for w in ["50%", "100%", "0px"] {
            let css = BASE.replace(".p{width:400px}", ".p{width:0px}").replace("width:100px", &format!("width:{w}"));
            let tree = tree_for(MARKUP, &css);
            assert!(!(0..tree.boxes.len()).any(|i| tree.float_kernel(i).is_some()), "0px parent {w}");
        }
    }


    /// Isolates the D kernel predicate itself (direct `float_kernel`, no WidthPass in the
    /// way): 250px parent * 65.2% is rational-whole (163) but a binary32 hole (Chrome
    /// 162.984375) and must be false; 200px parent * 50% (100) must be true.
    #[test]
    fn variant_d_kernel_predicate_isolated_float32_hole() {
        let kernel = |parent: u32, w: &str| {
            let css = BASE.replace(".p{width:400px}", &format!(".p{{width:{parent}px}}")).replace("width:100px", &format!("width:{w}"));
            let tree = tree_for(MARKUP, &css);
            (0..tree.boxes.len()).any(|i| tree.float_kernel(i).is_some())
        };
        assert!(!kernel(250, "65.2%"), "rational-whole float32 hole");
        assert!(kernel(200, "50%"));
        assert!(kernel(400, "25%"));
    }

    /// Variant B (no clear). Chrome 156 (floatkernel-b/ctx, whole-image compared):
    /// the normal block starts at the parent top, the parent is only 7 tall (the 50px
    /// float escapes) and the next block starts at y7. Plain stacking would give y50
    /// and parent height 57, so this DOES distinguish the float layout branches.
    #[test]
    fn variant_b_uncleared_float_layout_differs_from_stacking() {
        let tree = tree_for(MARKUP_B, BASE_B);
        assert!(!tree.unsupported);
        let h = UsedHeightPass::from_boxes(&tree, 785, 600);
        let v = VerticalPass::from_boxes(&tree, 785, 600);
        assert!(!h.unsupported && !v.unsupported);
        let p = (0..tree.boxes.len()).find(|&i| tree.float_kernel(i).is_some()).unwrap();
        let (f, n) = tree.float_kernel(p).unwrap();
        assert_eq!(h.heights[p], Some(7));
        assert_eq!((v.content_y[f], v.content_y[n]), (Some(0), Some(0)));
        assert_eq!(v.content_y[tree.boxes.len() - 1], Some(7));
    }

    #[test]
    fn variant_b_negative_shapes_are_refused() {
        for (css, markup) in [
            (format!("{BASE_B}.n{{width:200px}}"), MARKUP_B),
            (format!("{BASE_B}.n{{height:0}}"), MARKUP_B),
            (format!("{BASE_B}.f{{margin:5px}}"), MARKUP_B),
            (BASE_B.to_string(), "<div class=p><div class=f></div><div class=n></div><div class=n></div></div>"),
            (BASE_B.to_string(), "<div class=p><div class=n></div><div class=f></div></div><div class=m></div>"),
            (BASE_B.to_string(), "<div class=p><div class=f></div><div class=n>x</div></div><div class=m></div>"),
        ] {
            let tree = tree_for(markup, &css);
            let v = VerticalPass::from_boxes(&tree, 785, 600);
            assert!(tree.unsupported || v.unsupported, "admitted: {markup} / {css}");
        }
    }
}

#[cfg(test)]
mod percent_guard_tests {
    use super::*;

    /// Test-only binary32 model of Chrome's operation (the device never runs this):
    /// LayoutUnit(float(basis * float(h/100) / 100.0f)), truncated to 1/64.
    fn chrome_raw64(basis: u32, h: u32) -> i64 {
        let p = (f64::from(h) / 100.0) as f32;
        let q = (basis as f32 * p) / 100.0_f32;
        (q * 64.0) as i64
    }

    /// Proof check over the admitted domain: every admitted case equals the exact
    /// integer in the binary32 model. Exhaustive over basis 0..=4096 and h = 0..=100_000 in steps of 25
    /// (not h up to 1_000_000); non-multiples of 25 are checked only for h 1..=2000 at basis 400.
    #[test]
    fn admitted_percent_is_exactly_the_binary32_integer() {
        let mut admitted = 0_u64;
        for basis in 0..=4096_u32 {
            for h in (0..=100_000_u32).step_by(25) {
                if let Some(px) = percent_exact_px(basis, h) {
                    admitted += 1;
                    assert_eq!(chrome_raw64(basis, h), i64::from(px) * 64, "{basis} {h}");
                    assert_eq!(u64::from(px) * 10_000, u64::from(basis) * u64::from(h));
                }
            }
        }
        assert!(admitted > 1000);
        for h in (1..=2000_u32).filter(|h| h % 25 != 0) {
            assert_eq!(percent_exact_px(400, h), None, "h={h} not a multiple of 25");
        }
    }

    #[test]
    fn known_refusals_and_admissions() {
        // Review/measured holes and fractions refuse.
        assert_eq!(percent_exact_px(250, 6520), None); // 65.2%: Chrome 162.984375
        assert_eq!(percent_exact_px(375, 1040), None); // 10.4%: 38.984375
        assert_eq!(percent_exact_px(201, 5000), None); // 100.5
        assert_eq!(percent_exact_px(400, 66_67), None); // not a multiple of 25 hundredths
        assert_eq!(percent_exact_px(641, 50), None); // 3.205
        // Whole results in the proven subset admit.
        assert_eq!(percent_exact_px(200, 5000), Some(100));
        assert_eq!(percent_exact_px(400, 2500), Some(100));
        assert_eq!(percent_exact_px(400, 10_000), Some(400));
        assert_eq!(percent_exact_px(800, 1250), Some(100));
        assert_eq!(percent_exact_px(200, 50), Some(1));
        // Size bound.
        assert_eq!(percent_exact_px((1 << 24) + 1, 10_000), None);
    }

    /// Shared pass behaviour: a percent width with a fractional result is unsupported.
    #[test]
    fn width_pass_refuses_fractional_percent_and_keeps_exact() {
        let build = |w: &str| {
            let html = format!("<!doctype html><style>body{{margin:0}}.c{{width:201px}}.t{{width:{w};height:10px}}</style><div class=c><div class=t></div></div>");
            let s = crate::parse_style_tree(html.as_bytes(), &[], &crate::Limits::DEFAULT);
            WidthPass::from_boxes(&BoxTree::from_style(&s), 785).unsupported
        };
        assert!(build("50%"), "100.5 refuses");
        assert!(!build("100%"));
        assert!(!build("20px"));
    }

    /// Isolates the HeightPass audit at the pass level (no consumer rational guard in
    /// the way): basis 250px, 65.2% height is rational-whole (163) but a binary32 hole
    /// (Chrome 162.984375); 12.5% of 250 is fractional; 50% of 250 = 125 is exact.
    #[test]
    fn height_pass_audit_isolated_hole_fraction_exact() {
        let un = |h: &str| {
            let html = format!("<!doctype html><style>html{{height:250px}}body{{margin:0;height:{h}}}</style><body></body>");
            let s = crate::parse_style_tree(html.as_bytes(), &[], &crate::Limits::DEFAULT);
            HeightPass::from_boxes(&BoxTree::from_style(&s), 600).unsupported
        };
        assert!(un("65.2%"), "rational-whole float32 hole");
        assert!(un("12.5%"), "fractional");
        assert!(!un("50%"), "exact 125");
    }
}
