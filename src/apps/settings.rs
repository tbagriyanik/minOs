use crate::drivers::video::Framebuffer;
use crate::drivers::keyboard::KeyboardLayout;
use crate::drivers::rtc::DateTime;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::gui::theme::{self, ThemeId};

use crate::gui::WindowState;
use crate::fs::vfs::VirtualFileSystem;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UiLanguage { Turkish, English }
impl UiLanguage { pub fn toggle(&mut self) { *self = match *self { Self::Turkish => Self::English, Self::English => Self::Turkish }; crate::gui::set_ui_language_english(self.is_english()); } pub fn is_english(&self) -> bool { matches!(self, Self::English) } }
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum FontChoice { TurkishPixel, Compact, ClearType }

impl FontChoice {
    /// Seçimi GUI font motoruna uygular (tek gerçek kaynak).
    pub fn apply(self) {
        let mode = match self {
            FontChoice::TurkishPixel => crate::gui::FontMode::Pixel,
            FontChoice::Compact => crate::gui::FontMode::Compact,
            FontChoice::ClearType => crate::gui::FontMode::ClearType,
        };
        crate::gui::set_font_mode(mode);
    }

    pub fn label(self, english: bool) -> &'static str {
        match (self, english) {
            (FontChoice::TurkishPixel, false) => "Yazı tipi: Türkçe Piksel",
            (FontChoice::TurkishPixel, true) => "Font: Turkish Pixel",
            (FontChoice::Compact, false) => "Yazı tipi: Sıkıştırılmış",
            (FontChoice::Compact, true) => "Font: Compact",
            (FontChoice::ClearType, false) => "Yazı tipi: ClearType",
            (FontChoice::ClearType, true) => "Font: ClearType",
        }
    }

    pub fn cycle(&mut self) {
        *self = match self {
            FontChoice::TurkishPixel => FontChoice::Compact,
            FontChoice::Compact => FontChoice::ClearType,
            FontChoice::ClearType => FontChoice::TurkishPixel,
        };
        self.apply();
    }
}

/// Yardımcı düğme/geometri tanımları (çizim ve tıklama testi ortak kullanır).
const TAB_X: usize = 10;
const TAB_W: usize = 90;
const TAB_H: usize = 22;
const CONTENT_X: usize = 115;

pub struct SystemSettings {
    // Temel Ayarlar
    pub active_tab: usize, // 0: Temel, 1: İleri, 2: Ağ, 3: Hakkında
    pub theme: ThemeId,
    pub keyboard_layout: KeyboardLayout,
    pub language: UiLanguage,
    pub font: FontChoice,
    pub region: u8,
    pub volume: u8,
    pub is_muted: bool,
    pub mouse_speed: u8,
    pub screen_res: (usize, usize),
    pub display_res: (usize, usize),
    pub night_light: bool,
    pub night_light_level: u8,

    // Saklanan Pencere Konum ve Durumları (8 Pencere)
    pub saved_window_states: [Option<WindowState>; 11],

    // İleri Seviye Ayarlar
    pub dhcp_enabled: bool,
    pub static_ip: [u8; 4],
    pub vfs_mounted: bool,
    pub kernel_debug_log: bool,
    pub status_msg: &'static str,
}

impl SystemSettings {
    pub const fn default() -> Self {
        Self {
            active_tab: 0,
            theme: ThemeId::VelvetDark,
            keyboard_layout: KeyboardLayout::TurkishQ,
            language: UiLanguage::English,
            font: FontChoice::TurkishPixel,
            region: 0,
            volume: 75,
            is_muted: false,
            mouse_speed: 5,
            screen_res: (1280, 720),
            display_res: (1280, 720),
            night_light: false,
            night_light_level: 55,
            saved_window_states: [None; 11],
            dhcp_enabled: true,
            static_ip: [192, 168, 1, 100],
            vfs_mounted: true,
            kernel_debug_log: true,
            status_msg: "Canlı CD Kalıcı Depolama: Hazır",
        }
    }

