use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};

fn find_bytes(haystack:&[u8], needle:&[u8])->Option<usize> {
    if needle.is_empty() || needle.len()>haystack.len(){return None;}
    haystack.windows(needle.len()).position(|w|w==needle)
}

fn split_http_lines(body:&[u8])->impl Iterator<Item=&str> {
    body.split(|b|*b==b'\n').filter_map(|line| {
        let line=if line.last()==Some(&b'\r'){&line[..line.len()-1]}else{line};
        if line.is_empty(){None}else{Some(core::str::from_utf8(line).unwrap_or("[binary or non-UTF8 content]"))}
    })
}

fn eq_ascii(a:&[u8], b:&[u8])->bool { a.eq_ignore_ascii_case(b) }

fn html_color(value:&[u8])->Option<u32> {
    let mut v=value;
    while v.first().map_or(false,|b|b.is_ascii_whitespace()||*b==b'\''||*b==b'"') { v=&v[1..]; }
    if v.first()==Some(&b'#') && v.len()>=7 {
        let mut rgb=0u32;
        for &b in &v[1..7] { let d=match b {b'0'..=b'9'=>(b-b'0') as u32,b'a'..=b'f'=>(b-b'a'+10) as u32,b'A'..=b'F'=>(b-b'A'+10) as u32,_=>return None};rgb=(rgb<<4)|d; }
        return Some(rgb);
    }
    match v.split(|b|*b==b';'||b.is_ascii_whitespace()||*b==b'\''||*b==b'"').next()? {
        b"black"=>Some(0x101010),b"white"=>Some(0xFFFFFF),b"red"=>Some(0xD03030),b"green"=>Some(0x188040),b"blue"=>Some(0x2050C0),b"yellow"=>Some(0xD0A020),b"gray"|b"grey"=>Some(0x808080),_=>None,
    }
}

/// Bounded safe text renderer: strips markup, scripts and styles; supports block breaks,
/// common entities and an inline CSS `color` declaration.
fn render_html_text(body:&[u8], lines:&mut [[u8;96];6], lens:&mut [usize;6], colors:&mut [u32;6])->usize {
    let mut count=1usize; let mut current=0u32; colors[0]=Color::TEXT_MAIN;
    let mut p=0usize; let mut hidden:Option<&[u8]>=None;
    while p<body.len() && count<=6 {
        if body[p]==b'<' {
            let start=p+1; let mut end=start; while end<body.len() && body[end]!=b'>' {end+=1;}
            if end==body.len(){break;}
            let mut tag=&body[start..end]; let closing=tag.first()==Some(&b'/'); if closing {tag=&tag[1..];}
            let name_end=tag.iter().position(|b|b.is_ascii_whitespace()||*b==b'/').unwrap_or(tag.len()); let name=&tag[..name_end];
            if let Some(h)=hidden { if closing && eq_ascii(name,h) {hidden=None;} p=end+1;continue; }
            if !closing && (eq_ascii(name,b"script")||eq_ascii(name,b"style")) {hidden=Some(name);p=end+1;continue;}
            if !closing && name_end<tag.len() {
                let attrs=&tag[name_end..]; let mut i=0;
                while i<attrs.len() { while i<attrs.len()&&(attrs[i].is_ascii_whitespace()||attrs[i]==b'/'){i+=1;} let k=i;while i<attrs.len()&&(attrs[i].is_ascii_alphanumeric()||attrs[i]==b'-'){i+=1;}let key=&attrs[k..i];while i<attrs.len()&&attrs[i].is_ascii_whitespace(){i+=1;}if i<attrs.len()&&attrs[i]==b'='{i+=1;while i<attrs.len()&&attrs[i].is_ascii_whitespace(){i+=1;}let quote=if i<attrs.len()&&(attrs[i]==b'\''||attrs[i]==b'"'){let q=attrs[i];i+=1;q}else{0};let v=i;while i<attrs.len()&&((quote!=0&&attrs[i]!=quote)||(quote==0&&!attrs[i].is_ascii_whitespace())){i+=1;}if eq_ascii(key,b"style"){let style=&attrs[v..i];for rule in style.split(|b|*b==b';'){if let Some(colon)=rule.iter().position(|b|*b==b':'){if eq_ascii(rule[..colon].trim_ascii(),b"color"){if let Some(c)=html_color(&rule[colon+1..]){current=c;}}}}}if quote!=0&&i<attrs.len(){i+=1;}}else if i==k{i+=1;} }
            }
            let block=matches!(name,b"p"|b"div"|b"h1"|b"h2"|b"h3"|b"li"|b"br"|b"hr"|b"tr"|b"section"|b"article");
            if block && lens[count-1]>0 && count<6 {count+=1;colors[count-1]=current;}
            p=end+1;continue;
        }
        if body[p]==b'&' { if let Some(semi)=body[p..].iter().take(12).position(|b|*b==b';') { let entity=&body[p+1..p+semi]; let decoded=match entity {b"amp"=>Some(b'&'),b"lt"=>Some(b'<'),b"gt"=>Some(b'>'),b"quot"=>Some(b'"'),b"apos"=>Some(b'\''),b"nbsp"=>Some(b' '),_=>None};if let Some(ch)=decoded{let n=lens[count-1];if n<96{lines[count-1][n]=ch;lens[count-1]+=1;}p+=semi+1;continue;} } }
        let b=body[p];p+=1;if b==b'\r'{continue;}if b==b'\n'||b==b'\t'{if lens[count-1]>0&&count<6{count+=1;colors[count-1]=current;}continue;}if b<0x20{continue;}let n=lens[count-1];if n<96{lines[count-1][n]=b;lens[count-1]+=1;}else if count<6{count+=1;colors[count-1]=current;lines[count-1][0]=b;lens[count-1]=1;}
    }
    while count>1&&lens[count-1]==0{count-=1;} count
}

