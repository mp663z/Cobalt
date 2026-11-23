//! EXPERIMENTAL, UNREVIEWED, opt-in: `::after` generated boxes on the extended-CSS
//! entry only. Narrow: `content:""`/normal/none, no var(), no rem values; every other
//! pseudo-element or content value refuses. Not adopted.
use kobo_web_document::{
    box_tree::BoxTree,
    css_background::{
        paint_backgrounds, paint_backgrounds_float_kernel, paint_backgrounds_generated_block,
    },
    display_list::{Command, DisplayList, Rgb},
    parse_style_tree, parse_style_tree_with_inline_sheets_and_selectors,
    style_tree::StyleTree,
    Limits, SupplyBudget,
};

fn ext(css: &str, body: &str) -> StyleTree {
    let html = format!("<!doctype html><style>body{{margin:0}}{css}</style>{body}");
    parse_style_tree_with_inline_sheets_and_selectors(
        html.as_bytes(),
        &[],
        &Limits::DEFAULT,
        None,
        SupplyBudget::CANONICAL,
    )
}
fn legacy(css: &str, body: &str) -> StyleTree {
    let html = format!("<!doctype html><style>body{{margin:0}}{css}</style>{body}");
    parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT)
}
const GOV: &str = ".p::after{content:\"\";display:block;clear:both}";
const BODY: &str = "<div class=p><div class=f>t</div></div><div class=m></div>";
fn node<'a>(t: &'a StyleTree, tag: &str) -> usize {
    t.nodes.iter().position(|n| n.tag == tag).expect(tag)
}

#[test]
fn generated_box_is_the_last_child_with_its_own_cascaded_style() {
    let t = ext(&format!(".p{{color:#f00}}{GOV}"), BODY);
    assert!(!t.unsupported && !t.truncated);
    let g = node(&t, "::after");
    let p = t.nodes[g].parent.expect("originating element");
    assert_eq!(t.nodes[p].children.len(), 2, "f and ::after");
    let last = *t.nodes[p].children.last().unwrap();
    assert_eq!(t.nodes[last].tag, "::after");
    assert_eq!(t.nodes[last].parent, Some(p));
    assert!(t.nodes[last].children.is_empty());
    // Index follows the originating element's whole subtree (the text child).
    assert!(t.nodes.iter().enumerate().filter(|(_, n)| n.parent == Some(p)).all(|(i, _)| i <= last));
    let s = t.nodes[last].style;
    assert_eq!(s.display, kobo_web_document::computed_style::Display::Block);
    assert_eq!(s.positioned.clear, kobo_web_document::computed_style::Clear::Both);
    // Inherited from the originating element; the clear is NOT applied to the element.
    assert_eq!(s.color, 0xff_00_00);
    assert_eq!(t.nodes[p].style.positioned.clear, kobo_web_document::computed_style::Clear::None);
    assert_eq!(t.nodes[p].style.display, kobo_web_document::computed_style::Display::Block);
    assert_eq!(t.nodes.iter().filter(|n| n.tag == "::after").count(), 1);
}

#[test]
fn specificity_and_source_order_cascade_inside_the_pseudo_element() {
    // Later `display:block` beats earlier `display:none` at equal specificity; the
    // more specific selector's `display:none` beats a later lower-specificity block.
    let a = ext(&format!(".p::after{{display:none}}{GOV}"), BODY);
    assert!(a.nodes.iter().any(|n| n.tag == "::after"));
    let b = ext(&format!("div.p::after{{display:none}}{GOV}"), BODY);
    assert!(b.nodes.iter().all(|n| n.tag != "::after"), "specific display:none wins");
}

#[test]
fn no_box_for_none_normal_or_display_none() {
    for css in [
        ".p::after{content:none}",
        ".p::after{content:normal;display:block;clear:both}",
        ".p::after{content:\"\";display:none}",
        ".q::after{content:\"\";display:block;clear:both}",
    ] {
        let t = ext(css, BODY);
        assert!(!t.unsupported, "{css}");
        assert!(t.nodes.iter().all(|n| n.tag != "::after"), "{css}");
    }
}

