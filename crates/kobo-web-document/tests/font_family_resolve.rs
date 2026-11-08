//! Synthetic catalog tests for deterministic `font-family` list resolution. No real fonts,
//! no host aliases: the catalog declares its faces and what it asserts absent.
use kobo_web_document::font_family::{parse_family_list, resolve_family, Entry, FaceCatalog, FamilyId, Generic, Lookup, Refusal};

struct Cat;
const ALPHA: FamilyId = FamilyId(1);
const BETA: FamilyId = FamilyId(2);
impl FaceCatalog for Cat {
    fn lookup(&self, n: &str) -> Lookup {
        match n {
            "alpha sans" => Lookup::Available(ALPHA),
            "beta mono" => Lookup::Available(BETA),
            "gone" | "also gone" | "fira sans" => Lookup::Absent,
            _ => Lookup::Unknown, // e.g. "helvetica": the catalog cannot rule out an alias
        }
    }
    fn generic(&self, g: Generic) -> Option<FamilyId> {
        match g { Generic::SansSerif => Some(ALPHA), Generic::Monospace => Some(BETA), Generic::Serif => None }
    }
}

#[test]
fn first_available_wins_in_order_and_absent_names_are_skipped() {
    assert_eq!(resolve_family("alpha sans, beta mono", &Cat), Ok(ALPHA));
    assert_eq!(resolve_family("\"beta mono\", alpha sans", &Cat), Ok(BETA));
    assert_eq!(resolve_family("gone, 'also gone', beta mono", &Cat), Ok(BETA));
    assert_eq!(resolve_family("\"fira sans\", sans-serif", &Cat), Ok(ALPHA));
    assert_eq!(resolve_family("  ALPHA   SANS ".to_ascii_lowercase().as_str(), &Cat), Ok(ALPHA));
    assert_eq!(resolve_family("gone, monospace", &Cat), Ok(BETA));
}

#[test]
fn undecidable_unmapped_or_empty_lists_refuse_rather_than_guess() {
    // Unknown name BEFORE an available one: a host alias could win, so it refuses.
    assert_eq!(resolve_family("helvetica, alpha sans", &Cat), Err(Refusal::Undecidable));
    assert_eq!(resolve_family("serif", &Cat), Err(Refusal::Undecidable)); // unmapped generic
    assert_eq!(resolve_family("gone, also gone", &Cat), Err(Refusal::NothingAvailable));
    for g in ["system-ui", "cursive", "fantasy", "ui-sans-serif", "emoji", "math", "fangsong", "ui-monospace", "ui-serif", "ui-rounded"] {
        assert_eq!(resolve_family(&format!("alpha sans, {g}"), &Cat), Ok(ALPHA), "decided before the generic: {g}");
        assert_eq!(resolve_family(&format!("gone, {g}"), &Cat), Err(Refusal::UnsupportedGeneric), "{g}");
    }
}

#[test]
fn strict_grammar_refuses_outside_cases() {
    for bad in ["", ",", "alpha sans,", ",alpha sans", "alpha, , beta", "\"alpha", "alpha\"", "'a\"b'", "var(--f)", "alpha sans, var(--x)",
        "a\\62 c", "1abc", "--", "-1x", "a;b", "\"\"", "''", "a b!", "\"a\nb\""] {
        assert_eq!(parse_family_list(bad), Err(Refusal::Syntax), "{bad:?}");
    }
    for kw in ["inherit", "initial", "unset", "revert", "revert-layer", "default", "alpha sans, inherit"] {
        assert_eq!(parse_family_list(kw), Err(Refusal::CssWideKeyword), "{kw}");
    }
    let many = (0..17).map(|i| format!("f{i}")).collect::<Vec<_>>().join(",");
    assert_eq!(parse_family_list(&many), Err(Refusal::Syntax));
    assert_eq!(parse_family_list(&"a".repeat(257)), Err(Refusal::Syntax));
    assert!(parse_family_list(&(0..16).map(|i| format!("f{i}")).collect::<Vec<_>>().join(",")).is_ok());
}

#[test]
fn quoting_names_generics_and_multiword_idents() {
    assert_eq!(parse_family_list("\"Alpha Sans\", Beta   MONO"), Ok(vec![Entry::Named("alpha sans".into()), Entry::Named("beta mono".into())]));
    assert_eq!(parse_family_list("\"sans-serif\""), Ok(vec![Entry::Named("sans-serif".into())])); // quoted generic is a NAME
    assert_eq!(parse_family_list("Segoe   UI, -apple-system"), Ok(vec![Entry::Named("segoe ui".into()), Entry::Named("-apple-system".into())]));
    assert_eq!(parse_family_list("sans-serif Mono"), Ok(vec![Entry::Named("sans-serif mono".into())])); // multiword is a name
    assert_eq!(parse_family_list("'a, b', c"), Ok(vec![Entry::Named("a, b".into()), Entry::Named("c".into())])); // comma inside quotes
    // A quoted name is not the generic: the catalog sees it as a name and says Unknown here.
    assert_eq!(resolve_family("\"sans-serif\"", &Cat), Err(Refusal::Undecidable));
    assert_eq!(parse_family_list("gds transport,arial,sans-serif").map(|v| v.len()), Ok(3));
}

