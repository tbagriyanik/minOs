//! Right-click context menu types and rendering.

use super::*;

// ---------------------------------------------------------------------------
// Bağlam Menüsü (sağ tık) — tıklanan yere göre değişir
// ---------------------------------------------------------------------------
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ContextKind {
    /// Masaüstü boş alanı
    Desktop,
    DesktopItem,
    /// Dosya Yöneticisi içindeki bir dosya
    FileManager,
    /// Not Defteri metin alanı (Kes/Kopyala/Yapıştır...)
    Notepad,
    /// Görev Yöneticisi süreç satırı (Sonlandır...)
    TaskManager,
    /// Pencere başlık çubuğu / gövdesi
    Window,
}

/// Masaüstü sağ tık menüsü.
pub const CTX_DESKTOP: [(&str, u32); 8] = [
    ("Dosya Yöneticisini Aç", Color::ACCENT_BLUE),
    ("Yeni >", Color::ACCENT_GREEN),
    ("Yapıştır", Color::ACCENT_GREEN),
    ("Sistem Ayarları", Color::ACCENT_PINK),
    ("Görev Yöneticisi", Color::ACCENT_YELLOW),
    ("Tema Değiştir", Color::ACCENT_GREEN),
    ("Masaüstünü Yenile", Color::TEXT_MUTED),
    ("Yeni Not Oluştur", Color::TEXT_MAIN),
];
pub const CTX_DESKTOP_ITEM: [(&str, u32); 9] = [
    ("Aç / Klasöre Gir", Color::ACCENT_GREEN),
    ("Not Defterinde Düzenle", Color::TEXT_MAIN),
    ("Kes", Color::ACCENT_RED),
    ("Kopyala", Color::ACCENT_BLUE),
    ("Yapıştır", Color::ACCENT_GREEN),
    ("Yeni >", Color::ACCENT_YELLOW),
    ("Adını Değiştir", Color::ACCENT_YELLOW),
    ("Sil", Color::ACCENT_RED),
    ("Özellikler", Color::TEXT_MUTED),
];

/// Dosya Yöneticisi'nde bir dosyaya veya klasöre sağ tık menüsü.
pub const CTX_FILE: [(&str, u32); 12] = [
    ("Aç / Klasöre Gir", Color::ACCENT_GREEN),
    ("Not Defterinde Düzenle", Color::TEXT_MAIN),
    ("Kes", Color::ACCENT_RED),
    ("Kopyala", Color::ACCENT_BLUE),
    ("Yapıştır", Color::ACCENT_GREEN),
    ("Yeni >", Color::ACCENT_YELLOW),
    ("Adını Değiştir", Color::ACCENT_YELLOW),
    ("Dosyayı / Klasörü Sil", Color::ACCENT_RED),
    ("Özellikler", Color::TEXT_MUTED),
    ("Sırala: Ad", Color::ACCENT_BLUE),
    ("Sırala: Boyut", Color::ACCENT_BLUE),
    ("Sırala: Tür", Color::ACCENT_BLUE),
];

/// Not Defteri metin alanına sağ tık menüsü.
pub const CTX_NOTEPAD: [(&str, u32); 6] = [
    ("Kes", Color::ACCENT_RED),
    ("Kopyala", Color::ACCENT_BLUE),
    ("Yapıştır", Color::ACCENT_GREEN),
    ("Sil", Color::ACCENT_YELLOW),
    ("Tümünü Seç", Color::ACCENT_PINK),
    ("Tümünü Temizle", Color::TEXT_MUTED),
];

/// Görev Yöneticisi süreç satırına sağ tık menüsü.
pub const CTX_TASKMANAGER: [(&str, u32); 1] = [("Sonlandır", Color::ACCENT_RED)];

/// Pencere başlık çubuğuna sağ tık menüsü.
pub const CTX_WINDOW: [(&str, u32); 4] = [
    ("Küçült", Color::ACCENT_YELLOW),
    ("Maksimize Et / Geri Yükle", Color::ACCENT_GREEN),
    ("Pencereyi Ortala", Color::ACCENT_BLUE),
    ("Kapat", Color::ACCENT_RED),
];

