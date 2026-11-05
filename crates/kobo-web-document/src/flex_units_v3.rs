//! v3 (adds Mode::ChromeGuarded: refuses exact/near ties; v1 and v2 files unchanged). v2 of art67 flex_units (v1 file kept unchanged next to it): Mode::ExactPx now mirrors the integer engine's refusal control flow
//! (strict growing test, freeze only on a zero flex FACTOR, refuse when free-space sign disagrees with the mode or the unfrozen weight sum is 0).
//! Scratch (art67, unadopted; NOT wired into any engine path): CSS Flexbox 9.7 for a single-line row in 1/64 px
//! units. `Mode::ExactPx` reproduces the integer engine's contract (every share must be an exact whole pixel,
//! otherwise None); `Mode::Chrome` uses the share rule fitted to Chrome 156 in art58-art60 (descriptive, not Blink source).
//! Fail-closed boundaries: all inputs are bounded (|value| <= LIMIT units), arithmetic is i128, grow/shrink are
//! integer factors (fractional factors are refused at CSS parse, so the "sum of factors < 1" spec step cannot occur:
//! an unfrozen factor sum is 0 or >= 1; zero-factor items freeze first).
pub const LIMIT: i64 = 1 << 30; // 16,777,216 px in 1/64 units: far beyond MAX_PIXELS contexts; larger inputs refuse

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    ExactPx,
    Chrome,
    /// Chrome rule, but refuse any share within 1e-9 (relative to the divisor) of a .5 tie: the f64 behaviour there is unproven.
    ChromeGuarded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Item {
    /// Inner flex base size (content box), 1/64 px.
    pub base: i64,
    /// Used min and max inner size; `max` is None for none.
    pub min: i64,
    pub max: Option<i64>,
    pub grow: u32,
    pub shrink: u32,
    /// Horizontal padding+border (both sides) and margins, 1/64 px.
    pub padb: i64,
    pub ml: i64,
    pub mr: i64,
}

fn bounded(v: i64) -> bool {
    v.abs() <= LIMIT
}

/// True when rem*w/t is within 1e-9 of a half-integer (exact integer arithmetic, i128).
pub fn near_tie(rem: i128, w: i128, t: i128) -> bool {
    if t == 0 {
        return false;
    }
    let num = (rem * w).abs();
    let r = num % t; // fractional part is r/t
    // |r/t - 1/2| <= 1e-9  <=>  |2r - t| * 1e9 <= 2t
    ((2 * r - t).abs()) * 1_000_000_000 <= 2 * t
}

fn share(mode: Mode, rem: i128, w: i128, t: i128) -> Option<i128> {
    if t == 0 {
        return Some(0);
    }
    match mode {
        Mode::ExactPx => {
            // exact whole-pixel share only: rem*w / t must be an integer multiple of 64 units
            let num = rem.checked_mul(w)?;
            let den = t.checked_mul(64)?;
            (num % den == 0).then(|| num / t)
        }
        Mode::Chrome | Mode::ChromeGuarded => {
            if mode == Mode::ChromeGuarded && near_tie(rem, w, t) {
                return None;
            }
            // ratio first in f64, then multiply, half away from zero (art58); values are bounded so f64 is exact enough
            Some(((rem as f64) * ((w as f64) / (t as f64))).round() as i128)
        }
    }
}

