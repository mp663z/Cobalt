//! FL52 increment 3a: uniform-family selection over a SYNTHETIC two-face catalog (no real font bytes).
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{paint_direct_text_blocks, FontProvider, PageError},
    family_page::{paint_uniform_family_page, select_uniform_family, FamilyFaces, PageRefusal},
    font_family::{FaceCatalog, FaceCoverage, FamilyId, Generic, Lookup, RunRefusal},
    inline_lines::GlyphBitmap,
    parse_style_tree, parse_style_tree_with_pinned_font_family, Limits, SupplyBudget,
};

struct Syn {
    sha: &'static str,
    bold_sha: &'static str,
    no_glyph: Option<char>,
    bold_glyph: bool,
}
impl FontProvider for Syn {
    fn face_sha256_hex(&self) -> Option<String> { Some(self.sha.into()) }
    fn bold_face_sha256_hex(&self) -> Option<String> { Some(self.bold_sha.into()) }
    fn has_glyph(&self, c: char, bold: bool) -> Option<bool> { Some(Some(c) != self.no_glyph && (!bold || self.bold_glyph)) }
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(5) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(20) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(15) }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] })
    }
}
/// "alpha" (id 1) = generic sans-serif; "beta" (id 2) = name "arial"; "gds transport" is Absent.
struct Cat {
    alpha: Syn,
    beta: Syn,
    /// catalog-claimed hashes for alpha (can be made to disagree with the provider)
    alpha_hash: (&'static str, &'static str),
    /// coverage claim: alpha covers `z` even if its provider says it does not
    claim_all: bool,
}
fn cat() -> Cat {
    Cat {
        alpha: Syn { sha: "a1", bold_sha: "a2", no_glyph: None, bold_glyph: true },
        beta: Syn { sha: "b1", bold_sha: "b2", no_glyph: None, bold_glyph: true },
        alpha_hash: ("a1", "a2"),
        claim_all: false,
    }
}
impl FaceCatalog for Cat {
    fn lookup(&self, n: &str) -> Lookup {
        match n { "arial" => Lookup::Available(FamilyId(2)), "gds transport" => Lookup::Absent, _ => Lookup::Unknown }
    }
    fn generic(&self, g: Generic) -> Option<FamilyId> { (g == Generic::SansSerif).then_some(FamilyId(1)) }
}
impl FaceCoverage for Cat {
    fn covers(&self, f: FamilyId, c: char, bold: bool) -> Option<bool> {
        let p = if f.0 == 1 { &self.alpha } else { &self.beta };
        if self.claim_all && f.0 == 1 { return Some(true); }
        p.has_glyph(c, bold)
    }
}
impl FamilyFaces for Cat {
    fn face(&self, f: FamilyId) -> Option<&dyn FontProvider> {
        match f.0 { 1 => Some(&self.alpha), 2 => Some(&self.beta), _ => None }
    }
    fn face_hashes(&self, f: FamilyId) -> Option<(String, String)> {
        match f.0 { 1 => Some((self.alpha_hash.0.into(), self.alpha_hash.1.into())), 2 => Some(("b1".into(), "b2".into())), _ => None }
    }
}

