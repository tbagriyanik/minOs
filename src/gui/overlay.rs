//! Overlays: Alt+Tab switcher and mouse cursor.

use super::*;

impl MinGuiCompositor {
    pub fn render_alt_tab_overlay(&self, fb: &mut Framebuffer) {
        let box_w = 470.min(fb.width.saturating_sub(24)).max(1);
        let open_count = if self.is_alt_tab_open && self.alt_tab_count > 0 { self.alt_tab_count }
            else { self.windows.iter().filter(|slot| slot.map_or(false, |win| win.is_open && !win.is_minimized)).count() };
        let columns = (box_w.saturating_sub(24) / 58).max(1);
        let rows = open_count.saturating_add(columns - 1) / columns;
        let grid_h = rows * 66;
        let box_h = (40 + grid_h + 58).min(fb.height.saturating_sub(24)).max(1);
        let box_x = fb.width.saturating_sub(box_w) / 2;
        let box_y = fb.height.saturating_sub(box_h) / 2;

        fb.draw_shadow(box_x, box_y, box_w, box_h, Color::SHADOW);
        fb.draw_rect(box_x, box_y, box_w, box_h, Color::TASKBAR_BG);
        fb.draw_rect_outline(box_x, box_y, box_w, box_h, Color::BORDER);
        fb.draw_rect(box_x, box_y, box_w, 26, Color::TITLE_BAR);
        let english = ui_english();
        Self::draw_string_shadow(fb, box_x + 12, box_y + 9,
            ui_text("Uygulama Geçişi (Alt + Tab)", "Window Switcher (Alt + Tab)"), Color::TEXT_MAIN, Color::SHADOW, 1);

        let mut active_app_title = ui_text("Pencere Seçilmedi", "No Window Selected");

        let (draw_order, draw_count) = self.draw_order();
        let (switch_order, switch_count) = if self.is_alt_tab_open && self.alt_tab_count > 0 {
            (self.alt_tab_order, self.alt_tab_count)
        } else {
            (draw_order, draw_count)
        };
        for app_count in 0..switch_count {
            if let Some(win) = self.windows[switch_order[app_count]] {
                let col = app_count % columns;
                let row = app_count / columns;
                let grid_w = columns * 58;
                let card_x = box_x + box_w.saturating_sub(grid_w) / 2 + col * 58;
                let card_y = box_y + 36 + row * 66;
                let is_sel = win.is_focused;
                let card_bg = if is_sel { Color::ACCENT_BLUE } else { Color::WINDOW_BG };

                if is_sel {
                    active_app_title = if english { Self::window_title(win.app_id as usize, true) } else { win.title };
                }

                fb.draw_rounded_rect(card_x, card_y, 50, 58, 6, card_bg);
                if is_sel {
                    fb.draw_rounded_rect_outline(card_x.saturating_sub(1), card_y.saturating_sub(1), 52, 60, 6, Color::ACCENT_PINK);
                }
                let icon_idx = Self::window_icon_index(win.app_id as usize);
                Self::draw_icon_by_index(fb, card_x + 15, card_y + 9, icon_idx);
                let tab_col = if is_sel { Self::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN };
                let tab_title = if english { Self::window_tab_title(win.app_id as usize, true) } else { win.tab_title };
                Self::draw_string_clipped(fb, card_x + 2, card_y + 38, tab_title, tab_col, (card_x + 2, card_y + 36, 46, 20));
            }
        }

        let footer_y = box_y + box_h.saturating_sub(52);
        fb.draw_rect(box_x + 12, footer_y, box_w.saturating_sub(24), 44, Color::TITLE_BAR_INACTIVE);
        let active_label = ui_text("Aktif Pencere:", "Active Window:");
        let active_label_w = Self::text_width(active_label, 1) + 8;
        Self::draw_string_clipped(fb, box_x + 20, footer_y + 5, active_label, Color::TEXT_MUTED,
            (box_x + 18, footer_y + 2, box_w.saturating_sub(36), 20));
        Self::draw_string_clipped(fb, box_x + 20 + active_label_w, footer_y + 5, active_app_title, Color::ACCENT_GREEN,
            (box_x + 20 + active_label_w, footer_y + 2, box_w.saturating_sub(active_label_w + 38), 20));
        Self::draw_string_clipped(fb, box_x + 20, footer_y + 24,
            ui_text("Alt tuşunu bıraktığınızda pencere öne gelir.", "Release Alt to bring the window forward."), Color::TEXT_MUTED,
            (box_x + 18, footer_y + 21, box_w.saturating_sub(36), 20));
    }
    pub fn draw_mouse_cursor(&self, fb: &mut Framebuffer, mx: usize, my: usize, pressed: bool) {
        fb.save_cursor_background(mx, my);
        // Not: BGA/VGA grafik modda HW sprite yoktur; yazılım imleci her
        // zaman çizilir. HW bayrağı yalnızca CRTC konum takibi içindir.
        // Fare bir pencere kenarı üzerindeyse yönlü yeniden boyutlandırma imleci çiz.
        let over_control = self.windows.iter().flatten().any(|win| {
            win.is_open && !win.is_minimized && in_rect(mx, my, win.close_rect())
        });
        if !over_control {
        if let Some((_, edge)) = self.topmost_resize_edge(mx, my) {
            self.draw_resize_cursor(fb, mx, my, edge);
            return;
        }
        }

        // 16x10 piksel imleç deseni.
        // 1080p ve yüksek çözünürlükler için net, keskin ve modern 19-piksel imleç deseni (16-bit genişlik)
        // İdeal orta büyüklükte (15 piksel yüksekliğinde, zarif ve net) modern ok imleci
        const CURSOR_PATTERN: [u16; 15] = [
            0b1000_0000_0000_0000,
            0b1100_0000_0000_0000,
            0b1110_0000_0000_0000,
            0b1111_0000_0000_0000,
            0b1111_1000_0000_0000,
            0b1111_1100_0000_0000,
            0b1111_1110_0000_0000,
            0b1111_1111_0000_0000,
            0b1111_1000_0000_0000,
            0b1101_1000_0000_0000,
            0b1000_1100_0000_0000,
            0b0000_1100_0000_0000,
            0b0000_0110_0000_0000,
            0b0000_0110_0000_0000,
            0b0000_0010_0000_0000,
        ];
        let (cursor_fill, cursor_outline) = if pressed {
            (Color::CURSOR_OUTLINE, Color::CURSOR_FILL)
        } else {
            (Color::CURSOR_FILL, Color::CURSOR_OUTLINE)
        };

        // 8-yönlü tam çevreleyen kalın dış hat (outline) ile koyu ve açık her zeminde kusursuz netlik:
        for (row, &word) in CURSOR_PATTERN.iter().enumerate() {
            for col in 0usize..16 {
                if (word & (1 << (15 - col))) != 0 {
                    // Çevre pikseller (3x3 kutu konturu)
                    fb.put_pixel(mx + col.saturating_sub(1), my + row, cursor_outline);
                    fb.put_pixel(mx + col + 1, my + row, cursor_outline);
                    fb.put_pixel(mx + col, my + row.saturating_sub(1), cursor_outline);
                    fb.put_pixel(mx + col, my + row + 1, cursor_outline);
                    fb.put_pixel(mx + col + 1, my + row + 1, cursor_outline);
                    fb.put_pixel(mx + col.saturating_sub(1), my + row + 1, cursor_outline);
                    fb.put_pixel(mx + col + 1, my + row.saturating_sub(1), cursor_outline);
                    fb.put_pixel(mx + col.saturating_sub(1), my + row.saturating_sub(1), cursor_outline);
                }
            }
        }
        // İç gövde dolgusu
        for (row, &word) in CURSOR_PATTERN.iter().enumerate() {
            for col in 0usize..16 {
                if (word & (1 << (15 - col))) != 0 {
                    fb.put_pixel(mx + col, my + row, cursor_fill);
                }
            }
        }
    }

