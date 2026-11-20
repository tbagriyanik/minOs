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

/// Son 30 saniyelik bellek ve CPU kullanım geçmişi için örnek sayısı.
pub const HISTORY_CAPACITY: usize = 30;

static MEMORY_HISTORY_MB: [AtomicUsize; HISTORY_CAPACITY] =
    [const { AtomicUsize::new(0) }; HISTORY_CAPACITY];
static CPU_HISTORY_PCT: [AtomicUsize; HISTORY_CAPACITY] =
    [const { AtomicUsize::new(0) }; HISTORY_CAPACITY];
static HISTORY_HEAD: AtomicUsize = AtomicUsize::new(0);
static HISTORY_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Son 30 saniyelik geçmişe yeni bir örnek kaydeder (RAM MB ve CPU %).
pub fn record_history_sample(used_ram_mb: usize, cpu_usage_pct: usize) {
    let count = HISTORY_COUNT.load(Ordering::Relaxed);
    let head = HISTORY_HEAD.load(Ordering::Relaxed);
    let next_slot = (head + count) % HISTORY_CAPACITY;

    MEMORY_HISTORY_MB[next_slot].store(used_ram_mb, Ordering::Relaxed);
    CPU_HISTORY_PCT[next_slot].store(cpu_usage_pct.min(100), Ordering::Relaxed);

    if count < HISTORY_CAPACITY {
        HISTORY_COUNT.store(count + 1, Ordering::Relaxed);
    } else {
        HISTORY_HEAD.store((head + 1) % HISTORY_CAPACITY, Ordering::Relaxed);
    }
}

/// Son 30 saniyelik geçmişteki örnek sayısını döndürür.
pub fn history_len() -> usize {
    HISTORY_COUNT.load(Ordering::Relaxed)
}

/// Kaydedilen geçmişi en eskiden en yeniye sırayla (RAM MB, CPU %) döner.
pub fn get_history(
    out_mem_mb: &mut [usize; HISTORY_CAPACITY],
    out_cpu_pct: &mut [usize; HISTORY_CAPACITY],
) -> usize {
    let count = HISTORY_COUNT.load(Ordering::Relaxed);
    let head = HISTORY_HEAD.load(Ordering::Relaxed);
    for i in 0..count {
        let idx = (head + i) % HISTORY_CAPACITY;
        out_mem_mb[i] = MEMORY_HISTORY_MB[idx].load(Ordering::Relaxed);
        out_cpu_pct[i] = CPU_HISTORY_PCT[idx].load(Ordering::Relaxed);
    }
    count
}

/// Son 30 saniyelik kullanım için metin tabanlı (ASCII bar / sparkline) grafik üretir.
/// Her örnek için min..max aralığına göre ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█' veya ASCII `_.-~*#` üretir.
/// Saf ASCII uyumluluğu için `_`, `.`, `-`, `=`, `o`, `O`, `#` seviyeleri kullanılır.
pub fn render_sparkline_ascii(values: &[usize], max_val: usize, out: &mut [u8]) -> usize {
    const BARS: &[u8; 8] = b"_.-=+*%#";
    let n = values.len().min(out.len());
    let max = max_val.max(1);
    for i in 0..n {
        let v = values[i];
        let idx = (v.saturating_mul(7) / max).min(7);
        out[i] = BARS[idx];
    }
    n
}

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
    for i in 0..HISTORY_CAPACITY {
        MEMORY_HISTORY_MB[i].store(0, Ordering::Relaxed);
        CPU_HISTORY_PCT[i].store(0, Ordering::Relaxed);
    }
    HISTORY_HEAD.store(0, Ordering::Relaxed);
    HISTORY_COUNT.store(0, Ordering::Relaxed);
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

    // Host testleri paralel çalıştığından global kayıtları korumak için
    // testler bu kilidi paylaşır.
    static TEST_LOCK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

    struct TestGuard;

    impl Drop for TestGuard {
        fn drop(&mut self) {
            TEST_LOCK.store(false, core::sync::atomic::Ordering::Release);
        }
    }

    fn test_guard() -> TestGuard {
        while TEST_LOCK
            .compare_exchange(
                false,
                true,
                core::sync::atomic::Ordering::Acquire,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            core::hint::spin_loop();
        }
        TestGuard
    }

    // Host testleri paralel çalıştığından global kayıtları sıfırlamayız;
    // her test kendi ayrılmış yüksek kimliklerini kullanır.
    #[test]
    fn publish_sample_and_peak_track_app_memory() {
        let _guard = test_guard();
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
        let _guard = test_guard();
        let before = total();
        publish(MAX_TRACKED_APPS, 9999);
        publish(usize::MAX, 9999);
        assert_eq!(sample(MAX_TRACKED_APPS), 0);
        assert_eq!(total(), before);
    }

    #[test]
    fn for_each_visits_only_published_apps() {
        let _guard = test_guard();
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
    fn clear_resets_all_counters_and_history() {
        let _guard = test_guard();
        publish(31, 5000);
        record_history_sample(100, 50);
        assert_eq!(sample(31), 5000);
        assert_eq!(history_len(), 1);
        assert!(total() > 0);

        clear();

        assert_eq!(sample(31), 0);
        assert_eq!(peak(31), 0, "clear should reset peak too");
        assert_eq!(total(), 0);
        assert_eq!(history_len(), 0, "clear should reset history");
    }

    #[test]
    fn history_ring_buffer_overflow_overwrites_oldest() {
        let _guard = test_guard();
        clear();
        // Fill beyond capacity to trigger wraparound.
        for i in 1..=35 {
            record_history_sample(i * 10, i * 2);
        }
        assert_eq!(history_len(), 30);
        let mut mem = [0usize; HISTORY_CAPACITY];
        let mut cpu = [0usize; HISTORY_CAPACITY];
        let count = get_history(&mut mem, &mut cpu);
        assert_eq!(count, 30);
        // Oldest (i=1..=5) should have been overwritten; first returned is i=6.
        assert_eq!(mem[0], 60);
        assert_eq!(mem[29], 350);
        assert_eq!(cpu[0], 12);
        assert_eq!(cpu[29], 70);
    }

    #[test]
    fn history_ring_buffer_and_sparkline() {
        let _guard = test_guard();
        clear();
        for i in 1..=35 {
            record_history_sample(i * 10, i * 2);
        }
        assert_eq!(history_len(), 30);
        let mut mem = [0usize; HISTORY_CAPACITY];
        let mut cpu = [0usize; HISTORY_CAPACITY];
        let count = get_history(&mut mem, &mut cpu);
        assert_eq!(count, 30);
        // Son eklenen 35. elemandır (350 MB, %70), ilk eleman 6. elemandır (60 MB, %12)
        assert_eq!(mem[0], 60);
        assert_eq!(mem[29], 350);
        assert_eq!(cpu[0], 12);
        assert_eq!(cpu[29], 70);

        let mut sparkline = [0u8; 30];
        let n = render_sparkline_ascii(&cpu, 100, &mut sparkline);
        assert_eq!(n, 30);
        // İlk değer %12 (düşük bar '_') son değer %70 (bar '+')
        assert_eq!(sparkline[0], b'_');
        assert_eq!(sparkline[29], b'+');
    }

    #[test]
    fn byte_helpers_use_capacity_and_underlying_type_size() {
        let v = alloc::vec![0u32; 8];
        assert_eq!(vec_bytes(&v), 32);
        assert_eq!(slice_bytes(&v[..2]), 8);
    }
}
