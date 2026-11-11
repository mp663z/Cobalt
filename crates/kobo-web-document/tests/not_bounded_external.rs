//! Bounded negation: public opt-in API, standards-mode fixtures. Computed
//! cascade/matcher tests, not a rendered-page or corpus admission claim.
use kobo_web_document::{Limits, SnapshotState, SupplyBudget, TargetState, InputState, FocusState};
use kobo_web_document::style_tree::StyleTree;
use kobo_web_document::style_diagnostics::RefusalReason;
fn snapshot() -> SnapshotState { SnapshotState {history:None,input:None,focus:None,target:None} }
fn run(css: &str, body: &str, snap: &SnapshotState) -> StyleTree {
    let html=format!("<!doctype html><html><head><style>{css}</style></head><body>{body}</body></html>");
    kobo_web_document::parse_style_tree_with_state(html.as_bytes(),&[],&Limits::DEFAULT,None,SupplyBudget::CANONICAL,snap)
}
fn bg(t:&StyleTree,tag:&str)->Option<u32> {t.nodes.iter().find(|n|n.tag==tag).unwrap().style.background_color}
#[test]
fn competing_rule_specificity_id_class_type_and_no_token_bonus() {
    // Type in argument wins an otherwise equal class count, despite order.
    let t=run(".a:not(span){background-color:red}.a{background-color:blue}","<p class=a>x</p>",&snapshot());
    assert!(!t.unsupported);assert_eq!(bg(&t,"p"),Some(0xff0000));
    // No extra class increment for the :not token: equal rank, later wins.
    let t=run(".a:not(.b){background-color:red}.a.c{background-color:blue}","<p class='a c'>x</p>",&snapshot());
    assert!(!t.unsupported);assert_eq!(bg(&t,"p"),Some(0x0000ff));
    let t=run("p:not(#absent){background-color:red}.a.b.c{background-color:blue}","<p class='a b c'>x</p>",&snapshot());
    assert_eq!(bg(&t,"p"),Some(0xff0000));
}
#[test]
fn inner_state_target_and_enablement_rank_and_dependencies() {
    let css="p:not(:hover){background-color:red}.a{background-color:blue}";
    let t=run(css,"<p class=a>x</p>",&snapshot());
    assert!(t.unsupported);assert_eq!(t.diagnostics.count(RefusalReason::CssSelector),1);
    let mut snap=snapshot();snap.input=Some(InputState::QuiescentCapture);
    let t=run(css,"<p class=a>x</p>",&snap);
    assert!(!t.unsupported);assert_eq!(bg(&t,"p"),Some(0xff0000));
    let css="p:not(:disabled){background-color:red}.a{background-color:blue}";
    let t=run(css,"<p class=a>x</p>",&snapshot());
    assert!(!t.unsupported,"reason={:?} selector={} bg={:?}",t.diagnostics.first_reason(), t.diagnostics.count(RefusalReason::CssSelector),bg(&t,"p"));assert_eq!(bg(&t,"p"),Some(0xff0000));
    let css="p:not(:target){background-color:red}.a{background-color:blue}";
    assert!(run(css,"<p class=a>x</p>",&snapshot()).unsupported);
    let mut snap=snapshot();snap.target=Some(TargetState::FragmentFreeCapture);
    let t=run(css,"<p class=a>x</p>",&snap);
    assert!(!t.unsupported);assert_eq!(bg(&t,"p"),Some(0xff0000));
    snap.target=Some(TargetState::Fragment("t".into()));
    let t=run(css,"<p class=a id=t>x</p>",&snap);
    assert!(!t.unsupported);assert_eq!(bg(&t,"p"),Some(0x0000ff));
}
#[test]
fn separate_inner_focus_bound_and_autofocus_guard() {
    let css="p:focus:not(:focus-visible){background-color:red}p{background-color:blue}";
    let mut snap=snapshot();snap.focus=Some(FocusState::UnfocusedInitial);
    let t=run(css,"<p>x</p>",&snap);
    assert!(!t.unsupported);assert_eq!(bg(&t,"p"),Some(0x0000ff));
    let css="p:not(:focus-visible){background-color:red}p{background-color:blue}";
    let t=run(css,"<p>x</p>",&snap);
    assert!(!t.unsupported);assert_eq!(bg(&t,"p"),Some(0xff0000));
    let t=run(css,"<p autofocus>x</p>",&snap);
    assert!(t.unsupported,"existing autofocus guard covers inner focus");
}
#[test]
fn negated_attribute_exhaustion_truncates_and_base_refuses() {
    let body=format!("<p data-x='{}'>x</p>","a".repeat(1000));
    let css="p:not([data-x=z]){background-color:red}";
    let t=run(css,&body,&snapshot());
    assert!(t.truncated,"submatch precharge exhaustion propagates to tree gate");
    assert!(t.diagnostics.count(RefusalReason::MatchWork)>0);
    let html=format!("<!doctype html><style>{css}</style><p>x</p>");
    let t=kobo_web_document::parse_style_tree(html.as_bytes(),&[],&Limits::DEFAULT);
    assert!(t.unsupported);assert!(t.diagnostics.count(RefusalReason::CssSelector)>0);
}
#[test]
#[ignore = "measurement: NOT_LINES supplies the retained original 542-line set"]
fn public_production_inventory() {
    let lines=std::fs::read_to_string(std::env::var("NOT_LINES").unwrap()).unwrap();
    // Synthetic test-only guarantees are true of this static fixture. This
    // does not enroll a browser capture or imply any real caller guarantee.
    let snap=SnapshotState {history:Some(kobo_web_document::HistoryState::FreshNoHistory),
        input:Some(InputState::QuiescentCapture),focus:Some(FocusState::UnfocusedInitial),
        target:Some(TargetState::FragmentFreeCapture)};
    let mut admitted=0;let mut refused=Vec::new();
    for (ix,line) in lines.lines().enumerate() {
        let t=run(&format!("{}{{color:red}}",line.trim()),"<div class=a id=b><p>x</p></div>",&snap);
        let ok=!t.unsupported && !t.truncated;
        if ok { admitted+=1; } else {refused.push(ix+1);}
        println!("PUBLIC-NOT line={} admitted={} selector_refusals={} first={:?} truncated={}",
            ix+1,ok,t.diagnostics.count(RefusalReason::CssSelector),t.diagnostics.first_reason(),t.truncated);
    }
    println!("PUBLIC-NOT total={} admitted={} refused={:?}",admitted+refused.len(),admitted,refused);
    assert_eq!(admitted,536); assert_eq!(refused,[18,21,43,47,268,270]);
}
