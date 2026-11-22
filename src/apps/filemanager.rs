#![allow(dead_code, static_mut_refs)]

//! Dosya Yöneticisi Uygulaması (File Manager) — Klasör Desteği, Dosya Açma, Yeni Dosya/Klasör ve Silme.

extern crate alloc;
use crate::drivers::video::Framebuffer;
use crate::fs::diskfs::{self, FsKind};
use crate::fs::vfs::{NodeType, VirtualFileSystem};
use crate::gui::scrollbar::{self, ScrollState};
use crate::gui::{scaled_statusbar_h, Color, MinGuiCompositor, TextBuf};
use alloc::string::String;
use alloc::vec::Vec;

fn numbered_name(prefix: &str, mut value: usize, extension: &str) -> String {
    let mut digits = [0u8; 10];
    let mut len = 0;
    if value == 0 {
        digits[0] = b'0';
        len = 1;
    }
    while value > 0 && len < digits.len() {
        digits[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
    }
    digits[..len].reverse();
    let mut name = String::from(prefix);
    for _ in len..3 {
        name.push('0');
    }
    if let Ok(number) = core::str::from_utf8(&digits[..len]) {
        name.push_str(number);
    }
    name.push_str(extension);
    name
}

/// Dosya listesi satır yüksekliği (px).
const ROW_H: usize = 24;
/// Listenin başladığı Y ofseti (pencere içine göre).
pub const LIST_TOP: usize = 86;

pub fn list_top() -> usize {
    LIST_TOP
}
/// Durum çubuğu için ayrılan alt alan (px).
const STATUS_H: usize = 0; // Use scaled_statusbar_h() instead

pub fn status_h() -> usize {
    scaled_statusbar_h()
}

pub const MAX_ITEMS: usize = 64;

pub struct FileClipboard {
    pub paths: [TextBuf; MAX_ITEMS],
    pub count: usize,
    pub cut: bool,
}

impl FileClipboard {
    pub const fn new() -> Self {
        Self {
            paths: [TextBuf::new(); MAX_ITEMS],
            count: 0,
            cut: false,
        }
    }
}

impl Default for FileClipboard {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct FileItem {
    pub name: &'static str,
    pub dyn_name: [u8; diskfs::NAME_MAX],
    pub dyn_name_len: usize,
    pub size_str: TextBuf,
    pub file_type: &'static str,
    pub icon_tag: &'static str,
    pub is_dir: bool,
}

impl FileItem {
    pub const fn empty() -> Self {
        Self {
            name: "",
            dyn_name: [0; diskfs::NAME_MAX],
            dyn_name_len: 0,
            size_str: TextBuf::new(),
            file_type: "",
            icon_tag: "",
            is_dir: false,
        }
    }

    pub fn display_name(&self) -> &str {
        if self.dyn_name_len > 0 {
            core::str::from_utf8(&self.dyn_name[..self.dyn_name_len]).unwrap_or("")
        } else {
            self.name
        }
    }
}

/// Boyut string'ini (örn: "1.2 KB") byte cinsinden sayıya çevirir.
fn parse_size(s: &str) -> u64 {
    let s = s.trim();
    if s == "<DIR>" || s == "<UP>" {
        return 0;
    }
    let mut num = 0u64;
    let mut mult = 1u64;
    let mut i = 0usize;
    while i < s.len() {
        let c = s.as_bytes()[i];
        if c.is_ascii_digit() {
            num = num * 10 + (c - b'0') as u64;
        } else if c == b'.' {
            // Ondalık kısmı atla (basitleştirilmiş)
            while i + 1 < s.len() && s.as_bytes()[i + 1] >= b'0' && s.as_bytes()[i + 1] <= b'9' {
                i += 1;
            }
        } else if c == b' ' {
            // Boşluk - birim gelebilir
        } else if c == b'K' || c == b'k' {
            mult = 1024;
        } else if c == b'M' || c == b'm' {
            mult = 1024 * 1024;
        } else if c == b'G' || c == b'g' {
            mult = 1024 * 1024 * 1024;
        }
        i += 1;
    }
    num * mult
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Vfs,
    Disk,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropTarget {
    Sidebar(usize),
    Folder(usize),
    CurrentDirectory,
}

/// Dosya Yöneticisi görünüm modu: klasik satır listesi veya küçük resimli ızgara.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Grid,
}

/// Sıralama kriteri.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Size,
    Type,
}

/// Sıralama yönü.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Klavye gezinmesindeki seçim değiştirici (modifier).
///
/// Ok tuşları / Home / End / PageUp / PageDown ile gezinirken Shift ve Ctrl
/// tuşlarının anlamı tek yerde tanımlıdır:
///
/// * `None`     — seçimi imlece taşı (tek seçim).
/// * `Shift`    — çapa (anchor) ile imleç arasını aralık olarak seç.
/// * `Ctrl`     — imleci taşı, mevcut seçimi koru.
/// * `CtrlShift`— imleci taşı ve imleçteki öğeyi seçime ekle/çıkar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelectMod {
    #[default]
    None,
    Shift,
    Ctrl,
    CtrlShift,
}

impl SelectMod {
    /// Klavye durumundan seçim değiştiricisini üretir.
    pub fn from_keys(shift: bool, ctrl: bool) -> Self {
        match (shift, ctrl) {
            (true, true) => SelectMod::CtrlShift,
            (true, false) => SelectMod::Shift,
            (false, true) => SelectMod::Ctrl,
            (false, false) => SelectMod::None,
        }
    }

    /// Aralık seçimi yapılıyor mu?
    pub fn is_range(self) -> bool {
        matches!(self, SelectMod::Shift | SelectMod::CtrlShift)
    }

    /// Çapa sabit tutulacak mı? (Shift'li gezinmede seçim çapası yerinde kalır.)
    pub fn keeps_anchor(self) -> bool {
        self.is_range()
    }
}

/// Hızlı Önizleme panelinde gösterilen içerik türü.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Empty,
    File,
    Text,
    Image,
    Directory,
}

/// Önizleme panelinde gösterilebilecek azami metin satırı.
pub const PREVIEW_LINES: usize = 22;
/// Izgara hücre ölçüleri (px).
const GRID_CELL_W: usize = 96;
const GRID_CELL_H: usize = 104;
/// Hızlı Önizleme paneli genişliği (px).
const PREVIEW_W: usize = 232;
/// Küçük resim (thumbnail) azami kenar ölçüsü (px).
const THUMB_MAX: usize = 28;
/// Küçük resim üretmek için okunacak azami dosya boyutu.
const THUMB_MAX_BYTES: usize = 1024 * 1024;

/// Çözümlenmiş küçük resim: `w` x `h` boyutunda, satır-major ARGB renk dizisi.
pub struct Thumb {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Thumb {
    /// 24-bit BMP verisinden en/boy oranını koruyan ölçekli bir küçük resim üretir.
    fn from_bmp(data: &[u8]) -> Option<Thumb> {
        if data.len() < 54 || &data[0..2] != b"BM" {
            return None;
        }
        if u16::from_le_bytes([data[28], data[29]]) != 24 {
            return None;
        }
        let raw_w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        let raw_h = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        if raw_w == 0 || raw_h == 0 {
            return None;
        }
        let src_w = raw_w.unsigned_abs() as usize;
        let src_h = raw_h.unsigned_abs() as usize;
        let offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
        let row_bytes = (src_w.checked_mul(3)? + 3) & !3;
        if offset < 54 || offset.checked_add(row_bytes.checked_mul(src_h)?)? > data.len() {
            return None;
        }
        let (tw, th) = if src_w >= src_h {
            (THUMB_MAX, (src_h * THUMB_MAX / src_w).max(1))
        } else {
            ((src_w * THUMB_MAX / src_h).max(1), THUMB_MAX)
        };
        let top_down = raw_h < 0;
        let mut px = alloc::vec![0u32; tw * th];
        for ty in 0..th {
            let sy = ty * src_h / th;
            let bmp_y = if top_down { sy } else { src_h - 1 - sy };
            for tx in 0..tw {
                let sx = tx * src_w / tw;
                let i = offset + bmp_y * row_bytes + sx * 3;
                if i + 2 >= data.len() {
                    continue;
                }
                px[ty * tw + tx] =
                    (data[i + 2] as u32) << 16 | (data[i + 1] as u32) << 8 | data[i] as u32;
            }
        }
        Some(Thumb { w: tw, h: th, px })
    }

    fn from_image(data: &[u8]) -> Option<Thumb> {
        if data.len() >= 2 && &data[0..2] == b"BM" {
            return Self::from_bmp(data);
        }
        let decoded = crate::apps::imgdecode::decode(data)?;
        let (src_w, src_h) = (decoded.width, decoded.height);
        let (tw, th) = if src_w >= src_h {
            (THUMB_MAX, (src_h * THUMB_MAX / src_w).max(1))
        } else {
            ((src_w * THUMB_MAX / src_h).max(1), THUMB_MAX)
        };
        let mut px = alloc::vec![0u32; tw * th];
        for ty in 0..th {
            let sy = ty * src_h / th;
            for tx in 0..tw {
                let sx = tx * src_w / tw;
                let i = (sy * src_w + sx) * 3;
                if i + 2 < decoded.rgb.len() {
                    px[ty * tw + tx] = (decoded.rgb[i] as u32) << 16
                        | (decoded.rgb[i + 1] as u32) << 8
                        | decoded.rgb[i + 2] as u32;
                }
            }
        }
        Some(Thumb { w: tw, h: th, px })
    }
}

/// Dosya işlemi onay iletişim kutusunun durumu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConfirmDialog {
    None,
    NewFile,
    NewFolder,
    Rename,
    Delete,
    DeletePermanent,
}

pub struct FileManagerApp {
    pub current_path: &'static str,
    pub path_buf: [u8; 96],
    pub path_len: usize,
    pub selected_idx: usize,
    pub selected_items: [bool; MAX_ITEMS],
    pub selection_anchor: usize,
    pub files: [FileItem; MAX_ITEMS],
    pub file_count: usize,
    pub source: Source,
    pub disk_kind: FsKind,
    pub status: &'static str,
    pub status_buf: TextBuf,
    pub rename_active: bool,
    pub rename_buf: [u8; diskfs::NAME_MAX],
    pub rename_len: usize,
    pub focused_control: usize, // 0-4: actions, 5: List, 6: Search
    pub actions_menu_open: bool,
    pub last_click_idx: Option<usize>,
    pub last_click_ms: u64,
    pub drag_item: Option<usize>,
    pub drag_start: (usize, usize),
    pub drag_active: bool,
    pub drag_target: Option<DropTarget>,
    pub drag_items: [bool; MAX_ITEMS],
    pub rename_cursor: usize,
    /// Görünüm modu (liste / ızgara).
    pub view_mode: ViewMode,
    /// Sıralama kriteri.
    pub sort_by: SortBy,
    /// Sıralama yönü.
    pub sort_order: SortOrder,
    /// Hızlı Önizleme paneli açık mı?
    pub preview_open: bool,
    pub preview_kind: PreviewKind,
    pub preview_title: TextBuf,
    pub preview_meta: TextBuf,
    pub preview_lines: [TextBuf; PREVIEW_LINES],
    pub preview_line_count: usize,
    preview_key: TextBuf,
    pub preview_has_img: bool,
    /// Live filename filter; backing file indices remain unchanged.
    pub search: [u8; 64],
    pub search_len: usize,
    /// Öğe indeksine göre çözümlenmiş küçük resimler (yalnız IMG dosyaları).
    thumbs: Vec<Option<Thumb>>,
    item_counter: usize,
    /// Açık onay iletişim kutusu türü.
    pub confirm_dialog: ConfirmDialog,
    /// Onay iletişim kutusunda seçili öğe indeksi.
    pub confirm_item_idx: usize,
    /// Onay iletişim kutusunda girilen isim baytları.
    pub confirm_name_buf: [u8; 64],
    pub confirm_name_len: usize,
    pub confirm_cursor: usize,
    /// Uzun işlem bandı: işlem adı (TR, EN) ve gerçek yüzde.
    /// `None` iken bant çizilmez.
    pub progress: Option<OpProgress>,
    /// Uzun işlemler için açılmış toast bildiriminin slotu. Sistem toast
    /// kuyruğu (görev çubuğu üstü) pencere içi bantla **aynı** gerçek
    /// yüzdeyi gösterir; böylece kullanıcı işlem ilerlemesini iki yerde de
    /// tutarlı görür.
    pub progress_toast: Option<usize>,
}

/// Dosya yöneticisi işlem bandı durumu.
///
/// Kopyalama/taşıma, silme, küçük resim üretimi ve boşaltma gibi uzun
/// işlemlerde pencere içinde **gerçek** ilerleme (yüzde + çubuk) gösterir;
/// kum saati imleci tek başına kullanıldığında kullanıcı ne kadar kaldığını
/// göremezdi.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpProgress {
    /// İşlem adı (TR, EN) — görünüm diline göre seçilir.
    pub label: (&'static str, &'static str),
    /// 0..=100 arası ilerleme yüzdesi.
    pub percent: u8,
    /// Toplam adım sayısı (`0` = bilinmiyor).
    pub total: usize,
    /// Tamamlanan adım sayısı.
    pub done: usize,
}

impl OpProgress {
    pub const fn new(label: (&'static str, &'static str), total: usize) -> Self {
        Self {
            label,
            percent: 0,
            total,
            done: 0,
        }
    }

    /// Adım ilerlemesini uygular; yüzde monoton olarak artar.
    pub fn set_steps(&mut self, done: usize, total: usize) {
        self.done = done.min(total.max(done));
        self.total = total;
        let pct = done
            .saturating_mul(100)
            .checked_div(total)
            .map_or(100, |p| p.min(100)) as u8;
        if pct > self.percent {
            self.percent = pct;
        }
    }

    /// Yüzdeyi doğrudan ayarlar (1..=99 beklenir; aşama geçişlerinde kullanılır).
    pub fn set_percent(&mut self, pct: u8) {
        let pct = pct.min(100);
        if pct > self.percent {
            self.percent = pct;
        }
    }

    /// Görünüm diline göre etiketi döndürür.
    pub fn label_str(&self) -> &'static str {
        if crate::gui::ui_english() {
            self.label.1
        } else {
            self.label.0
        }
    }

    /// "%42 (3/7)" biçiminde ilerleme metni.
    pub fn detail_text(&self) -> TextBuf {
        let mut b = TextBuf::new();
        b.push_usize(self.percent as usize);
        b.push_str("%");
        if self.total > 0 {
            b.push_str("  ");
            b.push_usize(self.done.min(self.total));
            b.push_str("/");
            b.push_usize(self.total);
        }
        b
    }
}

impl FileManagerApp {
    /// Kullanıcıya gösterilen sürükle-bırak hatalarını sınıflandırır; kararlı
    /// hata kodu, açıklama ve somut kurtarma adımı tek satırda birleştirilir.
    fn drop_error(kind: crate::apps::errors::AppErrorKind, detail: &str) -> String {
        let err = crate::apps::errors::AppError::new("Dosya Yöneticisi", kind);
        let mut message = String::from(err.kind.code());
        message.push_str(" · ");
        message.push_str(err.message(crate::gui::ui_english()));
        message.push(' ');
        message.push_str(detail);
        message.push(' ');
        message.push_str(err.recovery(crate::gui::ui_english()));
        message
    }

    /// Dynamically allocated thumbnail buffers (struct size is added by the
    /// runtime publisher via `size_of_val`).
    pub fn memory_usage_bytes(&self) -> usize {
        let thumbs = self
            .thumbs
            .iter()
            .filter_map(|thumb| thumb.as_ref())
            .map(|thumb| crate::apps::memory::vec_bytes(&thumb.px))
            .sum::<usize>();
        crate::apps::memory::vec_bytes(&self.thumbs).saturating_add(thumbs)
    }

    pub fn search_matches(&self, idx: usize) -> bool {
        if idx >= self.file_count {
            return false;
        }
        if self.search_len == 0 {
            return true;
        }
        let name = self.files[idx].display_name().as_bytes();
        let needle = &self.search[..self.search_len];
        name.windows(needle.len()).any(|window| {
            window
                .iter()
                .zip(needle)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
        })
    }

    pub fn visible_count(&self) -> usize {
        (0..self.file_count)
            .filter(|&idx| self.search_matches(idx))
            .count()
    }

    pub fn search_char(&mut self, ch: char) {
        let mut encoded = [0u8; 4];
        for &byte in ch.encode_utf8(&mut encoded).as_bytes() {
            if self.search_len < self.search.len() {
                self.search[self.search_len] = byte;
                self.search_len += 1;
            }
        }
        self.selected_items = [false; MAX_ITEMS];
        self.selected_idx = (0..self.file_count)
            .find(|&idx| self.search_matches(idx))
            .unwrap_or(0);
    }

    pub fn search_backspace(&mut self) {
        if self.search_len == 0 {
            return;
        }
        self.search_len -= 1;
        while self.search_len > 0 && self.search[self.search_len] & 0xC0 == 0x80 {
            self.search_len -= 1;
        }
        self.selected_items = [false; MAX_ITEMS];
        self.selected_idx = (0..self.file_count)
            .find(|&idx| self.search_matches(idx))
            .unwrap_or(0);
    }

    pub fn clear_search(&mut self) {
        self.search_len = 0;
        self.selected_items = [false; MAX_ITEMS];
        self.selected_idx = 0;
    }

