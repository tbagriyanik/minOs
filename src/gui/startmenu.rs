//! Start menu layout and rendering.

use super::*;

impl MinGuiCompositor {
    /// Bir grubun başlığının Y konumu.
    pub fn start_menu_group_y(menu_y: usize, group_index: usize) -> usize {
        menu_y + sp(MENU_CONTENT_TOP) + group_index * sp(32)
    }

    /// Bir menü ÖĞESİNİN Y konumu.
    pub fn start_menu_item_y(menu_y: usize, item_index: usize) -> usize {
        let (g, k) = Self::item_group_and_index(item_index);
        let (_, sub_y, _, _) = Self::start_menu_submenu_rect(menu_y, g, menu_y + MENU_H + 4);
        sub_y + sp(28) + k * sp(24)
    }

    pub fn item_group_and_index(index: usize) -> (usize, usize) {
        let mut remaining = index;
        for (g, (_, _, count)) in MENU_GROUPS.iter().enumerate() {
            if remaining < *count {
                return (g, remaining);
            }
            remaining -= *count;
        }
        (3, 0)
    }

    pub fn start_menu_category_rect(menu_y: usize, group_index: usize) -> (usize, usize, usize, usize) {
        let x = 6 + 6;
        let y = menu_y + sp(MENU_CONTENT_TOP) + group_index * sp(32) + sp(4);
        let w = sp(190) - 12;
        let h = sp(28);
        (x, y, w, h)
    }

    pub fn start_search_results_rect(menu_y: usize, group_index: usize, tb_y: usize, count: usize) -> (usize, usize, usize, usize) {
        let sub_w = sp(240);
        let sub_h = sp(32) + count * sp(28) + sp(8);
        let sub_x = 6 + sp(190) + 4;
        let cat_y = menu_y + sp(MENU_CONTENT_TOP) + group_index * sp(32) + sp(4);
        let sub_y = cat_y.min(tb_y.saturating_sub(sub_h + 4));
        (sub_x, sub_y, sub_w, sub_h)
    }

    pub fn start_menu_submenu_rect(menu_y: usize, group_index: usize, tb_y: usize) -> (usize, usize, usize, usize) {
        let sub_w = sp(240);
        let count = MENU_GROUPS[group_index.min(3)].2;
        let sub_h = sp(32) + count * sp(28) + sp(8);
        let sub_x = 6 + sp(190) + 4;
        let cat_y = menu_y + sp(74) + group_index * sp(32) + sp(4);
        let sub_y = cat_y.min(tb_y.saturating_sub(sub_h + 4));
        (sub_x, sub_y, sub_w, sub_h)
    }

    pub fn start_menu_subitem_rect(menu_y: usize, group_index: usize, item_in_group: usize, tb_y: usize) -> (usize, usize, usize, usize) {
        let (sub_x, sub_y, sub_w, _) = Self::start_menu_submenu_rect(menu_y, group_index, tb_y);
        let x = sub_x + sp(6);
        let y = sub_y + sp(30) + item_in_group * sp(28);
        let w = sub_w.saturating_sub(sp(12));
        let h = sp(25);
        (x, y, w, h)
    }

    /// Başlat menüsü öğesinin tıklanabilir dikdörtgeni.
    pub fn start_menu_item_rect(menu_y: usize, index: usize, tb_y: usize) -> (usize, usize, usize, usize) {
        let (g, k) = Self::item_group_and_index(index);
        Self::start_menu_subitem_rect(menu_y, g, k, tb_y)
    }

    /// Menü öğesinin hedef uygulama indeksini döndürür.
    pub fn start_menu_app_idx(index: usize) -> Option<usize> {
        const APP_IDX: [usize; 14] = [0, 1, 7, 10, 11, 13, 2, 8, 9, 3, 4, 5, 6, 12];
        APP_IDX.get(index).copied()
    }
    /// Sorguyu bir ad içinde **parça (substring)** olarak arar ve bulunduğu
    /// konumun puanını döner. Küçük puan daha iyi eşleşmedir:
    /// `0` = ad başında, `1` = kelime başında, `2` = ad içinde herhangi bir yerde.
    /// Eşleşme yoksa `None`.
    fn start_search_score(name: &[u8], query: &[u8]) -> Option<u8> {
        if query.is_empty() || name.len() < query.len() {
            return None;
        }
        let first = query[0];
        let mut i = 0usize;
        let limit = name.len() - query.len();
        while i <= limit {
            // Ad içinde sorgunun ilk harfini ara.
            if !name[i].eq_ignore_ascii_case(&first) {
                i += 1;
                continue;
            }
            if name[i..i + query.len()].eq_ignore_ascii_case(query) {
                // Sıralama: ad başı (0), kelime başı (1), diğer (2).
                let score = if i == 0 {
                    0
                } else if name[i - 1] == b' ' {
                    1
                } else {
                    2
                };
                return Some(score);
            }
            i += 1;
        }
        None
    }

