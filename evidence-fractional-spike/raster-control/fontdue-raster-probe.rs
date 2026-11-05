use fontdue::{Font, FontSettings};
fn main(){
 for (n,path) in [("LibEmb","/home/sandbox/cb/fctl/LiberationSerif-Regular.ttf"),("DejaEmb","/home/sandbox/cb/fctl/DejaVuSans.ttf")]{
  let f=Font::from_bytes(std::fs::read(path).unwrap(),FontSettings::default()).unwrap();
  for px in [13.0f32,13.5,14.0,14.4,16.0,16.5,17.6] { for ch in ['H','e']{
    let (m,bm)=f.rasterize(ch,px);
    let s:u64=bm.iter().map(|&b|b as u64).sum();
    let nz=bm.iter().filter(|&&b|b>0).count();
    println!("{n} {px} {ch} cov255={s} w={} h={} xmin={} ymin={} nz={nz} adv={:.4}",m.width,m.height,m.xmin,m.ymin,m.advance_width);
  }}
 }
}