    pub fn row_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        idx: usize,
    ) -> (usize, usize, usize, usize) {
        Self::row_rect_scrolled(win_x, win_y, win_w, idx, 0)
    }

    pub fn row_rect_scrolled(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        idx: usize,
        sy: usize,
    ) -> (usize, usize, usize, usize) {
        let sidebar_w = 110;
        let list_x = win_x + sidebar_w + 14;
        let list_w = win_w.saturating_sub(sidebar_w + 22 + scrollbar::SCROLLBAR_THICKNESS);
        let row_y = (win_y + LIST_TOP + idx * ROW_H).saturating_sub(sy);
        (list_x, row_y, list_w, ROW_H)
    }

    pub fn refresh_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(286), win_y + 32, 50, 22)
    }

    pub fn new_file_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(230), win_y + 32, 50, 22)
    }

    pub fn new_folder_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(174), win_y + 32, 54, 22)
    }

    pub fn delete_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(114), win_y + 32, 46, 22)
    }

    pub fn open_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(62), win_y + 32, 50, 22)
    }

    /// Görünüm değiştirme (Liste/Izgara) düğmesi.
    pub fn view_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(348), win_y + 32, 54, 22)
    }

    /// Hızlı Önizleme panelini aç/kapat düğmesi.
    pub fn preview_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(408), win_y + 32, 54, 22)
    }

    pub fn search_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        if win_w < 300 {
            // Küçük çözünürlüklerde arama kutusunu taşmayacak şekilde boyutlandır
            let w = win_w.saturating_sub(100).max(40);
            (win_x + 50, win_y + 35, w, 22)
        } else {
            (
                win_x + 122,
                win_y + 32,
                win_w.saturating_sub(540).max(72),
                22,
            )
        }
    }

    pub fn actions_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(40), win_y + 32, 28, 22)
    }

    /// "İşlemler" açılır menüsünün pencere içi dikdörtgeni.
    /// Düğmenin altına açılır; pencere dibine sığmazsa düğmenin üstüne çevrilir
    /// ve her durumda pencere sınırları içine kelepçelenir.
    pub fn actions_menu_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let w = 180usize.min(win_w.saturating_sub(16));
        let h = 176usize.min(win_h.saturating_sub(8));
        let btn = Self::actions_button_rect(win_x, win_y, win_w);
        let mut x = (win_x + win_w).saturating_sub(190).max(win_x + 8);
        let x_max = (win_x + win_w).saturating_sub(w + 8);
        if x > x_max {
            x = x_max;
        }
        let below = btn.1 + btn.3 + 4;
        let above = btn.1.saturating_sub(h + 4);
        let mut y = if below + h <= win_y + win_h {
            below
        } else {
            above
        };
        let y_min = win_y + 4;
        let y_max = (win_y + win_h).saturating_sub(h + 4);
        if y < y_min {
            y = y_min;
        } else if y > y_max && y_max >= y_min {
            y = y_max;
        }
        (x, y, w, h)
    }

    pub fn actions_menu_item_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        index: usize,
    ) -> (usize, usize, usize, usize) {
        let (x, y, w, _) = Self::actions_menu_rect(win_x, win_y, win_w, win_h);
        (x + 4, y + 4 + index * 24, w - 8, 22)
    }

    /// Sütun başlıkları dikdörtgenleri (isim, boyut, tür).
    pub fn header_name_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        let lx = Self::list_x(win_x);
        let lw = win_w.saturating_sub(110 + 22 + scrollbar::SCROLLBAR_THICKNESS);
        (lx, win_y + 66, lw.saturating_sub(120), 20)
    }

    pub fn header_size_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        let lx = Self::list_x(win_x);
        let lw = win_w.saturating_sub(110 + 22 + scrollbar::SCROLLBAR_THICKNESS);
        (lx + lw.saturating_sub(115), win_y + 66, 70, 20)
    }

    pub fn header_type_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        let lx = Self::list_x(win_x);
        let lw = win_w.saturating_sub(110 + 22 + scrollbar::SCROLLBAR_THICKNESS);
        (lx + lw.saturating_sub(40), win_y + 66, 40, 20)
    }

    /// Görünüm modunu değiştirir (Liste <-> Izgara).
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::List => ViewMode::Grid,
            ViewMode::Grid => ViewMode::List,
        };
    }

    /// Izgara görünümü/önizleme için küçük resimler hazır değilse üretir.
    pub fn ensure_thumbs(&mut self, vfs: &VirtualFileSystem) {
        // Küçük resim çözme (PNG/JPEG) uzun sürebilir; imleç kum saati olsun.
        // rebuild_thumbs zaten begin_progress/update_progress/finish_progress ile
        // gerçek ilerleme çubuğu gösterir; burada ayrı BusyScope gereksiz.
        if self.thumbs.len() != self.file_count {
            self.rebuild_thumbs(vfs);
        }
    }

    /// Hızlı Önizleme panelini aç/kapat.
    pub fn toggle_preview(&mut self) {
        self.preview_open = !self.preview_open;
        self.preview_key.clear();
        if !self.preview_open {
            self.preview_kind = PreviewKind::Empty;
            self.preview_line_count = 0;
        }
    }

    pub fn is_grid(&self) -> bool {
        self.view_mode == ViewMode::Grid
    }

    /// Sıralama kriterini döngüsel olarak değiştirir (İsim -> Boyut -> Tür -> İsim).
    pub fn cycle_sort_by(&mut self) {
        self.sort_by = match self.sort_by {
            SortBy::Name => SortBy::Size,
            SortBy::Size => SortBy::Type,
            SortBy::Type => SortBy::Name,
        };
        self.resort_files();
    }

    /// Sıralama yönünü tersine çevirir (A-Z <-> Z-A).
    pub fn toggle_sort_order(&mut self) {
        self.sort_order = match self.sort_order {
            SortOrder::Ascending => SortOrder::Descending,
            SortOrder::Descending => SortOrder::Ascending,
        };
        self.resort_files();
    }

    /// Mevcut sıralama kriterine göre dosya listesini yeniden sıralar.
    /// Geçerli sıralama ölçütüne göre dosya listesini yeniden sıralar.
    pub fn resort_files(&mut self) {
        let mut indices: [usize; MAX_ITEMS] = [0; MAX_ITEMS];
        for (i, slot) in indices[..self.file_count].iter_mut().enumerate() {
            *slot = i;
        }

        // Basit bubble sort (MAX_ITEMS küçük olduğu için yeterli)
        for i in 0..self.file_count.saturating_sub(1) {
            for j in (i + 1)..self.file_count {
                let should_swap = match self.sort_by {
                    SortBy::Name => {
                        let a = self.files[indices[i]].display_name();
                        let b = self.files[indices[j]].display_name();
                        if self.sort_order == SortOrder::Ascending {
                            a > b
                        } else {
                            a < b
                        }
                    }
                    SortBy::Size => {
                        let a = parse_size(self.files[indices[i]].size_str.as_str());
                        let b = parse_size(self.files[indices[j]].size_str.as_str());
                        if self.sort_order == SortOrder::Ascending {
                            a > b
                        } else {
                            a < b
                        }
                    }
                    SortBy::Type => {
                        let a = self.files[indices[i]].file_type;
                        let b = self.files[indices[j]].file_type;
                        if a == b {
                            let name_a = self.files[indices[i]].display_name();
                            let name_b = self.files[indices[j]].display_name();
                            if self.sort_order == SortOrder::Ascending {
                                name_a > name_b
                            } else {
                                name_a < name_b
                            }
                        } else if self.sort_order == SortOrder::Ascending {
                            a > b
                        } else {
                            a < b
                        }
                    }
                };
                if should_swap {
                    indices.swap(i, j);
                }
            }
        }

        // Diziyi yeniden düzenle (üst dizin .. her zaman en üstte kalmalı)
        let mut new_files = [FileItem::empty(); MAX_ITEMS];
        let mut write_idx = 0usize;

        // .. öğesini en başa al
        for i in 0..self.file_count {
            if self.files[indices[i]].display_name() == ".." {
                new_files[write_idx] = self.files[indices[i]];
                write_idx += 1;
                break;
            }
        }

        // Klasörleri sonra dosyaları ekle
        for &idx in indices[..self.file_count].iter() {
            if self.files[idx].display_name() != ".." && self.files[idx].is_dir {
                new_files[write_idx] = self.files[idx];
                write_idx += 1;
            }
        }

        for &idx in indices[..self.file_count].iter() {
            if !self.files[idx].is_dir && self.files[idx].display_name() != ".." {
                new_files[write_idx] = self.files[idx];
                write_idx += 1;
            }
        }

        self.files = new_files;
        // Seçimi koru
        if self.selected_idx < self.file_count {
            self.selected_idx = 0;
        }
    }

    /// Izgaradaki sütun sayısı (liste genişliğine göre).
    fn grid_columns(list_w: usize) -> usize {
        (list_w / GRID_CELL_W).max(1)
    }

    /// Liste alanının sol X konumu (pencereye göre).
    pub fn list_x(win_x: usize) -> usize {
        win_x + 110 + 14
    }

    /// Liste alanının genişliği. Önizleme paneli açıkken daralır.
    pub fn list_width(&self, win_w: usize) -> usize {
        let w = win_w.saturating_sub(110 + 22 + scrollbar::SCROLLBAR_THICKNESS);
        if self.preview_open {
            w.saturating_sub(PREVIEW_W + 8)
        } else {
            w
        }
    }

    /// Hızlı Önizleme panelinin ekran koordinatları (x, y, w, h).
    pub fn preview_rect(
        &self,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let thick = scrollbar::SCROLLBAR_THICKNESS;
        let w = PREVIEW_W.min(win_w.saturating_sub(150));
        let x = win_x + win_w.saturating_sub(thick + 14 + w);
        let y = win_y + 60;
        let h = win_h.saturating_sub(60 + status_h());
        (x, y, w, h)
    }

    /// Bir öğenin ekran üzerindeki dikdörtgeni (geçerli görünüm moduna göre).
    pub fn item_rect(
        &self,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        idx: usize,
        sy: usize,
    ) -> (usize, usize, usize, usize) {
        let lx = Self::list_x(win_x);
        let lw = self.list_width(win_w);
        let top = win_y + LIST_TOP;
        match self.view_mode {
            ViewMode::List => (lx, (top + idx * ROW_H).saturating_sub(sy), lw, ROW_H),
            ViewMode::Grid => {
                let cols = Self::grid_columns(lw);
                let col = idx % cols;
                let row = idx / cols;
                (
                    lx + col * GRID_CELL_W,
                    (top + row * GRID_CELL_H).saturating_sub(sy),
                    GRID_CELL_W,
                    GRID_CELL_H,
                )
            }
        }
    }

    /// Fare konumundaki öğe indeksini döndürür (liste/ızgara bağımsız).
    pub fn hit_test(
        &self,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        mx: usize,
        my: usize,
        sy: usize,
    ) -> Option<usize> {
        let lx = Self::list_x(win_x);
        let lw = self.list_width(win_w);
        let top = win_y + LIST_TOP;
        let bottom = win_y + win_h.saturating_sub(status_h());
        if mx < lx || mx >= lx + lw || my < top || my >= bottom {
            return None;
        }
        let mut visible_idx = 0;
        for i in 0..self.file_count {
            if !self.search_matches(i) {
                continue;
            }
            if crate::gui::in_rect(mx, my, self.item_rect(win_x, win_y, win_w, visible_idx, sy)) {
                return Some(i);
            }
            visible_idx += 1;
        }
        None
    }

    /// İçeriğin toplam yüksekliği (kaydırma için).
    pub fn content_height(&self, win_w: usize) -> usize {
        let visible = self.visible_count();
        match self.view_mode {
            ViewMode::List => visible * ROW_H,
            ViewMode::Grid => {
                let cols = Self::grid_columns(self.list_width(win_w));
                let rows = visible.div_ceil(cols);
                rows * GRID_CELL_H
            }
        }
    }

    /// Bir sayfaya sığan öğe sayısı (PageUp/PageDown adımı).
    pub fn page_step(&self, win_w: usize, view_h: usize) -> usize {
        match self.view_mode {
            ViewMode::List => (view_h / ROW_H).max(1),
            ViewMode::Grid => {
                let cols = Self::grid_columns(self.list_width(win_w));
                (view_h / GRID_CELL_H).max(1) * cols
            }
        }
    }

    pub fn focus_next_control(&mut self) {
        self.focused_control = (self.focused_control + 1) % 7;
    }

    /// Shift+Tab ile bir önceki denetime gider.
    pub fn focus_prev_control(&mut self) {
        self.focused_control = (self.focused_control + 6) % 7;
    }

    pub fn select_prev(&mut self) {
        if let Some(prev) = (0..self.selected_idx)
            .rev()
            .find(|&i| self.search_matches(i))
        {
            self.selected_idx = prev;
        }
    }

    pub fn select_next(&mut self) {
        if let Some(next) =
            (self.selected_idx + 1..self.file_count).find(|&i| self.search_matches(i))
        {
            self.selected_idx = next;
        }
    }

    /// İmleci hedef öğeye taşır ve seçimi `mod` değiştiricisine göre uygular.
    ///
    /// * Shift: çapa ile hedef arasındaki görünür öğeler seçilir (aralık).
    /// * Ctrl: seçim değişmez, yalnızca imleç taşınır.
    /// * Ctrl+Shift: imleç taşınır ve hedef öğe seçime eklenir/çıkarılır.
    /// * Yok: yalnızca hedef öğe seçili olur.
    ///
    /// `..` (üst dizin) öğesi hiçbir zaman seçime girmez.
    pub fn move_cursor_to(&mut self, target: usize, mods: SelectMod) {
        if target >= self.file_count || !self.search_matches(target) {
            return;
        }
        let is_up = self.files[target].display_name() == "..";
        match mods {
            SelectMod::None => {
                self.selected_items = [false; MAX_ITEMS];
                if !is_up {
                    self.selected_items[target] = true;
                }
                self.selection_anchor = target;
            }
            SelectMod::Shift => {
                self.selected_items = [false; MAX_ITEMS];
                let anchor = self.selection_anchor.min(self.file_count.saturating_sub(1));
                let start = anchor.min(target);
                let end = anchor.max(target);
                for i in start..=end {
                    if self.search_matches(i) && self.files[i].display_name() != ".." {
                        self.selected_items[i] = true;
                    }
                }
            }
            SelectMod::Ctrl => {}
            SelectMod::CtrlShift => {
                if !is_up {
                    self.selected_items[target] = !self.selected_items[target];
                }
            }
        }
        self.selected_idx = target;
    }

    /// Aralık için bir sonraki görünür öğe indeksini arar (`step` negatifse geri).
    fn search_index(&self, from: usize, step: isize) -> Option<usize> {
        if self.file_count == 0 {
            return None;
        }
        let mut i = from as isize + step;
        while i >= 0 && (i as usize) < self.file_count {
            if self.search_matches(i as usize) {
                return Some(i as usize);
            }
            i += step;
        }
        None
    }

    /// Yukarı ok: listede bir öğe, ızgarada bir satır (sütun kadar) yukarı.
    pub fn move_selection_up(&mut self, win_w: usize) {
        self.move_selection_up_ext(win_w, SelectMod::None);
    }

    /// Yukarı ok + değiştiriciler.
    pub fn move_selection_up_ext(&mut self, win_w: usize, mods: SelectMod) {
        if self.visible_count() == 0 {
            return;
        }
        let step = match self.view_mode {
            ViewMode::List => 1,
            ViewMode::Grid => Self::grid_columns(self.list_width(win_w)) as isize,
        };
        if let Some(target) = self.search_index(self.selected_idx, -step.max(1)) {
            self.move_cursor_to(target, mods);
        } else if self.selected_idx >= self.file_count {
            // Seçim yoksa ilk görünür öğeye git.
            if let Some(first) = (0..self.file_count).find(|&i| self.search_matches(i)) {
                self.move_cursor_to(first, mods);
            }
        }
    }

    /// Aşağı ok: listede bir öğe, ızgarada bir satır (sütun kadar) aşağı.
    pub fn move_selection_down(&mut self, win_w: usize) {
        self.move_selection_down_ext(win_w, SelectMod::None);
    }

    /// Aşağı ok + değiştiriciler.
    pub fn move_selection_down_ext(&mut self, win_w: usize, mods: SelectMod) {
        if self.visible_count() == 0 {
            return;
        }
        let step = match self.view_mode {
            ViewMode::List => 1,
            ViewMode::Grid => Self::grid_columns(self.list_width(win_w)) as isize,
        };
        if let Some(target) = self.search_index(self.selected_idx, step.max(1)) {
            self.move_cursor_to(target, mods);
        }
    }

    /// Sol ok: yalnızca ızgarada bir satırın başındaki öğeyi seçer.
    pub fn move_selection_left(&mut self, win_w: usize) {
        self.move_selection_left_ext(win_w, SelectMod::None);
    }

    /// Sol ok + değiştiriciler. Izgarada satır başındaki öğeye döner.
    pub fn move_selection_left_ext(&mut self, win_w: usize, mods: SelectMod) {
        if self.view_mode != ViewMode::Grid {
            return;
        }
        let cols = Self::grid_columns(self.list_width(win_w)).max(1);
        let row_start = self.selected_idx.saturating_sub(self.selected_idx % cols);
        if row_start != self.selected_idx {
            self.move_cursor_to(row_start, mods);
        }
    }

    /// Sağ ok: yalnızca ızgarada sonraki hücre.
    pub fn move_selection_right(&mut self, win_w: usize) {
        self.move_selection_right_ext(win_w, SelectMod::None);
    }

    /// Sağ ok + değiştiriciler. Izgarada satır sonundaki öğeye gider.
    pub fn move_selection_right_ext(&mut self, win_w: usize, mods: SelectMod) {
        if self.view_mode != ViewMode::Grid {
            return;
        }
        let cols = Self::grid_columns(self.list_width(win_w)).max(1);
        let row_start = self.selected_idx.saturating_sub(self.selected_idx % cols);
        let row_end = (row_start + cols - 1).min(self.file_count.saturating_sub(1));
        if row_end != self.selected_idx {
            self.move_cursor_to(row_end, mods);
        }
    }

    /// Tüm öğeleri seçer (Ctrl+A). `..` öğesi hariç.
    pub fn select_all(&mut self) -> usize {
        self.selected_items = [false; MAX_ITEMS];
        let mut n = 0usize;
        for i in 0..self.file_count {
            if self.search_matches(i) && self.files[i].display_name() != ".." {
                self.selected_items[i] = true;
                n += 1;
            }
        }
        n
    }

    /// Seçimi temizler (Ctrl+Shift+A / Esc). Dönen değer önceden seçili miydi?
    pub fn clear_selection(&mut self) -> bool {
        let had = self.selected_items.iter().any(|&s| s);
        self.selected_items = [false; MAX_ITEMS];
        had
    }

    /// Mevcut seçimdeki (seçili) öğe sayısı.
    pub fn selected_count(&self) -> usize {
        self.selected_items.iter().filter(|&&s| s).count()
    }

    /// Seçili öğeyi görünür tutacak kaydırma konusunu döndürür.
    /// Hem liste hem ızgara yerleşimine göre hesaplar.
    pub fn ensure_selection_visible(&self, win_w: usize, view_h: usize, scroll_y: usize) -> usize {
        if view_h == 0 || self.visible_count() == 0 {
            return scroll_y;
        }
        let max_off = self.content_height(win_w).saturating_sub(view_h);
        let visible_idx = (0..self.selected_idx)
            .filter(|&idx| self.search_matches(idx))
            .count();
        let (item_top, item_h) = match self.view_mode {
            ViewMode::List => (visible_idx * ROW_H, ROW_H),
            ViewMode::Grid => {
                let cols = Self::grid_columns(self.list_width(win_w));
                let row = visible_idx / cols;
                (row * GRID_CELL_H, GRID_CELL_H)
            }
        };
        let item_bottom = item_top + item_h;
        let mut sy = scroll_y.min(max_off);
        if item_top < sy {
            sy = item_top;
        } else if item_bottom > sy + view_h {
            sy = item_bottom.saturating_sub(view_h);
        }
        sy.min(max_off)
    }

    /// Satır yüksekliği (px) — kaydırma hesapları için.
    pub fn row_h() -> usize {
        ROW_H
    }

    /// Listenin ilk öğesini seçer (Home).
    pub fn select_first(&mut self) {
        self.select_first_ext(SelectMod::None);
    }

    /// Home + değiştiriciler.
    pub fn select_first_ext(&mut self, mods: SelectMod) {
        if let Some(first) = (0..self.file_count).find(|&i| self.search_matches(i)) {
            self.move_cursor_to(first, mods);
        }
    }

    /// Listenin son öğesini seçer (End).
    pub fn select_last(&mut self) {
        self.select_last_ext(SelectMod::None);
    }

    /// End + değiştiriciler.
    pub fn select_last_ext(&mut self, mods: SelectMod) {
        if let Some(last) = (0..self.file_count).rev().find(|&i| self.search_matches(i)) {
            self.move_cursor_to(last, mods);
        }
    }

    /// Sayfa sayfa gezinme (PageUp / PageDown).
    pub fn select_page(&mut self, down: bool, rows: usize) {
        self.select_page_ext(down, rows, SelectMod::None);
    }

    /// PageUp / PageDown + değiştiriciler.
    ///
    /// Sayfa adımı, listenin ucuna bir sayfadan yakın olduğunda o uca atlar
    /// (Windows Gezgini davranışı): son sayfada PageDown → son öğe, ilk
    /// sayfada PageUp → ilk öğe.
    pub fn select_page_ext(&mut self, down: bool, rows: usize, mods: SelectMod) {
        if self.visible_count() == 0 {
            return;
        }
        let step = rows.max(1) as isize;
        let first = (0..self.file_count).find(|&i| self.search_matches(i));
        let last = (0..self.file_count).rev().find(|&i| self.search_matches(i));
        let target = if down {
            match self.search_index(self.selected_idx, step) {
                Some(t) => Some(t),
                // Son sayfa: tek adım bile kalmasa da son öğeye git.
                None => last,
            }
        } else {
            match self.search_index(self.selected_idx, -step) {
                Some(t) => Some(t),
                // İlk sayfa: tek adım bile kalmadıysa ilk öğeye git.
                None => first,
            }
        };
        if let Some(t) = target {
            self.move_cursor_to(t, mods);
        }
    }

    pub const fn new() -> Self {
        Self {
            current_path: "/root/Documents",
            path_buf: [0; 96],
            path_len: 0,
            selected_idx: 0,
            selected_items: [false; MAX_ITEMS],
            selection_anchor: 0,
            files: [FileItem::empty(); MAX_ITEMS],
            file_count: 0,
            source: Source::Vfs,
            disk_kind: FsKind::None,
            status: "",
            status_buf: TextBuf::new(),
            rename_active: false,
            rename_buf: [0; diskfs::NAME_MAX],
            rename_len: 0,
            focused_control: 5,
            actions_menu_open: false,
            last_click_idx: None,
            last_click_ms: 0,
            drag_item: None,
            drag_start: (0, 0),
            drag_active: false,
            drag_target: None,
            drag_items: [false; MAX_ITEMS],
            rename_cursor: 0,
            view_mode: ViewMode::List,
            sort_by: SortBy::Name,
            sort_order: SortOrder::Ascending,
            preview_open: false,
            preview_kind: PreviewKind::Empty,
            preview_title: TextBuf::new(),
            preview_meta: TextBuf::new(),
            preview_lines: [TextBuf::new(); PREVIEW_LINES],
            preview_line_count: 0,
            preview_key: TextBuf::new(),
            preview_has_img: false,
            search: [0; 64],
            search_len: 0,
            thumbs: Vec::new(),
            item_counter: 1,
            confirm_dialog: ConfirmDialog::None,
            confirm_item_idx: 0,
            confirm_name_buf: [0; 64],
            confirm_name_len: 0,
            confirm_cursor: 0,
            progress: None,
            progress_toast: None,
        }
    }

    pub fn set_status_text(&mut self, text: &str) {
        self.status = "";
        self.status_buf.clear();
        self.status_buf.push_str(text);
    }

    pub fn path(&self) -> &str {
        if self.path_len > 0 {
            core::str::from_utf8(&self.path_buf[..self.path_len]).unwrap_or(self.current_path)
        } else {
            self.current_path
        }
    }

    pub fn set_path(&mut self, p: &str) {
        self.selected_items = [false; MAX_ITEMS];
        self.last_click_idx = None;
        self.drag_item = None;
        self.drag_active = false;
        self.drag_target = None;
        self.status = "";
        self.status_buf.clear();
        self.path_len = 0;
        for b in p.bytes() {
            if self.path_len < self.path_buf.len() {
                self.path_buf[self.path_len] = b;
                self.path_len += 1;
            }
        }
        self.current_path = match p {
            "/" => "/",
            "/root" => "/root",
            "/root/Documents" => "/root/Documents",
            "/root/Media" => "/root/Media",
            "/root/Downloads" => "/root/Downloads",
            "/boot" => "/boot",
            "/media/usb0" => "/media/usb0",
            "/media/hdd0" => "/media/hdd0",
            "/media/cdrom" => "/media/cdrom",
            "/root/Trash" => "/root/Trash",
            _ => "/root/Documents",
        };
    }

    /// VFS'ten mevcut dizinin içeriğini okur ve dosya listesini doldurur.
    pub fn refresh_from_vfs(&mut self, vfs: &VirtualFileSystem) {
        self.selected_items = [false; MAX_ITEMS];
        self.last_click_idx = None;
        self.drag_item = None;
        self.drag_active = false;
        self.drag_target = None;
        let path = String::from(self.path());
        let is_not_root = path != "/";

        // Özel sürücü durumlarını kontrol et
        let is_usb = path == "/media/usb0";
        let is_cdrom = path == "/media/cdrom";
        let is_hdd = path == "/media/hdd0";
        let is_boot = path == "/boot";

        // Sürücü mevcut mu kontrol et
        let usb_available = crate::fs::diskfs::usb_available();
        let cd_available = crate::fs::diskfs::cd_available();
        let hdd_available = crate::drivers::ata::drive_available(0);

        let items = if is_hdd && !hdd_available {
            Vec::new()
        } else {
            vfs.list_dir(&path)
        };

        self.file_count = 0;
        self.selected_idx = 0;

        // Kök dizinde değilsek üst dizine dönme girdisi ekle [..].
        // Masaüstünde (/desktop) üst klasör kavramı yoktur: ".." simgesi
        // çizilmesin ve "/desktop/.." gibi geçersiz bir yol açılmasın.
        if is_not_root && path != "/desktop" {
            let mut up = FileItem::empty();
            let up_name = b"..";
            up.dyn_name[..2].copy_from_slice(up_name);
            up.dyn_name_len = 2;
            up.is_dir = true;
            up.icon_tag = "DIR";
            up.file_type = crate::gui::tr(crate::gui::I18nKey::ParentFolder);
            let mut s = TextBuf::new();
            s.push_str("<UP>");
            up.size_str = s;
            self.files[self.file_count] = up;
            self.file_count += 1;
        }

        // Sürücü durumu mesajları
        if is_usb && !usb_available {
            self.status = crate::gui::ui_text(
                "⚠ USB disk bağlı değil. Lütfen bir USB sürücü takın.",
                "⚠ USB drive not connected. Please insert a USB drive.",
            );
        } else if is_cdrom && !cd_available {
            self.status = crate::gui::ui_text(
                "⚠ CD-ROM sürücüsünde disk yok. Lütfen bir disk yerleştirin.",
                "⚠ No disc in CD-ROM drive. Please insert a disc.",
            );
        } else if is_hdd && !hdd_available {
            self.status = crate::apps::errors::AppErrorKind::StorageUnavailable
                .message(crate::gui::ui_english());
        }
        if is_boot && items.is_empty() {
            self.status = crate::gui::ui_text(
                "ℹ /boot bölümü boş. Sistem dosyaları burada depolanır.",
                "ℹ /boot partition is empty. System files are stored here.",
            );
        } else if items.is_empty() && !is_usb && !is_cdrom && !is_hdd && !is_boot {
            self.status = crate::gui::ui_text(
                "📁 Bu klasör boş. Yeni dosya/klasör oluşturmak için 'Yeni' menüsünü kullanın.",
                "📁 This folder is empty. Use 'New' menu to create files/folders.",
            );
        }

        // Önce klasörler
        for it in items.iter().filter(|i| i.node_type == NodeType::Directory) {
            if self.file_count >= MAX_ITEMS {
                break;
            }
            let mut row = FileItem::empty();
            let name_bytes = it.name.as_bytes();
            let take = name_bytes.len().min(diskfs::NAME_MAX);
            row.dyn_name[..take].copy_from_slice(&name_bytes[..take]);
            row.dyn_name_len = take;
            row.is_dir = true;
            row.icon_tag = "DIR";
            row.file_type = crate::gui::tr(crate::gui::I18nKey::Folder);
            row.size_str = format_size(0, true);
            self.files[self.file_count] = row;
            self.file_count += 1;
        }

        // Sonra dosyalar
        for it in items.iter().filter(|i| i.node_type == NodeType::File) {
            if self.file_count >= MAX_ITEMS {
                break;
            }
            let mut row = FileItem::empty();
            let name_bytes = it.name.as_bytes();
            let take = name_bytes.len().min(diskfs::NAME_MAX);
            row.dyn_name[..take].copy_from_slice(&name_bytes[..take]);
            row.dyn_name_len = take;
            row.is_dir = false;
            row.icon_tag = file_kind(&it.name);
            row.file_type = match row.icon_tag {
                "TXT" => crate::gui::ui_text("Metin Belgesi", "Text Document"),
                "3D" => crate::gui::ui_text("3D Model", "3D Model"),
                "IMG" => crate::gui::tr(crate::gui::I18nKey::ImageFile),
                "SND" => crate::gui::ui_text("Ses Dosyası", "Audio"),
                "VID" => crate::gui::ui_text("Video Kaydı", "Video"),
                "WEB" => crate::gui::tr(crate::gui::I18nKey::WebPage),
                "CFG" => crate::gui::ui_text("Ayar Dosyası", "Config"),
                "LOG" => crate::gui::tr(crate::gui::I18nKey::SystemLog),
                _ => crate::gui::ui_text("Dosya", "File"),
            };
            row.size_str = format_size(it.size, false);
            self.files[self.file_count] = row;
            self.file_count += 1;
        }

        // Izgara / önizleme için gerekliyse küçük resimleri (thumbnail) bir kez
        // (liste tamamlandıktan sonra) tazele. Eskiden bu çağrı her dosya için
        // yapılıyordu; liste O(n²) kez yeniden çözülüyordu.
        self.thumbs.clear();
        if self.view_mode == ViewMode::Grid || self.preview_open {
            self.rebuild_thumbs(vfs);
        }
        self.preview_key.clear();
        if self.preview_open {
            self.update_preview(vfs);
        }

        // Mevcut sıralama kriterine göre dosyaları sırala
        self.resort_files();

        self.status = "";
    }

    /// Görünür görsel öğeler için küçük resimleri (thumbnail) çözer.
    ///
    /// PNG/JPEG çözúmú uzun sürebilir; işlem süresince pencere içinde gerçek
    /// ilerleme (işlenen görsel / toplam görsel) gösterilir.
    fn rebuild_thumbs(&mut self, vfs: &VirtualFileSystem) {
        self.thumbs.clear();
        let mut budget = 48usize;
        let base = String::from(self.path());
        // İlerleme yalnızca çözúlecek görsel varsa başlat.
        let image_total = (0..self.file_count)
            .filter(|&i| !self.files[i].is_dir && self.files[i].icon_tag == "IMG")
            .count();
        let mut image_done = 0usize;
        if image_total > 0 {
            self.begin_progress(("Küçük resimler", "Thumbnails"), image_total, 0);
        }
        for i in 0..self.file_count {
            let file = self.files[i];
            let is_img = !file.is_dir && file.icon_tag == "IMG";
            if !is_img || budget == 0 {
                self.thumbs.push(None);
                continue;
            }
            let mut full = base.clone();
            if !full.ends_with('/') {
                full.push('/');
            }
            full.push_str(file.display_name());
            let thumb = vfs.read_file(&full).and_then(|d| {
                if d.len() > THUMB_MAX_BYTES {
                    None
                } else {
                    Thumb::from_image(d)
                }
            });
            if thumb.is_some() {
                budget -= 1;
            }
            self.thumbs.push(thumb);
            image_done += 1;
            self.update_progress(image_done, image_total);
        }
        if image_total > 0 {
            self.finish_progress();
        }
    }

    /// Hızlı Önizleme panelinin içeriğini seçili öğeye göre yeniler.
    /// Yalnızca seçim/konum değiştiğinde gerçekten çalışır (önbellek anahtarı).
    /// Değişiklik olduysa `true` döner.
    pub fn update_preview(&mut self, vfs: &VirtualFileSystem) -> bool {
        if !self.preview_open {
            return false;
        }
        let mut key = TextBuf::new();
        key.push_str(self.path());
        if self.selected_idx < self.file_count {
            key.push_byte(b'/');
            key.push_str(self.files[self.selected_idx].display_name());
        }
        if key.as_str() == self.preview_key.as_str() {
            return false;
        }
        self.preview_key = key;
        self.preview_line_count = 0;
        self.preview_has_img = false;
        self.preview_title = TextBuf::new();
        self.preview_meta = TextBuf::new();
        for l in self.preview_lines.iter_mut() {
            *l = TextBuf::new();
        }

        if self.selected_idx >= self.file_count {
            self.preview_kind = PreviewKind::Empty;
            return true;
        }
        let item = self.files[self.selected_idx];
        let name = item.display_name();
        if name == ".." {
            self.preview_kind = PreviewKind::Empty;
            return true;
        }
        self.preview_title.push_str(name);
        let full = self.selected_full_path();

        if item.is_dir {
            self.preview_kind = PreviewKind::Directory;
            let entries = vfs.list_dir(&full);
            let mut meta = TextBuf::new();
            meta.push_usize(entries.len());
            meta.push_str(crate::gui::ui_text(" öğe", " items"));
            self.preview_meta = meta;
            let n = entries.len().min(PREVIEW_LINES);
            for k in 0..n {
                self.preview_lines[k].push_str(entries[k].name.as_str());
            }
            self.preview_line_count = n;
        } else {
            let mut meta = TextBuf::new();
            meta.push_str(item.file_type);
            meta.push_str(" - ");
            meta.push_str(item.size_str.as_str());
            self.preview_meta = meta;
            match item.icon_tag {
                "TXT" | "CFG" | "LOG" => {
                    self.preview_kind = PreviewKind::Text;
                    if let Some(data) = vfs.read_file(&full) {
                        let mut start = 0usize;
                        let mut i = 0usize;
                        while i <= data.len() && self.preview_line_count < PREVIEW_LINES {
                            if i == data.len() || data[i] == b'\n' {
                                let mut line = &data[start..i];
                                if line.last() == Some(&b'\r') {
                                    line = &line[..line.len() - 1];
                                }
                                if let Ok(s) = core::str::from_utf8(line) {
                                    self.preview_lines[self.preview_line_count].push_str(s);
                                }
                                self.preview_line_count += 1;
                                start = i + 1;
                            }
                            i += 1;
                        }
                    }
                }
                "IMG" => {
                    self.preview_kind = PreviewKind::Image;
                    // Önizleme sonradan (Space ile) açıldıysa küçük resim
                    // önbelleği henüz boş olabilir; gerekiyorsa üret.
                    if self
                        .thumbs
                        .get(self.selected_idx)
                        .and_then(|t| t.as_ref())
                        .is_none()
                    {
                        self.rebuild_thumbs(vfs);
                    }
                    self.preview_has_img = self
                        .thumbs
                        .get(self.selected_idx)
                        .and_then(|t| t.as_ref())
                        .is_some();
                }
                _ => {
                    self.preview_kind = PreviewKind::File;
                }
            }
        }
        true
    }

    /// Küçük resmi ölçekli olarak çizer (merkezi `cx`, üst `cy`).
    fn draw_thumb(
        fb: &mut Framebuffer,
        cx: usize,
        cy: usize,
        box_w: usize,
        box_h: usize,
        thumb: &Thumb,
    ) {
        if thumb.w == 0 || thumb.h == 0 {
            return;
        }
        let scale = (box_w / thumb.w).min(box_h / thumb.h).max(1);
        let dw = thumb.w * scale;
        let dh = thumb.h * scale;
        let ox = cx + box_w.saturating_sub(dw) / 2;
        let oy = cy + box_h.saturating_sub(dh) / 2;
        fb.draw_rect(ox - 1, oy - 1, dw + 2, dh + 2, Color::BORDER);
        for y in 0..thumb.h {
            for x in 0..thumb.w {
                let c = thumb.px[y * thumb.w + x];
                fb.draw_rect(ox + x * scale, oy + y * scale, scale, scale, c);
            }
        }
    }

    pub fn select_row(&mut self, idx: usize, ctrl: bool, shift: bool) {
        if idx >= self.file_count || !self.search_matches(idx) {
            return;
        }
        if self.files[idx].display_name() == ".." {
            self.selected_items = [false; MAX_ITEMS];
            self.selected_idx = idx;
            return;
        }
        // Klavye ile aynı seçim kuralları (tek kod yolu).
        let mods = SelectMod::from_keys(shift, ctrl);
        if mods == SelectMod::Ctrl {
            // Ctrl+tık: öğeyi seçime ekle/çıkar.
            self.selected_items[idx] = !self.selected_items[idx];
            self.selection_anchor = idx;
            self.selected_idx = idx;
        } else {
            self.move_cursor_to(idx, mods);
        }
    }

    /// Öğe seçili mi?
    ///
    /// Seçim durumu `selected_items` üzerinden okunur; imleç (odak) satırı
    /// ayrıca odak halkası ile işaretlenir (`is_cursor_row`). Böylece Ctrl
    /// ile seçimden çıkarılan bir öğe, imleç orada olsa da seçili görünmez.
    pub fn is_row_selected(&self, idx: usize) -> bool {
        self.selected_items.get(idx).copied().unwrap_or(false)
    }

    /// İmlecin (klavye odağının) üzerinde olduğu satır mı?
    pub fn is_cursor_row(&self, idx: usize) -> bool {
        idx == self.selected_idx
    }

    /// Klavye odağının üzerindeki öğe; listenin boş olması hâlinde `None`.
    pub fn cursor_item(&self) -> Option<&FileItem> {
        if self.selected_idx < self.file_count {
            Some(&self.files[self.selected_idx])
        } else {
            None
        }
    }

    pub fn copy_selection(&mut self, clipboard: &mut FileClipboard, cut: bool) {
        if !self.selected_items.iter().any(|&s| s) {
            if self.selected_idx >= self.file_count {
                return;
            }
            self.selected_items[self.selected_idx] = true;
        }
        clipboard.count = 0;
        let current = String::from(self.path());
        for i in 0..self.file_count {
            if !self.selected_items[i] || self.files[i].display_name() == ".." {
                continue;
            }
            let mut path = current.clone();
            if !path.ends_with('/') {
                path.push('/');
            }
            path.push_str(self.files[i].display_name());
            let mut saved = TextBuf::new();
            saved.push_str(path.as_str());
            clipboard.paths[clipboard.count] = saved;
            clipboard.count += 1;
        }
        clipboard.cut = cut;
        self.status = if cut {
            crate::gui::ui_text("Taşımak için seçildi.", "Selected to move.")
        } else {
            crate::gui::ui_text("Kopyalamak için seçildi.", "Selected to copy.")
        };
    }

    pub fn paste_selection(
        &mut self,
        clipboard: &mut FileClipboard,
        vfs: &mut VirtualFileSystem,
    ) -> usize {
        let dest = String::from(self.path());
        if dest == "/media/cdrom" {
            self.status = crate::gui::ui_text("CD-ROM salt okunurdur.", "CD-ROM is read-only.");
            return 0;
        }

        let total_files = clipboard.count;
        if total_files == 0 {
            self.status = crate::gui::tr(crate::gui::I18nKey::NothingToPaste);
            return 0;
        }
        // Kopyalama/kesme-taşıma disk I/O'su içerebilir; pencere içinde gerçek
        // ilerleme bandı gösterilir (begin_progress/update_progress/finish_progress).
        let mut tracker = crate::apps::file_ops::ProgressTracker::new(total_files);
        let was_cut = clipboard.cut;
        let label = if was_cut {
            ("Taşınıyor", "Moving")
        } else {
            ("Kopyalanıyor", "Copying")
        };
        self.begin_progress(label, total_files, 0);

        let mut done = 0;
        let mut last_pct = 0usize;
        let mut failed: Option<String> = None;

        for path in clipboard.paths.iter().take(clipboard.count) {
            let src = String::from(path.as_str());
            let result = if was_cut {
                vfs.move_item(&src, &dest).map(|_| ())
            } else {
                vfs.copy_item(&src, &dest).map(|_| ())
            };

            if result.is_ok() {
                done += 1;
            }
            let pct = tracker.step();
            // Her öğede yüzde değiştiyse bandı güncelle (gerçek ilerleme).
            if pct != last_pct {
                last_pct = pct;
                self.update_progress(done, total_files);
            }
            if result.is_err() {
                failed = Some(src);
                break;
            }
        }

        if was_cut && done > 0 {
            clipboard.count = 0;
            clipboard.cut = false;
        }
        self.refresh_from_vfs(vfs);
        self.finish_progress();
        if let Some(failed_path) = failed {
            if (dest == "/media/hdd0" || dest.starts_with("/media/hdd0/"))
                && !crate::drivers::ata::drive_available(0)
            {
                self.status = crate::apps::errors::AppErrorKind::StorageUnavailable
                    .message(crate::gui::ui_english());
                return done;
            }
            let name = failed_path
                .rsplit('/')
                .next()
                .unwrap_or(failed_path.as_str());
            let mut msg = TextBuf::new();
            msg.push_str(crate::gui::ui_text("Kopyalama hatası: ", "Copy failed: "));
            msg.push_str(name);
            self.set_status_text(msg.as_str());
            return done;
        }
        self.status = if done > 0 {
            if was_cut {
                crate::gui::ui_text("Tamamlandı.", "Completed.")
            } else {
                crate::gui::tr(crate::gui::I18nKey::PasteCompleted)
            }
        } else {
            crate::gui::tr(crate::gui::I18nKey::NothingToPaste)
        };
        done
    }

    /// İşlem bandını başlatır (başlangıç yüzdesi ile).
    pub fn begin_progress(&mut self, label: (&'static str, &'static str), total: usize, start: u8) {
        let mut p = OpProgress::new(label, total);
        p.set_percent(start);
        self.progress = Some(p);
    }

    /// İşlem bandını başlatır ve varsa sistem toast kuyruğunda da eşzamanlı
    /// bir ilerleme bildirimi açar. İki gösterim aynı yüzdeyi paylaşır.
    pub fn begin_progress_with_toast(
        &mut self,
        compositor: &mut MinGuiCompositor,
        label: (&'static str, &'static str),
        detail: &str,
        total: usize,
        start: u8,
    ) {
        self.begin_progress(label, total, start);
        let title = if crate::gui::ui_english() {
            label.1
        } else {
            label.0
        };
        let slot = compositor.push_progress_toast(
            crate::gui::ToastKind::Info,
            title,
            detail,
            crate::monotonic_ms(),
            start,
        );
        self.progress_toast = Some(slot);
    }

    /// İşlem bandındaki adım ilerlemesini günceller.
    pub fn update_progress(&mut self, done: usize, total: usize) {
        if let Some(p) = self.progress.as_mut() {
            p.set_steps(done, total);
        }
    }

    /// İşlem bandındaki yüzdeyi doğrudan ayarlar.
    pub fn set_progress_percent(&mut self, pct: u8) {
        if let Some(p) = self.progress.as_mut() {
            p.set_percent(pct);
        }
    }

    /// Toast ilerleme bildirimini bandın güncel yüzdesiyle eşitler.
    pub fn sync_progress_toast(&self, compositor: &mut MinGuiCompositor) {
        if let (Some(slot), Some(p)) = (self.progress_toast, self.progress) {
            compositor.update_toast_progress(slot, p.percent);
        }
    }

    /// İşlem bandını kapatır (işlem bitti/başarısız).
    pub fn finish_progress(&mut self) {
        self.progress = None;
        self.progress_toast = None;
    }

    /// İşlem bandını kapatır ve ilgili toast'u sonuç/başarısızlık durumuna
    /// getirir (`ok == false` ise hata bildirimi).
    pub fn finish_progress_with_toast(
        &mut self,
        compositor: &mut MinGuiCompositor,
        message: &str,
        ok: bool,
    ) {
        if let Some(slot) = self.progress_toast.take() {
            let now = crate::monotonic_ms();
            if ok {
                compositor.complete_toast_progress(slot, message, now);
            } else {
                compositor.fail_toast_progress(slot, message, now);
            }
        }
        self.progress = None;
    }

    /// Arka planda uzun bir işlem sürüyor mu?
    pub fn is_busy(&self) -> bool {
        self.progress.is_some()
    }

    /// İmza adı/paketi verilen dosyayı doğrudan oluşturur (onay sorulmaz).
    pub fn new_custom_file(
        &mut self,
        prefix: &str,
        ext: &str,
        initial_content: &[u8],
        vfs: &mut VirtualFileSystem,
    ) {
        let mut counter = self.item_counter;
        let name = loop {
            let candidate = numbered_name(prefix, counter, ext);
            if !vfs
                .list_dir(self.path())
                .iter()
                .any(|item| item.name.eq_ignore_ascii_case(&candidate))
            {
                break candidate;
            }
            counter += 1;
        };
        self.item_counter = counter + 1;

        let mut full_path = String::from(self.path());
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(&name);

        let saved = vfs.write_file(&full_path, initial_content);
        self.refresh_from_vfs(vfs);
        self.status = if saved {
            crate::gui::ui_text("Yeni öğe oluşturuldu.", "New item created.")
        } else if full_path.starts_with("/media/hdd0/") && !crate::drivers::ata::drive_available(0)
        {
            crate::apps::errors::AppErrorKind::StorageUnavailable.message(crate::gui::ui_english())
        } else {
            crate::gui::ui_text(
                "Dosya oluşturuldu; diske kaydedilemedi.",
                "File created; disk save failed.",
            )
        };
    }

    /// Yeni dosya oluşturur (onay isteği gösterir).
    pub fn new_file(&mut self, vfs: &mut VirtualFileSystem) {
        self.confirm_dialog = ConfirmDialog::NewFile;
        self.confirm_item_idx = 0;
        self.confirm_name_len = 0;
        self.confirm_cursor = 0;
        // Varsayılan isim önerisi
        let mut counter = self.item_counter;
        let name = loop {
            let candidate = numbered_name("yeni_", counter, ".txt");
            if !vfs
                .list_dir(self.path())
                .iter()
                .any(|item| item.name.eq_ignore_ascii_case(&candidate))
            {
                break candidate;
            }
            counter += 1;
        };
        self.item_counter = counter + 1;
        // Varsayılan ismi onay kutusuna kopyala
        for (i, b) in name.bytes().enumerate() {
            if i < self.confirm_name_buf.len() {
                self.confirm_name_buf[i] = b;
                self.confirm_name_len += 1;
            }
        }
    }

    /// Onaylı yeni dosya oluşturma işlemini tamamlar.
    pub fn confirm_new_file(&mut self, vfs: &mut VirtualFileSystem) {
        if self.confirm_dialog != ConfirmDialog::NewFile {
            return;
        }
        let name = core::str::from_utf8(&self.confirm_name_buf[..self.confirm_name_len])
            .unwrap_or("")
            .trim();
        if name.is_empty() || name.contains('/') {
            self.status = crate::gui::ui_text("Geçersiz dosya adı.", "Invalid file name.");
            self.confirm_dialog = ConfirmDialog::None;
            return;
        }
        let mut full_path = String::from(self.path());
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(name);
        let saved = vfs.write_file(&full_path, "Yeni oluşturulan dosya içeriği.\n".as_bytes());
        self.refresh_from_vfs(vfs);
        self.status = if saved {
            crate::gui::ui_text("Yeni dosya oluşturuldu.", "New file created.")
        } else if full_path.starts_with("/media/hdd0/") && !crate::drivers::ata::drive_available(0)
        {
            crate::apps::errors::AppErrorKind::StorageUnavailable.message(crate::gui::ui_english())
        } else {
            crate::gui::ui_text(
                "Dosya oluşturuldu; diske kaydedilemedi.",
                "File created; disk save failed.",
            )
        };
        self.confirm_dialog = ConfirmDialog::None;
    }

    /// Onaylı yeni klasör oluşturma işlemini tamamlar.
    pub fn confirm_new_folder(&mut self, vfs: &mut VirtualFileSystem) {
        if self.confirm_dialog != ConfirmDialog::NewFolder {
            return;
        }
        let name = core::str::from_utf8(&self.confirm_name_buf[..self.confirm_name_len])
            .unwrap_or("")
            .trim();
        if name.is_empty() || name.contains('/') {
            self.status = crate::gui::ui_text("Geçersiz klasör adı.", "Invalid folder name.");
            self.confirm_dialog = ConfirmDialog::None;
            return;
        }
        let mut full_path = String::from(self.path());
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(name);
        let saved = vfs.create_dir(&full_path);
        self.refresh_from_vfs(vfs);
        self.status = if saved {
            crate::gui::ui_text("Yeni klasör oluşturuldu.", "New folder created.")
        } else {
            crate::gui::ui_text("Klasör oluşturulamadı.", "Folder creation failed.")
        };
        self.confirm_dialog = ConfirmDialog::None;
    }

    /// Onay iptal edildi.
    pub fn cancel_confirm(&mut self) {
        self.confirm_dialog = ConfirmDialog::None;
        self.confirm_name_len = 0;
        self.confirm_cursor = 0;
    }

    /// Yeni klasör oluşturur (onay isteği gösterir).
    pub fn new_folder(&mut self, vfs: &mut VirtualFileSystem) {
        self.confirm_dialog = ConfirmDialog::NewFolder;
        self.confirm_item_idx = 0;
        self.confirm_name_len = 0;
        self.confirm_cursor = 0;
        // Varsayılan isim önerisi
        let mut counter = self.item_counter;
        let name = loop {
            let candidate = numbered_name("klas_", counter, "");
            if !vfs
                .list_dir(self.path())
                .iter()
                .any(|item| item.name.eq_ignore_ascii_case(&candidate))
            {
                break candidate;
            }
            counter += 1;
        };
        self.item_counter = counter + 1;
        // Varsayılan ismi onay kutusuna kopyala
        for (i, b) in name.bytes().enumerate() {
            if i < self.confirm_name_buf.len() {
                self.confirm_name_buf[i] = b;
                self.confirm_name_len += 1;
            }
        }
    }

    /// Seçili dosya veya klasörü siler (onay isteği gösterir).
    pub fn delete_selected(&mut self) -> bool {
        if !self.selected_items.iter().any(|&s| s) {
            if self.selected_idx >= self.file_count {
                return false;
            }
            self.selected_items[self.selected_idx] = true;
        }
        // Çöp kutusunda mı kontrol et
        let is_trash = self.path() == crate::fs::vfs::TRASH_DIR;
        self.confirm_dialog = if is_trash {
            ConfirmDialog::DeletePermanent
        } else {
            ConfirmDialog::Delete
        };
        self.confirm_item_idx = self.selected_idx;
        true
    }

    /// Onaylı silme işlemini tamamlar.
    pub fn confirm_delete(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        if self.confirm_dialog != ConfirmDialog::Delete
            && self.confirm_dialog != ConfirmDialog::DeletePermanent
        {
            return false;
        }
        let is_trash = self.path() == crate::fs::vfs::TRASH_DIR;
        // Seçilen öğe sayısını say ve ilerleme çubuğu başlat.
        let total = (0..self.file_count)
            .filter(|&i| {
                self.selected_items[i]
                    && self.search_matches(i)
                    && self.files[i].display_name() != ".."
            })
            .count();
        let mut done = 0usize;
        if total > 0 {
            self.begin_progress(("Siliniyor", "Deleting"), total, 0);
        }
        let mut any = false;
        for i in (0..self.file_count).rev() {
            if !self.selected_items[i] || !self.search_matches(i) {
                continue;
            }
            let item = self.files[i];
            let name = item.display_name();
            if name == ".." {
                continue;
            }
            let mut full_path = String::from(self.path());
            if !full_path.ends_with('/') {
                full_path.push('/');
            }
            full_path.push_str(name);
            let ok = if is_trash {
                if item.is_dir {
                    vfs.delete_tree(&full_path)
                } else {
                    vfs.delete_file(&full_path)
                }
            } else {
                vfs.move_to_trash(&full_path)
            };
            done += 1;
            if total > 0 {
                self.update_progress(done, total);
            }
            any |= ok;
        }
        if total > 0 {
            self.finish_progress();
        }
        if any {
            self.refresh_from_vfs(vfs);
            self.status = if is_trash {
                crate::gui::ui_text(
                    "Seçilen öğeler kalıcı olarak silindi.",
                    "Selected items permanently deleted.",
                )
            } else {
                crate::gui::ui_text(
                    "Seçilen öğeler geri dönüşüm kutusuna taşındı.",
                    "Selected items moved to Recycle Bin.",
                )
            };
        } else {
            self.status =
                crate::gui::ui_text("Silinemedi (klasör dolu olabilir).", "Delete failed.");
        }
        self.confirm_dialog = ConfirmDialog::None;
        any
    }

    pub fn begin_rename(&mut self) {
        if self.selected_idx >= self.file_count
            || self.files[self.selected_idx].display_name() == ".."
        {
            return;
        }
        if !self.selected_items[self.selected_idx] {
            self.selected_items = [false; MAX_ITEMS];
            self.selected_items[self.selected_idx] = true;
        }
        self.confirm_dialog = ConfirmDialog::Rename;
        self.confirm_item_idx = self.selected_idx;
        let name = self.files[self.selected_idx].display_name().as_bytes();
        let take = crate::gui::textedit::truncate_to_boundary(name, self.confirm_name_buf.len());
        self.confirm_name_len = take;
        self.confirm_name_buf[..self.confirm_name_len]
            .copy_from_slice(&name[..self.confirm_name_len]);
        self.confirm_cursor = self.confirm_name_len;
    }

    pub fn confirm_rename_char(&mut self, c: char) {
        crate::gui::textedit::insert_char(
            &mut self.confirm_name_buf,
            &mut self.confirm_name_len,
            &mut self.confirm_cursor,
            c,
        );
    }

    pub fn confirm_rename_backspace(&mut self) {
        crate::gui::textedit::backspace(
            &mut self.confirm_name_buf,
            &mut self.confirm_name_len,
            &mut self.confirm_cursor,
        );
    }

    pub fn confirm_rename_delete(&mut self) {
        crate::gui::textedit::delete_forward(
            &mut self.confirm_name_buf,
            &mut self.confirm_name_len,
            &mut self.confirm_cursor,
        );
    }

    pub fn confirm_rename_move_cursor(&mut self, right: bool) {
        if right {
            self.confirm_cursor = crate::gui::textedit::next_char_boundary(
                &self.confirm_name_buf,
                self.confirm_cursor,
                self.confirm_name_len,
            );
        } else {
            self.confirm_cursor = crate::gui::textedit::prev_char_boundary(
                &self.confirm_name_buf[..self.confirm_name_len],
                self.confirm_cursor,
            );
        }
    }

    pub fn confirm_rename(&mut self, vfs: &mut VirtualFileSystem) {
        if self.confirm_dialog != ConfirmDialog::Rename {
            return;
        }
        let name = core::str::from_utf8(&self.confirm_name_buf[..self.confirm_name_len])
            .unwrap_or("")
            .trim();
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            self.status = crate::gui::ui_text("Geçersiz ad.", "Invalid name.");
            self.confirm_dialog = ConfirmDialog::None;
            return;
        }
        if self.confirm_item_idx >= self.file_count {
            self.confirm_dialog = ConfirmDialog::None;
            return;
        }
        let old_name = self.files[self.confirm_item_idx].display_name();
        let mut old_path = String::from(self.path());
        if !old_path.ends_with('/') {
            old_path.push('/');
        }
        old_path.push_str(old_name);
        let mut new_path = String::from(self.path());
        if !new_path.ends_with('/') {
            new_path.push('/');
        }
        new_path.push_str(name);
        if vfs.rename(&old_path, &new_path) {
            self.refresh_from_vfs(vfs);
            self.status = crate::gui::ui_text("Ad değiştirildi.", "Name changed.");
        } else {
            self.status = crate::gui::ui_text("Ad değiştirilemedi.", "Rename failed.");
        }
        self.confirm_dialog = ConfirmDialog::None;
    }

    /// Geri Dönüşüm Kutusu'nda seçili öğeyi önceki konumuna değil, `/root/Documents`
    /// klasörüne geri yükler. Öğe adı çakışırsa numaralandırılır.
    pub fn restore_selected(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        if self.path() != crate::fs::vfs::TRASH_DIR {
            return false;
        }
        if self.selected_idx >= self.file_count {
            return false;
        }
        let item = &self.files[self.selected_idx];
        let name = String::from(item.display_name());
        if name == ".." {
            return false;
        }
        // Tek öğelik geri yükleme için gerçek ilerleme çubuğu göster.
        self.begin_progress(("Geri yükleniyor", "Restoring"), 1, 0);
        let mut trash_path = String::from(crate::fs::vfs::TRASH_DIR);
        trash_path.push('/');
        trash_path.push_str(&name);
        let ok = vfs.restore_from_trash(&trash_path, "/root/Documents");
        self.update_progress(1, 1);
        self.finish_progress();
        if ok {
            self.refresh_from_vfs(vfs);
            self.status = crate::gui::ui_text("Öğe geri yüklendi.", "Item restored.");
        } else {
            self.status = crate::gui::ui_text("Geri yüklenemedi.", "Restore failed.");
        }
        ok
    }

    /// Geri Dönüşüm Kutusu'nu boşaltır.
    pub fn empty_trash(&mut self, vfs: &mut VirtualFileSystem) -> usize {
        // Boşaltma tek bir VFS çağrısıdır ama disk yazımını bekler: ENGEL
        // yerine gerçek bir adım ilerlemesi göster (öğe sayısı kadar).
        let total = vfs.trash_count();
        self.begin_progress(
            ("Çöp kutusu boşaltılıyor", "Emptying Recycle Bin"),
            total,
            0,
        );
        if total > 0 {
            self.set_progress_percent(50);
        }
        let n = vfs.empty_trash();
        self.update_progress(total, total);
        self.finish_progress();
        if self.path() == crate::fs::vfs::TRASH_DIR {
            self.refresh_from_vfs(vfs);
        }
        self.status = if n > 0 {
            crate::gui::ui_text("Geri dönüşüm kutusu boşaltıldı.", "Recycle Bin emptied.")
        } else {
            crate::gui::ui_text(
                "Geri dönüşüm kutusu zaten boş.",
                "Recycle Bin already empty.",
            )
        };
        n
    }

    /// Çöp kutusunu boşaltmadan önce onay ister (kalıcı silme).
    /// Onay sonrası `empty_trash` çağrılmalıdır.
    pub fn request_empty_trash(&mut self) -> bool {
        self.confirm_dialog = ConfirmDialog::DeletePermanent;
        self.confirm_item_idx = usize::MAX; // "tümü" işareti
        self.status = crate::gui::ui_text(
            "Çöp kutusu boşaltılacak. Onaylayın.",
            "The recycle bin will be emptied. Please confirm.",
        );
        true
    }

    /// Seçili öğeyi açar veya klasöre girer.
    /// Dosya ise `Some((icon_tag, full_path_string))` döner.
    pub fn enter_selected(&mut self, vfs: &VirtualFileSystem) -> Option<(&'static str, String)> {
        if self.selected_idx >= self.file_count {
            return None;
        }
        let item = self.files[self.selected_idx];
        let name = item.display_name();

        if item.is_dir {
            if name == ".." {
                // Üst klasöre çık
                let cur = self.path();
                if let Some(pos) = cur.rfind('/') {
                    let parent = if pos == 0 { "/" } else { &cur[..pos] };
                    let parent_clone = String::from(parent);
                    self.set_path(&parent_clone);
                } else {
                    self.set_path("/");
                }
            } else {
                // Alt klasöre gir
                let mut next = String::from(self.path());
                if !next.ends_with('/') {
                    next.push('/');
                }
                next.push_str(name);
                self.set_path(&next);
            }
            self.refresh_from_vfs(vfs);
            None
        } else {
            let mut full_path = String::from(self.path());
            if !full_path.ends_with('/') {
                full_path.push('/');
            }
            full_path.push_str(name);
            Some((item.icon_tag, full_path))
        }
    }

    pub fn selected_full_path(&self) -> String {
        let mut p = String::from(self.path());
        if self.selected_idx < self.file_count {
            let name = self.files[self.selected_idx].display_name();
            if name != ".." {
                if !p.ends_with('/') {
                    p.push('/');
                }
                p.push_str(name);
            }
        }
        p
    }

    /// Backspace: üst klasöre çıkar (kök dizinde etkisizdir).
    ///
    /// Dönen değer, yol gerçekten değiştiyse `true`; bu durumda çağıran
    /// `refresh_from_vfs` ile listeyi yenilemelidir.
    pub fn go_up(&mut self) -> bool {
        let cur = self.path();
        if cur == "/" || cur.is_empty() {
            return false;
        }
        let parent = match cur.rfind('/') {
            Some(0) => "/",
            Some(pos) => &cur[..pos],
            None => "/",
        };
        let parent = String::from(parent);
        if parent == cur {
            return false;
        }
        self.set_path(&parent);
        true
    }

    /// Seçili (veya seçim yoksa imleçteki) öğelerin tam yollarını döndürür.
    /// `..` öğesi her zaman atlanır.
    pub fn selected_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let base = String::from(self.path());
        let has_sel = self.selected_items.iter().any(|&s| s);
        for i in 0..self.file_count {
            if self.files[i].display_name() == ".." {
                continue;
            }
            let picked = if has_sel {
                self.selected_items[i]
            } else {
                i == self.selected_idx
            };
            if !picked {
                continue;
            }
            let mut p = base.clone();
            if !p.ends_with('/') {
                p.push('/');
            }
            p.push_str(self.files[i].display_name());
            out.push(p);
        }
        out
    }

    pub fn row_count(&self) -> usize {
        self.file_count
    }

    pub fn row(&self, idx: usize) -> Option<&FileItem> {
        if idx < self.file_count {
            Some(&self.files[idx])
        } else {
            None
        }
    }

    pub fn list_content_h(&self) -> usize {
        self.content_height(0)
    }

    pub fn sidebar_path(index: usize) -> &'static str {
        match index {
            0 => "/",
            1 => "/desktop",
            2 => "/root/Documents",
            3 => "/root/Media",
            4 => "/root/Downloads",
            5 => "/boot",
            6 => "/media/usb0",
            7 => "/media/hdd0",
            8 => "/media/cdrom",
            _ => crate::fs::vfs::TRASH_DIR,
        }
    }

    pub fn sidebar_name(index: usize) -> &'static str {
        if crate::gui::ui_english() {
            match index {
                0 => "Root (/)",
                1 => "Desktop",
                2 => "Documents",
                3 => "Media",
                4 => "Downloads",
                5 => "System",
                6 => "USB Drive",
                7 => "Hard Disk",
                8 => "CD-ROM",
                _ => "Recycle Bin",
            }
        } else {
            match index {
                0 => "Kök (/)",
                1 => "Masaüstü",
                2 => "Belgeler",
                3 => "Medyalar",
                4 => "İndirilenler",
                5 => "Sistem",
                6 => "USB Disk",
                7 => "Sabit Disk",
                8 => "CD-ROM",
                _ => "Geri Dönüşüm",
            }
        }
    }

    pub fn drop_target_at(
        &self,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        sy: usize,
        mx: usize,
        my: usize,
    ) -> Option<DropTarget> {
        let sidebar_w = 110;
        let sidebar_h = win_h.saturating_sub(status_h() + 60);

        // Sidebar drop target check
        if mx >= win_x + 6
            && mx < win_x + 6 + sidebar_w
            && my >= win_y + 66
            && my < win_y + 66 + 10 * 22
        {
            let idx = (my.saturating_sub(win_y + 66)) / 22;
            if idx < 10 && (win_y + 66 + idx * 22 + 8) <= win_y + 60 + sidebar_h {
                return Some(DropTarget::Sidebar(idx));
            }
        }

        // File list drop target check (must be a directory and not the item being dragged)
        let list_top = win_y + LIST_TOP;
        let list_bottom = win_y + win_h - status_h();
        for i in 0..self.file_count {
            if self.files[i].is_dir
                && Some(i) != self.drag_item
                && my >= list_top
                && my < list_bottom
            {
                let r = self.item_rect(win_x, win_y, win_w, i, sy);
                if crate::gui::in_rect(mx, my, r) {
                    return Some(DropTarget::Folder(i));
                }
            }
        }

        // Dropping anywhere else inside file manager window targets current directory
        if mx >= win_x && mx < win_x + win_w && my >= win_y && my < win_y + win_h {
            return Some(DropTarget::CurrentDirectory);
        }

        None
    }

    pub fn target_directory(&self, target: DropTarget) -> Option<String> {
        match target {
            DropTarget::Sidebar(idx) => Some(String::from(Self::sidebar_path(idx))),
            DropTarget::CurrentDirectory => Some(String::from(self.path())),
            DropTarget::Folder(i) => {
                if i >= self.file_count || !self.files[i].is_dir {
                    return None;
                }
                let name = self.files[i].display_name();
                if name == ".." {
                    let cur = self.path();
                    if let Some(pos) = cur.rfind('/') {
                        let parent = if pos == 0 { "/" } else { &cur[..pos] };
                        Some(String::from(parent))
                    } else {
                        Some(String::from("/"))
                    }
                } else {
                    let mut path = String::from(self.path());
                    if !path.ends_with('/') {
                        path.push('/');
                    }
                    path.push_str(name);
                    Some(path)
                }
            }
        }
    }

    pub fn target_display_name(&self, target: DropTarget) -> String {
        match target {
            DropTarget::Sidebar(idx) => String::from(Self::sidebar_name(idx)),
            DropTarget::CurrentDirectory => String::from(self.path()),
            DropTarget::Folder(i) => {
                if i < self.file_count {
                    String::from(self.files[i].display_name())
                } else {
                    String::from("Klasör")
                }
            }
        }
    }

    pub fn execute_drop(
        &mut self,
        src_idx: usize,
        target: DropTarget,
        is_copy: bool,
        vfs: &mut crate::fs::vfs::VirtualFileSystem,
    ) -> Result<String, String> {
        use crate::apps::errors::AppErrorKind;

        if src_idx >= self.file_count {
            return Err(Self::drop_error(
                AppErrorKind::FileNotFound,
                crate::gui::ui_text("Kaynak öğe bulunamadı.", "Source item not found."),
            ));
        }
        if self.files[src_idx].display_name() == ".." {
            return Err(Self::drop_error(
                AppErrorKind::InvalidInput,
                crate::gui::ui_text("Üst klasör taşınamaz.", "Parent folder cannot be moved."),
            ));
        }

        let Some(target_dir) = self.target_directory(target) else {
            return Err(Self::drop_error(
                AppErrorKind::InvalidInput,
                crate::gui::ui_text("Hedef klasör geçersiz.", "Invalid target folder."),
            ));
        };

        if target_dir == "/media/cdrom" {
            return Err(Self::drop_error(
                AppErrorKind::PermissionDenied,
                crate::gui::ui_text("CD-ROM salt okunurdur.", "CD-ROM is read-only."),
            ));
        }

        let target_disp = self.target_display_name(target);
        if self.selected_items[src_idx] {
            self.drag_items = self.selected_items;
        } else {
            self.drag_items = [false; MAX_ITEMS];
            self.drag_items[src_idx] = true;
        }
        let mut count = 0;
        let mut first_name = String::new();
        for i in 0..self.file_count {
            if !self.drag_items[i] || self.files[i].display_name() == ".." {
                continue;
            }
            let src_name = self.files[i].display_name();
            if first_name.is_empty() {
                first_name.push_str(src_name);
            }
            let mut src_path = String::from(self.path());
            if !src_path.ends_with('/') {
                src_path.push('/');
            }
            src_path.push_str(src_name);
            if is_copy && target_dir != crate::fs::vfs::TRASH_DIR {
                vfs.copy_item(&src_path, &target_dir)
                    .map_err(|err| Self::drop_error(AppErrorKind::classify(err), err))?;
            } else {
                vfs.move_item(&src_path, &target_dir)
                    .map_err(|err| Self::drop_error(AppErrorKind::classify(err), err))?;
            }
            count += 1;
        }
        if count == 0 {
            return Err(Self::drop_error(
                AppErrorKind::FileNotFound,
                crate::gui::ui_text("Kaynak öğe bulunamadı.", "Source item not found."),
            ));
        }

        let mut msg = TextBuf::new();
        if is_copy && target_dir != crate::fs::vfs::TRASH_DIR {
            msg.push_str(crate::gui::ui_text("Kopyalandı: ", "Copied: "));
        } else if target_dir == crate::fs::vfs::TRASH_DIR {
            msg.push_str(crate::gui::ui_text("Çöpe taşındı: ", "Moved to Trash: "));
        } else {
            msg.push_str(crate::gui::ui_text("Taşındı: ", "Moved: "));
        }
        msg.push_str(&first_name);
        if count > 1 {
            msg.push_str(" + ");
            msg.push_usize(count - 1);
        }
        msg.push_str(" -> ");
        msg.push_str(&target_disp);
        self.set_status_text(msg.as_str());

        Ok(target_dir)
    }

    pub fn draw_drag_badge(&self, fb: &mut Framebuffer, mx: usize, my: usize, is_copy: bool) {
        let Some(item_idx) = self.drag_item else {
            return;
        };
        if item_idx >= self.file_count {
            return;
        }
        let file = &self.files[item_idx];
        let name = file.display_name();

        let op_prefix = if is_copy {
            crate::gui::ui_text("+ Kopyala: ", "+ Copy: ")
        } else {
            crate::gui::ui_text("-> Taşı: ", "-> Move: ")
        };
        let accent_color = if is_copy {
            Color::ACCENT_GREEN
        } else {
            Color::ACCENT_BLUE
        };

        let mut text = TextBuf::new();
        text.push_str(op_prefix);
        text.push_str(name);

        if let Some(target) = self.drag_target {
            text.push_str(" -> ");
            let target_name = self.target_display_name(target);
            text.push_str(&target_name);
        }

        let tw = MinGuiCompositor::text_width(text.as_str(), 1);
        let badge_w = tw + 28;
        let badge_h = 24;
        let badge_x = (mx + 14).min(fb.width.saturating_sub(badge_w + 4));
        let badge_y = (my + 14).min(fb.height.saturating_sub(badge_h + 4));

        fb.draw_rounded_rect(badge_x, badge_y, badge_w, badge_h, 4, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(badge_x, badge_y, badge_w, badge_h, 4, accent_color);

        if file.is_dir {
            Self::draw_folder_icon(fb, badge_x + 5, badge_y + 4, false);
        } else {
            Self::draw_file_icon(fb, badge_x + 5, badge_y + 4, file.icon_tag);
        }

        MinGuiCompositor::draw_string(
            fb,
            badge_x + 24,
            badge_y + 5,
            text.as_str(),
            Color::TEXT_MAIN,
        );
    }

    /// Klasör Simgesi: Arka sekme, gövde, parlak sarı/amber kapak ve yukarı oku (..) desteği
    pub fn draw_folder_icon(fb: &mut Framebuffer, x: usize, y: usize, is_up: bool) {
        Self::draw_folder_icon_sized(fb, x, y, 1, is_up);
    }

    /// Ölçekli klasör simgesi (ızgara görünümü için). `s` = ölçek çarpanı.
    pub fn draw_folder_icon_sized(fb: &mut Framebuffer, x: usize, y: usize, s: usize, is_up: bool) {
        let s = s.max(1);
        // Klasör Arka Sekmesi (Tab)
        fb.draw_rect(x + s, y, 6 * s, 3 * s, Color::ACCENT_YELLOW);
        // Klasör Arka Gövdesi (Back Body)
        fb.draw_rect(
            x + s,
            y + 2 * s,
            14 * s,
            10 * s,
            MinGuiCompositor::shade(Color::ACCENT_YELLOW, -30),
        );
        // Klasör Ön Kapağı (Front Flap - canlı parlak sarı)
        fb.draw_rect(x, y + 4 * s, 16 * s, 9 * s, Color::ACCENT_YELLOW);
        // Üst kenar parıltısı (Highlight)
        fb.draw_line(x + s, y + 4 * s, x + 15 * s, y + 4 * s, 0xFFE082);
        // İnce alt kenarlık / gölge
        fb.draw_line(
            x,
            y + 12 * s,
            x + 15 * s,
            y + 12 * s,
            MinGuiCompositor::shade(Color::ACCENT_YELLOW, -50),
        );

        // Eğer üst klasör ise (..) içine beyaz yukarı oku çiz
        if is_up {
            fb.draw_line(x + 8 * s, y + 6 * s, x + 8 * s, y + 10 * s, Color::WHITE);
            fb.draw_line(x + 7 * s, y + 7 * s, x + 9 * s, y + 7 * s, Color::WHITE);
            fb.draw_line(x + 6 * s, y + 8 * s, x + 10 * s, y + 8 * s, Color::WHITE);
        }
    }

    /// Belge / Dosya Simgesi: Katlanmış köşeli sayfa ve dosya türü vurgusu
    pub fn draw_file_icon(fb: &mut Framebuffer, x: usize, y: usize, tag: &str) {
        Self::draw_file_icon_sized(fb, x, y, 1, tag);
    }

    /// Ölçekli dosya simgesi (ızgara görünümü için). `s` = ölçek çarpanı.
    pub fn draw_file_icon_sized(fb: &mut Framebuffer, x: usize, y: usize, s: usize, tag: &str) {
        let s = s.max(1);
        let accent = match tag {
            "3D" => Color::ACCENT_PINK,
            "IMG" => Color::ACCENT_YELLOW,
            "SND" => Color::ACCENT_GREEN,
            "VID" => Color::ACCENT_RED,
            "WEB" => Color::ACCENT_BLUE,
            "CFG" => Color::ACCENT_PINK,
            _ => Color::TEXT_MAIN,
        };
        // Sayfa beyaz kağıt gövdesi
        fb.draw_rect(x + 2 * s, y + s, 11 * s, 13 * s, Color::WHITE);
        // Sağ üst katlanmış köşe (Folded Corner)
        fb.draw_rect(x + 9 * s, y + s, 4 * s, 4 * s, Color::TITLE_BAR);
        // İçerik satır çizgileri
        fb.draw_line(x + 4 * s, y + 5 * s, x + 10 * s, y + 5 * s, accent);
        fb.draw_line(x + 4 * s, y + 7 * s, x + 10 * s, y + 7 * s, accent);
        fb.draw_line(x + 4 * s, y + 9 * s, x + 8 * s, y + 9 * s, accent);
        // İnce sayfa çerçevesi
        fb.draw_rect_outline(x + 2 * s, y + s, 11 * s, 13 * s, Color::BORDER);
    }

    /// Geri Dönüşüm Kutusu Simgesi: Kapaklı, çizgili bir çöp kutusu.
    pub fn draw_trash_icon(fb: &mut Framebuffer, x: usize, y: usize) {
        // Kapak (üst yatay şerit)
        fb.draw_rect(x + 2, y + 1, 12, 2, Color::ACCENT_GREEN);
        // Kapak tutamacı (küçük çıkıntı)
        fb.draw_rect(x + 6, y, 4, 1, Color::ACCENT_GREEN);
        // Gövde (aşağı doğru daralan bir kutu)
        fb.draw_rect(
            x + 3,
            y + 4,
            10,
            10,
            MinGuiCompositor::shade(Color::ACCENT_GREEN, -40),
        );
        // Gövde kenar çizgileri (dikey çizgiler)
        fb.draw_line(x + 6, y + 5, x + 6, y + 12, Color::ACCENT_GREEN);
        fb.draw_line(x + 10, y + 5, x + 10, y + 12, Color::ACCENT_GREEN);
        // Alt kenar
        fb.draw_line(x + 3, y + 13, x + 12, y + 13, Color::ACCENT_GREEN);
    }

    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        scroll: &ScrollState,
        is_copy: bool,
    ) {
        let sidebar_w = 110;
        let list_x = Self::list_x(win_x);
        let list_w = self.list_width(win_w);
        let list_top = win_y + LIST_TOP;
        let list_bottom = win_y + win_h - status_h();
        let list_view_h = list_bottom.saturating_sub(list_top);
        let content_h = self.content_height(win_w);
        let sy = scroll.y.min(content_h.saturating_sub(list_view_h));

        // Üst Araç Çubuğu (Toolbar)
        fb.draw_rect(win_x + 6, win_y + 30, win_w - 12, 32, Color::TITLE_BAR);

        // Breadcrumb-style yol gösterimi
        let path = self.path();
        let parts: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut x_pos = win_x + 12;
        let y_pos = win_y + 36;

        // Kök dizin butonu
        let root_label = crate::gui::ui_text("🏠 Kök", "🏠 Root");
        let root_w = MinGuiCompositor::text_width(root_label, 1) + 16;
        fb.draw_rounded_rect(x_pos, y_pos, root_w, 20, 4, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, x_pos + 8, y_pos + 4, root_label, Color::WHITE);
        x_pos += root_w + 8;

        // Yol parçaları
        let mut current_path = String::from("/");
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                // Ayırıcı
                let sep = " / ";
                let sep_w = MinGuiCompositor::text_width(sep, 1);
                MinGuiCompositor::draw_string(fb, x_pos, y_pos + 4, sep, Color::TEXT_MUTED);
                x_pos += sep_w;
            }
            current_path.push_str(part);
            let part_label = *part;
            let part_w = MinGuiCompositor::text_width(part_label, 1) + 16;
            let is_last = i == parts.len().saturating_sub(1);
            let bg_color = if is_last {
                Color::ACCENT_BLUE
            } else {
                Color::TASKBAR_BG
            };
            let text_color = if is_last {
                Color::WHITE
            } else {
                Color::TEXT_MAIN
            };
            fb.draw_rounded_rect(x_pos, y_pos, part_w, 20, 4, bg_color);
            MinGuiCompositor::draw_string(fb, x_pos + 8, y_pos + 4, part_label, text_color);
            x_pos += part_w + 8;
            if !is_last {
                current_path.push('/');
            }
        }

        // Üst klasör butonu (kullanılabilirse ve ekranda yer varsa)
        if path != "/" && path != "/desktop" && win_w >= 300 {
            let up_label = crate::gui::ui_text("⬆ Üst Klasör", "⬆ Parent");
            let up_w = MinGuiCompositor::text_width(up_label, 1) + 16;
            let up_x = win_x + win_w - up_w - 120; // Sağ tarafta, arama kutusunun solunda
            fb.draw_rounded_rect(up_x, y_pos, up_w, 20, 4, Color::ACCENT_GREEN);
            MinGuiCompositor::draw_string(fb, up_x + 8, y_pos + 4, up_label, Color::WHITE);
        }

        let search_r = Self::search_rect(win_x, win_y, win_w);
        fb.draw_rounded_rect(
            search_r.0,
            search_r.1,
            search_r.2,
            search_r.3,
            4,
            Color::WINDOW_BG,
        );
        fb.draw_rounded_rect_outline(
            search_r.0,
            search_r.1,
            search_r.2,
            search_r.3,
            4,
            Color::BORDER,
        );
        if self.focused_control == 6 {
            fb.draw_rounded_rect_outline(
                search_r.0.saturating_sub(2),
                search_r.1.saturating_sub(2),
                search_r.2 + 4,
                search_r.3 + 4,
                5,
                Color::ACCENT_YELLOW,
            );
        }
        if self.search_len == 0 {
            MinGuiCompositor::draw_string(
                fb,
                search_r.0 + 6,
                search_r.1 + 6,
                crate::gui::ui_text("Ada göre filtrele", "Filter by name"),
                Color::TEXT_MUTED,
            );
        } else if let Ok(query) = core::str::from_utf8(&self.search[..self.search_len]) {
            MinGuiCompositor::draw_string_clipped(
                fb,
                search_r.0 + 6,
                search_r.1 + 6,
                query,
                Color::TEXT_MAIN,
                (
                    search_r.0 + 4,
                    search_r.1 + 2,
                    search_r.2.saturating_sub(8),
                    search_r.3.saturating_sub(4),
                ),
            );
        }

        // İşlemler, görünüm ve önizleme seçenekleri taşma menüsüne taşındı.
        let actions = Self::actions_button_rect(win_x, win_y, win_w);
        fb.draw_rect(actions.0, actions.1, actions.2, actions.3, Color::WINDOW_BG);
        for dot in 0..3 {
            fb.draw_rect(
                actions.0 + 13,
                actions.1 + 5 + dot * 5,
                2,
                2,
                Color::TEXT_MAIN,
            );
        }
        if self.focused_control < 5 {
            fb.draw_rect_outline(
                actions.0.saturating_sub(2),
                actions.1.saturating_sub(2),
                actions.2 + 4,
                actions.3 + 4,
                Color::ACCENT_YELLOW,
            );
        }

        // Sol Gezinti Paneli (Sidebar) — Sürücüler ve Hızlı Klasörler
        let sidebar_h = win_h.saturating_sub(status_h() + 60);
        fb.draw_rect(
            win_x + 6,
            win_y + 60,
            sidebar_w,
            sidebar_h,
            Color::TITLE_BAR_INACTIVE,
        );

        let nav = if crate::gui::ui_english() {
            [
                "Root (/)",
                "Desktop",
                "Documents",
                "Media",
                "Downloads",
                "System",
                "USB Drive",
                "Hard Disk",
                "CD-ROM",
                "Recycle Bin",
            ]
        } else {
            [
                "Kök (/)",
                "Masaüstü",
                "Belgeler",
                "Medyalar",
                "İndirilenler",
                "Sistem",
                "USB Disk",
                "Sabit Disk",
                "CD-ROM",
                "Geri Dönüşüm",
            ]
        };

        for (i, item) in nav.iter().enumerate() {
            let iy = win_y + 66 + i * 22;
            if iy + 8 > win_y + 60 + sidebar_h {
                break;
            }
            let cur = self.path();
            let is_active = match i {
                0 => cur == "/",
                1 => cur == "/desktop",
                2 => cur == "/root/Documents",
                3 => cur == "/root/Media",
                4 => cur == "/root/Downloads",
                5 => cur == "/boot",
                6 => cur == "/media/usb0",
                7 => cur == "/media/hdd0",
                8 => cur == "/media/cdrom",
                9 => cur == crate::fs::vfs::TRASH_DIR,
                _ => false,
            };
            let is_drag_target =
                self.drag_active && self.drag_target == Some(DropTarget::Sidebar(i));
            if is_drag_target {
                let target_color = if is_copy {
                    Color::ACCENT_GREEN
                } else {
                    Color::ACCENT_BLUE
                };
                fb.draw_rounded_rect(
                    win_x + 7,
                    iy - 2,
                    sidebar_w - 2,
                    20,
                    3,
                    MinGuiCompositor::shade(Color::WINDOW_BG, 26),
                );
                fb.draw_rounded_rect_outline(win_x + 7, iy - 2, sidebar_w - 2, 20, 3, target_color);
            } else if is_active {
                fb.draw_rect(win_x + 7, iy - 2, sidebar_w - 2, 20, Color::TITLE_BAR);
            }
            if i <= 5 {
                Self::draw_folder_icon(fb, win_x + 9, iy + 2, false);
            } else if i == 6 {
                // USB Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_GREEN);
                fb.draw_rect(win_x + 12, iy + 1, 8, 3, Color::WHITE);
            } else if i == 7 {
                // HDD Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_YELLOW);
                fb.draw_line(win_x + 11, iy + 8, win_x + 21, iy + 8, Color::BLACK);
            } else if i == 8 {
                // CD Simgesi
                fb.draw_rect(win_x + 9, iy + 3, 14, 10, Color::ACCENT_PINK);
                fb.draw_rect(win_x + 14, iy + 6, 4, 4, Color::WHITE);
            } else {
                // Geri Dönüşüm Kutusu Simgesi (çöp kutusu)
                Self::draw_trash_icon(fb, win_x + 9, iy + 2);
            }
            let col = if is_drag_target {
                if is_copy {
                    Color::ACCENT_GREEN
                } else {
                    Color::ACCENT_BLUE
                }
            } else if is_active {
                Color::ACCENT_BLUE
            } else if i == 6 {
                if diskfs::usb_available() {
                    Color::ACCENT_GREEN
                } else {
                    Color::TEXT_MUTED
                }
            } else if i == 7 {
                if crate::drivers::ata::device(0)
                    .map(|dev| dev.kind == crate::drivers::ata::DeviceKind::Ata)
                    .unwrap_or(false)
                {
                    Color::ACCENT_YELLOW
                } else {
                    Color::TEXT_MUTED
                }
            } else if i == 8 {
                if diskfs::cd_available() {
                    Color::ACCENT_PINK
                } else {
                    Color::TEXT_MUTED
                }
            } else {
                Color::TEXT_MAIN
            };
            MinGuiCompositor::draw_string(fb, win_x + 28, iy + 2, item, col);
        }

        // Sağ Liste Başlıkları (yalnız liste görünümünde).
        let col_name_x = list_x + 28;
        let col_type_x = list_x + 180;
        let col_size_x = list_x + 285;
        let col_size_w = list_w.saturating_sub(285);
        fb.draw_rect(list_x, win_y + 60, list_w, 20, Color::TITLE_BAR);
        if self.view_mode == ViewMode::List {
            // Sıralama göstergesi (ok)
            let sort_arrow = if self.sort_order == SortOrder::Ascending {
                " ▲"
            } else {
                " ▼"
            };

            let name_label = crate::gui::ui_text("Ad", "Name");
            let name_text = if self.sort_by == SortBy::Name {
                let mut s = String::from(name_label);
                s.push_str(sort_arrow);
                s
            } else {
                String::from(name_label)
            };
            MinGuiCompositor::draw_string_clipped(
                fb,
                col_name_x,
                win_y + 65,
                name_text.as_str(),
                Color::TEXT_MUTED,
                (col_name_x, win_y + 60, 145, 20),
            );

            if list_w > 210 {
                let type_label = crate::gui::ui_text("Tür", "Type");
                let type_text = if self.sort_by == SortBy::Type {
                    let mut s = String::from(type_label);
                    s.push_str(sort_arrow);
                    s
                } else {
                    String::from(type_label)
                };
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    col_type_x,
                    win_y + 65,
                    type_text.as_str(),
                    Color::TEXT_MUTED,
                    (col_type_x, win_y + 60, 95, 20),
                );
            }
            if list_w > 320 {
                let size_header = crate::gui::ui_text("Boyut", "Size");
                let size_text = if self.sort_by == SortBy::Size {
                    let mut s = String::from(size_header);
                    s.push_str(sort_arrow);
                    s
                } else {
                    String::from(size_header)
                };
                let header_x = col_size_x
                    + col_size_w
                        .saturating_sub(MinGuiCompositor::text_width(size_text.as_str(), 1) + 6);
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    header_x,
                    win_y + 65,
                    size_text.as_str(),
                    Color::TEXT_MUTED,
                    (col_size_x, win_y + 60, col_size_w, 20),
                );
            }
        } else {
            MinGuiCompositor::draw_string_clipped(
                fb,
                list_x + 8,
                win_y + 65,
                crate::gui::ui_text("Izgara görünümü", "Grid view"),
                Color::TEXT_MUTED,
                (list_x + 6, win_y + 60, list_w.saturating_sub(12), 20),
            );
        }

        // Dosya ve Klasör Listesi / Izgarası.
        if self.visible_count() == 0 {
            let empty_y = list_top + 10;
            let empty_h = list_view_h.saturating_sub(20);
            let empty_title = if self.search_len > 0 {
                crate::gui::ui_text("Eşleşen dosya yok", "No matching files")
            } else {
                crate::gui::tr(crate::gui::I18nKey::EmptyFolder)
            };
            let empty_sub = if self.search_len > 0 {
                crate::gui::ui_text(
                    "Farklı bir arama terimi deneyin.",
                    "Try a different search query.",
                )
            } else {
                crate::gui::ui_text(
                    "Yeni dosya veya klasör ekleyebilirsiniz.",
                    "You can add new files or folders.",
                )
            };
            let action_lbl = if self.search_len > 0 {
                Some(crate::gui::tr(crate::gui::I18nKey::Refresh))
            } else {
                Some(crate::gui::tr(crate::gui::I18nKey::CreateFolder))
            };
            crate::gui::ui_helpers::draw_empty_state(
                fb,
                list_x,
                empty_y,
                list_w,
                empty_h,
                crate::gui::ui_helpers::EmptyStateIcon::Folder,
                empty_title,
                empty_sub,
                action_lbl,
                false,
            );
        }
        let mut visible_idx = 0usize;
        for i in 0..self.file_count {
            if !self.search_matches(i) {
                continue;
            }
            let (ix, draw_y, iw, ih) = self.item_rect(win_x, win_y, win_w, visible_idx, sy);
            visible_idx += 1;
            if draw_y + ih <= list_top || draw_y >= list_bottom {
                continue;
            }
            let file = &self.files[i];
            let is_sel = self.is_row_selected(i);
            // Klavye imleci: seçim olmasa da görünür olsun (odak halkası) — görme
            // engeli dışındaki "hangi öğe odakta?" sorusu klavyeyle cevaplanır.
            let is_cursor = self.is_cursor_row(i);
            let is_drag_target =
                self.drag_active && self.drag_target == Some(DropTarget::Folder(i));
            let is_drag_source = self.drag_active && self.drag_item == Some(i);
            let is_up = file.display_name() == "..";
            let target_color = if is_copy {
                Color::ACCENT_GREEN
            } else {
                Color::ACCENT_BLUE
            };

            if self.view_mode == ViewMode::List {
                if is_drag_target {
                    fb.draw_rounded_rect(
                        list_x,
                        draw_y,
                        list_w,
                        ROW_H - 2,
                        3,
                        MinGuiCompositor::shade(Color::WINDOW_BG, 26),
                    );
                    fb.draw_rounded_rect_outline(
                        list_x,
                        draw_y,
                        list_w,
                        ROW_H - 2,
                        3,
                        target_color,
                    );
                } else if is_drag_source {
                    fb.draw_rect(
                        list_x,
                        draw_y,
                        list_w,
                        ROW_H - 2,
                        MinGuiCompositor::shade(Color::WINDOW_BG, 14),
                    );
                    fb.draw_rect_outline(list_x, draw_y, list_w, ROW_H - 2, Color::TEXT_MUTED);
                } else if is_sel {
                    fb.draw_rect(list_x, draw_y, list_w, ROW_H - 2, Color::TITLE_BAR);
                    if self.focused_control == 5 {
                        fb.draw_rect_outline(list_x, draw_y, list_w, ROW_H - 2, Color::ACCENT_BLUE);
                    }
                } else if is_cursor && self.focused_control == 5 {
                    // Odakta ama seçili değil (ör. Ctrl ile seçimden çıkarıldı):
                    // noktalı olmayan ince bir imleç halkası çiz.
                    fb.draw_rect_outline(list_x, draw_y, list_w, ROW_H - 2, Color::TEXT_MUTED);
                }

                if file.is_dir {
                    Self::draw_folder_icon(fb, list_x + 6, draw_y + 3, is_up);
                } else {
                    Self::draw_file_icon(fb, list_x + 6, draw_y + 3, file.icon_tag);
                }

                let name_col = if is_drag_target {
                    target_color
                } else if is_sel {
                    MinGuiCompositor::text_on(Color::TITLE_BAR)
                } else {
                    Color::TEXT_MAIN
                };
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    col_name_x,
                    draw_y + 4,
                    file.display_name(),
                    name_col,
                    (col_name_x, draw_y + 2, 145, ROW_H - 4),
                );

                if list_w > 210 {
                    MinGuiCompositor::draw_string_clipped(
                        fb,
                        col_type_x,
                        draw_y + 4,
                        file.file_type,
                        Color::TEXT_MUTED,
                        (col_type_x, draw_y + 2, 95, ROW_H - 4),
                    );
                }
                if list_w > 320 {
                    let size_text = file.size_str.as_str();
                    let size_x = col_size_x
                        + col_size_w.saturating_sub(MinGuiCompositor::text_width(size_text, 1) + 6);
                    MinGuiCompositor::draw_string_clipped(
                        fb,
                        size_x,
                        draw_y + 4,
                        size_text,
                        Color::TEXT_MAIN,
                        (col_size_x, draw_y + 2, col_size_w, ROW_H - 4),
                    );
                }
            } else {
                // Izgara kartı: büyük simge veya küçük resim + ad + tür.
                if is_drag_target {
                    fb.draw_rounded_rect(
                        ix,
                        draw_y,
                        iw,
                        ih - 4,
                        5,
                        MinGuiCompositor::shade(Color::WINDOW_BG, 26),
                    );
                    fb.draw_rounded_rect_outline(ix, draw_y, iw, ih - 4, 5, target_color);
                } else if is_drag_source {
                    fb.draw_rounded_rect(
                        ix,
                        draw_y,
                        iw,
                        ih - 4,
                        5,
                        MinGuiCompositor::shade(Color::WINDOW_BG, 14),
                    );
                    fb.draw_rounded_rect_outline(ix, draw_y, iw, ih - 4, 5, Color::TEXT_MUTED);
                } else if is_sel {
                    fb.draw_rounded_rect(ix, draw_y, iw, ih - 4, 5, Color::TITLE_BAR);
                    if self.focused_control == 5 {
                        fb.draw_rounded_rect_outline(ix, draw_y, iw, ih - 4, 5, Color::ACCENT_BLUE);
                    }
                }

                let icon_box_w = 64usize;
                let icon_box_h = 54usize;
                let icon_x = ix + iw.saturating_sub(icon_box_w) / 2;
                let icon_y = draw_y + 6;
                let thumb = self.thumbs.get(i).and_then(|t| t.as_ref());
                if let Some(t) = thumb {
                    Self::draw_thumb(fb, icon_x, icon_y, icon_box_w, icon_box_h, t);
                } else if file.is_dir {
                    let s = 3usize;
                    Self::draw_folder_icon_sized(
                        fb,
                        icon_x + icon_box_w.saturating_sub(16 * s) / 2,
                        icon_y + icon_box_h.saturating_sub(13 * s) / 2,
                        s,
                        is_up,
                    );
                } else {
                    let s = 3usize;
                    Self::draw_file_icon_sized(
                        fb,
                        icon_x + icon_box_w.saturating_sub(15 * s) / 2,
                        icon_y + icon_box_h.saturating_sub(15 * s) / 2,
                        s,
                        file.icon_tag,
                    );
                }

                let name_col = if is_drag_target {
                    target_color
                } else if is_sel {
                    MinGuiCompositor::text_on(Color::TITLE_BAR)
                } else {
                    Color::TEXT_MAIN
                };
                let type_col = if is_sel {
                    MinGuiCompositor::text_on(Color::TITLE_BAR)
                } else {
                    Color::TEXT_MUTED
                };
                let cell_clip_w = iw.saturating_sub(6);
                let name = file.display_name();
                let name_tw = MinGuiCompositor::text_width(name, 1).min(cell_clip_w);
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    ix + iw.saturating_sub(name_tw) / 2,
                    draw_y + ih - 36,
                    name,
                    name_col,
                    (ix + 3, draw_y + ih - 38, cell_clip_w, 16),
                );
                let ftype = if file.is_dir {
                    crate::gui::ui_text("Klasör", "Folder")
                } else {
                    file.file_type
                };
                let type_tw = MinGuiCompositor::text_width(ftype, 1).min(cell_clip_w);
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    ix + iw.saturating_sub(type_tw) / 2,
                    draw_y + ih - 20,
                    ftype,
                    type_col,
                    (ix + 3, draw_y + ih - 22, cell_clip_w, 16),
                );
            }
        }

        // Hızlı Önizleme paneli.
        if self.preview_open {
            self.render_preview(fb, win_x, win_y, win_w, win_h);
        }

        // Uzun işlem bandı (kopyalama/silme/küçük resim/boşaltma): gerçek yüzde.
        if let Some(p) = self.progress {
            Self::render_progress_banner(fb, p, win_x, win_y, win_w, win_h);
        }

        // Alt Durum Çubuğu
        let status_y = win_y + win_h - status_h() + 6;
        fb.draw_rect(win_x + 6, status_y, win_w - 12, 18, Color::TASKBAR_BG);
        let mut sb = TextBuf::new();
        if self.rename_active {
            sb.push_str(crate::gui::ui_text("Yeni ad: ", "New name: "));
            if let Ok(name) = core::str::from_utf8(&self.rename_buf[..self.rename_len]) {
                sb.push_str(name);
            }
            sb.push_str(crate::gui::ui_text(
                "  | Enter: kaydet  Esc: iptal",
                "  | Enter: save  Esc: cancel",
            ));
        } else if !self.status_buf.is_empty() {
            sb.push_str(self.status_buf.as_str());
        } else if !self.status.is_empty() {
            sb.push_str(self.status);
        } else {
            sb.push_usize(self.visible_count());
            if self.search_len > 0 {
                sb.push_str(crate::gui::ui_text(
                    " eşleşme | Filtre: ",
                    " matches | Filter: ",
                ));
                if let Ok(query) = core::str::from_utf8(&self.search[..self.search_len]) {
                    sb.push_str(query);
                }
                sb.push_str("  (Esc: ");
                sb.push_str(crate::gui::ui_text("temizle)", "clear)"));
            } else {
                sb.push_str(crate::gui::ui_text(" Öğe | ", " Items | "));
            }
            sb.push_str(" | ");
            sb.push_str(self.path());
        }
        if self.actions_menu_open {
            let (menu_x, menu_y, menu_w, menu_h) =
                Self::actions_menu_rect(win_x, win_y, win_w, win_h);
            fb.draw_shadow(menu_x, menu_y, menu_w, menu_h, Color::SHADOW);
            fb.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::TASKBAR_BG);
            fb.draw_rect_outline(menu_x, menu_y, menu_w, menu_h, Color::BORDER);
            let labels = [
                crate::gui::ui_text("Yenile", "Refresh"),
                crate::gui::ui_text("Yeni dosya", "New file"),
                crate::gui::ui_text("Yeni klasör", "New folder"),
                crate::gui::ui_text("Sil", "Delete"),
                crate::gui::ui_text("Aç", "Open"),
                if self.view_mode == ViewMode::Grid {
                    crate::gui::ui_text("Liste görünümü", "List view")
                } else {
                    crate::gui::ui_text("Izgara görünümü", "Grid view")
                },
                crate::gui::ui_text("Önizleme", "Preview"),
            ];
            for (i, label) in labels.iter().enumerate() {
                let item = Self::actions_menu_item_rect(win_x, win_y, win_w, win_h, i);
                fb.draw_rect(item.0, item.1, item.2, item.3, Color::WINDOW_BG);
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    item.0 + 8,
                    item.1 + 6,
                    label,
                    Color::TEXT_MAIN,
                    (
                        item.0 + 4,
                        item.1 + 1,
                        item.2.saturating_sub(8),
                        item.3.saturating_sub(2),
                    ),
                );
            }
        }
        MinGuiCompositor::draw_string(fb, win_x + 12, status_y + 4, sb.as_str(), Color::TEXT_MUTED);
        if self.rename_active {
            let prefix = core::str::from_utf8(&self.rename_buf[..self.rename_cursor]).unwrap_or("");
            let cx = win_x
                + 12
                + MinGuiCompositor::text_width(crate::gui::ui_text("Yeni ad: ", "New name: "), 1)
                + MinGuiCompositor::text_width(prefix, 1);
            fb.draw_rect(cx, status_y + 4, 1, 12, Color::ACCENT_BLUE);
        }

        // Onay iletişim kutusu çiz
        if self.confirm_dialog != ConfirmDialog::None {
            self.render_confirm_dialog(fb, win_x, win_y, win_w, win_h);
        }

        // Klavye odağı halkası (Tab navigasyonu için)
        let focused_rect = match self.focused_control {
            0 => Some(Self::refresh_button_rect(win_x, win_y, win_w)),
            1 => Some(Self::new_file_button_rect(win_x, win_y, win_w)),
            2 => Some(Self::new_folder_button_rect(win_x, win_y, win_w)),
            3 => Some(Self::delete_button_rect(win_x, win_y, win_w)),
            4 => Some(Self::open_button_rect(win_x, win_y, win_w)),
            5 => {
                // Dosya listesi/izgara - ilk görünen öğe veya seçim
                if self.files.is_empty() {
                    None
                } else if self.view_mode == ViewMode::Grid {
                    Some(self.item_rect(win_x, win_y, win_w, 0, 0))
                } else {
                    Some(Self::row_rect(win_x, win_y, win_w, 0))
                }
            }
            6 => Some(Self::search_rect(win_x, win_y, win_w)),
            _ => None,
        };
        if let Some((x, y, w, h)) = focused_rect {
            MinGuiCompositor::draw_focus_ring(fb, x, y, w, h);
        }
    }

    /// Hızlı Önizleme panelini çizer (seçili öğe bilgisi + içerik/metin/küçük resim).
    pub fn render_preview(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let (px, py, pw, ph) = self.preview_rect(win_x, win_y, win_w, win_h);
        if pw < 40 || ph < 40 {
            return;
        }
        fb.draw_rect(px, py, pw, ph, Color::TITLE_BAR_INACTIVE);
        fb.draw_rect(px, py, pw, 20, Color::TITLE_BAR);
        MinGuiCompositor::draw_string_clipped(
            fb,
            px + 8,
            py + 4,
            crate::gui::ui_text("Hızlı Önizleme", "Quick Preview"),
            Color::ACCENT_BLUE,
            (px + 6, py + 2, pw.saturating_sub(12), 18),
        );

        let inner_x = px + 8;
        let inner_w = pw.saturating_sub(16);
        let clip = (inner_x, py + 22, inner_w, ph.saturating_sub(28));
        let mut yy = py + 26;

        if self.preview_kind == PreviewKind::Empty {
            crate::gui::ui_helpers::draw_empty_state(
                fb,
                inner_x,
                yy,
                inner_w,
                ph.saturating_sub(34),
                crate::gui::ui_helpers::EmptyStateIcon::NoSelection,
                crate::gui::tr(crate::gui::I18nKey::NoSelection),
                crate::gui::ui_text(
                    "Önizlemek için bir öğe seçin.",
                    "Select an item to preview.",
                ),
                None,
                false,
            );
            return;
        }

        // Küçük resim veya büyük tür simgesi.
        if self.preview_kind == PreviewKind::Image && self.preview_has_img {
            if let Some(t) = self.thumbs.get(self.selected_idx).and_then(|t| t.as_ref()) {
                let box_w = inner_w.min(180);
                let box_h = 110usize.min(ph / 2);
                Self::draw_thumb(fb, inner_x, yy, box_w, box_h, t);
                yy += box_h + 8;
            }
        } else {
            let is_dir = self.preview_kind == PreviewKind::Directory;
            if is_dir {
                Self::draw_folder_icon_sized(fb, inner_x, yy, 2, false);
            } else {
                let tag = self
                    .files
                    .get(self.selected_idx)
                    .map(|f| f.icon_tag)
                    .unwrap_or("");
                Self::draw_file_icon_sized(fb, inner_x, yy, 2, tag);
            }
            yy += 34;
        }

        // Başlık (ad), meta (tür/boyut), ayraç ve içerik satırları.
        MinGuiCompositor::draw_string_clipped(
            fb,
            inner_x,
            yy,
            self.preview_title.as_str(),
            Color::TEXT_MAIN,
            clip,
        );
        yy += 16;
        MinGuiCompositor::draw_string_clipped(
            fb,
            inner_x,
            yy,
            self.preview_meta.as_str(),
            Color::ACCENT_GREEN,
            clip,
        );
        yy += 16;
        fb.draw_rect(inner_x, yy, inner_w, 1, Color::BORDER);
        yy += 6;

        let line_h = 13usize;
        for k in 0..self.preview_line_count {
            if yy + line_h > py + ph - 4 {
                break;
            }
            let col = match self.preview_kind {
                PreviewKind::Text => Color::TEXT_MAIN,
                _ => Color::TEXT_MUTED,
            };
            MinGuiCompositor::draw_string_clipped(
                fb,
                inner_x,
                yy,
                self.preview_lines[k].as_str(),
                col,
                clip,
            );
            yy += line_h;
        }
        if self.preview_kind == PreviewKind::File {
            MinGuiCompositor::draw_string_clipped(
                fb,
                inner_x,
                yy,
                crate::gui::ui_text("(ikili dosya - önizleme yok)", "(binary file - no preview)"),
                Color::TEXT_MUTED,
                clip,
            );
        }
    }

    /// Onay iletişim kutusunu çizer.
    /// Uzun işlem bandını durum çubuğunun hemen üstünde çizer.
    ///
    /// Kart: işlem adı + yüzde + adım sayısı + yuvarlatılmış ilerleme çubuğu.
    fn render_progress_banner(
        fb: &mut Framebuffer,
        p: OpProgress,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let margin = 12usize;
        let banner_w = win_w.saturating_sub(margin * 2);
        if banner_w < 80 || win_h < STATUS_H + 80 {
            return;
        }
        let banner_h = 44usize;
        let bx = win_x + margin;
        let by = win_y + win_h - STATUS_H - banner_h - 6;
        fb.draw_shadow(bx, by, banner_w, banner_h, Color::SHADOW);
        fb.draw_rounded_rect(bx, by, banner_w, banner_h, 6, Color::TASKBAR_BG);
        fb.draw_rounded_rect_outline(bx, by, banner_w, banner_h, 6, Color::ACCENT_BLUE);

        MinGuiCompositor::draw_string_clipped(
            fb,
            bx + 10,
            by + 6,
            p.label_str(),
            Color::TEXT_MAIN,
            (bx + 8, by + 4, banner_w.saturating_sub(120), 16),
        );
        let detail = p.detail_text();
        let dw = MinGuiCompositor::text_width(detail.as_str(), 1);
        MinGuiCompositor::draw_string(
            fb,
            bx + banner_w.saturating_sub(dw + 10),
            by + 6,
            detail.as_str(),
            Color::TEXT_MUTED,
        );
        let bar = (
            bx + 10,
            by + banner_h.saturating_sub(16),
            banner_w.saturating_sub(20),
            6usize,
        );
        MinGuiCompositor::render_toast_progress_bar(fb, bar, p.percent, Color::ACCENT_BLUE);
    }

    fn render_confirm_dialog(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let dialog_w = 400.min(win_w.saturating_sub(40));
        let dialog_h = 180;
        let dialog_x = win_x + (win_w.saturating_sub(dialog_w)) / 2;
        let dialog_y = win_y + (win_h.saturating_sub(dialog_h)) / 2;

        // Arka plan gölgelendirme
        fb.draw_rect(win_x, win_y, win_w, win_h, Color::SHADOW);

        // İletişim kutusu
        fb.draw_rounded_rect(dialog_x, dialog_y, dialog_w, dialog_h, 8, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(
            dialog_x,
            dialog_y,
            dialog_w,
            dialog_h,
            8,
            Color::ACCENT_BLUE,
        );

        // Başlık çubuğu
        fb.draw_rounded_rect(dialog_x, dialog_y, dialog_w, 36, 8, Color::TITLE_BAR);
        let title = match self.confirm_dialog {
            ConfirmDialog::Delete => crate::gui::ui_text("Silme Onayı", "Confirm Delete"),
            ConfirmDialog::DeletePermanent => {
                crate::gui::ui_text("Kalıcı Silme Onayı", "Confirm Permanent Delete")
            }
            ConfirmDialog::NewFile => crate::gui::ui_text("Yeni Dosya Oluştur", "Create New File"),
            ConfirmDialog::NewFolder => {
                crate::gui::ui_text("Yeni Klasör Oluştur", "Create New Folder")
            }
            ConfirmDialog::Rename => crate::gui::ui_text("Yeniden Adlandır", "Rename"),
            ConfirmDialog::None => "",
        };
        let title_w = MinGuiCompositor::text_width(title, 1);
        MinGuiCompositor::draw_string(
            fb,
            dialog_x + (dialog_w.saturating_sub(title_w)) / 2,
            dialog_y + 10,
            title,
            Color::TEXT_MAIN,
        );

        // İçerik
        let content_y = dialog_y + 50;
        match self.confirm_dialog {
            ConfirmDialog::Delete | ConfirmDialog::DeletePermanent => {
                let msg = if self.confirm_dialog == ConfirmDialog::Delete {
                    crate::gui::ui_text(
                        "Seçili öğeler Geri Dönüşüm Kutusu'na taşınacak. Devam etmek istiyor musunuz?",
                        "Selected items will be moved to Recycle Bin. Continue?",
                    )
                } else {
                    crate::gui::ui_text(
                        "Seçili öğeler KALICI OLARAK silinecek. Bu işlem geri alınamaz! Devam?",
                        "Selected items will be PERMANENTLY deleted. This cannot be undone! Continue?",
                    )
                };
                let msg_w = MinGuiCompositor::text_width(msg, 1);
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    dialog_x + (dialog_w.saturating_sub(msg_w)) / 2,
                    content_y,
                    msg,
                    Color::TEXT_MAIN,
                    (dialog_x + 20, content_y, dialog_w - 40, 40),
                );

                // Öğe sayısı
                let mut count = 0;
                for i in 0..self.file_count {
                    if self.selected_items[i] && self.search_matches(i) {
                        count += 1;
                    }
                }
                let count_tr = alloc::format!("{} öğe seçili", count);
                let count_en = alloc::format!("{} items selected", count);
                let count_msg = crate::gui::ui_text(&count_tr, &count_en);
                let count_w = MinGuiCompositor::text_width(count_msg, 1);
                MinGuiCompositor::draw_string(
                    fb,
                    dialog_x + (dialog_w.saturating_sub(count_w)) / 2,
                    content_y + 30,
                    count_msg,
                    Color::ACCENT_YELLOW,
                );
            }
            ConfirmDialog::NewFile | ConfirmDialog::NewFolder => {
                let label = if self.confirm_dialog == ConfirmDialog::NewFile {
                    crate::gui::ui_text("Dosya adı:", "File name:")
                } else {
                    crate::gui::ui_text("Klasör adı:", "Folder name:")
                };
                MinGuiCompositor::draw_string(
                    fb,
                    dialog_x + 20,
                    content_y,
                    label,
                    Color::TEXT_MUTED,
                );

                // İsim giriş kutusu
                let input_x = dialog_x + 20;
                let input_y = content_y + 22;
                let input_w = dialog_w - 40;
                fb.draw_rect(input_x, input_y, input_w, 28, Color::DARK_BG);
                fb.draw_rect_outline(input_x, input_y, input_w, 28, Color::BORDER);

                let name_str =
                    core::str::from_utf8(&self.confirm_name_buf[..self.confirm_name_len])
                        .unwrap_or("");
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    input_x + 8,
                    input_y + 6,
                    name_str,
                    Color::TEXT_MAIN,
                    (input_x + 6, input_y + 2, input_w - 12, 24),
                );

                // İmleç
                let prefix = core::str::from_utf8(&self.confirm_name_buf[..self.confirm_cursor])
                    .unwrap_or("");
                let cx = input_x + 8 + MinGuiCompositor::text_width(prefix, 1);
                fb.draw_rect(cx, input_y + 6, 1, 16, Color::ACCENT_BLUE);
            }
            ConfirmDialog::Rename => {
                let label = crate::gui::ui_text("Yeni ad:", "New name:");
                MinGuiCompositor::draw_string(
                    fb,
                    dialog_x + 20,
                    content_y,
                    label,
                    Color::TEXT_MUTED,
                );

                // İsim giriş kutusu
                let input_x = dialog_x + 20;
                let input_y = content_y + 22;
                let input_w = dialog_w - 40;
                fb.draw_rect(input_x, input_y, input_w, 28, Color::DARK_BG);
                fb.draw_rect_outline(input_x, input_y, input_w, 28, Color::BORDER);

                let name_str =
                    core::str::from_utf8(&self.confirm_name_buf[..self.confirm_name_len])
                        .unwrap_or("");
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    input_x + 8,
                    input_y + 6,
                    name_str,
                    Color::TEXT_MAIN,
                    (input_x + 6, input_y + 2, input_w - 12, 24),
                );

                // İmleç
                let prefix = core::str::from_utf8(&self.confirm_name_buf[..self.confirm_cursor])
                    .unwrap_or("");
                let cx = input_x + 8 + MinGuiCompositor::text_width(prefix, 1);
                fb.draw_rect(cx, input_y + 6, 1, 16, Color::ACCENT_BLUE);
            }
            ConfirmDialog::None => {}
        }

        // Butonlar
        let btn_y = dialog_y + dialog_h - 44;
        let btn_w = 80;
        let btn_h = 28;
        let gap = 20;
        let total_btn_w = 2 * btn_w + gap;
        let btn_start_x = dialog_x + (dialog_w.saturating_sub(total_btn_w)) / 2;

        // İptal butonu
        let cancel_x = btn_start_x;
        fb.draw_rounded_rect(cancel_x, btn_y, btn_w, btn_h, 4, Color::TASKBAR_BG);
        fb.draw_rounded_rect_outline(cancel_x, btn_y, btn_w, btn_h, 4, Color::BORDER);
        let cancel_label = crate::gui::ui_text("İptal", "Cancel");
        let cancel_w = MinGuiCompositor::text_width(cancel_label, 1);
        MinGuiCompositor::draw_string(
            fb,
            cancel_x + (btn_w.saturating_sub(cancel_w)) / 2,
            btn_y + 7,
            cancel_label,
            Color::TEXT_MAIN,
        );

        // Onay butonu
        let ok_x = btn_start_x + btn_w + gap;
        let ok_color = match self.confirm_dialog {
            ConfirmDialog::DeletePermanent => Color::ACCENT_RED,
            ConfirmDialog::Delete => Color::ACCENT_YELLOW,
            _ => Color::ACCENT_GREEN,
        };
        fb.draw_rounded_rect(ok_x, btn_y, btn_w, btn_h, 4, ok_color);
        let ok_label = crate::gui::ui_text("Onayla", "Confirm");
        let ok_w = MinGuiCompositor::text_width(ok_label, 1);
        MinGuiCompositor::draw_string(
            fb,
            ok_x + (btn_w.saturating_sub(ok_w)) / 2,
            btn_y + 7,
            ok_label,
            MinGuiCompositor::text_on(ok_color),
        );
    }
}

