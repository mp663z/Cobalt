//! Bounded `@media` width evaluation (diagnostic, isolated).
//!
//! Grammar (everything else is `Unknown`, which the style tree refuses):
//!   [only] (screen|all|print) [and <feature>]*   |   <feature> [and <feature>]*
//!   <feature> = "(" min-width|max-width ":" <number> (px|em) ")"
//! Numbers are unsigned decimals. Lengths compare exactly in integer
//! arithmetic against the supplied viewport width, never through rounding.
//! Relative units use the initial font size, not author CSS (MQ4 section 1.3).
//! Known false drops the block, known true applies it, unknown refuses.
use crate::style_syntax::{media_fit, MediaFit};

/// Viewport the stylesheet is evaluated against. Callers must pass the width
/// they will also lay out with; there is no default. A different width needs a
/// fresh parse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediaContext {
    viewport_width_px: u32,
    /// MEASUREMENT-ONLY prototype: host asserts a light colour scheme. Default false = legacy.
    light_scheme: bool,
}

impl MediaContext {
    /// `None` for a zero width: no real viewport exists to evaluate against.
    #[must_use]
    pub const fn new(viewport_width_px: u32) -> Option<Self> {
        if viewport_width_px == 0 {
            None
        } else {
            Some(Self { viewport_width_px, light_scheme: false })
        }
    }
    #[must_use]
    pub const fn with_light_scheme(mut self) -> Self {
        self.light_scheme = true;
        self
    }
    #[must_use]
    pub const fn viewport_width_px(self) -> u32 {
        self.viewport_width_px
    }
}

/// CSS initial `font-size` in px (the engine's `Computed::INITIAL`).
const INITIAL_FONT_PX: u128 = 16;
const MAX_DIGITS: usize = 12;
const MAX_FRACTION: usize = 6;
const MAX_FEATURES: usize = 8;

pub(crate) fn evaluate(prelude: &str, media: Option<MediaContext>) -> MediaFit {
    if !prelude.contains('(') {
        return media_fit(prelude);
    }
    let Some(media) = media else {
        return MediaFit::Unknown;
    };
    let Some(known) = evaluate_features(prelude, media) else {
        return MediaFit::Unknown;
    };
    if known {
        MediaFit::Screen
    } else {
        MediaFit::Skip
    }
}

/// `Some(truth)` for a fully valid supported query, `None` for anything else.
fn evaluate_features(prelude: &str, media: MediaContext) -> Option<bool> {
    if !prelude.is_ascii() || prelude.contains("/*") || prelude.contains(',') {
        return None;
    }
    let mut rest = prelude.trim_matches(|c: char| c.is_ascii_whitespace());
    let mut truth = true;
    if !rest.starts_with('(') {
        let mut words = rest.splitn(2, |c: char| c.is_ascii_whitespace());
        let mut first = words.next()?;
        let mut tail = words.next()?;
        if first.eq_ignore_ascii_case("only") {
            let mut more = tail
                .trim_start()
                .splitn(2, |c: char| c.is_ascii_whitespace());
            first = more.next()?;
            tail = more.next()?;
        }
        if first.eq_ignore_ascii_case("print") {
            truth = false;
        } else if !first.eq_ignore_ascii_case("screen") && !first.eq_ignore_ascii_case("all") {
            return None;
        }
        let tail = tail.trim_start();
        let after_and = strip_keyword(tail, "and")?;
        rest = after_and.trim_start();
        if !rest.starts_with('(') {
            return None;
        }
    }
    let mut features = 0;
    loop {
        features += 1;
        if features > MAX_FEATURES {
            return None;
        }
        let close = rest.find(')')?;
        truth &= feature(&rest[1..close], media)?;
        let tail = rest[close + 1..].trim_start();
        if tail.is_empty() {
            // The block must have been separated from `)` only by whitespace.
            return Some(truth);
        }
        // `and` must be separated from the previous `)` by whitespace (or
        // nothing) and followed by whitespace, then a parenthesized feature.
        let after = strip_keyword(tail, "and")?;
        rest = after.trim_start();
        if !rest.starts_with('(') {
            return None;
        }
    }
}

/// Strip `word` (ASCII case-insensitive) that must be followed by whitespace.
fn strip_keyword<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let head = text.get(..word.len())?;
    let tail = text.get(word.len()..)?;
    (head.eq_ignore_ascii_case(word) && tail.starts_with(|c: char| c.is_ascii_whitespace()))
        .then_some(tail)
}

fn feature(inner: &str, media: MediaContext) -> Option<bool> {
    if inner.contains(['(', ')']) {
        return None;
    }
    let (name, value) = inner.split_once(':')?;
    let name = name.trim_matches(|c: char| c.is_ascii_whitespace());
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    if name.eq_ignore_ascii_case("prefers-color-scheme") {
        if !media.light_scheme {
            return None;
        }
        return if value.eq_ignore_ascii_case("light") {
            Some(true)
        } else if value.eq_ignore_ascii_case("dark") {
            Some(false)
        } else {
            None
        };
    }
    let min = if name.eq_ignore_ascii_case("min-width") {
        true
    } else if name.eq_ignore_ascii_case("max-width") {
        false
    } else {
        return None;
    };
    let unit_at = value.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (number, unit) = value.split_at(unit_at);
    let per_unit = if unit.eq_ignore_ascii_case("px") {
        1
    } else if unit.eq_ignore_ascii_case("em") {
        INITIAL_FONT_PX
    } else {
        return None;
    };
    let (numerator, scale) = decimal(number)?;
    let viewport = u128::from(media.viewport_width_px).checked_mul(scale)?;
    let bound = numerator.checked_mul(per_unit)?;
    Some(if min {
        viewport >= bound
    } else {
        viewport <= bound
    })
}