#[test]
fn everything_else_fails_closed() {
    for css in [
        ".p::after{content:\"x\";display:block}",
        ".p::after{content:attr(href)}",
        ".p::before{content:\"x\";display:block}",
        ".p::before{content:attr(href)}",
        ".p::before::after{content:\"\"}",
        ".p:after{content:\"\";display:block}",
        ".p::first-line{color:#f00}",
        ".p{--c:both}.p::after{content:\"\";display:block;clear:var(--c)}",
        ".p::after{content:\"\";font-size:1.55rem}",
        ".p{content:\"\"}",
        ".f{content:\"\"}",
        ".p{content:normal}.f{content:\"\"}",
    ] {
        let t = ext(css, BODY);
        assert!(t.unsupported || t.truncated, "{css}");
    }
    // `content:normal` on an element is the initial value: accepted.
    assert!(!ext(".p{content:normal}", BODY).unsupported);
}

#[test]
fn legacy_and_default_entries_still_refuse_after() {
    let t = legacy(GOV, BODY);
    assert!(t.unsupported);
    assert!(t.nodes.iter().all(|n| n.tag != "::after"));
    let d = legacy(".p{content:\"\"}", BODY);
    assert!(d.unsupported, "content stays unknown outside the extended entry");
}

fn pixel(list: &DisplayList, x: i32, y: i32) -> Option<Rgb> {
    let mut hit = None;
    for c in list.commands() {
        if let Command::Fill { rect, color, .. } = c {
            if x >= rect.x && x < rect.x + rect.width as i32 && y >= rect.y && y < rect.y + rect.height as i32 {
                hit = Some(*color);
            }
        }
    }
    hit
}

/// `::before` mirrors `::after`: the generated box is the FIRST child of the
/// originating element, with its own cascaded style.
#[test]
fn before_box_is_the_first_child_with_its_own_cascaded_style() {
    const GOV_B: &str = ".p::before{content:\"\";display:block;clear:both}";
    let t = ext(&format!(".p{{color:#f00}}{GOV_B}"), BODY);
    assert!(!t.unsupported && !t.truncated);
    let g = node(&t, "::before");
    let p = t.nodes[g].parent.expect("originating element");
    assert_eq!(t.nodes[p].children.len(), 2, "::before and f");
    let first = *t.nodes[p].children.first().unwrap();
    assert_eq!(t.nodes[first].tag, "::before");
    assert_eq!(t.nodes[first].parent, Some(p));
    // Index PRECEDES the originating element's subtree.
    assert!(
        first < t.nodes[p].children[1],
        "before index precedes real child"
    );
    let s = t.nodes[first].style;
    assert_eq!(s.display, kobo_web_document::computed_style::Display::Block);
    assert_eq!(s.color, 0xff_00_00, "inherits from the originating element");
    // One element with BOTH: before, real children, after.
    let t = ext(&format!("{GOV_B}{GOV}"), BODY);
    let p = node(&t, "div");
    let tags: Vec<&str> = t.nodes[p]
        .children
        .iter()
        .map(|&c| t.nodes[c].tag.as_str())
        .collect();
    assert_eq!(tags, ["::before", "div", "::after"]);
}

/// Pixel evidence of ORDER: a painted `::before` block sits ABOVE the real
/// content, pushing it down; the same page without the rule paints the real
/// content at the top.
#[test]
fn generated_before_boxes_stay_closed_to_both_painters() {
    // Mirror of the durable ::after review regression (E2 REVIEW FAIL c728c185):
    // a generated ordinary block is paintable by NEITHER painter. The same
    // confinement applies to ::before (box_tree generated flag covers both
    // families).
    let t = ext(
        ".p{width:400px;background:#ccc}.f{height:50px;background:#f00}.p::before{content:\"\";display:block;height:20px;background:#0f0}",
        "<div class=p><div class=f></div></div>",
    );
    assert!(!t.unsupported && !t.truncated);
    let bt = BoxTree::from_style(&t);
    assert!(
        paint_backgrounds(&bt, 785, 600).is_err(),
        "default painter closed"
    );
    assert!(
        paint_backgrounds_float_kernel(&bt, 785, 600).is_err(),
        "kernel painter closed"
    );
    // The kernel's cleared child is always the SECOND child of its parent;
    // a ::before is always the FIRST child, so no current kernel shape admits
    // a generated ::before: [cleared ::before, float] is refused by both.
    let k = ext(
        ".p{width:400px;background:#ccc}.f{float:left;width:100px;height:50px;background:#f00}.p::before{content:\"\";display:block;clear:both}",
        "<div class=p><div class=f></div></div>",
    );
    assert!(!k.unsupported && !k.truncated);
    let btk = BoxTree::from_style(&k);
    assert!(paint_backgrounds(&btk, 785, 600).is_err());
    assert!(
        paint_backgrounds_float_kernel(&btk, 785, 600).is_err(),
        "::before cannot be the kernel cleared child"
    );
    assert!(
        paint_backgrounds_generated_block(&btk, 785, 600).is_err(),
        "generated-block refuses any float/clear tree-globally"
    );
}

