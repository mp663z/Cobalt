//! PRIVATE containment v1 (default-off, no production enablement): executable
//! F1-F6 fixtures frozen in TEST-MATRIX.md (delta7, reviewer-PASSED) BEFORE
//! the code existed. Each assertion below pins the frozen outcome; the
//! propagation rules are delta5's R1a/R1c/R3 and the SQ1/SQ3 reviewer
//! dispositions. Metrics are synthetic exact providers (8px advance, 18px
//! line); no painter involvement, E2 confinement untouched.
use kobo_web_document::box_tree::{BoxTree, HeightRefusal, UsedHeightPass};
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors, style_tree::StyleTree,
    Limits, SupplyBudget,
};

fn ext(body: &str) -> StyleTree {
    let h = format!("<!doctype html><style>body{{margin:0}}div,section,article{{margin:0;padding:0}}</style><div>{body}</div>");
    parse_style_tree_with_inline_sheets_and_selectors(
        h.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    )
}

struct Pass {
    bt: BoxTree,
    h: UsedHeightPass,
    tags: Vec<String>,
}

fn run(t: &StyleTree) -> Pass {
    let bt = BoxTree::from_style(t);
    let h = UsedHeightPass::from_boxes_with_direct_text(&bt, 800, 600, |_, _, _| Some(8), |_| {
        Some(18)
    });
    let tags = bt
        .boxes
        .iter()
        .map(|b| b.source.map(|s| t.nodes[s].tag.to_string()).unwrap_or_default())
        .collect();
    Pass { bt, h, tags }
}

impl Pass {
    fn idx(&self, tag: &str) -> usize {
        self.tags
            .iter()
            .position(|t| t == tag)
            .unwrap_or_else(|| panic!("no box with tag {tag}"))
    }
    /// Equivalence invariant (pinned by test): the imperatively maintained
    /// whole-pass bool equals seed (tree-global kinds only) OR any per-node
    /// refusal bit. The bool is not derived at read time; this test proves
    /// the imperative maintenance never diverges from the invariant.
    fn assert_derived_bool(&self, seed: bool) {
        let any = self.h.refusals.iter().any(|r| !r.is_empty());
        assert_eq!(
            self.h.unsupported, seed || any,
            "unsupported must derive as seed || any refusal bit"
        );
    }
}

#[test]
fn f1_clean_baseline_no_flags() {
    let p = run(&ext("<section>alpha</section><article>bravo</article>"));
    p.assert_derived_bool(false);
    assert!(!p.h.unsupported, "F1: clean tree must not refuse");
    assert!(p.h.refusals.iter().all(|r| r.is_empty()), "F1: no refusal bits");
    assert_eq!(p.h.heights[p.idx("section")], Some(18));
    assert_eq!(p.h.heights[p.idx("article")], Some(18));
}

// SUPERSEDED v2 (delta10, Part E E-C1): per-run colour is now SUPPORTED, so
// the F2-shaped fixture no longer refuses. Pre-colour frozen expectation
// (delta7): bool true, article None + EMPTY_OR_UNMEASURED, section Some(18).
// Preserved in git history at 3e6cb22; refusal counts may legitimately drop
// where support is correctly added.
#[test]
fn f2_v2_colour_supported_sibling_measures() {
    let p = run(&ext(
        "<section>alpha</section><article>bravo <span style='color:#00f'>blue</span></article>",
    ));
    p.assert_derived_bool(false);
    assert!(!p.h.unsupported, "F2v2: colour difference alone is supported");
    assert!(p.h.refusals.iter().all(|r| r.is_empty()));
    let (a, b) = (p.idx("section"), p.idx("article"));
    assert_eq!(p.h.heights[a], Some(18));
    assert_eq!(p.h.heights[b], Some(18));
}

// E-C2 positive at the UsedHeightPass level (true F5 shape with the clean run
// in bold, isolating colour from other refusal kinds).
#[test]
fn f5_v2_colour_supported_owner_measures() {
    let p = run(&ext(
        "<section>alpha</section><article><b>clean</b> <span style='color:#00f'>fail</span></article>",
    ));
    assert!(!p.h.unsupported);
    assert!(p.h.refusals.iter().all(|r| r.is_empty()));
    assert_eq!(p.h.heights[p.idx("section")], Some(18));
    assert_eq!(p.h.heights[p.idx("article")], Some(18));
}

