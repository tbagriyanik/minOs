//! System tray layout, calendar and volume popup.

use super::*;

impl MinGuiCompositor {
    pub fn tray_layout(width: usize, height: usize) -> TrayLayout {
        let net_available = crate::drivers::e1000::is_ready();
        let audio_available = crate::drivers::hda::is_ready();
        Self::tray_layout_with_devices(width, height, net_available, audio_available)
    }

    pub fn tray_layout_with_devices(
        width: usize,
        height: usize,
        net_available: bool,
        audio_available: bool,
    ) -> TrayLayout {
        let tb_y = height.saturating_sub(taskbar_h());
        let by = tb_y + 5;
        // Tray düğmeleri aktif font ölçeğine göre büyür.
        let h = sp(26).min(taskbar_h().saturating_sub(8));
        // "Masaüstünü Göster" şeridi en sağda (Windows benzeri ince dikey alan).
        let show_desktop_w = sp(10);
        let show_desktop_x = width.saturating_sub(show_desktop_w);

        // Genişlikler, çizilen ikon + metnin gerçek piksel genişliğine göre seçilir.
        //   saat:  "HH:MM:SS" = 8 hücre -> 92px
        //   ses:   ikon ~26px -> 44px
        //   ağ:    ikon 24px + "eth0" 40px -> 68px
        //   klavye:ikon 30px + "TR"/"EN" 20px -> 50px
        let clock_w = sp(92);
        let vol_w = sp(44);
        let net_w = sp(68);
        let kbd_w = sp(50);
        let gap = sp(8);

        // Küçük ekran / yüksek ölçekte çakışmayı önlemek için sol sınır:
        // Başlat düğmesi sağ kenarı + boşluk. Bu sınırın altına düşen düşük
        // öncelikli tepsi öğeleri (klavye -> ağ -> ses) gizlenir.
        let left_limit = sp(6) + sp(112) + gap;

        let clock_x = show_desktop_x.saturating_sub(gap + clock_w);
        let mut curr_x = clock_x;

        let volume_rect = if audio_available {
            let x = curr_x.saturating_sub(gap + vol_w);
            if x >= left_limit {
                curr_x = x;
                (x, by, vol_w, h)
            } else {
                (0, 0, 0, 0)
            }
        } else {
            (0, 0, 0, 0)
        };

        let network_rect = if net_available {
            let x = curr_x.saturating_sub(gap + net_w);
            if x >= left_limit {
                curr_x = x;
                (x, by, net_w, h)
            } else {
                (0, 0, 0, 0)
            }
        } else {
            (0, 0, 0, 0)
        };

        let kbd_x = curr_x.saturating_sub(gap + kbd_w);
        let kbd_rect = if kbd_x >= left_limit {
            (kbd_x, by, kbd_w, h)
        } else {
            (0, 0, 0, 0)
        };

        let desktop_rect = (show_desktop_x, tb_y, show_desktop_w, taskbar_h());

        TrayLayout {
            tb_y,
            start: (6, by, sp(112), h),
            clock: (clock_x, by, clock_w, h),
            minimize_all: desktop_rect,
            volume: volume_rect,
            network: network_rect,
            kbd: kbd_rect,
            desktop: desktop_rect,
        }
    }

    /// Tepside görünen en sol öğenin X konumu (gizli öğeler yok sayılır).
    /// Görev çubuğu sekmeleri bu sınıra kadar uzayabilir.
    pub fn tray_left_bound(width: usize, height: usize) -> usize {
        let l = Self::tray_layout(width, height);
        let mut right = l.clock.0;
        for r in [l.volume, l.network, l.kbd] {
            if r.2 > 0 {
                right = right.min(r.0);
            }
        }
        right
    }

    // -----------------------------------------------------------------------
    // 4. Takvim Kaplaması (Clock -> Calendar)
    // -----------------------------------------------------------------------
    pub fn calendar_rect(width: usize, layout: &TrayLayout) -> (usize, usize, usize, usize) {
        let w = 252;
        let h = 238;
        let x = (layout.clock.0 + layout.clock.2)
            .saturating_sub(w)
            .min(width.saturating_sub(w));
        let y = layout.tb_y.saturating_sub(h + 6);
        (x, y, w, h)
    }