impl Default for FileManagerApp {
    fn default() -> Self {
        Self::new()
    }
}

pub fn file_kind(name: &str) -> &'static str {
    let lower = |b: u8| if b.is_ascii_uppercase() { b + 32 } else { b };
    let bytes = name.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        if bytes[i] == b'.' {
            let ext = &bytes[i + 1..];
            let matches_ext = |want: &[u8]| {
                ext.len() == want.len() && ext.iter().zip(want).all(|(a, b)| lower(*a) == *b)
            };
            if matches_ext(b"txt") || matches_ext(b"md") || matches_ext(b"nfo") {
                return "TXT";
            }
            if matches_ext(b"cfg") || matches_ext(b"ini") {
                return "CFG";
            }
            if matches_ext(b"log") {
                return "LOG";
            }
            if matches_ext(b"3d") || matches_ext(b"obj") {
                return "3D";
            }
            if matches_ext(b"bmp")
                || matches_ext(b"png")
                || matches_ext(b"jpg")
                || matches_ext(b"jpeg")
                || matches_ext(b"gif")
            {
                return "IMG";
            }
            if matches_ext(b"wav")
                || matches_ext(b"mp3")
                || matches_ext(b"flac")
                || matches_ext(b"ogg")
            {
                return "SND";
            }
            if matches_ext(b"mp4")
                || matches_ext(b"avi")
                || matches_ext(b"mkv")
                || matches_ext(b"webm")
            {
                return "VID";
            }
            if matches_ext(b"htm")
                || matches_ext(b"html")
                || matches_ext(b"url")
                || matches_ext(b"xhtml")
            {
                return "WEB";
            }
            return "BIN";
        }
    }
    "BIN"
}