pub struct BrowserApp {
    pub url: [u8; 128],
    pub url_len: usize,
    pub address_focused: bool,
    pub active_tab: usize,
    pub focused_control: usize,
    pub page_loaded: bool,
    pub status: &'static str,
    pub net_online: bool,
    pub ping_ok: bool,
    pub web_request_sent: bool,
    pub page_title: [u8; 64],
    pub page_title_len: usize,
    pub page_lines: [[u8; 96]; 6],
    pub page_line_lens: [usize; 6],
    pub page_line_colors: [u32; 6],
    pub page_line_count: usize,
    pub request_pending: bool,
    pub request_host: [u8; 96],
    pub request_host_len: usize,
    pub request_path: [u8; 180],
    pub request_path_len: usize,
    pub request_ip: crate::drivers::netstack::Ipv4Address,
    pub request_port: u16,
    pub request_deadline: usize,
    pub request_state: u8,
    pub response_data: [u8; 2048],
    pub response_len: usize,
}

impl BrowserApp {
    pub fn new() -> Self {
        let mut app = Self {
            url: [0; 128], url_len: 0, address_focused: false,
            active_tab: 0, focused_control: 0, page_loaded: true, status: crate::gui::ui_text("MinOS yerel yardım sayfası", "MinOS local help page"),
            net_online: false, ping_ok: false,
            web_request_sent: false,
            page_title: [0; 64], page_title_len: 0,
            page_lines: [[0; 96]; 6], page_line_lens: [0; 6], page_line_count: 0,
            page_line_colors:[Color::TEXT_MAIN;6],
            request_pending: false, request_host: [0;96], request_host_len:0,
            request_path:[0;180], request_path_len:0, request_ip:crate::drivers::netstack::Ipv4Address([0;4]), request_port:80,
            request_deadline:0, request_state:0, response_data:[0;2048], response_len:0,
        };
        app.set_url(b"minos://help");
        app
    }

