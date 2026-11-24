//! Settled vertical movement. Motion never redraws; release emits one action.
use crate::{ActionId,Layout,Rect};
#[derive(Clone,Copy,Debug)]
pub enum Phase{Down,Move,Up,Cancel}
#[derive(Default,Debug)]
pub struct ScrollGesture{capture:Option<Capture>,cancelled:bool}
#[derive(Debug)]
struct Capture{rect:Rect,base:ActionId,start:(i32,i32),moved:bool}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum Outcome{Pass,Captured,Consumed,Action(ActionId)}
impl ScrollGesture{
    pub fn cancel(&mut self){self.cancelled|=self.capture.is_some();self.capture=None;}
    pub fn observe(&mut self,phase:Phase,x:i32,y:i32,layout:&Layout)->Outcome{
        if matches!(phase,Phase::Cancel){self.cancel();return Outcome::Consumed;}
        if matches!(phase,Phase::Down){
            self.cancelled=false;
            self.capture=layout.scroll_regions.iter().rev().find(|(rect,_)|rect.contains(x,y)).map(|(rect,base)|Capture{rect:*rect,base:*base,start:(x,y),moved:false});
            return if self.capture.is_some(){Outcome::Captured}else{Outcome::Pass};
        }
        if self.cancelled{if matches!(phase,Phase::Up){self.cancelled=false;}return Outcome::Consumed;}
        let Some(c)=self.capture.as_mut() else{return Outcome::Pass;};
        // Layout, rotation, keyboard or overlay changes cancel the captured
        // gesture. Never reinterpret its release as a tap in the new frame.
        if !layout.scroll_regions.iter().any(|(rect,base)|*rect==c.rect&&*base==c.base){self.cancel();if matches!(phase,Phase::Up){self.cancelled=false;}return Outcome::Consumed;}
        let dx=i64::from(x)-i64::from(c.start.0);let dy=i64::from(y)-i64::from(c.start.1);
        let threshold=(c.rect.height/80).max(8) as i64;
        c.moved|=dx.abs()>threshold||dy.abs()>threshold;
        if !matches!(phase,Phase::Up){return Outcome::Captured;}
        let c=self.capture.take().expect("capture checked");
        if !c.moved{return Outcome::Pass;}
        if dy.abs()<=dx.abs()||dy.abs()<=threshold{return Outcome::Consumed;}
        let amount=(-dy*100/i64::from(c.rect.height.max(1))).clamp(-100,100) as i32;
        c.base.0.checked_add((amount+100) as u32).map_or(Outcome::Consumed,|id|Outcome::Action(ActionId(id)))
    }
}
#[cfg(test)]
mod tests{
 use super::*;
 fn layout()->Layout{Layout{scroll_regions:vec![(Rect{x:0,y:100,width:600,height:800},ActionId(1000))],..Default::default()}}
 #[test]fn release_scrolls_by_distance_not_by_page_and_taps_pass(){
  let mut g=ScrollGesture::default();let l=layout();
  assert_eq!(g.observe(Phase::Down,200,700,&l),Outcome::Captured);
  assert_eq!(g.observe(Phase::Move,200,500,&l),Outcome::Captured);
  assert_eq!(g.observe(Phase::Up,200,300,&l),Outcome::Action(ActionId(1150)));
  assert_eq!(g.observe(Phase::Up,200,300,&l),Outcome::Pass);
  g.observe(Phase::Down,200,700,&l);assert_eq!(g.observe(Phase::Up,201,702,&l),Outcome::Pass);
 }
 #[test]fn horizontal_cancel_outside_and_reflow_do_not_activate_release_links(){
  let mut g=ScrollGesture::default();let l=layout();
  assert_eq!(g.observe(Phase::Down,200,50,&l),Outcome::Pass);
  g.observe(Phase::Down,200,700,&l);assert_eq!(g.observe(Phase::Up,500,700,&l),Outcome::Consumed);
  g.observe(Phase::Down,200,700,&l);assert_eq!(g.observe(Phase::Up,200,100,&Layout::default()),Outcome::Consumed);
  g.observe(Phase::Down,200,700,&l);g.observe(Phase::Cancel,0,0,&l);assert_eq!(g.observe(Phase::Up,200,100,&l),Outcome::Consumed);
 }
}
