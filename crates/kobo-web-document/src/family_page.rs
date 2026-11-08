//! FL52 increment 3a (UNWIRED, private, unadopted): uniform-family page selection over an explicit catalog.
//!
//! Scope: every Text box must carry a retained family list (`FamilyKey` resolving in the given view), resolve via
//! `select_run_family` (first CSS-available face must cover the WHOLE run, else refuse), and all boxes must resolve to the
//! SAME family. The catalog is the source of truth for family identity: the face provider it returns must agree with the
//! catalog's face hashes and with its coverage claims (`has_glyph`), or the page is refused. No per-run metrics: mixed
//! families are refused. Admission is unchanged: only declarations the existing pinned intake already retains can
//! reach this path (today exactly the two pinned lists); this changes the RESOLVED face, never which declarations are
//! admitted. A catalog's `Absent` claims (e.g. for `gds transport`, `arial`) are the catalog's own grounded statements
//! about its available faces and are not verified here. Face identity is bound by hashes (regular and bold) between
//! catalog and provider; the FL48 odd-leading proof is per attested face/version and is NOT inherited: a provider
//! returning `normal_line_gap == None` for a resolved face keeps odd explicit leading refused. `covers` replaces only the
//! has_glyph check; style/weight/ASCII/whitespace gates run before it. Existing entries are untouched; `FontProvider::face_catalog()==None` never reaches this module.
use crate::box_tree::{BoxKind, BoxTree};
use crate::computed_style::FontStyle;
use crate::css_text_page::{paint_direct_text_blocks, FontProvider, NormalLineMetrics, PageError};
use crate::display_list::DisplayList;
use crate::font_family::{select_run_family, FaceCoverage, FamilyId, FamilyView, RunRefusal};
use crate::inline_lines::GlyphBitmap;

/// A coverage catalog that can also hand out the single-face provider it proves for a family.
pub trait FamilyFaces: FaceCoverage {
    fn face(&self, family: FamilyId) -> Option<&dyn FontProvider>;
    /// Lowercase hex SHA-256 of (regular, bold) font bytes the catalog claims for `family`.
    fn face_hashes(&self, family: FamilyId) -> Option<(String, String)>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageRefusal {
    /// Tree already refused upstream (`unsupported`/`truncated`).
    TreeRefused,
    /// A Text box has no retained list (no declaration, or a CSS-wide keyword reset): no explicit face policy.
    MissingKey,
    /// The key does not resolve in the supplied view (foreign tree).
    ForeignKey,
    /// The text box's immediate parent (its strut owner) has a different list than the text.
    ParentMismatch,
    Style,
    NonAscii,
    Run(RunRefusal),
    MixedFamilies,
    NoText,
    /// Catalog has no face/hashes for the resolved family, or they disagree with the provider.
    FaceDisagreement,
}

/// Delegating adaptor so painters taking `&impl FontProvider` can run against a catalog-owned face.
pub struct DynFace<'a>(pub &'a dyn FontProvider);
impl FontProvider for DynFace<'_> {
    fn face_sha256_hex(&self) -> Option<String> { self.0.face_sha256_hex() }
    fn bold_face_sha256_hex(&self) -> Option<String> { self.0.bold_face_sha256_hex() }
    fn has_glyph(&self, c: char, bold: bool) -> Option<bool> { self.0.has_glyph(c, bold) }
    fn normal_line_metrics(&self, size: u32, bold: bool) -> Option<NormalLineMetrics> { self.0.normal_line_metrics(size, bold) }
    fn normal_line_gap(&self, size: u32, bold: bool) -> Option<u32> { self.0.normal_line_gap(size, bold) }
    fn advance(&self, c: char, size: u32, bold: bool) -> Option<u32> { self.0.advance(c, size, bold) }
    fn line_height(&self, size: u32, bold: bool) -> Option<u32> { self.0.line_height(size, bold) }
    fn baseline_offset(&self, size: u32, bold: bool) -> Option<i32> { self.0.baseline_offset(size, bold) }
    fn raster(&self, c: char, size: u32, bold: bool) -> Option<GlyphBitmap> { self.0.raster(c, size, bold) }
}