/// Kernel-B pixel evidence: the generated-block entry paints [::before, real,
/// ::after] in tree order with exact integer rect offsets and colors.
#[test]
fn generated_block_entry_paints_before_real_after_in_order() {
    let t = ext(
        ".p{width:200px;background:#ccc}.f{height:30px;background:#f00}\
         .p::before{content:\"\";display:block;height:20px;background:#0f0}\
         .p::after{content:\"\";display:block;height:20px;background:#00f}",
        "<div class=p><div class=f></div></div>",
    );
    assert!(!t.unsupported && !t.truncated);
    let bt = BoxTree::from_style(&t);
    let list = paint_backgrounds_generated_block(&bt, 400, 300).expect("generated-block paints");
    // Exact integer band boundaries (body margin 0 from the helper):
    // ::before y0..20 green, real y20..50 red, ::after y50..70 blue, .p grey
    // underneath, nothing past width 200 or height 70.
    assert_eq!(pixel(&list, 100, 0), Some(Rgb(0, 255, 0)));
    assert_eq!(pixel(&list, 100, 19), Some(Rgb(0, 255, 0)));
    assert_eq!(pixel(&list, 100, 20), Some(Rgb(255, 0, 0)));
    assert_eq!(pixel(&list, 100, 49), Some(Rgb(255, 0, 0)));
    assert_eq!(pixel(&list, 100, 50), Some(Rgb(0, 0, 255)));
    assert_eq!(pixel(&list, 100, 69), Some(Rgb(0, 0, 255)));
    assert_eq!(pixel(&list, 199, 10), Some(Rgb(0, 255, 0)));
    assert_eq!(pixel(&list, 200, 10), None, "no fill past the 200px parent");
    assert_eq!(pixel(&list, 100, 70), None, "no fill past the 70px parent");
    // Default and kernel painters stay closed to the same tree.
    assert!(paint_backgrounds(&bt, 400, 300).is_err());
    assert!(paint_backgrounds_float_kernel(&bt, 400, 300).is_err());
    // Symmetric: an ::after-only ordinary block paints under the same entry.
    let a = ext(
        ".p{width:200px;background:#ccc}.f{height:30px;background:#f00}.p::after{content:\"\";display:block;height:20px;background:#00f}",
        "<div class=p><div class=f></div></div>",
    );
    let la = paint_backgrounds_generated_block(&BoxTree::from_style(&a), 400, 300)
        .expect("after-only paints");
    assert_eq!(pixel(&la, 100, 29), Some(Rgb(255, 0, 0)));
    assert_eq!(pixel(&la, 100, 30), Some(Rgb(0, 0, 255)));
    // Transparent/absent generated background: ADMITTED as a no-op fill (the
    // .p background shows through the ::before band).
    let n = ext(
        ".p{width:200px;background:#ccc}.f{height:30px;background:#f00}.p::before{content:\"\";display:block;height:20px}",
        "<div class=p><div class=f></div></div>",
    );
    let ln =
        paint_backgrounds_generated_block(&BoxTree::from_style(&n), 400, 300).expect("no-op admit");
    assert_eq!(
        pixel(&ln, 100, 10),
        Some(Rgb(204, 204, 204)),
        "empty generated band shows the parent background"
    );
    assert_eq!(pixel(&ln, 100, 20), Some(Rgb(255, 0, 0)));
}

