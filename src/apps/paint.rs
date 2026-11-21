use crate::drivers::video::Framebuffer;
use crate::gui::{
    scaled_statusbar_h, scaled_titlebar_h, scaled_toolbar_h, sp, Color, MinGuiCompositor,
};

/// Temel ızgara birimini aktif UI ölçeğine göre büyütür.
#[inline(always)]
fn sc(base: usize) -> usize {
    sp(base)
}

/// Tuval üzerinde tutulabilecek en fazla nokta.
const MAX_POINTS: usize = 4096;
/// Fırça karesinin kenar uzunluğu (px) — kalemin merkezi bu kareye ortalanır.
const MIN_BRUSH_SIZE: usize = 1;
const MAX_BRUSH_SIZE: usize = 24;
/// Silginin yarıçapı (px).
const ERASER_RADIUS: usize = 14;
/// Araç çubuğu yüksekliği (px). Pencere içi tuvalin üst boşluğu buradan gelir.
pub const TOOLBAR_H: usize = 0; // Use scaled_toolbar_h() instead
pub const STATUS_H: usize = 0; // Use scaled_statusbar_h() instead

pub fn toolbar_h() -> usize {
    scaled_toolbar_h()
}

pub fn status_h() -> usize {
    scaled_statusbar_h()
}
/// Tuval renkleri tema jetonu değildir; seçilen RGB değerleri çizimde ve BMP'de sabit kalır.
pub const PALETTE: [u32; 4] = [0x0078D7, 0xE53935, 0x2E8B57, 0x000000];
/// Renk seçicide sunulan geniş palet (6 x 4).
pub const PICKER_COLORS: [u32; 24] = [
    0x000000, 0x404040, 0x808080, 0xC0C0C0, 0xFFFFFF, 0x7F0000, 0xFF0000, 0xFF7F00, 0xFFD400,
    0xFFFF00, 0xBFFF00, 0x00FF00, 0x00FF7F, 0x00FFFF, 0x007FFF, 0x0000FF, 0x7F00FF, 0xFF00FF,
    0xFF007F, 0x8B4513, 0xD2691E, 0xF5DEB3, 0x2E8B57, 0x0078D7,
];
const EMPTY_POINT: PaintPoint = PaintPoint {
    x: 0,
    y: 0,
    color: PALETTE[0],
};
/// Geri al/ileri al için saklanan en fazla anlık görüntü sayısı.
pub const HISTORY_LIMIT: usize = 20;

const BMP_W: usize = 800;
const BMP_H: usize = 600;

/// Etkin çizim aracı.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PaintTool {
    Brush,
    Eraser,
    Line,
    Rect,
    Circle,
}

