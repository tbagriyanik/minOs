//! Taskbar tab layout and taskbar rendering.

use super::*;

impl MinGuiCompositor {
    /// Görev çubuğu ilk uygulama sekmesinin X konumu (font ölçeğine duyarlı).
    pub fn taskbar_tab_start() -> usize { sp(118) }
    /// Görev çubuğu sekmesinin genişliği (font ölçeğine duyarlı).
    pub fn taskbar_tab_width() -> usize { sp(84) }
    /// Ardışık sekmeler arası adım (font ölçeğine duyarlı).
    pub fn taskbar_tab_step() -> usize { sp(88) }
    /// Görev çubuğunda kaç pencere sekmesinin sığacağını ve taşma sayacını hesaplar.
    /// Dönen değerler: başlangıç X, sekme genişliği, adım, normal sekme sayısı, toplam yuva.
    pub fn taskbar_tabs_layout(width: usize, height: usize, open_count: usize) -> (usize, usize, usize, usize, usize) {
        let start = Self::taskbar_tab_start();
        let right = Self::tray_layout(width, height).kbd.0.saturating_sub(sp(4));
        let available = right.saturating_sub(start);
        let min_slot = sp(30).max(1);
        let capacity = (available / min_slot).max(1);
        let slots = open_count.min(capacity);
        if slots == 0 { return (start, Self::taskbar_tab_width(), Self::taskbar_tab_step(), 0, 0); }
        let overflow = open_count > capacity;
        let step = (available / slots).max(1).min(Self::taskbar_tab_step());
        let tab_width = Self::taskbar_tab_width().min(step.saturating_sub(sp(4)).max(1));
        let shown = if overflow { slots.saturating_sub(1) } else { slots };
        (start, tab_width, step, shown, slots)
    }
    /// Verilen X konumundaki görev çubuğu sekmesinin dikdörtgeni.
    pub fn taskbar_tab_rect_at(tab_x: usize, tb_y: usize) -> (usize, usize, usize, usize) {
        let h = sp(26).min(taskbar_h().saturating_sub(8));
        (tab_x, tb_y + 5, Self::taskbar_tab_width(), h)
    }
    // -----------------------------------------------------------------------
    // 3. Görev Çubuğu, Sistem Tepsisi (saat/ses/tema) ve Kaplamalar
    // -----------------------------------------------------------------------
    pub fn render_taskbar_and_overlays(
        &self,
        fb: &mut Framebuffer,
        mouse: &MouseState,
        kbd: &KeyboardState,
        now: &DateTime,
        volume: u8,
        is_muted: bool,
        english: bool,
        ethernet_available: bool,
    ) {
        let l = Self::tray_layout(fb.width, fb.height);
        let tb_y = l.tb_y;
        let mx = mouse.x as usize;
        let my = mouse.y as usize;

        // Görev Çubuğu Zemin (gradyan)
        fb.draw_vgradient(0, tb_y, fb.width, taskbar_h(), Color::TASKBAR_BG, Self::shade(Color::TASKBAR_BG, 8));
        fb.draw_rect(0, tb_y, fb.width, 1, Color::BORDER);

        // Başlat Menüsü Butonu
        let start_hover = in_rect(mx, my, l.start) || self.is_start_menu_open;
        let start_bg = if self.is_start_menu_open { Color::ACCENT_BLUE } else { Color::ACCENT_BLUE };
        Self::draw_button(fb, l.start, start_bg, "", Color::WHITE, start_hover);
        // Kedi ikonu + "minOs" metni buton içinde yatay olarak ortalanır;
        // metin buton kenarlarına yapışmaz veya taşmaz.
        {
            let label = "minOs";
            let text_w = Self::text_width(label, 1);
            let icon_w = Self::icon_size();
            let inner_gap = sp(8);
            let block_w = icon_w + inner_gap + text_w;
            let block_x = l.start.0 + l.start.2.saturating_sub(block_w) / 2;
            let icon_y = tb_y + taskbar_h().saturating_sub(icon_w) / 2;
            let text_h = Self::text_height();
            let text_y = tb_y + taskbar_h().saturating_sub(text_h) / 2;
            Self::draw_cat_icon(fb, block_x, icon_y);
            Self::draw_string_shadow(fb, block_x + icon_w + inner_gap, text_y, label, Self::text_on(start_bg), Color::SHADOW, 1);
        }

        // Uygulama Sekmeleri (Yuvarlatılmış 5px) — font ölçeğine göre büyür.
        let open_count = self.windows.iter().filter(|slot| slot.map_or(false, |win| win.is_open)).count();
        let (tab_start, tab_width, tab_step, shown_tabs, tab_slots) = Self::taskbar_tabs_layout(fb.width, fb.height, open_count);
        let mut tab_slot = 0usize;
        for opt_win in self.windows.iter() {
            if let Some(win) = opt_win {
                if win.is_open {
                    if tab_slot >= shown_tabs { break; }
                    let tab_x = tab_start + tab_slot * tab_step;
                    let (_, _, tw, th) = MinGuiCompositor::taskbar_tab_rect_at(tab_x, tb_y);
                    let tw = tw.min(tab_width);
                    let rect = (tab_x, tb_y + 5, tw, th);
                    let active = win.is_focused && !win.is_minimized;
                    let base = if active { Color::TITLE_BAR } else { Color::TITLE_BAR_INACTIVE };
                    let hovered = in_rect(mx, my, rect);
                    let bg = if hovered { Self::shade(base, 20) } else { base };
                    fb.draw_rounded_rect(rect.0, rect.1, rect.2, rect.3, 5, bg);
                    fb.draw_rounded_rect_outline(rect.0, rect.1, rect.2, rect.3, 5, Color::BORDER);
                    if active {
                        fb.draw_rounded_rect(rect.0 + 2, rect.1 + 2, 3, rect.3 - 4, 1, Color::ACCENT_BLUE);
                    }
                    let tab_text_color = if win.is_minimized { Color::TEXT_MUTED } else { Color::TEXT_MAIN };
                    let loc_tab = Self::window_tab_title(win.app_id as usize, ui_english());
                    let tab_title = if !loc_tab.is_empty() { loc_tab } else { win.tab_title };
                    let text_h = Self::text_height();
                    let text_y = rect.1 + rect.3.saturating_sub(text_h) / 2;
                    if tw >= sp(58) {
                        Self::draw_icon_by_index(fb, tab_x + 5, text_y, Self::window_icon_index(win.app_id as usize));
                        Self::draw_string_clipped(fb, tab_x + sp(30), text_y, tab_title, tab_text_color,
                            (tab_x + sp(28), rect.1, rect.2.saturating_sub(sp(32)), rect.3));
                    } else {
                        Self::draw_icon_by_index(fb, tab_x + tw.saturating_sub(Self::icon_size()) / 2, text_y,
                            Self::window_icon_index(win.app_id as usize));
                    }
                    tab_slot += 1;
                }
            }
        }
        if tab_slots > shown_tabs {
            let badge_x = tab_start + shown_tabs * tab_step;
            let (_, _, _, tab_h) = MinGuiCompositor::taskbar_tab_rect_at(badge_x, tb_y);
            let badge_w = tab_width.min(l.kbd.0.saturating_sub(sp(4)).saturating_sub(badge_x));
            if badge_w > 0 {
                let badge = (badge_x, tb_y + 5, badge_w, tab_h);
                fb.draw_rounded_rect(badge.0, badge.1, badge.2, badge.3, 5, Color::ACCENT_BLUE);
                let mut hidden = TextBuf::new();
                hidden.push_byte(b'+');
                hidden.push_usize(open_count.saturating_sub(shown_tabs));
                let text_w = Self::text_width(hidden.as_str(), 1);
                let text_h = Self::text_height();
                Self::draw_string_clipped(fb, badge.0 + badge.2.saturating_sub(text_w) / 2,
                    badge.1 + badge.3.saturating_sub(text_h) / 2, hidden.as_str(), Self::text_on(Color::ACCENT_BLUE), badge);
            }
        }

        // Sistem Tepsisi: saat, ses, Ethernet, tema ve klavye düzeni.
        // 1. Klavye / Dil Düğmesi (l.kbd)
        let kbd_tag = kbd.layout_tag();
        let kbd_col = match kbd.layout {
            crate::drivers::keyboard::KeyboardLayout::TurkishQ => Color::ACCENT_RED,
            crate::drivers::keyboard::KeyboardLayout::EnglishUS => Color::ACCENT_BLUE,
        };
        let kbd_hover = in_rect(mx, my, l.kbd);
        // Klavye resmi: kısa etiket yerine sade bir klavye simgesi.
        let keyboard_icon = if kbd_hover { Color::TEXT_MAIN } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(l.kbd.0 + sp(6), l.kbd.1 + sp(6), sp(18), sp(14), 2, keyboard_icon);
        for row in 0..2 {
            for col in 0..5 {
                fb.draw_rect(l.kbd.0 + sp(9) + col * sp(3), l.kbd.1 + sp(9) + row * sp(3), sp(2), sp(1), Color::TASKBAR_BG);
            }
        }
        fb.draw_rect(l.kbd.0 + sp(11), l.kbd.1 + sp(17), sp(8), sp(1), Color::TASKBAR_BG);
        Self::draw_string(fb, l.kbd.0 + sp(30), tb_y + (taskbar_h().saturating_sub(Self::text_height())) / 2, kbd_tag, kbd_col);

        // 2. Tema Düğmesi (l.theme)
        let theme_hover = in_rect(mx, my, l.theme);
        fb.draw_rounded_rect(l.theme.0 + sp(6), l.theme.1 + sp(8), sp(10), sp(10), 3, Color::ACCENT_PINK);
        let t_name = theme::current().short_name();
        Self::draw_string(fb, l.theme.0 + sp(22), tb_y + (taskbar_h().saturating_sub(Self::text_height())) / 2, t_name, if theme_hover { Color::ACCENT_PINK } else { Color::TEXT_MAIN });

        // 3. Ses Düğmesi (l.volume) - Yalnızca ses aygıtı varsa çiz
        if l.volume.2 > 0 {
            let vol_hover = in_rect(mx, my, l.volume) || self.is_volume_popup_open;
            let icon_color = if is_muted { Color::ACCENT_RED } else if vol_hover { Color::TEXT_MAIN } else { Color::ACCENT_GREEN };
            // Hoparlör gövdesi ve ses dalgaları.
            fb.draw_rect(l.volume.0 + sp(8), l.volume.1 + sp(11), sp(4), sp(6), icon_color);
            fb.draw_rect(l.volume.0 + sp(12), l.volume.1 + sp(8), sp(3), sp(12), icon_color);
            if !is_muted && volume > 0 {
                fb.draw_rect(l.volume.0 + sp(17), l.volume.1 + sp(10), sp(1), sp(8), icon_color);
                if volume > 35 { fb.draw_rect(l.volume.0 + sp(20), l.volume.1 + sp(8), sp(1), sp(12), icon_color); }
                if volume > 70 { fb.draw_rect(l.volume.0 + sp(23), l.volume.1 + sp(6), sp(1), sp(16), icon_color); }
            } else {
                fb.draw_rect(l.volume.0 + sp(17), l.volume.1 + sp(9), sp(2), sp(2), icon_color);
                fb.draw_rect(l.volume.0 + sp(20), l.volume.1 + sp(16), sp(2), sp(2), icon_color);
            }
        }
        // Ethernet simgesi yalnızca e1000 aygıtı başarıyla başlatıldıysa görünür.
        if ethernet_available && l.network.2 > 0 {
            let net_hover = in_rect(mx, my, l.network);
            let linked = crate::drivers::e1000::link_up();
            let c = if linked { if net_hover { Color::TEXT_MAIN } else { Color::ACCENT_GREEN } } else { Color::TEXT_MUTED };
            // Ethernet jakı: gövde, pinler, kablo ve bağlantı durumu ışığı.
            fb.draw_rounded_rect_outline(l.network.0 + sp(3), l.network.1 + sp(6), sp(14), sp(14), 2, c);
            fb.draw_rect(l.network.0 + sp(6), l.network.1 + sp(12), sp(8), sp(6), c);
            fb.draw_rect(l.network.0 + sp(7), l.network.1 + sp(9), sp(2), sp(4), c);
            fb.draw_rect(l.network.0 + sp(11), l.network.1 + sp(9), sp(2), sp(4), c);
            fb.draw_rect(l.network.0 + sp(9), l.network.1 + sp(19), sp(2), sp(2), c);
            fb.draw_rect(l.network.0 + sp(20), l.network.1 + sp(7), sp(3), sp(3), c);
            Self::draw_string(fb, l.network.0 + sp(28), tb_y + (taskbar_h().saturating_sub(Self::text_height())) / 2, "eth0", c);
        }

        // 4. Saat Düğmesi (l.clock)
        let clock_hover = in_rect(mx, my, l.clock) || self.is_calendar_open;
        let mut clock = TextBuf::new();
        clock.push_u2(now.hour as u32);
        clock.push_byte(b':');
        clock.push_u2(now.minute as u32);
        clock.push_byte(b':');
        clock.push_u2(now.second as u32);
        Self::draw_string(fb, l.clock.0 + sp(8), tb_y + (taskbar_h().saturating_sub(Self::text_height())) / 2, clock.as_str(), if clock_hover { Color::ACCENT_BLUE } else { Color::TEXT_MAIN });

        // Akıllı tepsi ağı simgesi

        // Başlat Menüsü
        if self.is_start_menu_open {
            self.render_start_menu(fb, tb_y, english, mx, my);
        }
        // Bağlam Menüsü
        if self.is_context_menu_open {
            self.render_context_menu(fb);
        }
        // Alt+Tab
        if self.is_alt_tab_open {
            self.render_alt_tab_overlay(fb);
        }
        // Takvim
        if self.is_calendar_open {
            self.render_calendar(fb, now, &l);
        }
        // Ses paneli
        if self.is_volume_popup_open {
            self.render_volume_popup(fb, &l, volume, is_muted);
        }

        // Görev Çubuğu Üzerinde Toast Bildirimleri (Hafif Gölgeli Modern Kartlar)
        self.render_toasts(fb, tb_y, mx, my);

        // Donanım Fare İmleci
        self.draw_mouse_cursor(fb, mx, my);
    }
}
