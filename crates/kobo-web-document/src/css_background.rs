//! Conservative CSS background-color painting on already proven content boxes.
//!
//! This paints clipped backgrounds and restricted solid borders, not rounded corners. It rejects
//! unsupported layouts rather than returning a plausible partial page. Text,
//! images, default margins and general block/inline layout remain unpainted.

use crate::box_tree::{BoxTree, VerticalPass, WidthPass};
use crate::css_paint_bridge::{
    paint_decorated_rectangles, paint_decorated_rectangles_float_kernel, BridgeError, FillSpec,
};
use crate::display_list::{DisplayList, Rgb};

/// Convert retained background colors to explicit fills in box order.
///
/// # Errors
/// Fails if the whole box tree cannot be proven to have paintable geometry,
/// if a selected rectangle is invalid, or when raster limits are exceeded.
pub fn paint_backgrounds(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
) -> Result<DisplayList, BridgeError> {
    paint_backgrounds_inner(tree, viewport_width, viewport_height, false)
}

/// EXPERIMENTAL explicit entry: like [`paint_backgrounds`] but admits the bounded
/// float kernel (one float:left or float:right block with a px or whole-px-percent width, then one auto-width block
/// that is `clear:both` or not), painting the float above in-flow block
/// backgrounds. Backgrounds only; no text. Every other painter still refuses
/// float/clear. Not a general float implementation.
///
/// # Errors
/// As [`paint_backgrounds`].
pub fn paint_backgrounds_float_kernel(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
) -> Result<DisplayList, BridgeError> {
    paint_backgrounds_inner(tree, viewport_width, viewport_height, true)
}

fn paint_backgrounds_inner(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    float_kernel: bool,
) -> Result<DisplayList, BridgeError> {
    // A root with `display: none` generates no boxes, and it does not
    // propagate its background (CSS Backgrounds 3, special backgrounds), so
    // the canvas is simply unpainted.
    if tree.roots.is_empty() && tree.boxes.is_empty() {
        return Ok(DisplayList::default());
    }
    // Reject malformed or disconnected arenas before the geometry passes,
    // which intentionally assume indices from BoxTree::from_style.
    // List-item markers are visible content, even when the item is empty.
    // A rectangle-only painter must not silently omit them.
    if !tree.paint_structure_valid_layers(crate::display_list::MAX_COMMANDS, float_kernel, true)
        || tree
            .boxes
            .iter()
            .any(|node| node.kind == crate::box_tree::BoxKind::ListItem)
    {
        return Err(BridgeError::Unsupported);
    }
    // CSS canvas propagation: a root background covers the viewport; if
    // transparent, the first body background propagates instead. The bridge
    // still validates *all* box geometry before the canvas fill is returned.
    let root = *tree.roots.first().ok_or(BridgeError::Unsupported)?;
    let root_box = tree.boxes.get(root).ok_or(BridgeError::Unsupported)?;
    if tree.roots.len() != 1 || root_box.source.is_none() {
        return Err(BridgeError::Unsupported);
    }
    let body = root_box.children.first().copied();
    let body_color = body.and_then(|index| tree.boxes.get(index)?.style.used_background_color());
    let canvas = root_box.style.used_background_color().or(body_color);
    let propagated_body = root_box.style.used_background_color().is_none() && body_color.is_some();
    let widths = WidthPass::from_boxes(tree, viewport_width);
    let vertical = VerticalPass::from_boxes(tree, viewport_width, viewport_height);
    let fills: Vec<_> = tree
        .boxes
        .iter()
        .enumerate()
        .filter_map(|(box_index, node)| {
            if box_index == root || (propagated_body && body == Some(box_index)) {
                return None;
            }
            node.style.used_background_color().map(|color| FillSpec {
                box_index,
                color: Rgb(
                    u8::try_from((color >> 16) & 0xff).unwrap_or(0),
                    u8::try_from((color >> 8) & 0xff).unwrap_or(0),
                    u8::try_from(color & 0xff).unwrap_or(0),
                ),
            })
        })
        .collect();
    let paint = if float_kernel {
        paint_decorated_rectangles_float_kernel
    } else {
        paint_decorated_rectangles
    };
    let local = paint(
        tree,
        &widths,
        &vertical,
        viewport_width,
        viewport_height,
        &fills,
    )?;
    let Some(color) = canvas else {
        return Ok(local);
    };
    let mut list = DisplayList::default();
    list.fill(
        crate::display_list::Rect {
            x: 0,
            y: 0,
            width: viewport_width,
            height: viewport_height,
        },
        Rgb(
            u8::try_from((color >> 16) & 255).unwrap_or(0),
            u8::try_from((color >> 8) & 255).unwrap_or(0),
            u8::try_from(color & 255).unwrap_or(0),
        ),
        crate::display_list::Source {
            node: root_box.source,
            action: None,
        },
    )
    .map_err(|_| BridgeError::Allocation)?;
    list.append_list(local)
        .map_err(|_| BridgeError::TooManyCommands)?;
    Ok(list)
}
