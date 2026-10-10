use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct CalculatorApp { pub input: [char; 64], pub len: usize }
impl CalculatorApp {
    pub const fn new() -> Self { Self { input: [' '; 64], len: 0 } }
    pub fn insert(&mut self, c: char) { if self.len < 63 { self.input[self.len] = c; self.len += 1; } }
    pub fn backspace(&mut self) { if self.len > 0 { self.len -= 1; } }
    pub fn clear(&mut self) { self.len = 0; }
    fn value(&self) -> i32 {
        let mut total=0i32; let mut current=0i32; let mut op=b'+'; let mut n=0i32; let mut has=false;
        for i in 0..=self.len { let c=if i==self.len {'+'} else {self.input[i]};
            if c>='0' && c<='9' { n=n.saturating_mul(10).saturating_add(c as i32-48); has=true; }
            else if c=='+' || c=='-' || c=='*' || c=='/' { if !has {continue;} match op { b'+' => { total=total.saturating_add(current); current=n; }, b'-' => { total=total.saturating_add(current); current=-n; }, b'*' => current=current.saturating_mul(n), b'/' => if n!=0 { current/=n }, _=>{} } op=c as u8; n=0; has=false; }
        } total.saturating_add(current)
    }
    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        fb.draw_rect(x+8,y+34,w-16,42,Color::DARK_BG);
        let mut s=[0u8;80]; let mut n=0; for i in 0..self.len { s[n]=self.input[i] as u8; n+=1; }
        let v=self.value(); let mut digits=[0u8;16]; let mut dn=0; let mut q=if v<0 {-v} else {v}; if q==0 {digits[0]=b'0';dn=1;} while q>0 {digits[dn]=b'0'+(q%10) as u8;dn+=1;q/=10;} if v<0 {digits[dn]=b'-';dn+=1;}
        for i in 0..dn/2 { digits.swap(i,dn-1-i); } for i in 0..dn {s[n]=digits[i];n+=1;} s[n]=0;
        if let Ok(text)=core::str::from_utf8(&s[..n]) { MinGuiCompositor::draw_string(fb,x+16,y+50,text,Color::ACCENT_GREEN); }
        MinGuiCompositor::draw_string(fb,x+16,y+92,"Ornek: 12+3*4 / Backspace / Esc",Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb,x+16,y+h.saturating_sub(24),"Backspace: sil   Esc: temizle",Color::TEXT_MUTED);
    }
}
