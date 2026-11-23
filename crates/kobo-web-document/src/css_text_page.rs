//! All-or-nothing paint for direct-text blocks in restricted normal flow.
//!
//! This path is deliberately separate from the background-only preview. It
//! requires one actual font provider for advances, line metrics and coverage;
//! it refuses all other inline shapes and unsupported CSS rather than painting
//! a convincing but incomplete page. It does not apply browser UA margins,
//! arbitrary borders, images or general CSS text shaping. Root/body solid backgrounds
//! propagate to the canvas only after the whole restricted page is proven.

use crate::box_tree::{BoxKind, BoxTree, PageTextLayout, UsedHeightPass, VerticalPass, WidthPass};
use crate::display_list::{
    DisplayList, Error as DisplayError, Rect, Rgb, Source, MAX_COMMANDS, MAX_PIXELS,
};
use crate::inline_lines::{paint_direct_glyphs, place_runs_text, GlyphBitmap, TextShaping};

/// Paired integer metrics for a separately verified normal-line strut.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalLineMetrics {
    pub height: u32,
    pub baseline: i32,
}

/// A shaped cluster over a source run, rastered in baseline-relative coordinates.
pub struct PageCluster {
    pub start: usize,
    pub end: usize,
    pub advance: u32,
    pub bitmap: GlyphBitmap,
}

/// Every method takes the glyph's face: `bold` false is the regular face,
/// true the bold face used for weight-700 runs. A provider without a bold
/// face returns `None` for bold, which refuses the page; it must never
/// substitute regular metrics or bitmaps for a bold glyph.
pub trait FontProvider {
    /// None preserves the existing explicit-face legacy contract, not parity.
    /// Lowercase hex SHA-256 of the regular face's font bytes, if the provider can prove it.
    /// `None` (default) makes the measured underline profile refuse.
    fn face_sha256_hex(&self) -> Option<String> {
        None
    }

    /// Lowercase hex SHA-256 of the BOLD face's font bytes, if the provider can prove it. `None`
    /// (default) makes the family-bound painter refuse. A contract about the supplied bytes only.
    fn bold_face_sha256_hex(&self) -> Option<String> {
        None
    }

    /// Whether the face selected by `bold` maps `character` through its cmap to a real glyph
    /// (glyph id != 0). `Some(false)` and `None` both refuse in the family-bound painter. Metrics
    /// or a raster being returned is NOT evidence of coverage (a .notdef box also has both).
    fn has_glyph(&self, _character: char, _bold: bool) -> Option<bool> {
        None
    }

    fn normal_line_metrics(&self, _size: u32, _bold: bool) -> Option<NormalLineMetrics> {
        None
    }

    /// Rounded CSS-pixel `lineGap` of the verified normal strut at this size, when the provider
    /// can prove it (same sampled sizes as `normal_line_metrics`). `None` (default) keeps odd
    /// explicit-leading refusal: Chrome's half-leading excludes the gap, so it is not derivable
    /// from `line_height` and `baseline_offset` alone.
    fn normal_line_gap(&self, _size: u32, _bold: bool) -> Option<u32> {
        None
    }

