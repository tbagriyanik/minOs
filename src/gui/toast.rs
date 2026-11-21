//! Toast notification types and rendering.
//!
//! Dört bildirim türü (bilgi, başarı, uyarı, hata) tek bir sözleşmeyle
//! sunulur:
//!
//! * Her tür kendi **rengi** ve **ikonu** ile çizilir (sol renk bandı + 16x16
//!   ikon); renk eşlemesi `ToastKind::accent` / `ToastKind::icon` ile tek
//!   yerde tanımlıdır.
//! * Her türün **varsayılan otomatik kapanma süresi** vardır
//!   (`ToastKind::default_duration_ms`); hata bildirimleri daha uzun kalır.
//! * Kullanıcı bir bildirimi **Esc** ile veya kapatma (✕) ikonuna tıklayarak
//!   kapatabilir. Fareyle üzerine gelmek bildirimi duraklatır; böylece uzun
//!   metinler okunurken kaybolmaz.
//! * Uzun işlemler (kopyalama, silme, küçük resim, format) bildirimin altında
//!   **gerçek ilerleme çubuğu** ve yüzde gösterir (`progress` alanı).
//!
//! Yaşam döngüsü saf mantıktır (`ToastNotification::advance`); çizim
//! `overlay.rs` içindeki `render_toast_progress_bar` ile paylaşılır.

extern crate alloc;

use super::*;
use alloc::string::ToString;

pub const MAX_TOASTS: usize = 4;

/// Varsayılan otomatik kapanma süreleri (ms).
pub const TOAST_INFO_MS: u64 = 3000;
pub const TOAST_SUCCESS_MS: u64 = 3000;
pub const TOAST_WARNING_MS: u64 = 4000;
pub const TOAST_ERROR_MS: u64 = 5000;
/// Süre belirtilmemiş bildirimler için emniyet ağı.
pub const TOAST_DEFAULT_MS: u64 = 4000;

/// Metin kapasiteleri (bayt).
pub const TOAST_TITLE_MAX: usize = 48;
pub const TOAST_MESSAGE_MAX: usize = 96;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

/// Bildirim ikonu (çizim tarafından yorumlanır).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastIcon {
    /// Bilgi "i" dairesi.
    Info,
    /// Başarı "✓" onay tiki.
    Success,
    /// Uyarı "!" üçgeni.
    Warning,
    /// Hata "✕" çarpı.
    Error,
}

impl ToastKind {
    /// Türün vurgu rengi (sol bant, ikon, ilerleme çubuğu).
    pub fn accent(self) -> u32 {
        match self {
            ToastKind::Info => Color::ACCENT_BLUE,
            ToastKind::Success => Color::ACCENT_GREEN,
            ToastKind::Warning => Color::ACCENT_YELLOW,
            ToastKind::Error => Color::ACCENT_RED,
        }
    }

    /// Türün ikonu.
    pub fn icon(self) -> ToastIcon {
        match self {
            ToastKind::Info => ToastIcon::Info,
            ToastKind::Success => ToastIcon::Success,
            ToastKind::Warning => ToastIcon::Warning,
            ToastKind::Error => ToastIcon::Error,
        }
    }

    /// Varsayılan otomatik kapanma süresi (ms).
    pub fn default_duration_ms(self) -> u64 {
        match self {
            ToastKind::Info => TOAST_INFO_MS,
            ToastKind::Success => TOAST_SUCCESS_MS,
            ToastKind::Warning => TOAST_WARNING_MS,
            ToastKind::Error => TOAST_ERROR_MS,
        }
    }

    /// Ekran okuyucu bildirim türü (erişilebilirlik katmanı).
    pub fn announce_kind(self) -> a11y::AnnounceKind {
        match self {
            ToastKind::Warning | ToastKind::Error => a11y::AnnounceKind::Alert,
            ToastKind::Info | ToastKind::Success => a11y::AnnounceKind::Status,
        }
    }
}

