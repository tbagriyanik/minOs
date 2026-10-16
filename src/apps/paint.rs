use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

/// Tuval üzerinde tutulabilecek en fazla nokta.
const MAX_POINTS: usize = 4096;
/// Fırça karesinin kenar uzunluğu (px) — kalemin merkezi bu kareye ortalanır.
const MIN_BRUSH_SIZE: usize = 1;
const MAX_BRUSH_SIZE: usize = 24;
/// Silginin yarıçapı (px).
const ERASER_RADIUS: usize = 14;
/// Araç çubuğu yüksekliği (px). Pencere içi tuvalin üst boşluğu buradan gelir.
pub const TOOLBAR_H: usize = 82;
pub const STATUS_H: usize = 24;
/// Tuval renkleri tema jetonu değildir; seçilen RGB değerleri çizimde ve BMP'de sabit kalır.
pub const PALETTE: [u32; 4] = [0x0078D7, 0xE53935, 0x2E8B57, 0x000000];
const EMPTY_POINT: PaintPoint = PaintPoint { x: 0, y: 0, color: PALETTE[0] };

const BMP_W: usize = 800;
const BMP_H: usize = 600;

#[derive(Clone, Copy)]
pub struct PaintPoint {
    pub x: usize,
    pub y: usize,
    pub color: u32,
}

pub struct PaintApp {
    pub len: usize,
    pub points: alloc::boxed::Box<[PaintPoint]>,
    pub color: u32,
    pub eraser: bool,
    pub brush_size: usize,
    pub zoom: usize,
    pub last: Option<(usize, usize)>,
    pub status_msg: &'static str,
    /// Son üretilen BMP; VFS'nin RAM kopyası olmadan kaydedilip tekrar açılır.
    pub saved_bmp: alloc::sync::Arc<[u8]>,
    /// "Dosya" menüsü açık mı?
    pub file_menu_open: bool,
    /// Dosya adı ile kaydedilirken kullanılan basit ad (ör. "cizim.bmp").
    pub file_name: [u8; 32],
    pub file_name_len: usize,
}

impl PaintApp {
    pub fn new() -> Self {
        Self {
            len: 0,
            points: alloc::vec![EMPTY_POINT; MAX_POINTS].into_boxed_slice(),
            color: PALETTE[0],
            eraser: false,
            brush_size: 7,
            zoom: 100,
            last: None,
            status_msg: "",
            saved_bmp: alloc::sync::Arc::from([]),
            file_menu_open: false,
            file_name: [0u8; 32],
            file_name_len: 0,
        }
    }

    /// Tuvalin başlangıç X koordinatı (pencere içi).
    pub fn canvas_x() -> usize { 8 }
    /// Tuvalin başlangıç Y koordinatı (pencere içi, araç çubuğunun altı).
    pub fn canvas_y() -> usize { TOOLBAR_H }

    /// Tuval alanının (x, y, w, h) pencere içi ölçülerini hesaplar.
    pub fn canvas_rect(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let x = Self::canvas_x();
        let y = Self::canvas_y();
        let cw = w.saturating_sub(16);
        let ch = h.saturating_sub(TOOLBAR_H + STATUS_H + 6);
        (x, y, cw, ch)
    }

    pub fn status_text(&self) -> &str {
        if !self.status_msg.is_empty() {
            self.status_msg
        } else if self.eraser {
            crate::gui::ui_text("Silgi modu aktif", "Eraser active")
        } else {
            crate::gui::ui_text("Çizim Tahtası Hazır", "Paint Ready")
        }
    }

    /// Pencere içi (px, py) koordinatını tuval koordinatına çevirir.
    /// Tuval dışındaysa `None` döner.
    pub fn to_canvas(&self, w: usize, h: usize, px: usize, py: usize) -> Option<(usize, usize)> {
        let (cx, cy, cw, ch) = Self::canvas_rect(w, h);
        if cw == 0 || ch == 0 || px < cx || py < cy || px >= cx + cw || py >= cy + ch {
            return None;
        }
        let zoom = self.zoom.max(1);
        let logical_x = (((px - cx) as isize - (cw / 2) as isize) * 100 / zoom as isize
            + (cw / 2) as isize).max(0) as usize;
        let logical_y = (((py - cy) as isize - (ch / 2) as isize) * 100 / zoom as isize
            + (ch / 2) as isize).max(0) as usize;
        if logical_x >= cw || logical_y >= ch { return None; }
        Some((logical_x, logical_y))
    }