    pub fn focus_next_control(&mut self) {
        // 0..2: Tabs (Help, Cats, Web), 3: Back, 4: Fwd, 5: Refresh, 6: Address bar, 7: Go button
        self.focused_control = (self.focused_control + 1) % 8;
        if self.focused_control == 6 {
            self.address_focused = true;
        } else {
            self.address_focused = false;
        }
        if self.focused_control < 3 {
            match self.focused_control {
                0 => { self.active_tab = 0; self.set_url(b"minos://help"); self.navigate(); },
                1 => { self.active_tab = 1; self.set_url(b"minos://cats"); self.navigate(); },
                _ => { self.active_tab = 3; self.page_loaded = true; },
            }
        }
    }

    pub fn select_tab_prev(&mut self) {
        let cur = match self.active_tab {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        let next = (cur + 2) % 3;
        self.focused_control = next;
        match next {
            0 => { self.active_tab = 0; self.set_url(b"minos://help"); self.navigate(); },
            1 => { self.active_tab = 1; self.set_url(b"minos://cats"); self.navigate(); },
            _ => { self.active_tab = 3; self.page_loaded = true; },
        }
    }

    pub fn select_tab_next(&mut self) {
        let cur = match self.active_tab {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        let next = (cur + 1) % 3;
        self.focused_control = next;
        match next {
            0 => { self.active_tab = 0; self.set_url(b"minos://help"); self.navigate(); },
            1 => { self.active_tab = 1; self.set_url(b"minos://cats"); self.navigate(); },
            _ => { self.active_tab = 3; self.page_loaded = true; },
        }
    }

    pub fn set_url(&mut self, value: &[u8]) {
        self.url = [0; 128];
        self.url_len = value.len().min(self.url.len());
        self.url[..self.url_len].copy_from_slice(&value[..self.url_len]);
    }

    pub fn set_web_content(&mut self, title: &str, lines: &[&str]) {
        self.page_title = [0; 64];
        let tb = title.as_bytes();
        self.page_title_len = tb.len().min(64);
        self.page_title[..self.page_title_len].copy_from_slice(&tb[..self.page_title_len]);

        self.page_line_count = lines.len().min(6);
        for i in 0..self.page_line_count {
            self.page_lines[i] = [0; 96];
            let lb = lines[i].as_bytes();
            let n = lb.len().min(96);
            self.page_lines[i][..n].copy_from_slice(&lb[..n]);
            self.page_line_lens[i] = n;
        }
    }

    pub fn address_char(&mut self, ch: char) {
        if ch.is_ascii_graphic() || ch == ' ' {
            if self.url_len < self.url.len() {
                self.url[self.url_len] = ch as u8;
                self.url_len += 1;
            }
        }
    }

    pub fn address_backspace(&mut self) {
        if self.url_len > 0 {
            self.url_len -= 1;
            self.url[self.url_len] = 0;
        }
    }

    pub fn navigate(&mut self) {
        let value = core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("");
        self.web_request_sent = false;
        if value.starts_with("https://") {
            self.active_tab=3; self.page_loaded=false; self.request_pending=false;
            self.status=crate::gui::ui_text("HTTPS/TLS bu çekirdek motorunda desteklenmiyor.","HTTPS/TLS is not supported by this kernel renderer.");
            self.address_focused=false; return;
        }
        if let Some(rest) = value.strip_prefix("http://") {
            let authority_end = rest.find('/').unwrap_or(rest.len());
            let authority = &rest[..authority_end];
            let mut path = if authority_end < rest.len() { &rest[authority_end..] } else { "/" };
            if let Some(q) = path.strip_prefix('?') { path = q; }
            let (host, port) = match authority.rsplit_once(':') {
                Some((h,p)) if !h.contains(']') => match p.parse::<u16>() { Ok(n) if n>0 => (h,n), _ => ("",0) },
                _ => (authority,80),
            };
            if host.is_empty() || host.len() > 96 || path.len()>180 || port==0 {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Geçersiz web adresi.", "Invalid web address.");
            } else if !crate::drivers::e1000::is_ready() {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Ağ kartı kullanılamıyor.", "Network adapter unavailable.");
            } else {
                self.active_tab = 3;
                self.page_loaded = false;
                self.web_request_sent = false;
                self.request_host = [0;96]; self.request_host_len=host.len(); self.request_host[..host.len()].copy_from_slice(host.as_bytes());
                self.request_path = [0;180]; self.request_path_len=path.len(); self.request_path[..path.len()].copy_from_slice(path.as_bytes());
                self.request_port=port; self.request_state=0; self.request_pending=true; self.response_len=0;
                self.status = crate::gui::ui_text("DNS çözümlemesi bekleniyor...", "Resolving host...");
                self.page_title=[0;64]; self.page_title_len=0; self.page_line_count=0;
            }
            self.address_focused = false;
            return;
        }
        match value {
            "minos://help" => {
                self.active_tab = 0;
                self.page_loaded = true;
                self.status = crate::gui::ui_text("Yerel MinOS yardım sayfası açıldı.", "Local MinOS help page opened.");
            }
            "minos://cats" => {
                self.active_tab = 1;
                self.page_loaded = true;
                self.status = crate::gui::ui_text("Yerel KediWiki sayfası açıldı.", "Local KediWiki page opened.");
            }
            "minos://net" | "net://status" => {
                // Ağ durumu paneli — e1000 durumu + gateway ARP/ping testi
                self.active_tab = 2;
                self.net_online = crate::drivers::e1000::is_ready();
                if self.net_online {
                    // Gateway'e ARP çözümlemesi ve ICMP Echo gönder
                    let gateway=crate::drivers::netstack::gateway_ip();
                    let sent = crate::drivers::netstack::ping(gateway);
                    self.ping_ok = sent || crate::drivers::netstack::arp_lookup(gateway).is_some();
                    self.page_loaded = true;
                    self.status = crate::gui::ui_text("Ağ durumu sayfası açıldı (sınama gönderildi).", "Network status page opened (test sent).");
                } else {
                    self.page_loaded = false;
                    self.status = crate::gui::ui_text("e1000 ağ kartı bulunamadı; ağ yığını pasif.", "e1000 adapter unavailable; network stack is inactive.");
                }
            }
            _ => {
                self.page_loaded = false;
                self.status = crate::gui::ui_text("Desteklenen: http:// ve minos:// adresleri.", "Supported schemes: http:// and minos://.");
            }
        }
        self.address_focused = false;
    }

    /// Ana döngüden çağrılır; ağ bekleyişi ekran olaylarını durdurmaz.
    pub fn poll_network(&mut self) {
        if !self.request_pending { return; }
        let now = crate::main_monotonic_ms();
        if self.request_deadline==0 { self.request_deadline=now.wrapping_add(8000); }
        if now.wrapping_sub(self.request_deadline) < usize::MAX/2 {
            self.request_pending=false;
            self.page_loaded=false;
            self.status=crate::gui::ui_text("Ağ isteği zaman aşımına uğradı.", "Network request timed out.");
            crate::drivers::netstack::tcp_clear_result();
            self.request_state=0;
            self.request_deadline=0;
            return;
        }
        match self.request_state {
            0 => {
                let host=core::str::from_utf8(&self.request_host[..self.request_host_len]).unwrap_or("");
                if let Some(ip)=crate::drivers::netstack::resolve_ipv4(host) {
                    self.request_ip=ip;
                    let port=self.request_port;
                    if crate::drivers::netstack::tcp_begin(ip,port,49153) { self.request_state=1; self.status=crate::gui::ui_text("TCP bağlantısı kuruluyor...", "Connecting over TCP..."); }
                    else { self.finish_network_error(crate::gui::ui_text("TCP bağlantısı başlatılamadı.", "Could not start TCP connection.")); }
                } else {
                    self.finish_network_error(crate::gui::ui_text("DNS adı çözümlenemedi.", "DNS lookup failed."));
                }
            }
            1 => {
                if crate::drivers::netstack::tcp_reset_received() { self.finish_network_error(crate::gui::ui_text("Sunucu bağlantıyı reddetti.", "Server rejected the connection.")); }
                else if crate::drivers::netstack::tcp_syn_acked() {
                    let host=core::str::from_utf8(&self.request_host[..self.request_host_len]).unwrap_or("");
                    let path=core::str::from_utf8(&self.request_path[..self.request_path_len]).unwrap_or("/");
                    let mut req=[0u8;512]; let mut n=0;
                    let mut add=|s:&[u8]| { let k=s.len().min(req.len()-n); req[n..n+k].copy_from_slice(&s[..k]); n+=k; };
                    add(b"GET "); add(path.as_bytes()); add(b" HTTP/1.0\r\nHost: "); add(host.as_bytes());
                    add(b"\r\nUser-Agent: MinOS/1.0\r\nAccept: text/html,text/plain\r\nConnection: close\r\n\r\n");
                    if crate::drivers::netstack::tcp_send(&req[..n]) { self.request_state=2; self.web_request_sent=true; self.status=crate::gui::ui_text("HTTP yanıtı bekleniyor...", "Waiting for HTTP response..."); }
                }
            }
            2 => {
                let room=self.response_data.len().saturating_sub(self.response_len);
                if room>0 { self.response_len+=crate::drivers::netstack::take_tcp_data(&mut self.response_data[self.response_len..]); }
                if self.response_len>0 {
                    if let Some(end)=find_bytes(&self.response_data[..self.response_len],b"\r\n\r\n") {
                        let status_end=find_bytes(&self.response_data[..end],b"\r\n").unwrap_or(end);
                        self.page_title=[0;64];
                        let title_len=self.request_host_len.min(64); self.page_title[..title_len].copy_from_slice(&self.request_host[..title_len]); self.page_title_len=title_len;
                        let status_bytes=&self.response_data[..status_end]; let status_len=status_bytes.len().min(96);
                        self.page_lines[0]=[0;96]; self.page_lines[0][..status_len].copy_from_slice(&status_bytes[..status_len]); self.page_line_lens[0]=status_len;
                        self.page_lines[1]=[0;96]; self.page_lines[1][..24].copy_from_slice(b"HTTP response received."); self.page_line_lens[1]=24; self.page_line_count=2;
                        let body_start=end+4;
                        let body=&self.response_data[body_start..self.response_len];
                        if body.iter().any(|b|*b==b'<') { self.page_line_count=render_html_text(body,&mut self.page_lines,&mut self.page_line_lens,&mut self.page_line_colors); }
                        else { for line in split_http_lines(body).take(6) {let next=self.page_line_count;self.page_lines[next]=[0;96];let bytes=line.as_bytes();let n=bytes.len().min(96);self.page_lines[next][..n].copy_from_slice(&bytes[..n]);self.page_line_lens[next]=n;self.page_line_colors[next]=Color::TEXT_MAIN;self.page_line_count+=1;} }
                        self.page_loaded=true;
                    }
                }
                if crate::drivers::netstack::tcp_finished() || self.response_len>=self.response_data.len() {
                    if self.response_len>0 && !self.page_loaded { self.finish_http_response(); }
                    if self.page_loaded { self.status=crate::gui::ui_text("HTTP yanıtı alındı.", "HTTP response received."); self.request_pending=false; self.request_state=0; self.request_deadline=0; crate::drivers::netstack::tcp_clear_result(); }
                    else { self.finish_network_error(crate::gui::ui_text("HTTP yanıtı okunamadı.", "Could not parse HTTP response.")); }
                }
            }
            _ => self.finish_network_error(crate::gui::ui_text("Ağ isteği başarısız.", "Network request failed.")),
        }
    }

    fn finish_http_response(&mut self) { self.page_loaded=true; self.status=crate::gui::ui_text("HTTP yanıtı alındı.","HTTP response received."); }
    fn finish_network_error(&mut self, status:&'static str) { self.request_pending=false; self.page_loaded=false; self.status=status; self.request_state=0; self.request_deadline=0; crate::drivers::netstack::tcp_clear_result(); }

    pub fn back(&mut self) {
        self.set_url(b"minos://help");
        self.navigate();
    }

    pub fn forward(&mut self) {
        self.set_url(b"minos://cats");
        self.navigate();
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize) {
        let english = crate::gui::ui_english();
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 22, Color::TITLE_BAR_INACTIVE);
        let tab0_bg = if self.active_tab == 0 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 8, win_y + 30, 140, 20, tab0_bg);
        MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 34, crate::gui::ui_text("MinOS Yardım", "MinOS Help"), Color::TEXT_MAIN);
        if self.focused_control == 0 { fb.draw_rect_outline(win_x + 8, win_y + 30, 140, 20, Color::ACCENT_YELLOW); }

        let tab1_bg = if self.active_tab == 1 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 152, win_y + 30, 85, 20, tab1_bg);
        MinGuiCompositor::draw_string(fb, win_x + 158, win_y + 34, "KediWiki", Color::TEXT_MAIN);
        if self.focused_control == 1 { fb.draw_rect_outline(win_x + 152, win_y + 30, 85, 20, Color::ACCENT_YELLOW); }

