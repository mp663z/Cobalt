//! Conservative bridge from verified block content boxes to solid fills.
//!
//! This is not CSS background painting. `FillSpec` colors are explicit paint
//! inputs, not the computed `color` property (which is a foreground color).
//! Only resolved-height, normal-flow content rectangles are available here.
//! The caller must build the box tree from `BoxTree::from_style`, not construct
//! an unverified tree by hand: unknown declarations set its `unsupported` flag.

use crate::box_tree::{BoxKind, BoxTree, UsedHeightPass, VerticalPass, WidthPass};
use crate::computed_style::BoxSizing;
use crate::display_list::{
    DisplayList, Error as DisplayError, Rect, Rgb, Source, MAX_COMMANDS, MAX_PIXELS,
};

/// Upper bound before calculating passes, to limit work on hostile inputs.
pub const MAX_BRIDGE_BOXES: usize = MAX_COMMANDS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FillSpec {
    /// Box index, not a DOM node or glyph run.
    pub box_index: usize,
    /// Solid color supplied by an explicit paint decision upstream.
    pub color: Rgb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeError {
    Unsupported,
    TooManyBoxes,
    TooManyCommands,
    InvalidViewport,
    InvalidGeometry,
    Allocation,
}

fn validate_tree(tree: &BoxTree) -> Result<(), BridgeError> {
    let count = tree.boxes.len();
    // Check indices before recomputing either pass: WidthPass assumes a sound
    // tree and indexes children directly. VerticalPass checks shape later.
    if tree
        .boxes
        .iter()
        .map(|node| node.children.len())
        .sum::<usize>()
        > MAX_BRIDGE_BOXES
    {
        return Err(BridgeError::TooManyBoxes);
    }
    if tree.roots.len() != 1
        || tree.roots[0] >= count
        || tree.boxes.iter().enumerate().any(|(index, node)| {
            node.source.is_none()
                || node
                    .parent
                    .is_some_and(|parent| parent >= count || parent == index)
                || node
                    .children
                    .iter()
                    .any(|&child| child >= count || child == index)
        })
    {
        return Err(BridgeError::Unsupported);
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PaintArea {
    Content,
    Padding,
}

/// Paint proven padding-box backgrounds, including integer-pixel padding but
/// no border, radius, image, or general CSS background propagation. The
/// entire box tree must still pass the restricted vertical and width passes.
///
/// # Errors
/// Returns an error for any unsupported or invalid geometry, bounds, or allocation.
pub fn paint_padding_rectangles(
    tree: &BoxTree,
    widths: &WidthPass,
    vertical: &VerticalPass,
    viewport_width: u32,
    viewport_height: u32,
    fills: &[FillSpec],
) -> Result<DisplayList, BridgeError> {
    paint_rectangles(
        tree,
        widths,
        vertical,
        viewport_width,
        viewport_height,
        fills,
        PaintArea::Padding,
    )
}

/// Make a new, all-or-nothing list in the order of the requested fills.
/// No text, images, borders, padding, clearance, backgrounds, or guessed
/// coordinates are synthesized. Only *content* rectangles are filled.
///
/// The passes are checked against the supplied viewport and tree so a stale
/// or fabricated coordinate cannot enter the display list. An unsupported or
/// truncated pass rejects the whole request, including an empty fill list.
/// The selected boxes must be resolved-height blocks or list items; source
/// indices point into the retained styled arena, not the old reader model.
///
/// # Errors
/// Returns an error for unsupported tree/pass data, missing or unrepresentable
/// geometry, an invalid viewport, resource limits, or failed allocation.
pub fn paint_content_rectangles(
    tree: &BoxTree,
    widths: &WidthPass,
    vertical: &VerticalPass,
    viewport_width: u32,
    viewport_height: u32,
    fills: &[FillSpec],
) -> Result<DisplayList, BridgeError> {
    paint_rectangles(
        tree,
        widths,
        vertical,
        viewport_width,
        viewport_height,
        fills,
        PaintArea::Content,
    )
}

#[allow(clippy::too_many_lines)] // Proven geometry validation and rectangle construction share one path.
fn paint_rectangles(
    tree: &BoxTree,
    widths: &WidthPass,
    vertical: &VerticalPass,
    viewport_width: u32,
    viewport_height: u32,
    fills: &[FillSpec],
    area: PaintArea,
) -> Result<DisplayList, BridgeError> {
    let count = tree.boxes.len();
    if count > MAX_BRIDGE_BOXES {
        return Err(BridgeError::TooManyBoxes);
    }
    if fills.len() > MAX_COMMANDS {
        return Err(BridgeError::TooManyCommands);
    }
    let pixels = usize::try_from(viewport_width).ok().and_then(|w| {
        usize::try_from(viewport_height)
            .ok()
            .and_then(|h| w.checked_mul(h))
    });
    if viewport_width == 0 || viewport_height == 0 || pixels.is_none_or(|n| n > MAX_PIXELS) {
        return Err(BridgeError::InvalidViewport);
    }
    if tree.unsupported
        || tree.quirks
        || tree.truncated
        || widths.unsupported
        || widths.truncated
        || vertical.unsupported
        || vertical.truncated
    {
        return Err(BridgeError::Unsupported);
    }
    validate_tree(tree)?;
    // VerticalPass has a deliberately narrow domain. Do not turn its output
    // into paint if styles carry edges this bridge has not implemented.
    if tree.boxes.iter().any(|node| {
        !matches!(node.kind, BoxKind::Block | BoxKind::ListItem)
            || (area == PaintArea::Content
                && (node.style.box_sizing != BoxSizing::ContentBox
                    || node.style.padding_left != 0
                    || node.style.padding_right != 0
                    || node.style.padding_top != 0
                    || node.style.padding_bottom != 0))
    }) {
        return Err(BridgeError::Unsupported);
    }
    let proven_widths = WidthPass::from_boxes(tree, viewport_width);
    let proven_vertical = VerticalPass::from_boxes(tree, viewport_width, viewport_height);
    let proven_heights = UsedHeightPass::from_boxes(tree, viewport_width, viewport_height);
    if proven_widths.unsupported
        || proven_widths.truncated
        || proven_vertical.unsupported
        || proven_vertical.truncated
        || proven_heights.unsupported
        || proven_heights.truncated
        || widths.widths != proven_widths.widths
        || widths.content_x != proven_widths.content_x
        || vertical.content_y != proven_vertical.content_y
    {
        return Err(BridgeError::Unsupported);
    }
    let mut list = DisplayList::default();
    for fill in fills {
        let node = tree
            .boxes
            .get(fill.box_index)
            .ok_or(BridgeError::InvalidGeometry)?;
        let width = widths.widths[fill.box_index]
            .ok_or(BridgeError::InvalidGeometry)?
            .content;
        let height = proven_heights.heights[fill.box_index].ok_or(BridgeError::InvalidGeometry)?;
        let x = widths.content_x[fill.box_index].ok_or(BridgeError::InvalidGeometry)?;
        let y = vertical.content_y[fill.box_index].ok_or(BridgeError::InvalidGeometry)?;
        let (x, y, width, height) = if area == PaintArea::Padding {
            (
                x.checked_sub(i64::from(node.style.padding_left))
                    .ok_or(BridgeError::InvalidGeometry)?,
                y.checked_sub(i64::from(node.style.padding_top))
                    .ok_or(BridgeError::InvalidGeometry)?,
                width
                    .checked_add(node.style.padding_left)
                    .and_then(|w| w.checked_add(node.style.padding_right))
                    .ok_or(BridgeError::InvalidGeometry)?,
                height
                    .checked_add(node.style.padding_top)
                    .and_then(|h| h.checked_add(node.style.padding_bottom))
                    .ok_or(BridgeError::InvalidGeometry)?,
            )
        } else {
            (x, y, width, height)
        };
        if width == 0 || height == 0 {
            return Err(BridgeError::InvalidGeometry);
        }
        let rect = Rect {
            x: i32::try_from(x).map_err(|_| BridgeError::InvalidGeometry)?,
            y: i32::try_from(y).map_err(|_| BridgeError::InvalidGeometry)?,
            width,
            height,
        };
        list.fill(
            rect,
            fill.color,
            Source {
                node: node.source,
                action: None,
            },
        )
        .map_err(|error| match error {
            DisplayError::TooManyCommands => BridgeError::TooManyCommands,
            _ => BridgeError::Allocation,
        })?;
    }
    Ok(list)
}