impl PaintTool {
    pub fn name(self) -> &'static str {
        match self {
            PaintTool::Brush => crate::gui::ui_text("Kalem", "Brush"),
            PaintTool::Eraser => crate::gui::ui_text("Silgi", "Eraser"),
            PaintTool::Line => crate::gui::ui_text("Çizgi", "Line"),
            PaintTool::Rect => crate::gui::ui_text("Dikdörtgen", "Rect"),
            PaintTool::Circle => crate::gui::ui_text("Daire", "Circle"),
        }
    }
}

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
    pub tool: PaintTool,
    pub brush_size: usize,
    pub zoom: usize,
    pub last: Option<(usize, usize)>,
    /// Şekil araçlarında sürüklemenin başlangıç ve geçerli noktası.
    pub shape_start: Option<(usize, usize)>,
    pub shape_current: Option<(usize, usize)>,
    /// Sağ tuşla silme darbesinin son noktası (sol tuş `last`'ten ayrıdır).
    pub erase_last: Option<(usize, usize)>,
    /// Geri al / ileri al yığınları (tuval anlık görüntüleri).
    pub undo_stack: alloc::vec::Vec<alloc::vec::Vec<PaintPoint>>,
    pub redo_stack: alloc::vec::Vec<alloc::vec::Vec<PaintPoint>>,
    /// Geniş renk seçici açık mı?
    pub picker_open: bool,
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
    /// Canvas points, undo/redo snapshots and the last saved BMP.
    pub fn memory_usage_bytes(&self) -> usize {
        let point_bytes = core::mem::size_of::<PaintPoint>();
        let canvas = self.points.len().saturating_mul(point_bytes);
        let history = |stack: &alloc::vec::Vec<alloc::vec::Vec<PaintPoint>>| {
            crate::apps::memory::vec_bytes(stack)
                .saturating_add(stack.iter().map(crate::apps::memory::vec_bytes).sum())
        };
        canvas
            .saturating_add(history(&self.undo_stack))
            .saturating_add(history(&self.redo_stack))
            .saturating_add(self.saved_bmp.len())
    }

    pub fn new() -> Self {
        Self {
            len: 0,
            points: alloc::vec![EMPTY_POINT; MAX_POINTS].into_boxed_slice(),
            color: PALETTE[0],
            tool: PaintTool::Brush,
            brush_size: 7,
            zoom: 100,
            last: None,
            shape_start: None,
            shape_current: None,
            erase_last: None,
            undo_stack: alloc::vec::Vec::new(),
            redo_stack: alloc::vec::Vec::new(),
            picker_open: false,
            status_msg: "",
            saved_bmp: alloc::sync::Arc::from([]),
            file_menu_open: false,
            file_name: [0u8; 32],
            file_name_len: 0,
        }
    }

    fn snapshot(&self) -> alloc::vec::Vec<PaintPoint> {
        let mut copy = alloc::vec::Vec::with_capacity(self.len);
        copy.extend_from_slice(&self.points[..self.len]);
        copy
    }

    /// Değişiklik öncesi tuvali geri al yığınına kaydeder.
    fn push_undo(&mut self) {
        if self.undo_stack.len() >= HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        let snapshot = self.snapshot();
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
    }

    fn restore(&mut self, points: alloc::vec::Vec<PaintPoint>) {
        let n = points.len().min(self.points.len());
        self.points[..n].copy_from_slice(&points[..n]);
        self.len = n;
        self.last = None;
        self.shape_start = None;
        self.shape_current = None;
        self.erase_last = None;
    }

    /// Son değişikliği geri alır.
    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        if self.redo_stack.len() >= HISTORY_LIMIT {
            self.redo_stack.remove(0);
        }
        self.redo_stack.push(current);
        self.restore(previous);
        self.status_msg = crate::gui::ui_text("Geri alındı", "Undone");
        true
    }

    /// Geri alınan değişikliği yeniden uygular.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = self.snapshot();
        if self.undo_stack.len() >= HISTORY_LIMIT {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(current);
        self.restore(next);
        self.status_msg = crate::gui::ui_text("Yinelendi", "Redone");
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    // ------------------------------------------------------------------
    // Yerleşim
    //
    // Pencere içi koordinat sistemi başlık çubuğunu da kapsar (y = 0 üst
    // kenar). İki şerit üstte durur: menü satırı (Dosya + dosya işlemleri +
    // fırça boyutu) ve araç şeridi (palet, silgi, şekiller, renk seçici,
    // zoom). Tuval bu şeritlerin altına, durum çubuğunun üstüne yerleşir.
    // Tüm ölçüler `sp()` ile UI ölçeğine duyarlıdır; çizim (`render_content`)
    // ve tıklama testi aynı `*_rect` yardımcılarını kullandığı için asla
    // ayrışmaz.
    // ------------------------------------------------------------------

    /// Menü satırının üst kenarı (başlık çubuğunun hemen altı).
    pub fn menu_y() -> usize {
        scaled_titlebar_h() + sc(4)
    }
    /// Menü satırı yüksekliği.
    pub fn menu_h() -> usize {
        scaled_toolbar_h()
    }
    /// Araç şeridinin üst kenarı.
    pub fn tool_y() -> usize {
        Self::menu_y() + Self::menu_h() + sc(4)
    }
    /// Araç şeridi yüksekliği.
    pub fn tool_h() -> usize {
        sc(26)
    }
    /// Şerit içi düğmelerin üst kenarı (şerit yüksekliğine göre ortalanmış).
    fn strip_btn_y(row_y: usize, row_h: usize) -> usize {
        row_y + row_h.saturating_sub(sc(20)) / 2
    }
    /// Menü satırı düğmelerinin üst kenarı.
    pub fn menu_btn_y() -> usize {
        Self::strip_btn_y(Self::menu_y(), Self::menu_h())
    }
    /// Araç şeridi düğmelerinin üst kenarı.
    pub fn tool_btn_y() -> usize {
        Self::strip_btn_y(Self::tool_y(), Self::tool_h())
    }
    /// Tuvalin üst kenarı (araç şeridinin altı).
    pub fn canvas_y() -> usize {
        Self::tool_y() + Self::tool_h() + sc(4)
    }
    /// Durum çubuğunun üst kenarı (pencere yüksekliğine göre).
    pub fn status_top(h: usize) -> usize {
        h.saturating_sub(sc(4) + scaled_statusbar_h())
    }
    /// Tuvalin alt kenarı (durum çubuğunun üstünde küçük bir boşlukla).
    fn canvas_bottom(h: usize) -> usize {
        Self::status_top(h).saturating_sub(sc(6))
    }

    /// Tuvalin başlangıç X koordinatı (pencere içi).
    pub fn canvas_x() -> usize {
        sc(8)
    }

    /// Tuval alanının (x, y, w, h) pencere içi ölçülerini hesaplar.
    pub fn canvas_rect(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let x = Self::canvas_x();
        let y = Self::canvas_y();
        let cw = w.saturating_sub(x * 2);
        let ch = Self::canvas_bottom(h).saturating_sub(y);
        (x, y, cw, ch)
    }

    pub fn status_text(&self) -> &str {
        if !self.status_msg.is_empty() {
            self.status_msg
        } else if self.tool == PaintTool::Eraser {
            crate::gui::ui_text("Silgi modu aktif", "Eraser active")
        } else if self.tool == PaintTool::Brush {
            crate::gui::ui_text("Çizim Tahtası Hazır", "Paint Ready")
        } else {
            crate::gui::ui_text(
                "Şekil aracı: sürükleyerek çizin",
                "Shape tool: drag to draw",
            )
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
            + (cw / 2) as isize)
            .max(0) as usize;
        let logical_y = (((py - cy) as isize - (ch / 2) as isize) * 100 / zoom as isize
            + (ch / 2) as isize)
            .max(0) as usize;
        if logical_x >= cw || logical_y >= ch {
            return None;
        }
        Some((logical_x, logical_y))
    }

    /// Verilen tuval noktasına fırça uygular. Windows Paint gibi: hızlı fare
    /// hareketlerinde iki nokta arasını enterpolasyonla doldurarak sürekli ve
    /// akıcı bir çizgi üretir (noktalı/kesik çizgi oluşmaz).
    pub fn paint(&mut self, x: usize, y: usize) {
        match self.tool {
            PaintTool::Brush => {
                if self.last.is_none() {
                    self.push_undo();
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
                    self.push_paint_point(px, py);
                }
                self.last = Some((x, y));
            }
            PaintTool::Eraser => {
                if self.last.is_none() {
                    self.push_undo();
                }
                let (sx, sy) = self.last.unwrap_or((x, y));
                self.erase_segment(sx, sy, x, y);
                self.last = Some((x, y));
            }
            PaintTool::Line | PaintTool::Rect | PaintTool::Circle => {
                if self.shape_start.is_none() {
                    self.push_undo();
                    self.shape_start = Some((x, y));
                }
                self.shape_current = Some((x, y));
            }
        }
    }

    fn push_paint_point(&mut self, x: usize, y: usize) {
        if self.len < MAX_POINTS {
            self.points[self.len] = PaintPoint {
                x,
                y,
                color: crate::gui::theme::resolve(self.color),
            };
            self.len += 1;
        }
    }

    /// Etkin şekil aracının noktalarını üretir.
    fn raster_shape<F: FnMut(usize, usize)>(&self, mut emit: F) {
        let (Some((x0, y0)), Some((x1, y1))) = (self.shape_start, self.shape_current) else {
            return;
        };
        match self.tool {
            PaintTool::Line => raster_line(
                x0 as isize,
                y0 as isize,
                x1 as isize,
                y1 as isize,
                &mut emit,
            ),
            PaintTool::Rect => {
                raster_line(
                    x0 as isize,
                    y0 as isize,
                    x1 as isize,
                    y0 as isize,
                    &mut emit,
                );
                raster_line(
                    x1 as isize,
                    y0 as isize,
                    x1 as isize,
                    y1 as isize,
                    &mut emit,
                );
                raster_line(
                    x1 as isize,
                    y1 as isize,
                    x0 as isize,
                    y1 as isize,
                    &mut emit,
                );
                raster_line(
                    x0 as isize,
                    y1 as isize,
                    x0 as isize,
                    y0 as isize,
                    &mut emit,
                );
            }
            PaintTool::Circle => {
                let dx = x1 as isize - x0 as isize;
                let dy = y1 as isize - y0 as isize;
                let radius = integer_sqrt((dx * dx + dy * dy) as usize);
                raster_circle(x0 as isize, y0 as isize, radius as isize, &mut emit);
            }
            _ => {}
        }
    }

    /// Silgi: merkez çevresindeki noktaları dairesel olarak kaldırır.
    pub fn erase(&mut self, x: usize, y: usize) {
        self.erase_segment(x, y, x, y);
    }

    /// İki nokta arasındaki doğru parçasının silgi yarıçapı içindeki tüm
    /// noktaları tek geçişte kaldırır. Hızlı sürüklemelerde dekesiz silme
    /// sağlar (nokta başına tam tarama yerine doğru-parçası uzaklığı).
    fn erase_segment(&mut self, x0: usize, y0: usize, x1: usize, y1: usize) {
        let r2 = (ERASER_RADIUS * ERASER_RADIUS) as isize;
        let sx = x0 as isize;
        let sy = y0 as isize;
        let ex = x1 as isize;
        let ey = y1 as isize;
        let dx = ex - sx;
        let dy = ey - sy;
        let len2 = dx * dx + dy * dy;
        let mut out = 0;
        for i in 0..self.len {
            let point = self.points[i];
            let px = point.x as isize;
            let py = point.y as isize;
            // Noktanın doğru parçasına en yakın noktasını bul (tam sayı).
            let (nx, ny) = if len2 == 0 {
                (sx, sy)
            } else {
                let proj = (px - sx) * dx + (py - sy) * dy;
                if proj <= 0 {
                    (sx, sy)
                } else if proj >= len2 {
                    (ex, ey)
                } else {
                    (sx + dx * proj / len2, sy + dy * proj / len2)
                }
            };
            let ddx = px - nx;
            let ddy = py - ny;
            if ddx * ddx + ddy * ddy > r2 {
                self.points[out] = point;
                out += 1;
            }
        }
        self.len = out;
    }

    /// Sağ tuşla sürükleyerek silme (Windows Paint davranışı). Sol tuş
    /// darbelerinden ayrı bir `erase_last` izi kullanır.
    pub fn erase_drag(&mut self, x: usize, y: usize) {
        if self.erase_last.is_none() {
            self.push_undo();
        }
        let (sx, sy) = self.erase_last.unwrap_or((x, y));
        self.erase_segment(sx, sy, x, y);
        self.erase_last = Some((x, y));
        self.status_msg = "";
    }

    /// Sağ tuş silme darbesini bitirir.
    pub fn end_erase_drag(&mut self) {
        self.erase_last = None;
    }

    pub fn end_stroke(&mut self) {
        if matches!(
            self.tool,
            PaintTool::Line | PaintTool::Rect | PaintTool::Circle
        ) {
            let color = crate::gui::theme::resolve(self.color);
            let mut new_points = alloc::vec::Vec::new();
            self.raster_shape(|x, y| {
                if new_points.len() < MAX_POINTS {
                    new_points.push(PaintPoint { x, y, color });
                }
            });
            for point in new_points {
                if self.len < MAX_POINTS {
                    self.points[self.len] = point;
                    self.len += 1;
                }
            }
        }
        self.last = None;
        self.shape_start = None;
        self.shape_current = None;
        self.erase_last = None;
    }

    pub fn clear(&mut self) {
        if self.len > 0 {
            self.push_undo();
        }
        self.len = 0;
        self.last = None;
        self.shape_start = None;
        self.shape_current = None;
        self.erase_last = None;
    }

    /// Belirtilen dosya adına BMP kaydeder (ör. "/root/Documents/deneme.bmp").
    pub fn save_bmp_named(
        &mut self,
        vfs: &mut crate::fs::vfs::VirtualFileSystem,
        canvas_w: usize,
        canvas_h: usize,
        path: &str,
    ) -> bool {
        let w = canvas_w.clamp(1, BMP_W);
        let h = canvas_h.clamp(1, BMP_H);
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
            let p = self.points[i];
            if p.x >= w || p.y >= h {
                continue;
            }
            let base = 54 + (h - 1 - p.y) * row_bytes + p.x * 3;
            bmp[base] = (p.color & 0xFF) as u8;
            bmp[base + 1] = ((p.color >> 8) & 0xFF) as u8;
            bmp[base + 2] = ((p.color >> 16) & 0xFF) as u8;
        }
        // Retain the bytes in the Paint instance and let VFS index them without
        // cloning a ~1.4 MiB file into the small kernel heap.
        let saved_bmp: alloc::sync::Arc<[u8]> = bmp.into();
        let ok = vfs.write_large_file(path, saved_bmp.clone());
        if ok {
            self.saved_bmp = saved_bmp;
        }
        self.status_msg = if ok {
            crate::gui::ui_text("Çizim kaydedildi", "Drawing saved")
        } else {
            crate::gui::ui_text("Kaydetme başarısız", "Save failed")
        };
        ok
    }

    /// Varsayılan dosya yolu ("Yeni" ve "Kaydet" bu adı kullanır).
    pub const DEFAULT_PATH: &'static str = "/root/Documents/cizim.bmp";

    pub fn save_bmp(
        &mut self,
        vfs: &mut crate::fs::vfs::VirtualFileSystem,
        canvas_w: usize,
        canvas_h: usize,
    ) -> bool {
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
    pub fn save_as(
        &mut self,
        vfs: &mut crate::fs::vfs::VirtualFileSystem,
        canvas_w: usize,
        canvas_h: usize,
        name: &str,
    ) -> bool {
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
            if v == 0 {
                num[0] = b'0';
                len = 1;
            }
            while v > 0 {
                num[len] = b'0' + (v % 10) as u8;
                v /= 10;
                len += 1;
            }
            for i in 0..len {
                candidate.push(num[len - 1 - i] as char);
            }
            candidate.push_str(ext);
            if !Self::path_exists(vfs, candidate.as_str()) {
                return candidate;
            }
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
                None => match vfs.read_file(Self::DEFAULT_PATH) {
                    Some(_) => {
                        self.status_msg =
                            crate::gui::ui_text("BMP açılamadı", "Could not open BMP");
                        return false;
                    }
                    None => {
                        self.status_msg = crate::gui::ui_text("BMP bulunamadı", "BMP not found");
                        return false;
                    }
                },
            };
            &stored
        };
        if data.len() < 54 || &data[0..2] != b"BM" || u16::from_le_bytes([data[28], data[29]]) != 24
        {
            self.status_msg = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP");
            return false;
        }
        let w =
            (i32::from_le_bytes([data[18], data[19], data[20], data[21]])).unsigned_abs() as usize;
        let h =
            (i32::from_le_bytes([data[22], data[23], data[24], data[25]])).unsigned_abs() as usize;
        let top_down = i32::from_le_bytes([data[22], data[23], data[24], data[25]]) < 0;
        let offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
        let row_bytes = (w
            .checked_mul(3)
            .and_then(|n| n.checked_add(3))
            .unwrap_or(usize::MAX))
            & !3;
        if offset < 54
            || row_bytes == usize::MAX
            || h > (data.len().saturating_sub(offset) / row_bytes.max(1)) + 1
        {
            self.status_msg = crate::gui::ui_text("Geçersiz BMP", "Invalid BMP");
            return false;
        }
        self.len = 0;
        for y in 0..h.min(BMP_H) {
            for x in 0..w.min(BMP_W) {
                let sy = if top_down { y } else { h - 1 - y };
                let i = offset + sy * row_bytes + x * 3;
                if i + 2 >= data.len() {
                    continue;
                }
                let color = (data[i + 2] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i] as u32;
                if color != 0xFFFFFF && self.len < MAX_POINTS {
                    self.points[self.len] = PaintPoint { x, y, color };
                    self.len += 1;
                }
            }
        }
        self.len = 0;
        self.last = None;
        self.shape_start = None;
        self.shape_current = None;
        self.erase_last = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.status_msg = crate::gui::ui_text("BMP açıldı", "BMP opened");
        true
    }

    /// Etiketli düğme: `enabled=false` iken tepki vermeyen soluk görünüm.
    /// Etkin düğmeler ortak `draw_button`'ı kullanır; böylece hover/basma
    /// animasyonu ve okunaklı metin rengi diğer uygulamalarla aynı olur.
    fn draw_paint_button(
        fb: &mut Framebuffer,
        r: (usize, usize, usize, usize),
        label: &str,
        bg: u32,
        enabled: bool,
    ) {
        if enabled {
            MinGuiCompositor::draw_button(fb, r, bg, label, Color::TEXT_MAIN, false);
            return;
        }
        let dim = MinGuiCompositor::shade(bg, -18);
        fb.draw_rect(r.0, r.1, r.2, r.3, dim);
        fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::BORDER);
        let tw = MinGuiCompositor::text_width(label, 1);
        let tx = r.0 + r.2.saturating_sub(tw) / 2;
        let th = MinGuiCompositor::text_height();
        let ty = r.1 + r.3.saturating_sub(th) / 2;
        MinGuiCompositor::draw_string_clipped(
            fb,
            tx,
            ty,
            label,
            Color::TEXT_MUTED,
            (
                r.0 + 2,
                r.1 + 1,
                r.2.saturating_sub(4),
                r.3.saturating_sub(2),
            ),
        );
    }

    /// Verilen RGB üzerinde okunaklı çerçeve rengi (siyah/beyaz seçimi).
    /// Palet ve renk seçicideki "seçili" işaretinin her renk üzerinde
    /// görünür kalmasını sağlar.
    fn contrast_on(rgb: u32) -> u32 {
        let r = (rgb >> 16) & 0xFF;
        let g = (rgb >> 8) & 0xFF;
        let b = rgb & 0xFF;
        if r * 299 + g * 587 + b * 114 >= 140_000 {
            Color::BLACK
        } else {
            Color::WHITE
        }
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
                let px = (canvas_x as isize
                    + canvas_w as isize / 2
                    + (point.x as isize - canvas_w as isize / 2) * zoom as isize / 100)
                    .max(0) as usize;
                let py = (canvas_y as isize
                    + canvas_h as isize / 2
                    + (point.y as isize - canvas_h as isize / 2) * zoom as isize / 100)
                    .max(0) as usize;
                let brush = (self.brush_size * zoom / 100).max(1);
                if px >= canvas_x
                    && py >= canvas_y
                    && px < canvas_x + canvas_w
                    && py < canvas_y + canvas_h
                {
                    fb.draw_rect(
                        px.saturating_sub(brush / 2),
                        py.saturating_sub(brush / 2),
                        brush,
                        brush,
                        point.color,
                    );
                }
            }
        }

        // Şekil önizlemesi: sürükleme sırasında henüz noktalara işlenmemiş
        // çizgi/dikdörtgen/daireyi doğrudan framebuffer'a çiz. Nokta boyutu
        // commit edilen fırça karesiyle aynıdır; sonuç WYSIWYG olur.
        if matches!(
            self.tool,
            PaintTool::Line | PaintTool::Rect | PaintTool::Circle
        ) && self.shape_start.is_some()
        {
            let color = crate::gui::theme::resolve(self.color);
            let pv = (self.brush_size * zoom / 100).max(2);
            self.raster_shape(|sx, sy| {
                if sx < canvas_w && sy < canvas_h {
                    let px = (canvas_x as isize
                        + canvas_w as isize / 2
                        + (sx as isize - canvas_w as isize / 2) * zoom as isize / 100)
                        .max(0) as usize;
                    let py = (canvas_y as isize
                        + canvas_h as isize / 2
                        + (sy as isize - canvas_h as isize / 2) * zoom as isize / 100)
                        .max(0) as usize;
                    if px >= canvas_x
                        && py >= canvas_y
                        && px < canvas_x + canvas_w
                        && py < canvas_y + canvas_h
                    {
                        fb.draw_rect(
                            px.saturating_sub(pv / 2),
                            py.saturating_sub(pv / 2),
                            pv,
                            pv,
                            color,
                        );
                    }
                }
            });
        }

        // Fırça/silgi ucu önizlemesi ve şekil artısı — imleç tuvalin
        // üstündeyken çizilir; koyu çerçeve beyaz tuvalde her zaman görünür.
        let (ptr_x, ptr_y) = MinGuiCompositor::pointer_position();
        let ptr_on_canvas = ptr_x >= canvas_x
            && ptr_x < canvas_x + canvas_w
            && ptr_y >= canvas_y
            && ptr_y < canvas_y + canvas_h;
        if ptr_on_canvas {
            match self.tool {
                PaintTool::Brush | PaintTool::Eraser => {
                    let size = if self.tool == PaintTool::Eraser {
                        (ERASER_RADIUS * 2 * zoom / 100).max(4)
                    } else {
                        (self.brush_size * zoom / 100).max(2)
                    };
                    fb.draw_rect_outline(
                        ptr_x.saturating_sub(size / 2),
                        ptr_y.saturating_sub(size / 2),
                        size,
                        size,
                        Color::BLACK,
                    );
                }
                PaintTool::Line | PaintTool::Rect | PaintTool::Circle => {
                    let color = crate::gui::theme::resolve(self.color);
                    fb.draw_line(
                        ptr_x.saturating_sub(6),
                        ptr_y,
                        ptr_x.saturating_add(6),
                        ptr_y,
                        color,
                    );
                    fb.draw_line(
                        ptr_x,
                        ptr_y.saturating_sub(6),
                        ptr_x,
                        ptr_y.saturating_add(6),
                        color,
                    );
                }
            }
        }

        // 2. Menü satırı: Dosya menüsü + dosya işlemleri (solda) ve fırça
        // boyutu kontrolü (sağda). Düğmeler ortak `draw_button` görünümünü
        // kullanır; hover/basma animasyonu otomatik gelir.
        fb.draw_rect(
            x + sc(6),
            y + Self::menu_y(),
            w.saturating_sub(sc(12)),
            Self::menu_h(),
            Color::TITLE_BAR,
        );
        fb.draw_line(
            x + sc(6),
            y + Self::menu_y() + Self::menu_h(),
            x + w.saturating_sub(sc(6)),
            y + Self::menu_y() + Self::menu_h(),
            Color::BORDER,
        );

        let mby = Self::menu_btn_y();
        let mlabel_y = mby + sc(20).saturating_sub(MinGuiCompositor::text_height()) / 2;
        let draw_btn = |fb: &mut Framebuffer,
                        r: (usize, usize, usize, usize),
                        label: &str,
                        bg: u32,
                        enabled: bool| {
            Self::draw_paint_button(fb, (x + r.0, y + r.1, r.2, r.3), label, bg, enabled);
        };

        // "Dosya" açılır menü düğmesi — etiketin sağında küçük üçgen imleç.
        let fr = Self::file_menu_rect(w);
        let file_bg = if self.file_menu_open {
            Color::ACCENT_BLUE
        } else {
            Color::WINDOW_BG
        };
        draw_btn(fb, fr, crate::gui::ui_text("Dosya", "File"), file_bg, true);
        let tri_color = MinGuiCompositor::text_on(file_bg);
        let tri_x = x + fr.0 + fr.2 - sc(14);
        let tri_y = y + fr.1 + fr.3 / 2;
        fb.draw_rect(tri_x, tri_y.saturating_sub(1), sc(7), sc(1), tri_color);
        fb.draw_rect(tri_x + sc(1), tri_y, sc(5), sc(1), tri_color);
        fb.draw_rect(tri_x + sc(2), tri_y + 1, sc(3), sc(1), tri_color);

        // Dosya işlemleri — eskiden görünmez isabet alanlarıydı; artık
        // gerçek düğmeler olarak çiziliyor, geri al/ileri al ve temizle
        // uygun olmadığında soluk (devre dışı) görünür.
        draw_btn(
            fb,
            Self::open_btn_rect(w),
            crate::gui::ui_text("Aç", "Open"),
            Color::WINDOW_BG,
            true,
        );
        draw_btn(
            fb,
            Self::save_btn_rect(w),
            crate::gui::ui_text("Kaydet", "Save"),
            Color::WINDOW_BG,
            true,
        );
        draw_btn(
            fb,
            Self::undo_btn_rect(),
            crate::gui::ui_text("Geri Al", "Undo"),
            Color::WINDOW_BG,
            self.can_undo(),
        );
        draw_btn(
            fb,
            Self::redo_btn_rect(),
            crate::gui::ui_text("Yinele", "Redo"),
            Color::WINDOW_BG,
            self.can_redo(),
        );
        draw_btn(
            fb,
            Self::clear_btn_rect(w),
            crate::gui::ui_text("Temizle", "Clear"),
            Color::WINDOW_BG,
            self.len > 0,
        );

        // Fırça boyutu kontrolü — menü satırının sağ ucunda.
        let gx = Self::brush_group_x(w);
        MinGuiCompositor::draw_string(
            fb,
            x + gx,
            y + mlabel_y,
            crate::gui::ui_text("Fırça:", "Brush:"),
            Color::TEXT_MAIN,
        );
        let mut b_buf = [0u8; 8];
        let b_str = format_num(self.brush_size, &mut b_buf);
        let val_x = x + gx + sc(46);
        MinGuiCompositor::draw_string(fb, val_x, y + mlabel_y, b_str, Color::ACCENT_BLUE);
        let px_x = val_x + MinGuiCompositor::text_width(b_str, 1) + sc(3);
        MinGuiCompositor::draw_string(fb, px_x, y + mlabel_y, "px", Color::TEXT_MUTED);
        draw_btn(
            fb,
            Self::brush_minus_rect(w),
            "-",
            Color::WINDOW_BG,
            self.brush_size > MIN_BRUSH_SIZE,
        );
        draw_btn(
            fb,
            Self::brush_plus_rect(w),
            "+",
            Color::WINDOW_BG,
            self.brush_size < MAX_BRUSH_SIZE,
        );

        // 3. Araç şeridi: hızlı palet + silgi + şekil araçları + renk
        // seçici (solda), zoom kontrolü (sağda).
        let ty = Self::tool_y();
        let tby = Self::tool_btn_y();
        let tlabel_y = tby + sc(20).saturating_sub(MinGuiCompositor::text_height()) / 2;
        fb.draw_rect(
            x + sc(6),
            y + ty,
            w.saturating_sub(sc(12)),
            Self::tool_h(),
            MinGuiCompositor::shade(Color::TITLE_BAR, -10),
        );
        fb.draw_rect_outline(
            x + sc(6),
            y + ty,
            w.saturating_sub(sc(12)),
            Self::tool_h(),
            Color::BORDER,
        );

        // Hızlı palet — seçili renk her zeminde okunaklı kontrast çerçevesi
        // alır; hover'da ince bir vurgu belirir.
        let (hx, hy) = MinGuiCompositor::pointer_position();
        for (i, &color) in PALETTE.iter().enumerate() {
            let r = Self::swatch_rect(i);
            let sxr = x + r.0;
            let syr = y + r.1;
            fb.draw_rect(sxr, syr, r.2, r.3, color);
            fb.draw_rect_outline(sxr, syr, r.2, r.3, Color::BORDER);
            let over = hx >= sxr && hx < sxr + r.2 && hy >= syr && hy < syr + r.3;
            if self.color == color {
                fb.draw_rect_outline(
                    sxr.saturating_sub(1),
                    syr.saturating_sub(1),
                    r.2 + 2,
                    r.3 + 2,
                    Self::contrast_on(color),
                );
            } else if over {
                fb.draw_rect_outline(
                    sxr.saturating_sub(1),
                    syr.saturating_sub(1),
                    r.2 + 2,
                    r.3 + 2,
                    MinGuiCompositor::shade(Color::BORDER, 60),
                );
            }
        }

        // Renk seçici düğmesi.
        draw_btn(
            fb,
            Self::picker_btn_rect(),
            crate::gui::ui_text("Renkler", "Colors"),
            if self.picker_open {
                Color::ACCENT_BLUE
            } else {
                Color::WINDOW_BG
            },
            true,
        );

        // Fırça ve silgi — etkin araç vurgu rengiyle işaretlenir.
        draw_btn(
            fb,
            Self::brush_tool_rect(),
            crate::gui::ui_text("Fırça", "Brush"),
            if self.tool == PaintTool::Brush {
                Color::ACCENT_BLUE
            } else {
                Color::WINDOW_BG
            },
            true,
        );
        draw_btn(
            fb,
            Self::eraser_rect(),
            crate::gui::ui_text("Silgi", "Eraser"),
            if self.tool == PaintTool::Eraser {
                Color::ACCENT_BLUE
            } else {
                Color::WINDOW_BG
            },
            true,
        );

        // Ayırıcı.
        let sep_x = x + sc(284);
        fb.draw_line(
            sep_x,
            y + ty + sc(6),
            sep_x,
            y + ty + Self::tool_h() - sc(6),
            Color::BORDER,
        );

        // Şekil araçları — etkin araç vurgu rengiyle işaretlenir.
        for (r, label, tool) in [
            (
                Self::tool_line_rect(),
                crate::gui::ui_text("Çizgi", "Line"),
                PaintTool::Line,
            ),
            (
                Self::tool_rect_rect(),
                crate::gui::ui_text("Dikdörtgen", "Rectangle"),
                PaintTool::Rect,
            ),
            (
                Self::tool_circle_rect(),
                crate::gui::ui_text("Daire", "Circle"),
                PaintTool::Circle,
            ),
        ] {
            draw_btn(
                fb,
                r,
                label,
                if self.tool == tool {
                    Color::ACCENT_BLUE
                } else {
                    Color::WINDOW_BG
                },
                true,
            );
        }

        // Zoom kontrolü (sağda): "100%" + [-] [+]
        let zm = Self::zoom_minus_rect(w);
        draw_btn(fb, zm, "-", Color::WINDOW_BG, self.zoom > 50);
        let zp = Self::zoom_plus_rect(w);
        draw_btn(fb, zp, "+", Color::WINDOW_BG, self.zoom < 300);
        let mut z_buf = [0u8; 8];
        let z_str = format_num(self.zoom, &mut z_buf);
        let mut zt = crate::gui::TextBuf::new();
        zt.push_str(z_str);
        zt.push_byte(b'%');
        let ztxt = zt.as_str();
        let zw = MinGuiCompositor::text_width(ztxt, 1);
        MinGuiCompositor::draw_string(
            fb,
            x + zm.0.saturating_sub(sc(8) + zw),
            y + tlabel_y,
            ztxt,
            Color::TEXT_MAIN,
        );

        // 4. Renk seçici açılır penceresi (tuvalin üstünde yüzer).
        if self.picker_open {
            let pr = Self::picker_rect();
            let pxr = x + pr.0;
            let pyr = y + pr.1;
            fb.draw_shadow(pxr, pyr, pr.2, pr.3, Color::SHADOW);
            fb.draw_rect(pxr, pyr, pr.2, pr.3, Color::WINDOW_BG);
            fb.draw_rect_outline(pxr, pyr, pr.2, pr.3, Color::BORDER);
            for (i, &cc) in PICKER_COLORS.iter().enumerate() {
                let cr = Self::picker_cell_rect(i);
                let cxr = x + cr.0;
                let cyr = y + cr.1;
                fb.draw_rect(cxr, cyr, cr.2, cr.3, cc);
                fb.draw_rect_outline(cxr, cyr, cr.2, cr.3, Color::BORDER);
                let over = hx >= cxr && hx < cxr + cr.2 && hy >= cyr && hy < cyr + cr.3;
                if self.color == cc {
                    fb.draw_rect_outline(
                        cxr.saturating_sub(1),
                        cyr.saturating_sub(1),
                        cr.2 + 2,
                        cr.3 + 2,
                        Self::contrast_on(cc),
                    );
                } else if over {
                    fb.draw_rect_outline(
                        cxr.saturating_sub(1),
                        cyr.saturating_sub(1),
                        cr.2 + 2,
                        cr.3 + 2,
                        MinGuiCompositor::shade(Color::BORDER, 60),
                    );
                }
            }
        }

        // 5. Alt durum çubuğu
        let sby = Self::status_top(h);
        let status_w = w.saturating_sub(sc(12));
        fb.draw_rect(x + sc(6), y + sby, status_w, status_h(), Color::TASKBAR_BG);
        fb.draw_line(
            x + sc(6),
            y + sby,
            x + sc(6) + status_w,
            y + sby,
            Color::BORDER,
        );
        let stext_y = y + sby + status_h().saturating_sub(MinGuiCompositor::text_height()) / 2;

        // Sol: durum mesajı
        MinGuiCompositor::draw_string_clipped(
            fb,
            x + sc(12),
            stext_y,
            self.status_text(),
            if self.tool == PaintTool::Eraser {
                Color::ACCENT_BLUE
            } else {
                Color::ACCENT_GREEN
            },
            (
                x + sc(10),
                y + sby + sc(2),
                status_w / 3,
                status_h().saturating_sub(sc(4)),
            ),
        );

        // Orta: dosya adı
        let doc_name = self.current_name();
        let doc_w = MinGuiCompositor::text_width(doc_name, 1);
        let doc_x = x + (w.saturating_sub(doc_w)) / 2;
        MinGuiCompositor::draw_string(fb, doc_x, stext_y, doc_name, Color::TEXT_MAIN);

        // Sağ: nokta sayısı + fırça + zoom + imlecin tuval koordinatı.
        // `alloc::format!` her karede heap tahsisi yapar; sabit boyutlu
        // `TextBuf` kullanılır.
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
        if let Some((cxp, cyp)) =
            self.to_canvas(w, h, ptr_x.saturating_sub(x), ptr_y.saturating_sub(y))
        {
            right_info.push_str(" | ");
            right_info.push_usize(cxp);
            right_info.push_byte(b',');
            right_info.push_usize(cyp);
        }
        let right_info = right_info.as_str();
        let right_w = MinGuiCompositor::text_width(right_info, 1);
        let right_x = x + w.saturating_sub(right_w + sc(16));
        MinGuiCompositor::draw_string(fb, right_x, stext_y, right_info, Color::TEXT_MUTED);

        // 6. "Dosya" açılır menüsü (en üst katman)
        if self.file_menu_open {
            let (mx, my, mw, mh) = Self::file_dropdown_rect(w, h);
            let drop_x = x + mx;
            let drop_y = y + my;
            fb.draw_shadow(drop_x, drop_y, mw, mh, Color::SHADOW);
            fb.draw_rect(drop_x, drop_y, mw, mh, Color::WINDOW_BG);
            fb.draw_rect_outline(drop_x, drop_y, mw, mh, Color::BORDER);

            let labels = Self::file_menu_labels();
            // Son üç öğe (Geri Al / Yinele / Temizle) uygun olmadığında
            // menüde de soluk görünür.
            let enabled = [
                true,
                true,
                true,
                true,
                self.can_undo(),
                self.can_redo(),
                self.len > 0,
            ];
            for (i, label) in labels.iter().enumerate() {
                let r = Self::file_menu_item_rect(w, h, i);
                let item_x = x + r.0;
                let item_y = y + r.1;
                let over = hx >= item_x && hx < item_x + r.2 && hy >= item_y && hy < item_y + r.3;
                fb.draw_rect(
                    item_x,
                    item_y,
                    r.2,
                    r.3,
                    if over {
                        MinGuiCompositor::shade(Color::TITLE_BAR_INACTIVE, 30)
                    } else {
                        Color::TITLE_BAR_INACTIVE
                    },
                );
                MinGuiCompositor::draw_string(
                    fb,
                    item_x + sc(8),
                    item_y + sc(4),
                    label,
                    if enabled[i] {
                        Color::TEXT_MAIN
                    } else {
                        Color::TEXT_MUTED
                    },
                );
                if i + 1 < Self::FILE_MENU_COUNT {
                    fb.draw_line(
                        item_x + sc(2),
                        item_y + r.3 + 1,
                        item_x + r.2 - sc(2),
                        item_y + r.3 + 1,
                        Color::BORDER,
                    );
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // İsabet alanları — `render_content` ile birebir aynı geometri.
    // Tıklama testi bu fonksiyonları kullandığı için çizim ve etkileşim
    // asla ayrışmaz. Tüm koordinatlar pencere içi (içerik) uzayındadır.
    // ------------------------------------------------------------------

    /// "Dosya" açılır menü düğmesi.
    pub fn file_menu_rect(_w: usize) -> (usize, usize, usize, usize) {
        (sc(12), Self::menu_btn_y(), sc(60), sc(20))
    }
    /// Aç / Kaydet / Geri Al / Yinele / Temizle düğmeleri (menü satırı).
    pub fn open_btn_rect(_w: usize) -> (usize, usize, usize, usize) {
        (sc(82), Self::menu_btn_y(), sc(50), sc(20))
    }
    pub fn save_btn_rect(_w: usize) -> (usize, usize, usize, usize) {
        (sc(138), Self::menu_btn_y(), sc(58), sc(20))
    }
    pub fn undo_btn_rect() -> (usize, usize, usize, usize) {
        (sc(202), Self::menu_btn_y(), sc(60), sc(20))
    }
    pub fn redo_btn_rect() -> (usize, usize, usize, usize) {
        (sc(268), Self::menu_btn_y(), sc(60), sc(20))
    }
    pub fn clear_btn_rect(_w: usize) -> (usize, usize, usize, usize) {
        (sc(334), Self::menu_btn_y(), sc(64), sc(20))
    }

    /// Fırça boyutu grubunun sol kenarı — menü satırının sağ ucuna dayalı.
    pub fn brush_group_x(w: usize) -> usize {
        w.saturating_sub(sc(12) + sc(128))
    }
    /// Fırça küçült/büyüt düğmeleri (menü satırı, sağ taraf).
    pub fn brush_minus_rect(w: usize) -> (usize, usize, usize, usize) {
        (
            Self::brush_group_x(w) + sc(80),
            Self::menu_btn_y(),
            sc(22),
            sc(20),
        )
    }
    pub fn brush_plus_rect(w: usize) -> (usize, usize, usize, usize) {
        (
            Self::brush_group_x(w) + sc(104),
            Self::menu_btn_y(),
            sc(22),
            sc(20),
        )
    }

    /// Renk kutusu (hızlı palet) dikdörtgeni.
    pub fn swatch_rect(index: usize) -> (usize, usize, usize, usize) {
        (
            sc(12) + index * sc(22),
            Self::tool_y() + sc(4),
            sc(18),
            sc(18),
        )
    }

    /// Renk seçici düğmesi ve açılır grid.
    fn picker_pop_x() -> usize {
        sc(104)
    }
    fn picker_pop_y() -> usize {
        Self::tool_y() + Self::tool_h() + sc(2)
    }
    pub fn picker_btn_rect() -> (usize, usize, usize, usize) {
        (sc(104), Self::tool_btn_y(), sc(58), sc(20))
    }
    pub fn picker_rect() -> (usize, usize, usize, usize) {
        (Self::picker_pop_x(), Self::picker_pop_y(), sc(140), sc(98))
    }
    pub fn picker_cell_rect(index: usize) -> (usize, usize, usize, usize) {
        let row = index / 6;
        let col = index % 6;
        (
            Self::picker_pop_x() + sc(6) + col * sc(22),
            Self::picker_pop_y() + sc(6) + row * sc(22),
            sc(18),
            sc(18),
        )
    }

    /// Fırça aracı düğmesi (araç şeridi).
    pub fn brush_tool_rect() -> (usize, usize, usize, usize) {
        (sc(170), Self::tool_btn_y(), sc(46), sc(20))
    }
    /// Silgi düğmesi.
    pub fn eraser_rect() -> (usize, usize, usize, usize) {
        (sc(224), Self::tool_btn_y(), sc(52), sc(20))
    }
    /// Şekil araçları (araç şeridi).
    pub fn tool_line_rect() -> (usize, usize, usize, usize) {
        (sc(292), Self::tool_btn_y(), sc(48), sc(20))
    }
    pub fn tool_rect_rect() -> (usize, usize, usize, usize) {
        (sc(348), Self::tool_btn_y(), sc(70), sc(20))
    }
    pub fn tool_circle_rect() -> (usize, usize, usize, usize) {
        (sc(426), Self::tool_btn_y(), sc(54), sc(20))
    }
    /// Zoom küçült/büyüt düğmeleri (araç şeridi, sağa dayalı).
    pub fn zoom_plus_rect(w: usize) -> (usize, usize, usize, usize) {
        (
            w.saturating_sub(sc(12) + sc(22)),
            Self::tool_btn_y(),
            sc(22),
            sc(20),
        )
    }
    pub fn zoom_minus_rect(w: usize) -> (usize, usize, usize, usize) {
        let plus = Self::zoom_plus_rect(w);
        (plus.0.saturating_sub(sc(26)), plus.1, sc(22), sc(20))
    }

    /// "Dosya" açılır menüsünün pencere içi (x, y, w, h) ölçüleri.
    /// Düğmenin altına açılır; alta sığmazsa üste çevrilir ve her zaman
    /// pencere sınırları içine kelepçelenir.
    pub fn file_dropdown_rect(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let mw = sc(190).min(w.saturating_sub(sc(16)));
        let mh = sc(170).min(h.saturating_sub(sc(8)));
        let mx_max = w.saturating_sub(mw + sc(4));
        let mx = sc(12).min(mx_max);
        // "Dosya" düğmesinin alt kenarı: menu_y + menu_h
        let below = Self::menu_y() + Self::menu_h() + sc(2);
        let mut my = below;
        if my + mh > h.saturating_sub(sc(4)) && Self::menu_y() >= mh + sc(6) {
            // Alta sığmıyor: düğmenin üstüne aç.
            my = Self::menu_y().saturating_sub(mh + sc(2));
        }
        let y_min = sc(4);
        let y_max = h.saturating_sub(mh + sc(4));
        if my < y_min {
            my = y_min;
        } else if my > y_max && y_max >= y_min {
            my = y_max;
        }
        (mx, my, mw, mh)
    }

    /// Menüdeki öğe sayısı.
    pub const FILE_MENU_COUNT: usize = 7;

    /// Menü öğelerinin etiketleri (iki dilli).
    pub fn file_menu_labels() -> [&'static str; 7] {
        [
            crate::gui::ui_text("Yeni (Ctrl+N)", "New (Ctrl+N)"),
            crate::gui::ui_text("Aç... (Ctrl+O)", "Open... (Ctrl+O)"),
            crate::gui::ui_text("Kaydet (Ctrl+S)", "Save (Ctrl+S)"),
            crate::gui::ui_text("Farklı Kaydet...", "Save As..."),
            crate::gui::ui_text("Geri Al (Ctrl+Z)", "Undo (Ctrl+Z)"),
            crate::gui::ui_text("Yinele (Ctrl+Y)", "Redo (Ctrl+Y)"),
            crate::gui::ui_text("Temizle", "Clear"),
        ]
    }

    /// Açılır menüdeki öğe (x, y, w, h) koordinatlarını (pencere içi) döndürür.
    pub fn file_menu_item_rect(w: usize, h: usize, index: usize) -> (usize, usize, usize, usize) {
        let (mx, my, mw, _mh) = Self::file_dropdown_rect(w, h);
        (
            mx + sc(4),
            my + sc(4) + index * sc(22),
            mw.saturating_sub(sc(8)),
            sc(21),
        )
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

impl Default for PaintApp {
    fn default() -> Self {
        Self::new()
    }
}

/// Bresenham çizgi rasterleştiricisi.
fn raster_line<F: FnMut(usize, usize)>(x0: isize, y0: isize, x1: isize, y1: isize, emit: &mut F) {
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x >= 0 && y >= 0 {
            emit(x as usize, y as usize);
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Orta nokta algoritmasıyla daire çizer.
fn raster_circle<F: FnMut(usize, usize)>(cx: isize, cy: isize, radius: isize, emit: &mut F) {
    if radius <= 0 {
        if cx >= 0 && cy >= 0 {
            emit(cx as usize, cy as usize);
        }
        return;
    }
    let mut x = radius;
    let mut y = 0isize;
    let mut err = 1 - radius;
    while x >= y {
        for (px, py) in [
            (cx + x, cy + y),
            (cx + y, cy + x),
            (cx - y, cy + x),
            (cx - x, cy + y),
            (cx - x, cy - y),
            (cx - y, cy - x),
            (cx + y, cy - x),
            (cx + x, cy - y),
        ] {
            if px >= 0 && py >= 0 {
                emit(px as usize, py as usize);
            }
        }
        y += 1;
        if err < 0 {
            err += 2 * y + 1;
        } else {
            x -= 1;
            err += 2 * (y - x) + 1;
        }
    }
}

fn integer_sqrt(value: usize) -> usize {
    if value < 2 {
        return value;
    }
    let mut guess = value;
    let mut next = guess.div_ceil(2);
    while next < guess {
        guess = next;
        next = (guess + value / guess) / 2;
    }
    guess
}

fn format_num(mut n: usize, buf: &mut [u8; 8]) -> &str {
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