/// Kernel-B refusal pins: every gate of the contract fails closed.
#[test]
fn generated_block_entry_refusal_pins() {
    let base = ".p{width:200px;background:#ccc}.f{height:30px;background:#f00}";
    let body = "<div class=p><div class=f></div></div>";
    let gen = ".p::before{content:\"\";display:block;height:20px;background:#0f0}";
    let refuses = |css: &str, body: &str, why: &str| {
        let t = ext(css, body);
        assert!(!t.unsupported && !t.truncated, "{why}: tree must parse");
        assert!(
            paint_backgrounds_generated_block(&BoxTree::from_style(&t), 400, 300).is_err(),
            "{why}"
        );
    };
    // float/clear anywhere in the tree (global refusal).
    refuses(
        &format!(".p{{width:200px;background:#ccc}}.f{{float:left;width:100px;height:30px;background:#f00}}{gen}"),
        body,
        "float present anywhere refuses",
    );
    // nonzero margin on the generated box (zero-edges).
    refuses(
        &format!("{base}{gen}").replace("background:#0f0", "background:#0f0;margin-left:1px"),
        body,
        "generated nonzero margin refuses",
    );
    // min-width clamp on the generated box (R2).
    refuses(
        &format!("{base}{gen}").replace("background:#0f0", "background:#0f0;min-width:10px"),
        body,
        "generated min-width refuses",
    );
    // retained text sibling (visible).
    refuses(
        &format!("{base}{gen}"),
        "<div class=p>x<div class=f></div></div>",
        "text sibling refuses",
    );
    // Source-grounded whitespace rule: whitespace-only text is DROPPED at box
    // construction (never a retained sibling box, same as every painter), so
    // the tree admits and the order is intact; any RETAINED text refuses
    // (covered above). Assert the structure, not just the paint result.
    let ws = ext(
        &format!("{base}{gen}"),
        "<div class=p> <div class=f></div></div>",
    );
    assert!(!ws.unsupported && !ws.truncated);
    let wsb = BoxTree::from_style(&ws);
    let p_box = wsb
        .boxes
        .iter()
        .position(|b| b.children.len() == 2)
        .expect("originating box");
    assert!(
        wsb.boxes[p_box]
            .children
            .iter()
            .all(|&c| wsb.boxes[c].text.is_none()),
        "no retained whitespace text box under the originating element"
    );
    let wl =
        paint_backgrounds_generated_block(&wsb, 400, 300).expect("whitespace dropped, admitted");
    assert_eq!(pixel(&wl, 100, 10), Some(Rgb(0, 255, 0)));
    assert_eq!(pixel(&wl, 100, 30), Some(Rgb(255, 0, 0)));
    // auto (absent) height on the generated box.
    refuses(
        &format!("{base}.p::before{{content:\"\";display:block;background:#0f0}}"),
        body,
        "generated auto height refuses",
    );
    // non-plain ancestor (relative parent).
    refuses(
        &format!(".p{{position:relative;width:200px;background:#ccc}}.f{{height:30px;background:#f00}}{gen}"),
        body,
        "relative ancestor refuses",
    );
    // R1: the root itself must be Block/Block/Static.
    refuses(
        &format!("html{{position:relative}}{base}{gen}"),
        body,
        "relative root refuses",
    );
}