fn probe(ch: char) -> Option<char> {
    match ch {
        '\u{20}'..='\u{7e}' | '\u{a0}' => Some(ch),
        '\t' | '\n' | '\r' | '\u{c}' => Some(' '),
        _ => None,
    }
}

/// Select the single family for the whole page and return it with the family-less twin (same guards as the
/// family-bound twin: normal style, weight 400|700, ASCII/whitespace only, per-char provider coverage).
pub fn select_uniform_family(
    tree: &BoxTree,
    views: &FamilyView,
    catalog: &dyn FamilyFaces,
) -> Result<(FamilyId, BoxTree), PageRefusal> {
    if tree.unsupported || tree.truncated {
        return Err(PageRefusal::TreeRefused);
    }
    let mut chosen: Option<FamilyId> = None;
    for node in &tree.boxes {
        if node.kind != BoxKind::Text {
            continue;
        }
        let st = node.style;
        if st.font_style != FontStyle::Normal || !matches!(st.font_weight, 400 | 700) {
            return Err(PageRefusal::Style);
        }
        let bold = st.font_weight == 700;
        let key = st.font_family_list.ok_or(PageRefusal::MissingKey)?;
        let list = views.get(key).ok_or(PageRefusal::ForeignKey)?;
        // The strut belongs to the containing box: its list must be the same one the text inherited.
        let parent_ok = node.parent.and_then(|p| tree.boxes.get(p)).is_some_and(|p| p.style.font_family_list == Some(key));
        if !parent_ok {
            return Err(PageRefusal::ParentMismatch);
        }
        let text = node.text.as_deref().unwrap_or("");
        let probed: Option<String> = text.chars().map(probe).collect();
        let probed = probed.ok_or(PageRefusal::NonAscii)?;
        let id = select_run_family(list, catalog, &probed, bold).map_err(PageRefusal::Run)?;
        // A genuinely empty text box (no characters at all; whitespace-only is NOT empty) takes no part in the
        // uniform-id vote. Its style, key, parent and family-resolution checks above still apply, and its box
        // keeps contributing to layout/strut exactly as in the existing painter.
        if probed.is_empty() {
            continue;
        }
        match chosen {
            None => chosen = Some(id),
            Some(c) if c == id => {}
            Some(_) => return Err(PageRefusal::MixedFamilies),
        }
    }
    let id = chosen.ok_or(PageRefusal::NoText)?;
    // Identity agreement: the provider handed to the painter must be the face the catalog describes.
    let (reg, bold_h) = catalog.face_hashes(id).ok_or(PageRefusal::FaceDisagreement)?;
    let face = catalog.face(id).ok_or(PageRefusal::FaceDisagreement)?;
    if face.face_sha256_hex().as_deref() != Some(reg.as_str()) || face.bold_face_sha256_hex().as_deref() != Some(bold_h.as_str()) {
        return Err(PageRefusal::FaceDisagreement);
    }
    for node in tree.boxes.iter().filter(|n| n.kind == BoxKind::Text) {
        let bold = node.style.font_weight == 700;
        for ch in node.text.as_deref().unwrap_or("").chars() {
            let p = probe(ch).ok_or(PageRefusal::NonAscii)?;
            if face.has_glyph(p, bold) != Some(true) {
                return Err(PageRefusal::FaceDisagreement);
            }
        }
    }
    let mut twin = tree.clone();
    for node in &mut twin.boxes {
        node.style.font_family = None;
    }
    Ok((id, twin))
}

/// Paint with the existing direct-text painter against the selected family's face.
///
/// # Errors
/// `Unsupported` for any [`PageRefusal`]; otherwise as `paint_direct_text_blocks`.
pub fn paint_uniform_family_page(
    tree: &BoxTree,
    views: &FamilyView,
    viewport_width: u32,
    viewport_height: u32,
    catalog: &dyn FamilyFaces,
) -> Result<DisplayList, PageError> {
    let (id, twin) = select_uniform_family(tree, views, catalog).map_err(|_| PageError::Unsupported)?;
    let face = catalog.face(id).ok_or(PageError::Unsupported)?;
    paint_direct_text_blocks(&twin, viewport_width, viewport_height, &DynFace(face))
}
