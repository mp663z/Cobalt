//! PRIVATE, unadopted candidate: opt-in pinned font-family intake + family-bound painter gate.
//! Fake faces: these pin ADMISSION and REFUSAL only. Hash equality is a contract about supplied
//! bytes, not raster/shaping parity with Chrome; the pinned table is one host's measurement.
use kobo_web_document::{
    box_tree::BoxTree,
    css_text_page::{
        paint_direct_text_blocks, paint_direct_text_blocks_family_bound, paint_direct_text_blocks_family_bound_shaped, paint_direct_text_blocks_shaped,
        paint_direct_text_blocks_underline, paint_single_text_page, FamilyProfile, FontProvider, UnderlineProfile,
    },
    inline_lines::{FitRule, GlyphBitmap, RunShaper, ShapedCluster, TextShaping},
    parse_style_tree, parse_style_tree_with_pinned_font_family, parse_style_tree_with_supply, Limits, SupplyBudget,
};
const P: FamilyProfile = FamilyProfile::Chrome156HostLiberation2Sans;
const LIST: &str = "\"GDS Transport\", arial, sans-serif";
struct Face {
    reg: Option<String>,
    bold: Option<String>,
    glyph: fn(char, bool) -> Option<bool>,
}
fn good_glyph(_: char, _: bool) -> Option<bool> { Some(true) }
impl Face {
    fn ok() -> Self {
        Self { reg: Some(P.regular_sha256_hex().into()), bold: Some(P.bold_sha256_hex().into()), glyph: good_glyph }
    }
}
impl FontProvider for Face {
    fn face_sha256_hex(&self) -> Option<String> { self.reg.clone() }
    fn bold_face_sha256_hex(&self) -> Option<String> { self.bold.clone() }
    fn has_glyph(&self, c: char, bold: bool) -> Option<bool> { (self.glyph)(c, bold) }
    fn advance(&self, _: char, _: u32, _: bool) -> Option<u32> { Some(5) }
    fn line_height(&self, _: u32, _: bool) -> Option<u32> { Some(20) }
    fn baseline_offset(&self, _: u32, _: bool) -> Option<i32> { Some(15) }
    fn raster(&self, _: char, _: u32, _: bool) -> Option<GlyphBitmap> {
        Some(GlyphBitmap { left: 0, top: -1, width: 1, height: 1, coverage: vec![255] })
    }
}
struct Wide;
impl RunShaper for Wide {
    fn shape_run(&self, text: &str, _: u32, _: bool) -> Option<Vec<ShapedCluster>> {
        Some(text.chars().enumerate().map(|(i, _)| ShapedCluster { first_char: i as u32, char_len: 1, units: 9216 }).collect())
    }
}
struct Refuses;
impl RunShaper for Refuses {
    fn shape_run(&self, _: &str, _: u32, _: bool) -> Option<Vec<ShapedCluster>> { None }
}
struct Uniform;
impl RunShaper for Uniform {
    fn shape_run(&self, text: &str, _: u32, _: bool) -> Option<Vec<ShapedCluster>> {
        Some(text.chars().enumerate().map(|(i, _)| ShapedCluster { first_char: i as u32, char_len: 1, units: 5120 }).collect())
    }
}
fn doc(css: &str, body: &str) -> String {
    format!("<!doctype html><style>html,body,p{{margin:0}}p{{line-height:20px}}{css}</style>{body}")
}
fn pinned(css: &str, body: &str) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_pinned_font_family(doc(css, body).as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL)
}
fn tree(css: &str, body: &str) -> BoxTree {
    let s = pinned(css, body);
    assert!(!s.unsupported, "pinned intake must admit: {css}");
    BoxTree::from_style(&s)
}
fn bound(t: &BoxTree, f: &Face) -> bool { paint_direct_text_blocks_family_bound(t, 200, 100, f, P).is_ok() }

