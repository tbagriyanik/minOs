//! GUI için sabit kapasiteli kirli bölge takibi.

use core::cmp::{max, min};

pub const MAX_DIRTY_RECTS: usize = 64;

/// Takip edilebilen maksimum pencere sayısı (uygulama + sanal masaüstü
/// pencereleri; `gui::window::MAX_WINDOWS` ile ayni kapasitedir).
pub const MAX_TRACKED_WINDOWS: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl DirtyRect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn union(self, other: Self) -> Self {
        let left = min(self.x, other.x);
        let top = min(self.y, other.y);
        let right = max(
            self.x.saturating_add(self.width),
            other.x.saturating_add(other.width),
        );
        let bottom = max(
            self.y.saturating_add(self.height),
            other.y.saturating_add(other.height),
        );
        Self::new(
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        )
    }
}

/// İki kirli bölgeyi birlestirir.
///
/// `None` girdiler "bu kaynaktan iş yok" demektir. Her iki bölge de varsa
/// birleşim (bounding) dikdörtgen döner. Böylece aynı karede gelen iki
/// farklı güncelleme kaynağı (orn. sürükleme + uygulama değişikliği)
/// birbirini ezmeden birlikte ele alınır.
pub fn merge_regions(a: Option<DirtyRect>, b: Option<DirtyRect>) -> Option<DirtyRect> {
    match (a, b) {
        (None, None) => None,
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (Some(x), Some(y)) => Some(x.union(y)),
    }
}

pub struct GlobalDirtyTracker {
    rects: [DirtyRect; MAX_DIRTY_RECTS],
    len: usize,
    overflowed: bool,
    /// Tüm izlenen kirli rectlerin birleşimi (bounding rect).
    /// MAX_DIRTY_RECTS'yi aştığında bile güncellenir; needs_full_redraw()
    /// bu birleşimi kullanarak tam ekran mı kontrol eder.
    bounding: DirtyRect,
    /// Per pencere render listesi: pencere başına birleşik kirli dikdörtgen.
    /// `DirtyRect::new(0, 0, 0, 0)` = o pencere için henüz kirli bölge yok.
    window_rects: [DirtyRect; MAX_TRACKED_WINDOWS],
    /// Tüm per pencere kirli rectlerin birleşimi (bounding rect).
    ///
    /// `mark_window` ile giren işaretler de cercevenin kirli bölgesine
    /// dahil edilmelidir: aksi halde pencere "kendi listesinde kirli"
    /// olmasına rağmen cerceve kirli dikdörtgeni onu kapsamaz ve
    /// pencere yalnızca kırpma bölgesi kadar yeniden çizilir. Böylece
    /// pencerenin değişen kısmı ekranda bayat kalır (parça parça güncelleme).
    window_bounding: DirtyRect,
}

impl GlobalDirtyTracker {
    pub const fn new() -> Self {
        Self {
            rects: [DirtyRect::new(0, 0, 0, 0); MAX_DIRTY_RECTS],
            len: 0,
            overflowed: false,
            bounding: DirtyRect::new(0, 0, 0, 0),
            window_rects: [DirtyRect::new(0, 0, 0, 0); MAX_TRACKED_WINDOWS],
            window_bounding: DirtyRect::new(0, 0, 0, 0),
        }
    }