    pub fn load_from_vfs(&mut self, vfs: &VirtualFileSystem) {
        let data = match vfs.read_file("settings.cfg") { Some(d) => d, None => return };
        let text = match core::str::from_utf8(data) { Ok(s) => s, Err(_) => return };
        for item in text.split(';') {
            let mut p = item.split('='); let key=p.next().unwrap_or(""); let val=p.next().unwrap_or("");
            match key {
                "volume" => if let Ok(v)=val.parse::<u8>() { self.volume=v.min(100); },
                "muted" => self.is_muted=val=="1",
                "night" => self.night_light=val=="1",
                "night_level" => if let Ok(v)=val.parse::<u8>() { self.night_light_level=v.min(100); },
                "keyboard" => self.keyboard_layout=if val=="en" { KeyboardLayout::EnglishUS } else { KeyboardLayout::TurkishQ },
                "language" => { self.language=if val=="en" { UiLanguage::English } else { UiLanguage::Turkish }; crate::gui::set_ui_language_english(self.language.is_english()); },
                "font" => { self.font=match val { "compact"=>FontChoice::Compact, "cleartype"=>FontChoice::ClearType, _=>FontChoice::TurkishPixel }; self.font.apply(); },
                "region" => if let Ok(v)=val.parse::<u8>() { self.region=v.min(2); },
                "res" => { let mut r=val.split('x'); if let (Some(a),Some(b))=(r.next(),r.next()) { if let (Ok(w),Ok(h))=(a.parse(),b.parse()) { self.screen_res=(w,h); } } },
                _ => {}
            }
        }
        self.status_msg="Ayarlar kalıcı depolamadan yüklendi.";
    }

