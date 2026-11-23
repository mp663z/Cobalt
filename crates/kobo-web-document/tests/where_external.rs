//! Bounded single-compound where, not forgiving-list conformance or painter evidence.
use kobo_web_document::{Limits,SupplyBudget,SnapshotState,HistoryState,InputState,FocusState,TargetState};
use kobo_web_document::style_tree::StyleTree;
fn snap()->SnapshotState{SnapshotState{history:None,input:None,focus:None,target:None}}
fn run(css:&str,body:&str,s:&SnapshotState)->StyleTree{let html=format!("<!doctype html><style>{css}</style>{body}");kobo_web_document::parse_style_tree_with_state(html.as_bytes(),&[],&Limits::DEFAULT,None,SupplyBudget::CANONICAL,s)}
#[test]
fn computed_browser_color_oracle_7cases(){
 let html=include_str!("fixtures/where_cascade_oracle.html");let t=kobo_web_document::parse_style_tree_with_state(html.as_bytes(),&[],&Limits::DEFAULT,None,SupplyBudget::CANONICAL,&snap());assert!(!t.unsupported && !t.truncated,"{:?}",t.diagnostics);
 let ps:Vec<_>=t.nodes.iter().filter(|n|n.tag=="p").map(|n|n.style.color).collect();assert_eq!(ps,vec![0x0000ff;7]);
}
#[test]
fn recursive_state_preflight_and_autofocus_guard(){
 let css=".absent:where(:not(:focus)){color:red}";assert!(run(css,"<p>x</p>",&snap()).unsupported);
 let mut s=snap();s.input=Some(InputState::QuiescentCapture);assert!(run(css,"<p>x</p>",&s).unsupported,"wrong family does not cover focus");s.focus=Some(FocusState::UnfocusedInitial);assert!(!run(css,"<p>x</p>",&s).unsupported);assert!(run(css,"<p autofocus>x</p>",&s).unsupported);
 let css="a:where(:link):not(:where(:hover)){color:red}";assert!(run(css,"<a href=x>x</a>",&s).unsupported);s.history=Some(HistoryState::FreshNoHistory);assert!(!run(css,"<a href=x>x</a>",&s).unsupported);
 let css="p:where(:target){color:red}";assert!(run(css,"<p id=x>x</p>",&s).unsupported);s.target=Some(TargetState::Fragment("x".into()));let t=run(css,"<p id=x>x</p>",&s);assert!(!t.unsupported);assert_eq!(t.nodes.iter().find(|n|n.tag=="p").unwrap().style.color,0xff0000);
}
#[test]
fn charged_real_exhaustion_partial_dom_and_base_refusal(){
 let t=run("p:where([data-x*=aaaaab]){color:red}",&format!("<p data-x='{}'>x</p>","a".repeat(1000)),&snap());assert!(t.truncated);assert!(t.diagnostics.count(kobo_web_document::style_diagnostics::RefusalReason::MatchWork)>0);
 let html="<!doctype html><style>p:not(:where(:last-of-type)){color:red}</style><div><p>x</p><p>y</p></div>";let lim=Limits{max_input_bytes:html.len()-10,..Limits::DEFAULT};let t=kobo_web_document::parse_style_tree_with_state(html.as_bytes(),&[],&lim,None,SupplyBudget::CANONICAL,&snap());assert!(t.truncated);assert!(kobo_web_document::parse_style_tree(html.as_bytes(),&[],&Limits::DEFAULT).unsupported);
}
#[test]
#[ignore="measurement: WHERE_LINES original10"]
fn public_original10_inventory(){let text=std::fs::read_to_string(std::env::var("WHERE_LINES").unwrap()).unwrap();let s=SnapshotState{history:Some(HistoryState::FreshNoHistory),input:Some(InputState::QuiescentCapture),focus:Some(FocusState::UnfocusedInitial),target:Some(TargetState::FragmentFreeCapture)};let mut total=0;let mut refused=Vec::new();for(ix,line)in text.lines().enumerate(){total+=1;let t=run(&format!("{line}{{color:red}}"),"<div><a class=new href=x>x</a><span></span></div>",&s);let yes=!t.unsupported && !t.truncated;if !yes{refused.push(ix+1);}println!("WHERE-PUBLIC line={} admitted={} truncated={} first={:?}",ix+1,yes,t.truncated,t.diagnostics.first_reason());}println!("WHERE-PUBLIC total={} admitted={} refused={:?}",total,total-refused.len(),refused);assert_eq!(total,10);assert!(refused.is_empty());}