    pub fn start_search_matches(&self, english: bool, results: &mut [usize; 14]) -> usize {
        let query = &self.start_search[..self.start_search_len];
        if query.is_empty() { return 0; }
        const NAMES: [[&str; 2]; 14] = [
            ["Dosya Yöneticisi", "File Manager"], ["Sistem Ayarları", "Settings"],
            ["Not Defteri", "Notepad"], ["Web Tarayıcısı", "Web Browser"],
            ["Resim Gösterici", "Image Viewer"], ["Müzik Çalar", "Music Player"],
            ["Video Oynatıcı", "Video Player"], ["Görev Yöneticisi", "Task Manager"],
            ["Hesap Makinesi", "Calculator"], ["Çizim Tahtası", "Paint"],
            ["Komut İstemi", "Terminal"], ["Tek Tıkla Kur", "One-Click Install"],
            ["3D Gösterici", "3D Viewer"], ["Paket Yöneticisi", "Package Manager"],
        ];
        // Önce tüm eşleşmeleri (app_id, puan) olarak topla.
        let mut scored: [(usize, u8); 14] = [(0, 0); 14];
        let mut count = 0;
        for (app_id, names) in NAMES.iter().enumerate() {
            let localized = Self::start_search_score(names[english as usize].as_bytes(), query);
            let english_hit = Self::start_search_score(names[1].as_bytes(), query);
            let score = match (localized, english_hit) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            if let Some(s) = score {
                scored[count] = (app_id, s);
                count += 1;
            }
        }
        // Puanı küçük olan (daha iyi eşleşen) önce gelecek şekilde sırala;
        // eşit puanda orijinal uygulama sırası korunur (kararlı sıralama).
        for i in 1..count {
            let mut j = i;
            while j > 0 && scored[j - 1].1 > scored[j].1 {
                scored.swap(j - 1, j);
                j -= 1;
            }
        }
        for i in 0..count {
            results[i] = scored[i].0;
        }
        count
    }

    pub fn push_start_search(&mut self, ch: char) {
        if ch.is_ascii_alphanumeric() && self.start_search_len < self.start_search.len() {
            self.start_search[self.start_search_len] = ch.to_ascii_lowercase() as u8;
            self.start_search_len += 1;
        }
    }

    pub fn clear_start_search(&mut self) {
        self.start_search_len = 0;
        self.start_search.fill(0);
        self.selected_start_item = 0;
    }

    /// Başlat menüsünü kapatır ve arama durumunu sıfırlar. Menü her kapandığında
    /// kullanılmalı; böylece bir sonraki açılış temiz başlar.
    pub fn close_start_menu(&mut self) {
        self.is_start_menu_open = false;
        self.confirm_shutdown = false;
        self.confirm_restart = false;
        self.clear_start_search();
    }

    pub fn move_start_search_selection(&mut self, english: bool, down: bool) {
        let mut results = [0; 14];
        let count = self.start_search_matches(english, &mut results);
        if down {
            if count > 0 { self.selected_start_item = (self.selected_start_item + 1).min(count - 1); }
        } else {
            self.selected_start_item = self.selected_start_item.saturating_sub(1);
        }
    }

    pub fn selected_start_search_result(&self, english: bool) -> Option<usize> {
        let mut results = [0; 14];
        let count = self.start_search_matches(english, &mut results);
        (self.selected_start_item < count).then_some(results[self.selected_start_item])
    }
    /// Başlat menüsünün ekran üzerindeki dikdörtgeni (x, y, w, h).
    /// Genişlik ve yükseklik aktif font ölçeğine göre otomatik büyür.
    pub fn start_menu_rect(tb_y: usize) -> (usize, usize, usize, usize) {
        let menu_w = sp(190);
        let menu_h = sp(MENU_H + 34);
        let menu_x = 6;
        let menu_y = tb_y.saturating_sub(menu_h + 4);
        (menu_x, menu_y, menu_w, menu_h)
    }