/// Gerçek ilerleme göstergesi (yüzde 0-100).
///
/// Yalnızca dönen bir kum saati yerine, işlem ilerledikçe güncellenen
/// yüzde değeri; bildirim kartının altında çubuk olarak çizilir.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ToastProgress {
    /// 0-100 arası ilerleme yüzdesi.
    percent: u8,
    /// İşlem tamamlandı mı? (Tamamlanınca çubuk dolu kalır.)
    complete: bool,
}

impl ToastProgress {
    pub const fn new(percent: u8) -> Self {
        Self {
            percent: if percent > 100 { 100 } else { percent },
            complete: false,
        }
    }

    /// Yüzdeyi döndürür (0-100).
    pub const fn percent(self) -> u8 {
        self.percent
    }

    /// İşlem tamamlandı mı?
    pub const fn is_complete(self) -> bool {
        self.complete
    }

    /// İlerlemeyi günceller. Değer yalnızca ileri gider (geri sarma yok) ve
    /// 100'e sabitlenir; `true` dönerse değer gerçekten değişti.
    pub fn set(&mut self, percent: u8) -> bool {
        let next = if percent > 100 { 100 } else { percent };
        if next <= self.percent {
            return false;
        }
        self.percent = next;
        self.complete = next == 100;
        true
    }

    /// İşlemi tamamlandı olarak işaretler (yüzde 100).
    pub fn finish(&mut self) {
        self.percent = 100;
        self.complete = true;
    }

    /// Yüzdeyi 0-100 aralığına sıkıştırılmış `usize` olarak döndürür.
    pub fn as_usize(self) -> usize {
        self.percent as usize
    }
}

#[derive(Clone)]
pub struct ToastNotification {
    pub title: [u8; TOAST_TITLE_MAX],
    pub title_len: usize,
    pub message: [u8; TOAST_MESSAGE_MAX],
    pub message_len: usize,
    pub kind: ToastKind,
    pub created_ms: u64,
    pub duration_ms: u64,
    /// Çıkış animasyonu başlangıcı; 0 = hâlâ aktif.
    pub exit_ms: u64,
    /// Otomatik kapanma sayacının son duraklatıldığı an (ms); 0 = duraklatma yok.
    pub paused_at_ms: u64,
    /// Gerçek ilerleme göstergesi; `None` = ilerleme çubuğu çizilmez.
    pub progress: Option<ToastProgress>,
}

impl ToastNotification {
    pub const fn empty() -> Self {
        Self {
            title: [0; TOAST_TITLE_MAX],
            title_len: 0,
            message: [0; TOAST_MESSAGE_MAX],
            message_len: 0,
            kind: ToastKind::Info,
            created_ms: 0,
            duration_ms: 0,
            exit_ms: 0,
            paused_at_ms: 0,
            progress: None,
        }
    }

    /// Başlığı `&str` olarak döndürür (geçersiz UTF-8 → boş dize).
    pub fn title_str(&self) -> &str {
        core::str::from_utf8(&self.title[..self.title_len]).unwrap_or("")
    }

    /// Mesajı `&str` olarak döndürür (geçersiz UTF-8 → boş dize).
    pub fn message_str(&self) -> &str {
        core::str::from_utf8(&self.message[..self.message_len]).unwrap_or("")
    }

    /// Başlık ve mesajı ayarlar (kapasiteye göre kırpılır).
    pub fn set_text(&mut self, title: &str, message: &str) {
        let tb = title.as_bytes();
        self.title_len = tb.len().min(self.title.len());
        self.title[..self.title_len].copy_from_slice(&tb[..self.title_len]);

        let mb = message.as_bytes();
        self.message_len = mb.len().min(self.message.len());
        self.message[..self.message_len].copy_from_slice(&mb[..self.message_len]);
    }

