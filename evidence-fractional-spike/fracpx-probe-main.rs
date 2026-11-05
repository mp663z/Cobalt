use fontdue::{Font, FontSettings};
fn main(){
 for path in ["/usr/share/fonts/truetype/liberation2/LiberationSerif-Regular.ttf","/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]{
  let f=Font::from_bytes(std::fs::read(path).unwrap(),FontSettings::default()).unwrap();
  println!("FONT {path}");
  for px in [13.0f32,13.5,13.99,14.0,14.4,16.0,16.5,17.6] {
    let lm=f.horizontal_line_metrics(px).unwrap();
    let m=f.metrics('H',px); let (_,bm)=f.rasterize('H',px);
    let m2=f.metrics('e',px);
    println!("px={px:6} ascent={:.4} descent={:.4} line_gap={:.4} new_line={:.4} | H adv={:.4} w={} h={} xmin={} ymin={} cov_sum={} | e adv={:.4}",lm.ascent,lm.descent,lm.line_gap,lm.new_line_size,m.advance_width,m.width,m.height,m.xmin,m.ymin,bm.iter().map(|&b|b as u32).sum::<u32>(),m2.advance_width);
  }
 }
}
