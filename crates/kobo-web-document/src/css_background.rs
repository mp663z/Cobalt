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
    // CSS backgrounds on the root paint the canvas, and the first body's
    // background propagates when the root is transparent. This narrow
    // padding-box painter implements neither rule. Refuse both rather than
    // returning a plausible rectangle with incorrect viewport coverage.
    let root = *tree.roots.first().ok_or(BridgeError::Unsupported)?;
    let root_box = tree.boxes.get(root).ok_or(BridgeError::Unsupported)?;
    if root_box.style.background_color.is_some()
        || root_box.children.first().is_some_and(|&body| {
            tree.boxes
                .get(body)
                .is_some_and(|node| node.style.background_color.is_some())
        })
    {
        return Err(BridgeError::Unsupported);
    }
    let widths = WidthPass::from_boxes(tree, viewport_width);
    let vertical = VerticalPass::from_boxes(tree, viewport_width, viewport_height);
    let fills: Vec<_> = tree
        .boxes
        .iter()
        .enumerate()
        .filter_map(|(box_index, node)| {
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
    paint_padding_rectangles(
        tree,
        &widths,
        &vertical,
        viewport_width,
        viewport_height,
        &fills,
    )
}