/// Chrome 156 floatkernel-e2/ctx a1-a6 (whole-image, 0 diffs) and b6 (4px ::after).
#[test]
fn explicit_painter_draws_the_clearfix_pseudo_element_layout() {
    let colors = ".p{width:400px;background:#ccc}.f{float:left;width:100px;height:50px;background:#f00}.m{width:400px;height:20px;background:#880}";
    let body = "<div class=p><div class=f></div></div><div class=m></div>";
    let t = ext(&format!("{colors}{GOV}"), body);
    let bt = BoxTree::from_style(&t);
    let list = paint_backgrounds_float_kernel(&bt, 785, 600).expect("pseudo clearfix");
    assert_eq!(pixel(&list, 10, 49), Some(Rgb(255, 0, 0)));
    assert_eq!(pixel(&list, 300, 49), Some(Rgb(204, 204, 204)), "parent holds the float");
    assert_eq!(pixel(&list, 300, 50), Some(Rgb(136, 136, 0)), "next block at y50");
    assert!(paint_backgrounds(&bt, 785, 600).is_err(), "default painter stays closed");
    // Without the pseudo-element the float escapes the parent: not a kernel, refused.
    let no = ext(colors, body);
    assert!(paint_backgrounds_float_kernel(&BoxTree::from_style(&no), 785, 600).is_err());
    // The same document on the legacy entry is refused at intake.
    let l = legacy(&format!("{colors}{GOV}"), body);
    assert!(l.unsupported);
    // A nonzero-height pseudo box (b6) lays out as the nonzero cleared kernel.
    let h = ext(&format!("{colors}{GOV}.p::after{{height:4px}}"), body);
    let list = paint_backgrounds_float_kernel(&BoxTree::from_style(&h), 785, 600).expect("b6");
    assert_eq!(pixel(&list, 300, 54), Some(Rgb(136, 136, 0)), "parent 54 tall");
    assert_eq!(pixel(&list, 300, 53), Some(Rgb(204, 204, 204)));
    // clear:none on the pseudo box is not a kernel (uncleared zero height): refused.
    let n = ext(&format!("{colors}{GOV}.p::after{{clear:none}}"), body);
    assert!(paint_backgrounds_float_kernel(&BoxTree::from_style(&n), 785, 600).is_err());
}

/// Review-found defect fixed at 6:20: generated boxes were counted against the DOM
/// node cap, so a document that retains every DOM node plus generated boxes was
/// truncated (the retained GOV cookie fixture: 46 DOM nodes, truncated=true,
/// RetainedNodes=1). The cap is now DOM nodes plus generated boxes.
#[test]
fn generated_boxes_do_not_trip_the_dom_node_cap() {
    let body: String = (0..8).map(|_| "<div class=p></div>").collect();
    let t = ext(GOV, &body);
    assert!(!t.truncated && !t.unsupported, "{:?}", t.diagnostics);
    assert_eq!(t.nodes.iter().filter(|n| n.tag == "::after").count(), 8);
    // The bound still holds: more generated boxes than the fixed cap truncates.
    let many: String = (0..70).map(|_| "<div class=p></div>").collect();
    let big = ext(GOV, &many);
    assert!(big.truncated);
}

/// Review counterexamples (E2 REVIEW FAIL c728c185), kept as durable regressions.
#[test]
fn review_counterexamples_replaced_rem_and_ordinary_painters() {
    let both_refuse = |css: &str, body: &str| {
        let t = ext(css, body);
        let bt = BoxTree::from_style(&t);
        let k = paint_backgrounds_float_kernel(&bt, 785, 600);
        let d = paint_backgrounds(&bt, 785, 600);
        assert!(t.unsupported || (k.is_err() && d.is_err()), "{css}");
        (t.unsupported, k.is_ok(), d.is_ok())
    };
    // 1. replaced / void originating elements generate no ::after in Chrome: refused.
    for tag in ["img", "input", "br", "hr", "textarea", "select", "button"] {
        let t = ext(
            &format!("{tag}{{display:block;width:400px;height:10px;background:#ccc}}{tag}::after{{content:\"\";display:block;width:100px;height:50px;background:#f00}}"),
            &format!("<{tag}>"),
        );
        assert!(t.unsupported, "{tag} originating element refused");
    }
    // 2. generated ordinary block (no float): refused by BOTH painters.
    let (_, k, d) = both_refuse(
        "div{width:400px;height:10px;background:#ccc}div::after{content:\"\";display:block;width:100px;height:50px;background:#f00}",
        "<div></div>",
    );
    assert!(!k && !d, "generated ordinary block is not paintable by any painter");
    // 3. rem margin on the pseudo box is resolved by the shared element path, never left
    //    unresolved: either whole-px resolved (16px) or refused; never auto-like.
    let t = ext(
        ".p{width:400px;height:10px;background:#ccc}.p::after{content:\"\";display:block;width:100px;height:50px;margin-left:1rem;background:#f00}",
        "<div class=p></div>",
    );
    let bt = BoxTree::from_style(&t);
    assert!(paint_backgrounds(&bt, 785, 600).is_err());
    assert!(paint_backgrounds_float_kernel(&bt, 785, 600).is_err());
    // 4. unrepresentable / zero values on the pseudo box refuse like an element's own.
    for css in [
        ".p::after{content:\"\";display:block;clear:both;font-size:0}",
        ".p::after{content:\"\";display:block;clear:both;line-height:0}",
        ".p::after{content:\"\";display:block;clear:both;padding-top:1.55rem}",
        ".p::after{content:\"\";display:block;clear:both;margin-left:1.55rem}",
    ] {
        assert!(ext(css, BODY).unsupported, "{css}");
    }
    // 5. declaration bound: a pseudo rule set above the element bound fails closed.
    let many: String = (0..300).map(|_| ".p::after{content:\"\";display:block;clear:both;width:1px}").collect();
    assert!(ext(&many, BODY).truncated || ext(&many, BODY).unsupported);
}