        let tab3_bg = if self.active_tab == 3 { Color::WINDOW_BG } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(win_x + 241, win_y + 30, 100, 20, tab3_bg);
        MinGuiCompositor::draw_string(fb, win_x + 247, win_y + 34, "Web Sayfasi", Color::ACCENT_GREEN);
        if self.focused_control == 2 { fb.draw_rect_outline(win_x + 241, win_y + 30, 100, 20, Color::ACCENT_YELLOW); }

        fb.draw_rect(win_x + 6, win_y + 52, win_w - 12, 28, Color::TITLE_BAR);
        fb.draw_rect(win_x + 12, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 18, win_y + 60, "<", Color::TEXT_MAIN);
        if self.focused_control == 3 { fb.draw_rect_outline(win_x + 12, win_y + 56, 22, 20, Color::ACCENT_YELLOW); }

        fb.draw_rect(win_x + 38, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 44, win_y + 60, ">", Color::TEXT_MAIN);
        if self.focused_control == 4 { fb.draw_rect_outline(win_x + 38, win_y + 56, 22, 20, Color::ACCENT_YELLOW); }

        fb.draw_rect(win_x + 64, win_y + 56, 22, 20, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 69, win_y + 60, "R", Color::ACCENT_GREEN);
        if self.focused_control == 5 { fb.draw_rect_outline(win_x + 64, win_y + 56, 22, 20, Color::ACCENT_YELLOW); }