    /// Başlat menüsündeki arama kutusunun dikdörtgeni (x, y, w, h).
    /// Profil şeridinin hemen altına yerleşir; çizim ve tıklama testi bunu kullanır.
    pub fn start_search_rect(menu_x: usize, menu_y: usize, menu_w: usize) -> (usize, usize, usize, usize) {
        let strip_h = sp(34) + Self::icon_size();
        let search_h = sp(22);
        // Arama alanını profil şeridinin altına ve kategori listesinin üstüne
        // koy; iki alan ölçek değişse de üst üste binmesin.
        let search_y = menu_y + strip_h + sp(4);
        (menu_x + sp(8), search_y, menu_w.saturating_sub(sp(16)), search_h)
    }

    pub fn render_start_menu(&self, fb: &mut Framebuffer, tb_y: usize, english: bool, mx: usize, my: usize) {
        let (menu_x, menu_y, menu_w, menu_h) = Self::start_menu_rect(tb_y);
        fb.draw_shadow(menu_x, menu_y, menu_w, menu_h, Color::SHADOW);
        fb.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::TASKBAR_BG);
        fb.draw_rect_outline(menu_x, menu_y, menu_w, menu_h, Color::BORDER);

        // Profil Şeridi (gradyan)
        let strip_h = sp(34) + Self::icon_size();
        fb.draw_vgradient(menu_x + 1, menu_y + 1, menu_w - 2, strip_h, Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -16));
        let icon_sz = Self::icon_size();
        Self::draw_cat_icon(fb, menu_x + sp(8), menu_y + sp(8));
        let title_x = menu_x + sp(8) + icon_sz + sp(6);
        let text_h2 = Self::text_height();
        let title_y = menu_y + (strip_h.saturating_sub(text_h2)) / 2;
        Self::draw_string_shadow(fb, title_x, title_y, "minOs", Color::TEXT_MAIN, Color::SHADOW, 1);
        let settings_hover = in_rect(mx, my, (menu_x + menu_w - 34, menu_y + 6, 26, 22));
        if settings_hover { fb.draw_rounded_rect(menu_x + menu_w - 34, menu_y + 6, 26, 22, 4, Color::ACCENT_BLUE); }
        // Ayarlar menü öğesinde kullanılan mevcut uygulama simgesini yeniden kullan.
        Self::draw_icon_by_index(fb, menu_x + menu_w - 28, menu_y + 9, Self::window_icon_index(1));
        let search_rect = Self::start_search_rect(menu_x, menu_y, menu_w);
        fb.draw_rounded_rect(search_rect.0, search_rect.1, search_rect.2, search_rect.3, 4, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(search_rect.0, search_rect.1, search_rect.2, search_rect.3, 4, Color::BORDER);
        let search_text = if self.start_search_len == 0 { ui_text("Uygulama ara...", "Search apps...") }
            else { core::str::from_utf8(&self.start_search[..self.start_search_len]).unwrap_or("") };
        Self::draw_string_clipped(fb, search_rect.0 + 6, search_rect.1 + 4, search_text, Color::TEXT_MAIN,
            (search_rect.0 + 4, search_rect.1 + 2, search_rect.2.saturating_sub(8), search_rect.3.saturating_sub(4)));

        // Ana Menü Alt Başlıkları (Kategoriler)
        let category_titles = if english {
            ["System", "Productivity", "Media", "Power"]
        } else {
            ["Sistem", "Üretkenlik", "Medya", "Güç"]
        };

        let active_g = self.active_start_group.min(3);

        for (g, title) in category_titles.iter().enumerate() {
            let r = Self::start_menu_category_rect(menu_y, g);
            let is_hovered = in_rect(mx, my, r);
            let is_active = g == active_g;

            let bg_col = if is_active {
                Color::ACCENT_BLUE
            } else if is_hovered {
                Color::TITLE_BAR
            } else {
                Color::WINDOW_BG
            };

            let fg_col = if is_active { Self::text_on(bg_col) } else { Color::TEXT_MAIN };

            fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 5, bg_col);
            if is_active {
                fb.draw_rounded_rect_outline(r.0, r.1, r.2, r.3, 5, Color::WHITE);
            }

            Self::draw_string(fb, r.0 + 8, r.1 + 10, title, fg_col);
            let arrow_x = (r.0 + r.2 - 13) as f32;
            let arrow_y = (r.1 + r.3 / 2) as f32;
            let arrow_color = Self::text_on(bg_col);
            Self::draw_antialiased_line(fb, arrow_x - 2.5, arrow_y - 4.0, arrow_x + 1.5, arrow_y, arrow_color, 1.7);
            Self::draw_antialiased_line(fb, arrow_x + 1.5, arrow_y, arrow_x - 2.5, arrow_y + 4.0, arrow_color, 1.7);
        }

        // Yana Açılır Alt Menü
        if self.start_search_len == 0 {
            self.render_start_submenu(fb, menu_y, active_g, tb_y, english, mx, my);
        } else {
            let mut results = [0; 14];
            let count = self.start_search_matches(english, &mut results);
            let (sub_x, sub_y, sub_w, sub_h) = Self::start_search_results_rect(menu_y, active_g, tb_y, count.max(1));
            fb.draw_shadow(sub_x, sub_y, sub_w, sub_h, Color::SHADOW);
            fb.draw_rounded_rect(sub_x, sub_y, sub_w, sub_h, 8, Color::TASKBAR_BG);
            fb.draw_rounded_rect_outline(sub_x, sub_y, sub_w, sub_h, 8, Color::BORDER);
            fb.draw_vgradient(sub_x + 1, sub_y + 1, sub_w - 2, 24, Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -16));
            Self::draw_string_shadow(fb, sub_x + 8, sub_y + 7, ui_text("Arama Sonuçları", "Search Results"), Color::TEXT_MAIN, Color::SHADOW, 1);

            let names = ["Dosya Yöneticisi", "Sistem Ayarları", "Not Defteri", "Web Tarayıcısı", "Resim Gösterici", "Müzik Çalar", "Video Oynatıcı", "Görev Yöneticisi", "Hesap Makinesi", "Çizim Tahtası", "Komut İstemi", "Tek Tıkla Kur", "3D Gösterici", "Paket Yöneticisi"];
            let en_names = ["File Manager", "Settings", "Notepad", "Web Browser", "Image Viewer", "Music Player", "Video Player", "Task Manager", "Calculator", "Paint", "Terminal", "One-Click Install", "3D Viewer", "Package Manager"];
            if count == 0 {
                Self::draw_string(fb, sub_x + 12, sub_y + 38, ui_text("Sonuç bulunamadı", "No apps found"), Color::TEXT_MUTED);
            }
            for row in 0..count {
                let r = (sub_x + sp(6), sub_y + sp(30) + row * sp(28), sub_w.saturating_sub(sp(12)), sp(25));
                let is_selected = row == self.selected_start_item;
                let is_hovered = in_rect(mx, my, r);
                if is_selected { fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 4, Color::ACCENT_BLUE); }
                else if is_hovered { fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 4, Color::TITLE_BAR); }
                let app_id = results[row];
                let app_name = if english { en_names[app_id] } else { names[app_id] };
                let text_y = r.1 + (r.3.saturating_sub(Self::text_height())) / 2;
                Self::draw_string_clipped(fb, r.0 + 8, text_y, app_name,
                    if is_selected { Self::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN },
                    (r.0 + 4, r.1 + 2, r.2 - 8, r.3 - 4));
            }
        }
    }

    pub fn render_start_submenu(&self, fb: &mut Framebuffer, menu_y: usize, group_idx: usize, tb_y: usize, english: bool, mx: usize, my: usize) {
        let (sub_x, sub_y, sub_w, sub_h) = Self::start_menu_submenu_rect(menu_y, group_idx, tb_y);

        fb.draw_shadow(sub_x, sub_y, sub_w, sub_h, Color::SHADOW);
        fb.draw_rounded_rect(sub_x, sub_y, sub_w, sub_h, 8, Color::TASKBAR_BG);
        fb.draw_rounded_rect_outline(sub_x, sub_y, sub_w, sub_h, 8, Color::BORDER);

        let sub_header_title = match group_idx {
            0 => crate::gui::ui_text("Sistem Uygulamaları", "System Apps"),
            1 => crate::gui::ui_text("Üretkenlik Araçları", "Productivity Tools"),
            2 => crate::gui::ui_text("Medya ve Eğlence", "Media & Entertainment"),
            _ => crate::gui::ui_text("Güç Seçenekleri", "Power Options"),
        };
        fb.draw_vgradient(sub_x + 1, sub_y + 1, sub_w - 2, 24, Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -16));
        Self::draw_string_shadow(fb, sub_x + 8, sub_y + 7, sub_header_title, Color::TEXT_MAIN, Color::SHADOW, 1);

        let menu_items: [(&str, u32, usize); 16] = if english { [
            ("File Manager", Color::ACCENT_YELLOW, 0),
            ("Settings", Color::ACCENT_BLUE, 1usize),
            ("Task Manager", Color::ACCENT_BLUE, 7),
            ("Terminal", Color::ACCENT_GREEN, 10),
            ("One-Click Install", Color::ACCENT_GREEN, 11),
            ("Package Manager", Color::ACCENT_PINK, 13),
            ("Notepad", Color::TEXT_MAIN, 2),
            ("Calculator", Color::ACCENT_GREEN, 8),
            ("Paint", Color::ACCENT_PINK, 9),
            ("Web Browser", Color::ACCENT_GREEN, 3),
            ("Image Viewer", Color::ACCENT_PINK, 4),
            ("Music Player", Color::ACCENT_YELLOW, 5),
            ("Video Player", Color::ACCENT_RED, 6),
            ("3D Viewer", Color::ACCENT_BLUE, 12),
            ("Shut Down", Color::ACCENT_RED, 255),
            ("Restart", Color::ACCENT_YELLOW, 254),
        ] } else { [
            ("Dosya Yöneticisi", Color::ACCENT_YELLOW, 0),
            ("Sistem Ayarları", Color::ACCENT_BLUE, 1usize),
            ("Görev Yöneticisi", Color::ACCENT_BLUE, 7),
            ("Komut İstemi", Color::ACCENT_GREEN, 10),
            ("Tek Tıkla Kur", Color::ACCENT_GREEN, 11),
            ("Paket Yöneticisi", Color::ACCENT_PINK, 13),
            ("Not Defteri", Color::TEXT_MAIN, 2),
            ("Hesap Makinesi", Color::ACCENT_GREEN, 8),
            ("Çizim Tahtası", Color::ACCENT_PINK, 9),
            ("Web Tarayıcısı", Color::ACCENT_GREEN, 3),
            ("Resim Gösterici", Color::ACCENT_PINK, 4),
            ("Müzik Çalar", Color::ACCENT_YELLOW, 5),
            ("Video Oynatıcı", Color::ACCENT_RED, 6),
            ("3D Gösterici", Color::ACCENT_BLUE, 12),
            ("Sistemi Kapat", Color::ACCENT_RED, 255),
            ("Yeniden Başlat", Color::ACCENT_YELLOW, 254),
        ] };

        let (_, first, count) = MENU_GROUPS[group_idx];

        for k in 0..count {
            let i = first + k;
            let (name, _col, app_or_code) = menu_items[i];
            let r = Self::start_menu_subitem_rect(menu_y, group_idx, k, tb_y);

            let is_hovered = in_rect(mx, my, r) || (self.selected_start_item == k);
            let bg_col = if (i == 14 && self.confirm_shutdown) || (i == 15 && self.confirm_restart) {
                Color::ACCENT_RED
            } else if is_hovered {
                Color::ACCENT_BLUE
            } else if i >= 14 {
                Color::TITLE_BAR
            } else {
                Color::WINDOW_BG
            };

            fb.draw_rounded_rect(r.0, r.1, r.2, r.3, 4, bg_col);
            if is_hovered {
                fb.draw_rounded_rect_outline(r.0, r.1, r.2, r.3, 4, Color::WHITE);
            }

            // Çizim: Simgeler (İkonlar) — öğe içinde dikey ortalanır.
            let icon_y = r.1 + r.3.saturating_sub(Self::icon_size()) / 2;
            if app_or_code < 14 {
                let icon_idx = Self::window_icon_index(app_or_code);
                Self::draw_icon_by_index_scaled(fb, r.0 + sp(6), icon_y, icon_idx);
            } else {
                // Güç simgeleri (kedi veya varsayılan simge)
                Self::draw_cat_icon_scaled(fb, r.0 + sp(6), icon_y);
            }

            let label = if i == 14 && self.confirm_shutdown {
                crate::gui::ui_text("KAPATMAYI ONAYLA (tekrar tıkla)", "CONFIRM SHUTDOWN (click again)")
            } else if i == 15 && self.confirm_restart {
                crate::gui::ui_text("YENİDEN BAŞLAT? TEKRAR TIKLA", "CONFIRM RESTART (click again)")
            } else {
                name
            };

            let text_col = if is_hovered { Self::text_on(bg_col) } else { Color::TEXT_MAIN };
            // Metin konumu ikon boyutundan türetilir: sol boşluk + ikon + boşluk.
            // Böylece ikon büyüse de metin ikonun üzerine binmez (taşma/kayma yok).
            let text_off = sp(6) + Self::icon_size() + sp(8);
            let text_x = r.0 + text_off;
            let text_y = r.1 + r.3.saturating_sub(Self::text_height()) / 2;
            Self::draw_string_clipped(fb, text_x, text_y, label, text_col,
                (r.0 + text_off - sp(2), r.1, r.2.saturating_sub(text_off), r.3));
        }
    }
}