    /// Yönlü yeniden boyutlandırma imleci: kenarlar için çift ok, köşeler için çapraz.
    fn draw_resize_cursor(&self, fb: &mut Framebuffer, mx: usize, my: usize, edge: ResizeEdge) {
        use ResizeEdge::*;
        // Her yön için 7x7 ikon deseni (1 = dolgu).
        const H_ARROW: [u8; 7] = [
            0b0001000, 0b0001100, 0b1111111, 0b1111111, 0b1111111, 0b0001100, 0b0001000,
        ];
        const V_ARROW: [u8; 7] = [
            0b0001000, 0b0011100, 0b0101010, 0b0001000, 0b0101010, 0b0011100, 0b0001000,
        ];
        const DIAG_TLBR: [u8; 7] = [
            0b1100000, 0b1110000, 0b1111000, 0b0001000, 0b0001100, 0b0001110, 0b0000011,
        ];
        const DIAG_TRBL: [u8; 7] = [
            0b0000011, 0b0000111, 0b0001111, 0b0001000, 0b0011000, 0b0111000, 0b1100000,
        ];
        let pat: &[u8; 7] = match edge {
            Left | Right => &H_ARROW,
            Top | Bottom => &V_ARROW,
            TopLeft | BottomRight => &DIAG_TLBR,
            TopRight | BottomLeft => &DIAG_TRBL,
        };
        // İmleci tutamacın merkezine hizala.
        let ox = mx.saturating_sub(3);
        let oy = my.saturating_sub(3);
        for (row, &bits) in pat.iter().enumerate() {
            for col in 0..7 {
                if (bits & (1 << (6 - col))) != 0 {
                    fb.put_pixel(ox + col, oy + row, Color::CURSOR_FILL);
                }
            }
        }
    }
}
