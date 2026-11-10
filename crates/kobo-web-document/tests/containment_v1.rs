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

#[test]
fn f2_color_fail_sibling_isolated() {
    let p = run(&ext(
        "<section>alpha</section><article>bravo <span style='color:#00f'>blue</span></article>",
    ));
    p.assert_derived_bool(false);
    assert!(p.h.unsupported, "F2: the pass bool still refuses (pre-containment equivalence)");
    let (a, b) = (p.idx("section"), p.idx("article"));
    assert_eq!(p.h.heights[a], Some(18), "F2 R1c: clean sibling measures");
    assert_eq!(p.h.refusals[a], HeightRefusal::NONE);
    assert_eq!(p.h.heights[b], None, "F2: color-fail block refuses its own height");
    assert_eq!(p.h.refusals[b], HeightRefusal::EMPTY_OR_UNMEASURED);
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
