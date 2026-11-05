use kobo_web_document::{parse_style_tree_with_inline_sheets_and_selectors as ext, Limits, SupplyBudget};
fn main(){
 let f=std::env::args().nth(1).unwrap(); let html=std::fs::read(&f).unwrap();
 let t=ext(&html,&[],&Limits::DEFAULT,None,SupplyBudget::CANONICAL);
 println!("unsupported={} truncated={} first={:?} nodes={}",t.unsupported,t.truncated,t.diagnostics.first_reason(),t.nodes.len());
 fn walk(t:&kobo_web_document::style_tree::StyleTree,i:usize,out:&mut Vec<usize>){ if t.nodes[i].text.is_none(){out.push(i);} for &c in &t.nodes[i].children{walk(t,c,out);} }
 let mut order=vec![]; for i in 0..t.nodes.len(){ if t.nodes[i].parent.is_none(){walk(&t,i,&mut order);} }
 for i in order { let n=&t.nodes[i]; println!("{}\t{:?}\t{:?}\t{:?}",n.tag,n.style.color,n.style.background_color,n.style.display); }
}
