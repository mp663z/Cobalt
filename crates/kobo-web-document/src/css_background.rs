//! Conservative CSS background-color painting on already proven content boxes.
//!
//! This paints content rectangles only, not borders or padding. It rejects
//! unsupported layouts rather than returning a plausible partial page. Text,
//! images, default margins and general block/inline layout remain unpainted.

use crate::box_tree::{BoxTree, VerticalPass, WidthPass};
use crate::css_paint_bridge::{paint_content_rectangles, BridgeError, FillSpec};
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
    paint_content_rectangles(
        tree,
        &widths,
        &vertical,
        viewport_width,
        viewport_height,
        &fills,
    )
}