#[test]
fn pinned_lists_admit_and_paint_with_both_hashes_and_coverage() {
    for fam in [LIST, "sans-serif", " \"GDS Transport\" ,arial ,  sans-serif ", "\"GDS Transport\",arial,sans-serif",
        // Declaration layer lowercases values; family names/generic keyword are ASCII case-insensitive.
        "\"gds transport\", ARIAL, Sans-Serif", "SANS-SERIF"] {
        let t = tree(&format!("p{{font-family:{fam}}}"), "<p>ab cd</p>");
        assert!(bound(&t, &Face::ok()), "{fam}");
    }
    // bold text, NBSP via entity, inherited family from body.
    assert!(bound(&tree(&format!("body{{font-family:{LIST}}}"), "<p style='font-weight:700'>ab</p><p>a&nbsp;b</p>"), &Face::ok()));
}

#[test]
fn only_exact_lists_are_admitted_by_intake() {
    for bad in [
        "serif", "monospace", "arial", "\"GDS Transport\", arial", "arial, \"GDS Transport\", sans-serif",
        "'GDS Transport', arial, sans-serif", "\"GDS  Transport\", arial, sans-serif", "\"GDS Transport \", arial, sans-serif",
        "\"GDS Transport\", arial, sans-serif, serif", "inherit", "x\"GDS Transport\", arial, sans-serif",
        "\"GDS Transport\", arial, sans-serif, ", "sans-serif, serif",
        "\"GDS, Transport\", arial, sans-serif",
    ] {
        let s = pinned(&format!("p{{font-family:{bad}}}"), "<p>ab</p>");
        assert!(s.unsupported, "must refuse: {bad}");
    }
}

#[test]
fn default_entries_and_other_painters_stay_closed() {
    for fam in [LIST, "sans-serif"] {
        let html = doc(&format!("p{{font-family:{fam}}}"), "<p>ab</p>");
        assert!(parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT).unsupported, "default {fam}");
        assert!(parse_style_tree_with_supply(html.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL).unsupported);
        // Trees whose TEXT carries the family are refused by every non-family text painter, including
        // shaped and underline (via collect_inline_runs). Family on boxes with no text is not covered here.
        let t = tree(&format!("p{{font-family:{fam}}}"), "<p>ab</p>");
        assert!(paint_direct_text_blocks(&t, 200, 100, &Face::ok()).is_err(), "direct {fam}");
        assert!(paint_single_text_page(&t, 200, 100, &Face::ok()).is_err(), "single {fam}");
        let sh = TextShaping { shaper: &Uniform, fit: FitRule { unit: 1024, slack: 0 } };
        assert!(paint_direct_text_blocks_shaped(&t, 200, 100, &Face::ok(), &sh).is_err(), "shaped {fam}");
        let u = paint_direct_text_blocks_underline(&t, 200, 100, &Face::ok(), UnderlineProfile::ChromeLiberationSerif156);
        assert!(u.is_err(), "underline {fam}");
    }
    // Narrow scope, explicit: a family-only box with no text is NOT refused by the ordinary text painter.
    let nt = tree(&format!("div{{font-family:{LIST};width:20px;height:20px;background:red}}"), "<div></div><p>ab</p>");
    assert!(paint_direct_text_blocks(&nt, 200, 100, &Face::ok()).is_ok(), "family-only empty div, family-less text: ordinary painter unchanged");
    assert!(!bound(&nt, &Face::ok()), "...but the bound painter refuses (text leaf has no family)");
    // A tree without any family still paints through the ordinary entry and is refused by the bound one.
    let plain = BoxTree::from_style(&parse_style_tree(doc("", "<p>ab</p>").as_bytes(), &[], &Limits::DEFAULT));
    assert!(paint_direct_text_blocks(&plain, 200, 100, &Face::ok()).is_ok());
    assert!(!bound(&plain, &Face::ok()), "no family: refused by the bound painter");
}

