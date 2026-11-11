//! External (public-API-only) enrollment test for the :target lane's caller
//! guarantees. `TargetState` is re-exported at the crate root so callers
//! outside the crate can assert the accepted SnapshotState target contracts.
//! Standards-mode fixtures (doctype) so cascade/paint claims are not
//! quirks-mode observations.
use kobo_web_document::{
    parse_style_tree, parse_style_tree_with_state, Limits, SnapshotState, SupplyBudget,
    TargetState,
};

const HTML: &str = "<!DOCTYPE html><html><head><style>a:target{background-color:#ff0000} a{background-color:#0000ff}</style></head><body><a id=t href=x>x</a></body></html>";
const RED: u32 = 0xff0000;
const BLUE: u32 = 0x0000ff;

fn anchor_bg(snapshot: &SnapshotState) -> Option<u32> {
    let tree = parse_style_tree_with_state(
        HTML.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        snapshot,
    );
    tree.nodes
        .iter()
        .find(|n| n.tag == "a")
        .and_then(|n| n.style.background_color)
}

/// Fragment guarantee asserted and matching: the :target rule enrolls, wins
/// on specificity (0,1,1) over the later plain-type rule (0,0,1), and paints.
#[test]
fn fragment_asserted_matching_paints_target_rule() {
    let snap = SnapshotState {
        history: None,
        input: None,
        focus: None,
        target: Some(TargetState::Fragment("t".to_string())),
    };
    assert_eq!(anchor_bg(&snap), Some(RED));
}

/// FragmentFreeCapture enrolls (the guarantee IS asserted) but matches no
/// target elements: the :target rule simply does not match and the plain
/// rule paints. This is distinct from the no-guarantee path: enrollment
/// happens, the match set is empty by contract.
#[test]
fn fragment_free_capture_enrolls_but_matches_nothing() {
    let snap = SnapshotState {
        history: None,
        input: None,
        focus: None,
        target: Some(TargetState::FragmentFreeCapture),
    };
    assert_eq!(anchor_bg(&snap), Some(BLUE));
}

/// No target guarantee: the :target rule fails closed at enrollment exactly
/// like an unparseable selector; only the plain rule remains.
#[test]
fn no_guarantee_fails_closed() {
    let snap = SnapshotState {
        history: None,
        input: None,
        focus: None,
        target: None,
    };
    assert_eq!(anchor_bg(&snap), Some(BLUE));
}

/// Default-intake identity: the default entry (no snapshot anywhere) is
/// unaffected by state API reachability, and an all-None snapshot on the
/// state entry agrees with it on a state-free sheet.
#[test]
fn default_identity() {
    let default_tree = parse_style_tree(HTML.as_bytes(), &[], &Limits::DEFAULT);
    let snap = SnapshotState {
        history: None,
        input: None,
        focus: None,
        target: None,
    };
    let state_tree = parse_style_tree_with_state(
        HTML.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
        &snap,
    );
    let bg = |t: &kobo_web_document::style_tree::StyleTree| {
        t.nodes
            .iter()
            .find(|n| n.tag == "a")
            .and_then(|n| n.style.background_color)
    };
    assert_eq!(bg(&default_tree), bg(&state_tree));
}