    /// Verilen tuval noktasına fırça uygular. Windows Paint gibi: hızlı fare
    /// hareketlerinde iki nokta arasını enterpolasyonla doldurarak sürekli ve
    /// akıcı bir çizgi üretir (noktalı/kesik çizgi oluşmaz).
    pub fn paint(&mut self, x: usize, y: usize) {
        if self.eraser {
            self.erase(x, y);
            self.last = Some((x, y));
            return;
        }

        let (sx, sy) = self.last.unwrap_or((x, y));
        let dx = x as isize - sx as isize;
        let dy = y as isize - sy as isize;
        let steps = dx.unsigned_abs().max(dy.unsigned_abs()).max(1);
        for i in 1..=steps {
            if self.len >= MAX_POINTS {
                break;
            }
            let px = (sx as isize + dx * i as isize / steps as isize).max(0) as usize;
            let py = (sy as isize + dy * i as isize / steps as isize).max(0) as usize;
            self.points[self.len] = PaintPoint {
                x: px,
                y: py,
                color: crate::gui::theme::resolve(self.color),
            };
            self.len += 1;
        }
        self.last = Some((x, y));
    }

    /// Silgi: merkez çevresindeki noktaları dairesel olarak kaldırır.
    pub fn erase(&mut self, x: usize, y: usize) {
        let mut out = 0;
        for i in 0..self.len {
            let point = self.points[i];
            let ddx = point.x.abs_diff(x) as isize;
            let ddy = point.y.abs_diff(y) as isize;
            let remove = ddx * ddx + ddy * ddy <= (ERASER_RADIUS * ERASER_RADIUS) as isize;
            if !remove {
                self.points[out] = point;
                out += 1;
            }
        }
        self.len = out;
    }

    pub fn end_stroke(&mut self) {
        self.last = None;
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.last = None;
    }

