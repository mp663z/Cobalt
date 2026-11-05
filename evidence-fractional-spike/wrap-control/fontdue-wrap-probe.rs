use fontdue::{Font, FontSettings};
fn main(){
 let text="The quick brown fox jumps over the lazy dog while seven wizards quietly boxed jumping zebras near the old stone bridge at 10:45 on 2026.";
 let words:Vec<&str>=text.split(' ').collect();
 let sizes:Vec<f32>=(0..).map(|i|(12*64+i*47) as f32/64.0).take_while(|&s|s<=24.0).collect();
 let mut widths=vec![];let mut w=120.0f64; while w<=420.0+1e-9 { widths.push((w*64.0).round()/64.0); w+=13.37; }
 for (n,path) in [("LibEmb","/home/sandbox/cb/fctl/LiberationSerif-Regular.ttf"),("DejaEmb","/home/sandbox/cb/fctl/DejaVuSans.ttf"),("SansEmb","/home/sandbox/cb/fctl/LiberationSans-Regular.ttf")]{
  let f=Font::from_bytes(std::fs::read(path).unwrap(),FontSettings::default()).unwrap();
  for &px in &sizes { let adv=|s:&str|->f32{s.chars().map(|c|f.metrics(c,px).advance_width).sum()};
    let sp=adv(" "); let ww:Vec<f32>=words.iter().map(|w|adv(w)).collect();
    for &wd in &widths { let wd=wd as f32; let mut brk=vec![]; let mut cur=ww[0];
      for i in 1..words.len(){ if cur+sp+ww[i] <= wd { cur+=sp+ww[i]; } else { brk.push(i); cur=ww[i]; } }
      println!("{n}\t{px}\t{wd}\t{}",brk.iter().map(|b|b.to_string()).collect::<Vec<_>>().join(","));
  }}
 }
}