pub fn format_size(bytes: usize, is_dir: bool) -> TextBuf {
    let mut out = TextBuf::new();
    if is_dir {
        out.push_str("<DIR>");
        return out;
    }
    if bytes >= 1024 * 1024 {
        out.push_usize(bytes / (1024 * 1024));
        out.push_byte(b'.');
        out.push_usize((bytes / (1024 * 102)) % 10);
        out.push_str(" MB");
    } else if bytes >= 1024 {
        out.push_usize(bytes / 1024);
        out.push_byte(b'.');
        out.push_usize((bytes / 102) % 10);
        out.push_str(" KB");
    } else {
        out.push_usize(bytes);
        out.push_str(" B");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test listesi: ".." + üç dosya. `..` öğesi hiçbir zaman seçilmez.
    fn sample_app() -> FileManagerApp {
        let mut app = FileManagerApp::new();
        let names = ["..", "a.txt", "b.txt", "c.txt"];
        for (i, n) in names.iter().enumerate() {
            let mut item = FileItem::empty();
            item.name = n;
            item.is_dir = *n == "..";
            app.files[i] = item;
        }
        app.file_count = names.len();
        app.selected_idx = 0;
        app.selected_items = [false; MAX_ITEMS];
        app.selection_anchor = 0;
        app
    }

    #[test]
    fn plain_down_moves_cursor_and_selects_only_it() {
        let mut app = sample_app();
        app.move_selection_down_ext(500, SelectMod::None);
        assert_eq!(app.selected_idx, 1);
        assert!(app.is_row_selected(1));
        assert_eq!(app.selected_count(), 1);
        app.move_selection_down_ext(500, SelectMod::None);
        assert_eq!(app.selected_idx, 2);
        assert!(app.is_row_selected(2));
        assert!(!app.is_row_selected(1), "tek seçim öncekini bırakmalı");
    }

    #[test]
    fn shift_arrows_build_a_range_from_the_anchor() {
        let mut app = sample_app();
        // İlk görünür öğeye in, çapayı orada kur.
        app.move_selection_down_ext(500, SelectMod::None);
        assert_eq!(app.selected_idx, 1);
        app.move_selection_down_ext(500, SelectMod::Shift);
        app.move_selection_down_ext(500, SelectMod::Shift);
        assert_eq!(app.selected_idx, 3);
        // Çapa sabit: 1..=3 aralığı seçili.
        for i in 1..=3 {
            assert!(app.is_row_selected(i), "satır {i} aralıkta olmalı");
        }
        assert!(!app.is_row_selected(0), "'..' seçilmemeli");
        // Yukarı dönünce aralık daralır (çapa yine 1'de).
        app.move_selection_up_ext(500, SelectMod::Shift);
        assert_eq!(app.selected_idx, 2);
        assert!(app.is_row_selected(1) && app.is_row_selected(2));
        assert!(!app.is_row_selected(3));
    }

    #[test]
    fn ctrl_arrows_move_the_cursor_without_touching_the_selection() {
        let mut app = sample_app();
        app.move_selection_down_ext(500, SelectMod::None); // a.txt seçili
        app.move_selection_down_ext(500, SelectMod::Ctrl);
        assert_eq!(app.selected_idx, 2);
        assert!(app.is_row_selected(1), "Ctrl seçimi korumalı");
        assert!(!app.is_row_selected(2), "Ctrl yeni satır seçmemeli");
        assert!(app.is_cursor_row(2), "imleç yeni satırda");
    }

    #[test]
    fn ctrl_shift_toggles_individual_rows() {
        let mut app = sample_app();
        app.move_selection_down_ext(500, SelectMod::None); // 1 seçili
        app.move_selection_down_ext(500, SelectMod::CtrlShift); // 2 ekle
        assert!(app.is_row_selected(1) && app.is_row_selected(2));
        app.move_selection_up_ext(500, SelectMod::CtrlShift); // 1 çıkar
        assert!(!app.is_row_selected(1));
        assert!(app.is_row_selected(2));
    }

    #[test]
    fn select_all_skips_parent_entry_and_clear_removes_it() {
        let mut app = sample_app();
        assert_eq!(app.select_all(), 3);
        assert_eq!(app.selected_count(), 3);
        assert!(!app.is_row_selected(0));
        assert!(app.clear_selection());
        assert_eq!(app.selected_count(), 0);
        assert!(!app.clear_selection(), "boş seçimi temizlemek etkisiz");
    }

    #[test]
    fn home_and_end_respect_modifiers() {
        let mut app = sample_app();
        app.select_last_ext(SelectMod::None);
        assert_eq!(app.selected_idx, 3);
        app.select_first_ext(SelectMod::Shift);
        assert_eq!(app.selected_idx, 1);
        // 1..=3 aralığı seçili olmalı (çapa 3'te).
        assert!(app.is_row_selected(1) && app.is_row_selected(3));
    }

    #[test]
    fn go_up_walks_to_root_and_stops() {
        let mut app = FileManagerApp::new();
        app.set_path("/root/Documents");
        assert_eq!(app.path(), "/root/Documents");
        assert!(app.go_up());
        assert_eq!(app.path(), "/root");
        assert!(app.go_up());
        assert_eq!(app.path(), "/");
        assert!(!app.go_up(), "kök dizinde Backspace etkisiz");
        assert_eq!(app.path(), "/");
    }

    #[test]
    fn out_of_range_targets_are_ignored() {
        let mut app = sample_app();
        app.move_cursor_to(99, SelectMod::None);
        assert_eq!(app.selected_idx, 0);
        assert_eq!(app.selected_count(), 0);
    }

    #[test]
    fn mod_is_derived_from_shift_and_ctrl_flags() {
        assert_eq!(SelectMod::from_keys(false, false), SelectMod::None);
        assert_eq!(SelectMod::from_keys(true, false), SelectMod::Shift);
        assert_eq!(SelectMod::from_keys(false, true), SelectMod::Ctrl);
        assert_eq!(SelectMod::from_keys(true, true), SelectMod::CtrlShift);
        assert!(SelectMod::Shift.is_range());
        assert!(!SelectMod::Ctrl.is_range());
    }

    #[test]
    fn op_progress_is_monotonic_and_counts_steps() {
        let mut p = OpProgress::new(("Kopyalanıyor", "Copying"), 4);
        assert_eq!(p.percent, 0);
        p.set_steps(1, 4);
        assert_eq!(p.percent, 25);
        // Geri sarma yüzdeyi düşürmez.
        p.set_steps(0, 4);
        assert_eq!(p.percent, 25);
        p.set_steps(3, 4);
        assert_eq!(p.percent, 75);
        assert_eq!(p.done, 3);
        p.set_steps(4, 4);
        assert_eq!(p.percent, 100);
        // Sıfır toplam = belirsiz işlem, %100'e yuvarlanır.
        let mut z = OpProgress::new(("x", "x"), 0);
        z.set_steps(0, 0);
        assert_eq!(z.percent, 100);
    }

    #[test]
    fn op_progress_detail_text_has_percent_and_steps() {
        let mut p = OpProgress::new(("Siliniyor", "Deleting"), 8);
        p.set_steps(2, 8);
        assert_eq!(p.detail_text().as_str(), "25%  2/8");
        let q = OpProgress::new(("x", "x"), 0);
        assert_eq!(q.detail_text().as_str(), "0%");
    }

    #[test]
    fn progress_banner_lifecycle() {
        let mut app = sample_app();
        assert!(!app.is_busy());
        app.begin_progress(("Kopyalanıyor", "Copying"), 4, 0);
        assert!(app.is_busy());
        app.update_progress(2, 4);
        assert_eq!(app.progress.map(|p| p.percent), Some(50));
        app.finish_progress();
        assert!(!app.is_busy());
        assert!(app.progress.is_none());
    }

    #[test]
    fn selected_paths_prefers_multi_selection() {
        let mut app = sample_app();
        app.move_cursor_to(1, SelectMod::None);
        let single = app.selected_paths();
        assert_eq!(single.len(), 1);
        assert!(single[0].ends_with("/a.txt"), "yol: {}", single[0]);

        app.select_all();
        let many = app.selected_paths();
        assert_eq!(many.len(), 3);
        assert!(many.iter().all(|p| !p.ends_with("/..")));
    }
}