    // -----------------------------------------------------------------
    // Geometri yardımcıları (main.rs tıklama testi de bunları kullanır)
    // -----------------------------------------------------------------
    pub fn tab_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + TAB_X, win_y + 38 + i * 24, TAB_W, TAB_H)
    }
    pub fn theme_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(100), win_y + 36, 88, 20)
    }
    pub fn kbd_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(100), win_y + 60, 88, 20)
    }
    pub fn font_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(100), win_y + 112, 88, 20)
    }
    pub fn language_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(100), win_y + 84, 88, 20)
    }
    pub fn volume_track_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + 210, 200, 10)
    }
    pub fn mute_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 210, win_y + 206, 66, 18)
    }
    pub fn save_btn_rect(win_x: usize, win_y: usize, win_h: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + win_h.saturating_sub(44), 90, 22)
    }
    pub fn reset_btn_rect(win_x: usize, win_y: usize, win_h: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 100, win_y + win_h.saturating_sub(44), 95, 22)
    }
    pub fn resolution_btn_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + i * 82, win_y + 198, 76, 22)
    }
    pub fn resolution_name(i: usize) -> &'static str { ["800x600", "1024x768", "1280x720", "1920x1080"][i.min(3)] }
    pub fn night_light_rect(x: usize, y: usize) -> (usize,usize,usize,usize) { (x+CONTENT_X+250,y+232,110,22) }
    pub fn night_light_track_rect(x: usize, y: usize) -> (usize,usize,usize,usize) { (x+CONTENT_X,y+272,200,10) }
    pub fn region_btn_rect(x: usize, y: usize) -> (usize,usize,usize,usize) { (x+CONTENT_X+160,y+296,190,22) }
    pub fn region_offset(&self) -> i8 { match self.region { 1 => 0, 2 => -5, _ => 3 } }
    pub fn region_name(&self, english: bool) -> &'static str {
        match (self.region, english) { (1,true)=>"UTC", (1,false)=>"UTC", (2,true)=>"US Eastern (UTC-5)", (2,false)=>"ABD Doğu (UTC-5)", (_,true)=>"Türkiye (UTC+3)", (_,false)=>"Türkiye (UTC+3)" }
    }
    /// Tema palet şeridi: her tema için bir renk örneği kutusu.
    pub fn theme_swatch_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + i * 24, win_y + 264, 20, 16)
    }
    /// Tema adı etiketi (örneklerin altında).
    pub fn theme_swatch_count() -> usize {
        ThemeId::ALL.len()
    }

    // -----------------------------------------------------------------
    // Aksiyonlar
    // -----------------------------------------------------------------
    pub fn reset_defaults(&mut self) {
        *self = Self::default();
        self.theme = theme::current();
        self.status_msg = "Varsayılan ayarlara dönüldü!";
    }

    pub fn cycle_theme(&mut self) {
        self.theme = theme::cycle();
        self.status_msg = "Tema değiştirildi: Sarman temayı sever!";
    }

    pub fn set_volume_from_click(&mut self, x: usize) {
        let t = Self::volume_track_rect(0, 0);
        self.set_volume_from_range(x, t.0, t.2);
    }
    pub fn set_volume_from_range(&mut self, x: usize, start: usize, width: usize) {
        let span = width.saturating_sub(1).max(1);
        let position = x.saturating_sub(start).min(span);
        let ratio = (position * 100 / span).min(100);
        self.volume = ratio as u8;
        if self.is_muted && self.volume > 0 {
            self.is_muted = false;
        }
        self.status_msg = "Ses düzeyi güncellendi.";
    }

    pub fn save_all(&mut self, compositor: &MinGuiCompositor, vfs: &mut VirtualFileSystem) {
        for (i, opt_win) in compositor.windows.iter().enumerate() {
            if let Some(ref win) = opt_win {
                self.saved_window_states[i] = Some(win.get_state());
            }
        }
        let (w,_h)=self.screen_res;
        let mut cfg=[0u8;192]; let mut n=0usize;
        fn put(d:&mut [u8], n:&mut usize, s:&[u8]) { let l=core::cmp::min(s.len(),d.len()-*n); d[*n..*n+l].copy_from_slice(&s[..l]); *n+=l; }
        fn put_num(d:&mut [u8], n:&mut usize, value:u8) {
            if value >= 100 { put(d,n,&[b'1',b'0'+(value-100)/10,b'0'+(value-100)%10]); }
            else if value >= 10 { put(d,n,&[b'0'+value/10,b'0'+value%10]); }
            else { put(d,n,&[b'0'+value]); }
        }
        put(&mut cfg,&mut n,b"volume="); put_num(&mut cfg,&mut n,self.volume);
        put(&mut cfg,&mut n,b";muted="); put(&mut cfg,&mut n,if self.is_muted{b"1"}else{b"0"}); put(&mut cfg,&mut n,b";res=");
        if w==800 {put(&mut cfg,&mut n,b"800x600")} else if w==1024 {put(&mut cfg,&mut n,b"1024x768")} else if w==1920 {put(&mut cfg,&mut n,b"1920x1080")} else {put(&mut cfg,&mut n,b"1280x720")};
        put(&mut cfg,&mut n,b";night="); put(&mut cfg,&mut n,if self.night_light{b"1"}else{b"0"});
        put(&mut cfg,&mut n,b";night_level="); put_num(&mut cfg,&mut n,self.night_light_level);
        put(&mut cfg,&mut n,b";keyboard="); put(&mut cfg,&mut n,if self.keyboard_layout==KeyboardLayout::EnglishUS{b"en"}else{b"tr"});
        put(&mut cfg,&mut n,b";language="); put(&mut cfg,&mut n,if self.language.is_english(){b"en"}else{b"tr"});
        put(&mut cfg,&mut n,b";font="); put(&mut cfg,&mut n,match self.font{FontChoice::TurkishPixel=>&b"pixel"[..],FontChoice::Compact=>&b"compact"[..],FontChoice::ClearType=>&b"cleartype"[..]});
        put(&mut cfg,&mut n,b";region="); put(&mut cfg,&mut n,&[b'0'+self.region.min(2)]);
        vfs.write_file("settings.cfg",&cfg[..n]);
        self.status_msg = "Ayarlar ve pencere düzeni kalıcı olarak kaydedildi.";
    }

    pub fn restore_windows(&self, compositor: &mut MinGuiCompositor) {
        for (i, opt_state) in self.saved_window_states.iter().enumerate() {
            if let Some(state) = opt_state {
                if let Some(ref mut win) = compositor.windows[i] {
                    win.apply_state(*state);
                }
            }
        }
    }

    // -----------------------------------------------------------------
    // Çizim
    // -----------------------------------------------------------------
    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        kbd_layout: KeyboardLayout,
        now: &DateTime,
        audio_driver: &'static str,
        network_driver: &'static str,
        network_status: &'static str,
    ) {
        // Sol Menü / Sekmeler
        fb.draw_rect(win_x + 6, win_y + 32, TAB_W + 8, win_h.saturating_sub(62), Color::TITLE_BAR_INACTIVE);
        let tabs = if self.language.is_english() { ["Basic", "Advanced", "Network", "About"] } else { ["Temel", "İleri", "Ağ", "Hakkında"] };
        for (i, t) in tabs.iter().enumerate() {
            let r = Self::tab_rect(win_x, win_y, i);
            let is_cur = i == self.active_tab;
            let bg = if is_cur { Color::TITLE_BAR } else { Color::TITLE_BAR_INACTIVE };
            fb.draw_rect(r.0, r.1, r.2, r.3, bg);
            if is_cur {
                fb.draw_rect(r.0, r.1, 3, r.3, Color::ACCENT_BLUE);
            }
            let col = if is_cur { Color::ACCENT_BLUE } else { Color::TEXT_MUTED };
            MinGuiCompositor::draw_string(fb, r.0 + 10, r.1 + 7, t, col);
        }

        // Sağ İçerik Alanı
        match self.active_tab {
            0 => {
                // --- Tema ---
                let mut theme_str = TextBuf::new();
                theme_str.push_str(if self.language.is_english() { "Theme: " } else { "Tema: " });
                theme_str.push_str(self.theme.name());
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 40, theme_str.as_str(), Color::TEXT_MAIN);
                MinGuiCompositor::draw_button(
                    fb,
                    Self::theme_btn_rect(win_x, win_y, win_w),
                    Color::ACCENT_BLUE,
                    if self.language.is_english() { "Change" } else { "Değiştir" },
                    Color::DARK_BG,
                    false,
                );
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 116,
                    self.font.label(self.language.is_english()), Color::ACCENT_YELLOW);
                MinGuiCompositor::draw_button(fb, Self::font_btn_rect(win_x, win_y, win_w), Color::ACCENT_YELLOW,
                    if self.language.is_english() { "Change" } else { "Değiştir" }, Color::DARK_BG, false);

                // --- Tema Palet Seridi (dogrudan secim icin renk ornekleri) ---
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 250,
                    if self.language.is_english() { "Palette:" } else { "Palet:" }, Color::TEXT_MUTED);
                for (i, tid) in ThemeId::ALL.iter().enumerate() {
                    let r = Self::theme_swatch_rect(win_x, win_y, i);
                    let (bg, accent) = theme::preview_colors(*tid);
                    fb.draw_rect(r.0, r.1, r.2, r.3, bg);
                    fb.draw_rect(r.0 + 4, r.1 + 4, r.2 - 8, r.3 - 8, accent);
                    let border = if *tid == self.theme { Color::ACCENT_GREEN } else { Color::BORDER };
                    fb.draw_rect_outline(r.0, r.1, r.2, r.3, border);
                }

                // --- Klavye / Dil ---
                let kbd_str = match kbd_layout {
                    KeyboardLayout::TurkishQ => if self.language.is_english() { "Keyboard layout: Turkish (TR-Q)" } else { "Klavye düzeni: Türkçe (TR-Q)" },
                    KeyboardLayout::EnglishUS => "Keyboard: English (EN-US)",
                };
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 64, kbd_str, Color::ACCENT_PINK);
                MinGuiCompositor::draw_button(
                    fb,
                    Self::kbd_btn_rect(win_x, win_y, win_w),
                    Color::ACCENT_PINK,
                    if self.language.is_english() { "Layout" } else { "Düzen" },
                    Color::DARK_BG,
                    false,
                );
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 88,
                    if self.language.is_english() { "Interface language" } else { "Arayüz dili" }, Color::ACCENT_PINK);
                MinGuiCompositor::draw_button(fb, Self::language_btn_rect(win_x, win_y, win_w), Color::ACCENT_PINK,
                    if self.language.is_english() { "Turkish" } else { "English" }, Color::DARK_BG, false);

                // --- Canlı Saat ---
                let mut clock = TextBuf::new();
                clock.push_str(if self.language.is_english() { "Time: " } else { "Saat: " });
                clock.push_u2(now.hour as u32);
                clock.push_byte(b':');
                clock.push_u2(now.minute as u32);
                clock.push_byte(b':');
                clock.push_u2(now.second as u32);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 142, clock.as_str(), Color::ACCENT_GREEN);

                // --- Tarih ---
                let mut date = TextBuf::new();
                date.push_str(if self.language.is_english() { "Date: " } else { "Tarih: " });
                date.push_u2(now.day as u32);
                date.push_byte(b' ');
                date.push_str(DateTime::month_name(now.month));
                date.push_byte(b' ');
                date.push_u4(now.year as u32);
                date.push_str(", ");
                date.push_str(DateTime::weekday_name(DateTime::weekday(now.year, now.month, now.day)));
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 164, date.as_str(), Color::TEXT_MAIN);

                // --- Ses ---
                let mut vol = TextBuf::new();
                vol.push_str(if self.language.is_english() { "Volume: %" } else { "Ses Düzeyi: %" });
                vol.push_usize(self.volume as usize);
                if self.is_muted {
                    vol.push_str(if self.language.is_english() { " (MUTED)" } else { " (SUSTURULDU)" });
                }
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 188, vol.as_str(), Color::TEXT_MAIN);

                let track = Self::volume_track_rect(win_x, win_y);
                fb.draw_rect(track.0, track.1, track.2, track.3, Color::TITLE_BAR_INACTIVE);
                let fill_w = (track.2 * self.volume as usize) / 100;
                let fill_col = if self.is_muted { Color::TEXT_MUTED } else { Color::ACCENT_GREEN };
                fb.draw_rect(track.0, track.1, fill_w, track.3, fill_col);
                fb.draw_rect(track.0 + fill_w.saturating_sub(4), track.1 - 4, 8, track.3 + 8, Color::TEXT_MAIN);

                let mute_r = Self::mute_btn_rect(win_x, win_y);
                let mute_bg = if self.is_muted { Color::ACCENT_RED } else { Color::TITLE_BAR };
                let mute_lbl = match (self.is_muted, self.language.is_english()) { (true,true)=>"Unmute", (false,true)=>"Mute", (true,false)=>"Sesi Aç", (false,false)=>"Sustur" };
                MinGuiCompositor::draw_button(fb, mute_r, mute_bg, mute_lbl, Color::TEXT_MAIN, false);
            }
            1 => {
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, "Çekirdek Günlüğü: Aktif (/dev/klog)", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, "VFS Sürücüsü: RamFS TarFS v1.0", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 90, "Donanım Hızlandırma: SSE2 Aktif", Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 114, "Önyükleme: Limine Stivale2", Color::TEXT_MUTED);
                let mut audio_status = TextBuf::new();
                audio_status.push_str("Ses sürücüsü: ");
                audio_status.push_str(audio_driver);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 138, audio_status.as_str(), Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 162, "Tema Motoru: 3 hazır tema hazır", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 176,
                    if self.language.is_english() { "Target resolution:" } else { "Hedef çözünürlük:" }, Color::ACCENT_YELLOW);
                for i in 0..4 { let r=Self::resolution_btn_rect(win_x,win_y,i); let active=self.screen_res == match i {0=>(800,600),1=>(1024,768),2=>(1280,720),_=>(1920,1080)}; MinGuiCompositor::draw_button(fb,r,if active{Color::ACCENT_GREEN}else{Color::TITLE_BAR},Self::resolution_name(i),Color::TEXT_MAIN,false); }
                MinGuiCompositor::draw_string(fb, win_x+CONTENT_X, win_y+236, match (self.night_light,self.language.is_english()) { (true,true)=>"Blue light: On", (false,true)=>"Blue light: Off", (true,false)=>"Mavi ışık: Açık", (false,false)=>"Mavi ışık: Kapalı" }, Color::ACCENT_YELLOW);
                MinGuiCompositor::draw_button(fb, Self::night_light_rect(win_x,win_y), Color::ACCENT_YELLOW, match (self.night_light,self.language.is_english()) { (true,true)=>"Off", (false,true)=>"On", (true,false)=>"Kapat", (false,false)=>"Aç" }, Color::DARK_BG, false);
                MinGuiCompositor::draw_string(fb, win_x+CONTENT_X, win_y+260, if self.language.is_english() { "Blue light strength:" } else { "Mavi ışık şiddeti:" }, Color::TEXT_MAIN);
                let nl = Self::night_light_track_rect(win_x,win_y);
                fb.draw_rect(nl.0,nl.1,nl.2,nl.3,Color::TITLE_BAR_INACTIVE);
                let fill = nl.2*self.night_light_level as usize/100;
                fb.draw_rect(nl.0,nl.1,fill,nl.3,Color::ACCENT_YELLOW);
                fb.draw_rect(nl.0+fill.saturating_sub(4),nl.1.saturating_sub(4),8,nl.3+8,Color::TEXT_MAIN);
                let mut level_text = TextBuf::new(); level_text.push_usize(self.night_light_level as usize); level_text.push_byte(b'%');
                MinGuiCompositor::draw_string(fb,nl.0+nl.2+8,nl.1+1,level_text.as_str(),Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x+CONTENT_X, win_y+296, if self.language.is_english() { "Region:" } else { "Bölge:" }, Color::TEXT_MAIN);
                MinGuiCompositor::draw_button(fb, Self::region_btn_rect(win_x,win_y), Color::ACCENT_BLUE, self.region_name(self.language.is_english()), Color::TEXT_MAIN, false);
            }
            2 => {
                let mut driver = TextBuf::new();
                driver.push_str("Ağ sürücüsü: ");
                driver.push_str(network_driver);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, driver.as_str(), Color::ACCENT_BLUE);
                let mut status = TextBuf::new();
                status.push_str("Durum: ");
                status.push_str(network_status);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, status.as_str(), Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 90, "Ağ yığını: IPv4 / ARP / ICMP", Color::TEXT_MUTED);
            }
            _ => {
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, "MinOS - Sarman Edition (v1.1)", Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, "Geliştirici: Sarman Çekirdek Ekibi", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 90, "Özel Sürüm: Live CD Persist Store", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 114, "github.com/tbagriyanik/minos", Color::ACCENT_PINK);
            }
        }

        // Alt Aksiyon Butonları (Kaydet & Sıfırla)
        MinGuiCompositor::draw_button(fb, Self::save_btn_rect(win_x, win_y, win_h), Color::ACCENT_GREEN, "Kaydet", Color::DARK_BG, false);
        MinGuiCompositor::draw_button(fb, Self::reset_btn_rect(win_x, win_y, win_h), Color::ACCENT_RED, "Sıfırla", Color::WHITE, false);

        // Durum Çubuğu
        let bar_y = win_y + win_h.saturating_sub(26);
        fb.draw_rect(win_x + 6, bar_y, win_w.saturating_sub(12), 20, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, bar_y + 6, self.status_msg, Color::ACCENT_YELLOW);
    }
}
