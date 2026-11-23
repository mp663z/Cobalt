//! Measurement-only scratch: for pure-inline owner blocks that PASS the exact
//! delta10 E-C5 bounded-gain measurement: SAME as fl_measurable_today but with the
//! colour admission check REMOVED from the collector replica (engine truth post-delta10).
//! Compare counts against certified fl_measurable_today for the exact colour gain.
//! the pinned Liberation face to census what is measurable with existing code
//! IF tree poison/containment were lifted (hypothetical, design sizing only).
//! Runs are collected in the real collector's traversal order. Width from
//! WidthPass when resolved (fallback 800 labeled). NOT an engine verdict.
use cobalt_text_preview::LocalFace;
use kobo_web_document::{box_tree::{BoxKind, BoxTree, WidthPass}, computed_style::{Direction, FontStyle, Margin, Position}, css_text_page::FontProvider, inline_lines::place_runs_normal, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
use std::collections::BTreeSet;
fn collect_ok(bt: &BoxTree, parent: usize) -> Option<Vec<(String, bool)>> {
    let owner = &bt.boxes[parent];
    if owner.kind != BoxKind::Block || owner.children.is_empty() { return None; }
    if !owner.children.iter().all(|&c| matches!(bt.boxes[c].kind, BoxKind::Text | BoxKind::Inline)) { return None; }
    let mut runs: Vec<(String, bool)> = Vec::new();
    let mut seen: BTreeSet<usize> = [parent].into_iter().collect();
    let mut stack: Vec<(usize, usize, usize)> = Vec::new();
    if owner.children.len() > 4096 { return Some(vec![]); }
    for &c in owner.children.iter().rev() { stack.push((c, parent, 0)); }
    let mut visited = 0usize; let mut edges = owner.children.len(); let mut leaves = 0usize;
    while let Some((index, expected, depth)) = stack.pop() {
        visited += 1;
        if visited > 4096 || edges > 4096 || !seen.insert(index) { return Some(vec![]); }
        let n = &bt.boxes[index];
        if n.parent != Some(expected) || n.source.is_none() { return Some(vec![]); }
        let st = n.style; let o = owner.style;
        if st.direction != Direction::Ltr || st.font_size != o.font_size || st.font_family.is_some()
            || st.font_style != FontStyle::Normal 
            || !matches!(st.font_weight, 400 | 700)
            || st.line_height != o.line_height || st.line_height_number != o.line_height_number || st.line_height_precise != o.line_height_precise { return Some(vec![]); }
        match n.kind {
            BoxKind::Text => {
                if !n.children.is_empty() || leaves >= 4096 { return Some(vec![]); }
                runs.push((n.text.clone()?, st.font_weight == 700)); leaves += 1;
            }
            BoxKind::Inline => {
                let depth = depth + 1;
                let legacy_direct = expected == parent && n.children.len() == 1 && bt.boxes.get(n.children[0]).is_some_and(|c| c.kind == BoxKind::Text);
                if depth > 96 || !n.inline_attributes || (!legacy_direct && !n.inline_phrase) || n.text.is_some()
                    || st.positioned.position != Position::Static
                    || [st.margin_left, st.margin_right, st.margin_top, st.margin_bottom].iter().any(|m| *m != Margin::Px(0))
                    || [st.padding_left, st.padding_right, st.padding_top, st.padding_bottom].iter().any(|p| *p != 0)
                    || st.background_color.is_some() || st.background_current_color
                    || st.box_shadow.is_some() || st.paints_outline() || st.decor.has_painted_border() || st.decor.layers != 1 { return Some(vec![]); }
                edges = edges.saturating_add(n.children.len());
                for &c in n.children.iter().rev() { stack.push((c, index, depth)); }
            }
            _ => return Some(vec![]),
        }
    }
    if leaves == 0 { return Some(vec![]); }
    Some(runs)
}
fn main() {
    let face = LocalFace::from_bytes(&std::fs::read("/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf").unwrap()).unwrap();
    for page in ["gov", "blog", "docs", "news-npr", "wikipedia"] {
        let path = format!("{}/cb/rr74/rp/{page}.html", std::env::var("HOME").unwrap());
        let bytes = std::fs::read(&path).unwrap();
        let st = parse_style_tree_with_inline_sheets_and_selectors(&bytes, &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        let bt = BoxTree::from_style(&st);
        let widths = WidthPass::from_boxes(&bt, 800);
        let mut ok = 0usize; let mut width_fallback = 0usize;
        let mut errs: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        let mut total_lines = 0usize;
        for i in 0..bt.boxes.len() {
            let Some(runs) = collect_ok(&bt, i) else { continue; };
            if runs.is_empty() { continue; } // collector-fail blocks skipped (already census'd)
            let size = bt.boxes[i].style.font_size;
            let (w, fb) = match widths.widths[i] { Some(w) => (w.content, false), None => (800, true) };
            if fb { width_fallback += 1; }
            let refs: Vec<(&str, bool)> = runs.iter().map(|(t, b)| (t.as_str(), *b)).collect();
            match place_runs_normal(&refs, w, |ch, bold| face.advance(ch, size, bold)) {
                Ok(lines) => { ok += 1; total_lines += lines.count as usize; }
                Err(e) => *errs.entry(format!("{e:?}")).or_insert(0) += 1,
            }
        }
        println!("{page}: measurable_ok={ok} (width_fallback_800={width_fallback}) total_lines={total_lines} errs={errs:?}");
    }
}