const CTX_DESKTOP_EN: [(&str, u32); 8] = [
    ("Open File Manager", Color::ACCENT_BLUE),
    ("New >", Color::ACCENT_GREEN),
    ("Paste", Color::ACCENT_GREEN),
    ("System Settings", Color::ACCENT_PINK),
    ("Task Manager", Color::ACCENT_YELLOW),
    ("Change Theme", Color::ACCENT_GREEN),
    ("Refresh Desktop", Color::TEXT_MUTED),
    ("New Note", Color::TEXT_MAIN),
];
const CTX_DESKTOP_ITEM_EN: [(&str, u32); 9] = [
    ("Open / Enter Folder", Color::ACCENT_GREEN),
    ("Edit in Notepad", Color::TEXT_MAIN),
    ("Cut", Color::ACCENT_RED),
    ("Copy", Color::ACCENT_BLUE),
    ("Paste", Color::ACCENT_GREEN),
    ("New >", Color::ACCENT_YELLOW),
    ("Rename", Color::ACCENT_YELLOW),
    ("Delete", Color::ACCENT_RED),
    ("Properties", Color::TEXT_MUTED),
];
const CTX_FILE_EN: [(&str, u32); 12] = [
    ("Open / Enter Folder", Color::ACCENT_GREEN),
    ("Edit in Notepad", Color::TEXT_MAIN),
    ("Cut", Color::ACCENT_RED),
    ("Copy", Color::ACCENT_BLUE),
    ("Paste", Color::ACCENT_GREEN),
    ("New >", Color::ACCENT_YELLOW),
    ("Rename", Color::ACCENT_YELLOW),
    ("Delete", Color::ACCENT_RED),
    ("Properties", Color::TEXT_MUTED),
    ("Sort: Name", Color::ACCENT_BLUE),
    ("Sort: Size", Color::ACCENT_BLUE),
    ("Sort: Type", Color::ACCENT_BLUE),
];
const CTX_NOTEPAD_EN: [(&str, u32); 6] = [
    ("Cut", Color::ACCENT_RED),
    ("Copy", Color::ACCENT_BLUE),
    ("Paste", Color::ACCENT_GREEN),
    ("Delete", Color::ACCENT_YELLOW),
    ("Select All", Color::ACCENT_PINK),
    ("Clear All", Color::TEXT_MUTED),
];
const CTX_TASKMANAGER_EN: [(&str, u32); 1] = [("End Task", Color::ACCENT_RED)];
const CTX_WINDOW_EN: [(&str, u32); 4] = [
    ("Minimize", Color::ACCENT_YELLOW),
    ("Maximize / Restore", Color::ACCENT_GREEN),
    ("Center Window", Color::ACCENT_BLUE),
    ("Close", Color::ACCENT_RED),
];

pub const CONTEXT_MENU_W: usize = 226;
pub const CONTEXT_MENU_ITEM_H: usize = 26;

/// Bağlam menüsü başlık (header) yüksekliği — aktif font ölçeğine göre.
#[inline(always)]
pub fn context_header_h() -> usize {
    crate::gui::sp(CONTEXT_MENU_ITEM_H - 4)
}

/// Bağlam menüsü öğe yüksekliği — aktif font ölçeğine göre.
#[inline(always)]
pub fn context_item_h() -> usize {
    crate::gui::sp(CONTEXT_MENU_ITEM_H)
}

/// Masaüstü ve Dosya Yöneticisi için Yeni (New) alt menüsü öğeleri.
pub const CTX_NEW: [(&str, u32); 4] = [
    ("Metin Belgesi (.txt)", Color::TEXT_MAIN),
    ("Klasör", Color::ACCENT_YELLOW),
    ("Çizim / Resim (.bmp)", Color::ACCENT_PINK),
    ("Boş Belge", Color::ACCENT_BLUE),
];