    /// Explicit face catalog with per-family coverage (FL51 increment 2 seam). `None` (default) preserves today's
    /// single explicit-face behavior exactly; nothing consumes this yet.
    fn face_catalog(&self) -> Option<&dyn crate::font_family::FaceCoverage> {
        None
    }
    /// Optional OpenType clusters for Page view. Offsets are UTF-8 byte boundaries.
    /// An absent shaper keeps scalar layout; exact painters never consult this hook.
    fn page_shape(&self, _text: &str, _size: u32, _bold: bool, _italic: bool) -> Option<Vec<PageCluster>> {
        None
    }
    /// Page-view style hook. Exact paint paths continue to use `advance`.
    fn page_advance(&self, character: char, size: u32, bold: bool, _italic: bool) -> Option<u32> {
        self.advance(character, size, bold)
    }
    /// Page-view style hook. Providers without italic cuts keep regular shape.
    fn page_raster(&self, character: char, size: u32, bold: bool, _italic: bool) -> Option<GlyphBitmap> {
        self.raster(character, size, bold)
    }
    fn advance(&self, character: char, size: u32, bold: bool) -> Option<u32>;
    fn line_height(&self, size: u32, bold: bool) -> Option<u32>;
    fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32>;
    fn raster(&self, character: char, size: u32, bold: bool) -> Option<GlyphBitmap>;
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

/// Paint normal-flow blocks with a proven inline text context. Parent-edge
/// margin collapse is supported through the proven passes. Horizontal margins use the proven width pass, including
/// auto centering. Adjoining sibling margins collapse: the largest positive
/// value wins against the largest negative value, so later siblings may
/// overlap earlier blocks. Percentage gaps require exact integer pixels and
/// resolve against parent width. Multiple block siblings and bare single-text
/// inline elements may share regular/bold faces; images, nested inlines and
/// unsupported styles still refuse the entire page.
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
pub fn paint_direct_text_blocks(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
) -> Result<DisplayList, PageError> {
    paint_direct_text_blocks_inner(tree, viewport_width, viewport_height, font, None, None)
}

/// PROTOTYPE (host research, unadopted): like [`paint_direct_text_blocks`],
/// but wrap decisions come from a shaped, page-level layout decided once and
/// shared by the height pass, the vertical pass and the painter. If any block
/// is ineligible (non-ASCII, non-left alignment, shaper `None`, malformed
/// clusters, a px conversion that fails) the WHOLE page is painted by the legacy
/// path, byte-identical to `paint_direct_text_blocks`. Glyph positions are
/// still integer px (a single named rounding, once per cumulative x).
///
/// # Errors
/// Same as [`paint_direct_text_blocks`].
pub fn paint_direct_text_blocks_shaped(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
    shaping: &TextShaping,
) -> Result<DisplayList, PageError> {
    paint_direct_text_blocks_inner(tree, viewport_width, viewport_height, font, Some(shaping), None)
}

/// EXPERIMENTAL, unadopted: like [`paint_direct_text_blocks`] but additionally paints
/// `text-decoration: underline` for one restricted shape, bound to a named measured profile.
/// Anything outside that shape (inline decoration, inherited decoration, wrapped lines, bold or
/// italic, other faces or sizes, skip-ink glyphs) refuses the WHOLE page. Use only with a tree
/// from [`BoxTree::from_style_with_text_decoration`].
///
/// # Errors
/// As [`paint_direct_text_blocks`], plus `Unsupported` outside the profile.
pub fn paint_direct_text_blocks_underline(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
    profile: UnderlineProfile,
) -> Result<DisplayList, PageError> {
    if font.face_sha256_hex().as_deref() != Some(profile.face_sha256_hex()) {
        return Err(PageError::Unsupported);
    }
    paint_direct_text_blocks_inner(tree, viewport_width, viewport_height, font, None, Some(profile))
}

/// Named host-measurement profile binding the pinned family lists to the exact face bytes the
/// oracle used. Hash equality is a contract about the supplied bytes, NOT proof of raster,
/// shaping or bitmap parity with Chrome, and the table is one host's measurement (Chrome
/// 156.0.8078.47, fonts-liberation2 2.1.5, "GDS Transport" not installed): not portable, not
/// device verified, no device font is bundled for it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FamilyProfile {
    Chrome156HostLiberation2Sans,
}

impl FamilyProfile {
    #[must_use]
    pub const fn regular_sha256_hex(self) -> &'static str {
        match self {
            Self::Chrome156HostLiberation2Sans => "8d91388f1d3604b3b8ae0e3ee2d140e50cd6122f9214514f4aca772540a4076d",
        }
    }
    #[must_use]
    pub const fn bold_sha256_hex(self) -> &'static str {
        match self {
            Self::Chrome156HostLiberation2Sans => "ba0e0dc3f7aca5b0afbc31e800531ee43be3aa79ae35b2ef1f6470a9547765c4",
        }
    }
}

/// EXPERIMENTAL, private, unadopted: like [`paint_direct_text_blocks`] for a tree from
/// [`crate::parse_style_tree_with_pinned_font_family`]. Refuses unless the provider proves BOTH
/// profile face hashes; every text leaf has computed family exactly SansSerif, style normal and
/// weight 400 or 700 after the cascade; every character is ASCII graphic, space (collapsible
/// ASCII whitespace counts as space) or NBSP and the matching face reports cmap coverage for it.
/// No fallback runs. The other text painters refuse text/runs that carry a family; a tree whose
/// family sits only on boxes without text is not refused by them (and background painters never
/// inspect family).
///
/// # Errors
/// As [`paint_direct_text_blocks`], plus `Unsupported` outside the profile.
pub fn paint_direct_text_blocks_family_bound(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
    profile: FamilyProfile,
) -> Result<DisplayList, PageError> {
    let twin = family_bound_twin(tree, font, profile)?;
    paint_direct_text_blocks_inner(&twin, viewport_width, viewport_height, font, None, None)
}

