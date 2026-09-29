//! Conservative CSS background-color painting on already proven content boxes.
//!
//! This paints padding rectangles, not borders or rounded corners. It rejects
//! unsupported layouts rather than returning a plausible partial page. Text,
//! images, default margins and general block/inline layout remain unpainted.

use crate::box_tree::{BoxTree, VerticalPass, WidthPass};
use crate::css_paint_bridge::{paint_padding_rectangles, BridgeError, FillSpec};
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
    // CSS canvas propagation: a root background covers the viewport; if
    // transparent, the first body background propagates instead. The bridge
    // still validates *all* box geometry before the canvas fill is returned.
    let root = *tree.roots.first().ok_or(BridgeError::Unsupported)?;
    let root_box = tree.boxes.get(root).ok_or(BridgeError::Unsupported)?;
    if tree.roots.len() != 1 || root_box.source.is_none() {
        return Err(BridgeError::Unsupported);
    }
    let body = root_box.children.first().copied();
    let body_color = body.and_then(|index| tree.boxes.get(index)?.style.background_color);
    let canvas = root_box.style.background_color.or(body_color);
    let propagated_body = root_box.style.background_color.is_none() && body_color.is_some();
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
            node.style.background_color.map(|color| FillSpec {
                box_index,
                color: Rgb(
                    u8::try_from((color >> 16) & 0xff).unwrap_or(0),
                    u8::try_from((color >> 8) & 0xff).unwrap_or(0),
                    u8::try_from(color & 0xff).unwrap_or(0),
                ),
            })
        })
        .collect();
    let local = paint_padding_rectangles(
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