#[test]
fn provider_contract_failures_refuse() {
    let t = tree(&format!("p{{font-family:{LIST}}}"), "<p>ab</p><p style='font-weight:700'>cd</p>");
    assert!(bound(&t, &Face::ok()));
    let mut f = Face::ok(); f.reg = None; assert!(!bound(&t, &f), "no regular hash");
    let mut f = Face::ok(); f.bold = None; assert!(!bound(&t, &f), "no bold hash (bold-only provider contract)");
    let mut f = Face::ok(); f.reg = Some("0".repeat(64)); assert!(!bound(&t, &f), "wrong regular hash");
    let mut f = Face::ok(); f.bold = Some("0".repeat(64)); assert!(!bound(&t, &f), "wrong bold hash");
    let mut f = Face::ok(); f.bold = f.reg.clone(); assert!(!bound(&t, &f), "bold hash = regular hash");
    // cmap coverage: Some(false), None, and per-face (bold-only miss) all refuse; raster/advance Some does not matter.
    let mut f = Face::ok(); f.glyph = |c, _| Some(c != 'b'); assert!(!bound(&t, &f), "regular glyph miss");
    let mut f = Face::ok(); f.glyph = |_, _| None; assert!(!bound(&t, &f), "coverage unknown");
    let mut f = Face::ok(); f.glyph = |c, bold| Some(!(bold && c == 'd')); assert!(!bound(&t, &f), "bold-face glyph miss");
    // Same miss on a char only the other face uses does not refuse.
    let mut f = Face::ok(); f.glyph = |c, bold| Some(!(bold && c == 'a')); assert!(bound(&t, &f), "bold miss on a regular-only char");
}

#[test]
fn style_weight_text_and_family_states_refuse() {
    let fam = format!("body{{font-family:{LIST}}}");
    for (body, why) in [
        ("<p style='font-weight:600'>ab</p>", "weight 600"),
        ("<p style='font-weight:800'>ab</p>", "weight 800"),
        ("<p style='font-style:italic'>ab</p>", "italic"),
        ("<p>caf&eacute;</p>", "non-ASCII decoded entity"),
        ("<p>a\u{200b}b</p>", "zero width space"),
        ("<p>a\u{2014}b</p>", "em dash"),
    ] {
        // Layer is explicit: intake refusal (Intake) or, if intake admits, the painter gate (Gate).
        // The gate's weight/ASCII checks are redundant with collect_inline_runs / the inner ASCII
        // rule (mutating them out does not change results), kept as defence in depth.
        let s = pinned(&fam, body);
        let layer = if s.unsupported { "Intake" } else if bound(&BoxTree::from_style(&s), &Face::ok()) { "PAINTED" } else { "Gate" };
        println!("{why}: {layer}");
        assert_ne!(layer, "PAINTED", "{why}");
    }
    // Family not on some text leaf (default family) refuses even beside a bound sibling.
    let t = tree(&format!(".b{{font-family:{LIST}}}"), "<p class=b>ab</p><p>cd</p>");
    assert!(!bound(&t, &Face::ok()), "mixed: unbound leaf");
    // A later non-pinned family on a descendant refuses at intake (whole tree unsupported).
    assert!(pinned(&format!("body{{font-family:{LIST}}} p{{font-family:serif}}"), "<p>ab</p>").unsupported);
}

fn bound_shaped(t: &BoxTree, f: &Face) -> bool {
    let sh = TextShaping { shaper: &Uniform, fit: FitRule { unit: 1024, slack: 0 } };
    paint_direct_text_blocks_family_bound_shaped(t, 200, 100, f, P, &sh).is_ok()
}

#[test]
fn family_bound_shaped_entry_applies_the_same_gate_first() {
    let t = tree(&format!("body{{font-family:{LIST}}}"), "<p>Hello world</p>");
    assert!(bound(&t, &Face::ok()) && bound_shaped(&t, &Face::ok()), "positive control");
    let mut wrong_reg = Face::ok();
    wrong_reg.reg = Some("00".into());
    let mut wrong_bold = Face::ok();
    wrong_bold.bold = None;
    let mut no_cov = Face::ok();
    no_cov.glyph = |c, _| Some(c != 'w');
    for f in [wrong_reg, wrong_bold, no_cov] {
        assert!(!bound(&t, &f) && !bound_shaped(&t, &f), "same refusal for both entries");
    }
    // non-ASCII text and weight 600 refuse in both
    let na = tree(&format!("body{{font-family:{LIST}}}"), "<p>caf\u{e9}</p>");
    assert!(!bound(&na, &Face::ok()) && !bound_shaped(&na, &Face::ok()));
    let w6 = tree(&format!("body{{font-family:{LIST};font-weight:600}}"), "<p>Hi</p>");
    assert!(!bound(&w6, &Face::ok()) && !bound_shaped(&w6, &Face::ok()));
    // a family-less tree is not made admissible by the shaped entry either
    let plain = BoxTree::from_style(&parse_style_tree(doc("", "<p>Hi</p>").as_bytes(), &[], &Limits::DEFAULT));
    assert!(!bound(&plain, &Face::ok()) && !bound_shaped(&plain, &Face::ok()));
}