/// Unsigned decimal as (digits as integer, `10^fraction_digits`). No sign, no
/// exponent, no bare trailing dot, bounded digit counts.
fn decimal(text: &str) -> Option<(u128, u128)> {
    let (int, frac) = match text.split_once('.') {
        Some((int, frac)) => {
            if frac.is_empty() || frac.contains('.') {
                return None;
            }
            (int, frac)
        }
        None => (text, ""),
    };
    if (int.is_empty() && frac.is_empty())
        || int.len() + frac.len() > MAX_DIGITS
        || frac.len() > MAX_FRACTION
        || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{int}{frac}");
    let numerator = digits.parse::<u128>().ok()?;
    let scale = 10_u128.checked_pow(u32::try_from(frac.len()).ok()?)?;
    Some((numerator, scale))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ev(query: &str, width: u32) -> MediaFit {
        evaluate(query, MediaContext::new(width))
    }
    #[test]
    fn initial_font_basis_is_the_engines_initial_font_size() {
        assert_eq!(
            INITIAL_FONT_PX,
            u128::from(crate::computed_style::Computed::INITIAL.font_size)
        );
        assert!(MediaContext::new(0).is_none());
        assert_eq!(MediaContext::new(7).unwrap().viewport_width_px(), 7);
    }
    #[test]
    fn fractional_em_boundaries_are_exact_not_rounded() {
        // 40.0625em = 641px exactly; 40.0525em = 640.84px.
        let min = "(min-width: 40.0625em)";
        let max = "(max-width: 40.0525em)";
        for (w, min_fit, max_fit) in [
            (639, MediaFit::Skip, MediaFit::Screen),
            (640, MediaFit::Skip, MediaFit::Screen),
            (641, MediaFit::Screen, MediaFit::Skip),
            (1072, MediaFit::Screen, MediaFit::Skip),
            (1264, MediaFit::Screen, MediaFit::Skip),
        ] {
            assert_eq!(ev(min, w), min_fit, "min at {w}");
            assert_eq!(ev(max, w), max_fit, "max at {w}");
        }
        assert_eq!(ev("(min-width: .5em)", 8), MediaFit::Screen);
        assert_eq!(ev("(min-width: .5em)", 7), MediaFit::Skip);
        assert_eq!(ev("(min-width: 0px)", 1), MediaFit::Screen);
        assert_eq!(ev("(max-width: 0px)", 1), MediaFit::Skip);
        assert_eq!(ev("(MIN-WIDTH:600PX)", 600), MediaFit::Screen);
        assert_eq!(ev("(min-width:600px)", 599), MediaFit::Skip);
    }
    #[test]
    fn compound_and_types() {
        let both = "(min-width:600px) and (max-width:700px)";
        assert_eq!(ev(both, 650), MediaFit::Screen);
        assert_eq!(ev(both, 599), MediaFit::Skip);
        assert_eq!(ev(both, 701), MediaFit::Skip);
        assert_eq!(ev("SCREEN AND (min-width:1px)", 5), MediaFit::Screen);
        assert_eq!(ev("only screen and (min-width:1px)", 5), MediaFit::Screen);
        assert_eq!(ev("all and (min-width:1px)", 5), MediaFit::Screen);
        assert_eq!(ev("print and (min-width:1px)", 5), MediaFit::Skip);
        assert_eq!(
            ev("screen and (min-width:1px) and (max-width:9px)", 5),
            MediaFit::Screen
        );
    }
    #[test]
    fn everything_else_is_unknown_never_a_pass() {
        for query in [
            "(min-width:-1px)",
            "(min-width:+1px)",
            "(min-width:1e3px)",
            "(min-width:1rem)",
            "(min-width:1)",
            "(min-width:40.px)",
            "(min-width:1 px)",
            "(min-width:)",
            "(min-width:.px)",
            "(width:600px)",
            "(min-width:600px) or (max-width:1px)",
            "not (min-width:1px)",
            "not screen and (min-width:1px)",
            "(min-width:1px),(max-width:2px)",
            "screen,(min-width:1px)",
            "(min-width:1px)and(max-width:2px)",
            "(min-width:1px) and",
            "(min-width:1px) and screen",
            "(min-width:(1px))",
            "(min-width:1px) (max-width:2px)",
            "(hover:hover)",
            "(min-width:1px) and (hover:hover)",
            "(max-width:0px) and (hover:hover)",
            "tv and (min-width:1px)",
            "(min-width:/**/1px)",
            "(min-width:1234567890123px)",
            "(min-width:.0000001px)",
            "only (min-width:1px)",
            "screen (min-width:1px)",
            "screen and",
            "(min-width:1px",
            "min-width:1px)",
            "(min-width:1px) and (min-width:1px) and (min-width:1px) and (min-width:1px) and (min-width:1px) and (min-width:1px) and (min-width:1px) and (min-width:1px) and (min-width:1px)",
            "(min-width:1\u{e9}px)",
        ] {
            assert_eq!(ev(query, 700), MediaFit::Unknown, "{query}");
        }
    }
    #[test]
    fn without_a_context_width_queries_refuse_and_type_only_is_unchanged() {
        assert_eq!(evaluate("(min-width:1px)", None), MediaFit::Unknown);
        assert_eq!(
            evaluate("screen and (min-width:1px)", None),
            MediaFit::Unknown
        );
        assert_eq!(evaluate("screen", None), MediaFit::Screen);
        assert_eq!(evaluate("print", None), MediaFit::Skip);
        assert_eq!(evaluate("", None), MediaFit::Screen);
        assert_eq!(evaluate("tv", None), MediaFit::Unknown);
    }
}