/// Resolve one row; returns (left, border-box width) per item in 1/64 px, or None (refuse).
#[must_use]
pub fn resolve(mode: Mode, container: i64, items: &[Item]) -> Option<Vec<(i64, i64)>> {
    if items.is_empty() || !bounded(container) {
        return None;
    }
    for i in items {
        if ![i.base, i.min, i.padb, i.ml, i.mr].iter().all(|&v| bounded(v))
            || i.max.is_some_and(|m| !bounded(m))
            || i.base < 0
            || i.min < 0
            || i.padb < 0
            || i.ml < 0
            || i.mr < 0
        {
            return None; // negative margins/sizes are unproven here
        }
    }
    let n = items.len();
    let clamp = |k: usize, v: i128| -> i128 {
        let v = match items[k].max {
            Some(m) => v.min(i128::from(m)),
            None => v,
        };
        v.max(i128::from(items[k].min))
    };
    let hyp: Vec<i128> = (0..n).map(|k| clamp(k, i128::from(items[k].base))).collect();
    let outer = |k: usize, size: i128| size + i128::from(items[k].padb) + i128::from(items[k].ml) + i128::from(items[k].mr);
    let free0 = i128::from(container) - (0..n).map(|k| outer(k, hyp[k])).sum::<i128>();
    let legacy = mode == Mode::ExactPx;
    let grow = if legacy { free0 > 0 } else { free0 >= 0 };
    let w: Vec<i128> = items
        .iter()
        .map(|i| if grow { i128::from(i.grow) } else { i128::from(i.shrink) * i128::from(i.base) })
        .collect();
    let mut frozen = vec![false; n];
    let mut size = hyp.clone();
    for k in 0..n {
        let b = i128::from(items[k].base);
        let factor0 = if grow { items[k].grow == 0 } else { items[k].shrink == 0 };
        let inflexible = if legacy { factor0 } else { w[k] == 0 };
        if inflexible || (grow && b > hyp[k]) || (!grow && b < hyp[k]) {
            frozen[k] = true;
        }
    }
    for _ in 0..=n {
        let unf: Vec<usize> = (0..n).filter(|&k| !frozen[k]).collect();
        if unf.is_empty() {
            break;
        }
        let free = i128::from(container)
            - (0..n).map(|k| outer(k, if frozen[k] { size[k] } else { i128::from(items[k].base) })).sum::<i128>();
        let mut t: i128 = unf.iter().map(|&k| w[k]).sum();
        if legacy && free != 0 && ((grow && free < 0) || (!grow && free > 0) || t == 0) {
            return None;
        }
        let mut rem = free;
        let mut tgt = vec![0_i128; n];
        for &k in unf.iter().rev() {
            let v = share(mode, rem, w[k], t)?;
            tgt[k] = i128::from(items[k].base) + v;
            rem -= v;
            t -= w[k];
        }
        let mut viol = 0_i128;
        let mut cl = vec![0_i128; n];
        for &k in &unf {
            cl[k] = clamp(k, tgt[k]);
            viol += cl[k] - tgt[k];
        }
        if viol == 0 {
            for &k in &unf {
                size[k] = tgt[k];
                frozen[k] = true;
            }
            break;
        }
        for &k in &unf {
            if (viol > 0 && cl[k] > tgt[k]) || (viol < 0 && cl[k] < tgt[k]) {
                size[k] = cl[k];
                frozen[k] = true;
            }
        }
    }
    if frozen.iter().any(|f| !f) {
        return None; // did not converge within n+1 rounds: refuse
    }
    let mut x = 0_i128;
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let l = x + i128::from(items[k].ml);
        let bw = size[k] + i128::from(items[k].padb);
        x = l + bw + i128::from(items[k].mr);
        out.push((i64::try_from(l).ok()?, i64::try_from(bw).ok()?));
    }
    if x > i128::from(LIMIT) * 4 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn it(base: i64, grow: u32, shrink: u32) -> Item {
        Item { base, min: 0, max: None, grow, shrink, padb: 0, ml: 0, mr: 0 }
    }
    #[test]
    fn three_equal_flex_one_in_100px_chrome_units() {
        // art51/art53 observation: 33.328125 / 33.34375 / 33.328125 px = 2133/2134/2133 units
        let r = resolve(Mode::Chrome, 100 * 64, &[it(0, 1, 1), it(0, 1, 1), it(0, 1, 1)]).unwrap();
        assert_eq!(r.iter().map(|x| x.1).collect::<Vec<_>>(), vec![2133, 2134, 2133]);
    }
    #[test]
    fn exact_px_mode_refuses_fractional_share() {
        assert_eq!(resolve(Mode::ExactPx, 100 * 64, &[it(0, 1, 1), it(0, 1, 1), it(0, 1, 1)]), None);
        let r = resolve(Mode::ExactPx, 300 * 64, &[it(0, 1, 1), it(0, 1, 1), it(0, 1, 1)]).unwrap();
        assert_eq!(r, vec![(0, 100 * 64), (100 * 64, 100 * 64), (200 * 64, 100 * 64)]);
    }
    #[test]
    fn bounds_refuse() {
        assert_eq!(resolve(Mode::Chrome, LIMIT + 1, &[it(0, 1, 1)]), None);
        assert_eq!(resolve(Mode::Chrome, 100, &[Item { ml: -1, ..it(0, 1, 1) }]), None);
    }
}
