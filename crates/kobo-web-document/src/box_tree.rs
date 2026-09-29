//! CSS 2.1 display box generation before used values and line layout.
//!
//! This is a separate, bounded path; the shipping reader remains the fallback.
//! It preserves DOM order and text. Anonymous block wrappers follow CSS 2.1
//! §9.2.1.1 for mixed block and inline children. It does not claim to layout
//! those boxes, and calls out block-in-inline splitting as unsupported.

use crate::computed_style::{BoxSizing, Computed, Direction, Display, Length, Margin};
use crate::inline_lines::place_direct_text;
use crate::style_tree::StyleTree;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoxKind {
    Block,
    Inline,
    InlineBlock,
    ListItem,
    Text,
    AnonymousBlock,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CssBox {
    pub kind: BoxKind,
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
        Margin::Auto => None,
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
            unsupported: tree.unsupported,
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
                BoxKind::Block | BoxKind::ListItem => {
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
/// collapsed sibling gaps, but only when edge margins are zero and the box
/// cannot collapse through itself. Inline content and margin-through remain
/// unsupported rather than assigned invented heights.
pub struct UsedHeightPass {
    pub heights: Vec<Option<u32>>,
    pub unsupported: bool,
    pub truncated: bool,
}

impl UsedHeightPass {
    #[must_use]
    pub fn from_boxes(tree: &BoxTree, viewport_width: u32, viewport_height: u32) -> Self {
        Self::calculate(tree, viewport_width, viewport_height, None, None)
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
        mut advance: impl FnMut(char, u32) -> Option<u32>,
        mut line_height: impl FnMut(u32) -> Option<u32>,
    ) -> Self {
        Self::calculate(
            tree,
            viewport_width,
            viewport_height,
            Some(&mut advance),
            Some(&mut line_height),
        )
    }

    #[allow(clippy::too_many_lines)] // Bounded bottom-up height walk and guarded direct-text case.
    fn calculate(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        mut advance: Option<&mut dyn FnMut(char, u32) -> Option<u32>>,
        mut line_height: Option<&mut dyn FnMut(u32) -> Option<u32>>,
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
                if advance.is_none()
                    || line_height.is_none()
                    || !node.parent.is_some_and(|parent| {
                        tree.boxes.get(parent).is_some_and(|owner| {
                            matches!(owner.kind, BoxKind::Block | BoxKind::ListItem)
                                && owner.style.height == Length::Auto
                                && owner.children.as_slice() == [index]
                        })
                    })
                {
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
            if node.children.len() == 1
                && tree
                    .boxes
                    .get(node.children[0])
                    .is_some_and(|child| child.kind == BoxKind::Text)
            {
                let measured = advance
                    .as_deref_mut()
                    .zip(line_height.as_deref_mut())
                    .and_then(|(advance, line_height)| {
                        place_direct_text(tree, index, width, advance, line_height).ok()
                    });
                result.heights[index] = measured.map(|lines| lines.content_height);
                if result.heights[index].is_none_or(|height| height == 0) {
                    result.unsupported = true;
                }
                continue;
            }
            let mut content = 0_i64;
            let mut previous: Option<usize> = None;
            let mut valid = true;
            for &child in &node.children {
                let Some(next) = tree.boxes.get(child) else {
                    valid = false;
                    break;
                };
                if !matches!(next.kind, BoxKind::Block | BoxKind::ListItem) {
                    valid = false;
                    break;
                }
                let top = resolve_vertical_margin(next.style.margin_top, width);
                if previous.is_none() && top != 0 {
                    valid = false; // parent/first-child collapse not implemented
                    break;
                }
                if let Some(prev) = previous {
                    content = content.saturating_add(collapse_adjoining_margins(&[
                        resolve_vertical_margin(tree.boxes[prev].style.margin_bottom, width),
                        top,
                    ]));
                }
                let Some(height) = result.heights[child] else {
                    valid = false;
                    break;
                };
                if height == 0 && next.style.padding_top == 0 && next.style.padding_bottom == 0 {
                    valid = false; // empty child's margins may collapse through
                    break;
                }
                content = content
                    .saturating_add(i64::from(height))
                    .saturating_add(i64::from(next.style.padding_top))
                    .saturating_add(i64::from(next.style.padding_bottom));
                previous = Some(child);
            }
            if let Some(last) = previous {
                if resolve_vertical_margin(tree.boxes[last].style.margin_bottom, width) != 0 {
                    valid = false; // last-child/parent collapse not implemented
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
/// subtree. The first child's top margin must be zero so parent/child margin
/// collapse is inert. Zero-height boxes, inline content, and additional roots
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
        Self::calculate(tree, viewport_width, &heights)
    }

    /// Diagnostic coordinates for direct-text blocks measured with one font
    /// provider. A separate full-page paint gate is still required: the
    /// background-only bridge intentionally refuses text nodes.
    #[must_use]
    pub fn from_boxes_with_direct_text(
        tree: &BoxTree,
        viewport_width: u32,
        viewport_height: u32,
        mut advance: impl FnMut(char, u32) -> Option<u32>,
        mut line_height: impl FnMut(u32) -> Option<u32>,
    ) -> Self {
        let heights = UsedHeightPass::from_boxes_with_direct_text(
            tree,
            viewport_width,
            viewport_height,
            &mut advance,
            &mut line_height,
        );
        Self::calculate(tree, viewport_width, &heights)
    }

    #[allow(clippy::too_many_lines)] // Validation and placement share one bounded tree walk.
    fn calculate(tree: &BoxTree, viewport_width: u32, heights: &UsedHeightPass) -> Self {
        let count = tree.boxes.len();
        let mut result = Self {
            content_y: vec![None; count],
            unsupported: tree.unsupported,
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
            if seen[index]
                || node.parent != parent
                || !(matches!(node.kind, BoxKind::Block | BoxKind::ListItem)
                    || (node.kind == BoxKind::Text
                        && parent.is_some_and(|owner| {
                            tree.boxes[owner].children.as_slice() == [index]
                                && tree.boxes[owner].style.height == Length::Auto
                                && heights.heights[owner].is_some()
                        })))
                || (node.kind != BoxKind::Text && heights.heights[index].is_none_or(|h| h == 0))
                || (node.kind != BoxKind::Text && widths.widths[index].is_none())
                || node.style.box_sizing != BoxSizing::ContentBox
            {
                result.unsupported = true;
                break;
            }
            seen[index] = true;
            if node.kind == BoxKind::Text {
                continue;
            }
            if let Some(&first) = node.children.first() {
                let Some(child) = tree.boxes.get(first) else {
                    result.unsupported = true;
                    break;
                };
                if resolve_vertical_margin(
                    child.style.margin_top,
                    widths.widths[index].map_or(0, |w| w.content),
                ) != 0
                {
                    result.unsupported = true;
                    break;
                }
            }
            for &child in node.children.iter().rev() {
                stack.push((child, Some(index)));
            }
        }
        if result.unsupported
            || seen.iter().any(|&visited| !visited)
            || resolve_vertical_margin(tree.boxes[root].style.margin_top, viewport_width) != 0
        {
            result.unsupported = true;
            return result;
        }
        result.content_y[root] = Some(i64::from(tree.boxes[root].style.padding_top));
        let mut stack = vec![root];
        while let Some(index) = stack.pop() {
            let node = &tree.boxes[index];
            if node.kind == BoxKind::Text {
                continue;
            }
            let containing_width = widths.widths[index].map_or(0, |w| w.content);
            let mut previous = None::<usize>;
            for &child in &node.children {
                if tree.boxes[child].kind == BoxKind::Text {
                    result.content_y[child] = result.content_y[index];
                    continue;
                }
                let gap = previous.map_or(0, |prev| {
                    collapse_adjoining_margins(&[
                        resolve_vertical_margin(
                            tree.boxes[prev].style.margin_bottom,
                            containing_width,
                        ),
                        resolve_vertical_margin(
                            tree.boxes[child].style.margin_top,
                            containing_width,
                        ),
                    ])
                });
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
    /// Compute block content widths top-down. Inline formatting does not
    /// supply a known x; an inline-block needs shrink-to-fit and cuts off
    /// its descendants' containing width as well.
    #[must_use]
    pub fn from_boxes(tree: &BoxTree, viewport_width: u32) -> Self {
        let mut result = Self {
            widths: vec![None; tree.boxes.len()],
            content_x: vec![None; tree.boxes.len()],
            unsupported: tree.unsupported,
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
            let (content, x) = match node.kind {
                BoxKind::Block | BoxKind::ListItem | BoxKind::AnonymousBlock => {
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
                    result.widths[index] = width;
                    let x = containing_x.and_then(|parent_x| {
                        width.and_then(|w| {
                            parent_x.checked_add(w.margin_left).and_then(|left| {
                                left.checked_add(i64::from(node.style.padding_left))
                            })
                        })
                    });
                    result.content_x[index] = x;
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

impl BoxTree {
    /// Convert a bounded styled DOM into boxes without painting or flattening
    /// mixed text. At most two boxes per styled node are allocated.
    #[must_use]
    pub fn from_style(styled: &StyleTree) -> Self {
        let mut tree = Self {
            truncated: styled.truncated,
            unsupported: styled.unsupported,
            quirks: styled.quirks,
            ..Self::default()
        };
        let limit = styled.nodes.len().saturating_mul(2);
        let mut stack: Vec<_> = styled
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.parent.is_none())
            .map(|(index, _)| (index, None))
            .rev()
            .collect();
        while let Some((source, parent)) = stack.pop() {
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
            tree.boxes.push(CssBox {
                kind,
                source: Some(source),
                parent,
                children: Vec::new(),
                style: node.style,
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
        // Iterate only the original boxes. Anonymous wrappers append later and
        // must not themselves be normalized again.
        let originals = tree.boxes.len();
        for parent in (0..originals).rev() {
            let kind = tree.boxes[parent].kind;
            let children = &tree.boxes[parent].children;
            let has_block = children
                .iter()
                .any(|&child| matches!(tree.boxes[child].kind, BoxKind::Block | BoxKind::ListItem));
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
                if matches!(tree.boxes[child].kind, BoxKind::Block | BoxKind::ListItem) {
                    tree.wrap_inline_run(parent, &mut run, limit);
                    tree.boxes[parent].children.push(child);
                } else {
                    run.push(child);
                }
            }
            tree.wrap_inline_run(parent, &mut run, limit);
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

    fn boxes(html: &str) -> BoxTree {
        BoxTree::from_style(&parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT))
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
            |_, size| Some(size / 2),
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
                |_, _| Some(8),
                |_| Some(20)
            )
            .unsupported
        );
        let unknown_font =
            UsedHeightPass::from_boxes_with_direct_text(&tree, 100, 100, |_, _| None, |_| Some(20));
        assert!(unknown_font.unsupported);
    }

    #[test]
    fn measured_direct_text_receives_content_origin_without_background_permission() {
        let tree = boxes("<html style='height:100px;padding-top:2px'><body style='padding-top:3px'><p style='font-size:20px;padding-top:4px'>ab cd</p></body></html>");
        let vertical = VerticalPass::from_boxes_with_direct_text(
            &tree,
            100,
            100,
            |_, size| Some(size / 2),
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
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
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
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
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
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
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
        assert!(VerticalPass::from_boxes(&tree, 100, 100).unsupported); // border-box y unsupported
    }

    #[test]
    fn vertical_pass_rejects_unknown_height_and_parent_child_collapse() {
        for html in [
            "<html style='height:600px'><body style='height:400px;margin-top:10px'></body></html>",
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
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
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
        let styled = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
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
