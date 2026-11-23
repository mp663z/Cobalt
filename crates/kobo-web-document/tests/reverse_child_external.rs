//! Public reverse child-index pins. Not corpus admission/paint evidence.
use kobo_web_document::{Limits,SupplyBudget};
use kobo_web_document::style_tree::StyleTree;
fn run(css:&str,body:&str)->StyleTree{let html=format!("<!doctype html><style>{css}</style>{body}");kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(),&[],&Limits::DEFAULT,None,SupplyBudget::CANONICAL)}
#[test]
fn cascade_specificity_not_and_left_combinator(){
 let body="<div><p class=a>x</p><span></span><p class=a>y</p></div>";
 let t=run("p:nth-last-child(1){color:red}.a{color:blue}",body);assert!(!t.unsupported && !t.truncated);assert_eq!(t.nodes.iter().filter(|n|n.tag=="p").map(|n|n.style.color).collect::<Vec<_>>(),[0x0000ff,0xff0000]);
 let t=run(":not(:nth-last-child(1)){color:red}.a{color:blue}",body);assert!(!t.unsupported && !t.truncated);assert_eq!(t.nodes.iter().filter(|n|n.tag=="p").map(|n|n.style.color).collect::<Vec<_>>(),[0x0000ff,0x0000ff]);
 let t=run("div:nth-last-child(1) > p:nth-last-child(3) ~ p{color:red}",body);assert!(!t.unsupported && !t.truncated);assert_eq!(t.nodes.iter().filter(|n|n.tag=="p").map(|n|n.style.color).collect::<Vec<_>>(),[0,0xff0000]);
}
#[test]
fn real_reverse_scan_exhaustion_and_partial_intake_fail_closed(){
 let body=format!("<div>{}</div>","<p>x</p>".repeat(4096));let t=run("p:nth-last-child(1){color:red}",&body);assert!(t.truncated);assert!(t.diagnostics.count(kobo_web_document::style_diagnostics::RefusalReason::MatchWork)>0);
 let html="<!doctype html><style>p:not(:nth-last-child(1)){color:red}</style><div><p>x</p><p>y</p></div>";let lim=Limits{max_input_bytes:html.len()-10,..Limits::DEFAULT};let t=kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(),&[],&lim,None,SupplyBudget::CANONICAL);assert!(t.truncated);
 let t=kobo_web_document::parse_style_tree(html.as_bytes(),&[],&Limits::DEFAULT);assert!(t.unsupported);
}
#[test]
#[ignore="measurement: REVERSE_CHILD_LINES original5"]
fn public_original5_inventory(){
 let lines=std::fs::read_to_string(std::env::var("REVERSE_CHILD_LINES").unwrap()).unwrap();let mut refused=Vec::new();let mut total=0;
 for (ix,line) in lines.lines().enumerate(){total+=1;let t=run(&format!("{line}{{color:red}}"),"<div><p>x</p><span></span><p>y</p></div>");let yes=!t.unsupported && !t.truncated;if !yes{refused.push(ix+1);}println!("REVERSE-CHILD-PUBLIC line={} admitted={} truncated={} first={:?}",ix+1,yes,t.truncated,t.diagnostics.first_reason());}
 println!("REVERSE-CHILD-PUBLIC total={} admitted={} refused={:?}",total,total-refused.len(),refused);assert_eq!(total,5);assert!(refused.is_empty());
}
