use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};
pub struct PaintApp { pub points: [(usize,usize);2048], pub len: usize, pub color: u32, pub eraser: bool, pub last: Option<(usize,usize)> }
impl PaintApp {
    pub const fn new()->Self{Self{points:[(0,0);2048],len:0,color:Color::ACCENT_BLUE,eraser:false,last:None}}
    pub fn paint(&mut self,x:usize,y:usize){if self.eraser { let mut out=0; for i in 0..self.len { let (px,py)=self.points[i]; if px.abs_diff(x)>10 || py.abs_diff(y)>10 { self.points[out]=(px,py); out+=1; } } self.len=out; self.last=Some((x,y)); return; } let (sx,sy)=self.last.unwrap_or((x,y)); let dx=x as isize-sx as isize; let dy=y as isize-sy as isize; let steps=dx.unsigned_abs().max(dy.unsigned_abs()).max(1); for i in 1..=steps { if self.len>=2048{break;} let px=(sx as isize+dx*i as isize/steps as isize).max(0) as usize; let py=(sy as isize+dy*i as isize/steps as isize).max(0) as usize; self.points[self.len]=(px,py);self.len+=1; } self.last=Some((x,y));}
    pub fn end_stroke(&mut self){self.last=None;}
    pub fn clear(&mut self){self.len=0;self.last=None;}
    pub fn render_content(&self,fb:&mut Framebuffer,x:usize,y:usize,w:usize,h:usize){fb.draw_rect(x+8,y+58,w-16,h.saturating_sub(68),Color::WHITE);for i in 0..self.len{let(p,q)=self.points[i];fb.draw_rect(x+8+p,y+58+q,7,7,self.color);}MinGuiCompositor::draw_string(fb,x+14,y+40,crate::gui::ui_text("Kalem: ","Pen: "),Color::TEXT_MAIN);for (i,c) in [Color::ACCENT_BLUE,Color::ACCENT_RED,Color::ACCENT_GREEN,Color::BLACK].iter().enumerate(){fb.draw_rect(x+58+i*22,y+36,16,16,*c);}fb.draw_rect(x+150,y+36,58,16,Color::TITLE_BAR);MinGuiCompositor::draw_string(fb,x+156,y+40,crate::gui::ui_text("Silgi","Eraser"),Color::TEXT_MAIN);}
}