    /// Sayaç üzerindeki süre `elapsed_ms` kadar ilerlemiş sayılır.
    ///
    /// Duraklatma dikkate alınır: `paused_at_ms != 0` iken süre işlemez.
    pub fn elapsed_ms(&self, now_ms: u64) -> u64 {
        if self.paused_at_ms != 0 {
            self.paused_at_ms.saturating_sub(self.created_ms)
        } else {
            now_ms.saturating_sub(self.created_ms)
        }
    }

    /// Otomatik kapanma süresi doldu mu?
    pub fn is_expired(&self, now_ms: u64) -> bool {
        self.exit_ms == 0 && self.elapsed_ms(now_ms) >= self.duration_ms
    }

    /// Fare üzerine geldiğinde sayacı duraklatır; toplam görünme süresi
    /// değişmez (yalnızca gecikir).
    pub fn pause(&mut self, now_ms: u64) {
        if self.paused_at_ms == 0 && self.exit_ms == 0 {
            self.paused_at_ms = now_ms;
        }
    }

    /// Duraklatmayı kaldırır ve geçen süreyi sayaca ekler.
    pub fn resume(&mut self, now_ms: u64) {
        if self.paused_at_ms != 0 {
            let paused_for = now_ms.saturating_sub(self.paused_at_ms);
            self.created_ms = self.created_ms.saturating_add(paused_for);
            self.paused_at_ms = 0;
        }
    }

    /// İlerleme değerini günceller; dönen değer değişimin uygulandığını belirtir.
    pub fn set_progress(&mut self, percent: u8) -> bool {
        match &mut self.progress {
            Some(p) => p.set(percent),
            None => {
                self.progress = Some(ToastProgress::new(percent));
                true
            }
        }
    }

    /// Bir sonraki yaşam döngüsü adımını uygular ve slottan silinip
    /// silinmeyeceğini döndürür.
    ///
    /// * Süre dolduysa (ve ilerleme tamamlanmadıysa) çıkış animasyonuna girer.
    /// * Çıkış animasyonu bitince `true` (slot boşaltılır).
    pub fn advance(&mut self, now_ms: u64, slide_out_ms: u64) -> bool {
        if self.exit_ms == 0 {
            let in_progress = self.progress.is_some_and(|p| !p.is_complete());
            if in_progress {
                // İlerleme bildirimi tamamlanana kadar ekranda kalır.
                return false;
            }
            if now_ms.saturating_sub(self.created_ms) >= self.duration_ms {
                self.exit_ms = now_ms;
            }
            false
        } else {
            now_ms.saturating_sub(self.exit_ms) >= slide_out_ms
        }
    }
}

impl MinGuiCompositor {
    /// Yeni bir toast bildirimi ekler veya kuyruktaki en eski olanı kaydırır.
    ///
    /// `duration_ms == 0` ise türün varsayılan süresi kullanılır
    /// (`ToastKind::default_duration_ms`).
    pub fn push_toast(
        &mut self,
        kind: ToastKind,
        title: &str,
        message: &str,
        created_ms: u64,
        duration_ms: u64,
    ) {
        let mut t = ToastNotification::empty();
        t.kind = kind;
        t.created_ms = created_ms;
        t.duration_ms = if duration_ms == 0 {
            kind.default_duration_ms()
        } else {
            duration_ms
        };
        t.set_text(title, message);

        self.insert_toast(t);
        self.announce_toast(kind, title, created_ms);
    }

    /// Bildirimi ekran okuyucu kuyruğuna da ekler (durum/uyarı ayrımı türe göre).
    fn announce_toast(&mut self, kind: ToastKind, title: &str, now_ms: u64) {
        self.announcer
            .announce(kind.announce_kind(), title, now_ms, a11y::DEFAULT_TTL_MS);
    }

