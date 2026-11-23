//! U1 verification runner (S1 semantic computed + S2 fixed-child box geometry). Engine side only; the Chrome oracle and the
//! comparison are in private-evidence/ul1/u1impl (chrome_run.py, compare.py). Entry: extended-CSS opt-in (production path).
//! The required-props audit is INTACT in the S1 dump (CssDeclaration counts include it). S2 clears `unsupported` ONLY when every
//! refusal reason other than the audit's expected CssDeclaration count is zero (labelled TEST-ONLY audit bypass, not production).
use kobo_web_document::{box_tree::BoxTree, computed_style::{Display, Margin}, css_background::paint_backgrounds, display_list::Command,
    parse_style_tree_with_inline_sheets_and_selectors, style_diagnostics::RefusalReason, Limits, SupplyBudget};
fn m(x: Margin) -> String { match x { Margin::Px(p) => format!("{p}px"), Margin::Auto => "auto".into(), Margin::Percent(p) => format!("pct({p})"), Margin::Em(e) => format!("em({e})") } }
fn d(x: Display) -> &'static str { match x { Display::Block => "block", Display::Inline => "inline", Display::None => "none", Display::InlineBlock => "inline-block", Display::ListItem => "list-item", Display::Flex => "flex", Display::Table => "table", Display::TableRowGroup => "table-row-group", Display::TableRow => "table-row", Display::TableCell => "table-cell", Display::TableCaption => "table-caption", Display::TableColumn => "table-column" } }
#[test]
#[ignore = "env-driven evidence runner (needs U1_DIR and U2_OUT); run with --ignored on COPY fixtures"]
fn matrix() {
    let dir = std::env::var("U1_DIR").unwrap();
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("html")).collect();
    files.sort();
    let mut out = String::new();
    for p in files {
        let name = p.file_stem().unwrap().to_str().unwrap().to_string();
        let h = std::fs::read(&p).unwrap();
        let mut t = parse_style_tree_with_inline_sheets_and_selectors(&h, &[], &Limits::DEFAULT, None, SupplyBudget::CANONICAL);
        let mut other = 0usize;
        for r in RefusalReason::ALL { if r != RefusalReason::CssDeclaration { other += t.diagnostics.count(r); } }
        out += &format!("## {name}\nMETA unsupported={} cssdecl={} other_reasons={}\n", t.unsupported, t.diagnostics.count(RefusalReason::CssDeclaration), other);
        if name.starts_with('c') {
            for n in &t.nodes {
                if matches!(n.tag.as_str(), "dl" | "dt" | "dd") {
                    let s = &n.style;
                    out += &format!("{}|{}|{}px|{}|{}|{}|{}|{}\n", n.tag, d(s.display), s.font_size, m(s.margin_top), m(s.margin_bottom), m(s.margin_left), m(s.margin_right), format!("{:?}", s.direction).to_lowercase());
                }
            }
        } else {
            let audited = t.nodes.iter().filter(|n| matches!(n.tag.as_str(), "dl" | "dt" | "dd" | "ul" | "ol" | "blockquote")).count();
            let extra = t.diagnostics.count(RefusalReason::CssDeclaration).saturating_sub(audited);
            out += &format!("S2META audited_elements={audited} extra_cssdecl={extra}\n");
            if other == 0 && extra == 0 {
                t.unsupported = false; // TEST-ONLY audit bypass (see header)
                let bt = BoxTree::from_style(&t);
                if bt.unsupported { out += "BOXTREE_UNSUPPORTED\n"; }
                else { match paint_backgrounds(&bt, 800, 600) {
                    Err(e) => out += &format!("PAINT_ERR {e:?}\n"),
                    Ok(l) => { for c in l.commands() { if let Command::Fill { rect, color, .. } = c { out += &format!("{},{},{},{},{:02x}{:02x}{:02x}\n", rect.x, rect.y, rect.width, rect.height, color.0, color.1, color.2); } } }
                } }
            } else { out += "S2_NOT_RUN other_reasons_or_extra_cssdecl\n"; }
        }
    }
    std::fs::write(format!("{dir}/../{}_engine.txt", std::path::Path::new(&dir).file_name().unwrap().to_str().unwrap()), out).unwrap();
}
