//! Conservative bridge from verified block content boxes to solid fills.
//!
//! This is not CSS background painting. `FillSpec` colors are explicit paint
//! inputs, not the computed `color` property (which is a foreground color).
//! Only resolved-height, normal-flow content rectangles are available here.
//! The caller must build the box tree from `BoxTree::from_style`, not construct
//! an unverified tree by hand: unknown declarations set its `unsupported` flag.

// The bridge emits CSS-px rectangles that the rasterizer treats as device px.
const _: () = assert!(crate::DEVICE_PX_PER_CSS_PX == 1, "bridge geometry assumes 1 CSS px = 1 device px");

use crate::box_tree::{BoxKind, BoxTree, UsedHeightPass, VerticalPass, WidthPass};
use crate::computed_style::{BoxSizing, Clip};
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

fn validate_tree(
    tree: &BoxTree,
    mode: crate::box_tree::PaintMode,
    allow_relative: bool,
) -> Result<(), BridgeError> {
    if tree.paint_structure_valid_layers_mode(MAX_BRIDGE_BOXES, mode, allow_relative) {
        Ok(())
    } else {
        Err(BridgeError::Unsupported)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PaintArea {
    Content,
    Padding,
    Decorated,
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
        crate::box_tree::PaintMode::Default,
    )
}

// Background painter owns preorder and border synthesis; explicit public
// rectangle primitives retain their fill order and do not synthesize borders.
pub(crate) fn paint_decorated_rectangles(
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
        PaintArea::Decorated,
        crate::box_tree::PaintMode::Default,
    )
}

/// Explicit float-kernel entry for the background painter only: same decorated
/// painter, but a confined kernel float is painted after in-flow block backgrounds.
pub(crate) fn paint_decorated_rectangles_float_kernel(
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
        PaintArea::Decorated,
        crate::box_tree::PaintMode::FloatKernel,
    )
}