/// EXPERIMENTAL, private, unadopted, candidate-local API growth: [`paint_direct_text_blocks_family_bound`]
/// with the shaped page-level layout of [`paint_direct_text_blocks_shaped`]. It applies the SAME
/// family gate first (both face hashes, cascade family/style/weight, ASCII + cmap coverage, no
/// fallback); only then is the family-less twin laid out with `shaping`. The shaper is caller
/// supplied and is NOT verified by this entry to match the pinned faces; a shaper that is not
/// built from the hash-proven bytes voids the binding, and this entry proves font identity and
/// coverage only, not shaper accuracy. Strict: `Ok` means the shaped page layout was built
/// (the single build the painter consumes; there is no separate probe); when it cannot be built for any
/// block (shaper `None`, malformed clusters, non-left alignment, ...) the entry returns
/// `Unsupported` and does NOT fall back to the legacy integer path (unlike
/// [`paint_direct_text_blocks_shaped`]).
///
/// # Errors
/// As [`paint_direct_text_blocks_family_bound`].
pub fn paint_direct_text_blocks_family_bound_shaped(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
    profile: FamilyProfile,
    shaping: &TextShaping,
) -> Result<DisplayList, PageError> {
    let twin = family_bound_twin(tree, font, profile)?;
    // Strict by construction: the single shaped build inside the painter either succeeds and is
    // consumed by every later pass, or the call returns Unsupported (no legacy fallback, no probe).
    paint_direct_text_blocks_core(&twin, viewport_width, viewport_height, font, Some(shaping), None, true)
}

fn family_bound_twin(
    tree: &BoxTree,
    font: &impl FontProvider,
    profile: FamilyProfile,
) -> Result<BoxTree, PageError> {
    if font.face_sha256_hex().as_deref() != Some(profile.regular_sha256_hex())
        || font.bold_face_sha256_hex().as_deref() != Some(profile.bold_sha256_hex())
    {
        return Err(PageError::Unsupported);
    }
    for node in &tree.boxes {
        if node.kind != BoxKind::Text {
            continue;
        }
        let st = node.style;
        if st.font_family != Some(crate::computed_style::GenericFamily::SansSerif)
            || st.font_style != crate::computed_style::FontStyle::Normal
            || !matches!(st.font_weight, 400 | 700)
        {
            return Err(PageError::Unsupported);
        }
        let bold = st.font_weight == 700;
        for ch in node.text.as_deref().unwrap_or("").chars() {
            let probe = match ch {
                '\u{20}'..='\u{7e}' | '\u{a0}' => ch,
                '\t' | '\n' | '\r' | '\u{c}' => ' ',
                _ => return Err(PageError::Unsupported),
            };
            if font.has_glyph(probe, bold) != Some(true) {
                return Err(PageError::Unsupported);
            }
        }
    }
    // The inner painters refuse any retained family; the proven tree is painted as its
    // family-less twin so no other check is bypassed.
    let mut twin = tree.clone();
    for node in &mut twin.boxes {
        node.style.font_family = None;
    }
    Ok(twin)
}

/// Named, measured underline geometry. The numbers are NOT derived from font tables: they were
/// measured from headless Chrome 156 pixels for the default serif (Liberation Serif Regular, 100%
/// zoom, DPR 1), `text-decoration: underline`, `text-decoration-skip-ink: auto`, line-height
/// normal, integer font sizes in {12,14,16,20,24,32} (pixel-verified vs Chrome 156; c40 baseline disagreed by 1 row and is refused). The caller must bind the
/// provider's face to this profile; the profile cannot verify the face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnderlineProfile {
    ChromeLiberationSerif156,
}

/// (offset below baseline row, thickness) for font sizes {12,14,16,20,24,32}, measured from Chrome 156.
fn chrome_liberation_serif_underline(size: u32) -> Option<(u32, u32)> {
    // Measured rule: thickness = max(1, floor(size/10)); offset = ceil(size/20), min 1.
    // Rule fits the Chrome table for 8..=57, but Cobalt-vs-Chrome pixels were verified only at the listed sizes.
    [12u32, 14, 16, 20, 24, 32].contains(&size).then(|| ((size.div_ceil(20)).max(1), (size / 10).max(1)))
}

