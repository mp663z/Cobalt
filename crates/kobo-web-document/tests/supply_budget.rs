//! H1 supply budget: CANONICAL equals the pre-budget literals and entries are
//! byte-identical; candidate budgets change only what they name.
use kobo_web_document::style_diagnostics::RefusalReason;
use kobo_web_document::{parse_style_tree, parse_style_tree_with_supply, Limits, SupplyBudget};

fn sheet(len: usize) -> Vec<u8> {
    let mut s = b"p{color:#123456}/*".to_vec();
    s.resize(len - 2, b'x');
    s.extend_from_slice(b"*/");
    s
}
const LINKS3: &str = "<link rel=stylesheet href=/a><link rel=stylesheet href=/b><link rel=stylesheet href=/c><p>T</p>";
const LINKS2: &str = "<link rel=stylesheet href=/a><link rel=stylesheet href=/b><p>T</p>";

fn run(
    html: &str,
    sheets: &[Vec<u8>],
    b: SupplyBudget,
) -> kobo_web_document::style_tree::StyleTree {
    parse_style_tree_with_supply(html.as_bytes(), sheets, &Limits::DEFAULT, None, b)
}

#[test]
fn canonical_equals_the_pre_budget_literals() {
    assert_eq!(SupplyBudget::CANONICAL.max_linked_sheets, 2);
    assert_eq!(SupplyBudget::CANONICAL.max_sheet_bytes, 256 * 1024);
    assert_eq!(SupplyBudget::CANONICAL.max_aggregate_bytes, 2 * 256 * 1024);
}

#[test]
fn canonical_entry_matches_the_existing_entry_for_every_gate() {
    let big = sheet(256 * 1024 + 1);
    let cases: Vec<(&str, Vec<Vec<u8>>)> = vec![
        (LINKS2, vec![sheet(100), sheet(100)]),
        (LINKS3, vec![sheet(100), sheet(100), sheet(100)]),
        (LINKS2, vec![sheet(100)]),
        (LINKS2, vec![big.clone(), sheet(10)]),
        (LINKS2, vec![sheet(256 * 1024), sheet(256 * 1024)]),
        (LINKS2, vec![vec![0xff, 0xfe], sheet(10)]),
    ];
    for (html, sheets) in cases {
        let a = parse_style_tree(html.as_bytes(), &sheets, &Limits::DEFAULT);
        let b = run(html, &sheets, SupplyBudget::CANONICAL);
        assert_eq!(a.unsupported, b.unsupported);
        assert_eq!(a.host_sheets_claimed, b.host_sheets_claimed);
        for r in RefusalReason::ALL {
            assert_eq!(a.diagnostics.count(r), b.diagnostics.count(r), "{r:?}");
        }
    }
}

#[test]
fn aggregate_gate_never_fires_under_canonical_even_at_both_sheets_full() {
    let t = run(
        LINKS2,
        &[sheet(256 * 1024), sheet(256 * 1024)],
        SupplyBudget::CANONICAL,
    );
    assert_eq!(t.diagnostics.count(RefusalReason::SupplyAggregate), 0);
    assert!(!t.unsupported);
}

#[test]
fn third_sheet_needs_the_sheet_count_budget_each_arm() {
    let s = vec![sheet(100), sheet(100), sheet(100)];
    let canon = run(LINKS3, &s, SupplyBudget::CANONICAL);
    assert!(canon.unsupported);
    assert!(canon.diagnostics.count(RefusalReason::StylesheetCount) >= 1);
    let cand = SupplyBudget {
        max_linked_sheets: 3,
        ..SupplyBudget::CANONICAL
    };
    let t = run(LINKS3, &s, cand);
    assert!(!t.unsupported);
    assert_eq!(t.host_sheets_claimed, Some(3));
    // data: arm: three retained data: sheets
    let d =
        "<link rel=stylesheet href='data:text/css;base64,cHtjb2xvcjpyZWR9'>".repeat(3) + "<p>T</p>";
    let canon_d = run(&d, &[], SupplyBudget::CANONICAL);
    assert!(canon_d.unsupported);
    assert_eq!(canon_d.diagnostics.count(RefusalReason::StylesheetCount), 1);
    assert!(!run(&d, &[], cand).unsupported);
}