fn px(t: &BoxTree, f: &Face, sh: Option<&dyn RunShaper>) -> Option<Vec<u8>> {
    use kobo_web_document::display_list::Rgb;
    let list = match sh {
        None => paint_direct_text_blocks_family_bound(t, 200, 100, f, P),
        Some(s) => {
            let ts = TextShaping { shaper: s, fit: FitRule { unit: 1024, slack: 0 } };
            paint_direct_text_blocks_family_bound_shaped(t, 200, 100, f, P, &ts)
        }
    }
    .ok()?;
    list.rasterize(200, 100, Rgb(255, 255, 255)).ok()
}

#[test]
fn family_bound_shaped_entry_is_strict_and_observably_shaped() {
    let t = tree(&format!("body{{font-family:{LIST}}}"), "<p>Hello world</p>");
    let f = Face::ok();
    let legacy = px(&t, &f, None).expect("legacy paints");
    // Wide shaper (9px per char vs face advance 5): success must differ from the legacy raster.
    let wide = px(&t, &f, Some(&Wide)).expect("shaped paints");
    assert_ne!(legacy, wide, "Ok must mean the shaped layout was used");
    // A shaper that rejects the run must REFUSE, not silently paint the legacy integer path.
    assert!(px(&t, &f, Some(&Refuses)).is_none());
    // NBSP: a shaper that rejects it refuses (the gate admits NBSP; strictness is the shaper's).
    struct AsciiOnly;
    impl RunShaper for AsciiOnly {
        fn shape_run(&self, text: &str, _: u32, _: bool) -> Option<Vec<ShapedCluster>> {
            if !text.chars().all(|c| ('\u{20}'..='\u{7e}').contains(&c)) { return None; }
            Some(text.chars().enumerate().map(|(i, _)| ShapedCluster { first_char: i as u32, char_len: 1, units: 9216 }).collect())
        }
    }
    let nb = tree(&format!("body{{font-family:{LIST}}}"), "<p>a&nbsp;b</p>");
    assert!(px(&nb, &f, None).is_some(), "plain family-bound admits NBSP");
    assert!(px(&nb, &f, Some(&AsciiOnly)).is_none(), "shaped entry must not fall back for NBSP");
}

// Stateful shaper: first call valid, later calls None. A probe-then-rebuild design would pass the
// probe and fall back to legacy on the rebuild; the single-build strict entry must either paint
// shaped from its one build or refuse, never paint the legacy raster.
struct FirstThenNone(std::cell::Cell<u32>);
impl RunShaper for FirstThenNone {
    fn shape_run(&self, text: &str, _: u32, _: bool) -> Option<Vec<ShapedCluster>> {
        let n = self.0.get();
        self.0.set(n + 1);
        if n > 0 { return None; }
        Some(text.chars().enumerate().map(|(i, _)| ShapedCluster { first_char: i as u32, char_len: 1, units: 9216 }).collect())
    }
}
#[test]
fn family_bound_shaped_entry_is_single_build_even_for_stateful_shapers() {
    let t = tree(&format!("body{{font-family:{LIST}}}"), "<p>Hello world</p>");
    let f = Face::ok();
    let legacy = px(&t, &f, None).unwrap();
    let st = FirstThenNone(std::cell::Cell::new(0));
    match px(&t, &f, Some(&st)) {
        None => {}
        Some(r) => assert_ne!(r, legacy, "Ok must be the shaped raster, never the legacy one"),
    }
}

#[test]
fn default_provider_has_no_face_catalog() {
    // FL51 increment 2 seam: an existing provider that does not opt in keeps today's behavior (no catalog).
    let f = Face::ok();
    assert!(f.face_catalog().is_none());
}
