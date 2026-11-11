//! Explicit browser-compatible Selectors3 empty, NOT draft4 whitespace relaxation.
use kobo_web_document::{Limits,SupplyBudget};
use kobo_web_document::style_tree::StyleTree;
fn run(css:&str,body:&str,lim:&Limits)->StyleTree{let h=format!("<!doctype html><style>{css}</style>{body}");kobo_web_document::parse_style_tree_with_inline_sheets_and_selectors(h.as_bytes(),&[],lim,None,SupplyBudget::CANONICAL)}
#[test]
fn complete_dom_cascade_combinators_and_wrapper_composition(){
 let css="p:empty{color:red}p:not(:empty){color:blue}div:where(:not(:empty)) > p:empty + p{color:green}";let t=run(css,"<div><p></p><p> </p><p><!--c--></p></div>",&Limits::DEFAULT);assert!(!t.unsupported && !t.truncated);assert_eq!(t.nodes.iter().filter(|n|n.tag=="p").map(|n|n.style.color).collect::<Vec<_>>(),[0xff0000,0x008000,0xff0000]);
 let t=run(":where(p:empty){color:red}p{color:blue}","<p></p>",&Limits::DEFAULT);assert_eq!(t.nodes.iter().find(|n|n.tag=="p").unwrap().style.color,0x0000ff);
}
#[test]
fn actual_text_cap_loss_emptylooking_retained_candidate_is_refused(){
 let lim=Limits{max_text_bytes:0,..Limits::DEFAULT};
 for css in ["p:empty{color:red}","p:not(:empty){color:red}","p:where(:empty){color:red}"]{
  let t=run(css,"<p>x</p>",&lim);assert!(t.truncated);assert!(t.diagnostics.dropped_text);assert!(t.diagnostics.count(kobo_web_document::style_diagnostics::RefusalReason::DroppedText)>0);let p=t.nodes.iter().find(|n|n.tag=="p").unwrap();assert!(p.children.is_empty(),"retained p is actually empty-looking after text loss");
 }
}
#[test]
fn input_node_truncation_under_empty_wrappers_refuses(){
 for css in ["p:empty{color:red}","p:not(:empty){color:red}","p:where(:empty){color:red}"]{
  for lim in [Limits{max_input_bytes:55,..Limits::DEFAULT},Limits{max_nodes:1,..Limits::DEFAULT}]{let t=run(css,"<div><section><p>x</p></section></div>",&lim);assert!(t.truncated || t.unsupported,"partial delivered DOM must refuse css={css} maxbytes={} maxnodes={} maxdepth={} flags={}/{}",lim.max_input_bytes,lim.max_nodes,lim.max_depth,t.unsupported,t.truncated);}
 }
}
#[test]
fn real_raw_comment_scan_exhaustion_and_base_refusal(){
 let body=format!("<p>{}</p>","<!--c-->".repeat(4096));let t=run("p:empty{color:red}",&body,&Limits::DEFAULT);assert!(t.truncated);assert!(t.diagnostics.count(kobo_web_document::style_diagnostics::RefusalReason::MatchWork)>0);
 let h="<!doctype html><style>p:empty{color:red}</style><p></p>";assert!(kobo_web_document::parse_style_tree(h.as_bytes(),&[],&Limits::DEFAULT).unsupported);
}
#[test]
#[ignore="measurement: EMPTY_LINES original11"]
fn public_original11_inventory(){let text=std::fs::read_to_string(std::env::var("EMPTY_LINES").unwrap()).unwrap();let mut total=0;let mut refused=Vec::new();for(ix,l)in text.lines().enumerate(){total+=1;let h=format!("<!doctype html><style>{l}{{color:red}}</style><div><p></p><p>x</p></div>");let s=kobo_web_document::SnapshotState{history:Some(kobo_web_document::HistoryState::FreshNoHistory),input:Some(kobo_web_document::InputState::QuiescentCapture),focus:Some(kobo_web_document::FocusState::UnfocusedInitial),target:Some(kobo_web_document::TargetState::FragmentFreeCapture)};let t=kobo_web_document::parse_style_tree_with_state(h.as_bytes(),&[],&Limits::DEFAULT,None,SupplyBudget::CANONICAL,&s);let yes=!t.unsupported && !t.truncated;if !yes{refused.push(ix+1);}println!("EMPTY-PUBLIC line={} admitted={} truncated={} first={:?}",ix+1,yes,t.truncated,t.diagnostics.first_reason());}println!("EMPTY-PUBLIC total={} admitted={} refused={:?}",total,total-refused.len(),refused);assert_eq!(total,11);assert!(refused.is_empty());}

#[test]
fn depth_config_unused_here_complete150chain_sentinel_and_empty_leaf(){
 // Existing API limitation, NOT a working truncation gate or depth-safety proof.
 let n=150;let tags=["div","section","article"];let opens=(0..n).map(|i|format!("<{} id=d{}>",tags[i%3],i)).collect::<String>();let closes=(0..n).rev().map(|i|format!("</{}>",tags[i%3])).collect::<String>();let body=format!("{opens}<p id=sentinel>SENTINEL</p><aside id=leaf></aside>{closes}");
 let low=Limits{max_depth:1,..Limits::DEFAULT};let high=Limits{max_depth:10000,..Limits::DEFAULT};
 for empty in [":empty",":where(:empty)"]{
  let css=format!("{empty}{{color:red}}:not(:empty){{color:blue}}");let a=run(&css,&body,&low);let b=run(&css,&body,&high);assert!(!a.truncated && !a.unsupported);
  assert_eq!(a.nodes.iter().filter(|x|tags.contains(&x.tag.as_str())).count(),n);assert_eq!(a.nodes.iter().filter(|x|x.tag=="p").count(),1);assert_eq!(a.nodes.iter().filter(|x|x.tag=="aside").count(),1);
  let p=a.nodes.iter().find(|x|x.tag=="p").unwrap();assert_eq!(p.children.iter().filter_map(|&h|a.nodes[h].text.as_deref()).collect::<String>(),"SENTINEL");assert_eq!(p.style.color,0x0000ff);assert_eq!(a.nodes.iter().find(|x|x.tag=="aside").unwrap().style.color,0xff0000);
  let mut at=a.nodes.iter().position(|x|x.tag=="p").unwrap();let mut seen=Vec::new();while let Some(parent)=a.nodes[at].parent{if tags.contains(&a.nodes[parent].tag.as_str()){seen.push(a.nodes[parent].tag.as_str());assert_eq!(a.nodes[parent].style.color,0x0000ff);}at=parent;}assert_eq!(seen,(0..n).rev().map(|i|tags[i%3]).collect::<Vec<_>>(),"exact complete alternating parent chain");
  assert_eq!(format!("{:?}",a.nodes),format!("{:?}",b.nodes),"full StyledNodes+Computed identity");assert_eq!(format!("{:?}",a.diagnostics),format!("{:?}",b.diagnostics),"ALL diagnostics identity");assert_eq!((a.unsupported,a.truncated,a.quirks,a.ext,a.host_sheets_claimed),(b.unsupported,b.truncated,b.quirks,b.ext,b.host_sheets_claimed));assert!(a.families.as_ref().unwrap().is_empty() && b.families.as_ref().unwrap().is_empty(),"family contents empty; per-parse identity tokens are intentionally distinct");
 }
}