/// Explicit generated-block entry for the background painter only: same
/// decorated painter, validating generated ordinary blocks under the
/// generated-block paint mode (symmetric `::before`/`::after`; tree-global
/// float/clear refusal).
pub(crate) fn paint_decorated_rectangles_generated_block(
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
        PaintArea::Decorated,
        crate::box_tree::PaintMode::GeneratedBlock,
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
        crate::box_tree::PaintMode::Default,
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
    mode: crate::box_tree::PaintMode,
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
    if tree.boxes.iter().any(|node| {
        if area == PaintArea::Decorated {
            !crate::css_border::supported(node.style, tree.ext)
        } else {
            node.style.decor.has_painted_border()
                || node.style.positioned.position != crate::computed_style::Position::Static
                || node.style.box_shadow.is_some()
                || node.style.paints_outline()
        }
    }) {
        // Rectangle-only primitives do not synthesize visible borders.
        return Err(BridgeError::Unsupported);
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
    validate_tree(tree, mode, area == PaintArea::Decorated)?;
    // VerticalPass has a deliberately narrow domain. Do not turn its output
    // into paint if styles carry edges this bridge has not implemented.
    if tree.boxes.iter().any(|node| {
        !matches!(node.kind, BoxKind::Block | BoxKind::ListItem | BoxKind::Flex)
            || (area == PaintArea::Content
                && (node.style.box_sizing != BoxSizing::ContentBox
                    || node.style.padding_left != 0
                    || node.style.padding_right != 0
                    || node.style.padding_top != 0
                    || node.style.padding_bottom != 0))
    }) {
        return Err(BridgeError::Unsupported);
    }
    // FL26: a hard box-shadow is drawn ONLY by the decorated painter, never by the float kernel, and never for the root, the
    // body (canvas propagation), a zero-sized border box, or a box below an `overflow:hidden` ancestor (no clip is modelled).
    if tree.boxes.iter().enumerate().any(|(i, node)| {
        (node.style.box_shadow.is_some() || node.style.paints_outline())
            && (area != PaintArea::Decorated
                // FL28: a flex item paints atomically (its outline is drawn in the item's own turn, so a later item's
                // background covers it: measured in Chrome 156, sh28/p5); outlines below a flex container refuse.
                || (node.style.paints_outline() && {
                    let mut up = node.parent;
                    let mut flex = false;
                    while let Some(a) = up {
                        flex |= tree.boxes[a].kind == BoxKind::Flex;
                        up = tree.boxes[a].parent;
                    }
                    flex
                })
                || mode == crate::box_tree::PaintMode::FloatKernel
                || tree.roots.first() == Some(&i)
                || tree.roots.first().and_then(|&r| tree.boxes[r].children.first()) == Some(&i)
                || {
                    let mut up = node.parent;
                    let mut hidden = false;
                    while let Some(a) = up {
                        hidden |= tree.boxes[a].style.overflow_hidden;
                        up = tree.boxes[a].parent;
                    }
                    hidden
                })
    }) {
        return Err(BridgeError::Unsupported);
    }
    // FL28: a visible outline needs an explicit colour (currentcolor is not resolved here) and a tree with no positioned box
    // (outline paint phase order against positioned layers is not modelled).
    if tree.boxes.iter().any(|n| n.style.paints_outline() && n.style.outline_color.is_none())
        || (tree.boxes.iter().any(|n| n.style.paints_outline())
            && tree.boxes.iter().any(|n| n.style.positioned.position != crate::computed_style::Position::Static))
    {
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
    let mut indexed = Vec::new();
    indexed
        .try_reserve_exact(count)
        .map_err(|_| BridgeError::Allocation)?;
    indexed.resize(count, None);
    for fill in fills {
        let slot = indexed
            .get_mut(fill.box_index)
            .ok_or(BridgeError::InvalidGeometry)?;
        if area == PaintArea::Decorated && slot.is_some() {
            return Err(BridgeError::InvalidGeometry);
        }
        *slot = Some(fill);
    }
    // CSS 2.2 App. E: in-flow block backgrounds, then floats, then positioned. A positioned box paints as one
    // layer with its non-positioned descendants (pseudo stacking context, z-index auto): the box, then those
    // descendants, in tree order. Nested positioned boxes take their own turn in tree order.
    let positioned = |i: usize| {
        matches!(
            tree.boxes[i].style.positioned.position,
            crate::computed_style::Position::Absolute | crate::computed_style::Position::Relative
        )
    };
    let mut owner: Vec<Option<usize>> = vec![None; count];
    for i in 0..count {
        owner[i] = tree.boxes[i].parent.and_then(|p| {
            if positioned(p) {
                Some(p)
            } else {
                owner.get(p).copied().flatten()
            }
        });
    }
    let mut order: Vec<usize> = (0..count)
        .filter(|&i| {
            !positioned(i)
                && owner[i].is_none()
                && !(mode == crate::box_tree::PaintMode::FloatKernel && tree.is_kernel_float(i))
        })
        .chain((0..count).filter(|&i| {
            mode == crate::box_tree::PaintMode::FloatKernel
                && tree.is_kernel_float(i)
                && owner[i].is_none()
        }))
        .collect();
    for p in (0..count).filter(|&i| positioned(i)) {
        order.push(p);
        order.extend((0..count).filter(|&i| !positioned(i) && owner[i] == Some(p)));
    }
    if order.len() != count {
        return Err(BridgeError::InvalidGeometry);
    }
    let iterations = if area == PaintArea::Decorated {
        count
    } else {
        fills.len()
    };
    // Decorated mode visits every box, including those without a fill.
    #[allow(clippy::needless_range_loop)]
    for iteration in 0..iterations {
        let index = if area == PaintArea::Decorated {
            order[iteration]
        } else {
            fills[iteration].box_index
        };
        let fill = if area == PaintArea::Decorated {
            indexed[index]
        } else {
            Some(&fills[iteration])
        };
        let node = tree.boxes.get(index).ok_or(BridgeError::InvalidGeometry)?;
        let width = widths.widths[index]
            .ok_or(BridgeError::InvalidGeometry)?
            .content;
        let height = proven_heights.heights[index].ok_or(BridgeError::InvalidGeometry)?;
        let x = widths.content_x[index].ok_or(BridgeError::InvalidGeometry)?;
        let y = vertical.content_y[index].ok_or(BridgeError::InvalidGeometry)?;
        let content = Rect {
            x: i32::try_from(x).map_err(|_| BridgeError::InvalidGeometry)?,
            y: i32::try_from(y).map_err(|_| BridgeError::InvalidGeometry)?,
            width,
            height,
        };
        let (x, y, width, height) = if area == PaintArea::Content {
            (x, y, width, height)
        } else {
            // `padding_*` is padding plus border; the background colour is
            // bounded by the clip of the bottom-most background layer.
            let border = node.style.decor.used_border();
            let (top, right, bottom, left) = match node.style.decor.colour_clip() {
                Clip::Border => (0, 0, 0, 0),
                Clip::Padding => (border[0], border[1], border[2], border[3]),
                Clip::Content => (
                    node.style.padding_top,
                    node.style.padding_right,
                    node.style.padding_bottom,
                    node.style.padding_left,
                ),
            };
            let grow = |edge: u32, inset: u32| edge.saturating_sub(inset);
            let (top, right, bottom, left) = (
                grow(node.style.padding_top, top),
                grow(node.style.padding_right, right),
                grow(node.style.padding_bottom, bottom),
                grow(node.style.padding_left, left),
            );
            (
                x.checked_sub(i64::from(left))
                    .ok_or(BridgeError::InvalidGeometry)?,
                y.checked_sub(i64::from(top))
                    .ok_or(BridgeError::InvalidGeometry)?,
                width
                    .checked_add(left)
                    .and_then(|w| w.checked_add(right))
                    .ok_or(BridgeError::InvalidGeometry)?,
                height
                    .checked_add(top)
                    .and_then(|h| h.checked_add(bottom))
                    .ok_or(BridgeError::InvalidGeometry)?,
            )
        };
        if let (Some(shadow), true) = (node.style.box_shadow, area == PaintArea::Decorated) {
            // Border box = content grown by `padding_*` (padding plus border). Only the part of the shadow shape outside it
            // is painted, below this box's own background and border, in this box's turn of the paint order.
            let bx0 = content.x.checked_sub(i32::try_from(node.style.padding_left).map_err(|_| BridgeError::InvalidGeometry)?);
            let by0 = content.y.checked_sub(i32::try_from(node.style.padding_top).map_err(|_| BridgeError::InvalidGeometry)?);
            let bw = i64::from(content.width) + i64::from(node.style.padding_left) + i64::from(node.style.padding_right);
            let bh = i64::from(content.height) + i64::from(node.style.padding_top) + i64::from(node.style.padding_bottom);
            let (bx0, by0) = (i64::from(bx0.ok_or(BridgeError::InvalidGeometry)?), i64::from(by0.ok_or(BridgeError::InvalidGeometry)?));
            if bw == 0 || bh == 0 {
                return Err(BridgeError::Unsupported);
            }
            let (bx1, by1) = (bx0 + bw, by0 + bh);
            let spread = i64::from(shadow.spread);
            let (sx0, sy0) = (bx0 - spread + i64::from(shadow.dx), by0 - spread + i64::from(shadow.dy));
            let (sx1, sy1) = (bx1 + spread + i64::from(shadow.dx), by1 + spread + i64::from(shadow.dy));
            let (ix0, ix1, iy0, iy1) = (sx0.max(bx0), sx1.min(bx1), sy0.max(by0), sy1.min(by1));
            let parts: Vec<(i64, i64, i64, i64)> = if ix0 >= ix1 || iy0 >= iy1 {
                vec![(sx0, sy0, sx1, sy1)]
            } else {
                vec![(sx0, sy0, sx1, iy0), (sx0, iy1, sx1, sy1), (sx0, iy0, ix0, iy1), (ix1, iy0, sx1, iy1)]
            };
            let rgb = Rgb(
                u8::try_from((shadow.rgb >> 16) & 0xff).unwrap_or(0),
                u8::try_from((shadow.rgb >> 8) & 0xff).unwrap_or(0),
                u8::try_from(shadow.rgb & 0xff).unwrap_or(0),
            );
            for (x0, y0, x1, y1) in parts {
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                list.fill(
                    Rect {
                        x: i32::try_from(x0).map_err(|_| BridgeError::InvalidGeometry)?,
                        y: i32::try_from(y0).map_err(|_| BridgeError::InvalidGeometry)?,
                        width: u32::try_from(x1 - x0).map_err(|_| BridgeError::InvalidGeometry)?,
                        height: u32::try_from(y1 - y0).map_err(|_| BridgeError::InvalidGeometry)?,
                    },
                    rgb,
                    Source { node: node.source, action: None },
                )
                .map_err(|error| match error {
                    DisplayError::TooManyCommands => BridgeError::TooManyCommands,
                    _ => BridgeError::Allocation,
                })?;
            }
        }
        if let Some(fill) = fill {
            if width == 0 || height == 0 {
                if area == PaintArea::Content {
                    return Err(BridgeError::InvalidGeometry);
                }
            } else {
                list.fill(
                    Rect {
                        x: i32::try_from(x).map_err(|_| BridgeError::InvalidGeometry)?,
                        y: i32::try_from(y).map_err(|_| BridgeError::InvalidGeometry)?,
                        width,
                        height,
                    },
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
        }
        if area == PaintArea::Decorated {
            crate::css_border::paint(
                &mut list,
                content,
                node.style,
                Source {
                    node: node.source,
                    action: None,
                },
                tree.ext,
            )
            .map_err(|error| match error {
                DisplayError::TooManyCommands => BridgeError::TooManyCommands,
                DisplayError::Allocation => BridgeError::Allocation,
                _ => BridgeError::InvalidGeometry,
            })?;
        }
    }
    // FL28: outlines are drawn in a phase AFTER every in-flow background and border (Chrome: a later sibling's background does
    // not cover an earlier box's ring), in tree order. The ring lies outside the border box, `offset` away, `width` thick.
    if area == PaintArea::Decorated {
        for index in 0..count {
            let node = &tree.boxes[index];
            if !node.style.paints_outline() {
                continue;
            }
            let width = widths.widths[index].ok_or(BridgeError::InvalidGeometry)?.content;
            let height = proven_heights.heights[index].ok_or(BridgeError::InvalidGeometry)?;
            let x = widths.content_x[index].ok_or(BridgeError::InvalidGeometry)?;
            let y = vertical.content_y[index].ok_or(BridgeError::InvalidGeometry)?;
            let bx0 = x - i64::from(node.style.padding_left);
            let by0 = y - i64::from(node.style.padding_top);
            let bw = i64::from(width) + i64::from(node.style.padding_left) + i64::from(node.style.padding_right);
            let bh = i64::from(height) + i64::from(node.style.padding_top) + i64::from(node.style.padding_bottom);
            if bw == 0 || bh == 0 {
                return Err(BridgeError::Unsupported);
            }
            let (t, o) = (i64::from(node.style.outline_width), i64::from(node.style.outline_offset));
            let (ox0, oy0, ow, oh) = (bx0 - o - t, by0 - o - t, bw + 2 * (o + t), bh + 2 * (o + t));
            let rgb = node.style.outline_color.ok_or(BridgeError::Unsupported)?;
            let colour = Rgb(
                u8::try_from((rgb >> 16) & 0xff).unwrap_or(0),
                u8::try_from((rgb >> 8) & 0xff).unwrap_or(0),
                u8::try_from(rgb & 0xff).unwrap_or(0),
            );
            for (rx, ry, rw, rh) in [(ox0, oy0, ow, t), (ox0, oy0 + oh - t, ow, t), (ox0, oy0 + t, t, oh - 2 * t), (ox0 + ow - t, oy0 + t, t, oh - 2 * t)] {
                if rw <= 0 || rh <= 0 {
                    continue;
                }
                list.fill(
                    Rect {
                        x: i32::try_from(rx).map_err(|_| BridgeError::InvalidGeometry)?,
                        y: i32::try_from(ry).map_err(|_| BridgeError::InvalidGeometry)?,
                        width: u32::try_from(rw).map_err(|_| BridgeError::InvalidGeometry)?,
                        height: u32::try_from(rh).map_err(|_| BridgeError::InvalidGeometry)?,
                    },
                    colour,
                    Source { node: node.source, action: None },
                )
                .map_err(|error| match error {
                    DisplayError::TooManyCommands => BridgeError::TooManyCommands,
                    _ => BridgeError::Allocation,
                })?;
            }
        }
    }
    Ok(list)
}