pub const CTX_NEW_EN: [(&str, u32); 4] = [
    ("Text Document (.txt)", Color::TEXT_MAIN),
    ("Folder", Color::ACCENT_YELLOW),
    ("Bitmap Image (.bmp)", Color::ACCENT_PINK),
    ("Empty File", Color::ACCENT_BLUE),
];

impl MinGuiCompositor {
    /// Aktif bağlam menüsünün öğe listesi.
    pub fn context_items(&self) -> &'static [(&'static str, u32)] {
        match (self.context_kind, ui_english()) {
            (ContextKind::Desktop, false) => &CTX_DESKTOP,
            (ContextKind::DesktopItem, false) => &CTX_DESKTOP_ITEM,
            (ContextKind::FileManager, false) => &CTX_FILE,
            (ContextKind::Notepad, false) => &CTX_NOTEPAD,
            (ContextKind::TaskManager, false) => &CTX_TASKMANAGER,
            (ContextKind::Window, false) => &CTX_WINDOW,
            (ContextKind::Desktop, true) => &CTX_DESKTOP_EN,
            (ContextKind::DesktopItem, true) => &CTX_DESKTOP_ITEM_EN,
            (ContextKind::FileManager, true) => &CTX_FILE_EN,
            (ContextKind::Notepad, true) => &CTX_NOTEPAD_EN,
            (ContextKind::TaskManager, true) => &CTX_TASKMANAGER_EN,
            (ContextKind::Window, true) => &CTX_WINDOW_EN,
        }
    }

    /// Bağlam menüsünün ekran üzerindeki dikdörtgeni (çizim ve tıklama ortak).
    /// Öğe yüksekliği aktif font ölçeğine göre otomatik büyür.
    pub fn context_menu_geom(&self, fb_w: usize, fb_h: usize) -> (usize, usize, usize, usize) {
        let items = self.context_items();
        let widest_item = items
            .iter()
            .map(|(label, _)| Self::text_width(label, 1))
            .max()
            .unwrap_or(0);
        let icon_pad = match self.context_kind {
            ContextKind::Desktop | ContextKind::DesktopItem | ContextKind::Notepad => sp(24),
            _ => 0,
        };
        let shortcut_pad = match self.context_kind {
            ContextKind::Notepad
            | ContextKind::TaskManager
            | ContextKind::Desktop
            | ContextKind::DesktopItem
            | ContextKind::FileManager => sp(64),
            _ => 0,
        };
        let w = sp(CONTEXT_MENU_W + 16)
            .max(widest_item + sp(24) + icon_pad + shortcut_pad)
            .min(fb_w);
        // Başlık + öğeler (ölçekli) + alt boşluk
        let h = context_header_h() + items.len() * context_item_h();
        let x = self.context_menu_x.min(fb_w.saturating_sub(w));
        let y = self.context_menu_y.min(fb_h.saturating_sub(h));
        (x, y, w, h)
    }

    pub fn render_context_menu(&self, fb: &mut Framebuffer) {
        let (menu_x, menu_y, menu_w, menu_h) = self.context_menu_geom(fb.width, fb.height);

        fb.draw_shadow(menu_x, menu_y, menu_w, menu_h, Color::SHADOW);
        fb.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::TASKBAR_BG);
        fb.draw_rect_outline(menu_x, menu_y, menu_w, menu_h, Color::BORDER);
        fb.draw_rect(menu_x, menu_y, menu_w, 2, Color::ACCENT_BLUE);

        // Menü başlığı (tür)
        let header = match self.context_kind {
            ContextKind::Desktop => ui_text("Masaüstü", "Desktop"),
            ContextKind::DesktopItem => ui_text("Masaüstü Öğesi", "Desktop Item"),
            ContextKind::FileManager => ui_text("Dosya İşlemleri", "File Actions"),
            ContextKind::Notepad => ui_text("Düzenleme", "Edit"),
            ContextKind::TaskManager => ui_text("Görev", "Task"),
            ContextKind::Window => ui_text("Pencere", "Window"),
        };
        let hdr_h = context_header_h();
        let item_h = context_item_h();
        Self::draw_string_clipped(
            fb,
            menu_x + 8,
            menu_y + sp(6),
            header,
            Color::TEXT_MUTED,
            (
                menu_x + 6,
                menu_y + 2,
                menu_w.saturating_sub(12),
                hdr_h.saturating_sub(3),
            ),
        );
        fb.draw_rect(
            menu_x + 4,
            menu_y + hdr_h - sp(4),
            menu_w - 8,
            1,
            Color::BORDER,
        );

        for (i, (label, color)) in self.context_items().iter().enumerate() {
            let item_y = menu_y + hdr_h + i * item_h;
            fb.draw_rect(
                menu_x + 4,
                item_y,
                menu_w - 8,
                item_h.saturating_sub(sp(4)),
                Color::WINDOW_BG,
            );

            let mut text_x = menu_x + sp(10);
            if self.context_kind == ContextKind::Desktop
                || self.context_kind == ContextKind::DesktopItem
            {
                // Masaüstü sağ tık menüsü simgeleri (12x12 piksel)
                let icon_x = menu_x + sp(10);
                let icon_y = item_y + (item_h.saturating_sub(sp(4)).saturating_sub(12)) / 2;
                Self::draw_desktop_context_icon(fb, icon_x, icon_y, i, *color);
                text_x += sp(20);
            } else if self.context_kind == ContextKind::Notepad {
                // Not Defteri / Textarea sağ tık menüsü simgeleri (12x12 piksel)
                let icon_x = menu_x + sp(10);
                let icon_y = item_y + (item_h.saturating_sub(sp(4)).saturating_sub(12)) / 2;
                Self::draw_notepad_context_icon(fb, icon_x, icon_y, i, *color);
                text_x += sp(20);
            }

            Self::draw_string_clipped(
                fb,
                text_x,
                item_y + sp(5),
                label,
                *color,
                (
                    text_x,
                    item_y + 2,
                    menu_w.saturating_sub(text_x - menu_x + sp(60)),
                    item_h.saturating_sub(4),
                ),
            );

            // Menü öğeleri için sağa hizalı kısayol etiketi (örn: Ctrl+C, Ctrl+V, Ctrl+X, Del, F2)
            let shortcut = match (self.context_kind, i) {
                (ContextKind::TaskManager, 0) => "Del",
                (ContextKind::Notepad, 0) => "Ctrl+X",
                (ContextKind::Notepad, 1) => "Ctrl+C",
                (ContextKind::Notepad, 2) => "Ctrl+V",
                (ContextKind::Notepad, 3) => "Del",
                (ContextKind::Notepad, 4) => "Ctrl+A",
                (ContextKind::Desktop, 2) => "Ctrl+V",
                (ContextKind::DesktopItem, 2) | (ContextKind::FileManager, 2) => "Ctrl+X",
                (ContextKind::DesktopItem, 3) | (ContextKind::FileManager, 3) => "Ctrl+C",
                (ContextKind::DesktopItem, 4) | (ContextKind::FileManager, 4) => "Ctrl+V",
                (ContextKind::DesktopItem, 6) | (ContextKind::FileManager, 6) => "F2",
                (ContextKind::DesktopItem, 7) | (ContextKind::FileManager, 7) => "Del",
                _ => "",
            };
            if !shortcut.is_empty() {
                let sc_w = Self::text_width(shortcut, 1);
                let sc_x = menu_x + menu_w.saturating_sub(sc_w + sp(12));
                Self::draw_string_clipped(
                    fb,
                    sc_x,
                    item_y + sp(5),
                    shortcut,
                    Color::TEXT_MUTED,
                    (sc_x, item_y + 2, sc_w + sp(4), item_h.saturating_sub(4)),
                );
            }
        }

        // Açık ise yana açılır Yeni alt menüsünü çiz
        if self.is_context_submenu_open && self.has_context_submenu() {
            self.render_context_submenu(fb);
        }
    }

    /// Mevcut bağlam menüsünün "Yeni ▶" alt menüsünü içerip içermediğini belirtir.
    pub fn has_context_submenu(&self) -> bool {
        self.context_kind == ContextKind::Desktop
            || self.context_kind == ContextKind::DesktopItem
            || self.context_kind == ContextKind::FileManager
    }

    /// "Yeni ▶" menü öğesinin ana menüdeki satır indeksi.
    pub fn context_submenu_parent_idx(&self) -> usize {
        match self.context_kind {
            ContextKind::Desktop => 1,
            ContextKind::DesktopItem => 4,
            ContextKind::FileManager => 5,
            _ => 1,
        }
    }

    /// "Yeni ▶" alt menüsünün ekran üzerindeki koordinatları (x, y, w, h).
    pub fn context_submenu_geom(&self, fb_w: usize, fb_h: usize) -> (usize, usize, usize, usize) {
        let (menu_x, menu_y, menu_w, _menu_h) = self.context_menu_geom(fb_w, fb_h);
        let items: &[(&str, u32)] = if ui_english() { &CTX_NEW_EN } else { &CTX_NEW };
        let widest_item = items
            .iter()
            .map(|(label, _)| Self::text_width(label, 1))
            .max()
            .unwrap_or(0);
        let sub_w = sp(180).max(widest_item + sp(36)).min(fb_w);
        let item_h = context_item_h();
        let hdr_h = context_header_h();
        let sub_h = hdr_h + items.len() * item_h;

        let parent_idx = self.context_submenu_parent_idx();
        let target_y = menu_y + hdr_h + parent_idx * item_h;

        // Ekranın sağına sığmıyorsa soluna aç
        let sub_x = if menu_x + menu_w + sub_w + 4 <= fb_w {
            menu_x + menu_w.saturating_sub(2)
        } else {
            menu_x.saturating_sub(sub_w.saturating_sub(2))
        };
        let sub_y = target_y.min(fb_h.saturating_sub(sub_h));
        (sub_x, sub_y, sub_w, sub_h)
    }

    /// "Yeni ▶" alt menüsünü çizer.
    pub fn render_context_submenu(&self, fb: &mut Framebuffer) {
        let (sub_x, sub_y, sub_w, sub_h) = self.context_submenu_geom(fb.width, fb.height);
        let items: &[(&str, u32)] = if ui_english() { &CTX_NEW_EN } else { &CTX_NEW };
        let hdr_h = context_header_h();
        let item_h = context_item_h();

        fb.draw_shadow(sub_x, sub_y, sub_w, sub_h, Color::SHADOW);
        fb.draw_rect(sub_x, sub_y, sub_w, sub_h, Color::TASKBAR_BG);
        fb.draw_rect_outline(sub_x, sub_y, sub_w, sub_h, Color::BORDER);
        fb.draw_rect(sub_x, sub_y, sub_w, 2, Color::ACCENT_YELLOW);

        let sub_header = ui_text("Yeni Oluştur", "Create New");
        Self::draw_string_clipped(
            fb,
            sub_x + 8,
            sub_y + sp(6),
            sub_header,
            Color::TEXT_MUTED,
            (
                sub_x + 6,
                sub_y + 2,
                sub_w.saturating_sub(12),
                hdr_h.saturating_sub(3),
            ),
        );
        fb.draw_rect(
            sub_x + 4,
            sub_y + hdr_h - sp(4),
            sub_w - 8,
            1,
            Color::BORDER,
        );

        for (i, (label, color)) in items.iter().enumerate() {
            let item_y = sub_y + hdr_h + i * item_h;
            fb.draw_rect(
                sub_x + 4,
                item_y,
                sub_w - 8,
                item_h.saturating_sub(sp(4)),
                Color::WINDOW_BG,
            );

            let icon_x = sub_x + sp(10);
            let icon_y = item_y + (item_h.saturating_sub(sp(4)).saturating_sub(12)) / 2;
            Self::draw_new_submenu_icon(fb, icon_x, icon_y, i, *color);

            let text_x = sub_x + sp(30);
            Self::draw_string_clipped(
                fb,
                text_x,
                item_y + sp(5),
                label,
                *color,
                (
                    text_x,
                    item_y + 2,
                    sub_w.saturating_sub(sp(36)),
                    item_h.saturating_sub(4),
                ),
            );
        }
    }

    /// Yeni alt menüsü için 12x12 piksel simgeler çizer.
    fn draw_new_submenu_icon(fb: &mut Framebuffer, x: usize, y: usize, index: usize, color: u32) {
        match index {
            0 => {
                // Metin Belgesi (.txt): Sayfa ve metin satırları
                fb.draw_rect_outline(x + 1, y, 8, 11, color);
                fb.draw_rect(x + 3, y + 3, 4, 1, color);
                fb.draw_rect(x + 3, y + 5, 4, 1, color);
                fb.draw_rect(x + 3, y + 7, 3, 1, color);
            }
            1 => {
                // Klasör: Sarı klasör
                fb.draw_rect(x + 1, y + 2, 4, 2, Color::ACCENT_YELLOW);
                fb.draw_rounded_rect(x, y + 3, 12, 8, 2, Color::ACCENT_YELLOW);
                fb.draw_rect(x + 2, y + 5, 8, 4, Color::WINDOW_BG);
            }
            2 => {
                // Çizim / Resim (.bmp): Resim çerçevesi / palet
                fb.draw_rect_outline(x, y + 1, 12, 10, color);
                fb.draw_rect(x + 2, y + 3, 2, 2, Color::ACCENT_RED);
                fb.draw_rect(x + 5, y + 6, 2, 2, Color::ACCENT_GREEN);
                fb.draw_rect(x + 8, y + 4, 2, 2, Color::ACCENT_BLUE);
            }
            _ => {
                // Boş Belge: Temiz sayfa
                fb.draw_rect_outline(x + 2, y, 8, 11, color);
                fb.draw_rect(x + 4, y + 5, 4, 2, Color::ACCENT_BLUE);
            }
        }
    }

    /// Yeni alt menüsünde tıklanan öğe indeksini döndürür.
    pub fn context_submenu_hit(
        &self,
        fb_w: usize,
        fb_h: usize,
        mx: usize,
        my: usize,
    ) -> Option<usize> {
        if !self.is_context_submenu_open || !self.has_context_submenu() {
            return None;
        }
        let (sub_x, sub_y, sub_w, sub_h) = self.context_submenu_geom(fb_w, fb_h);
        if !in_rect(mx, my, (sub_x, sub_y, sub_w, sub_h)) {
            return None;
        }
        let hdr_h = context_header_h();
        if my < sub_y + hdr_h {
            return None;
        }
        let rel = my - (sub_y + hdr_h);
        let idx = rel / context_item_h();
        if idx < 4 {
            Some(idx)
        } else {
            None
        }
    }

    /// Not Defteri / Textarea sağ tık bağlam menüsü için 12x12 piksel simgeler çizer.
    fn draw_notepad_context_icon(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        index: usize,
        color: u32,
    ) {
        match index {
            0 => {
                // Kes (Makas simgesi)
                let c = color;
                // Sol halka & sağ halka
                fb.draw_rect_outline(x + 1, y + 8, 3, 3, c);
                fb.draw_rect_outline(x + 7, y + 8, 3, 3, c);
                // Makas bıçakları
                fb.draw_rect(x + 3, y + 6, 2, 2, c);
                fb.draw_rect(x + 6, y + 6, 2, 2, c);
                fb.draw_rect(x + 4, y + 4, 3, 2, c);
                fb.draw_rect(x + 2, y + 1, 2, 3, c);
                fb.draw_rect(x + 7, y + 1, 2, 3, c);
            }
            1 => {
                // Kopyala (Üst üste iki sayfa simgesi)
                let c = color;
                // Arka sayfa
                fb.draw_rect_outline(x + 3, y, 7, 8, Color::TEXT_MUTED);
                // Ön sayfa
                fb.draw_rect(x, y + 3, 8, 9, Color::WINDOW_BG);
                fb.draw_rect_outline(x, y + 3, 8, 9, c);
                fb.draw_rect(x + 2, y + 6, 4, 1, c);
                fb.draw_rect(x + 2, y + 8, 4, 1, c);
            }
            2 => {
                // Yapıştır (Pano ve belge simgesi)
                let c = color;
                // Pano tablası
                fb.draw_rect(x + 1, y + 2, 10, 10, Color::ACCENT_YELLOW);
                // Pano klipsi
                fb.draw_rect(x + 4, y, 4, 3, Color::TEXT_MAIN);
                // Üstündeki kağıt
                fb.draw_rect(x + 3, y + 4, 6, 7, Color::WHITE);
                fb.draw_rect(x + 4, y + 6, 4, 1, c);
                fb.draw_rect(x + 4, y + 8, 3, 1, c);
            }
            3 => {
                // Sil (Çöp kutusu / silme çarpısı)
                let c = color;
                fb.draw_rect(x + 2, y + 2, 8, 2, c);
                fb.draw_rect(x + 4, y, 4, 2, c);
                fb.draw_rect(x + 3, y + 4, 6, 7, c);
                fb.draw_rect(x + 4, y + 5, 1, 5, Color::WINDOW_BG);
                fb.draw_rect(x + 6, y + 5, 1, 5, Color::WINDOW_BG);
            }
            4 => {
                // Tümünü Seç (Kesikli seçim çerçevesi ve dolu alan)
                let c = color;
                fb.draw_rect(x, y + 1, 11, 10, Color::DARK_BG);
                fb.draw_rect(x + 2, y + 3, 7, 6, c);
                // Köşe tutamaçları
                fb.draw_rect(x, y + 1, 2, 2, Color::WHITE);
                fb.draw_rect(x + 9, y + 1, 2, 2, Color::WHITE);
                fb.draw_rect(x, y + 9, 2, 2, Color::WHITE);
                fb.draw_rect(x + 9, y + 9, 2, 2, Color::WHITE);
            }
            _ => {
                // Tümünü Temizle (Boş sayfa veya süpürge)
                let c = color;
                fb.draw_rect_outline(x + 1, y + 1, 9, 10, c);
                fb.draw_rect(x + 3, y + 3, 5, 1, Color::ACCENT_RED);
                fb.draw_rect(x + 3, y + 5, 5, 1, Color::ACCENT_RED);
                fb.draw_rect(x + 3, y + 7, 5, 1, Color::ACCENT_RED);
            }
        }
    }

    /// Masaüstü sağ tık bağlam menüsü için 12x12 piksel simgeler çizer.
    fn draw_desktop_context_icon(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        index: usize,
        color: u32,
    ) {
        match index {
            0 => {
                // Dosya Yöneticisi: Klasör simgesi
                fb.draw_rect(x + 1, y + 2, 4, 2, Color::ACCENT_YELLOW);
                fb.draw_rounded_rect(x, y + 3, 12, 8, 2, Color::ACCENT_BLUE);
                fb.draw_rect(x + 2, y + 5, 8, 4, Color::WHITE);
            }
            1 => {
                // Yeni ▶ (Klasör/dosya simgesi ve artı)
                fb.draw_rounded_rect(x, y + 2, 11, 8, 2, Color::ACCENT_YELLOW);
                fb.draw_rect(x + 4, y + 4, 3, 1, Color::WINDOW_BG);
                fb.draw_rect(x + 5, y + 3, 1, 3, Color::WINDOW_BG);
            }
            2 => {
                // Yapıştır (Pano tablası ve kağıt)
                fb.draw_rect(x + 1, y + 2, 10, 10, Color::ACCENT_YELLOW);
                fb.draw_rect(x + 4, y, 4, 3, Color::TEXT_MAIN);
                fb.draw_rect(x + 3, y + 4, 6, 7, Color::WHITE);
            }
            3 => {
                // Sistem Ayarları: Dişli simgesi
                fb.draw_rect(x + 4, y, 4, 12, Color::ACCENT_PINK);
                fb.draw_rect(x, y + 4, 12, 4, Color::ACCENT_PINK);
                fb.draw_rect(x + 2, y + 2, 8, 8, Color::ACCENT_PINK);
                fb.draw_rect(x + 4, y + 4, 4, 4, Color::WINDOW_BG);
            }
            4 => {
                // Görev Yöneticisi: Aktivite / Nabız grafiği
                fb.draw_rect_outline(x, y + 1, 12, 10, Color::BORDER);
                let pulse_col = Color::ACCENT_GREEN;
                fb.draw_rect(x + 1, y + 6, 2, 1, pulse_col);
                fb.draw_rect(x + 3, y + 4, 2, 4, pulse_col);
                fb.draw_rect(x + 5, y + 2, 2, 7, pulse_col);
                fb.draw_rect(x + 7, y + 7, 2, 3, pulse_col);
                fb.draw_rect(x + 9, y + 5, 2, 2, pulse_col);
            }
            5 => {
                // Tema Değiştir: Palet / Boya fırçası
                fb.draw_rounded_rect(x, y + 1, 11, 10, 3, Color::ACCENT_GREEN);
                fb.draw_rect(x + 2, y + 3, 2, 2, Color::ACCENT_RED);
                fb.draw_rect(x + 6, y + 3, 2, 2, Color::ACCENT_BLUE);
                fb.draw_rect(x + 3, y + 7, 2, 2, Color::ACCENT_YELLOW);
                fb.draw_rect(x + 7, y + 7, 2, 2, Color::WINDOW_BG);
            }
            6 => {
                // Masaüstünü Yenile: Dairesel yenileme okları (Refresh)
                let c = color;
                fb.draw_rect(x + 3, y + 1, 6, 1, c);
                fb.draw_rect(x + 1, y + 3, 1, 4, c);
                fb.draw_rect(x + 3, y + 9, 6, 1, c);
                fb.draw_rect(x + 10, y + 4, 1, 4, c);
                fb.draw_rect(x + 8, y, 2, 3, c);
                fb.draw_rect(x + 2, y + 8, 2, 3, c);
            }
            _ => {
                // Yeni Not / Not Defteri
                fb.draw_rounded_rect(x + 1, y + 1, 9, 10, 2, Color::TEXT_MAIN);
                fb.draw_rect(x + 3, y + 3, 5, 1, Color::ACCENT_BLUE);
                fb.draw_rect(x + 3, y + 5, 5, 1, Color::ACCENT_BLUE);
                fb.draw_rect(x + 8, y + 8, 3, 3, Color::ACCENT_YELLOW);
            }
        }
    }

    /// Bağlam menüsünde tıklanan öğe indeksini döndürür (menü dışındaysa None).
    pub fn context_hit(&self, fb_w: usize, fb_h: usize, mx: usize, my: usize) -> Option<usize> {
        let (menu_x, menu_y, menu_w, menu_h) = self.context_menu_geom(fb_w, fb_h);
        if !in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h)) {
            return None;
        }
        let hdr_h = context_header_h();
        if my < menu_y + hdr_h {
            // Başlık satırı — öğe yok
            return None;
        }
        let items = self.context_items();
        let rel = my - (menu_y + hdr_h);
        let idx = rel / context_item_h();
        if idx < items.len() {
            Some(idx)
        } else {
            None
        }
    }
}