    /// İlerleme göstergeli bir bildirim ekler (kopyalama/silme/küçük resim/
    /// format gibi uzun işlemler). İlerleme 100'e ulaşana kadar bildirim
    /// otomatik kapanmaz.
    pub fn push_progress_toast(
        &mut self,
        kind: ToastKind,
        title: &str,
        message: &str,
        created_ms: u64,
        percent: u8,
    ) -> usize {
        let mut t = ToastNotification::empty();
        t.kind = kind;
        t.created_ms = created_ms;
        t.duration_ms = kind.default_duration_ms();
        t.set_text(title, message);
        t.progress = Some(ToastProgress::new(percent));

        let slot = self.insert_toast(t);
        self.announce_toast(kind, title, created_ms);
        slot
    }

    /// Var olan ilerleme bildiriminin yüzdesini günceller (yüzde ileri gider).
    ///
    /// `slot` geçersizse veya bildirim artık yoksa sessizce yok sayılır.
    pub fn update_toast_progress(&mut self, slot: usize, percent: u8) -> bool {
        if slot >= MAX_TOASTS {
            return false;
        }
        let Some(t) = self.toasts[slot].as_mut() else {
            return false;
        };
        t.set_progress(percent)
    }

    /// İlerleme bildirimini tamamlar; mesajı güncelleyip otomatik kapanmaya
    /// bırakır. `now_ms` çıkış animasyonunun referansıdır.
    pub fn complete_toast_progress(&mut self, slot: usize, message: &str, now_ms: u64) -> bool {
        if slot >= MAX_TOASTS {
            return false;
        }
        let Some(t) = self.toasts[slot].as_mut() else {
            return false;
        };
        let title = t.title_str().to_string();
        t.set_text(&title, message);
        match &mut t.progress {
            Some(p) => p.finish(),
            None => t.progress = Some(ToastProgress::new(100)),
        }
        t.created_ms = now_ms;
        t.exit_ms = 0;
        t.paused_at_ms = 0;
        true
    }

    /// İlerleme bildirimini başarısız olarak işaretler (tür hata, süre uzun).
    pub fn fail_toast_progress(&mut self, slot: usize, message: &str, now_ms: u64) -> bool {
        if slot >= MAX_TOASTS {
            return false;
        }
        let Some(t) = self.toasts[slot].as_mut() else {
            return false;
        };
        let title = t.title_str().to_string();
        t.set_text(&title, message);
        t.kind = ToastKind::Error;
        t.progress = None;
        t.created_ms = now_ms;
        t.exit_ms = 0;
        t.paused_at_ms = 0;
        t.duration_ms = ToastKind::Error.default_duration_ms();
        true
    }

