//! External (public-API-only) enrollment test for the :target lane's caller
//! guarantees. `TargetState` is re-exported at the crate root so callers
//! outside the crate can assert the accepted SnapshotState target contracts.
//! Standards-mode fixtures (doctype) so cascade/paint claims are not
//! quirks-mode observations.
//!
//! Amendment 1 (review gap close): FragmentFreeCapture and the no-guarantee
//! path are asserted DISTINCT through the public surface - enrollment
//! (unsupported == false, no selector refusal) vs fail-closed at enrollment
//! (unsupported == true, exactly one CssSelector refusal). The identity test
//! compares the full public retained tree state (all nodes, all computed
//! properties, flags, diagnostics counts), not one color of one node.
use kobo_web_document::style_diagnostics::RefusalReason;
use kobo_web_document::style_tree::StyleTree;
use kobo_web_document::{
    parse_style_tree_with_state, Limits, SnapshotState, SupplyBudget,
    TargetState,
};

const HTML: &str = "<!DOCTYPE html><html><head><style>a:target{background-color:#ff0000} a{background-color:#0000ff}</style></head><body><a id=t href=x>x</a></body></html>";
const RED: u32 = 0xff0000;
const BLUE: u32 = 0x0000ff;

fn tree_with(snapshot: &SnapshotState) -> StyleTree {
    parse_style_tree_with_state(
        HTML.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        snapshot,
    )
}

fn anchor_bg(tree: &StyleTree) -> Option<u32> {
    tree.nodes
        .iter()
        .find(|n| n.tag == "a")
        .and_then(|n| n.style.background_color)
}

fn snap(target: Option<TargetState>) -> SnapshotState {
    SnapshotState {
        history: None,
        input: None,
        focus: None,
        target,
    }
}

/// Fragment guarantee asserted and matching: the :target rule enrolls
/// (no refusal), matches, and wins on specificity (0,1,1) over the later
/// plain-type rule (0,0,1) - RED proves match AND cascade rank, not just
/// enrollment.
#[test]
fn fragment_asserted_matching_paints_target_rule() {
    let tree = tree_with(&snap(Some(TargetState::Fragment("t".to_string()))));
    assert_eq!(anchor_bg(&tree), Some(RED));
    assert!(!tree.unsupported, "enrolled rule must not flag unsupported");
    assert_eq!(tree.diagnostics.count(RefusalReason::CssSelector), 0);
}

/// FragmentFreeCapture is a VALID assertion: the :target rule ENROLLS
/// (unsupported stays false, zero selector refusals) but the contract
/// guarantees no target elements, so the match set is empty and the plain
/// rule paints. Provably distinct from the no-guarantee path below.
#[test]
fn fragment_free_capture_enrolls_but_matches_nothing() {
    let tree = tree_with(&snap(Some(TargetState::FragmentFreeCapture)));
    assert_eq!(anchor_bg(&tree), Some(BLUE));
    assert!(
        !tree.unsupported,
        "enrolled-but-unmatched must not flag unsupported"
    );
    assert_eq!(
        tree.diagnostics.count(RefusalReason::CssSelector),
        0,
        "enrolled rule records no selector refusal"
    );
}

/// No target guarantee: the :target rule fails closed at enrollment exactly
/// like an unparseable selector - the tree flags unsupported and records
/// exactly one selector refusal; only the plain rule remains.
#[test]
fn no_guarantee_fails_closed() {
    let tree = tree_with(&snap(None));
    assert_eq!(anchor_bg(&tree), Some(BLUE));
    assert!(tree.unsupported, "refused-at-enrollment must flag unsupported");
    assert_eq!(
        tree.diagnostics.count(RefusalReason::CssSelector),
        1,
        "exactly the :target rule is refused"
    );
}

/// Identity: the ext entry without a snapshot
/// (`parse_style_tree_with_inline_sheets_and_selectors`) and the state entry
/// under an all-None snapshot agree on the FULL public retained state -
/// every node's full computed style, the tree flags and every diagnostics
/// count, not one property of one node. Adding an empty guarantee set must
/// change nothing. (Cross-entry agreement with the DEFAULT entry is not the
/// claim: the ext entry has candidate-local UA link styling the default
/// entry lacks. Default-intake identity across this change is evidenced by
/// the corpus 5-of-5 pre/post dump byte comparison.)
#[test]
fn all_none_snapshot_full_tree_identity_with_no_snapshot() {
    let no_snapshot_tree = kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(
        HTML.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    );
    let state_tree = tree_with(&snap(None));
    assert_eq!(no_snapshot_tree.nodes, state_tree.nodes);
    assert_eq!(no_snapshot_tree.unsupported, state_tree.unsupported);
    assert_eq!(no_snapshot_tree.truncated, state_tree.truncated);
    assert_eq!(no_snapshot_tree.quirks, state_tree.quirks);
    assert_eq!(no_snapshot_tree.ext, state_tree.ext);
    for reason in RefusalReason::ALL {
        assert_eq!(
            no_snapshot_tree.diagnostics.count(reason),
            state_tree.diagnostics.count(reason),
            "diagnostics count differs for {reason:?}"
        );
    }
}