    /// Belirtilen dosya adına BMP kaydeder (ör. "/root/Documents/deneme.bmp").
    pub fn save_bmp_named(&mut self, vfs: &mut crate::fs::vfs::VirtualFileSystem, canvas_w: usize, canvas_h: usize, path: &str) -> bool {
        let w = canvas_w.min(BMP_W).max(1); let h = canvas_h.min(BMP_H).max(1);
        let row_bytes = (w * 3 + 3) & !3;
        let image_size = row_bytes * h;
        let file_size = 54 + image_size;
        let mut header = [0u8; 54];
        header[0..2].copy_from_slice(b"BM");
        header[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
        header[10..14].copy_from_slice(&(54u32).to_le_bytes());
        header[14..18].copy_from_slice(&(40u32).to_le_bytes());
        header[18..22].copy_from_slice(&(w as i32).to_le_bytes());
        header[22..26].copy_from_slice(&(h as i32).to_le_bytes());
        header[26..28].copy_from_slice(&(1u16).to_le_bytes());
        header[28..30].copy_from_slice(&(24u16).to_le_bytes());
        header[34..38].copy_from_slice(&(image_size as u32).to_le_bytes());
        // Build the BMP in one buffer. Keeping separate `image` and `bmp`
        // vectors temporarily needs nearly 3 MiB, larger than the kernel heap.
        let mut bmp = alloc::vec::Vec::new();
        if bmp.try_reserve_exact(file_size).is_err() {
            self.status_msg = crate::gui::ui_text("Bellek yetersiz", "Not enough memory");
            return false;
        }
        bmp.resize(file_size, 0xFF);
        bmp[..54].copy_from_slice(&header);
        for i in 0..self.len {
            let p = self.points[i]; if p.x >= w || p.y >= h { continue; }
            let base = 54 + (h - 1 - p.y) * row_bytes + p.x * 3;
            bmp[base] = (p.color & 0xFF) as u8;
            bmp[base + 1] = ((p.color >> 8) & 0xFF) as u8;
            bmp[base + 2] = ((p.color >> 16) & 0xFF) as u8;
        }
        // Retain the bytes in the Paint instance and let VFS index them without
        // cloning a ~1.4 MiB file into the small kernel heap.
        let saved_bmp: alloc::sync::Arc<[u8]> = bmp.into();
        let ok = vfs.write_large_file(path, saved_bmp.clone());
        if ok { self.saved_bmp = saved_bmp; }
        self.status_msg = if ok { crate::gui::ui_text("Çizim kaydedildi", "Drawing saved") } else { crate::gui::ui_text("Kaydetme başarısız", "Save failed") };
        ok
    }

    /// Varsayılan dosya yolu ("Yeni" ve "Kaydet" bu adı kullanır).
    pub const DEFAULT_PATH: &'static str = "/root/Documents/cizim.bmp";

    pub fn save_bmp(&mut self, vfs: &mut crate::fs::vfs::VirtualFileSystem, canvas_w: usize, canvas_h: usize) -> bool {
        self.save_bmp_named(vfs, canvas_w, canvas_h, Self::DEFAULT_PATH)
    }

    /// "Yeni": tuvali temizler ve geçerli dosya adını sıfırlar.
    pub fn new_document(&mut self) {
        self.clear();
        self.saved_bmp = alloc::sync::Arc::from([]);
        self.file_name_len = 0;
        self.status_msg = crate::gui::ui_text("Yeni tuval", "New canvas");
    }

    /// "Farklı Kaydet": verilen ada yeni bir BMP yazar ve bu adı geçerli dosya yapar.
    /// Ad VFS'de zaten varsa "_2", "_3" … ekleyerek çakışmayan bir ad bulur.
    pub fn save_as(&mut self, vfs: &mut crate::fs::vfs::VirtualFileSystem, canvas_w: usize, canvas_h: usize, name: &str) -> bool {
        let final_name = Self::unique_name(vfs, name);
        let mut path = alloc::string::String::from("/root/Documents/");
        path.push_str(final_name.as_str());
        let ok = self.save_bmp_named(vfs, canvas_w, canvas_h, &path);
        if ok {
            let bytes = final_name.as_bytes();
            let n = bytes.len().min(self.file_name.len());
            self.file_name[..n].copy_from_slice(&bytes[..n]);
            self.file_name_len = n;
            self.status_msg = crate::gui::ui_text("Farklı kaydedildi", "Saved as new file");
        }
        ok
    }

    /// "Farklı Kaydet" için geçerli adı "_kopya" sonekli yeni bir ad türetir.
    /// Örn. "cizim.bmp" -> "cizim_kopya.bmp".
    pub fn derive_save_as_name(current: &str) -> alloc::string::String {
        let mut out = alloc::string::String::from("cizim_kopya.bmp");
        if let Some(stem_end) = current.rfind('.') {
            if stem_end > 0 {
                out.clear();
                out.push_str(&current[..stem_end]);
                out.push_str("_kopya");
                out.push_str(&current[stem_end..]);
            }
        }
        out
    }

    /// VFS'de adı çakışmayan bir dosya adı üretir ("a.bmp" -> "a_2.bmp", "a_3.bmp" …).
    fn unique_name(vfs: &crate::fs::vfs::VirtualFileSystem, name: &str) -> alloc::string::String {
        let mut candidate = alloc::string::String::from(name);
        if !Self::path_exists(vfs, candidate.as_str()) {
            return candidate;
        }
        let (stem, ext) = match name.rfind('.') {
            Some(i) if i > 0 => (&name[..i], &name[i..]),
            _ => (name, ""),
        };
        let mut n = 2u32;
        loop {
            candidate.clear();
            candidate.push_str(stem);
            candidate.push('_');
            let mut num = [0u8; 10];
            let mut len = 0;
            let mut v = n;
            if v == 0 { num[0] = b'0'; len = 1; }
            while v > 0 { num[len] = b'0' + (v % 10) as u8; v /= 10; len += 1; }
            for i in 0..len { candidate.push(num[len - 1 - i] as char); }
            candidate.push_str(ext);
            if !Self::path_exists(vfs, candidate.as_str()) { return candidate; }
            n += 1;
        }
    }

    fn path_exists(vfs: &crate::fs::vfs::VirtualFileSystem, name: &str) -> bool {
        let mut path = alloc::string::String::from("/root/Documents/");
        path.push_str(name);
        vfs.exists(path.as_str())
    }

    /// Kaydedilen dosyanın basit adı (yoksa "cizim.bmp").
    pub fn current_name(&self) -> &str {
        if self.file_name_len == 0 {
            "cizim.bmp"
        } else {
            core::str::from_utf8(&self.file_name[..self.file_name_len]).unwrap_or("cizim.bmp")
        }
    }

    pub fn open_bmp(&mut self, vfs: &crate::fs::vfs::VirtualFileSystem) -> bool {
        let stored;
        let data: &[u8] = if !self.saved_bmp.is_empty() {
            &self.saved_bmp
        } else {
            stored = match vfs.read_large_file(Self::DEFAULT_PATH) {
                Some(d) => d,
                None => {
                    match vfs.read_file(Self::DEFAULT_PATH) {
                        Some(_) => { self.status_msg = crate::gui::ui_text("BMP açılamadı", "Could not open BMP"); return false; }
                        None => { self.status_msg = crate::gui::ui_text("BMP bulunamadı", "BMP not found"); return false; }
                    }
                }
            };
            &stored
        };
        if data.len() < 54 || &data[0..2] != b"BM" || u16::from_le_bytes([data[28], data[29]]) != 24 { self.status_msg = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP"); return false; }
        let w = (i32::from_le_bytes([data[18],data[19],data[20],data[21]])).unsigned_abs() as usize;
        let h = (i32::from_le_bytes([data[22],data[23],data[24],data[25]])).unsigned_abs() as usize;
        let top_down = i32::from_le_bytes([data[22],data[23],data[24],data[25]]) < 0;
        let offset = u32::from_le_bytes([data[10],data[11],data[12],data[13]]) as usize;
        let row_bytes = (w.checked_mul(3).and_then(|n| n.checked_add(3)).unwrap_or(usize::MAX)) & !3;
        if offset < 54 || row_bytes == usize::MAX || h > (data.len().saturating_sub(offset) / row_bytes.max(1)) + 1 {
            self.status_msg = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP"); return false;
        }
        self.len = 0;
        for y in 0..h.min(BMP_H) { for x in 0..w.min(BMP_W) {
            let sy = if top_down { y } else { h - 1 - y }; let i = offset + sy * row_bytes + x * 3;
            if i + 2 >= data.len() { continue; }
            let color = (data[i + 2] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i] as u32;
            if color != 0xFFFFFF && self.len < MAX_POINTS { self.points[self.len] = PaintPoint { x, y, color }; self.len += 1; }
        }
        }
        self.last = None;
        self.status_msg = crate::gui::ui_text("BMP açıldı", "BMP opened");
        true
    }

    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        // 1. Tuval Alanı (Canvas)
        let (cx, cy, cw, ch) = Self::canvas_rect(w, h);
        let canvas_x = x + cx;
        let canvas_y = y + cy;
        let canvas_w = cw;
        let canvas_h = ch;
        fb.draw_rect(canvas_x, canvas_y, canvas_w, canvas_h, Color::WHITE);
        fb.draw_rect_outline(canvas_x, canvas_y, canvas_w, canvas_h, Color::BORDER);
        let zoom = self.zoom.max(1);
        for i in 0..self.len {
            let point = self.points[i];
            if point.x < canvas_w && point.y < canvas_h {
                let px = (canvas_x as isize + canvas_w as isize / 2
                    + (point.x as isize - canvas_w as isize / 2) * zoom as isize / 100).max(0) as usize;
                let py = (canvas_y as isize + canvas_h as isize / 2
                    + (point.y as isize - canvas_h as isize / 2) * zoom as isize / 100).max(0) as usize;
                let brush = (self.brush_size * zoom / 100).max(1);
                if px >= canvas_x && py >= canvas_y && px < canvas_x + canvas_w && py < canvas_y + canvas_h {
                    fb.draw_rect(px.saturating_sub(brush / 2), py.saturating_sub(brush / 2), brush, brush, point.color);
                }
            }
        }

        // 2. Üst Menü Çubuğu (Menu Bar: y + 28 .. y + 52)
        fb.draw_rect(x + 6, y + 28, w.saturating_sub(12), 24, Color::TITLE_BAR);
        let file_rect = Self::file_menu_rect(w);
        let file_btn_bg = if self.file_menu_open { Color::ACCENT_BLUE } else { Color::WINDOW_BG };
        let file_btn_fg = if self.file_menu_open { MinGuiCompositor::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN };
        fb.draw_rect(x + file_rect.0, y + file_rect.1, file_rect.2, file_rect.3, file_btn_bg);
        fb.draw_rect_outline(x + file_rect.0, y + file_rect.1, file_rect.2, file_rect.3, Color::BORDER);
        MinGuiCompositor::draw_string(fb, x + file_rect.0 + 8, y + file_rect.1 + 4,
            crate::gui::ui_text("Dosya", "File"), file_btn_fg);

        // Hızlı Aksiyon Butonları (Sağ Taraf)
        let clear_x = x + w.saturating_sub(68);
        fb.draw_rect(clear_x, y + 29, 60, 22, Color::WINDOW_BG);
        fb.draw_rect_outline(clear_x, y + 29, 60, 22, Color::BORDER);
        MinGuiCompositor::draw_string(fb, clear_x + 6, y + 33, crate::gui::ui_text("Temizle", "Clear"), Color::ACCENT_RED);

        let save_x = x + w.saturating_sub(134);
        fb.draw_rect(save_x, y + 29, 62, 22, Color::ACCENT_BLUE);
        fb.draw_rect_outline(save_x, y + 29, 62, 22, Color::BORDER);
        MinGuiCompositor::draw_string(fb, save_x + 8, y + 33, crate::gui::ui_text("Kaydet", "Save"), MinGuiCompositor::text_on(Color::ACCENT_BLUE));

        let open_x = x + w.saturating_sub(196);
        fb.draw_rect(open_x, y + 29, 58, 22, Color::WINDOW_BG);
        fb.draw_rect_outline(open_x, y + 29, 58, 22, Color::BORDER);
        MinGuiCompositor::draw_string(fb, open_x + 10, y + 33, crate::gui::ui_text("Aç", "Open"), Color::TEXT_MAIN);

        // 3. İkinci Satır: Araç ve Renk Çubuğu (Tools & Palette Strip: y + 54 .. y + 78)
        fb.draw_rect(x + 6, y + 54, w.saturating_sub(12), 26, MinGuiCompositor::shade(Color::TITLE_BAR, -10));
        fb.draw_rect_outline(x + 6, y + 54, w.saturating_sub(12), 26, Color::BORDER);

        MinGuiCompositor::draw_string(fb, x + 12, y + 60, crate::gui::ui_text("Kalem:", "Pen:"), Color::TEXT_MAIN);

        // 4 Renk Kutusu (Palette Swatches)
        for (i, color) in PALETTE.iter().enumerate() {
            let swatch_x = x + 60 + i * 22;
            fb.draw_rect(swatch_x, y + 58, 18, 18, *color);
            fb.draw_rect_outline(swatch_x, y + 58, 18, 18, Color::BORDER);
            if !self.eraser && self.color == *color {
                fb.draw_rect_outline(swatch_x.saturating_sub(1), y + 57, 20, 20, Color::WHITE);
            }
        }

        // Silgi Butonu
        let eraser_x = x + 154;
        let eraser_bg = if self.eraser { Color::ACCENT_BLUE } else { Color::WINDOW_BG };
        let eraser_fg = if self.eraser { MinGuiCompositor::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN };
        fb.draw_rect(eraser_x, y + 56, 56, 22, eraser_bg);
        fb.draw_rect_outline(eraser_x, y + 56, 56, 22, Color::BORDER);
        MinGuiCompositor::draw_string(fb, eraser_x + 8, y + 60, crate::gui::ui_text("Silgi", "Eraser"), eraser_fg);

        // Fırça Boyutu: "Fırça: {}px [-] [+]"
        let brush_label_x = x + 218;
        MinGuiCompositor::draw_string(fb, brush_label_x, y + 60, crate::gui::ui_text("Fırça:", "Brush:"), Color::TEXT_MAIN);
        let mut b_buf = [0u8; 8];
        let b_str = format_num(self.brush_size, &mut b_buf);
        MinGuiCompositor::draw_string(fb, brush_label_x + 44, y + 60, b_str, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, brush_label_x + 60, y + 60, "px", Color::TEXT_MUTED);

        let b_minus_x = brush_label_x + 80;
        fb.draw_rect(b_minus_x, y + 56, 20, 22, Color::WINDOW_BG);
        fb.draw_rect_outline(b_minus_x, y + 56, 20, 22, Color::BORDER);
        MinGuiCompositor::draw_string(fb, b_minus_x + 6, y + 60, "-", Color::TEXT_MAIN);

        let b_plus_x = b_minus_x + 24;
        fb.draw_rect(b_plus_x, y + 56, 20, 22, Color::WINDOW_BG);
        fb.draw_rect_outline(b_plus_x, y + 56, 20, 22, Color::BORDER);
        MinGuiCompositor::draw_string(fb, b_plus_x + 5, y + 60, "+", Color::TEXT_MAIN);

        // Yakınlaştırma (Zoom): "%100 [-] [+]" (Sağ Taraf)
        let z_plus_x = x + w.saturating_sub(32);
        fb.draw_rect(z_plus_x, y + 56, 22, 22, Color::WINDOW_BG);
        fb.draw_rect_outline(z_plus_x, y + 56, 22, 22, Color::BORDER);
        MinGuiCompositor::draw_string(fb, z_plus_x + 6, y + 60, "+", Color::TEXT_MAIN);

        let z_minus_x = z_plus_x.saturating_sub(26);
        fb.draw_rect(z_minus_x, y + 56, 22, 22, Color::WINDOW_BG);
        fb.draw_rect_outline(z_minus_x, y + 56, 22, 22, Color::BORDER);
        MinGuiCompositor::draw_string(fb, z_minus_x + 7, y + 60, "-", Color::TEXT_MAIN);

        let mut z_buf = [0u8; 8];
        let z_str = format_num(self.zoom, &mut z_buf);
        let z_label_x = z_minus_x.saturating_sub(46);
        MinGuiCompositor::draw_string(fb, z_label_x, y + 60, "%", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, z_label_x + 10, y + 60, z_str, Color::TEXT_MAIN);

        // 4. Alt Durum Çubuğu (Status Bar: y + h - 24 .. y + h - 2)
        let status_y = y + h.saturating_sub(24);
        let status_w = w.saturating_sub(12);
        fb.draw_rect(x + 6, status_y, status_w, 22, Color::TASKBAR_BG);
        fb.draw_line(x + 6, status_y, x + 6 + status_w, status_y, Color::BORDER);

        // Sol: Durum Mesajı
        MinGuiCompositor::draw_string_clipped(
            fb,
            x + 12,
            status_y + 4,
            self.status_text(),
            if self.eraser { Color::ACCENT_BLUE } else { Color::ACCENT_GREEN },
            (x + 10, status_y + 2, status_w / 3, 18),
        );

        // Orta: Dosya Adı
        let doc_name = self.current_name();
        let doc_w = MinGuiCompositor::text_width(doc_name, 1);
        let doc_x = x + (w.saturating_sub(doc_w)) / 2;
        MinGuiCompositor::draw_string(fb, doc_x, status_y + 4, doc_name, Color::TEXT_MAIN);

        // Sağ: Bilgi Paneli (Nokta Sayısı & Fırça & Zoom)
        // `alloc::format!` her karede heap tahsisi yapar; çekirdek heap'inde
        // pahalı ve hataya açıktır. Sabit boyutlu `TextBuf` kullanılır.
        let mut right_info = crate::gui::TextBuf::new();
        right_info.push_str(crate::gui::ui_text("Nokta:", "Pts:"));
        right_info.push_byte(b' ');
        right_info.push_usize(self.len);
        right_info.push_byte(b'/');
        right_info.push_usize(MAX_POINTS);
        right_info.push_str(" | ");
        right_info.push_str(crate::gui::ui_text("Fırça:", "Brush:"));
        right_info.push_byte(b' ');
        right_info.push_usize(self.brush_size);
        right_info.push_str("px | %");
        right_info.push_usize(self.zoom);
        let right_info = right_info.as_str();
        let right_w = MinGuiCompositor::text_width(right_info, 1);
        let right_x = x + w.saturating_sub(right_w + 16);
        MinGuiCompositor::draw_string(fb, right_x, status_y + 4, right_info, Color::TEXT_MUTED);

        // 5. Açılır Menü (File Dropdown: En Üst Katman)
        if self.file_menu_open {
            let (mx, my, mw, mh) = Self::file_dropdown_rect(w);
            let drop_x = x + mx;
            let drop_y = y + my;
            fb.draw_shadow(drop_x, drop_y, mw, mh, Color::SHADOW);
            fb.draw_rect(drop_x, drop_y, mw, mh, Color::WINDOW_BG);
            fb.draw_rect_outline(drop_x, drop_y, mw, mh, Color::BORDER);

            let labels = Self::file_menu_labels();
            for (i, label) in labels.iter().enumerate() {
                let r = Self::file_menu_item_rect(w, i);
                let item_y = y + r.1;
                let item_x = x + r.0;
                fb.draw_rect(item_x, item_y, r.2, r.3, Color::TITLE_BAR_INACTIVE);
                MinGuiCompositor::draw_string(fb, item_x + 8, item_y + 4, label, Color::TEXT_MAIN);
                if i + 1 < Self::FILE_MENU_COUNT {
                    fb.draw_line(item_x + 2, item_y + r.3 + 1, item_x + r.2 - 2, item_y + r.3 + 1, Color::BORDER);
                }
            }
        }
    }

    /// "Dosya" menüsü düğmesinin pencere içi (x, y, w, h) ölçüleri.
    /// `render_content` içindeki menü çubuğu (y + 28) ile birebir aynıdır.
    pub fn file_menu_rect(_w: usize) -> (usize, usize, usize, usize) {
        (8, 28, 56, 22)
    }

    /// Hızlı aksiyon düğmeleri — `render_content` ile aynı koordinatlar.
    /// Çizim ve tıklama testi bu fonksiyonları kullanır.
    pub fn open_btn_rect(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(196), 29, 58, 22)
    }
    pub fn save_btn_rect(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(134), 29, 62, 22)
    }
    pub fn clear_btn_rect(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(68), 29, 60, 22)
    }
    /// Renk kutusu (palet) dikdörtgeni — `render_content` ile aynı.
    pub fn swatch_rect(index: usize) -> (usize, usize, usize, usize) {
        (60 + index * 22, 58, 18, 18)
    }
    /// Silgi düğmesi.
    pub fn eraser_rect() -> (usize, usize, usize, usize) {
        (154, 56, 56, 22)
    }
    /// Fırça küçült/büyüt düğmeleri.
    pub fn brush_label_x() -> usize { 218 }
    pub fn brush_minus_rect() -> (usize, usize, usize, usize) {
        (Self::brush_label_x() + 80, 56, 20, 22)
    }
    pub fn brush_plus_rect() -> (usize, usize, usize, usize) {
        (Self::brush_label_x() + 104, 56, 20, 22)
    }
    /// Zoom küçült/büyüt düğmeleri (pencere genişliğine göre sağa dayalı).
    pub fn zoom_plus_rect(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(32), 56, 22, 22)
    }
    pub fn zoom_minus_rect(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(58), 56, 22, 22)
    }

    /// Açılır menü listesinin pencere içi (x, y, w, h) ölçüleri.
    pub fn file_dropdown_rect(_w: usize) -> (usize, usize, usize, usize) {
        (8, 52, 170, 96)
    }

    /// Menüdeki öğe sayısı.
    pub const FILE_MENU_COUNT: usize = 4;

    /// Menü öğelerinin etiketleri (iki dilli).
    pub fn file_menu_labels() -> [&'static str; 4] {
        [
            crate::gui::ui_text("Yeni (Ctrl+N)", "New (Ctrl+N)"),
            crate::gui::ui_text("Aç... (Ctrl+O)", "Open... (Ctrl+O)"),
            crate::gui::ui_text("Kaydet (Ctrl+S)", "Save (Ctrl+S)"),
            crate::gui::ui_text("Farklı Kaydet...", "Save As..."),
        ]
    }

    /// Açılır menüdeki öğe (x, y, w, h) koordinatlarını (pencere içi) döndürür.
    pub fn file_menu_item_rect(_w: usize, index: usize) -> (usize, usize, usize, usize) {
        let (mx, my, mw, _mh) = Self::file_dropdown_rect(0);
        (mx + 4, my + 4 + index * 22, mw.saturating_sub(8), 21)
    }

        pub fn adjust_brush(&mut self, delta: i8) {
        if delta > 0 {
            self.brush_size = (self.brush_size + 1).min(MAX_BRUSH_SIZE);
        } else if delta < 0 {
            self.brush_size = self.brush_size.saturating_sub(1).max(MIN_BRUSH_SIZE);
        }
    }

    pub fn adjust_zoom(&mut self, delta: i8) {
        if delta > 0 {
            self.zoom = (self.zoom + 10).min(300);
        } else if delta < 0 {
            self.zoom = self.zoom.saturating_sub(10).max(50);
        }
    }
}

fn format_num<'a>(mut n: usize, buf: &'a mut [u8; 8]) -> &'a str {
    if n == 0 {
        return "0";
    }
    let mut i = 8;
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("0")
}