    /// Boş bir slot varsa oraya, yoksa en eskisinin yerine yazar (sola kaydırma).
    /// Dönen değer bildirimin yerleştiği slot indeksidir.
    fn insert_toast(&mut self, t: ToastNotification) -> usize {
        for (i, slot) in self.toasts.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(t);
                return i;
            }
        }
        // Slotlar doluysa en eskisini (0) atıp sola kaydır, en sona ekle
        for i in 0..MAX_TOASTS - 1 {
            self.toasts[i] = self.toasts[i + 1].clone();
        }
        self.toasts[MAX_TOASTS - 1] = Some(t);
        MAX_TOASTS - 1
    }

    /// Süresi dolan bildirimleri temizler.
    /// Bildirim giriş/çıkış animasyonlarının milisaniye cinsinden süreleri.
    pub const TOAST_SLIDE_IN_MS: u64 = 160;
    pub const TOAST_SLIDE_OUT_MS: u64 = 140;

    /// Süresi dolan bildirimleri çıkış animasyonuna alır; animasyon da
    /// bitince slotu boşaltır. İlerleme bildirimleri tamamlanmadan kapanmaz.
    pub fn update_toasts(&mut self, now_ms: u64) {
        for slot in &mut self.toasts {
            if let Some(t) = slot {
                if t.advance(now_ms, Self::TOAST_SLIDE_OUT_MS) {
                    *slot = None;
                }
            }
        }
    }

    /// Tüm bildirimleri hemen kapatır (Esc). Dönen değer kapatılan bildirim
    /// sayısıdır; `0` ise tuş başka bir katmana ait olabilir.
    pub fn dismiss_toasts(&mut self) -> usize {
        let mut closed = 0usize;
        for slot in &mut self.toasts {
            if slot.take().is_some() {
                closed += 1;
            }
        }
        closed
    }

    /// En yeni (son) bildirimi kapatır. Dönen değer kapatıldıysa `true`.
    pub fn dismiss_latest_toast(&mut self) -> bool {
        for i in (0..MAX_TOASTS).rev() {
            if self.toasts[i].take().is_some() {
                return true;
            }
        }
        false
    }

    /// Etkin bildirim sayısı.
    pub fn active_toast_count(&self) -> usize {
        self.toasts.iter().filter(|t| t.is_some()).count()
    }

    /// Fare üzerine geldiğinde ilgili bildirimi duraklatır, ayrıldığında
    /// devam ettirir. Dönen değer bir bildirimle etkileşim olduğunu belirtir.
    pub fn hover_toasts(&mut self, fb_w: usize, tb_y: usize, mx: usize, my: usize, now_ms: u64) {
        for (i, geom) in self.toast_geoms(fb_w, tb_y).into_iter().enumerate() {
            let Some(t) = self.toasts[i].as_mut() else {
                continue;
            };
            if let Some(g) = geom {
                if in_rect(mx, my, g) {
                    t.pause(now_ms);
                } else {
                    t.resume(now_ms);
                }
            }
        }
    }

    /// Bildirim kartlarının ekran dikdörtgenleri (slot sırasıyla, boş
    /// slotlar için `None`).
    pub fn toast_geoms(
        &self,
        fb_w: usize,
        tb_y: usize,
    ) -> [Option<(usize, usize, usize, usize)>; MAX_TOASTS] {
        let mut out = [None; MAX_TOASTS];
        let toast_margin = sp(10);
        let mut current_bottom = tb_y.saturating_sub(toast_margin);
        for (i, slot) in self.toasts.iter().enumerate() {
            let Some(t) = slot else {
                continue;
            };
            let toast_h = Self::toast_height(t.progress.is_some());
            let toast_w = Self::toast_width(fb_w);
            let toast_x = fb_w.saturating_sub(toast_w + toast_margin);
            let toast_y = current_bottom.saturating_sub(toast_h);
            out[i] = Some((toast_x, toast_y, toast_w, toast_h));
            current_bottom = toast_y.saturating_sub(toast_margin);
        }
        out
    }

    /// Bildirim kartı genişliği (ekran genişliğine göre sınırlanır).
    pub fn toast_width(fb_w: usize) -> usize {
        sp(280).min(fb_w.saturating_sub(sp(24)))
    }

    /// Bildirim kartı yüksekliği; ilerleme çubuğu varsa daha yüksek.
    pub fn toast_height(has_progress: bool) -> usize {
        if has_progress {
            sp(76)
        } else {
            sp(60)
        }
    }

    /// Fare tıklaması bir bildirime veya kapatma (X) butonuna denk geldi mi?
    pub fn toast_click(&mut self, fb_w: usize, tb_y: usize, mx: usize, my: usize) -> bool {
        let geoms = self.toast_geoms(fb_w, tb_y);
        for (i, geom) in geoms.into_iter().enumerate() {
            let Some((toast_x, toast_y, toast_w, _toast_h)) = geom else {
                continue;
            };
            if in_rect(
                mx,
                my,
                (toast_x, toast_y, toast_w, Self::toast_height(true)),
            ) {
                // Tıklanan bildirimi kapat
                self.toasts[i] = None;
                return true;
            }
        }
        false
    }

    /// Bildirimleri görev çubuğunun hemen sağ üstünde şık gölgeli modern kartlar olarak çizer.
    pub fn render_toasts(&self, fb: &mut Framebuffer, tb_y: usize, mx: usize, my: usize) {
        if self.active_toast_count() == 0 {
            return;
        }

        let toast_margin = sp(10);
        let now_ms = crate::monotonic_ms();
        let motion_ok = !crate::gui::anim::reduced_motion();

        for (i, t) in self.toasts.iter().enumerate() {
            let Some(t) = t else {
                continue;
            };
            let geoms = self.toast_geoms(fb.width, tb_y);
            let Some((toast_x, toast_y, toast_w, toast_h)) = geoms[i] else {
                continue;
            };
            let slide_span = (toast_w + toast_margin) as f32;
            // Giriş: sağdan sola ease-out kayma; çıkış: sağa ease-in-out.
            // Azaltılmış harekette kayma atlanır; yaşam döngüsü aynı kalır.
            let slide_px = if !motion_ok {
                0
            } else if t.exit_ms != 0 {
                let p = crate::gui::anim::progress(
                    now_ms.wrapping_sub(t.exit_ms),
                    Self::TOAST_SLIDE_OUT_MS,
                );
                (crate::gui::anim::ease_in_out_quad(p) * slide_span) as usize
            } else {
                let p = crate::gui::anim::progress(
                    now_ms.wrapping_sub(t.created_ms),
                    Self::TOAST_SLIDE_IN_MS,
                );
                ((1.0 - crate::gui::anim::ease_out_quad(p)) * slide_span) as usize
            };
            let toast_x = toast_x + slide_px;

            // 1. Yumuşak Gölgelendirme
            fb.draw_shadow(toast_x, toast_y, toast_w, toast_h, Color::SHADOW);

            // 2. Kart Gövdesi (Modern Yarı Saydam/Koyu Zemin & İnce Çerçeve)
            let is_hover = in_rect(mx, my, (toast_x, toast_y, toast_w, toast_h));
            let bg_color = if is_hover {
                Self::shade(Color::TASKBAR_BG, 12)
            } else {
                Color::TASKBAR_BG
            };
            fb.draw_rounded_rect(toast_x, toast_y, toast_w, toast_h, 8, bg_color);
            fb.draw_rounded_rect_outline(toast_x, toast_y, toast_w, toast_h, 8, Color::BORDER);

            // 3. Durum Rengi Gösterge Çizgisi (Sol kenarda şık dikey bant)
            let accent = t.kind.accent();
            fb.draw_rounded_rect(
                toast_x + 2,
                toast_y + 4,
                4,
                toast_h.saturating_sub(8),
                2,
                accent,
            );

            // 4. Tür İkonu (16x16)
            let icon_x = toast_x + sp(12);
            let icon_y = toast_y + sp(10);
            Self::draw_toast_icon(fb, t.kind.icon(), icon_x, icon_y, accent);

            // 5. Başlık ve Mesaj Metinleri
            let text_x = icon_x + 22;
            let text_max_w = toast_w.saturating_sub(sp(54));

            Self::draw_string_clipped(
                fb,
                text_x,
                toast_y + sp(10),
                t.title_str(),
                Color::TEXT_MAIN,
                (text_x, toast_y + 6, text_max_w, 18),
            );
            Self::draw_string_clipped(
                fb,
                text_x,
                toast_y + sp(30),
                t.message_str(),
                Color::TEXT_MUTED,
                (text_x, toast_y + 28, text_max_w, 20),
            );

            // 6. Gerçek ilerleme çubuğu (varsa) + yüzde
            if let Some(p) = t.progress {
                let bar = (
                    text_x,
                    toast_y + sp(50),
                    text_max_w.saturating_sub(sp(44)),
                    sp(6),
                );
                Self::render_toast_progress_bar(fb, bar, p.percent(), accent);
                let mut pct = TextBuf::new();
                pct.push_usize(p.percent() as usize);
                pct.push_str("%");
                Self::draw_string(
                    fb,
                    bar.0 + bar.2 + sp(8),
                    toast_y + sp(46),
                    pct.as_str(),
                    Color::TEXT_MUTED,
                );
            }

            // 7. Sağ üst kapatma "✕" ikonu
            let close_x = toast_x + toast_w.saturating_sub(sp(18));
            let close_y = toast_y + sp(8);
            let close_hover = in_rect(
                mx,
                my,
                (close_x.saturating_sub(2), close_y.saturating_sub(2), 14, 14),
            );
            let close_col = if close_hover {
                Color::ACCENT_RED
            } else {
                Color::TEXT_MUTED
            };
            fb.draw_rect(close_x, close_y, 2, 2, close_col);
            fb.draw_rect(close_x + 6, close_y + 6, 2, 2, close_col);
            fb.draw_rect(close_x + 6, close_y, 2, 2, close_col);
            fb.draw_rect(close_x, close_y + 6, 2, 2, close_col);
            fb.draw_rect(close_x + 3, close_y + 3, 2, 2, close_col);
        }
    }

    /// Bildirim türü ikonunu çizer (16x16).
    pub fn draw_toast_icon(
        fb: &mut Framebuffer,
        icon: ToastIcon,
        icon_x: usize,
        icon_y: usize,
        accent: u32,
    ) {
        match icon {
            ToastIcon::Info => {
                // Bilgi "i" dairesi
                fb.draw_rounded_rect_outline(icon_x, icon_y, 16, 16, 8, accent);
                fb.draw_rect(icon_x + 7, icon_y + 4, 2, 2, accent);
                fb.draw_rect(icon_x + 7, icon_y + 8, 2, 5, accent);
            }
            ToastIcon::Success => {
                // Başarı "✓" onay tiki
                fb.draw_rounded_rect_outline(icon_x, icon_y, 16, 16, 8, accent);
                fb.draw_rect(icon_x + 4, icon_y + 8, 2, 3, accent);
                fb.draw_rect(icon_x + 6, icon_y + 10, 2, 3, accent);
                fb.draw_rect(icon_x + 8, icon_y + 7, 2, 3, accent);
                fb.draw_rect(icon_x + 10, icon_y + 5, 2, 3, accent);
            }
            ToastIcon::Warning => {
                // Uyarı "!" üçgeni
                fb.draw_rect_outline(icon_x + 1, icon_y + 1, 14, 14, accent);
                fb.draw_rect(icon_x + 7, icon_y + 4, 2, 5, accent);
                fb.draw_rect(icon_x + 7, icon_y + 11, 2, 2, accent);
            }
            ToastIcon::Error => {
                // Hata "✕" çarpı
                fb.draw_rounded_rect_outline(icon_x, icon_y, 16, 16, 8, accent);
                fb.draw_rect(icon_x + 4, icon_y + 4, 3, 3, accent);
                fb.draw_rect(icon_x + 9, icon_y + 9, 3, 3, accent);
                fb.draw_rect(icon_x + 9, icon_y + 4, 3, 3, accent);
                fb.draw_rect(icon_x + 4, icon_y + 9, 3, 3, accent);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::string::ToString;

    #[test]
    fn kind_defaults_are_distinct_and_ordered() {
        assert_eq!(ToastKind::Info.default_duration_ms(), TOAST_INFO_MS);
        assert_eq!(ToastKind::Success.default_duration_ms(), TOAST_SUCCESS_MS);
        assert_eq!(ToastKind::Warning.default_duration_ms(), TOAST_WARNING_MS);
        assert_eq!(ToastKind::Error.default_duration_ms(), TOAST_ERROR_MS);
        assert!(ToastKind::Error.default_duration_ms() > ToastKind::Info.default_duration_ms());
        assert_eq!(
            ToastKind::Warning.announce_kind(),
            a11y::AnnounceKind::Alert
        );
        assert_eq!(
            ToastKind::Success.announce_kind(),
            a11y::AnnounceKind::Status
        );
    }

    #[test]
    fn each_kind_has_its_own_icon_and_accent() {
        let kinds = [
            ToastKind::Info,
            ToastKind::Success,
            ToastKind::Warning,
            ToastKind::Error,
        ];
        let mut icons = [ToastIcon::Info; 4];
        for (i, k) in kinds.iter().enumerate() {
            icons[i] = k.icon();
        }
        // Tüm ikonlar birbirinden farklı olmalı.
        for i in 0..4 {
            for j in (i + 1)..4 {
                assert_ne!(icons[i], icons[j], "kind {i} ve {j} aynı ikonu paylaşıyor");
                assert_ne!(
                    kinds[i].accent(),
                    kinds[j].accent(),
                    "kind {i} ve {j} aynı rengi paylaşıyor"
                );
            }
        }
    }

    #[test]
    fn progress_only_moves_forward() {
        let mut p = ToastProgress::new(10);
        assert!(p.set(40));
        assert_eq!(p.percent(), 40);
        assert!(!p.set(20));
        assert_eq!(p.percent(), 40);
        assert!(!p.is_complete());
        assert!(p.set(100));
        assert!(p.is_complete());
        assert!(!p.set(100));
        assert_eq!(p.as_usize(), 100);
        // 100'ün üzeri sıkıştırılır.
        assert_eq!(ToastProgress::new(200).percent(), 100);
    }

    #[test]
    fn toast_auto_expires_after_its_duration() {
        let mut t = ToastNotification::empty();
        t.kind = ToastKind::Info;
        t.duration_ms = 3000;
        t.created_ms = 1000;
        assert!(!t.is_expired(3999));
        assert!(t.is_expired(4000));
        // İlk adım çıkış animasyonunu başlatır, hemen silmez.
        assert!(!t.advance(4000, 140));
        assert_ne!(t.exit_ms, 0);
        assert!(!t.advance(4139, 140));
        assert!(t.advance(4140, 140));
    }

    #[test]
    fn progress_toast_waits_for_completion() {
        let mut t = ToastNotification::empty();
        t.kind = ToastKind::Info;
        t.duration_ms = 1000;
        t.created_ms = 0;
        t.progress = Some(ToastProgress::new(25));
        // Süre dolsa bile ilerleme tamamlanmadığı için kapanmaz.
        assert!(!t.advance(10_000, 140));
        assert_eq!(t.exit_ms, 0);
        t.progress.as_mut().unwrap().finish();
        assert!(!t.advance(10_000, 140));
        assert_ne!(t.exit_ms, 0);
    }

    #[test]
    fn pause_extends_the_visible_time() {
        let mut t = ToastNotification::empty();
        t.kind = ToastKind::Info;
        t.duration_ms = 3000;
        t.created_ms = 0;
        t.pause(1000);
        // 1000 ms'de duraklatıldı; 5000 ms'de süre hâlâ 1000 ms.
        assert_eq!(t.elapsed_ms(5000), 1000);
        assert!(!t.is_expired(5000));
        t.resume(4000);
        // 3000 ms duraklatıldı → created_ms 3000'e kayar.
        assert_eq!(t.created_ms, 3000);
        assert_eq!(t.elapsed_ms(4000), 1000);
        assert!(!t.is_expired(5999));
        assert!(t.is_expired(6000));
    }

    #[test]
    fn text_is_truncated_to_capacity() {
        let mut t = ToastNotification::empty();
        let long_title = "T".repeat(TOAST_TITLE_MAX * 2);
        let long_msg = "M".repeat(TOAST_MESSAGE_MAX * 2);
        t.set_text(&long_title, &long_msg);
        assert_eq!(t.title_len, TOAST_TITLE_MAX);
        assert_eq!(t.message_len, TOAST_MESSAGE_MAX);
        assert_eq!(t.title_str().len(), TOAST_TITLE_MAX);
        assert_eq!(t.message_str().len(), TOAST_MESSAGE_MAX);
    }
}
