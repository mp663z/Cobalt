//! delta10 reviewer follow-up (relayed via main 18:16): REAL engine-path colour
//! measurement. fl_witness_kinds attributes first-failure kinds with its own
//! hand-maintained collector REPLICA (its colour check at line 32 mirrored the
//! pre-delta10 engine); fl_measurable_color is a replica with the colour check
//! removed. Neither executes the engine. This instrument runs the ACTUAL
//! kobo_web_document::inline_lines::place_runs_text collector (post-delta10
//! code) on every measurable witness sibling, and cross-tabs the outcome:
//!   old_replica_reason x real_engine_result.
//! The post-delta10 replica (colour check removed) is kept ONLY to attribute
//! the remaining engine refusals, and every number it produces is labelled
//! REPLICA. Owner-run measurement, not a usability claim.
use cobalt_text_preview::LocalFace;
use kobo_web_document::{box_tree::{BoxKind, BoxTree}, computed_style::{Direction, FontStyle, Margin, Position}, css_text_page::FontProvider, inline_lines::{place_runs_normal, place_runs_text}, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
use std::collections::{BTreeMap, BTreeSet};

// colour_check: true = pre-delta10 replica (identifies the old colour-varies cohort),
// false = post-delta10 replica (attributes remaining real engine refusals only).
fn collect_reason(bt: &BoxTree, parent: usize, colour_check: bool) -> Result<Vec<(String, bool)>, &'static str> {
    let owner = &bt.boxes[parent];
    if owner.kind != BoxKind::Block || owner.children.is_empty() { return Err("not-block-or-empty"); }
    if !owner.children.iter().all(|&c| matches!(bt.boxes[c].kind, BoxKind::Text | BoxKind::Inline)) { return Err("not-pure-inline"); }
    let mut runs: Vec<(String, bool)> = Vec::new();
    let mut seen: BTreeSet<usize> = [parent].into_iter().collect();
    let mut stack: Vec<(usize, usize, usize)> = Vec::new();
    if owner.children.len() > 4096 { return Err("bounded"); }
    for &c in owner.children.iter().rev() { stack.push((c, parent, 0)); }
    let mut visited = 0usize; let mut edges = owner.children.len(); let mut leaves = 0usize;
    while let Some((index, expected, depth)) = stack.pop() {
        visited += 1;
        if visited > 4096 || edges > 4096 || !seen.insert(index) { return Err("bounded"); }
        let n = &bt.boxes[index];
        if n.parent != Some(expected) || n.source.is_none() { return Err("structure"); }
        let st = n.style; let o = owner.style;
        if st.direction != Direction::Ltr { return Err("rtl"); }
        if st.font_size != o.font_size { return Err("font-size-varies"); }
        if st.font_family.is_some() { return Err("font-family-override"); }
        if st.font_style != FontStyle::Normal { return Err("italic-or-oblique"); }
        if colour_check && st.color != o.color { return Err("color-varies"); }
        if !matches!(st.font_weight, 400 | 700) { return Err("weight-beyond-400-700"); }
        if st.line_height != o.line_height || st.line_height_number != o.line_height_number || st.line_height_precise != o.line_height_precise { return Err("line-height-varies"); }
        match n.kind {
            BoxKind::Text => {
                if !n.children.is_empty() || leaves >= 4096 { return Err("bounded"); }
                runs.push((n.text.clone().ok_or("no-text")?, st.font_weight == 700)); leaves += 1;
            }
            BoxKind::Inline => {
                let depth = depth + 1;
                let legacy_direct = expected == parent && n.children.len() == 1 && bt.boxes.get(n.children[0]).is_some_and(|c| c.kind == BoxKind::Text);
                if depth > 96 { return Err("depth"); }
                if !n.inline_attributes { return Err("inline-attributes"); }
                if !legacy_direct && !n.inline_phrase { return Err("not-phrase"); }
                if n.text.is_some() { return Err("inline-with-text"); }
                if st.positioned.position != Position::Static { return Err("positioned-inline"); }
                if [st.margin_left, st.margin_right, st.margin_top, st.margin_bottom].iter().any(|m| *m != Margin::Px(0)) { return Err("inline-margin"); }
                if [st.padding_left, st.padding_right, st.padding_top, st.padding_bottom].iter().any(|p| *p != 0) { return Err("inline-padding"); }
                if st.background_color.is_some() || st.background_current_color { return Err("inline-background"); }
                if st.box_shadow.is_some() { return Err("box-shadow"); }
                if st.paints_outline() { return Err("outline"); }
                if st.decor.has_painted_border() { return Err("inline-border"); }
                if st.decor.layers != 1 { return Err("decor-layers"); }
                edges = edges.saturating_add(n.children.len());
                for &c in n.children.iter().rev() { stack.push((c, index, depth)); }
            }
            _ => return Err("other-kind"),
        }
    }
    if leaves == 0 { return Err("no-leaves"); }
    Ok(runs)
}

