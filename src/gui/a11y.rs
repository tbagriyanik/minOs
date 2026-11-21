//! Ekran okuyucu (screen reader) bildirim katmanı.
//!
//! minOs'ta ARIA/MSAA benzeri bir erişilebilirlik API'si bulunmuyor; bunun
//! yerine odak halkası ve yüksek kontrast teması kısmi bir denklik sağlıyor
//! (bkz. `docs/ACCESSIBILITY.md`). Bu modül, ekranı göremeyen kullanıcılar
//! için **görsel bir bildirim bandı** sağlar: odak değişimi, pencere açılışı
//! ve sistem mesajları ekranda kısa bir süre yazı olarak gösterilir.
//!
//! Modül **saf** mantıktır (çizim bağımlılığı yoktur); böylece kuyruk
//! davranışı host testlerinde doğrudan doğrulanabilir.

/// Aynı anda tutulan bildirim sayısı üst sınırı.
pub const MAX_ANNOUNCEMENTS: usize = 8;

/// Bir bildirim metninin azami uzunluğu (bayt).
pub const MAX_TEXT: usize = 96;

/// Varsayılan görünme süresi (ms).
pub const DEFAULT_TTL_MS: u64 = 4000;

/// Bildirim türü.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AnnounceKind {
    /// Durum bilgisi (odak değişimi, pencere açılışı).
    Status,
    /// Uyarı (hata, uyarı, onay).
    Alert,
}

/// Tek bir ekran okuyucu bildirimi.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Announcement {
    /// UTF-8 metin (bayt).
    pub text: [u8; MAX_TEXT],
    /// Geçerli metin uzunluğu (bayt).
    pub len: usize,
    /// Bildirim türü.
    pub kind: AnnounceKind,
    /// Oluşturulma zamanı (monotonic ms).
    pub created_ms: u64,
    /// Görünme süresi (ms); `created_ms + ttl_ms` sonunda bayatlar.
    pub ttl_ms: u64,
}

impl Announcement {
    /// Boş bir bildirim.
    pub const fn empty() -> Self {
        Self {
            text: [0; MAX_TEXT],
            len: 0,
            kind: AnnounceKind::Status,
            created_ms: 0,
            ttl_ms: 0,
        }
    }

    /// Bildirim metnini `&str` olarak döndürür.
    ///
    /// Geçersiz UTF-8 olursa boş dize döner.
    pub fn text_str(&self) -> &str {
        core::str::from_utf8(&self.text[..self.len]).unwrap_or("")
    }

    /// Bildirimin süresi dolmuş mu?
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.created_ms) >= self.ttl_ms
    }
}

/// Bildirim kuyruğu.
///
/// Yeni bildirimler eskilerin üzerine yazar; `current()` her zaman en son
/// bildirimi döndürür. Böylece hızlı arka arkaya olaylarda hiçbir mesaj
/// tamamen kaybolmaz.
///
/// Depolama sıralı bir kuyruk olarak tutulur (en eski `items[0]`); bu sayede
/// süresi dolan girdiler baştan silindiğinde halkada boşluk kalmaz.
#[derive(Clone)]
pub struct Announcer {
    items: [Option<Announcement>; MAX_ANNOUNCEMENTS],
    /// Saklanan bildirim sayısı (`items[0..len]` geçerlidir).
    len: usize,
}

impl Announcer {
    /// Boş bir kuyruk.
    pub const fn new() -> Self {
        Self {
            items: [None; MAX_ANNOUNCEMENTS],
            len: 0,
        }
    }

    /// Yeni bir bildirim ekler.
    ///
    /// Metin `MAX_TEXT` baytı aşarsa kırpılır. Boş metin eklenmez.
    /// Dönen değer, bildirimin gerçekten eklendiğini belirtir.
    pub fn announce(&mut self, kind: AnnounceKind, text: &str, now_ms: u64, ttl_ms: u64) -> bool {
        if text.is_empty() {
            return false;
        }
        let bytes = text.as_bytes();
        let n = bytes.len().min(MAX_TEXT);
        let mut item = Announcement::empty();
        item.text[..n].copy_from_slice(&bytes[..n]);
        item.len = n;
        item.kind = kind;
        item.created_ms = now_ms;
        item.ttl_ms = if ttl_ms == 0 { DEFAULT_TTL_MS } else { ttl_ms };

        // Kuyruk doluysa en eski girdiyi düşür.
        if self.len == MAX_ANNOUNCEMENTS {
            self.remove_oldest();
        }
        self.items[self.len] = Some(item);
        self.len += 1;
        true
    }

