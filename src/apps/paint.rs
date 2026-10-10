use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
pub struct PaintApp { pub points: [(usize,usize);128], pub len: usize }
impl PaintApp { pub const fn new()->Self{Self{points:[(0,0);128],len:0}} pub fn paint(&mut self,x:usize,y:usize){if self.len<128{self.points[self.len]=(x,y);self.len+=1;}} pub fn clear(&mut self){self.len=0;} pub fn render_content(&self,fb:&mut Framebuffer,x:usize,y:usize,w:usize,h:usize){fb.draw_rect(x+8,y+34,w-16,h.saturating_sub(44),Color::WHITE);for i in 0..self.len{let(p,q)=self.points[i];fb.draw_rect(x+8+p,y+34+q,4,4,Color::ACCENT_BLUE);}MinGuiCompositor::draw_string(fb,x+14,y+40,"Sol tıkla: çiz | C: temizle",Color::BLACK);}}
