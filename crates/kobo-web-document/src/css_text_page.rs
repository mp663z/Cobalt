//! All-or-nothing paint for a single direct-text block under block ancestors.
//!
//! This path is deliberately separate from the background-only preview. It
//! requires one actual font provider for advances, line metrics and coverage;
//! it refuses all other inline shapes and unsupported CSS rather than painting
//! a convincing but incomplete page. It does not apply browser UA margins,
//! background propagation, borders, images or general CSS text shaping.

use crate::box_tree::{BoxKind, BoxTree, UsedHeightPass, VerticalPass, WidthPass};
use crate::computed_style::BoxSizing;
use crate::display_list::{
    DisplayList, Error as DisplayError, Rect, Rgb, Source, MAX_COMMANDS, MAX_PIXELS,
};
use crate::inline_lines::{paint_direct_glyphs, place_direct_text, GlyphBitmap};

pub trait FontProvider {
    fn advance(&self, character: char, size: u32) -> Option<u32>;
    fn line_height(&self, size: u32) -> Option<u32>;
    fn baseline_offset(&self, size: u32) -> Option<i32>;
    fn raster(&self, character: char, size: u32) -> Option<GlyphBitmap>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageError {
    Unsupported,
    InvalidViewport,
    InvalidGeometry,
    Paint(DisplayError),
}

fn rgb(color: u32) -> Rgb {
    Rgb(
        ((color >> 16) & 255) as u8,
        ((color >> 8) & 255) as u8,
        (color & 255) as u8,
    )
}

/// Paint a proven single direct-text block and its ancestor padding backgrounds.
/// No image is returned unless every box is accounted for and the same font
/// provider measures and rasters the text. This is an experimental restricted
/// path, not the shipping reader or general browser rendering.
///
/// # Errors
/// Returns an error on unsupported CSS/tree, invalid geometry or font data,
/// resource limits, or a failed paint operation.
#[allow(clippy::too_many_lines)]
pub fn paint_single_text_page(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
) -> Result<DisplayList, PageError> {
    let pixels = usize::try_from(viewport_width).ok().and_then(|w| {
        usize::try_from(viewport_height)
            .ok()
            .and_then(|h| w.checked_mul(h))
    });
    if viewport_width == 0 || viewport_height == 0 || pixels.is_none_or(|n| n > MAX_PIXELS) {
        return Err(PageError::InvalidViewport);
    }
    if tree.unsupported
        || tree.quirks
        || tree.truncated
        || tree.boxes.len() > MAX_COMMANDS
        || tree.roots.len() != 1
        || tree.roots[0] >= tree.boxes.len()
    {
        return Err(PageError::Unsupported);
    }
    // A sibling or nested text block needs per-box paint ordering; this
    // restricted bridge only paints one direct text node at the end of an
    // ancestor chain. Reject any branching before building a list.
    if tree.boxes.iter().any(|node| node.children.len() > 1) {
        return Err(PageError::Unsupported);
    }
    let widths = WidthPass::from_boxes(tree, viewport_width);
    let heights = UsedHeightPass::from_boxes_with_direct_text(
        tree,
        viewport_width,
        viewport_height,
        |character, size| font.advance(character, size),
        |size| font.line_height(size),
    );
    let vertical = VerticalPass::from_boxes_with_direct_text(
        tree,
        viewport_width,
        viewport_height,
        |character, size| font.advance(character, size),
        |size| font.line_height(size),
    );
    if widths.unsupported
        || widths.truncated
        || heights.unsupported
        || heights.truncated
        || vertical.unsupported
        || vertical.truncated
    {
        return Err(PageError::Unsupported);
    }
    let mut text_index = None;
    let mut seen = vec![false; tree.boxes.len()];
    let mut stack = vec![(tree.roots[0], None)];
    while let Some((index, parent)) = stack.pop() {
        let node = tree.boxes.get(index).ok_or(PageError::Unsupported)?;
        if seen[index]
            || node.parent != parent
            || node.source.is_none()
            || node.style.box_sizing != BoxSizing::ContentBox
        {
            return Err(PageError::Unsupported);
        }
        seen[index] = true;
        if node.kind == BoxKind::Text {
            if text_index.replace(index).is_some()
                || node.text.is_none()
                || !node.children.is_empty()
                || !parent.is_some_and(|p| {
                    tree.boxes[p].children.as_slice() == [index]
                        && tree.boxes[p].style.height == crate::computed_style::Length::Auto
                })
            {
                return Err(PageError::Unsupported);
            }
        } else if !matches!(node.kind, BoxKind::Block | BoxKind::ListItem) || node.text.is_some() {
            return Err(PageError::Unsupported);
        }
        for &child in node.children.iter().rev() {
            stack.push((child, Some(index)));
        }
    }
    if seen.iter().any(|&visited| !visited) {
        return Err(PageError::Unsupported);
    }
    let text_index = text_index.ok_or(PageError::Unsupported)?;
    let parent = tree.boxes[text_index]
        .parent
        .ok_or(PageError::Unsupported)?;
    let width = widths.widths[parent]
        .ok_or(PageError::InvalidGeometry)?
        .content;
    let lines = place_direct_text(
        tree,
        parent,
        width,
        |character, size| font.advance(character, size),
        |size| font.line_height(size),
    )
    .map_err(|_| PageError::Unsupported)?;
    if lines.content_height == 0 || heights.heights[parent] != Some(lines.content_height) {
        return Err(PageError::Unsupported);
    }
    // CSS propagates an HTML body's background to the viewport while its
    // root is transparent. Neither root nor body propagation is implemented
    // here. Refuse both instead of painting only the local content box.
    // Parent-child margin collapse is also not implemented.
    let root = tree.roots[0];
    if tree.boxes[root].style.background_color.is_some()
        || tree.boxes[root]
            .children
            .first()
            .is_some_and(|&body| tree.boxes[body].style.background_color.is_some())
        || tree.boxes.iter().any(|node| {
            node.kind != BoxKind::Text
                && (node.style.margin_left != crate::computed_style::Margin::Px(0)
                    || node.style.margin_right != crate::computed_style::Margin::Px(0)
                    || node.style.margin_top != crate::computed_style::Margin::Px(0)
                    || node.style.margin_bottom != crate::computed_style::Margin::Px(0))
        })
    {
        return Err(PageError::Unsupported);
    }
    let mut list = DisplayList::default();
    for (index, node) in tree.boxes.iter().enumerate() {
        if node.kind == BoxKind::Text {
            continue;
        }
        if let Some(color) = node.style.background_color {
            let width = widths.widths[index]
                .ok_or(PageError::InvalidGeometry)?
                .content;
            let height = heights.heights[index].ok_or(PageError::InvalidGeometry)?;
            let x = widths.content_x[index].ok_or(PageError::InvalidGeometry)?
                - i64::from(node.style.padding_left);
            let y = vertical.content_y[index].ok_or(PageError::InvalidGeometry)?
                - i64::from(node.style.padding_top);
            let width = width
                .checked_add(node.style.padding_left)
                .and_then(|n| n.checked_add(node.style.padding_right))
                .ok_or(PageError::InvalidGeometry)?;
            let height = height
                .checked_add(node.style.padding_top)
                .and_then(|n| n.checked_add(node.style.padding_bottom))
                .ok_or(PageError::InvalidGeometry)?;
            if width == 0 || height == 0 {
                return Err(PageError::InvalidGeometry);
            }
            list.fill(
                Rect {
                    x: i32::try_from(x).map_err(|_| PageError::InvalidGeometry)?,
                    y: i32::try_from(y).map_err(|_| PageError::InvalidGeometry)?,
                    width,
                    height,
                },
                rgb(color),
                Source {
                    node: node.source,
                    action: None,
                },
            )
            .map_err(PageError::Paint)?;
        }
    }
    let size = tree.boxes[text_index].style.font_size;
    let line_height = font.line_height(size).ok_or(PageError::Unsupported)?;
    let baseline = font.baseline_offset(size).ok_or(PageError::Unsupported)?;
    let glyphs = paint_direct_glyphs(
        &lines,
        i32::try_from(widths.content_x[parent].ok_or(PageError::InvalidGeometry)?)
            .map_err(|_| PageError::InvalidGeometry)?,
        i32::try_from(vertical.content_y[text_index].ok_or(PageError::InvalidGeometry)?)
            .map_err(|_| PageError::InvalidGeometry)?,
        width,
        size,
        line_height,
        baseline,
        rgb(tree.boxes[text_index].style.color),
        |character, size| font.raster(character, size),
    )
    .map_err(|_| PageError::Unsupported)?;
    list.append_list(glyphs).map_err(PageError::Paint)?;
    Ok(list)
}