    pub fn track(&mut self, window_id: usize, rect: DirtyRect) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        // Bounding rect'i her zaman güncelle (overflow olsa bile). İlk rect
        // `(0,0,0,0)` başlangıcıyla birleştirilirse sol/üst kenarlar yanlışlıkla
        // 0'a kayar; ilk izlemede doğrudan rect'i kullan.
        if self.bounding.width == 0 || self.bounding.height == 0 {
            self.bounding = rect;
        } else {
            self.bounding = self.bounding.union(rect);
        }
        if self.len < MAX_DIRTY_RECTS {
            self.rects[self.len] = rect;
            self.len += 1;
        } else {
            self.overflowed = true;
        }
        self.mark_window(window_id, rect);
    }

    /// Bir pencereye kirli dikdörtgen ekler; per pencere render listesini
    /// günceller (union ile).
    ///
    /// İşaret aynı zamanda cercevenin genel kirli bölgesine de yansıtılır:
    /// yalnızca per pencere listesine yazmak, pencereyi "kirli" yapmasına
    /// rağmen cerceve kirli dikdörtgeninde kapsam dışı bırakır ve pencere
    /// kırpma bölgesi kadar çizilir. Sonuc: pencerenin bir kısmı yeni,
    /// bir kısmı eski (bayat) görünür.
    pub fn mark_window(&mut self, window_id: usize, rect: DirtyRect) {
        if window_id < MAX_TRACKED_WINDOWS && rect.width > 0 && rect.height > 0 {
            let current = self.window_rects[window_id];
            let merged = if current.width == 0 && current.height == 0 {
                rect
            } else {
                current.union(rect)
            };
            self.window_rects[window_id] = merged;
            if self.window_bounding.width == 0 || self.window_bounding.height == 0 {
                self.window_bounding = merged;
            } else {
                self.window_bounding = self.window_bounding.union(merged);
            }
        }
    }

    /// Bir pencerenin kirli bölge render listesini sorgular; temizse `None`.
    pub fn window_dirty(&self, window_id: usize) -> Option<DirtyRect> {
        if window_id >= MAX_TRACKED_WINDOWS {
            return None;
        }
        let rect = self.window_rects[window_id];
        if rect.width == 0 || rect.height == 0 {
            None
        } else {
            Some(rect)
        }
    }

    /// Bir pencerenin kirli bölge render listesini temizler.
    pub fn clear_window(&mut self, window_id: usize) {
        if window_id < MAX_TRACKED_WINDOWS {
            self.window_rects[window_id] = DirtyRect::new(0, 0, 0, 0);
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.overflowed = false;
        self.bounding = DirtyRect::new(0, 0, 0, 0);
        self.window_bounding = DirtyRect::new(0, 0, 0, 0);
        for rect in self.window_rects.iter_mut() {
            *rect = DirtyRect::new(0, 0, 0, 0);
        }
    }

    pub fn as_slice(&self) -> &[DirtyRect] {
        &self.rects[..self.len]
    }

    /// Takipçide bekleyen herhangi bir çizim işi var mı?
    ///
    /// Per pencere (`mark_window`) işaretleri de hesaba katılır: yalnızca
    /// `track` çağrılarına bakmak, yalnızca pencere işaretleri bulunan
    /// karelerde redraw tetiklenmemesine ve pencere içeriğinin bayat
    /// kalmasına yol açardı.
    pub fn is_empty(&self) -> bool {
        self.len == 0
            && !self.overflowed
            && self.window_bounding.width == 0
            && self.window_bounding.height == 0
    }
    /// Tam ekran redraw gerekip gerekmediğini belirler.
    /// GlobalDirtyTracker MAX_DIRTY_RECTS (64) Capacity aşıldığında bile,
    /// birleşimi (bounding rect) hesaba katılarak daha akıllı redraw kararı
    /// verilebilir: bounding rect ekranın tamamını kapsıyorsa full redraw,
    /// aksi halde partial redraw mümkün.
    pub fn needs_full_redraw(&self) -> bool {
        // Eğer overflow olmadıysa eski davranışa devam et.
        if !self.overflowed {
            return false;
        }
        // Overflow olundu: bounding rect'e bakarak tam ekran mı?
        // Boyutlar sıfırsa anlamı yok.
        self.bounding.width > 0 && self.bounding.height > 0
    }

    /// Bu turun kirli bölge birleşimi (global + per pencere).
    ///
    /// Per pencere işaretleri de dahil edilir; aksi halde bir pencere
    /// "kendi listesinde kirli" olduğu halde cerceve kirli dikdörtgeni onu
    /// kapsamaz ve pencere yalnizca kirpma bolgesi kadar cizilir.
    pub fn bounding_rect(&self) -> Option<DirtyRect> {
        let global = if self.bounding.width == 0 || self.bounding.height == 0 {
            None
        } else {
            Some(self.bounding)
        };
        let per_window = if self.window_bounding.width == 0 || self.window_bounding.height == 0 {
            None
        } else {
            Some(self.window_bounding)
        };
        match (global, per_window) {
            (None, None) => None,
            (Some(g), None) => Some(g),
            (None, Some(w)) => Some(w),
            (Some(g), Some(w)) => Some(g.union(w)),
        }
    }

    pub fn take(&mut self) -> ([DirtyRect; MAX_DIRTY_RECTS], usize, bool) {
        let result = (self.rects, self.len, self.overflowed);
        self.clear();
        result
    }
}