#[test]
fn f3_no_inline_owner_sibling_isolated() {
    let p = run(&ext(
        "<section>alpha</section><article><div>block</div>loose</article>",
    ));
    p.assert_derived_bool(false);
    let (a, b) = (p.idx("section"), p.idx("article"));
    assert_eq!(p.h.heights[a], Some(18), "F3 R1c: clean sibling measures");
    assert_eq!(p.h.refusals[a], HeightRefusal::NONE);
    assert_eq!(p.h.heights[b], None);
    assert_eq!(p.h.refusals[b], HeightRefusal::COMPOSITION_UNRESOLVED);
    let loose = p
        .bt
        .boxes
        .iter()
        .position(|bx| bx.text.as_deref() == Some("loose"))
        .expect("loose text node");
    assert_eq!(p.h.refusals[loose], HeightRefusal::NO_INLINE_OWNER);
}

#[test]
fn f4_inline_block_stays_whole_pass() {
    let p = run(&ext(
        "<section>alpha</section><article><span style='display:inline-block'>ib</span></article>",
    ));
    // SQ3 (frozen): InlineBlock remains a whole-pass refusal in v1; the bool
    // refuses the tree even though the IB sits in one subtree.
    assert!(p.h.unsupported, "F4: whole-tree refuse preserved for InlineBlock");
    p.assert_derived_bool(true); // WidthPass InlineBlock poison reaches the seed.
    // Composition still runs (the deleted early-return): the clean sibling's
    // height resolves, invisibly to consumers that gate on the bool first.
    assert_eq!(p.h.heights[p.idx("section")], Some(18));
    assert_eq!(p.h.refusals[p.idx("section")], HeightRefusal::NONE);
}

// NOTE (delta10): this fixture's 'clean' run is <i> = ITALIC, itself an E-C3
// refusal kind. Post-delta10 it pins ITALIC poison crossing to the owner
// block, not colour. The F5-shaped COLOUR case is f5_v2_colour_supported.
#[test]
fn f5_inline_poison_crosses_to_owner_not_sibling() {
    let p = run(&ext(
        "<section>alpha</section><article><i>clean</i> <span style='color:#00f'>fail</span></article>",
    ));
    p.assert_derived_bool(false);
    let (a, c) = (p.idx("section"), p.idx("article"));
    // R3: the color-fail inline poisons its whole OWNER BLOCK (part-clean does
    // not rescue it) and never crosses to the sibling.
    assert_eq!(p.h.heights[c], None, "F5: owner block refuses whole");
    assert_eq!(p.h.refusals[c], HeightRefusal::EMPTY_OR_UNMEASURED);
    assert_eq!(p.h.heights[a], Some(18), "F5: sibling measures");
    assert_eq!(p.h.refusals[a], HeightRefusal::NONE);
}

#[test]
fn f6_missing_stylesheet_stays_tree_wide() {
    let t = parse_style_tree(
        "<!doctype html><link rel=stylesheet href=/a><style>body{margin:0}div,section,article{margin:0;padding:0}</style><div><section>alpha</section><article>bravo</article></div>"
            .as_bytes(),
        &[],
        &Limits::DEFAULT,
    );
    assert!(t.unsupported, "F6 fixture must carry the MissingStylesheet seed");
    let p = run(&t);
    // SQ1 (frozen): cascade/parse-global kinds refuse whole-tree forever;
    // they have no per-node bit in the type, so this can never silently
    // become subtree-containment.
    assert!(p.h.unsupported, "F6: whole-tree refuse preserved for tree-global kinds");
    assert_eq!(p.h.heights[p.idx("section")], None);
    assert_eq!(p.h.heights[p.idx("article")], None);
}

#[test]
fn height_refusal_bit_ops() {
    let mut r = HeightRefusal::NO_INLINE_OWNER;
    r |= HeightRefusal::NO_METRICS_PROVIDER;
    assert_eq!(r.bits(), 0b11);
    assert!(!r.is_empty());
    assert_eq!(r | HeightRefusal::NONE, r);
    assert!(HeightRefusal::NONE.is_empty());
}


// ---- delta9: VerticalPass per-node consumption (frozen TEST-MATRIX.md Part D) ----
use kobo_web_document::box_tree::{VerticalPass, VerticalRefusal};

struct VPass {
    bt: BoxTree,
    v: VerticalPass,
    tags: Vec<String>,
}

fn vrun(t: &StyleTree) -> VPass {
    let bt = BoxTree::from_style(t);
    let v = VerticalPass::from_boxes_with_direct_text(&bt, 800, 600, |_, _, _| Some(8), |_| {
        Some(18)
    });
    let tags = bt
        .boxes
        .iter()
        .map(|b| b.source.map(|s| t.nodes[s].tag.to_string()).unwrap_or_default())
        .collect();
    VPass { bt, v, tags }
}