    /// Varsayılan süreyle bildirim ekler.
    pub fn announce_status(&mut self, text: &str, now_ms: u64) -> bool {
        self.announce(AnnounceKind::Status, text, now_ms, DEFAULT_TTL_MS)
    }

    /// Varsayılan süreyle uyarı bildirimi ekler.
    pub fn announce_alert(&mut self, text: &str, now_ms: u64) -> bool {
        self.announce(AnnounceKind::Alert, text, now_ms, DEFAULT_TTL_MS)
    }

    /// En son bildirimi döndürür (yoksa `None`).
    pub fn current(&self) -> Option<&Announcement> {
        if self.len == 0 {
            return None;
        }
        self.items[self.len - 1].as_ref()
    }

    /// Süresi dolmuş bildirimleri kuyruktan temizler.
    ///
    /// Dönen değer, temizlenen bildirim sayısıdır.
    pub fn expire(&mut self, now_ms: u64) -> usize {
        let mut removed = 0usize;
        while self.len > 0 {
            let expired = self.items[0]
                .as_ref()
                .map(|a| a.is_expired(now_ms))
                .unwrap_or(false);
            if !expired {
                break;
            }
            self.remove_oldest();
            removed += 1;
        }
        removed
    }

    /// En eski girdiyi kuyruktan çıkarır ve kalanları kaydırır.
    fn remove_oldest(&mut self) {
        if self.len == 0 {
            return;
        }
        for i in 1..self.len {
            self.items[i - 1] = self.items[i];
        }
        self.items[self.len - 1] = None;
        self.len -= 1;
    }

    /// Kuyruktaki bildirim sayısı.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Kuyruk boş mu?
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Tüm bildirimleri siler.
    pub fn clear(&mut self) {
        for slot in self.items.iter_mut() {
            *slot = None;
        }
        self.len = 0;
    }
}

impl Default for Announcer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_announcer_has_no_current() {
        let a = Announcer::new();
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);
        assert!(a.current().is_none());
    }

    #[test]
    fn announce_keeps_the_latest_message() {
        let mut a = Announcer::new();
        assert!(a.announce_status("Pencere açıldı", 100));
        assert!(a.announce_alert("Hata oluştu", 200));
        assert_eq!(a.len(), 2);
        let cur = a.current().expect("bildirim var");
        assert_eq!(cur.text_str(), "Hata oluştu");
        assert_eq!(cur.kind, AnnounceKind::Alert);
        assert_eq!(cur.created_ms, 200);
    }

    #[test]
    fn empty_text_is_rejected() {
        let mut a = Announcer::new();
        assert!(!a.announce_status("", 0));
        assert!(a.is_empty());
    }

    #[test]
    fn long_text_is_truncated_to_capacity() {
        let mut a = Announcer::new();
        let long = "x".repeat(MAX_TEXT * 2);
        assert!(a.announce_status(&long, 0));
        let cur = a.current().expect("bildirim var");
        assert_eq!(cur.len, MAX_TEXT);
        assert_eq!(cur.text_str().len(), MAX_TEXT);
    }

    #[test]
    fn expired_announcements_are_removed() {
        let mut a = Announcer::new();
        a.announce(AnnounceKind::Status, "bir", 0, 100);
        a.announce(AnnounceKind::Status, "iki", 0, 100);
        assert_eq!(a.len(), 2);
        // 100 ms sonra ikisi de bayatlar.
        assert_eq!(a.expire(100), 2);
        assert!(a.is_empty());
        assert!(a.current().is_none());
    }

    #[test]
    fn ring_buffer_keeps_newest_when_full() {
        let mut a = Announcer::new();
        for i in 0..MAX_ANNOUNCEMENTS {
            a.announce_status(&format!("m{i}"), 0);
        }
        assert_eq!(a.len(), MAX_ANNOUNCEMENTS);
        a.announce_status("son", 0);
        assert_eq!(a.len(), MAX_ANNOUNCEMENTS);
        assert_eq!(a.current().expect("bildirim var").text_str(), "son");
    }

    #[test]
    fn clear_empties_the_queue() {
        let mut a = Announcer::new();
        a.announce_status("merhaba", 0);
        a.clear();
        assert!(a.is_empty());
        assert!(a.current().is_none());
    }
}
