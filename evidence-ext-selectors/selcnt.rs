use kobo_web_document::{parse_style_tree_with_supply as plain, parse_style_tree_with_inline_sheets as inl, parse_style_tree_with_inline_sheets_and_selectors as ext, style_diagnostics::RefusalReason as R, Limits, SupplyBudget};
fn b64(s:&str)->Option<Vec<u8>>{let mut o=vec![];let(mut a,mut n)=(0u32,0);for c in s.bytes(){let v=match c{b'A'..=b'Z'=>c-b'A',b'a'..=b'z'=>c-b'a'+26,b'0'..=b'9'=>c-b'0'+52,b'+'=>62,b'/'=>63,b'='=>break,_=>return None};a=(a<<6)|v as u32;n+=6;if n>=8{n-=8;o.push((a>>n) as u8);a&=(1<<n)-1;}}Some(o)}
fn main(){
 let f=std::env::args().nth(1).unwrap(); let html=std::fs::read(&f).unwrap(); let text=String::from_utf8_lossy(&html).into_owned(); let prefix="data:text/css;base64,";
 let mut sheets:Vec<Vec<u8>>=vec![]; let mut i=0;
 while let Some(p)=text[i..].find("<link"){let s=i+p;let e=text[s..].find('>').map(|x|s+x).unwrap_or(text.len());let tag=&text[s..e];
  if tag.contains("stylesheet"){ if let Some(h)=tag.find("href=\""){let v=&tag[h+6..];if let Some(q)=v.find('"'){let href=&v[..q];if href.starts_with(prefix){ if tag.contains("media=") && tag.contains("dark") {i=e;continue;} if let Some(d)=b64(&href[prefix.len()..]){sheets.push(d);}}}}} i=e;}
 let bud=SupplyBudget{max_linked_sheets:8,max_sheet_bytes:262144,max_aggregate_bytes:2097152};
 for (name,t) in [("plain",plain(&html,&sheets,&Limits::DEFAULT,None,bud)),("inline",inl(&html,&sheets,&Limits::DEFAULT,None,bud)),("ext",ext(&html,&sheets,&Limits::DEFAULT,None,bud))]{
  let d=&t.diagnostics;
  println!("{name:6} unsupported={} truncated={} first={:?} CssSelector={} CssDeclaration={} ComputedValue={} MatchWork={} expanded={} match_attempts={} retained_nodes={}",t.unsupported,t.truncated,d.first_reason(),d.count(R::CssSelector),d.count(R::CssDeclaration),d.count(R::ComputedValue),d.count(R::MatchWork),d.expanded_declarations,d.match_attempts,d.retained_nodes);
 }
}