/// Scoped proof for the NEW admitted behavior: rem on the pseudo box resolves through the
/// same helper as an element (root 16px). Whole-px results compute exactly; non-whole
/// results refuse. Computed values are compared with the equivalent px declaration.
#[test]
fn pseudo_rem_resolves_like_the_equivalent_px_and_unrepresentable_refuses() {
    let k = "content:\"\";display:block;clear:both;";
    let pairs = [("margin-left:1rem", "margin-left:16px"), ("padding-top:2rem", "padding-top:32px"), ("font-size:1.5rem", "font-size:24px")];
    for (rem, px) in pairs {
        let a = ext(&format!(".p::after{{{k}{rem}}}"), BODY);
        let b = ext(&format!(".p::after{{{k}{px}}}"), BODY);
        assert!(!a.unsupported && !b.unsupported, "{rem}");
        let sa = &a.nodes[node(&a, "::after")].style;
        let sb = &b.nodes[node(&b, "::after")].style;
        assert_eq!(format!("{sa:?}"), format!("{sb:?}"), "{rem} == {px}");
    }
    for bad in ["margin-left:1.55rem", "padding-top:1.55rem", "font-size:1.55rem"] {
        assert!(ext(&format!(".p::after{{{k}{bad}}}"), BODY).unsupported, "{bad}");
    }
}

/// Budget contracts: the declaration boundary equals an element's (the 256th matched
/// declaration refuses, the 255th is accepted) and match work is charged once per rule.
#[test]
fn pseudo_budget_contracts_match_the_element_path() {
    let decl = |n: usize| (0..n).map(|i| format!("{}:1px;", ["width", "height", "margin-left", "margin-top", "margin-right", "margin-bottom", "padding-left", "padding-top"][i % 8])).collect::<String>();
    // n declarations in ONE rule per path.
    for n in [255_usize, 256] {
        let el = ext(&format!(".m{{{}}}", decl(n)), BODY);
        let ps = ext(&format!(".p::after{{content:\"\";display:block;clear:both;{}}}", decl(n - 3)), BODY);
        // pseudo carries content+display+clear (3) plus n-3 = n declarations.
        assert_eq!(el.truncated, ps.truncated, "boundary n={n}");
        assert_eq!(n == 256, el.truncated, "n={n}");
    }
    // Match work is charged per expanded declaration-rule per element, identically for a
    // pseudo declaration and an ordinary one (3 declarations: same increment, no double charge).
    let base = ext("", BODY).diagnostics.match_attempts;
    let ord = ext(".zz{width:1px;height:1px;margin-left:1px}", BODY).diagnostics.match_attempts;
    let pse = ext(".q::after{content:\"\";display:block;clear:both}", BODY).diagnostics.match_attempts;
    assert_eq!(ord - base, pse - base, "3 ordinary vs 3 pseudo declarations");
}

