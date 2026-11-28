//! Narrow symbolic checks over the actual empty-history implementation.
//! No claim about populated histories, URL parsing or the whole browser.
use super::{Go, History};

#[kani::proof]
fn empty_history_ignores_arbitrary_place_and_rollback() {
    let mut history = History::new();
    let page: usize = kani::any();
    let place: Option<usize> = kani::any();
    let rollback: usize = kani::any();
    history.set_position(page, place);
    history.return_to(rollback);
    assert!(history.current().is_none());
    assert!(history.entries().is_empty());
    assert_eq!(history.position(), 0);
    assert!(!history.can_go_back());
    assert!(!history.can_go_forward());
    assert_eq!(history.back(), Go::Stay);
    assert_eq!(history.forward(), Go::Stay);
}

// ---- Populated-history harnesses, RELATIVE TO THREE STUBS. NOT a proof about URL handling. ----
// Stubs (all safe code; no unsafe, no lint changes). Proofs are RELATIVE TO THEM:
// Stubs 1-4, the private helpers of `Url::parse` (clean, parse_authority, normalize_path, split_tail):
//   identity-like, exact only for the one literal used, "http://b" (no whitespace, credentials, port,
//   query or fragment; one-letter host; empty path tail). The REAL `Url::parse` still runs on top of
//   them, so the Url is a genuine parsed value, but URL cleaning/authority/path/query handling is
//   NOT verified here.
// Stubs 5 and 6, `Url::same_document` and `<Url as PartialEq>::eq`: return an ARBITRARY bool. That
//   over-approximates: every claim below holds for any answer these could give. These harnesses say
//   nothing about WHICH Go variant (Stay/Page/Fragment/Load) is chosen, only about History's cursor,
//   truncation and drain logic.
use kobo_web_document::url::{Scheme, UrlError};
use kobo_web_document::Url;
fn stub_clean(input: &str) -> Result<String, UrlError> {
    Ok(input.to_owned())
}
fn stub_authority(authority: &str, _scheme: Scheme) -> Result<(String, Option<u16>), UrlError> {
    Ok((authority.to_owned(), None))
}
fn stub_norm(path: &str) -> String {
    path.to_owned()
}
fn stub_tail(tail: &str) -> (String, Option<String>, Option<String>) {
    (tail.to_owned(), None, None)
}
fn stub_same_document(_a: &Url, _b: &Url) -> bool {
    kani::any()
}
fn stub_url_eq(_a: &Url, _b: &Url) -> bool {
    kani::any()
}
use super::{Entry, MAX_HISTORY};
use std::cmp::PartialEq;

fn the_url() -> Url {
    Url::parse("http://b").unwrap()
}
fn entry(url: &Url) -> Entry {
    Entry { url: url.clone(), page: kani::any(), place: kani::any() }
}
/// Any two-entry history with an in-range cursor.
fn history2_at(url: &Url, current: usize) -> History {
    // Spare capacity so the push in navigate does not model a reallocation.
    let mut entries = Vec::with_capacity(4);
    entries.push(entry(url));
    entries.push(entry(url));
    History { entries, current }
}
fn history2() -> History {
    let url = the_url();
    let current: usize = kani::any();
    kani::assume(current < 2);
    history2_at(&url, current)
}
fn well_formed(h: &History) -> bool {
    h.entries.len() <= MAX_HISTORY && (h.entries.is_empty() || h.current < h.entries.len())
}

#[kani::proof]
#[kani::stub(kobo_web_document::url::clean, stub_clean)]
#[kani::stub(kobo_web_document::url::parse_authority, stub_authority)]
#[kani::stub(kobo_web_document::url::normalize_path, stub_norm)]
#[kani::stub(kobo_web_document::url::split_tail, stub_tail)]
#[kani::stub(kobo_web_document::url::Url::same_document, stub_same_document)]
#[kani::unwind(10)]
fn populated_back_keeps_cursor_in_range() {
    let mut h = history2();
    let before = h.current;
    assert_eq!(h.can_go_back(), before > 0);
    let stayed = matches!(h.back(), Go::Stay);
    assert!(well_formed(&h));
    assert_eq!(h.entries.len(), 2);
    assert_eq!(stayed, before == 0);
    assert_eq!(h.current, before - usize::from(!stayed));
}