        let url_x = win_x + 94;
        let url_w = win_w.saturating_sub(160);
        fb.draw_rect(url_x, win_y + 56, url_w, 20, if self.address_focused { Color::WINDOW_BG } else { Color::DARK_BG });
        let shown = core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("");
        let mut address = TextBuf::new();
        address.push_str(shown);
        if self.address_focused { address.push_byte(b'_'); }
        MinGuiCompositor::draw_string(fb, url_x + 6, win_y + 60, address.as_str(), Color::TEXT_MAIN);
        if self.focused_control == 6 { fb.draw_rect_outline(url_x, win_y + 56, url_w, 20, Color::ACCENT_YELLOW); }

        let git_r = (win_x + win_w - 60, win_y + 56, 50, 20);
        fb.draw_rect(git_r.0, git_r.1, git_r.2, git_r.3, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + win_w - 48, win_y + 60, "GIT", Color::DARK_BG);
        if self.focused_control == 7 { fb.draw_rect_outline(git_r.0, git_r.1, git_r.2, git_r.3, Color::ACCENT_YELLOW); }

        let page_x = win_x + 6;
        let page_y = win_y + 82;
        let page_w = win_w - 12;
        let page_h = 170;
        fb.draw_rect(page_x, page_y, page_w, page_h, Color::DARK_BG);
        if self.page_loaded {
            if self.active_tab == 3 {
                // Canlı Web Sayfası Render Paneli (HTML DOM)
                fb.draw_rect(page_x + 8, page_y + 8, page_w - 16, 26, Color::TITLE_BAR);
                let title_str = core::str::from_utf8(&self.page_title[..self.page_title_len]).unwrap_or("Web Document");
                MinGuiCompositor::draw_string(fb, page_x + 16, page_y + 14, title_str, Color::ACCENT_GREEN);

                // Beyaz/Açık HTML Kağıt Alanı
                let doc_x = page_x + 8;
                let doc_y = page_y + 38;
                let doc_w = page_w - 16;
                let doc_h = page_h - 46;
                fb.draw_rect(doc_x, doc_y, doc_w, doc_h, Color::WINDOW_BG);

                let mut cur_y = doc_y + 8;
                for i in 0..self.page_line_count {
                    let line_str = core::str::from_utf8(&self.page_lines[i][..self.page_line_lens[i]]).unwrap_or("");
                    let text_color = self.page_line_colors[i];
                    MinGuiCompositor::draw_string(fb, doc_x + 12, cur_y, line_str, text_color);
                    cur_y += 18;
                }
            } else if self.active_tab == 2 {
                // Ağ durumu paneli
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("MinOS | Ağ Durumu (e1000)", "MinOS | Network Status (e1000)"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("ICMP / ARP tabanlı sınama paneli", "ICMP / ARP test panel"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                let net_str = match (self.net_online, english) { (true,true) => "e1000: ONLINE (MMIO active)", (false,true) => "e1000: OFFLINE", (true,false) => "e1000: ÇEVRIMICI (MMIO aktif)", (false,false) => "e1000: ÇEVRIMDIŞI" };
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, net_str, Color::ACCENT_GREEN);
                let ping_str = match (self.ping_ok, english) { (true,true) => "Gateway ARP: FOUND (10.0.2.2)", (false,true) => "Gateway ARP: WAITING / NO REPLY", (true,false) => "Gateway ARP: BULUNDU (10.0.2.2)", (false,false) => "Gateway ARP: BEKLIYOR / YANIT YOK" };
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 84, ping_str, Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 104, "IP: 10.0.2.15/24 GW: 10.0.2.2", Color::TEXT_MUTED);
            } else if self.active_tab == 0 {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("MinOS | Yerel Yardım ve Belgeler", "MinOS | Local Help and Documents"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://cats, minos://help, minos://net", "Local addresses: minos://cats, minos://help, minos://net"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, crate::gui::ui_text("minos://net ile ağ durumunu sınayın.", "Check network status at minos://net."), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 84, crate::gui::ui_text("Yerel sayfalar adres çubuğundan açılabilir.", "Open local pages from the address bar."), Color::TEXT_MUTED);
            } else {
                fb.draw_rect(page_x + 10, page_y + 10, page_w - 20, 36, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 16, crate::gui::ui_text("KediWiki | Yerel Sayfa", "KediWiki | Local Page"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 32, crate::gui::ui_text("Yerel adresler: minos://cats ve minos://help", "Local addresses: minos://cats and minos://help"), Color::ACCENT_BLUE);
                fb.draw_rect(page_x + 10, page_y + 54, page_w - 20, 92, Color::WINDOW_BG);
                MinGuiCompositor::draw_string(fb, page_x + 20, page_y + 64, crate::gui::ui_text("Kediler ve MinOS!", "Cats and MinOS!"), Color::TEXT_MAIN);
            }
        } else {
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 28, crate::gui::ui_text("Bu adres açılamadı.", "This address could not be opened."), Color::ACCENT_RED);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 50, crate::gui::ui_text("Desteklenen adresler: minos://help, minos://cats, minos://net", "Supported: minos://help, minos://cats, minos://net"), Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, page_x + 18, page_y + 72, crate::gui::ui_text("Yerel: minos://help | Ağ: minos://net", "Local: minos://help | Network: minos://net"), Color::TEXT_MUTED);
        }
        fb.draw_rect(win_x + 6, win_y + 256, win_w - 12, 18, Color::TASKBAR_BG);
        let status = match self.status {
            "MinOS yerel yardım sayfası" | "MinOS local help page" => crate::gui::ui_text("MinOS yerel yardım sayfası", "MinOS local help page"),
            "Yerel MinOS yardım sayfası açıldı." | "Local MinOS help page opened." => crate::gui::ui_text("Yerel MinOS yardım sayfası açıldı.", "Local MinOS help page opened."),
            "Yerel KediWiki sayfası açıldı." | "Local KediWiki page opened." => crate::gui::ui_text("Yerel KediWiki sayfası açıldı.", "Local KediWiki page opened."),
            "Ağ durumu sayfası açıldı (sınama gönderildi)." | "Network status page opened (test sent)." => crate::gui::ui_text("Ağ durumu sayfası açıldı (sınama gönderildi).", "Network status page opened (test sent)."),
            "e1000 ağ kartı bulunamadı; ağ yığını pasif." | "e1000 adapter unavailable; network stack is inactive." => crate::gui::ui_text("e1000 ağ kartı bulunamadı; ağ yığını pasif.", "e1000 adapter unavailable; network stack is inactive."),
            "Geçersiz web adresi." | "Invalid web address." => crate::gui::ui_text("Geçersiz web adresi.", "Invalid web address."),
            "Ağ kartı kullanılamıyor." | "Network adapter unavailable." => crate::gui::ui_text("Ağ kartı kullanılamıyor.", "Network adapter unavailable."),
            "DNS çözümlemesi bekleniyor..." | "Resolving host..." => crate::gui::ui_text("DNS çözümlemesi bekleniyor...", "Resolving host..."),
            "DNS adı çözümlenemedi." | "DNS lookup failed." => crate::gui::ui_text("DNS adı çözümlenemedi.", "DNS lookup failed."),
            "TCP bağlantısı kuruluyor..." | "Connecting over TCP..." => crate::gui::ui_text("TCP bağlantısı kuruluyor...", "Connecting over TCP..."),
            "TCP bağlantısı başlatılamadı." | "Could not start TCP connection." => crate::gui::ui_text("TCP bağlantısı başlatılamadı.", "Could not start TCP connection."),
            "Sunucu bağlantıyı reddetti." | "Server rejected the connection." => crate::gui::ui_text("Sunucu bağlantıyı reddetti.", "Server rejected the connection."),
            "HTTP yanıtı bekleniyor..." | "Waiting for HTTP response..." => crate::gui::ui_text("HTTP yanıtı bekleniyor...", "Waiting for HTTP response..."),
            "HTTP yanıtı alındı." | "HTTP response received." => crate::gui::ui_text("HTTP yanıtı alındı.", "HTTP response received."),
            "HTTP yanıtı okunamadı." | "Could not parse HTTP response." => crate::gui::ui_text("HTTP yanıtı okunamadı.", "Could not parse HTTP response."),
            "Ağ isteği zaman aşımına uğradı." | "Network request timed out." => crate::gui::ui_text("Ağ isteği zaman aşımına uğradı.", "Network request timed out."),
            "Desteklenen: http:// ve minos:// adresleri." | "Supported schemes: http:// and minos://." => crate::gui::ui_text("Desteklenen: http:// ve minos:// adresleri.", "Supported schemes: http:// and minos://."),
            "HTTPS/TLS bu çekirdek motorunda desteklenmiyor." | "HTTPS/TLS is not supported by this kernel renderer." => crate::gui::ui_text("HTTPS/TLS bu çekirdek motorunda desteklenmiyor.", "HTTPS/TLS is not supported by this kernel renderer."),
            "DNS sorgusu gönderildi; HTTP bağlantısı hazırlanıyor." | "DNS query sent; preparing HTTP connection." => crate::gui::ui_text("DNS sorgusu gönderildi; HTTP bağlantısı hazırlanıyor.", "DNS query sent; preparing HTTP connection."),
            "DNS sorgusu gönderilemedi." | "DNS query could not be sent." => crate::gui::ui_text("DNS sorgusu gönderilemedi.", "DNS query could not be sent."),
            _ => self.status,
        };
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 260, status, Color::TEXT_MUTED);
    }
}
