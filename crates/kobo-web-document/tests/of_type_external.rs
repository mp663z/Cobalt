//! Public entry pins, standards-mode. No corpus paint/readiness claim.
use kobo_web_document::{Limits,SupplyBudget};
use kobo_web_document::style_tree::StyleTree;
fn run(css:&str,body:&str)->StyleTree{
 let html=format!("<!doctype html><style>{css}</style>{body}");
 kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(),&[],&Limits::DEFAULT,None,SupplyBudget::CANONICAL)
}
#[test]
fn typed_cascade_own_type_not_token_bonus_and_combinators(){
 let body="<div><p class=a>x</p><span></span><p class='a b'>y</p></div>";
 let t=run("p:last-of-type{color:red}.a{color:blue}",body);
 assert!(!t.unsupported && !t.truncated);
 let ps:Vec<_>=t.nodes.iter().filter(|n|n.tag=="p").map(|n|n.style.color).collect();assert_eq!(ps,[0x0000ff,0xff0000]);
 let t=run(":not(:first-of-type){color:red}.a{color:blue}",body);
 let ps:Vec<_>=t.nodes.iter().filter(|n|n.tag=="p").map(|n|n.style.color).collect();assert_eq!(ps,[0x0000ff,0x0000ff],"no inferred type or not token bonus; equal specificity later wins");
 let t=run("div:first-of-type > p:nth-of-type(2){color:red}",body);assert!(!t.unsupported && !t.truncated);
 assert_eq!(t.nodes.iter().filter(|n|n.tag=="p").map(|n|n.style.color).collect::<Vec<_>>(),[0,0xff0000]);
}
#[test]
fn typed_actual_name_scan_exhaustion_truncates_and_base_refuses(){
 let body=format!("<div>{}</div>","<p>x</p>".repeat(160));
 let t=run("p:last-of-type{color:red}",&body);
 assert!(t.truncated,"actual namespace/local bytes + sibling visits exceed4096");
 let html="<!doctype html><style>p:first-of-type{color:red}</style><p>x</p>";
 let t=kobo_web_document::parse_style_tree(html.as_bytes(),&[],&Limits::DEFAULT);
 assert!(t.unsupported);
}
#[test]
#[ignore="measurement: OF_TYPE_LINES original62-line proposal set"]
fn public_original62_inventory(){
 let text=std::fs::read_to_string(std::env::var("OF_TYPE_LINES").unwrap()).unwrap();
 let state=kobo_web_document::SnapshotState {history:Some(kobo_web_document::HistoryState::FreshNoHistory),input:Some(kobo_web_document::InputState::QuiescentCapture),focus:Some(kobo_web_document::FocusState::UnfocusedInitial),target:Some(kobo_web_document::TargetState::FragmentFreeCapture)};
 let mut refused=Vec::new();let mut total=0;
 for (ix,line) in text.lines().enumerate(){
  total+=1;let html=format!("<!doctype html><style>{line}{{color:red}}</style><div class=a><p>x</p><span></span><p>y</p></div>");
  let t=kobo_web_document::parse_style_tree_with_state(html.as_bytes(),&[],&Limits::DEFAULT,None,SupplyBudget::CANONICAL,&state);
  let admitted=!t.unsupported && !t.truncated;if !admitted{refused.push(ix+1);}
  println!("OF-TYPE-PUBLIC line={} admitted={} truncated={} first={:?}",ix+1,admitted,t.truncated,t.diagnostics.first_reason());
 }
 println!("OF-TYPE-PUBLIC total={} admitted={} refused={:?}",total,total-refused.len(),refused);
 assert_eq!(total,62);assert!(refused.is_empty());
}

#[test]
fn partial_dom_never_becomes_complete_for_type_relative_negation(){
 let html="<!doctype html><style>p:not(:last-of-type){color:red}</style><div><p>x</p><p>y</p></div>";
 let limits=Limits {max_input_bytes:html.len()-10,..Limits::DEFAULT};
 let t=kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(html.as_bytes(),&[],&limits,None,SupplyBudget::CANONICAL);
 assert!(t.truncated,"partial intake remains a whole-tree refusal");
}