#[kani::proof]
#[kani::stub(kobo_web_document::url::clean, stub_clean)]
#[kani::stub(kobo_web_document::url::parse_authority, stub_authority)]
#[kani::stub(kobo_web_document::url::normalize_path, stub_norm)]
#[kani::stub(kobo_web_document::url::split_tail, stub_tail)]
#[kani::stub(kobo_web_document::url::Url::same_document, stub_same_document)]
#[kani::unwind(10)]
fn populated_forward_keeps_cursor_in_range() {
    let mut h = history2();
    let before = h.current;
    assert_eq!(h.can_go_forward(), before < 1);
    let stayed = matches!(h.forward(), Go::Stay);
    assert!(well_formed(&h));
    assert_eq!(h.entries.len(), 2);
    assert_eq!(stayed, before == 1);
    assert_eq!(h.current, before + usize::from(!stayed));
}

#[kani::proof]
#[kani::stub(kobo_web_document::url::clean, stub_clean)]
#[kani::stub(kobo_web_document::url::parse_authority, stub_authority)]
#[kani::stub(kobo_web_document::url::normalize_path, stub_norm)]
#[kani::stub(kobo_web_document::url::split_tail, stub_tail)]
#[kani::unwind(10)]
fn populated_return_to_is_ignored_out_of_range() {
    let mut h = history2();
    let before = h.current;
    let to: usize = kani::any();
    h.return_to(to);
    assert!(well_formed(&h));
    if to < 2 { assert_eq!(h.current, to); } else { assert_eq!(h.current, before); }
}

#[kani::proof]
#[kani::stub(kobo_web_document::url::clean, stub_clean)]
#[kani::stub(kobo_web_document::url::parse_authority, stub_authority)]
#[kani::stub(kobo_web_document::url::normalize_path, stub_norm)]
#[kani::stub(kobo_web_document::url::split_tail, stub_tail)]
#[kani::unwind(10)]
fn populated_set_position_touches_only_current() {
    let mut h = history2();
    let cur = h.current;
    let other = 1 - cur;
    let (op, ol) = (h.entries[other].page, h.entries[other].place);
    let (page, place): (usize, Option<usize>) = (kani::any(), kani::any());
    h.set_position(page, place);
    assert_eq!(h.current, cur);
    assert_eq!(h.entries.len(), 2);
    assert_eq!(h.entries[cur].page, page);
    assert_eq!(h.entries[cur].place, place);
    assert_eq!(h.entries[other].page, op);
    assert_eq!(h.entries[other].place, ol);
}

/// Either navigate was a no-op (Stay), or forward entries were dropped and a fresh entry added.
fn navigate_check(cur: usize) {
    let url = the_url();
    let mut h = history2_at(&url, cur);
    let first = (h.entries[0].page, h.entries[0].place);
    let go = h.navigate(url.clone());
    assert!(well_formed(&h));
    if matches!(go, Go::Stay) {
        assert_eq!(h.entries.len(), 2);
        assert_eq!(h.current, cur);
    } else {
        assert_eq!(h.entries.len(), cur + 2);
        assert_eq!(h.current, cur + 1);
        assert_eq!(h.entries[cur + 1].page, 0);
        assert_eq!(h.entries[cur + 1].place, None);
        assert_eq!((h.entries[0].page, h.entries[0].place), first);
        assert!(!h.can_go_forward());
    }
}
#[kani::proof]
#[kani::stub(kobo_web_document::url::clean, stub_clean)]
#[kani::stub(kobo_web_document::url::parse_authority, stub_authority)]
#[kani::stub(kobo_web_document::url::normalize_path, stub_norm)]
#[kani::stub(kobo_web_document::url::split_tail, stub_tail)]
#[kani::stub(kobo_web_document::url::Url::same_document, stub_same_document)]
#[kani::stub(<Url as PartialEq>::eq, stub_url_eq)]
#[kani::unwind(10)]
fn populated_navigate_cursor0() { navigate_check(0); }
#[kani::proof]
#[kani::stub(kobo_web_document::url::clean, stub_clean)]
#[kani::stub(kobo_web_document::url::parse_authority, stub_authority)]
#[kani::stub(kobo_web_document::url::normalize_path, stub_norm)]
#[kani::stub(kobo_web_document::url::split_tail, stub_tail)]
#[kani::stub(kobo_web_document::url::Url::same_document, stub_same_document)]
#[kani::stub(<Url as PartialEq>::eq, stub_url_eq)]
#[kani::unwind(10)]
fn populated_navigate_cursor1() { navigate_check(1); }