// Adversarial catalog: claims EVERYTHING it is asked about is available, so a keyword or generic
// that slipped through classification as a plain name would resolve to Ok(NAMED).
struct Greedy;
const NAMED: FamilyId = FamilyId(9);
const GEN: FamilyId = FamilyId(8);
impl FaceCatalog for Greedy {
    fn lookup(&self, _: &str) -> Lookup { Lookup::Available(NAMED) }
    fn generic(&self, _: Generic) -> Option<FamilyId> { Some(GEN) }
}

#[test]
fn keyword_and_generic_classification_is_ascii_case_insensitive() {
    for g in ["SANS-SERIF", "Sans-Serif", "sans-SERIF"] {
        assert_eq!(resolve_family(g, &Greedy), Ok(GEN), "{g}");
    }
    assert_eq!(resolve_family("SERIF", &Greedy), Ok(GEN));
    assert_eq!(resolve_family("Monospace", &Greedy), Ok(GEN));
    for kw in ["Inherit", "INITIAL", "Unset", "REVERT", "Revert-Layer", "DEFAULT"] {
        assert_eq!(resolve_family(kw, &Greedy), Err(Refusal::CssWideKeyword), "{kw}");
    }
    for g in ["System-UI", "CURSIVE", "Fantasy", "UI-Serif", "UI-SANS-SERIF", "UI-Monospace", "Ui-Rounded", "EMOJI", "Math", "FangSong"] {
        assert_eq!(resolve_family(g, &Greedy), Err(Refusal::UnsupportedGeneric), "{g}");
    }
    // Quoted forms are names whatever the case.
    assert_eq!(resolve_family("\"SANS-SERIF\"", &Greedy), Ok(NAMED));
    assert_eq!(resolve_family("\"Inherit\"", &Greedy), Ok(NAMED));
    assert_eq!(resolve_family("'System-UI'", &Greedy), Ok(NAMED));
}

#[test]
fn css_wide_keyword_in_any_unquoted_word_position_refuses() {
    for v in ["foo inherit", "inherit foo", "a Initial b", "x UNSET", "default font", "my default", "Revert-Layer x"] {
        assert_eq!(resolve_family(v, &Greedy), Err(Refusal::CssWideKeyword), "{v}");
    }
    // Quoted multiword names containing keywords are ordinary names.
    assert_eq!(resolve_family("\"foo inherit\"", &Greedy), Ok(NAMED));
    // A generic word inside a multiword unquoted name is part of a NAME.
    assert_eq!(resolve_family("sans-serif Mono", &Greedy), Ok(NAMED));
}

// ---- FL51 increment 2: coverage-gated single-face-per-run selection ----
use kobo_web_document::font_family::{select_run_family, FaceCoverage, RunRefusal};
struct Two; // family "a" (id 1) covers 'x' only; family "b" (id 2) covers everything; "u" is unknown-coverage (id 3)
impl FaceCatalog for Two {
    fn lookup(&self, n: &str) -> Lookup {
        match n { "a" => Lookup::Available(FamilyId(1)), "b" => Lookup::Available(FamilyId(2)), "u" => Lookup::Available(FamilyId(3)), "gone" => Lookup::Absent, _ => Lookup::Unknown }
    }
    fn generic(&self, _: Generic) -> Option<FamilyId> { None }
}
impl FaceCoverage for Two {
    fn covers(&self, f: FamilyId, c: char, _: bool) -> Option<bool> {
        match f.0 { 1 => Some(c == 'x'), 2 => Some(true), _ => if c == 'x' { Some(true) } else { None } }
    }
}
#[test]
fn first_available_face_partial_coverage_refuses_never_skips_to_later_face() {
    assert_eq!(select_run_family("a, b", &Two, "xx", false), Ok(FamilyId(1)));
    assert_eq!(select_run_family("a, b", &Two, "xy", false), Err(RunRefusal::Uncovered)); // b would cover, still refuse
    assert_eq!(select_run_family("b, a", &Two, "xy", false), Ok(FamilyId(2)));
    assert_eq!(select_run_family("gone, a, b", &Two, "x", false), Ok(FamilyId(1))); // Absent IS skipped (not available)
    assert_eq!(select_run_family("gone, a, b", &Two, "xz", false), Err(RunRefusal::Uncovered));
}
#[test]
fn unknown_coverage_and_family_refusals_propagate() {
    assert_eq!(select_run_family("u, b", &Two, "xy", false), Err(RunRefusal::UnknownCoverage));
    assert_eq!(select_run_family("u", &Two, "x", false), Ok(FamilyId(3)));
    assert_eq!(select_run_family("nope, b", &Two, "x", false), Err(RunRefusal::Family(Refusal::Undecidable)));
    assert_eq!(select_run_family("gone", &Two, "x", false), Err(RunRefusal::Family(Refusal::NothingAvailable)));
    assert_eq!(select_run_family("system-ui, b", &Two, "x", false), Err(RunRefusal::Family(Refusal::UnsupportedGeneric)));
    assert_eq!(select_run_family("inherit", &Two, "x", false), Err(RunRefusal::Family(Refusal::CssWideKeyword)));
    assert_eq!(select_run_family("a, b", &Two, "", false), Ok(FamilyId(1))); // empty run: nothing to cover
}