impl VPass {
    fn idx(&self, tag: &str) -> usize {
        self.tags.iter().position(|t| t == tag).expect("tag")
    }
    fn text_idx(&self, txt: &str) -> usize {
        self.bt
            .boxes
            .iter()
            .position(|b| b.text.as_deref() == Some(txt))
            .expect("text")
    }
    fn assert_all_y_none(&self) {
        assert!(
            self.v.content_y.iter().all(Option::is_none),
            "refused tree: fill-gate must keep every content_y None"
        );
    }
}

#[test]
fn v1_clean_composes_no_flags() {
    let p = vrun(&ext("<section>alpha</section><article>bravo</article>"));
    assert!(!p.v.unsupported);
    assert!(p.v.refusals.iter().all(|r| r.is_empty()));
    assert_eq!(p.v.content_y[p.idx("section")], Some(0));
    assert_eq!(p.v.content_y[p.idx("article")], Some(18));
}

// SUPERSEDED v2 (delta10, Part E E-C1): colour supported -> the whole tree
// composes. Pre-colour frozen expectation (Part D): bool true, all y None,
// article+ancestors NO_HEIGHT, span/texts KIND_NOT_ADMITTED. In git history.
#[test]
fn v2_v2_colour_supported_composes() {
    let p = vrun(&ext(
        "<section>alpha</section><article>bravo <span style='color:#00f'>blue</span></article>",
    ));
    assert!(!p.v.unsupported);
    assert!(p.v.refusals.iter().all(|r| r.is_empty()));
    assert_eq!(p.v.content_y[p.idx("section")], Some(0));
    assert_eq!(p.v.content_y[p.idx("article")], Some(18));
}

#[test]
fn v5_v2_colour_supported_vertical_composes() {
    let p = vrun(&ext(
        "<section>alpha</section><article><b>clean</b> <span style='color:#00f'>fail</span></article>",
    ));
    assert!(!p.v.unsupported);
    assert!(p.v.refusals.iter().all(|r| r.is_empty()));
    assert_eq!(p.v.content_y[p.idx("article")], Some(18));
}

#[test]
fn v3_no_inline_owner_attribution() {
    let p = vrun(&ext("<section>alpha</section><article><div>block</div>loose</article>"));
    assert!(p.v.unsupported);
    p.assert_all_y_none();
    assert_eq!(p.v.refusals[p.idx("section")], VerticalRefusal::NONE);
    assert_eq!(p.v.refusals[p.text_idx("block")], VerticalRefusal::NONE);
    assert_eq!(p.v.refusals[p.idx("article")], VerticalRefusal::NO_HEIGHT);
    assert_eq!(p.v.refusals[p.text_idx("loose")], VerticalRefusal::KIND_NOT_ADMITTED);
}

#[test]
fn v4_inline_block_whole_pass_attribution() {
    let p = vrun(&ext(
        "<section>alpha</section><article><span style='display:inline-block'>ib</span></article>",
    ));
    assert!(p.v.unsupported);
    p.assert_all_y_none();
    assert_eq!(p.v.refusals[p.idx("section")], VerticalRefusal::NONE);
    assert_eq!(p.v.refusals[p.idx("span")], VerticalRefusal::KIND_NOT_ADMITTED);
    assert_eq!(p.v.refusals[p.idx("article")], VerticalRefusal::NO_HEIGHT);
}

// NOTE (delta10): '<i>clean</i>' is italic - this pins italic attribution,
// not colour. The colour case is v5_v2_colour_supported_vertical_composes.
#[test]
fn v5_owner_poison_attribution() {
    let p = vrun(&ext(
        "<section>alpha</section><article><i>clean</i> <span style='color:#00f'>fail</span></article>",
    ));
    assert!(p.v.unsupported);
    p.assert_all_y_none();
    assert_eq!(p.v.refusals[p.idx("section")], VerticalRefusal::NONE);
    assert_eq!(p.v.refusals[p.idx("article")], VerticalRefusal::NO_HEIGHT);
    assert_eq!(p.v.refusals[p.text_idx("clean")], VerticalRefusal::KIND_NOT_ADMITTED);
}

#[test]
fn v6_tree_seed_no_attribution() {
    let t = parse_style_tree(
        "<!doctype html><link rel=stylesheet href=/a><style>body{margin:0}div,section,article{margin:0;padding:0}</style><div><section>alpha</section><article>bravo</article></div>"
            .as_bytes(),
        &[],
        &Limits::DEFAULT,
    );
    let p = vrun(&t);
    // SQ1: tree-global kinds keep the early-return; nothing composes and NO
    // per-node attribution is recorded at all.
    assert!(p.v.unsupported);
    p.assert_all_y_none();
    assert!(p.v.refusals.iter().all(|r| r.is_empty()));
}
