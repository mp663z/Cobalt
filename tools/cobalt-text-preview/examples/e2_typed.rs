//! FL67 (v2: baseline is a non-matching q{color:red} so node indices align; v1 output retained as e2_typed_v1_confounded.out) (synthetic) typed-vs-no-op probe on E2 (parse_style_tree_with_inline_sheets_and_selectors): does a kept one-rule change
//! the <p> node's Computed vs the no-rule baseline (TYPED) or leave it identical (NO-OP/ignored at parse)? Parse-stage only. Includes non-initial values.
use kobo_web_document::*;
fn tree(css: &str) -> style_tree::StyleTree {
    let html = format!("<!doctype html><html><head><style>{}</style></head><body><p>Hello <b>w</b></p><div>d</div></body></html>", css);
    parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(), &[], &Limits::DEFAULT, MediaContext::new(800), SupplyBudget::CANONICAL)
}
fn main() {
    let base = tree("q{color:red}");
    let bp: Vec<String> = base.nodes.iter().map(|n| format!("{:?}", n.style)).collect();
    let cases = ["p{position:relative}","p{cursor:pointer}","p{list-style:none}","p{list-style:disc}","p{overflow:hidden}","p{overflow:scroll}","p{overflow-x:hidden}","p{transform:none}","p{transform:rotate(1deg)}","p{transition:none}","p{transition:all 1s}","p{white-space:nowrap}","p{white-space:pre}","p{min-height:10px}","p{min-height:0}","p:first-child{color:red}","p[a]{color:red}","p+p{color:red}","p{color:red}","p{position:absolute}","p{display:flex}","p{width:50%}"];
    println!("{:<30} {:<8} {}", "case", "parse", "p/other nodes whose Computed differs from no-rule baseline");
    for c in cases {
        let t = tree(c);
        if t.unsupported || t.truncated { println!("{:<30} REFUSE {:?}", c, t.diagnostics.first_reason()); continue; }
        let diff: Vec<String> = t.nodes.iter().enumerate().filter(|(i,n)| bp.get(*i).map(|b| *b != format!("{:?}", n.style)).unwrap_or(true)).map(|(i,n)| format!("{}#{}", n.tag, i)).collect();
        println!("{:<30} keep    {}", c, if diff.is_empty() { "NONE (no-op at Computed)".to_string() } else { diff.join(",") });
    }
}
