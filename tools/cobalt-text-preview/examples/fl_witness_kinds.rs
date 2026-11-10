//! Measurement-only scratch (untracked): the ACTUAL poisons behind Part C's
//! 41 witnesses. (1) Per-page UsedHeightPass refusal-bits histogram with the
//! real Liberation face (which kinds fire, how many nodes). (2) For every
//! measurable witness block (collector-pass + real place_runs_normal Ok with a
//! collector-failing pure-inline SIBLING), the sibling's FIRST collector
//! failure reason - what a standards-correct support fix would need to
//! implement to remove the refusal instead of bypassing it.
use cobalt_text_preview::LocalFace;
use kobo_web_document::{box_tree::{BoxKind, BoxTree, HeightRefusal, UsedHeightPass, WidthPass}, computed_style::{Direction, FontStyle, Margin, Position}, css_text_page::FontProvider, inline_lines::place_runs_normal, parse_style_tree_with_inline_sheets_and_selectors, Limits, SupplyBudget};
use std::collections::{BTreeMap, BTreeSet};

fn collect_reason(bt: &BoxTree, parent: usize) -> Result<Vec<(String, bool)>, &'static str> {
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
        if st.color != o.color { return Err("color-varies"); }
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
        let widths = WidthPass::from_boxes(&bt, 800);
        let mut adv = |c: char, s: u32, b: bool| face.advance(c, s, b);
        let heights = UsedHeightPass::from_boxes_with_line_policy(&bt, 800, 600, &mut adv, |s, _| face.line_height(s, false));
        // (1) refusal-bits histogram
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for r in &heights.refusals {
            if r.is_empty() { continue; }
            let b = r.bits();
            for (bit, name) in [(1,"NO_METRICS_PROVIDER"),(2,"NO_INLINE_OWNER"),(4,"UNSUPPORTED_KIND"),(8,"FLEX_UNRESOLVED"),(16,"INDEFINITE_PERCENT"),(32,"NO_WIDTH"),(64,"EMPTY_OR_UNMEASURED"),(128,"FLOAT_UNRESOLVED"),(256,"MIN_HEIGHT_MARGIN_ESCAPE"),(512,"COMPOSITION_UNRESOLVED"),(1024,"STRETCH_UNRESOLVED")] {
                if b & bit != 0 { *kinds.entry(name.to_string()).or_insert(0) += 1; }
            }
        }
        let _ = HeightRefusal::NONE;
        // (2) witness-sibling first-failure histogram
        let mut witnesses = 0usize;
        let mut sib_reasons: BTreeMap<&'static str, usize> = BTreeMap::new();
        for i in 0..bt.boxes.len() {
            let Ok(runs) = collect_reason(&bt, i) else { continue; };
            let size = bt.boxes[i].style.font_size;
            let w = widths.widths[i].map(|w| w.content).unwrap_or(800);
            let refs: Vec<(&str, bool)> = runs.iter().map(|(t, b)| (t.as_str(), *b)).collect();
            if place_runs_normal(&refs, w, |ch, bold| face.advance(ch, size, bold)).is_err() { continue; }
            // measurable block: does it have a collector-failing pure-inline sibling?
            let Some(par) = bt.boxes[i].parent else { continue; };
            let mut witness = false;
            for &sib in &bt.boxes[par].children {
                if sib == i { continue; }
                match collect_reason(&bt, sib) {
                    Ok(_) => {}
                    Err("not-block-or-empty") | Err("not-pure-inline") => {}
                    Err(reason) => { *sib_reasons.entry(reason).or_insert(0) += 1; witness = true; }
                }
            }
            if witness { witnesses += 1; }
        }
        println!("{page}: refusal_histogram={kinds:?}");
        println!("{page}: witnesses={witnesses} sibling_first_failure={sib_reasons:?}");
    }
}
