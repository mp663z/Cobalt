use kobo_web_document::{parse_style_tree_with_supply as plain, parse_style_tree_with_inline_sheets as inl, Limits, SupplyBudget};
fn b64(s:&str)->Option<Vec<u8>>{let mut o=vec![];let(mut a,mut n)=(0u32,0);for c in s.bytes(){let v=match c{b'A'..=b'Z'=>c-b'A',b'a'..=b'z'=>c-b'a'+26,b'0'..=b'9'=>c-b'0'+52,b'+'=>62,b'/'=>63,b'='=>break,_=>return None};a=(a<<6)|v as u32;n+=6;if n>=8{n-=8;o.push((a>>n) as u8);a&=(1<<n)-1;}}Some(o)}
fn main(){
 let f=std::env::args().nth(1).unwrap(); let html=std::fs::read(&f).unwrap(); let text=String::from_utf8_lossy(&html).into_owned(); let prefix="data:text/css;base64,";
 let mut sheets:Vec<Vec<u8>>=vec![]; let mut i=0;
 while let Some(p)=text[i..].find("<link"){let s=i+p;let e=text[s..].find('>').map(|x|s+x).unwrap_or(text.len());let tag=&text[s..e];
  if tag.contains("stylesheet"){ if let Some(h)=tag.find("href=\""){let v=&tag[h+6..];if let Some(q)=v.find('"'){let href=&v[..q];if href.starts_with(prefix){ if tag.contains("media=") && tag.contains("dark") {i=e;continue;} if let Some(d)=b64(&href[prefix.len()..]){sheets.push(d);}}}}} i=e;}
 let bud=SupplyBudget{max_linked_sheets:8,max_sheet_bytes:262144,max_aggregate_bytes:2097152};
 let t=plain(&html,&sheets,&Limits::DEFAULT,None,bud);
 println!("plain expanded={} match_attempts={} retained_nodes={}",t.diagnostics.expanded_declarations,t.diagnostics.match_attempts,t.diagnostics.retained_nodes);
 let t=inl(&html,&sheets,&Limits::DEFAULT,None,bud);
 println!("inline_entry expanded={} match_attempts={} retained_nodes={}",t.diagnostics.expanded_declarations,t.diagnostics.match_attempts,t.diagnostics.retained_nodes);
 for (n,s) in sheets.iter().enumerate(){ let c=String::from_utf8_lossy(s); let mut k=0; for m in c.match_indices("text-decoration"){ k+=1; if k<=40 { let e=c[m.0..].find(|x|x==';'||x=='}').map(|x|m.0+x).unwrap_or(c.len()); let st=c[..m.0].rfind('{').map(|x|c[..x].rfind('}').map(|y|y+1).unwrap_or(0)).unwrap_or(0); println!("sheet{n} sel=[{}] decl=[{}]", c[st..c[..m.0].rfind('{').unwrap_or(0)].trim().chars().take(70).collect::<String>(), &c[m.0..e]); } } println!("sheet{n} bytes={} text-decoration occurrences={}",s.len(),k);}

 let base=(plain(&html,&sheets,&Limits::DEFAULT,None,bud).diagnostics.expanded_declarations,plain(&html,&sheets,&Limits::DEFAULT,None,bud).diagnostics.match_attempts);
 let (mut se,mut sm)=(0i64,0i64);
 for (n,s) in sheets.iter().enumerate(){ let c=String::from_utf8_lossy(s).into_owned(); for m in c.match_indices("text-decoration"){ let e=c[m.0..].find(|x|x==';'||x=='}').map(|x|m.0+x).unwrap_or(c.len()); let e2=if c[e..].starts_with(';'){e+1}else{e};
   let st=c[..m.0].rfind('{').map(|x|c[..x].rfind('}').map(|y|y+1).unwrap_or(0)).unwrap_or(0); let sel=c[st..c[..m.0].rfind('{').unwrap_or(0)].trim().chars().take(60).collect::<String>();
   let mut v=sheets.clone(); let nc=format!("{}{}",&c[..m.0],&c[e2..]); v[n]=nc.into_bytes(); let r=plain(&html,&v,&Limits::DEFAULT,None,bud);
   let de=base.0 as i64-r.diagnostics.expanded_declarations as i64; let dm=base.1 as i64-r.diagnostics.match_attempts as i64; se+=de; sm+=dm;
   println!("REMOVE sheet{n} [{}] [{}] -> expanded -{de} match_attempts -{dm}",sel,&c[m.0..e]); } }
 println!("sum of per-declaration contributions: expanded {se} match_attempts {sm}");
}