    pub fn render_calendar(&self, fb: &mut Framebuffer, now: &DateTime, layout: &TrayLayout) {
        let (x, y, w, h) = Self::calendar_rect(fb.width, layout);

        fb.draw_shadow(x, y, w, h, Color::SHADOW);
        fb.draw_rect(x, y, w, h, Color::TASKBAR_BG);
        fb.draw_rect_outline(x, y, w, h, Color::BORDER);

        // Başlık: < Ay Yıl >
        fb.draw_vgradient(
            x + 1,
            y + 1,
            w - 2,
            30,
            Color::TITLE_BAR,
            Self::shade(Color::TITLE_BAR, -14),
        );
        Self::draw_string(fb, x + 10, y + 11, "<", Color::ACCENT_BLUE);
        let mut title = TextBuf::new();
        title.push_str(if ui_english() {
            DateTime::month_name_en(self.cal_month)
        } else {
            DateTime::month_name(self.cal_month)
        });
        title.push_byte(b' ');
        title.push_u4(self.cal_year as u32);
        let tw = Self::text_width(title.as_str(), 1);
        Self::draw_string(
            fb,
            x + (w - tw) / 2,
            y + 11,
            title.as_str(),
            Color::TEXT_MAIN,
        );
        Self::draw_string(fb, x + w - 22, y + 11, ">", Color::ACCENT_BLUE);

        // Gün başlıkları (Pt..Pz)
        let grid_x = x + 10;
        let grid_y = y + 40;
        let cell_w = (w - 20) / 7;
        for d in 0..7usize {
            let label = if ui_english() {
                DateTime::weekday_short_en(d as u8)
            } else {
                DateTime::weekday_short(d as u8)
            };
            let tw = Self::text_width(label, 1);
            Self::draw_string(
                fb,
                grid_x + d * cell_w + cell_w.saturating_sub(tw) / 2,
                grid_y,
                label,
                Color::TEXT_MUTED,
            );
        }

        // Ayın günleri
        let first_w = DateTime::weekday(self.cal_year, self.cal_month, 1) as usize;
        let dim = DateTime::days_in_month(self.cal_year, self.cal_month) as usize;
        let today_month = self.cal_year == now.year && self.cal_month == now.month;
        let row_h = 26;
        for day in 1..=dim {
            let idx = first_w + (day - 1);
            let row = idx / 7;
            let col = idx % 7;
            let cx = grid_x + col * cell_w;
            let cy = grid_y + 16 + row * row_h;
            if cy + 20 > y + h - 8 {
                break;
            }
            let is_today = today_month && day == now.day as usize;
            if is_today {
                fb.draw_rect(cx, cy, cell_w - 2, 20, Color::ACCENT_BLUE);
            }
            let mut db = TextBuf::new();
            db.push_u2(day as u32);
            let col_txt = if is_today {
                Self::text_on(Color::ACCENT_BLUE)
            } else {
                Color::TEXT_MAIN
            };
            let tw = Self::text_width(db.as_str(), 1);
            Self::draw_string(
                fb,
                cx + cell_w.saturating_sub(tw) / 2,
                cy + 6,
                db.as_str(),
                col_txt,
            );
        }

        // Alt bilgi: tam tarih ve saat
        fb.draw_rect(x + 1, y + h - 26, w - 2, 25, Color::TITLE_BAR_INACTIVE);
        let mut foot = TextBuf::new();
        foot.push_u2(now.hour as u32);
        foot.push_byte(b':');
        foot.push_u2(now.minute as u32);
        foot.push_byte(b':');
        foot.push_u2(now.second as u32);
        foot.push_str("  ");
        let weekday = DateTime::weekday(now.year, now.month, now.day);
        foot.push_str(if ui_english() {
            DateTime::weekday_name_en(weekday)
        } else {
            DateTime::weekday_name(weekday)
        });
        Self::draw_string(fb, x + 12, y + h - 20, foot.as_str(), Color::ACCENT_GREEN);
    }

    // -----------------------------------------------------------------------
    // 5. Ses Paneli (Volume Popup)
    // -----------------------------------------------------------------------
    pub fn volume_slider_rect(&self, layout: &TrayLayout) -> (usize, usize, usize, usize) {
        let w = 220;
        let x = layout.volume.0.saturating_sub(w).saturating_sub(10).max(10);
        let y = layout.tb_y.saturating_sub(96);
        (x, y, w, 90)
    }

    pub fn volume_track_rect(&self, layout: &TrayLayout) -> (usize, usize, usize, usize) {
        let panel = self.volume_slider_rect(layout);
        (panel.0 + 16, panel.1 + 46, panel.2 - 32, 12)
    }

    pub fn volume_mute_rect(&self, layout: &TrayLayout) -> (usize, usize, usize, usize) {
        let panel = self.volume_slider_rect(layout);
        (panel.0 + panel.2 - 78, panel.1 + 66, 62, 20)
    }

    pub fn render_volume_popup(
        &self,
        fb: &mut Framebuffer,
        layout: &TrayLayout,
        volume: u8,
        is_muted: bool,
    ) {
        let panel = self.volume_slider_rect(layout);
        let (x, y, w, h) = panel;

        fb.draw_shadow(x, y, w, h, Color::SHADOW);
        fb.draw_rect(x, y, w, h, Color::TASKBAR_BG);
        fb.draw_rect_outline(x, y, w, h, Color::BORDER);
        fb.draw_vgradient(
            x + 1,
            y + 1,
            w - 2,
            26,
            Color::TITLE_BAR,
            Self::shade(Color::TITLE_BAR, -14),
        );
        Self::draw_string_shadow(
            fb,
            x + 12,
            y + 10,
            ui_text("Ses Düzeyi", "Volume"),
            Color::TEXT_MAIN,
            Color::SHADOW,
            1,
        );

        // Yüzde
        let mut pct = TextBuf::new();
        pct.push_byte(b'%');
        pct.push_u2(volume as u32);
        let pc = if is_muted {
            Color::TEXT_MUTED
        } else {
            Color::ACCENT_GREEN
        };
        Self::draw_string(fb, x + w - 42, y + 10, pct.as_str(), pc);

        // Kaydırıcı yolu
        let track = self.volume_track_rect(layout);
        fb.draw_rect(
            track.0,
            track.1,
            track.2,
            track.3,
            Color::TITLE_BAR_INACTIVE,
        );
        let fill_w = (track.2 * volume as usize) / 100;
        if !is_muted {
            fb.draw_rect(track.0, track.1, fill_w, track.3, Color::ACCENT_BLUE);
        }
        // Tutamaç
        let knob_x = track.0 + fill_w.saturating_sub(4);
        fb.draw_rect(knob_x, track.1 - 4, 8, track.3 + 8, Color::TEXT_MAIN);

        // Sessiz düğmesi
        let mute = self.volume_mute_rect(layout);
        let bg = if is_muted {
            Color::ACCENT_RED
        } else {
            Color::TITLE_BAR
        };
        let label = match (is_muted, ui_english()) {
            (true, true) => "Unmute",
            (false, true) => "Mute",
            (true, false) => "Sesi Aç",
            (false, false) => "Sustur",
        };
        Self::draw_button(fb, mute, bg, label, Color::TEXT_MAIN, false);
    }
}
