//! art67 precision-boundary fixture (scratch, unadopted). Pins the known divergence between the Python models
//! (floor(x+0.5)) and Rust f64::round on x = 0.49999999999999994, and the near-tie guard.
use kobo_web_document::flex_units_v3::{near_tie, resolve, Item, Mode};

#[test]
fn rounding_primitives_diverge_at_the_largest_double_below_half() {
    let x = 0.499_999_999_999_999_94_f64;
    assert_eq!(x.round(), 0.0); // Rust: half away from zero on the exact value
    assert_eq!((x + 0.5).floor(), 1.0); // Python-model style: x + 0.5 rounds up to 1.0 in f64
}

#[test]
fn near_tie_detects_exact_and_near_half() {
    assert!(near_tie(1, 1, 2)); // 0.5
    assert!(near_tie(-7641, 1, 2)); // -3820.5
    assert!(!near_tie(100, 1, 3));
    assert!(near_tie(1_000_000_000, 1, 2_000_000_001)); // 0.49999999975..: within 1e-9
    assert!(!near_tie(1_000, 1, 2_001)); // 0.4997..: not within 1e-9
}

#[test]
fn guarded_mode_refuses_ties_but_unguarded_resolves() {
    let it = |base| Item { base, min: 0, max: None, grow: 1, shrink: 1, padb: 0, ml: 0, mr: 0 };
    // two equal items sharing an odd unit count: 101 units / 2 = 50.5 tie
    let items = [it(0), it(0)];
    assert!(resolve(Mode::Chrome, 101, &items).is_some());
    assert_eq!(resolve(Mode::ChromeGuarded, 101, &items), None);
}
