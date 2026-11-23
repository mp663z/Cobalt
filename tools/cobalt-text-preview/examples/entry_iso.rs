//! FL65 constructed entry-isolation (synthetic pages only, measurement-only): for each one-rule CSS case, whether four PARSE entries
//! keep or refuse the page. Parse-stage only: says nothing about layout/paint support. No admission change.
use kobo_web_document::*;
fn main() {
    let cases: Vec<&str> = vec![
        "p{color:red}", "p{foo-bar:1}", "p{color:var(--a)}", ":root{--a:red}",
        "p{background-color:#eee}", "p{background-color:rgb(0 0 0)}", "p{display:block}", "p{display:inline-block}", "p{display:flex}", "p{display:none}",
        "p{font-size:16px}", "p{font-size:1.2em}", "p{font-size:90%}", "p{width:100px}", "p{width:50%}", "p{width:auto}", "p{position:relative}", "p{position:absolute}", "p{top:0}",
        "p{border-radius:4px}", "p{box-shadow:0 0 1px #000}", "p{content:\"x\"}", "p{cursor:pointer}", "p{font-family:sans-serif}", "p{font-family:Arial,sans-serif}",
        "p{list-style:none}", "p{overflow:hidden}", "p{overflow-x:hidden}", "p{text-decoration:none}", "p{text-decoration:underline}", "p{transform:none}", "p{transition:none}",
        "p{vertical-align:top}", "p{white-space:nowrap}", "p{z-index:1}", "p{align-self:center}", "p{flex-wrap:wrap}", "p{min-height:10px}", "p{border:1px solid #000}", "p{border-color:#000}",
        "p:hover{color:red}", "p:first-child{color:red}", "p::before{content:\"x\"}", "p[a]{color:red}", "a:link{color:red}", "p:not(.x){color:red}", "p:lang(en){color:red}", "p+p{color:red}", "p>b{color:red}",
        "@media (min-width:600px){p{color:red}}", "@media (prefers-color-scheme:dark){p{color:red}}", "@media print{p{color:red}}", "@font-face{font-family:x}",
    ];
    let m = MediaContext::new(800);
    let b = SupplyBudget::CANONICAL;
    let l = Limits::DEFAULT;
    println!("{:<46} {:<22} {:<22} {:<22} {:<22}", "case", "E1 with_supply", "E2 inline+selectors", "E3 supply+textdeco", "E4 pinned_family");
    for c in &cases {
        let html = format!("<!doctype html><html><head><style>{}</style></head><body><p>Hello <b>w</b> <a href=\"#\">l</a></p></body></html>", c);
        let h = html.as_bytes();
        let r = |t: style_tree::StyleTree| if t.unsupported || t.truncated { format!("REFUSE {:?}", t.diagnostics.first_reason()) } else { "keep".to_string() };
        let e1 = r(parse_style_tree_with_supply(h, &[], &l, m, b));
        let e2 = r(parse_style_tree_with_inline_sheets_and_selectors(h, &[], &l, m, b));
        let e3 = r(parse_style_tree_with_supply_and_text_decoration(h, &[], &l, m, b));
        let e4 = r(parse_style_tree_with_pinned_font_family(h, &[], &l, m, b));
        println!("{:<46} {:<22} {:<22} {:<22} {:<22}", c.chars().take(45).collect::<String>(), e1, e2, e3, e4);
    }
}
