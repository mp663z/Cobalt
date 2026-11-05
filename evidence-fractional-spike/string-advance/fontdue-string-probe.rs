use fontdue::{Font, FontSettings};
fn main(){
 let strs=["H","HHHHHHHHHH","e","eeeeeeeeee","The quick brown fox jumps over the lazy dog","AVAVAVAVAV","Hello, world. 0123456789","iiiiiiiiiiiiiiiiiiii"];
 for (n,path) in [("LibEmb","/home/sandbox/cb/fctl/LiberationSerif-Regular.ttf"),("DejaEmb","/home/sandbox/cb/fctl/DejaVuSans.ttf")]{
  let f=Font::from_bytes(std::fs::read(path).unwrap(),FontSettings::default()).unwrap();
  for px in [13.0f32,13.5,13.99,14.0,14.4,16.0,16.5,17.6] { for s in strs {
    let mut sum=0f32; let mut ks=0f32; let mut prev:Option<char>=None;
    for ch in s.chars(){ sum+=f.metrics(ch,px).advance_width; if let Some(p)=prev { if let Some(k)=f.horizontal_kern(p,ch,px){ks+=k;} } prev=Some(ch);}
    println!("{n}\t{px}\t{s}\t{sum:.4}\t{ks:.4}");
  }}
 }
}