/// Characters whose glyph does not reach the underline band in Chrome 156 with skip-ink auto
/// (no gap observed at 12/16/20/24/32 px). Q g j p q y , ; ( ) [ ] @ were observed to skip ink
/// and everything not listed here was not measured: refused.
fn underline_char_allowed(c: char) -> bool {
    match c {
        'A'..='Z' => c != 'Q',
        'a'..='z' => !matches!(c, 'g' | 'j' | 'p' | 'q' | 'y'),
        '0'..='9' | ' ' | '.' | ':' | '!' | '?' | '-' | '/' | '\\' | '\'' | '"' | '#' | '&' | '*' | '+' | '=' | '%' => true,
        _ => false,
    }
}

impl UnderlineProfile {
    /// SHA-256 of the exact face bytes the profile was measured against (fonts-liberation2 2.1.5).
    #[must_use]
    pub const fn face_sha256_hex(self) -> &'static str {
        match self {
            Self::ChromeLiberationSerif156 => "29d12439831b7f59194efec85872f24f54eff05738933f9a860220d2abff88ba",
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn underline_rect(
        self,
        tree: &BoxTree,
        index: usize,
        lines: &crate::inline_lines::DirectTextLines,
        content_x: i32,
        content_y: i32,
        baseline: i32,
    ) -> Result<Rect, PageError> {
        use crate::computed_style::{TextAlign, TextDecoration};
        let node = &tree.boxes[index];
        // The decorating box must be this block, with only direct text children and no decorated ancestor.
        let mut ancestor = node.parent;
        while let Some(p) = ancestor {
            if tree.boxes[p].style.text_decoration != TextDecoration::None {
                return Err(PageError::Unsupported);
            }
            ancestor = tree.boxes[p].parent;
        }
        if node.style.text_decoration != TextDecoration::Underline
            || node.kind != BoxKind::Block
            || node.children.iter().any(|&c| tree.boxes[c].kind != BoxKind::Text)
            || tree.boxes.iter().enumerate().any(|(i, b)| {
                i != index
                    && b.kind != BoxKind::Text // text leaves mirror their parent's computed style
                    && b.style.text_decoration != TextDecoration::None
            })
            || !matches!(node.style.text_align, TextAlign::Initial | TextAlign::Left)
            || node.style.font_weight != 400
            || node.style.font_style != crate::computed_style::FontStyle::Normal
            || lines.lines.count != 1
            || lines.lines.glyphs.iter().any(|g| g.bold || g.line != 0)
            || lines.lines.glyphs.is_empty()
            || !lines.lines.glyphs.iter().all(|g| underline_char_allowed(g.character))
        {
            return Err(PageError::Unsupported);
        }
        let (offset, thickness) = match self {
            Self::ChromeLiberationSerif156 => chrome_liberation_serif_underline(node.style.font_size),
        }
        .ok_or(PageError::Unsupported)?;
        let first = lines.lines.glyphs.first().ok_or(PageError::Unsupported)?;
        let last = lines.lines.glyphs.last().ok_or(PageError::Unsupported)?;
        let x0 = i64::from(content_x) + i64::from(first.x);
        let x1 = i64::from(content_x) + i64::from(last.x) + i64::from(last.advance);
        let y = i64::from(content_y) + i64::from(baseline) + i64::from(offset);
        Ok(Rect {
            x: i32::try_from(x0).map_err(|_| PageError::InvalidGeometry)?,
            y: i32::try_from(y).map_err(|_| PageError::InvalidGeometry)?,
            width: u32::try_from(x1 - x0).map_err(|_| PageError::InvalidGeometry)?,
            height: thickness,
        })
    }
}

/// The shaped page-layout construction (called exactly once per paint).
fn shaped_page_layout(
    tree: &BoxTree,
    widths: &WidthPass,
    shaping: &TextShaping,
    font: &impl FontProvider,
) -> Option<PageTextLayout> {
    PageTextLayout::build(tree, widths, shaping, |index, size| {
        let node = &tree.boxes[index];
        if node.style.line_height.is_none() && node.style.line_height_number.is_none() && node.style.line_height_precise.is_none() {
            font.normal_line_metrics(size, false)
                .map(|m| m.height)
                .or_else(|| font.line_height(size, false))
        } else {
            font.line_height(size, false)
        }
    })
}

fn paint_direct_text_blocks_inner(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
    shaping: Option<&TextShaping>,
    underline: Option<UnderlineProfile>,
) -> Result<DisplayList, PageError> {
    paint_direct_text_blocks_core(tree, viewport_width, viewport_height, font, shaping, underline, false)
}

/// `strict_shaped`: when `shaping` is `Some` and the shaped page layout cannot be built, return
/// `Unsupported` from this SAME single build instead of the whole-page legacy fallback. The
/// layout built here is the one every later pass consumes; nothing is probed and rebuilt.
#[allow(clippy::too_many_lines)]
fn paint_direct_text_blocks_core(
    tree: &BoxTree,
    viewport_width: u32,
    viewport_height: u32,
    font: &impl FontProvider,
    shaping: Option<&TextShaping>,
    underline: Option<UnderlineProfile>,
    strict_shaped: bool,
) -> Result<DisplayList, PageError> {
    let pixels = usize::try_from(viewport_width).ok().and_then(|w| {
        usize::try_from(viewport_height)
            .ok()
            .and_then(|h| w.checked_mul(h))
    });
    if viewport_width == 0 || viewport_height == 0 || pixels.is_none_or(|n| n > MAX_PIXELS) {
        return Err(PageError::InvalidViewport);
    }
    // Any decorated box is refused unless the caller opted in with a profile AND the box is
    // inside the validated one-line direct-text subset (checked per block below).
    if underline.is_none()
        && tree
            .boxes
            .iter()
            .any(|b| b.style.text_decoration != crate::computed_style::TextDecoration::None)
    {
        return Err(PageError::Unsupported);
    }
    if tree.unsupported
        || tree.quirks
        || tree.truncated
        || tree.boxes.len() > MAX_COMMANDS
        || tree.roots.len() != 1
        || tree.roots[0] >= tree.boxes.len()
        || !tree.paint_structure_valid_layers(MAX_COMMANDS, false, true)
        || tree
            .boxes
            .iter()
            .any(|node| !crate::css_border::supported(node.style, false))
    {
        // Different side colours still need diagonal joins.
        return Err(PageError::Unsupported);
    }
    // Validate every complete inline subtree before any metric provider is called.
    for (index, node) in tree.boxes.iter().enumerate() {
        if node.kind == BoxKind::Block
            && !node.children.is_empty()
            && node
                .children
                .iter()
                .all(|&c| matches!(tree.boxes[c].kind, BoxKind::Text | BoxKind::Inline))
        {
            crate::inline_lines::collect_inline_runs(tree, index)
                .map_err(|_| PageError::Unsupported)?;
        }
    }
    let widths = WidthPass::from_boxes(tree, viewport_width);
    let page_layout = shaping.and_then(|shaping| shaped_page_layout(tree, &widths, shaping, font));
    if shaping.is_some() && page_layout.is_none() && strict_shaped {
        return Err(PageError::Unsupported);
    }
    if shaping.is_some() && page_layout.is_none() {
        // Whole-page legacy fallback: no mixed metric systems.
        return paint_direct_text_blocks_inner(tree, viewport_width, viewport_height, font, None, underline);
    }
    let heights = if let Some(layout) = &page_layout {
        UsedHeightPass::from_boxes_with_page_text(tree, viewport_width, viewport_height, layout)
    } else {
        UsedHeightPass::from_boxes_with_line_policy(
        tree,
        viewport_width,
        viewport_height,
        |character, size, bold| font.advance(character, size, bold),
        |size, normal| {
            if normal {
                font.normal_line_metrics(size, false)
                    .map(|m| m.height)
                    .or_else(|| font.line_height(size, false))
            } else {
                font.line_height(size, false)
            }
        },
    )
    };
    let vertical = if let Some(layout) = &page_layout {
        VerticalPass::from_boxes_with_page_text(tree, viewport_width, viewport_height, layout)
    } else {
        VerticalPass::from_boxes_with_line_policy(
        tree,
        viewport_width,
        viewport_height,
        |character, size, bold| font.advance(character, size, bold),
        |size, normal| {
            if normal {
                font.normal_line_metrics(size, false)
                    .map(|m| m.height)
                    .or_else(|| font.line_height(size, false))
            } else {
                font.line_height(size, false)
            }
        },
    )
    };
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
        // A pure inline-content block holds one inline formatting context:
        // text nodes and single-text inline elements. The placer validates
        // the runs themselves; this walk proves the tree's shape only.
        if node.kind == BoxKind::Text {
            text_count += 1;
            let shaped = crate::box_tree::inline_owner(tree, index).is_some();
            if node.text.is_none() || !node.children.is_empty() || !shaped {
                return Err(PageError::Unsupported);
            }
        } else if node.kind == BoxKind::Inline {
            if node.text.is_some() || crate::box_tree::inline_owner(tree, index).is_none() {
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
    // the canvas and its local background is suppressed. Adjoining sibling
    // margins and parent-edge through-collapse both use the proven geometry.
    if tree.boxes.iter().any(|node| node.style.box_shadow.is_some() || node.style.paints_outline()) {
        return Err(PageError::Unsupported);
    }
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
            // The cascade resolves exact em margins to pixels; a surviving
            // `Em` is subpixel or overflowing geometry, which cannot paint.
            crate::computed_style::Margin::Auto | crate::computed_style::Margin::Em(_) => false,
        };
        if !exact(node.style.margin_top) || !exact(node.style.margin_bottom) {
            return true;
        }
        // Parent-edge margin collapse needs no extra paint refusal: the
        // proven passes already walk the through-collapse chain, keep an
        // escaped edge margin out of auto heights, and carry an escaped
        // first-child margin in the parent's own placement gap.
        false
    }) || tree.boxes[root].style.margin_bottom != crate::computed_style::Margin::Px(0)
        || tree.boxes[root].style.margin_top != crate::computed_style::Margin::Px(0)
    {
        return Err(PageError::Unsupported);
    }
    let mut list = DisplayList::default();
    let mut underlined = 0usize;
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
    // CSS 2.2 App. E: layer None is all normal-flow content (no relative ancestor-or-self). Each `position:relative`
    // block then paints as one layer (its backgrounds, then its inline content), and each absolute leaf paints its own
    // background/border, all in tree order. Without relative boxes this is exactly the previous two-phase order.
    let is_pos = |i: usize, p: crate::computed_style::Position| tree.boxes[i].style.positioned.position == p;
    let mut owner: Vec<Option<usize>> = vec![None; tree.boxes.len()];
    for i in 0..tree.boxes.len() {
        owner[i] = if is_pos(i, crate::computed_style::Position::Relative) {
            Some(i)
        } else {
            tree.boxes[i].parent.and_then(|p| owner.get(p).copied().flatten())
        };
    }
    let mut items: Vec<(Option<usize>, bool)> = vec![(None, false)];
    for i in 0..tree.boxes.len() {
        if is_pos(i, crate::computed_style::Position::Relative) {
            items.push((Some(i), false));
        } else if is_pos(i, crate::computed_style::Position::Absolute) {
            items.push((Some(i), true));
        }
    }
    for (layer, abs_leaf) in items {
    if abs_leaf {
        let index = layer.ok_or(PageError::InvalidGeometry)?;
        let node = &tree.boxes[index];
        if let Some(color) = node.style.used_background_color() {
            paint_block_background(&widths, &heights, &vertical, node, index, color, &mut list)?;
        }
        paint_block_border(&widths, &heights, &vertical, node, index, &mut list)?;
        continue;
    }
    // Each block's background and border precede its descendants' paint.
    for (index, node) in tree.boxes.iter().enumerate() {
        if node.kind != BoxKind::Block
            || node.style.positioned.position == crate::computed_style::Position::Absolute
            || owner[index] != layer
        {
            continue;
        }
        if index != root && !(propagated_body && body == Some(index)) {
            if let Some(color) = node.style.used_background_color() {
                paint_block_background(
                    &widths, &heights, &vertical, node, index, color, &mut list,
                )?;
            }
        }
        paint_block_border(&widths, &heights, &vertical, node, index, &mut list)?;
    }
    // Phase two: every block's inline runs after all backgrounds, matching
    // CSS painting order for non-positioned blocks, so overlapped earlier
    // text still paints above a later sibling's background.
    for (index, node) in tree.boxes.iter().enumerate() {
        if node.kind == BoxKind::Block
            && owner[index] == layer
            && !node.children.is_empty()
            && node
                .children
                .iter()
                .all(|&child| matches!(tree.boxes[child].kind, BoxKind::Text | BoxKind::Inline))
        {
            let width = widths.widths[index]
                .ok_or(PageError::InvalidGeometry)?
                .content;
            let legacy_lines;
            let lines = if let Some(layout) = &page_layout {
                let shaped = layout.blocks[index].as_ref().ok_or(PageError::Unsupported)?;
                // Heights, vertical pass and painter all read this one layout, so
                // agreement is by construction (no separate cross-check). Provider
                // instability is caught by the pre-existing glyph-placement gates.
                shaped
            } else {
                legacy_lines =             place_runs_text(
                tree,
                index,
                width,
                |character, size, bold| font.advance(character, size, bold),
                |size| {
                    if node.style.line_height.is_none() && node.style.line_height_number.is_none() && node.style.line_height_precise.is_none() {
                        font.normal_line_metrics(size, false)
                            .map(|m| m.height)
                            .or_else(|| font.line_height(size, false))
                    } else {
                        font.line_height(size, false)
                    }
                },
            )
            .map_err(|_| PageError::Unsupported)?;
                &legacy_lines
            };
            if lines.content_height == 0
                || heights.heights[index].is_none_or(|height| {
                    height == 0 && node.style.padding_top == 0 && node.style.padding_bottom == 0
                })
            {
                return Err(PageError::Unsupported);
            }
            let size = node.style.font_size;
            let normal =
                node.style.line_height.is_none() && node.style.line_height_number.is_none() && node.style.line_height_precise.is_none();
            let paired = normal
                .then(|| font.normal_line_metrics(size, false))
                .flatten();
            let natural = paired
                .map(|m| m.height)
                .or_else(|| font.line_height(size, false))
                .ok_or(PageError::Unsupported)?;
            let line_height = node
                .style
                .used_line_height(natural)
                .ok_or(PageError::Unsupported)?;
            // CSS half-leading centers the font's line box in explicit height.
            // Leading may be negative when CSS line-height is shorter than the
            // font strut. Ink can extend outside the CSS line box. Odd leading
            // floors the offset (Chrome 156 oracle, DejaVu Sans, lh78 stair).
            let leading = i64::from(line_height) - i64::from(natural);
            let base_offset = paired
                .map(|m| m.baseline)
                .or_else(|| font.baseline_offset(size, false))
                .filter(|&offset| offset >= 0 && i64::from(offset) < i64::from(natural))
                .ok_or(PageError::Unsupported)?;
            // Chrome 156 (private lh78 oracle): the half-leading splits `line-height - (ascent +
            // descent)`, floored, with the line gap excluded. When the provider proves the gap and
            // its strut agrees with the legacy metrics, that formula is used for either parity.
            // Otherwise only even leading is admitted, as before (exact for gap 0 or 1).
            let proven = font.normal_line_gap(size, false).and_then(|gap| {
                let strut = font.normal_line_metrics(size, false)?;
                let gap_i = i64::from(gap);
                let ascent = i64::from(strut.baseline) - gap_i / 2;
                if i64::from(strut.height) != i64::from(natural)
                    || gap > strut.height
                    || ascent != i64::from(base_offset)
                {
                    return None;
                }
                let ad = i64::from(natural) - gap_i;
                Some((i64::from(line_height) - ad).div_euclid(2))
            });
            let half = match proven {
                Some(half) => half,
                None if leading % 2 == 0 => leading / 2,
                None => return Err(PageError::Unsupported),
            };
            let baseline = base_offset
                .checked_add(i32::try_from(half).map_err(|_| PageError::InvalidGeometry)?)
                .ok_or(PageError::InvalidGeometry)?;
            if lines.lines.glyphs.iter().any(|glyph| glyph.bold) {
                // Bold runs share the block strut only when the bold face's
                // line metrics match the regular face exactly; otherwise the
                // single line height and baseline would be a guess.
                if (paired.is_some() && font.normal_line_metrics(size, true) != paired)
                    || (paired.is_none()
                        && (font.line_height(size, true) != Some(natural)
                            || font.baseline_offset(size, true).filter(|&offset| {
                                offset >= 0 && i64::from(offset) < i64::from(natural)
                            }) != font.baseline_offset(size, false)))
                {
                    return Err(PageError::Unsupported);
                }
            }
            // delta10: each glyph paints its own leaf's resolved colour. The
            // map is built from the actual leaf boxes of this block; a leaf
            // that cannot be resolved refuses the page (no fallback colour).
            let leaf_colors = lines
                .leaves
                .iter()
                .map(|&source| {
                    tree.boxes
                        .iter()
                        .find(|b| b.source == Some(source))
                        .map(|b| rgb(b.style.color))
                })
                .collect::<Option<Vec<_>>>()
                .ok_or(PageError::Unsupported)?;
            let glyphs = paint_direct_glyphs(
                lines,
                i32::try_from(widths.content_x[index].ok_or(PageError::InvalidGeometry)?)
                    .map_err(|_| PageError::InvalidGeometry)?,
                i32::try_from(vertical.content_y[index].ok_or(PageError::InvalidGeometry)?)
                    .map_err(|_| PageError::InvalidGeometry)?,
                width,
                size,
                line_height,
                baseline,
                |leaf| leaf_colors[leaf],
                |character, size, bold| font.raster(character, size, bold),
            )
            .map_err(|_| PageError::Unsupported)?;
            list.append_list(glyphs).map_err(PageError::Paint)?;
            // CSS 2.1 16.3.1: a decoration takes the colour of the element that ESTABLISHED
            // it. underline_rect admits only a decorated block whose children are direct
            // text (which mirrors the block's style), with no decorated ancestor and no
            // other decorated box - so the establishing element IS this block and
            // rgb(node.style.color) is the establishing element's resolved colour, not a
            // block-colour substitution. Every shape where they could diverge (an inline
            // descendant with its own colour, e.g. a UA-underlined blue link in a black
            // block) fails underline_rect or the decorated-box count check below and the
            // whole page refuses (pinned in tests/delta10_underline_colour.rs).
            if node.style.text_decoration != crate::computed_style::TextDecoration::None {
                let profile = underline.ok_or(PageError::Unsupported)?;
                let rect = profile.underline_rect(
                    tree,
                    index,
                    lines,
                    i32::try_from(widths.content_x[index].ok_or(PageError::InvalidGeometry)?)
                        .map_err(|_| PageError::InvalidGeometry)?,
                    i32::try_from(vertical.content_y[index].ok_or(PageError::InvalidGeometry)?)
                        .map_err(|_| PageError::InvalidGeometry)?,
                    baseline,
                )?;
                list.fill(
                    rect,
                    rgb(node.style.color),
                    Source {
                        node: node.source,
                        action: None,
                    },
                )
                .map_err(PageError::Paint)?;
                underlined += 1;
            }
        }
    }
    }
    // Every decorated non-text box must have been painted as a validated underline block.
    if tree.boxes.iter().filter(|b| b.kind != BoxKind::Text && b.style.text_decoration != crate::computed_style::TextDecoration::None).count() != underlined {
        return Err(PageError::Unsupported);
    }
    Ok(list)
}

