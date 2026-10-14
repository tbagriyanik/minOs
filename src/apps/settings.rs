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
/// Yardımcı düğme/geometri tanımları (çizim ve tıklama testi ortak kullanır).
const TAB_X: usize = 10;
const TAB_W: usize = 116;
const TAB_H: usize = 26;
const CONTENT_X: usize = 140;

pub struct SystemSettings {
    // Temel Ayarlar
    pub active_tab: usize, // 0: Genel, 1: Aygıt, 2: Gelişmiş
    pub focused_control: usize,
    pub theme: ThemeId,
    pub keyboard_layout: KeyboardLayout,
    pub language: UiLanguage,
    pub region: u8,
    pub volume: u8,
    pub is_muted: bool,
    pub mouse_speed: u8,
    pub screen_res: (usize, usize),
    pub display_res: (usize, usize),
    pub zoom: u8,
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
            focused_control: 0,
            theme: ThemeId::VelvetDark,
            keyboard_layout: KeyboardLayout::TurkishQ,
            language: UiLanguage::English,
            region: 0,
            volume: 75,
            is_muted: false,
            mouse_speed: 5,
            screen_res: (1280, 720),
            display_res: (1280, 720),
            zoom: 100,
            night_light: false,
            night_light_level: 55,
            saved_window_states: [None; 11],
            dhcp_enabled: true,
            static_ip: [192, 168, 1, 100],
            vfs_mounted: true,
            kernel_debug_log: true,
            status_msg: "Live CD Persistent Storage: Ready",
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
                "dhcp" => self.dhcp_enabled=val!="0",
                "keyboard" => self.keyboard_layout=if val=="en" { KeyboardLayout::EnglishUS } else { KeyboardLayout::TurkishQ },
                "language" => { self.language=if val=="en" { UiLanguage::English } else { UiLanguage::Turkish }; crate::gui::set_ui_language_english(self.language.is_english()); },
                "zoom" => { self.zoom=match val { "125"=>125, "150"=>150, _=>100 }; crate::gui::set_font_zoom(self.zoom); },
                "region" => if let Ok(v)=val.parse::<u8>() { self.region=v.min(2); },
                "res" => { let mut r=val.split('x'); if let (Some(a),Some(b))=(r.next(),r.next()) { if let (Ok(w),Ok(h))=(a.parse(),b.parse()) { self.screen_res=(w,h); } } },
                "theme" => { self.theme = match val { "daylight"=>ThemeId::Daylight, "nord"=>ThemeId::Nord, "terminal"=>ThemeId::TerminalGreen, "sunset"=>ThemeId::Sunset, _=>ThemeId::VelvetDark }; theme::set_theme(self.theme); },
                _ => {}
            }
        }
        self.status_msg=crate::gui::ui_text("Ayarlar kalıcı depolamadan yüklendi.", "Settings loaded from persistent storage.");
    }

    // -----------------------------------------------------------------
    // Geometri yardımcıları (main.rs tıklama testi de bunları kullanır)
    // -----------------------------------------------------------------
    pub fn tab_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + TAB_X, win_y + 38 + i * 28, TAB_W, TAB_H)
    }
    pub fn theme_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(100), win_y + 36, 88, 20)
    }
    pub fn kbd_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(100), win_y + 60, 88, 20)
    }
    pub fn language_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(100), win_y + 84, 88, 20)
    }
    pub fn volume_track_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + 210, 200, 10)
    }
    pub fn mute_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 200, win_y + 204, 96, 22)
    }
    pub fn save_btn_rect(win_x: usize, win_y: usize, win_h: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + win_h.saturating_sub(48), 84, 22)
    }
    pub fn reset_btn_rect(win_x: usize, win_y: usize, win_h: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 96, win_y + win_h.saturating_sub(48), 84, 22)
    }
    pub fn resolution_btn_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + (i % 2) * 158, win_y + 244 + (i / 2) * 30, 150, 24)
    }
    pub fn resolution_name(i: usize) -> &'static str { ["800x600", "1024x768", "1280x720", "1920x1080"][i.min(3)] }
    pub fn zoom_btn_rect(x: usize, y: usize, i: usize) -> (usize,usize,usize,usize) { (x+CONTENT_X+i*72,y+314,66,22) }
    pub fn night_light_rect(x: usize, y: usize) -> (usize,usize,usize,usize) { (x+CONTENT_X+230,y+342,100,24) }
    pub fn night_light_track_rect(x: usize, y: usize) -> (usize,usize,usize,usize) { (x+CONTENT_X,y+392,200,10) }
    pub fn region_btn_rect(x: usize, y: usize) -> (usize,usize,usize,usize) { (x+CONTENT_X+84,y+414,220,24) }
    pub fn dhcp_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) { (win_x + CONTENT_X, win_y + 244, 130, 22) }
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
        let display_res = self.display_res;
        let saved_window_states = self.saved_window_states;
        *self = Self::default();
        self.theme = theme::current();
        self.display_res = display_res;
        self.screen_res = display_res;
        self.saved_window_states = saved_window_states;
        self.status_msg = crate::gui::ui_text("Varsayılan ayarlara dönüldü!", "Settings restored to defaults!");
    }

    /// Ayarları aktif sistem bileşenlerine uygular.
    pub fn apply_runtime(&self, keyboard: &mut crate::drivers::keyboard::KeyboardState, compositor: &mut MinGuiCompositor, now: &mut DateTime) {
        crate::gui::set_font_zoom(self.zoom);
        crate::gui::theme::set_theme(self.theme);
        keyboard.layout = self.keyboard_layout;
        crate::gui::set_ui_language_english(self.language.is_english());
        *now = DateTime::read().with_hour_offset(self.region_offset());
        compositor.cal_year = now.year;
        compositor.cal_month = now.month;
    }

    pub fn cycle_theme(&mut self) {
        self.theme = theme::cycle();
        self.status_msg = crate::gui::ui_text("Tema değiştirildi: Sarman temayı sever!", "Theme changed: Sarman likes this one!");
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
        self.status_msg = crate::gui::ui_text("Ses düzeyi güncellendi.", "Volume updated.");
    }

    pub fn save_all(&mut self, compositor: &MinGuiCompositor, vfs: &mut VirtualFileSystem) {
        for (i, opt_win) in compositor.windows.iter().take(self.saved_window_states.len()).enumerate() {
            if let Some(ref win) = opt_win {
                self.saved_window_states[i] = Some(win.get_state());
            }
        }
        let mut cfg=[0u8;384]; let mut n=0usize;
        fn put(d:&mut [u8], n:&mut usize, s:&[u8]) { let l=core::cmp::min(s.len(),d.len()-*n); d[*n..*n+l].copy_from_slice(&s[..l]); *n+=l; }
        fn put_num(d:&mut [u8], n:&mut usize, value:u8) {
            if value >= 100 { put(d,n,&[b'1',b'0'+(value-100)/10,b'0'+(value-100)%10]); }
            else if value >= 10 { put(d,n,&[b'0'+value/10,b'0'+value%10]); }
            else { put(d,n,&[b'0'+value]); }
        }
        let put_u16 = |d: &mut [u8], n: &mut usize, value: usize| {
            let value = value.min(9999);
            let mut digits = [0u8; 4]; let mut len = 0;
            let mut v = value;
            loop { digits[3-len] = b'0' + (v % 10) as u8; len += 1; v /= 10; if v == 0 || len == 4 { break; } }
            put(d, n, &digits[4-len..]);
        };
        put(&mut cfg,&mut n,b"volume="); put_num(&mut cfg,&mut n,self.volume);
        put(&mut cfg,&mut n,b";muted="); put(&mut cfg,&mut n,if self.is_muted{b"1"}else{b"0"}); put(&mut cfg,&mut n,b";res=");
        put_u16(&mut cfg,&mut n,self.screen_res.0); put(&mut cfg,&mut n,b"x"); put_u16(&mut cfg,&mut n,self.screen_res.1);
        put(&mut cfg,&mut n,b";night="); put(&mut cfg,&mut n,if self.night_light{b"1"}else{b"0"});
        put(&mut cfg,&mut n,b";night_level="); put_num(&mut cfg,&mut n,self.night_light_level);
        put(&mut cfg,&mut n,b";keyboard="); put(&mut cfg,&mut n,if self.keyboard_layout==KeyboardLayout::EnglishUS{b"en"}else{b"tr"});
        put(&mut cfg,&mut n,b";language="); put(&mut cfg,&mut n,if self.language.is_english(){b"en"}else{b"tr"});
        put(&mut cfg,&mut n,b";zoom="); put(&mut cfg,&mut n,match self.zoom { 125=>b"125" as &[u8], 150=>b"150", _=>b"100" });
        put(&mut cfg,&mut n,b";region="); put(&mut cfg,&mut n,&[b'0'+self.region.min(2)]);
        put(&mut cfg,&mut n,b";dhcp="); put(&mut cfg,&mut n,if self.dhcp_enabled{b"1"}else{b"0"});
        put(&mut cfg,&mut n,b";theme="); put(&mut cfg,&mut n,match self.theme { ThemeId::VelvetDark=>b"velvet" as &[u8], ThemeId::Daylight=>b"daylight", ThemeId::Nord=>b"nord", ThemeId::TerminalGreen=>b"terminal", ThemeId::Sunset=>b"sunset" });
        vfs.write_file("settings.cfg",&cfg[..n]);
        self.status_msg = crate::gui::ui_text("Ayarlar ve pencere düzeni kalıcı olarak kaydedildi.", "Settings and window layout saved.");
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

    pub fn focus_next_control(&mut self) {
        // Sekme 0 (Genel) 10 kontrol: 3 sekme + tema + kbd + dil + sustur + kaydet + sıfırla
        // Sekme 1 (Aygıt) 19 kontrol: 3 sekme + 4 çözünürlük + 3 yakınlaştırma + 4 bölge + dhcp + kaydet + sıfırla
        // Sekme 2 (Gelişmiş) 8 kontrol: 3 sekme + mavi ışık + şiddet + tema + kaydet + sıfırla
        let total = match self.active_tab {
            0 => 10,
            1 => 19,
            _ => 8,
        };
        self.focused_control = (self.focused_control + 1) % total;
        if self.focused_control < 3 {
            self.active_tab = self.focused_control;
        }
    }

    pub fn select_tab_prev(&mut self) {
        self.active_tab = (self.active_tab + 2) % 3;
        self.focused_control = self.active_tab;
    }

    pub fn select_tab_next(&mut self) {
        self.active_tab = (self.active_tab + 1) % 3;
        self.focused_control = self.active_tab;
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
        total_ram_bytes: u64,
        cpu_vendor: &'static str,
        cpu_logical_count: u32,
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
            if self.focused_control == i {
                fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::ACCENT_YELLOW);
            }
            let col = if is_cur { Color::ACCENT_BLUE } else { Color::TEXT_MUTED };
            MinGuiCompositor::draw_string_clipped(fb, r.0 + 8, r.1 + 5, t, col,
                (r.0 + 4, r.1 + 2, r.2.saturating_sub(8), r.3.saturating_sub(4)));
        }

        // Sağ İçerik Alanı
        match self.active_tab {
            0 => {
                // --- Tema ---
                let mut theme_str = TextBuf::new();
                theme_str.push_str(if self.language.is_english() { "Theme: " } else { "Tema: " });
                theme_str.push_str(self.theme.name());
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 40, theme_str.as_str(), Color::TEXT_MAIN);
                let theme_r = Self::theme_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(
                    fb,
                    theme_r,
                    Color::ACCENT_BLUE,
                    if self.language.is_english() { "Change" } else { "Değiştir" },
                    Color::DARK_BG,
                    false,
                );
                if self.focused_control == 4 { fb.draw_rect_outline(theme_r.0, theme_r.1, theme_r.2, theme_r.3, Color::ACCENT_YELLOW); }

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
                let kbd_r = Self::kbd_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(
                    fb,
                    kbd_r,
                    Color::ACCENT_PINK,
                    if self.language.is_english() { "Layout" } else { "Düzen" },
                    Color::DARK_BG,
                    false,
                );
                if self.focused_control == 5 { fb.draw_rect_outline(kbd_r.0, kbd_r.1, kbd_r.2, kbd_r.3, Color::ACCENT_YELLOW); }

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 88,
                    if self.language.is_english() { "Interface language" } else { "Arayüz dili" }, Color::ACCENT_PINK);
                let lang_r = Self::language_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(fb, lang_r, Color::ACCENT_PINK,
                    if self.language.is_english() { "Turkish" } else { "English" }, Color::DARK_BG, false);
                if self.focused_control == 6 { fb.draw_rect_outline(lang_r.0, lang_r.1, lang_r.2, lang_r.3, Color::ACCENT_YELLOW); }

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
                date.push_str(if self.language.is_english() { DateTime::month_name_en(now.month) } else { DateTime::month_name(now.month) });
                date.push_byte(b' ');
                date.push_u4(now.year as u32);
                date.push_str(", ");
                let weekday = DateTime::weekday(now.year, now.month, now.day);
                date.push_str(if self.language.is_english() { DateTime::weekday_name_en(weekday) } else { DateTime::weekday_name(weekday) });
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
                if self.focused_control == 8 { fb.draw_rect_outline(mute_r.0, mute_r.1, mute_r.2, mute_r.3, Color::ACCENT_YELLOW); }
            }
            1 => {
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, crate::gui::ui_text("Çekirdek Günlüğü: Aktif (/dev/klog)", "Kernel Log: Active (/dev/klog)"), Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, crate::gui::ui_text("VFS Sürücüsü: RamFS TarFS v1.0", "VFS Driver: RamFS TarFS v1.0"), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 90, crate::gui::ui_text("Donanım Hızlandırma: SSE2 Aktif", "Hardware Acceleration: SSE2 Active"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 114, crate::gui::ui_text("Önyükleme: Limine Stivale2", "Bootloader: Limine Stivale2"), Color::TEXT_MUTED);
                let mut audio_status = TextBuf::new();
                audio_status.push_str(crate::gui::ui_text("Ses sürücüsü: ", "Audio driver: "));
                audio_status.push_str(audio_driver);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 138, audio_status.as_str(), Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 162, crate::gui::ui_text("Tema Motoru: 3 hazır tema", "Theme Engine: 3 available themes"), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 178,
                    if self.language.is_english() { "Target resolution:" } else { "Hedef çözünürlük:" }, Color::ACCENT_YELLOW);
                for i in 0..4 {
                    let r = Self::resolution_btn_rect(win_x,win_y,i);
                    let active = self.screen_res == match i {0=>(800,600),1=>(1024,768),2=>(1280,720),_=>(1920,1080)};
                    MinGuiCompositor::draw_button(fb, r, if active{Color::ACCENT_GREEN}else{Color::TITLE_BAR}, Self::resolution_name(i), Color::TEXT_MAIN, false);
                    if self.focused_control == 4 + i { fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::ACCENT_YELLOW); }
                }
                MinGuiCompositor::draw_string(fb, win_x+CONTENT_X, win_y+250, if self.language.is_english() { "Font zoom:" } else { "Yazı yakınlaştırma:" }, Color::TEXT_MAIN);
                for (i, value) in [100u8,125,150].iter().enumerate() {
                    let label = match value { 125=>"125%", 150=>"150%", _=>"100%" };
                    let r = Self::zoom_btn_rect(win_x,win_y,i);
                    MinGuiCompositor::draw_button(fb, r, if self.zoom==*value{Color::ACCENT_GREEN}else{Color::TITLE_BAR}, label, Color::TEXT_MAIN, false);
                    if self.focused_control == 8 + i { fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::ACCENT_YELLOW); }
                }
                MinGuiCompositor::draw_string(fb, win_x+CONTENT_X, win_y+294, match (self.night_light,self.language.is_english()) { (true,true)=>"Blue light: On", (false,true)=>"Blue light: Off", (true,false)=>"Mavi ışık: Açık", (false,false)=>"Mavi ışık: Kapalı" }, Color::ACCENT_YELLOW);
                let nl_btn = Self::night_light_rect(win_x,win_y);
                MinGuiCompositor::draw_button(fb, nl_btn, Color::ACCENT_YELLOW, match (self.night_light,self.language.is_english()) { (true,true)=>"Off", (false,true)=>"On", (true,false)=>"Kapat", (false,false)=>"Aç" }, Color::DARK_BG, false);
                if self.focused_control == 11 { fb.draw_rect_outline(nl_btn.0, nl_btn.1, nl_btn.2, nl_btn.3, Color::ACCENT_YELLOW); }

                MinGuiCompositor::draw_string(fb, win_x+CONTENT_X, win_y+324, if self.language.is_english() { "Blue light strength:" } else { "Mavi ışık şiddeti:" }, Color::TEXT_MAIN);
                let nl = Self::night_light_track_rect(win_x,win_y);
                fb.draw_rect(nl.0,nl.1,nl.2,nl.3,Color::TITLE_BAR_INACTIVE);
                let fill = nl.2*self.night_light_level as usize/100;
                fb.draw_rect(nl.0,nl.1,fill,nl.3,Color::ACCENT_YELLOW);
                fb.draw_rect(nl.0+fill.saturating_sub(4),nl.1.saturating_sub(4),8,nl.3+8,Color::TEXT_MAIN);
                let mut level_text = TextBuf::new(); level_text.push_usize(self.night_light_level as usize); level_text.push_byte(b'%');
                MinGuiCompositor::draw_string(fb,nl.0+nl.2+8,nl.1+1,level_text.as_str(),Color::TEXT_MAIN);

                MinGuiCompositor::draw_string(fb, win_x+CONTENT_X, win_y+362, if self.language.is_english() { "Region:" } else { "Bölge:" }, Color::TEXT_MAIN);
                let reg_r = Self::region_btn_rect(win_x,win_y);
                MinGuiCompositor::draw_button(fb, reg_r, Color::ACCENT_BLUE, self.region_name(self.language.is_english()), Color::TEXT_MAIN, false);
                if self.focused_control == 12 { fb.draw_rect_outline(reg_r.0, reg_r.1, reg_r.2, reg_r.3, Color::ACCENT_YELLOW); }
            }
            2 => {
                let mut driver = TextBuf::new();
                driver.push_str(crate::gui::ui_text("Ağ sürücüsü: ", "Network driver: "));
                driver.push_str(network_driver);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, driver.as_str(), Color::ACCENT_BLUE);
                let mut status = TextBuf::new();
                status.push_str(crate::gui::ui_text("Durum: ", "Status: "));
                status.push_str(network_status);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, status.as_str(), Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string_clipped(fb, win_x + CONTENT_X, win_y + 90,
                    crate::gui::ui_text("Yığın: Ethernet / IPv4 / ARP / ICMP / UDP / TCP / DHCP / DNS", "Stack: Ethernet / IPv4 / ARP / ICMP / UDP / TCP / DHCP / DNS"),
                    Color::TEXT_MUTED, (win_x + CONTENT_X, win_y + 88, win_w.saturating_sub(CONTENT_X + 12), 18));
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 116, if self.dhcp_enabled { crate::gui::ui_text("IP yöntemi: DHCP", "IP mode: DHCP") } else { crate::gui::ui_text("IP yöntemi: Statik", "IP mode: Static") }, Color::ACCENT_YELLOW);
                let mut ip = TextBuf::new(); ip.push_str("IP: "); for (i, b) in self.static_ip.iter().enumerate() { if i > 0 { ip.push_byte(b'.'); } ip.push_usize(*b as usize); }
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 138, ip.as_str(), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 160, "Gateway: 10.0.2.2    DNS: 10.0.2.3", Color::TEXT_MAIN);
                let dhcp_btn = (win_x + CONTENT_X, win_y + 178, 130, 22);
                MinGuiCompositor::draw_button(fb, dhcp_btn, Color::ACCENT_BLUE, if self.dhcp_enabled { "Static IP" } else { "DHCP" }, Color::DARK_BG, false);
                if self.focused_control == 4 { fb.draw_rect_outline(dhcp_btn.0, dhcp_btn.1, dhcp_btn.2, dhcp_btn.3, Color::ACCENT_YELLOW); }
            }
            _ => {
                let mut version = TextBuf::new();
                version.push_str("minOs - Sarman Edition (v");
                version.push_str(env!("CARGO_PKG_VERSION"));
                version.push_byte(b')');
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, version.as_str(), Color::ACCENT_BLUE);
                let mut cpu = TextBuf::new();
                cpu.push_str("CPU: "); cpu.push_str(cpu_vendor); cpu.push_str(" x86_64 | CPUID ");
                cpu.push_u2(cpu_logical_count); cpu.push_str(crate::gui::ui_text(" mantıksal işlemci", " logical CPU(s)"));
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, cpu.as_str(), Color::TEXT_MAIN);
                let mut ram_info = TextBuf::new();
                ram_info.push_str(crate::gui::ui_text("Çekirdeğe kullanılabilir RAM: ", "RAM usable by kernel: "));
                ram_info.push_usize((total_ram_bytes / (1024 * 1024)) as usize);
                ram_info.push_str(" MB");
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 90, ram_info.as_str(), Color::ACCENT_YELLOW);
                let mut display = TextBuf::new();
                display.push_str(crate::gui::ui_text("Ekran: ", "Display: "));
                display.push_usize(self.display_res.0); display.push_byte(b'x'); display.push_usize(self.display_res.1);
                display.push_str(" | 32 bpp");
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 114, display.as_str(), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 138,
                    crate::gui::ui_text("Fare: PS/2 | 3 düğme desteği", "Mouse: PS/2 | 3-button support"), Color::ACCENT_PINK);
                let keyboard = match kbd_layout {
                    KeyboardLayout::TurkishQ => crate::gui::ui_text("Klavye: PS/2 | Türkçe Q", "Keyboard: PS/2 | Turkish Q"),
                    KeyboardLayout::EnglishUS => crate::gui::ui_text("Klavye: PS/2 | English US", "Keyboard: PS/2 | English US"),
                };
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 162, keyboard, Color::ACCENT_GREEN);
                let mut network = TextBuf::new();
                network.push_str(crate::gui::ui_text("Ağ: ", "Network: "));
                network.push_str(network_driver); network.push_str(" | "); network.push_str(network_status);
                MinGuiCompositor::draw_string_clipped(fb, win_x + CONTENT_X, win_y + 186, network.as_str(), Color::ACCENT_BLUE,
                    (win_x + CONTENT_X, win_y + 184, win_w.saturating_sub(CONTENT_X + 12), 18));
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 214,
                    crate::gui::ui_text("SMP: çok çekirdekli iş planlaması etkin değil", "SMP: multi-core scheduling is not enabled"), Color::TEXT_MUTED);
            }
        }

        // Alt Aksiyon Butonları (Kaydet & Sıfırla)
        let save_r = Self::save_btn_rect(win_x, win_y, win_h);
        let reset_r = Self::reset_btn_rect(win_x, win_y, win_h);
        MinGuiCompositor::draw_button(fb, save_r, Color::ACCENT_GREEN, crate::gui::ui_text("Kaydet", "Save"), Color::DARK_BG, false);
        MinGuiCompositor::draw_button(fb, reset_r, Color::ACCENT_RED, crate::gui::ui_text("Sıfırla", "Reset"), Color::WHITE, false);

        let (save_idx, reset_idx) = match self.active_tab {
            0 => (9, 10),
            1 => (17, 18),
            _ => (6, 7),
        };
        if self.focused_control == save_idx { fb.draw_rect_outline(save_r.0, save_r.1, save_r.2, save_r.3, Color::ACCENT_YELLOW); }
        if self.focused_control == reset_idx { fb.draw_rect_outline(reset_r.0, reset_r.1, reset_r.2, reset_r.3, Color::ACCENT_YELLOW); }

        // Durum Çubuğu
        let bar_y = win_y + win_h.saturating_sub(22);
        fb.draw_rect(win_x + 6, bar_y, win_w.saturating_sub(12), 20, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, bar_y + 6, self.status_msg, Color::ACCENT_YELLOW);
    }
}