fn main() {
    let face = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf").unwrap()).unwrap();
    for page in ["gov", "blog", "docs", "news-npr", "wikipedia"] {
        let path = format!("{}/cb/rr74/rp/{page}.html", std::env::var("HOME").unwrap());
        let bytes = std::fs::read(&path).unwrap();
        let st = parse_style_tree_with_inline_sheets_and_selectors(&bytes, &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        let bt = BoxTree::from_style(&st);
        let widths = kobo_web_document::box_tree::WidthPass::from_boxes(&bt, 800);
        let mut engine_ok = 0usize;
        let mut engine_err = 0usize;
        // old colour-varies cohort cross-tab
        let mut cohort_now_ok = 0usize;
        let mut cohort_now_err: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut engine_err_reasons: BTreeMap<&'static str, usize> = BTreeMap::new();
        for i in 0..bt.boxes.len() {
            let Ok(runs) = collect_reason(&bt, i, false) else { continue; };
            let size = bt.boxes[i].style.font_size;
            let w = widths.widths[i].map(|w| w.content).unwrap_or(800);
            let refs: Vec<(&str, bool)> = runs.iter().map(|(t, b)| (t.as_str(), *b)).collect();
            if place_runs_normal(&refs, w, |ch, bold| face.advance(ch, size, bold)).is_err() { continue; }
            let Some(par) = bt.boxes[i].parent else { continue; };
            for &sib in &bt.boxes[par].children {
                if sib == i { continue; }
                // only collector-relevant siblings (same filter as fl_witness_kinds)
                match collect_reason(&bt, sib, true) {
                    Ok(_) | Err("not-block-or-empty") | Err("not-pure-inline") => continue,
                    Err(_) => {}
                }
                // REAL engine collector on the sibling:
                let sw = widths.widths[sib].map(|x| x.content).unwrap_or(800);
                let ssize = bt.boxes[sib].style.font_size;
                let engine = place_runs_text(&bt, sib, sw, |ch, s, b| face.advance(ch, s, b), |s| {
                    if bt.boxes[sib].style.line_height.is_none() && bt.boxes[sib].style.line_height_number.is_none() && bt.boxes[sib].style.line_height_precise.is_none() {
                        face.normal_line_metrics(s, false).map(|m| m.height).or_else(|| face.line_height(s, false))
                    } else {
                        face.line_height(s, false)
                    }
                });
                let old_reason = collect_reason(&bt, sib, true).err().unwrap_or("ok");
                match engine {
                    Ok(_) => {
                        engine_ok += 1;
                        if old_reason == "color-varies" { cohort_now_ok += 1; }
                    }
                    Err(e) => {
                        engine_err += 1;
                        let kind: &'static str = match e {
                            kobo_web_document::inline_lines::LineError::UnsupportedText => "ENGINE:UnsupportedText",
                            kobo_web_document::inline_lines::LineError::InvalidMetrics => "ENGINE:InvalidMetrics",
                            kobo_web_document::inline_lines::LineError::UnbreakableWord => "ENGINE:UnbreakableWord",
                            kobo_web_document::inline_lines::LineError::TooManyGlyphs => "ENGINE:TooManyGlyphs",
                            kobo_web_document::inline_lines::LineError::TooManyLines => "ENGINE:TooManyLines",
                            kobo_web_document::inline_lines::LineError::UnsupportedTree => "ENGINE:UnsupportedTree",
                            kobo_web_document::inline_lines::LineError::FractionalAlignment => "ENGINE:FractionalAlignment",
                        };
                        let new_reason = collect_reason(&bt, sib, false).err().unwrap_or("replica-clean");
                        *engine_err_reasons.entry(kind).or_insert(0) += 1;
                        *engine_err_reasons.entry(if new_reason == "replica-clean" { "replica-clean (block-level gate)" } else { new_reason }).or_insert(0) += 1;
                        if old_reason == "color-varies" { *cohort_now_err.entry(kind).or_insert(0) += 1; }
                    }
                }
            }
        }
        println!("{page}: ENGINE-PATH sibling outcomes: ok={engine_ok} err={engine_err}");
        println!("{page}: ENGINE-PATH old-colour-varies cohort: now_ok={cohort_now_ok} now_err(REPLICA-attributed)={cohort_now_err:?}");
        println!("{page}: ENGINE-PATH refusal kinds (real LineError + REPLICA attribution)={engine_err_reasons:?}");
    }
}
