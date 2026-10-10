use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
pub struct PaintApp { pub points: [(usize,usize);512], pub len: usize, pub color: u32 }
impl PaintApp { pub const fn new()->Self{Self{points:[(0,0);512],len:0,color:Color::ACCENT_BLUE}} pub fn paint(&mut self,x:usize,y:usize){if self.len<512{self.points[self.len]=(x,y);self.len+=1;}} pub fn clear(&mut self){self.len=0;} pub fn render_content(&self,fb:&mut Framebuffer,x:usize,y:usize,w:usize,h:usize){fb.draw_rect(x+8,y+58,w-16,h.saturating_sub(68),Color::WHITE);for i in 0..self.len{let(p,q)=self.points[i];fb.draw_rect(x+8+p,y+58+q,5,5,self.color);}MinGuiCompositor::draw_string(fb,x+14,y+40,"Kalem   |   C: Temizle",Color::TEXT_MAIN);}}