/// Private percent-guard candidate (UNREVIEWED): default entry + default background
/// painter refuse percent widths that are fractional or float32-holes; exact ones paint.
#[test]
fn default_background_painter_percent_guard() {
    let case = |basis: u32, w: &str| {
        let html = format!("<!doctype html><style>body{{margin:0}}.c{{width:{basis}px}}.t{{width:{w};height:10px;background:#f00}}</style><div class=c><div class=t></div></div>");
        let t = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        paint_backgrounds(&BoxTree::from_style(&t), 785, 600).is_ok()
    };
    for (b, w) in [(201, "50%"), (333, "50%"), (500, "0.5%"), (641, "0.5%"), (785, "12.5%"), (250, "65.2%"), (400, "66.67%"), (400, "66.6667%")] {
        assert!(!case(b, w), "{b} {w} refused");
    }
    for (b, w) in [(200, "50%"), (400, "25%"), (800, "12.5%"), (400, "100%")] {
        assert!(case(b, w), "{b} {w} admitted");
    }
}

/// Variant D's own whole-px test is exact-rational only; the shared WidthPass audit must
/// also refuse the float32 hole (250px * 65.2% = 162.984375 in Chrome) for the kernel.
#[test]
fn float_kernel_percent_hole_refused_exact_admitted() {
    let k = |basis: u32, w: &str| {
        let html = format!("<!doctype html><style>body{{margin:0}}.p{{width:{basis}px;background:#ccc}}.f{{float:left;width:{w};height:50px;background:#f00}}.m{{width:{basis}px;height:20px;background:#880}}</style><div class=p><div class=f></div></div><div class=m></div>");
        let full = html.replace("<style>", "<style>.p::after{content:\"\";display:block;clear:both}");
        let t = parse_style_tree_with_inline_sheets_and_selectors(full.as_bytes(), &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        paint_backgrounds_float_kernel(&BoxTree::from_style(&t), 785, 600).is_ok()
    };
    assert!(!k(250, "65.2%"), "float32 hole refused");
    assert!(k(200, "50%"), "exact admitted");
}

/// Consumer audit (private percent-guard candidate, UNREVIEWED), CURRENT-STATE regression:
/// on the default background painter, percent-bearing flex items and absolute-positioned
/// boxes are refused by structural checks (not by the percent helper); flex min/max-width, even in
/// px, is refused. This pins today's refusals; it does not claim percent support or that
/// other painters (text bridge) were probed here.
#[test]
fn flex_and_absolute_percent_are_currently_refused_by_the_default_background_painter() {
    let flex = |item: &str| {
        let html = format!("<!doctype html><style>body{{margin:0}}.f{{display:flex;width:400px}}.i{{height:10px;background:#f00;{item}}}.j{{height:10px;width:100px;background:#00f}}</style><div class=f><div class=i></div><div class=j></div></div>");
        let t = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        paint_backgrounds(&BoxTree::from_style(&t), 785, 600).is_ok()
    };
    assert!(flex("width:100px"), "control: px flex item admitted");
    for item in ["max-width:50%", "min-width:50%", "max-width:100px", "width:50%", "flex-basis:50%", "margin-left:25%"] {
        assert!(!flex(item), "{item}");
    }
    // CORRECTED (reviewer 07:47): the earlier version asserted absolute boxes refuse, using
    // an empty body, where the refusal was incidental (zero-height body), NOT an absolute
    // ban. Here a normal 400x20 sibling makes the page paintable, a px absolute box is the
    // positive control (paints), and percent-width absolute boxes are refused by the
    // positioned-support check in from_style (width must be Px unless a zero container),
    // which is independent of the percent helper. Basis 400 (body) differs from the
    // positioned containing block (viewport 785), so 25%/50% are exact vs the body but not
    // vs the viewport: they must NOT paint.
    let abs = |w: &str| {
        let html = format!("<!doctype html><style>body{{margin:0;width:400px}}.n{{width:400px;height:20px;background:#880}}.a{{position:absolute;width:{w};height:10px;background:#f00}}</style><div class=n></div><div class=a></div>");
        let t = parse_style_tree(html.as_bytes(), &[], &Limits::DEFAULT);
        paint_backgrounds(&BoxTree::from_style(&t), 785, 600).is_ok()
    };
    assert!(abs("100px"), "positive control: px absolute paints beside a normal block");
    for w in ["25%", "50%", "100%", "65.2%", "0%"] {
        assert!(!abs(w), "abs {w}");
    }
}