#[test]
fn per_sheet_budget_raises_only_when_named() {
    let big = vec![sheet(300 * 1024)];
    let html = "<link rel=stylesheet href=/a><p>T</p>";
    let canon = run(html, &big, SupplyBudget::CANONICAL);
    assert!(canon.unsupported);
    assert_eq!(canon.diagnostics.count(RefusalReason::CssScanBudget), 1);
    let cand = SupplyBudget {
        max_sheet_bytes: 512 * 1024,
        max_aggregate_bytes: 1024 * 1024,
        ..SupplyBudget::CANONICAL
    };
    let t = run(html, &big, cand);
    assert!(!t.unsupported);
    assert_eq!(t.diagnostics.count(RefusalReason::CssScanBudget), 0);
}

#[test]
fn aggregate_gate_is_new_ordered_after_per_sheet_and_in_document_order() {
    let cand = SupplyBudget {
        max_aggregate_bytes: 150,
        max_sheet_bytes: 256 * 1024,
        max_linked_sheets: 2,
    };
    let t = run(LINKS2, &[sheet(100), sheet(100)], cand);
    assert!(t.unsupported);
    assert_eq!(
        t.diagnostics.count(RefusalReason::SupplyAggregate),
        1,
        "second sheet crosses"
    );
    let ok = SupplyBudget {
        max_aggregate_bytes: 200,
        ..cand
    };
    let t = run(LINKS2, &[sheet(100), sheet(100)], ok);
    assert!(!t.unsupported);
    assert_eq!(t.diagnostics.count(RefusalReason::SupplyAggregate), 0);
    // per-sheet refusal wins and does not also record the aggregate gate
    let tight = SupplyBudget {
        max_sheet_bytes: 50,
        max_aggregate_bytes: 10,
        max_linked_sheets: 2,
    };
    let t = run(LINKS2, &[sheet(100), sheet(100)], tight);
    assert!(t.diagnostics.count(RefusalReason::CssScanBudget) >= 1);
    assert_eq!(t.diagnostics.count(RefusalReason::SupplyAggregate), 0);
}

#[test]
fn data_arm_per_sheet_budget_is_the_candidate_value_not_a_constant() {
    // 128-byte decoded data: sheet: refused at max_sheet_bytes=64, admitted at 128.
    let html = "<link rel=stylesheet href='data:text/css;base64,cHtjb2xvcjojMTIzNDU2fS8qeHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4Ki8='><p>T</p>";
    let at = |n: usize| SupplyBudget {
        max_sheet_bytes: n,
        ..SupplyBudget::CANONICAL
    };
    let canon = run(html, &[], SupplyBudget::CANONICAL);
    assert!(!canon.unsupported, "128 B is under the canonical cap");
    let refused = run(html, &[], at(64));
    assert!(refused.unsupported, "64 B candidate cap must refuse 128 B");
    let admitted = run(html, &[], at(128));
    assert!(!admitted.unsupported, "128 B candidate cap admits 128 B");
    let off_by_one = run(html, &[], at(127));
    assert!(off_by_one.unsupported, "127 B cap refuses 128 B");
}

#[test]
fn data_arm_refusal_comes_from_the_decoder_before_the_aggregate_gate() {
    // Distinguishes the decoder's own per-sheet cap from the later scan cap:
    // a decoder-refused 128 B sheet adds nothing to the aggregate, so a
    // following 64 B sheet fits an aggregate of 100 and SupplyAggregate never
    // fires. If the decoder ignored the candidate cap, the 128 B sheet would
    // reach the aggregate gate and refuse there.
    let html = "<link rel=stylesheet href='data:text/css;base64,cHtjb2xvcjojMTIzNDU2fS8qeHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4Ki8='>\
                <link rel=stylesheet href='data:text/css;base64,cHtjb2xvcjojMTIzNDU2fS8qeHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHh4eHgqLw=='><p>T</p>";
    let b = SupplyBudget {
        max_sheet_bytes: 64,
        max_aggregate_bytes: 100,
        ..SupplyBudget::CANONICAL
    };
    let t = run(html, &[], b);
    assert!(t.unsupported);
    assert_eq!(t.diagnostics.count(RefusalReason::CssScanBudget), 1);
    assert_eq!(t.diagnostics.count(RefusalReason::SupplyAggregate), 0);
}
