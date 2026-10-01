//! All-or-nothing paint for direct-text blocks in restricted normal flow.
//!
//! This path is deliberately separate from the background-only preview. It
//! requires one actual font provider for advances, line metrics and coverage;
//! it refuses all other inline shapes and unsupported CSS rather than painting
//! a convincing but incomplete page. It does not apply browser UA margins,
//! borders, images or general CSS text shaping. Root/body solid backgrounds
//! propagate to the canvas only after the whole restricted page is proven.

use crate::box_tree::{BoxKind, BoxTree, UsedHeightPass, VerticalPass, WidthPass};
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
pub fn paint_single_text_page(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
) -> Result<DisplayList, PageError> {
    if tree.boxes.iter().any(|node| node.children.len() > 1) {
        return Err(PageError::Unsupported);
    }
    paint_direct_text_blocks(tree, viewport_width, viewport_height, font)
}

/// Paint zero-parent-edge-margin normal-flow blocks with exactly one
/// direct text child. Horizontal margins use the proven width pass, including
/// auto centering. Adjoining sibling margins collapse: the largest positive
/// value wins against the largest negative value, so later siblings may
/// overlap earlier blocks. Percentage gaps require exact integer pixels and
/// resolve against parent width. Multiple block siblings are allowed; mixed
/// inline runs, images and unsupported styles still refuse the entire page.
/// Paint follows CSS order for non-positioned normal flow: every block
/// background in retained preorder first, then every text run, so overlapped
/// earlier text still paints above a later sibling's background. Horizontal
/// glyph ink may overflow the content box; viewport clipping happens only
/// during rasterization. Direct text may also overflow a positive definite
/// block height without expanding that block or moving its next sibling.
/// Zero content height is also allowed with vertical padding, which prevents
/// through-collapse and provides a nonempty padding-box background.
/// Ink may overflow the line box after exact half-leading; advances and
/// line placement remain bounded. Border-box sizing uses the existing width
/// and definite-height passes to subtract padding before text placement.
/// Percentage widths must resolve to exact integer pixels; diagnostic
/// flooring is not enough to prove wrapping or painted geometry. Percentage
/// heights likewise require exact pixels against specified definite ancestors.
/// Horizontal percentage margins and two-auto-margin centering must also
/// produce integer origins rather than diagnostic rounded positions.
///
/// # Errors
/// Returns an error on unsupported trees, invalid metrics or paint limits.
#[allow(clippy::too_many_lines)]
pub fn paint_direct_text_blocks(
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
        || !tree.paint_structure_valid(MAX_COMMANDS)
    {
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
    // WidthPass is diagnostic and floors percentage widths. Integer paint
    // must not silently change wrapping or background geometry by rounding.
    if tree.boxes.iter().any(|node| {
        if node.kind == BoxKind::Text {
            return false;
        }
        let containing = node.parent.map_or(viewport_width, |parent| {
            widths.widths[parent].map_or(0, |width| width.content)
        });
        matches!(node.style.width, crate::computed_style::Length::Percent(value)
            if (u64::from(containing) * u64::from(value)) % 10_000 != 0)
    }) {
        return Err(PageError::Unsupported);
    }
    // Percent heights resolve against specified definite ancestors, never
    // an auto height derived from children. The diagnostic pass floors them.
    let specified = crate::box_tree::HeightPass::from_boxes(tree, viewport_height);
    if tree.boxes.iter().any(|node| {
        if node.kind == BoxKind::Text {
            return false;
        }
        let containing = node
            .parent
            .map_or(Some(viewport_height), |parent| specified.heights[parent]);
        matches!(node.style.height, crate::computed_style::Length::Percent(value)
            if containing.is_none_or(|base| (u64::from(base) * u64::from(value)) % 10_000 != 0))
    }) {
        return Err(PageError::Unsupported);
    }
    if tree.boxes.iter().enumerate().any(|(index, node)| {
        if node.kind == BoxKind::Text {
            return false;
        }
        let containing = node.parent.map_or(viewport_width, |parent| {
            widths.widths[parent].map_or(0, |width| width.content)
        });
        let fractional = |margin| {
            matches!(margin, crate::computed_style::Margin::Percent(value)
            if (i64::from(containing) * i64::from(value)) % 10_000 != 0)
        };
        if fractional(node.style.margin_left) || fractional(node.style.margin_right) {
            return true;
        }
        // Two auto margins divide positive spare width equally. An odd
        // remainder needs half-pixel origins, not the diagnostic floor.
        let both_auto = node.style.margin_left == crate::computed_style::Margin::Auto
            && node.style.margin_right == crate::computed_style::Margin::Auto;
        let free = widths.widths[index].map_or(0, |width| {
            i64::from(containing)
                - i64::from(width.content)
                - i64::from(node.style.padding_left)
                - i64::from(node.style.padding_right)
        });
        both_auto
            && node.style.width != crate::computed_style::Length::Auto
            && free > 0
            && free % 2 != 0
    }) {
        return Err(PageError::Unsupported);
    }
    let mut text_count = 0;
    let mut seen = vec![false; tree.boxes.len()];
    let mut stack = vec![(tree.roots[0], None)];
    while let Some((index, parent)) = stack.pop() {
        let node = tree.boxes.get(index).ok_or(PageError::Unsupported)?;
        if seen[index] || node.parent != parent || node.source.is_none() {
            return Err(PageError::Unsupported);
        }
        seen[index] = true;
        if node.kind == BoxKind::Text {
            text_count += 1;
            if node.text.is_none()
                || !node.children.is_empty()
                || parent.is_none_or(|p| tree.boxes[p].children.as_slice() != [index])
            {
                return Err(PageError::Unsupported);
            }
        } else if node.kind != BoxKind::Block || node.text.is_some() {
            return Err(PageError::Unsupported);
        }
        for &child in node.children.iter().rev() {
            stack.push((child, Some(index)));
        }
    }
    if seen.iter().any(|&visited| !visited) {
        return Err(PageError::Unsupported);
    }
    if text_count == 0 {
        return Err(PageError::Unsupported);
    }
    // The tree is proven above: its root and first child are the retained
    // HTML root and body. Root color wins; otherwise the body's color paints
    // the canvas and its local background is suppressed. Margin collapse is
    // still unsupported at parent edges. Adjoining sibling margins, both
    // positive and negative, use the already-proven normal-flow collapse.
    let root = tree.roots[0];
    let body = tree.boxes[root].children.first().copied();
    let root_color = tree.boxes[root].style.used_background_color();
    let body_color = body.and_then(|index| tree.boxes[index].style.used_background_color());
    let canvas = root_color.or(body_color);
    let propagated_body = root_color.is_none() && body_color.is_some();
    if tree.boxes.iter().any(|node| {
        if node.kind == BoxKind::Text {
            return false;
        }
        let containing = node.parent.map_or(viewport_width, |parent| {
            widths.widths[parent].map_or(0, |width| width.content)
        });
        // Negative adjoining margins are supported: the proven geometry
        // already combines positive and negative collapses, and the
        // two-phase paint below keeps overlap in CSS order.
        let exact = |margin| match margin {
            crate::computed_style::Margin::Px(_) => true,
            crate::computed_style::Margin::Percent(value) => {
                (i64::from(containing) * i64::from(value)) % 10_000 == 0
            }
            crate::computed_style::Margin::Auto => false,
        };
        if !exact(node.style.margin_top) || !exact(node.style.margin_bottom) {
            return true;
        }
        // A final child's bottom margin may collapse out of its parent;
        // that geometry is not part of this restricted paint proof yet.
        node.children.last().is_some_and(|&child| {
            tree.boxes[child].kind != BoxKind::Text
                && tree.boxes[child].style.margin_bottom != crate::computed_style::Margin::Px(0)
        })
    }) || tree.boxes[root].style.margin_bottom != crate::computed_style::Margin::Px(0)
    {
        return Err(PageError::Unsupported);
    }
    let mut list = DisplayList::default();
    if let Some(color) = canvas {
        list.fill(
            Rect {
                x: 0,
                y: 0,
                width: viewport_width,
                height: viewport_height,
            },
            rgb(color),
            Source {
                node: tree.boxes[root].source,
                action: None,
            },
        )
        .map_err(PageError::Paint)?;
    }
    // Phase one: block backgrounds in retained preorder. A later sibling
    // covers an earlier block's background where negative margins overlap.
    for (index, node) in tree.boxes.iter().enumerate() {
        if node.kind != BoxKind::Text {
            if index == root || (propagated_body && body == Some(index)) {
                continue;
            }
            if let Some(color) = node.style.used_background_color() {
                paint_block_background(
                    &widths, &heights, &vertical, node, index, color, &mut list,
                )?;
            }
        }
    }
    // Phase two: every text run after all backgrounds, matching CSS painting
    // order for non-positioned blocks, so overlapped earlier text stays
    // visible above a later sibling's background.
    for (index, node) in tree.boxes.iter().enumerate() {
        if node.kind == BoxKind::Text {
            let parent = node.parent.ok_or(PageError::Unsupported)?;
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
            if lines.content_height == 0
                || heights.heights[parent].is_none_or(|height| {
                    height == 0
                        && tree.boxes[parent].style.padding_top == 0
                        && tree.boxes[parent].style.padding_bottom == 0
                })
            {
                return Err(PageError::Unsupported);
            }
            let size = tree.boxes[index].style.font_size;
            let natural = font.line_height(size).ok_or(PageError::Unsupported)?;
            let line_height = node
                .style
                .used_line_height(natural)
                .ok_or(PageError::Unsupported)?;
            // CSS half-leading centers the font's line box in explicit height.
            // Leading may be negative when CSS line-height is shorter than the
            // font strut. Ink can extend outside the CSS line box; odd
            // leading needs fractional coordinates, so refuse rather than round.
            let leading = i64::from(line_height) - i64::from(natural);
            if leading % 2 != 0 {
                return Err(PageError::Unsupported);
            }
            let baseline = font
                .baseline_offset(size)
                .filter(|&offset| offset >= 0 && i64::from(offset) < i64::from(natural))
                .ok_or(PageError::Unsupported)?
                .checked_add(i32::try_from(leading / 2).map_err(|_| PageError::InvalidGeometry)?)
                .ok_or(PageError::InvalidGeometry)?;
            let glyphs = paint_direct_glyphs(
                &lines,
                i32::try_from(widths.content_x[parent].ok_or(PageError::InvalidGeometry)?)
                    .map_err(|_| PageError::InvalidGeometry)?,
                i32::try_from(vertical.content_y[index].ok_or(PageError::InvalidGeometry)?)
                    .map_err(|_| PageError::InvalidGeometry)?,
                width,
                size,
                line_height,
                baseline,
                rgb(tree.boxes[index].style.color),
                |character, size| font.raster(character, size),
            )
            .map_err(|_| PageError::Unsupported)?;
            list.append_list(glyphs).map_err(PageError::Paint)?;

            continue;
        }
    }
    Ok(list)
}

/// Paint one block's padding-box background from the proven geometry.
fn paint_block_background(
    widths: &crate::box_tree::WidthPass,
    heights: &crate::box_tree::UsedHeightPass,
    vertical: &crate::box_tree::VerticalPass,
    node: &crate::box_tree::CssBox,
    index: usize,
    color: u32,
    list: &mut DisplayList,
) -> Result<(), PageError> {
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
    .map_err(PageError::Paint)
}