impl Default for GlobalDirtyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_covers_both_rectangles() {
        let a = DirtyRect::new(10, 20, 30, 15);
        let b = DirtyRect::new(25, 5, 20, 30);
        assert_eq!(a.union(b), DirtyRect::new(10, 5, 35, 30));
    }

    #[test]
    fn zero_sized_rectangles_are_ignored() {
        let mut tracker = GlobalDirtyTracker::new();
        tracker.track(0, DirtyRect::new(0, 0, 0, 10));
        tracker.track(0, DirtyRect::new(0, 0, 10, 0));
        assert!(tracker.is_empty());
    }

    #[test]
    fn overflow_requests_full_redraw() {
        let mut tracker = GlobalDirtyTracker::new();
        for i in 0..=MAX_DIRTY_RECTS {
            tracker.track(0, DirtyRect::new(i, 0, 1, 1));
        }
        assert_eq!(tracker.as_slice().len(), MAX_DIRTY_RECTS);
        assert!(tracker.needs_full_redraw());
        tracker.clear();
        assert!(tracker.is_empty());
        assert!(!tracker.needs_full_redraw());
    }

    #[test]
    fn bounding_rect_covers_tracked_regions() {
        let mut tracker = GlobalDirtyTracker::new();
        tracker.track(0, DirtyRect::new(10, 10, 5, 5));
        tracker.track(0, DirtyRect::new(30, 20, 10, 10));
        assert_eq!(
            tracker.bounding_rect(),
            Some(DirtyRect::new(10, 10, 30, 20))
        );
    }

    #[test]
    fn take_returns_and_clears_frame_regions() {
        let mut tracker = GlobalDirtyTracker::new();
        tracker.track(0, DirtyRect::new(1, 2, 3, 4));
        let (rects, len, overflowed) = tracker.take();
        assert_eq!(len, 1);
        assert_eq!(rects[0], DirtyRect::new(1, 2, 3, 4));
        assert!(!overflowed);
        assert!(tracker.is_empty());
    }

    #[test]
    fn per_window_dirty_lists_track_union_per_window() {
        let mut tracker = GlobalDirtyTracker::new();
        // Pencere 3'a iki kisimli kirli bolge → birlesik rect.
        tracker.mark_window(3, DirtyRect::new(10, 20, 30, 40));
        tracker.mark_window(3, DirtyRect::new(50, 60, 30, 40));
        // Union: x=[10,80) width=70, y=[20,100) height=80
        assert_eq!(
            tracker.window_dirty(3),
            Some(DirtyRect::new(10, 20, 70, 80))
        );
        // Farkli pencere kirli listesi izole olmali.
        assert_eq!(
            tracker.window_dirty(4),
            None,
            "pencere 4 kirli listesine kayitli degil"
        );
        tracker.mark_window(4, DirtyRect::new(0, 0, 10, 10));
        assert_eq!(tracker.window_dirty(4), Some(DirtyRect::new(0, 0, 10, 10)));
        // Track uzerinden de per pencere listesi dolmalı (geriye uyum).
        tracker.track(2, DirtyRect::new(5, 5, 8, 8));
        assert_eq!(tracker.window_dirty(2), Some(DirtyRect::new(5, 5, 8, 8)));
        assert_eq!(tracker.window_dirty(999), None, "sinir disi pencere id");
    }

    #[test]
    fn clear_window_resets_only_that_windows_list() {
        let mut tracker = GlobalDirtyTracker::new();
        tracker.mark_window(1, DirtyRect::new(1, 2, 3, 4));
        tracker.mark_window(2, DirtyRect::new(5, 6, 7, 8));
        tracker.clear_window(1);
        assert_eq!(tracker.window_dirty(1), None);
        assert_eq!(tracker.window_dirty(2), Some(DirtyRect::new(5, 6, 7, 8)));
        tracker.clear();
        assert_eq!(tracker.window_dirty(2), None);
        assert!(tracker.is_empty());
    }

    #[test]
    fn per_window_marks_count_as_pending_work() {
        // Yalnizca mark_window çağrısı varsa takipçi boş sayılmaz:
        // aksi halde pencere değişikliği redraw tetiklemez ve içerik bayat kalır.
        let mut tracker = GlobalDirtyTracker::new();
        assert!(tracker.is_empty());
        tracker.mark_window(3, DirtyRect::new(40, 50, 60, 70));
        assert!(
            !tracker.is_empty(),
            "per pencere isareti bekleyen is olarak sayilmali"
        );
        assert_eq!(
            tracker.bounding_rect(),
            Some(DirtyRect::new(40, 50, 60, 70)),
            "per pencere isareti cerceve kirli bolgesine dahil edilmeli"
        );
    }

    #[test]
    fn bounding_rect_unions_global_and_per_window_regions() {
        let mut tracker = GlobalDirtyTracker::new();
        tracker.track(0, DirtyRect::new(0, 900, 100, 20));
        tracker.mark_window(5, DirtyRect::new(300, 200, 100, 100));
        let merged = tracker.bounding_rect().expect("birlesik bolge");
        // Global: x=[0,100) y=[900,920); per pencere: x=[300,400) y=[200,300)
        assert_eq!(merged, DirtyRect::new(0, 200, 400, 720));
    }

    #[test]
    fn clear_resets_per_window_bounding() {
        let mut tracker = GlobalDirtyTracker::new();
        tracker.mark_window(2, DirtyRect::new(10, 10, 20, 20));
        tracker.clear();
        assert!(tracker.is_empty());
        assert_eq!(tracker.bounding_rect(), None);
    }

    #[test]
    fn merge_regions_unions_both_sources() {
        let a = DirtyRect::new(10, 10, 30, 30);
        let b = DirtyRect::new(100, 40, 20, 20);
        assert_eq!(merge_regions(Some(a), Some(b)), Some(a.union(b)));
        assert_eq!(merge_regions(Some(a), None), Some(a));
        assert_eq!(merge_regions(None, Some(b)), Some(b));
        assert_eq!(merge_regions(None, None), None);
    }
}
