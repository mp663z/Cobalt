use fontdue::{Font, FontSettings};
fn main(){
 let s="The quick brown fox jumps over the lazy dog 0123456789";
 for (n,path) in [("LibEmb","/home/sandbox/cb/fctl/LiberationSerif-Regular.ttf"),("DejaEmb","/home/sandbox/cb/fctl/DejaVuSans.ttf"),("SansEmb","/home/sandbox/cb/fctl/LiberationSans-Regular.ttf")]{
  let f=Font::from_bytes(std::fs::read(path).unwrap(),FontSettings::default()).unwrap();
  let mut k=8*64; while k<=40*64 { let px=k as f32/64.0; let l=f.horizontal_line_metrics(px).unwrap();
    let w:f32=s.chars().map(|c|f.metrics(c,px).advance_width).sum();
    println!("{n}\t{}\t{:.6}\t{:.6}\t{:.6}\t{:.6}",px,l.ascent,l.descent,l.line_gap,w); k+=7; }
 }
}