fn paint_block_border(
    widths: &WidthPass,
    heights: &UsedHeightPass,
    vertical: &VerticalPass,
    node: &crate::box_tree::CssBox,
    index: usize,
    list: &mut DisplayList,
) -> Result<(), PageError> {
    crate::css_border::paint(
        list,
        Rect {
            x: i32::try_from(widths.content_x[index].ok_or(PageError::InvalidGeometry)?)
                .map_err(|_| PageError::InvalidGeometry)?,
            y: i32::try_from(vertical.content_y[index].ok_or(PageError::InvalidGeometry)?)
                .map_err(|_| PageError::InvalidGeometry)?,
            width: widths.widths[index]
                .ok_or(PageError::InvalidGeometry)?
                .content,
            height: heights.heights[index].ok_or(PageError::InvalidGeometry)?,
        },
        node.style,
        Source {
            node: node.source,
            action: None,
        },
        false,
    )
    .map_err(|error| match error {
        // A non-solid pattern without a proven integer fit is unsupported,
        // not a provider or allocation error. No partial list is returned.
        DisplayError::InvalidSurface => PageError::Unsupported,
        other => PageError::Paint(other),
    })?;
    Ok(())
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
    // `padding_*` is padding plus border; the background colour is bounded
    // by the clip of the bottom-most background layer, mirroring the
    // proven bridge semantics. A background clipped to an empty area
    // paints nothing: that is the clip's correct result, not a failure.
    let border = node.style.decor.used_border();
    let (top, right, bottom, left) = match node.style.decor.colour_clip() {
        crate::computed_style::Clip::Border => (0, 0, 0, 0),
        crate::computed_style::Clip::Padding => (border[0], border[1], border[2], border[3]),
        crate::computed_style::Clip::Content => (
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
    let x = widths.content_x[index]
        .ok_or(PageError::InvalidGeometry)?
        .checked_sub(i64::from(left))
        .ok_or(PageError::InvalidGeometry)?;
    let y = vertical.content_y[index]
        .ok_or(PageError::InvalidGeometry)?
        .checked_sub(i64::from(top))
        .ok_or(PageError::InvalidGeometry)?;
    let width = width
        .checked_add(left)
        .and_then(|n| n.checked_add(right))
        .ok_or(PageError::InvalidGeometry)?;
    let height = height
        .checked_add(top)
        .and_then(|n| n.checked_add(bottom))
        .ok_or(PageError::InvalidGeometry)?;
    if width == 0 || height == 0 {
        return Ok(());
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