const L1: &str = "sans-serif";
const L2: &str = "\"GDS Transport\", arial, sans-serif"; // pinned lists; arial is Available in this catalog -> beta
const L3: &str = "\"GDS Transport\", sans-serif"; // not a pinned list (default entry refuses it)
fn page(css: &str, body: &str) -> (kobo_web_document::style_tree::StyleTree, BoxTree) {
    let h = format!("<!doctype html><style>html,body,p,section{{margin:0}}p{{line-height:20px}}{css}</style>{body}");
    let s = parse_style_tree_with_pinned_font_family(h.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
    let t = BoxTree::from_style(&s);
    (s, t)
}
fn sel(css: &str, body: &str, c: &Cat) -> Result<FamilyId, PageRefusal> {
    let (s, t) = page(css, body);
    select_uniform_family(&t, s.families.as_ref().unwrap(), c).map(|(id, _)| id)
}

#[test]
fn uniform_page_selects_family_and_paints_same_as_direct_painter() {
    let c = cat();
    let css = format!("section{{font-family:{L1}}}");
    let (s, t) = page(&css, "<section><p>words</p></section>");
    assert!(!s.unsupported);
    let v = s.families.as_ref().unwrap();
    assert_eq!(select_uniform_family(&t, v, &c).map(|(i, _)| i), Ok(FamilyId(1)));
    let got = paint_uniform_family_page(&t, v, 200, 100, &c).unwrap();
    let mut twin = t.clone();
    for n in &mut twin.boxes { n.style.font_family = None; }
    let want = paint_direct_text_blocks(&twin, 200, 100, &c.alpha).unwrap();
    let bg = kobo_web_document::display_list::Rgb(255, 255, 255);
    assert_eq!(got.commands().len(), want.commands().len());
    assert!(got.commands().len() > 0);
    assert_eq!(got.rasterize(200, 100, bg).unwrap(), want.rasterize(200, 100, bg).unwrap());
    // list 2 resolves to the OTHER family (Absent skipped, arial Available)
    let css2 = format!("section{{font-family:{L2}}}");
    assert_eq!(sel(&css2, "<section><p>words</p></section>", &c), Ok(FamilyId(2)));
}

#[test]
fn missing_keyword_reset_and_foreign_key_refuse() {
    let c = cat();
    assert_eq!(sel("", "<section><p>words</p></section>", &c), Err(PageRefusal::MissingKey));
    let css = format!("section{{font-family:{L1}}} p{{font-family:initial}}");
    // today the pinned intake already refuses the whole tree for a keyword reset (TreeRefused); MissingKey is the
    // selector's own guard for a keyless text box and is exercised by the no-declaration case above.
    assert_eq!(sel(&css, "<section><p>words</p></section>", &c), Err(PageRefusal::TreeRefused));
    let css = format!("section{{font-family:{L1}}}");
    let (_, t) = page(&css, "<section><p>words</p></section>");
    let (other, _) = page(&format!("section{{font-family:{L2}}}"), "<section><p>words</p></section>");
    assert_eq!(select_uniform_family(&t, other.families.as_ref().unwrap(), &c).err(), Some(PageRefusal::ForeignKey));
}

#[test]
fn mixed_families_refuse_even_when_each_resolves() {
    let c = cat();
    let css = format!("#a{{font-family:{L1}}} #b{{font-family:{L2}}}");
    let body = "<section><p id=a>one</p><p id=b>two</p></section>";
    assert_eq!(sel(&css, body, &c), Err(PageRefusal::MixedFamilies));
}

#[test]
fn partial_coverage_nonascii_style_bold_and_notext_refuse() {
    let mut c = cat();
    c.alpha.no_glyph = Some('z');
    let css = format!("section{{font-family:{L1}}}");
    assert_eq!(sel(&css, "<section><p>zebra</p></section>", &c), Err(PageRefusal::Run(RunRefusal::Uncovered)));
    let c = cat();
    assert_eq!(sel(&css, "<section><p>caf\u{e9}</p></section>", &c), Err(PageRefusal::NonAscii));
    assert_eq!(sel(&format!("{css} p{{font-style:italic}}"), "<section><p>words</p></section>", &c), Err(PageRefusal::Style));
    assert_eq!(sel(&format!("{css} p{{font-weight:600}}"), "<section><p>words</p></section>", &c), Err(PageRefusal::Style));
    let mut cb = cat();
    cb.alpha.bold_glyph = false;
    assert_eq!(sel(&format!("{css} p{{font-weight:700}}"), "<section><p>words</p></section>", &cb), Err(PageRefusal::Run(RunRefusal::Uncovered)));
    assert_eq!(sel(&format!("{css} p{{font-weight:700}}"), "<section><p>words</p></section>", &c), Ok(FamilyId(1)));
    assert_eq!(sel(&css, "<section></section>", &c), Err(PageRefusal::NoText));
}

#[test]
fn catalog_provider_disagreement_refuses() {
    let css = format!("section{{font-family:{L1}}}");
    let body = "<section><p>zebra</p></section>";
    // catalog hashes differ from the provider's
    let mut c = cat();
    c.alpha_hash = ("zz", "a2");
    assert_eq!(sel(&css, body, &c), Err(PageRefusal::FaceDisagreement));
    let mut c = cat();
    c.alpha_hash = ("a1", "zz");
    assert_eq!(sel(&css, body, &c), Err(PageRefusal::FaceDisagreement));
    // catalog claims coverage the provider denies
    let mut c = cat();
    c.alpha.no_glyph = Some('z');
    c.claim_all = true;
    assert_eq!(sel(&css, body, &c), Err(PageRefusal::FaceDisagreement));
}

#[test]
fn tree_refused_upstream_never_selects_or_paints() {
    let c = cat();
    // default entry refuses font-family lists (whole-render unsupported), keys still retained
    let h = format!("<!doctype html><style>section{{font-family:{L1}}}</style><section><p>words</p></section>");
    let s = parse_style_tree(h.as_bytes(), &[], &Limits::DEFAULT);
    assert!(s.unsupported);
    let t = BoxTree::from_style(&s);
    let v = s.families.as_ref().unwrap();
    assert_eq!(select_uniform_family(&t, v, &c).err(), Some(PageRefusal::TreeRefused));
    assert_eq!(paint_uniform_family_page(&t, v, 200, 100, &c).err(), Some(PageError::Unsupported));
    let _ = L3;
}

#[test]
fn anonymous_block_and_inline_text_inherit_the_key_and_select() {
    let c = cat();
    let css = format!("section{{font-family:{L1}}}");
    assert_eq!(sel(&css, "<section><p>block</p>tail</section>", &c), Ok(FamilyId(1)));
    assert_eq!(sel(&css, "<section>direct <b>bold</b> text</section>", &c), Ok(FamilyId(1)));
}

#[test]
fn empty_text_box_skips_only_the_vote_and_whitespace_only_does_not() {
    let c = cat();
    let css = format!("#a{{font-family:{L1}}} #b{{font-family:{L2}}}");
    let (s, mut t) = page(&css, "<section><p id=a>one</p><p id=b>two</p></section>");
    let v = s.families.as_ref().unwrap();
    assert_eq!(select_uniform_family(&t, v, &c).err(), Some(PageRefusal::MixedFamilies));
    let idx = t.boxes.iter().position(|b| b.text.as_deref() == Some("two")).unwrap();
    t.boxes[idx].text = Some(String::new());
    assert_eq!(select_uniform_family(&t, v, &c).map(|(i, _)| i), Ok(FamilyId(1)));
    // whitespace-only is not empty: it votes, so the mix still refuses
    t.boxes[idx].text = Some(" \t".into());
    assert_eq!(select_uniform_family(&t, v, &c).err(), Some(PageRefusal::MixedFamilies));
    // empty box keeps its key check: a keyless empty box still refuses
    t.boxes[idx].text = Some(String::new());
    t.boxes[idx].style.font_family_list = None;
    assert_eq!(select_uniform_family(&t, v, &c).err(), Some(PageRefusal::MissingKey));
}

#[test]
fn catalog_coverage_claims_never_widen_ascii_or_whitespace_gates() {
    // This catalog claims coverage of EVERYTHING (Syn has no_glyph None): non-ASCII must still refuse.
    let c = cat();
    let css = format!("section{{font-family:{L1}}}");
    for t in ["caf\u{e9}", "\u{2603}", "a\u{2028}b"] {
        assert_eq!(sel(&css, &format!("<section><p>{t}</p></section>"), &c), Err(PageRefusal::NonAscii), "{t:?}");
    }
    // tab/newline normalise to space and nbsp is allowed
    assert_eq!(sel(&css, "<section><p>a\tb\nc&nbsp;d</p></section>", &c), Ok(FamilyId(1)));
}

#[test]
fn odd_explicit_leading_is_not_inherited_by_a_new_face() {
    // Syn proves no strut/gap (normal_line_gap default None): odd explicit leading must stay refused, even leading is as before.
    let c = cat();
    let css = format!("section{{font-family:{L1}}} p{{line-height:21px}}");
    let (s, t) = page(&css, "<section><p>words</p></section>");
    assert_eq!(paint_uniform_family_page(&t, s.families.as_ref().unwrap(), 200, 100, &c).err(), Some(PageError::Unsupported));
    let css = format!("section{{font-family:{L1}}} p{{line-height:20px}}");
    let (s, t) = page(&css, "<section><p>words</p></section>");
    assert!(paint_uniform_family_page(&t, s.families.as_ref().unwrap(), 200, 100, &c).is_ok());
}

#[test]
fn text_parent_key_split_refuses_parent_mismatch_and_matching_keys_select() {
    let c = cat();
    let css = format!("#a{{font-family:{L1}}} #b{{font-family:{L2}}}");
    let (s, t0) = page(&css, "<section><p id=a>one</p><p id=b>two</p></section>");
    let v = s.families.as_ref().unwrap();
    let ti = t0.boxes.iter().position(|b| b.text.as_deref() == Some("one")).unwrap();
    let pi = t0.boxes[ti].parent.unwrap();
    let other = t0.boxes.iter().find(|b| b.text.as_deref() == Some("two")).unwrap().style.font_family_list;
    assert!(other.is_some() && other != t0.boxes[ti].style.font_family_list);
    // inverse: make the page uniform (second box empty so it does not vote), keys matching text/parent -> selects
    let mut ok = t0.clone();
    let bi = ok.boxes.iter().position(|b| b.text.as_deref() == Some("two")).unwrap();
    ok.boxes[bi].text = Some(String::new());
    assert_eq!(select_uniform_family(&ok, v, &c).map(|(i, _)| i), Ok(FamilyId(1)));
    // split A: parent carries the OTHER family's valid key
    let mut a = ok.clone();
    a.boxes[pi].style.font_family_list = other;
    assert_eq!(select_uniform_family(&a, v, &c).err(), Some(PageRefusal::ParentMismatch));
    // split B: parent has no key
    let mut b = ok.clone();
    b.boxes[pi].style.font_family_list = None;
    assert_eq!(select_uniform_family(&b, v, &c).err(), Some(PageRefusal::ParentMismatch));
    // split C: text has no parent link
    let mut d = ok.clone();
    d.boxes[ti].parent = None;
    assert_eq!(select_uniform_family(&d, v, &c).err(), Some(PageRefusal::ParentMismatch));
}
