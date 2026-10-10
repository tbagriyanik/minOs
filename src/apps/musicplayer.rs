use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct MusicPlayerApp {
    pub is_playing: bool,
    pub volume: usize,
    pub current_track: usize,
}

impl MusicPlayerApp {
    pub const fn new() -> Self {
        Self {
            is_playing: true,
            volume: 85,
            current_track: 0,
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, _win_h: usize, tick: usize) {
        // Üst Şarkı Bilgi Alanı
        fb.draw_rect(win_x + 6, win_y + 28, win_w - 12, 42, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 34, "🎵 Lofi Cat Beats - Ring 0 Chill Hop", Color::ACCENT_PINK);
        MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 50, "DJ Purr & The Microkernel | 44.1 kHz 16-bit FLAC", Color::TEXT_MUTED);

        // Dinamik Animasyonlu Ekolayzer / Spektrum Çubukları
        let eq_x = win_x + 10;
        let eq_y = win_y + 74;
        let eq_w = 230;
        let eq_h = 75;
        fb.draw_rect(eq_x, eq_y, eq_w, eq_h, 0x11111B);

        let num_bars = 18;
        let bar_width = 9;
        let spacing = 3;

        for i in 0..num_bars {
            let bx = eq_x + 10 + i * (bar_width + spacing);
            // Dinamik yükseklik hesaplama (Canlı müzik hissi)
            let wave = if self.is_playing {
                let phase = (tick * 4 + i * 17) % 60;
                let h = if phase > 30 { 60 - phase } else { phase };
                h + (i % 5) * 2 + 6
            } else {
                4
            };

            let bar_h = wave.min(eq_h - 10);
            let by = eq_y + eq_h - bar_h - 4;

            // Dikey renk geçişi (Yeşil -> Sarı -> Pembe/Kırmızı)
            for y_offset in 0..bar_h {
                let py = by + y_offset;
                let c = if y_offset < 8 {
                    Color::ACCENT_RED // Tepe noktası
                } else if y_offset < 24 {
                    Color::ACCENT_YELLOW // Orta bölge
                } else {
                    Color::ACCENT_GREEN // Alt bölge
                };
                for px in 0..bar_width {
                    fb.put_pixel(bx + px, py, c);
                }
            }

            // Tepe Tepe Noktası (Peak hold nokta ışığı)
            if self.is_playing && bar_h > 10 {
                fb.draw_rect(bx, by.saturating_sub(4), bar_width, 2, Color::WHITE);
            }
        }

        // Sağ Çalma Listesi (Playlist)
        let pl_x = win_x + eq_w + 16;
        let pl_y = win_y + 74;
        let pl_w = win_w - eq_w - 24;
        fb.draw_rect(pl_x, pl_y, pl_w, eq_h + 38, Color::TITLE_BAR_INACTIVE);
        MinGuiCompositor::draw_string(fb, pl_x + 8, pl_y + 6, "ÇALMA LİSTESİ", Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, pl_x + 8, pl_y + 24, "▶ 1. Ring0 Beats", Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, pl_x + 8, pl_y + 42, "  2. IRQ Lullaby", Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, pl_x + 8, pl_y + 60, "  3. Cyber Meow", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, pl_x + 8, pl_y + 78, "  4. Paging Groove", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, pl_x + 8, pl_y + 96, "  5. Limine Chill", Color::TEXT_MUTED);

        // İlerleme Çubuğu (Timeline Scrubber)
        let prog_y = win_y + 158;
        MinGuiCompositor::draw_string(fb, eq_x, prog_y, "01:42", Color::TEXT_MAIN);
        let bar_bg_x = eq_x + 44;
        let bar_bg_w = eq_w - 94;
        fb.draw_rect(bar_bg_x, prog_y + 2, bar_bg_w, 6, Color::DARK_BG);
        fb.draw_rect(bar_bg_x, prog_y + 2, (bar_bg_w * 48) / 100, 6, Color::ACCENT_BLUE);
        fb.draw_rect(bar_bg_x + (bar_bg_w * 48) / 100 - 2, prog_y, 5, 10, Color::WHITE);
        MinGuiCompositor::draw_string(fb, bar_bg_x + bar_bg_w + 8, prog_y, "03:30", Color::TEXT_MUTED);

        // Kontrol Düğmeleri
        let ctrl_y = win_y + 180;
        fb.draw_rect(win_x + 10, ctrl_y, 36, 24, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 16, ctrl_y + 8, "|<<", Color::TEXT_MAIN);

        fb.draw_rect(win_x + 52, ctrl_y, 46, 24, Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, win_x + 58, ctrl_y + 8, "|| >", Color::DARK_BG);

        fb.draw_rect(win_x + 104, ctrl_y, 36, 24, Color::WINDOW_BG);
        MinGuiCompositor::draw_string(fb, win_x + 110, ctrl_y + 8, ">>|", Color::TEXT_MAIN);

        // Ses Düzeyi Göstergesi
        MinGuiCompositor::draw_string(fb, win_x + 150, ctrl_y + 8, "Ses: %85", Color::TEXT_MAIN);

        // Alt Durum Çubuğu
        fb.draw_rect(win_x + 6, win_y + 218, win_w - 12, 18, Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 222, "AC97 / HD Audio | 320 kbps Çalma Modu | CatBeats v1", Color::TEXT_MUTED);
    }
}
