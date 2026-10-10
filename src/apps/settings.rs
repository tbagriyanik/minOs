use crate::drivers::video::Framebuffer;
use crate::drivers::keyboard::KeyboardLayout;
use crate::drivers::rtc::DateTime;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::gui::theme::{self, ThemeId};

use crate::gui::WindowState;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UiLanguage { Turkish, English }
impl UiLanguage { pub fn toggle(&mut self) { *self = match *self { Self::Turkish => Self::English, Self::English => Self::Turkish }; } pub fn is_english(&self) -> bool { matches!(self, Self::English) } }
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum FontChoice { TurkishPixel, Compact }

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
    pub volume: u8,
    pub is_muted: bool,
    pub mouse_speed: u8,
    pub screen_res: (usize, usize),

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
            language: UiLanguage::Turkish,
            font: FontChoice::TurkishPixel,
            volume: 75,
            is_muted: false,
            mouse_speed: 5,
            screen_res: (1280, 720),
            saved_window_states: [None; 11],
            dhcp_enabled: true,
            static_ip: [192, 168, 1, 100],
            vfs_mounted: true,
            kernel_debug_log: true,
            status_msg: "Live CD Persist Store: Hazir",
        }
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
        (win_x + win_w.saturating_sub(100), win_y + 84, 88, 20)
    }
    pub fn volume_track_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + 146, 200, 10)
    }
    pub fn mute_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 210, win_y + 142, 66, 18)
    }
    pub fn save_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + 172, 90, 22)
    }
    pub fn reset_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 100, win_y + 172, 95, 22)
    }

    // -----------------------------------------------------------------
    // Aksiyonlar
    // -----------------------------------------------------------------
    pub fn reset_defaults(&mut self) {
        let vol = self.volume;
        let theme_id = self.theme;
        *self = Self::default();
        self.volume = vol;
        self.theme = theme_id;
        self.status_msg = "Varsayilan Ayarlara Donuldu!";
    }

    pub fn cycle_theme(&mut self) {
        self.theme = theme::cycle();
        self.status_msg = "Tema degistirildi: Sarman temayi sever!";
    }

    pub fn set_volume_from_click(&mut self, x: usize) {
        let t = Self::volume_track_rect(0, 0);
        let ratio = ((x.saturating_sub(t.0)) * 100 / t.2).min(100);
        self.volume = ratio as u8;
        if self.is_muted && self.volume > 0 {
            self.is_muted = false;
        }
        self.status_msg = "Ses duzeyi guncellendi.";
    }

    pub fn save_all(&mut self, compositor: &MinGuiCompositor) {
        for (i, opt_win) in compositor.windows.iter().enumerate() {
            if let Some(ref win) = opt_win {
                self.saved_window_states[i] = Some(win.get_state());
            }
        }
        self.status_msg = "Pencere Duzeni ve Ayarlar Saklandi!";
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
    ) {
        // Sol Menü / Sekmeler
        fb.draw_rect(win_x + 6, win_y + 32, TAB_W + 8, win_h.saturating_sub(62), Color::TITLE_BAR_INACTIVE);
        let tabs = if self.language.is_english() { ["Basic", "Advanced", "Network", "About"] } else { ["Temel", "Ileri", "Ag", "Hakkinda"] };
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
                    "Degistir",
                    Color::DARK_BG,
                    false,
                );
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 128,
                    if self.language.is_english() { "Font: Turkish Pixel" } else { "Font: Turkce Piksel" }, Color::ACCENT_YELLOW);
                MinGuiCompositor::draw_button(fb, Self::font_btn_rect(win_x, win_y, win_w), Color::ACCENT_YELLOW,
                    if self.language.is_english() { "Change" } else { "Degistir" }, Color::DARK_BG, false);

                // --- Klavye / Dil ---
                let kbd_str = match kbd_layout {
                    KeyboardLayout::TurkishQ => if self.language.is_english() { "Keyboard: Turkish (TR-Q)" } else { "Klavye / Dil: Turkce (TR-Q)" },
                    KeyboardLayout::EnglishUS => "Keyboard: English (EN-US)",
                };
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 64, kbd_str, Color::ACCENT_PINK);
                MinGuiCompositor::draw_button(
                    fb,
                    Self::kbd_btn_rect(win_x, win_y, win_w),
                    Color::ACCENT_PINK,
                    if self.language.is_english() { "Change" } else { "Degistir" },
                    Color::DARK_BG,
                    false,
                );

                // --- Canlı Saat ---
                let mut clock = TextBuf::new();
                clock.push_str("Saat: ");
                clock.push_u2(now.hour as u32);
                clock.push_byte(b':');
                clock.push_u2(now.minute as u32);
                clock.push_byte(b':');
                clock.push_u2(now.second as u32);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 88, clock.as_str(), Color::ACCENT_GREEN);

                // --- Tarih ---
                let mut date = TextBuf::new();
                date.push_str("Tarih: ");
                date.push_u2(now.day as u32);
                date.push_byte(b' ');
                date.push_str(DateTime::month_name(now.month));
                date.push_byte(b' ');
                date.push_u4(now.year as u32);
                date.push_str(", ");
                date.push_str(DateTime::weekday_name(DateTime::weekday(now.year, now.month, now.day)));
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 108, date.as_str(), Color::TEXT_MAIN);

                // --- Ses ---
                let mut vol = TextBuf::new();
                vol.push_str("Ses Duzeyi: %");
                vol.push_u2(self.volume as u32);
                if self.is_muted {
                    vol.push_str(" (SUSTURULDU)");
                }
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 130, vol.as_str(), Color::TEXT_MAIN);

                let track = Self::volume_track_rect(win_x, win_y);
                fb.draw_rect(track.0, track.1, track.2, track.3, Color::TITLE_BAR_INACTIVE);
                let fill_w = (track.2 * self.volume as usize) / 100;
                let fill_col = if self.is_muted { Color::TEXT_MUTED } else { Color::ACCENT_GREEN };
                fb.draw_rect(track.0, track.1, fill_w, track.3, fill_col);
                fb.draw_rect(track.0 + fill_w.saturating_sub(4), track.1 - 4, 8, track.3 + 8, Color::TEXT_MAIN);

                let mute_r = Self::mute_btn_rect(win_x, win_y);
                let mute_bg = if self.is_muted { Color::ACCENT_RED } else { Color::TITLE_BAR };
                let mute_lbl = if self.is_muted { "Sesi Ac" } else { "Sustur" };
                MinGuiCompositor::draw_button(fb, mute_r, mute_bg, mute_lbl, Color::TEXT_MAIN, false);
            }
            1 => {
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, "Cekirdek Gunlugu: Aktif (/dev/klog)", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, "VFS Surucusu: RamFS TarFS v1.0", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 90, "Donanim Hizlandirma: SSE2 Aktif", Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 114, "Onyukleme: Limine Stivale2", Color::TEXT_MUTED);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 138, "RTC / CMOS Saat: Calisiyor", Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 162, "Tema Motoru: 3 hazir tema hazir", Color::TEXT_MAIN);
            }
            2 => {
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, "Ag Arayuzu: RTL8139 / e1000", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, "IP Adresi: 192.168.1.100 (DHCP OK)", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 90, "Alt Ag Maskesi: 255.255.255.0", Color::TEXT_MUTED);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 114, "Varsayilan Ag Gecidi: 192.168.1.1", Color::TEXT_MUTED);
            }
            _ => {
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 42, "MinOS - Sarman Edition (v1.1)", Color::ACCENT_BLUE);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 66, "Gelistirici: Sarman Cekirdek Ekibi", Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 90, "Ozel Surum: Live CD Persist Store", Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + 114, "github.com/tbagriyanik/minos", Color::ACCENT_PINK);
            }
        }

        // Alt Aksiyon Butonları (Kaydet & Sıfırla)
        MinGuiCompositor::draw_button(fb, Self::save_btn_rect(win_x, win_y), Color::ACCENT_GREEN, "Kaydet", Color::DARK_BG, false);
        MinGuiCompositor::draw_button(fb, Self::reset_btn_rect(win_x, win_y), Color::ACCENT_RED, "Sifirla", Color::WHITE, false);

        // Durum Çubuğu
        let bar_y = win_y + win_h.saturating_sub(26);
        fb.draw_rect(win_x + 6, bar_y, win_w.saturating_sub(12), 20, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, bar_y + 6, self.status_msg, Color::ACCENT_YELLOW);
    }
}
