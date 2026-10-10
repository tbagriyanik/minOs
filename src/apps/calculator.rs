use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct CalculatorApp { pub input: [char; 64], pub len: usize }
impl CalculatorApp {
    pub const fn new() -> Self { Self { input: [' '; 64], len: 0 } }
    pub fn insert(&mut self, c: char) { if self.len < 63 { self.input[self.len] = c; self.len += 1; } }
    pub fn backspace(&mut self) { if self.len > 0 { self.len -= 1; } }
    pub fn clear(&mut self) { self.len = 0; }
    pub fn press(&mut self, label: char) { if label == 'C' { self.clear(); } else if label == '=' { let v=self.value(); self.clear(); let mut n=v; if n<0 {self.insert('-'); n=-n;} let mut d=[0u8;12]; let mut k=0; if n==0 {d[0]=b'0';k=1;} while n>0 {d[k]=b'0'+(n%10) as u8;k+=1;n/=10;} for i in (0..k).rev(){self.insert(d[i] as char);} } else { self.insert(label); } }
    fn value(&self) -> i32 {
        let mut total=0i32; let mut term=0i32; let mut sign=1i32; let mut op=b'+'; let mut n=0i32; let mut has=false;
        for i in 0..=self.len { let c=if i==self.len {'+'} else {self.input[i]};
            if c>='0' && c<='9' { n=n.saturating_mul(10).saturating_add(c as i32-48); has=true; }
            else if c=='+' || c=='-' || c=='*' || c=='/' {
                if !has { continue; }
                match op { b'*' => term=term.saturating_mul(n), b'/' => if n!=0 { term/=n }, _ => term=n }
                if c=='+' || c=='-' { total=total.saturating_add(sign.saturating_mul(term)); term=0; sign=if c=='-' {-1} else {1}; }
                op=c as u8; n=0; has=false;
            }
        } total.saturating_add(sign.saturating_mul(term))
    }
    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        fb.draw_rect(x+8,y+34,w-16,42,Color::DARK_BG);
        let mut s=[0u8;80]; let mut n=0; for i in 0..self.len { s[n]=self.input[i] as u8; n+=1; }
        if let Ok(text)=core::str::from_utf8(&s[..n]) { MinGuiCompositor::draw_string(fb,x+16,y+42,text,Color::TEXT_MAIN); }
        let v=self.value(); let mut digits=[0u8;16]; let mut dn=0; let mut q=if v<0 {-v} else {v}; if q==0 {digits[0]=b'0';dn=1;} while q>0 {digits[dn]=b'0'+(q%10) as u8;dn+=1;q/=10;} if v<0 {digits[dn]=b'-';dn+=1;}
        for i in 0..dn/2 { digits.swap(i,dn-1-i); } for i in 0..dn {s[i]=digits[i];} if let Ok(text)=core::str::from_utf8(&s[..dn]) { MinGuiCompositor::draw_string(fb,x+16,y+60,text,Color::ACCENT_GREEN); }
        MinGuiCompositor::draw_string(fb,x+16,y+92,crate::gui::ui_text("Ornek: 12+3*4 / Backspace / Esc","Example: 12+3*4 / Backspace / Esc"),Color::TEXT_MUTED);
        let keys = [['7','8','9','/'],['4','5','6','*'],['1','2','3','-'],['C','0','=','+']];
        for row in 0..4 { for col in 0..4 { let bx=x+16+col*48; let by=y+112+row*24; fb.draw_rect(bx,by,40,20,if keys[row][col]=='=' {Color::ACCENT_GREEN} else {Color::TITLE_BAR}); let mut b=[0u8;1]; b[0]=keys[row][col] as u8; if let Ok(s)=core::str::from_utf8(&b){MinGuiCompositor::draw_string(fb,bx+16,by+6,s,Color::TEXT_MAIN);} } }
        MinGuiCompositor::draw_string(fb,x+16,y+h.saturating_sub(24),crate::gui::ui_text("Backspace: sil   Esc: temizle","Backspace: delete   Esc: clear"),Color::TEXT_MUTED);
    }
    pub fn key_at(x: usize, y: usize, wx: usize, wy: usize) -> Option<char> { if x<wx+16 || y<wy+112 {return None;} let col=(x-wx-16)/48; let row=(y-wy-112)/24; if col>=4||row>=4{return None;} Some([['7','8','9','/'],['4','5','6','*'],['1','2','3','-'],['C','0','=','+']][row][col]) }
}
