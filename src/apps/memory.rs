//! Uygulama bazında bellek kullanımı ölçümü.
//!
//! Çekirdekte gerçek bir süreç/heap izolasyonu olmadığı için (yerleşik
//! uygulamalar ring 0'da aynı heap'i paylaşır) ölçüm şu iki kaynaktan
//! beslenir:
//!
//! 1. Her uygulamanın kendi `memory_usage_bytes()` metodu — `Vec` kapasiteleri,
//!    görüntü tamponları, geçmiş yığınları gibi dinamik alanlar.
//! 2. `core::mem::size_of_val` ile statik yapı boyutu (dinamik alan
//!    bildirmeyen uygulamalar için güvenli alt sınır).
//!
//! Değerler her karede (veya görev yöneticisi yenileme turunda) yayınlanır;
//! Görev Yöneticisi "Bellek" sütununda ve `[APP-MEM]` seri satırında
//! gösterilir. Modül saf `core` + `alloc` kullanır ve host testlerinde de
//! derlenir.

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// İzlenebilen azami uygulama kimliği (yerleşik uygulamalar 0..=17).
pub const MAX_TRACKED_APPS: usize = 32;

/// Stable display names for the built-in application memory report.
pub fn app_name(app_id: usize) -> &'static str {
    match app_id {
        0 => "Dosya Yöneticisi",
        1 => "Sistem Ayarları",
        2 => "Not Defteri",
        3 => "Web Gezgini",
        4 => "Resim Gösterici",
        5 => "Müzik Çalar",
        6 => "Video Oynatıcı",
        7 => "Görev Yöneticisi",
        8 => "Hesap Makinesi",
        9 => "Çizim Tahtası",
        10 => "Komut İstemi",
        11 => "Kurulum Sihirbazı",
        12 => "3D Gösterici",
        13 => "Paket Yöneticisi",
        14 => "Kontrol Paneli",
        15 => "Oyunlar (FPS)",
        16 => "FPS 3D",
        17 => "Şehir Sürüşü",
        _ => "Bilinmeyen",
    }
}

static APP_BYTES: [AtomicUsize; MAX_TRACKED_APPS] =
    [const { AtomicUsize::new(0) }; MAX_TRACKED_APPS];
static PEAK_BYTES: [AtomicUsize; MAX_TRACKED_APPS] =
    [const { AtomicUsize::new(0) }; MAX_TRACKED_APPS];
static SNAPSHOT_SEQ: AtomicU64 = AtomicU64::new(0);

/// Bir uygulamanın son ölçülen bayt değerini yayınlar.
pub fn publish(app_id: usize, bytes: usize) {
    if app_id >= MAX_TRACKED_APPS {
        return;
    }
    APP_BYTES[app_id].store(bytes, Ordering::Relaxed);
    // Zirve değeri "en az şu kadarı kullandı" bilgisini korur; UI'da anlık
    // değer düşüşleri geçmişi sıfırlamaz.
    let mut peak = PEAK_BYTES[app_id].load(Ordering::Relaxed);
    while bytes > peak {
        match PEAK_BYTES[app_id].compare_exchange_weak(
            peak,
            bytes,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(current) => peak = current,
        }
    }
    SNAPSHOT_SEQ.fetch_add(1, Ordering::Relaxed);
}

/// Son ölçülen değer (yayınlanmadıysa 0).
pub fn sample(app_id: usize) -> usize {
    if app_id >= MAX_TRACKED_APPS {
        return 0;
    }
    APP_BYTES[app_id].load(Ordering::Relaxed)
}

/// Şimdiye kadar görülen en yüksek değer.
pub fn peak(app_id: usize) -> usize {
    if app_id >= MAX_TRACKED_APPS {
        return 0;
    }
    PEAK_BYTES[app_id].load(Ordering::Relaxed)
}

/// Tüm uygulamaların anlık toplamı.
pub fn total() -> usize {
    let mut sum = 0usize;
    for i in 0..MAX_TRACKED_APPS {
        sum = sum.saturating_add(sample(i));
    }
    sum
}

/// Yayın sayacı; seri raporun hangi turlarda güncellendiğini izlemek için.
pub fn snapshot_seq() -> u64 {
    SNAPSHOT_SEQ.load(Ordering::Relaxed)
}

/// Kayıtları sırayla ziyaret eder (0 değerli uygulamalar atlanır).
pub fn for_each<F: FnMut(usize, usize)>(mut visit: F) {
    for i in 0..MAX_TRACKED_APPS {
        let bytes = sample(i);
        if bytes > 0 {
            visit(i, bytes);
        }
    }
}

/// Testler/host yardımcıları için tüm kayıtları sıfırlar.
pub fn clear() {
    for i in 0..MAX_TRACKED_APPS {
        APP_BYTES[i].store(0, Ordering::Relaxed);
        PEAK_BYTES[i].store(0, Ordering::Relaxed);
    }
}

/// Dilim/vector'ın ayırdığı baytı hesaplar (kapasite, uzunluk değil).
#[inline(always)]
pub fn slice_bytes<T>(slice: &[T]) -> usize {
    slice.len().saturating_mul(core::mem::size_of::<T>())
}

/// Vector'ın ayırdığı baytı hesaplar (kapasite, uzunluk değil).
#[inline(always)]
pub fn vec_bytes<T>(vec: &alloc::vec::Vec<T>) -> usize {
    vec.capacity().saturating_mul(core::mem::size_of::<T>())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Host testleri paralel çalıştığından global kayıtları sıfırlamayız;
    // her test kendi ayrılmış yüksek kimliklerini kullanır.
    #[test]
    fn publish_sample_and_peak_track_app_memory() {
        publish(28, 4096);
        publish(29, 8192);
        assert_eq!(sample(28), 4096);
        assert_eq!(sample(29), 8192);
        assert!(total() >= 12288);
        assert_eq!(peak(28), 4096);
        publish(28, 2048);
        assert_eq!(sample(28), 2048);
        assert_eq!(peak(28), 4096, "zirve değer korunmalı");
    }

    #[test]
    fn out_of_range_app_ids_are_ignored() {
        let before = total();
        publish(MAX_TRACKED_APPS, 9999);
        publish(usize::MAX, 9999);
        assert_eq!(sample(MAX_TRACKED_APPS), 0);
        assert_eq!(total(), before);
    }

    #[test]
    fn for_each_visits_only_published_apps() {
        publish(30, 100);
        publish(31, 200);
        let mut seen = alloc::vec::Vec::new();
        for_each(|id, bytes| {
            if id == 30 || id == 31 {
                seen.push((id, bytes));
            }
        });
        assert!(seen.contains(&(30usize, 100usize)));
        assert!(seen.contains(&(31usize, 200usize)));
    }

    #[test]
    fn byte_helpers_use_capacity_and_underlying_type_size() {
        let v = alloc::vec![0u32; 8];
        assert_eq!(vec_bytes(&v), 32);
        assert_eq!(slice_bytes(&v[..2]), 8);
    }
}
