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
