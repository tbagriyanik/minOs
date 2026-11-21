//! Kontrol Paneli (Control Panel)
//!
//! Sistem, ağ, depolama ve sistem izleme bilgilerini canlı çekirdek API'lerinden
//! okuyup gösteren salt okunur panel; alt kısımdaki düğme ile tema değiştirilebilir.
//! Eski sürümdeki sabit/sahte değerler (128 MB RAM, sabit 1280x720, çelişkili
//! ağ durumu) yerine gerçek veriler kullanılır.

use crate::drivers::keyboard::KeyboardLayout;
use crate::drivers::rtc::DateTime;
use crate::drivers::status::DriverId;
use crate::drivers::video::Framebuffer;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// Sekme sayısı: Sistem, Ağ, Hakkında, Testler, İzleme, Sürücüler.
pub const TAB_COUNT: usize = 6;

/// İzleme (kullanım) ekranının ölçüm/karar katmanı.
use crate::apps::monitor;

/// Sistem izleme ekranının kendi render süresi (µs). Kullanım ekranının
/// kendisini yavaşlattığını ölçmek için kullanılır.
static MONITOR_RENDER_US: AtomicU64 = AtomicU64::new(0);

/// İzleme ekranının son ölçülen render süresini döndürür (µs).
pub fn monitor_render_us() -> u64 {
    MONITOR_RENDER_US.load(Ordering::Relaxed)
}

static ACTIVE_TAB: AtomicUsize = AtomicUsize::new(0);
static REFRESHED_AT_MS: AtomicU64 = AtomicU64::new(0);
/// Klavye (Tab / Shift+Tab) ile gezilen odaklı denetim:
/// 0..TAB_COUNT sekmeler, TAB_COUNT "Ayarlar" düğmesi,
/// TAB_COUNT+1 "Ağ Testi" (yalnız Ağ sekmesi), "Performans Testi" (yalnız Testler
/// sekmesi) veya "Yeniden Tara" (yalnız Sürücüler sekmesi).
static FOCUSED_CONTROL: AtomicUsize = AtomicUsize::new(0);
/// "Ayarlar" düğmesinin Enter karşılığı.
pub const ACTION_SETTINGS: usize = TAB_COUNT;
/// Etkin sekmeye özgü ikincil eylem (Ağ/Sürücüler "A/Y", Testler "P").
pub const ACTION_EXTRA: usize = TAB_COUNT + 1;
/// 0: bos, 1: yanit bekleniyor, 2: basarili, 3: basarisiz.
static NET_TEST_STATE: AtomicUsize = AtomicUsize::new(0);
static NET_TEST_START_MS: AtomicU64 = AtomicU64::new(0);

// --- Performans testi (Testler sekmesi) ---

use crate::apps::perfbench::{self, PerfRates};

// Sistem izleme için çekirdek tanılama ve bellek profili erişimi.
use crate::apps::memory;
use crate::kernel::diag;
use crate::kernel::heap::profile as heap_profile;

/// Sürücü başlatma + İzleme ekranının toplam maliyet bütçesi (µs).
/// Kare bütçesi 16 ms; bu ekranın payı 4 ms tutulur.
pub const DRIVER_INIT_BUDGET_US: u64 = 4_000;

/// `diag::DriverInitTiming` → `monitor::DriverTiming` dönüşümü.
fn timing_row(t: &diag::DriverInitTiming) -> monitor::DriverTiming {
    monitor::DriverTiming {
        name: t.name,
        us: t.duration_us,
        ok: t.ok,
    }
}

/// Süreyi "1.2 ms" / "850 µs" biçiminde yazar.
fn push_ms(buf: &mut TextBuf, us: u64) {
    if us >= 1_000 {
        let (ms, frac) = monitor::split_ms(us);
        buf.push_usize(ms as usize);
        if frac > 0 {
            buf.push_str(".");
            buf.push_usize(frac as usize);
        }
        buf.push_str(" ms");
    } else {
        buf.push_usize(us as usize);
        buf.push_str(" µs");
    }
}

/// Performans testinin aşamaları.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PerfPhase {
    Idle,
    /// Donanım mikro-benchmark'ları çalışıyor (CPU / bellek / grafik).
    Benchmark,
    /// Kare teslim hızı ve kararlılığı ölçülüyor.
    Collecting,
    Finished,
}

/// Performans testi sonuçları.
#[derive(Clone, Copy, Default)]
pub struct PerfResults {
    /// Gözlenen kare teslim hızı.
    pub fps: u32,
    /// Grafik dolgu hızından türetilen render kapasitesi.
    pub capacity_fps: u32,
    /// Kare süresi standart sapması (µs).
    pub jitter_us: u32,
    /// CPU hızı (Mops/s).
    pub cpu_mops: u32,
    /// Bellek bant genişliği (MB/s).
    pub mem_mbps: u32,
    /// Grafik dolgu hızı (Mpix/s).
    pub gfx_mpix: u32,
    /// 0..100 arası toplam puan.
    pub score: u32,
}

static PERF_PHASE: AtomicUsize = AtomicUsize::new(0);
static PERF_SAMPLES: AtomicU32 = AtomicU32::new(0);
static PERF_SUM_US: AtomicU64 = AtomicU64::new(0);
static PERF_SUM_SQ_US: AtomicU64 = AtomicU64::new(0);
static PERF_SKIP_FIRST: AtomicU32 = AtomicU32::new(0);
static PERF_TARGET_US: AtomicU64 = AtomicU64::new(0);
static PERF_CPU_MOPS: AtomicU32 = AtomicU32::new(0);
static PERF_MEM_MBPS: AtomicU32 = AtomicU32::new(0);
static PERF_GFX_MPIX: AtomicU32 = AtomicU32::new(0);
static PERF_FPS: AtomicU32 = AtomicU32::new(0);
static PERF_CAP_FPS: AtomicU32 = AtomicU32::new(0);
static PERF_JITTER_US: AtomicU32 = AtomicU32::new(0);
static PERF_SCORE: AtomicU32 = AtomicU32::new(0);
static PERF_PARTS: AtomicU32 = AtomicU32::new(0);
/// İş yükü sağlık toplamı: derleyicinin ölçümü optimize edip silmesini engeller.
static PERF_CHECKSUM: AtomicU64 = AtomicU64::new(0);
/// Ham ölçüm süreleri (µs) — teşhis ve kalibrasyon için.
static PERF_CPU_US: AtomicU64 = AtomicU64::new(0);
static PERF_MEM_US: AtomicU64 = AtomicU64::new(0);
static PERF_GFX_US: AtomicU64 = AtomicU64::new(0);

/// Kare teslim ölçümünün süresi (3 saniye).
const PERF_DURATION_US: u64 = 3_000_000;

/// Depolama sürücüsü satırı: algılama, ad ve (varsa) kapasite sorguları.
/// Sürücüler tablosunda her satır için kullanılır.
#[derive(Clone, Copy)]
struct StorageDriver {
    id: DriverId,
    detected: fn() -> bool,
    name_tr: fn() -> &'static str,
    capacity_bytes: fn() -> u64,
}

pub fn perf_phase() -> PerfPhase {
    match PERF_PHASE.load(Ordering::Relaxed) {
        1 => PerfPhase::Benchmark,
        2 => PerfPhase::Collecting,
        3 => PerfPhase::Finished,
        _ => PerfPhase::Idle,
    }
}

/// Testi başlatır: önce donanım mikro-benchmark'ları, ardından 3 saniyelik
/// kare teslim ölçümü yapılır.
pub fn start_perf_test() {
    PERF_PHASE.store(1, Ordering::Relaxed);
    PERF_SAMPLES.store(0, Ordering::Relaxed);
    PERF_SUM_US.store(0, Ordering::Relaxed);
    PERF_SUM_SQ_US.store(0, Ordering::Relaxed);
    PERF_SKIP_FIRST.store(0, Ordering::Relaxed);
    PERF_TARGET_US.store(0, Ordering::Relaxed);
    PERF_CPU_MOPS.store(0, Ordering::Relaxed);
    PERF_MEM_MBPS.store(0, Ordering::Relaxed);
    PERF_GFX_MPIX.store(0, Ordering::Relaxed);
    PERF_FPS.store(0, Ordering::Relaxed);
    PERF_CAP_FPS.store(0, Ordering::Relaxed);
    PERF_JITTER_US.store(0, Ordering::Relaxed);
    PERF_SCORE.store(0, Ordering::Relaxed);
    PERF_PARTS.store(0, Ordering::Relaxed);
    PERF_CHECKSUM.store(0, Ordering::Relaxed);
}

/// Geçen süreyi mikrosaniye olarak döndürür (TSC tabanlı).
fn now_us() -> u64 {
    let cycles = crate::boot::time::read_cpu_cycles();
    let per_us = unsafe { crate::boot::time::TSC_CYCLES_PER_US }.max(1);
    cycles / per_us
}

/// Bir kare süresini örnekler. Örnek alındıysa (ilerleme veya sonuç
/// güncellendiyse) `true` döner; arayüzün yeniden çizilmesi için kullanılır.
///
/// `target_us` olay döngüsünün o anki kare hedef aralığıdır (16 ms veya
/// 33 ms); yalnızca bilgilendirme amaçlıdır.
pub fn perf_sample(frame_us: u64, target_us: u64) -> bool {
    match perf_phase() {
        PerfPhase::Idle | PerfPhase::Finished => return false,
        PerfPhase::Benchmark => {
            run_benchmarks();
            PERF_PHASE.store(2, Ordering::Relaxed);
            PERF_SAMPLES.store(0, Ordering::Relaxed);
            PERF_SUM_US.store(0, Ordering::Relaxed);
            PERF_SUM_SQ_US.store(0, Ordering::Relaxed);
            // Benchmark bloğu bir kare süresine eklendiği için ilk aralık
            // atlanır; aksi halde ortalama kare süresi yapay olarak şişer.
            PERF_SKIP_FIRST.store(1, Ordering::Relaxed);
            PERF_TARGET_US.store(target_us, Ordering::Relaxed);
            return true;
        }
        PerfPhase::Collecting => {}
    }

    if PERF_SKIP_FIRST.load(Ordering::Relaxed) == 1 {
        PERF_SKIP_FIRST.store(0, Ordering::Relaxed);
        return true;
    }

    PERF_SAMPLES.fetch_add(1, Ordering::Relaxed);
    PERF_SUM_US.fetch_add(frame_us, Ordering::Relaxed);
    PERF_SUM_SQ_US.fetch_add(frame_us.saturating_mul(frame_us), Ordering::Relaxed);

    if PERF_SUM_US.load(Ordering::Relaxed) >= PERF_DURATION_US {
        finish_perf_test();
    }
    true
}

/// Donanım mikro-benchmark'ları: CPU, bellek ve grafik hızı.
///
/// Her iş yükü, zamanlayıcı granülaritesi sonucu bozmasın diye en az
/// `perfbench::MIN_BENCH_US` mikrosaniye sürecek şekilde tekrarlanır.
fn run_benchmarks() {
    let mut mem_buf: alloc::vec::Vec<u32> = alloc::vec![0u32; perfbench::MEM_BYTES / 4];
    let mut gfx_buf: alloc::vec::Vec<u32> = alloc::vec![0u32; perfbench::GFX_PIXELS];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;

    // CPU
    let t0 = now_us();
    let mut cpu_iters = 0u64;
    let mut cpu_sum = 0u64;
    loop {
        cpu_sum ^= perfbench::cpu_workload(&mut state, perfbench::CPU_CHUNK);
        cpu_iters = cpu_iters.saturating_add(perfbench::CPU_CHUNK);
        if now_us().saturating_sub(t0) >= perfbench::MIN_BENCH_US {
            break;
        }
        if cpu_iters >= 50_000_000 {
            break;
        }
    }
    let cpu_us = now_us().saturating_sub(t0).max(1);

    // Bellek
    let t1 = now_us();
    let mut mem_passes = 0u32;
    let mut mem_sum = 0u64;
    loop {
        mem_sum ^= perfbench::mem_workload(&mut mem_buf, perfbench::MEM_PASSES);
        mem_passes = mem_passes.saturating_add(perfbench::MEM_PASSES);
        if now_us().saturating_sub(t1) >= perfbench::MIN_BENCH_US {
            break;
        }
        if mem_passes >= 64 {
            break;
        }
    }
    let mem_us = now_us().saturating_sub(t1).max(1);

    // Grafik
    let t2 = now_us();
    let mut gfx_passes = 0u32;
    let mut gfx_sum = 0u64;
    loop {
        gfx_sum ^= perfbench::gfx_workload(&mut gfx_buf, perfbench::GFX_PASSES);
        gfx_passes = gfx_passes.saturating_add(perfbench::GFX_PASSES);
        if now_us().saturating_sub(t2) >= perfbench::MIN_BENCH_US {
            break;
        }
        if gfx_passes >= 64 {
            break;
        }
    }
    let gfx_us = now_us().saturating_sub(t2).max(1);

    // Sonuçlar kullanılmadığında iş yükü optimize edilip silinebilir;
    // sağlık toplamı bir atomik değişkene yazılarak bu engellenir.
    PERF_CHECKSUM.store(cpu_sum ^ mem_sum ^ gfx_sum, Ordering::Relaxed);

    PERF_CPU_US.store(cpu_us, Ordering::Relaxed);
    PERF_MEM_US.store(mem_us, Ordering::Relaxed);
    PERF_GFX_US.store(gfx_us, Ordering::Relaxed);

    PERF_CPU_MOPS.store(
        perfbench::PerfRates::cpu_mops(cpu_iters, cpu_us),
        Ordering::Relaxed,
    );
    PERF_MEM_MBPS.store(
        perfbench::PerfRates::mem_mbps(
            (perfbench::MEM_BYTES as u64) * u64::from(mem_passes),
            mem_us,
        ),
        Ordering::Relaxed,
    );
    PERF_GFX_MPIX.store(
        perfbench::PerfRates::gfx_mpix(
            (perfbench::GFX_PIXELS as u64) * u64::from(gfx_passes),
            gfx_us,
        ),
        Ordering::Relaxed,
    );
}

/// Ölçümü tamamlar ve sonuçları saklar.
fn finish_perf_test() {
    let samples = u64::from(PERF_SAMPLES.load(Ordering::Relaxed).max(1));
    let sum = PERF_SUM_US.load(Ordering::Relaxed);
    let sum_sq = PERF_SUM_SQ_US.load(Ordering::Relaxed);

    let avg_us = (sum / samples).max(1);
    let fps = (1_000_000 / avg_us).min(9_999) as u32;

    // Standart sapma: sqrt(E[x²] - E[x]²). Paydamayı u128'e taşıyarak
    // taşma olmadan hesaplanır.
    let var_num = (sum_sq as u128) * (samples as u128) - (sum as u128) * (sum as u128);
    let variance = (var_num / (samples as u128 * samples as u128)) as u64;
    let jitter_us = perfbench::isqrt(variance) as u32;

    let rates = PerfRates {
        cpu_mops: PERF_CPU_MOPS.load(Ordering::Relaxed),
        mem_mbps: PERF_MEM_MBPS.load(Ordering::Relaxed),
        gfx_mpix: PERF_GFX_MPIX.load(Ordering::Relaxed),
    };
    let scores = perfbench::score(rates, fps, jitter_us);

    PERF_FPS.store(fps, Ordering::Relaxed);
    PERF_CAP_FPS.store(rates.capacity_fps(), Ordering::Relaxed);
    PERF_JITTER_US.store(jitter_us, Ordering::Relaxed);
    PERF_SCORE.store(scores.total, Ordering::Relaxed);
    PERF_PARTS.store(
        (scores.cpu << 24) | (scores.mem << 16) | (scores.gfx << 8) | scores.pacing,
        Ordering::Relaxed,
    );
    PERF_PHASE.store(3, Ordering::Relaxed);

    print_perf_serial(rates, fps, jitter_us, scores.total);
}

/// Sonuçları seri konsola yazar (test/otomasyon ve hata ayıklama için).
fn print_perf_serial(rates: PerfRates, fps: u32, jitter_us: u32, score: u32) {
    crate::boot::console::serial_print("[PERF] fps=");
    crate::boot::console::serial_print_dec(u64::from(fps));
    crate::boot::console::serial_print(" cap_fps=");
    crate::boot::console::serial_print_dec(u64::from(rates.capacity_fps()));
    crate::boot::console::serial_print(" cpu_mops=");
    crate::boot::console::serial_print_dec(u64::from(rates.cpu_mops));
    crate::boot::console::serial_print(" mem_mbps=");
    crate::boot::console::serial_print_dec(u64::from(rates.mem_mbps));
    crate::boot::console::serial_print(" gfx_mpix=");
    crate::boot::console::serial_print_dec(u64::from(rates.gfx_mpix));
    crate::boot::console::serial_print(" jitter_us=");
    crate::boot::console::serial_print_dec(u64::from(jitter_us));
    crate::boot::console::serial_print(" cpu_us=");
    crate::boot::console::serial_print_dec(PERF_CPU_US.load(Ordering::Relaxed));
    crate::boot::console::serial_print(" mem_us=");
    crate::boot::console::serial_print_dec(PERF_MEM_US.load(Ordering::Relaxed));
    crate::boot::console::serial_print(" gfx_us=");
    crate::boot::console::serial_print_dec(PERF_GFX_US.load(Ordering::Relaxed));
    crate::boot::console::serial_print(" score=");
    crate::boot::console::serial_print_dec(u64::from(score));
    crate::boot::console::serial_print("\n");
}

/// Son test sonuçlarını döndürür.
pub fn perf_results() -> PerfResults {
    PerfResults {
        fps: PERF_FPS.load(Ordering::Relaxed),
        capacity_fps: PERF_CAP_FPS.load(Ordering::Relaxed),
        jitter_us: PERF_JITTER_US.load(Ordering::Relaxed),
        cpu_mops: PERF_CPU_MOPS.load(Ordering::Relaxed),
        mem_mbps: PERF_MEM_MBPS.load(Ordering::Relaxed),
        gfx_mpix: PERF_GFX_MPIX.load(Ordering::Relaxed),
        score: PERF_SCORE.load(Ordering::Relaxed),
    }
}

/// Puan derecesi etiketi.
fn perf_grade(score: u32) -> &'static str {
    match perfbench::grade(score) {
        perfbench::Grade::Excellent => crate::gui::ui_text("Mükemmel", "Excellent"),
        perfbench::Grade::VeryGood => crate::gui::ui_text("Çok iyi", "Very good"),
        perfbench::Grade::Good => crate::gui::ui_text("İyi", "Good"),
        perfbench::Grade::Fair => crate::gui::ui_text("Orta", "Fair"),
        perfbench::Grade::Poor => crate::gui::ui_text("Zayıf", "Poor"),
    }
}

/// Performans testi düğmesi dikdörtgeni.
pub fn perf_btn_rect(
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
) -> (usize, usize, usize, usize) {
    let bw = sp(210).min(win_w.saturating_sub(24));
    (win_x + 12, win_y + win_h.saturating_sub(56), bw, 30)
}

/// Performans testi düğmesine tıklama.
pub fn click_perf_btn(
    px: usize,
    py: usize,
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
) -> bool {
    if crate::gui::in_rect(px, py, perf_btn_rect(win_x, win_y, win_w, win_h)) {
        if perf_phase() != PerfPhase::Collecting {
            start_perf_test();
        }
        return true;
    }
    false
}

/// Etkin sekme indeksi (0..TAB_COUNT).
pub fn active_tab() -> usize {
    ACTIVE_TAB.load(Ordering::Relaxed).min(TAB_COUNT - 1)
}

pub fn content_extent() -> (usize, usize) {
    let rows = match active_tab() {
        0 => 16,
        1 => 18,
        2 => 12,
        3 => 22,
        4 => 45,
        _ => crate::drivers::status::DriverId::COUNT + 30,
    };
    let step = 24 + zoom_extra();
    let width = sp(1200);
    let full_height = 62 + zoom_extra() + 12 + sp(72) + rows * step + sp(48);
    let height = full_height.saturating_sub(crate::gui::scaled_titlebar_h());
    (width, height)
}

/// Sekmeyi seçer.
pub fn select_tab(tab: usize) {
    if tab < TAB_COUNT {
        ACTIVE_TAB.store(tab, Ordering::Relaxed);
        normalize_focus();
    }
}

/// Etkin sekmede ikincil eylem düğmesi (Ağ Testi / Performans / Yeniden Tara)
/// var mı?
fn has_extra_action() -> bool {
    matches!(active_tab(), 1 | 3 | 5)
}

/// Odağın geçerli sekmede görünen bir denetimi göstermesini sağlar.
fn normalize_focus() {
    if FOCUSED_CONTROL.load(Ordering::Relaxed) > TAB_COUNT && !has_extra_action() {
        FOCUSED_CONTROL.store(TAB_COUNT, Ordering::Relaxed);
    }
}

/// Odaklı denetim sayısı; yalnızca etkin sekmede görünen düğmeler sayılır.
///
/// Sekmeler 0..TAB_COUNT, ardından "Ayarlar" düğmesi (TAB_COUNT) ve en
/// sonda etkin sekmeye özgü tek düğme (TAB_COUNT+1).
fn focusable_count() -> usize {
    TAB_COUNT + 1 + usize::from(has_extra_action())
}

/// Odaklı denetim indeksini döndürür.
pub fn focused_control() -> usize {
    let cur = FOCUSED_CONTROL.load(Ordering::Relaxed);
    if cur > TAB_COUNT && !has_extra_action() {
        return TAB_COUNT;
    }
    cur.min(focusable_count().saturating_sub(1))
}

/// Odak bir eylem düğmesinde mi (sekmede değil)?
pub fn focus_on_action() -> bool {
    focused_control() >= TAB_COUNT
}

/// Odağı verilen sekmeye taşır (Tab çubuğu tıklaması).
pub fn focus_tab(tab: usize) {
    if tab < TAB_COUNT {
        FOCUSED_CONTROL.store(tab, Ordering::Relaxed);
    }
}

/// Tab / Shift+Tab ile odaklı denetime geçer.
pub fn focus_next_control(reverse: bool) {
    let count = focusable_count();
    if count == 0 {
        return;
    }
    let cur = focused_control();
    FOCUSED_CONTROL.store(
        if reverse {
            (cur + count - 1) % count
        } else {
            (cur + 1) % count
        },
        Ordering::Relaxed,
    );
}

/// Etkin sekmeye göre sol/sağ ok ile sekme değiştirir.
pub fn select_tab_step(forward: bool) {
    let cur = active_tab();
    let next = if forward {
        (cur + 1) % TAB_COUNT
    } else {
        (cur + TAB_COUNT - 1) % TAB_COUNT
    };
    select_tab(next);
}

/// Odaklı denetimin ekran dikdörtgeni (çizim ve Enter eşlemesi ortak kullanır).
pub fn focused_rect(
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
) -> Option<(usize, usize, usize, usize)> {
    let f = focused_control();
    if f < TAB_COUNT {
        return Some(tab_rect(win_x, win_y, win_w, f));
    }
    if f == ACTION_SETTINGS {
        return Some(refresh_btn_rect(win_x, win_y, win_w, win_h));
    }
    if f == ACTION_EXTRA && active_tab() == 1 {
        return Some(net_test_btn_rect(win_x, win_y, win_w, win_h));
    }
    if f == ACTION_EXTRA && active_tab() == 3 {
        return Some(perf_btn_rect(win_x, win_y, win_w, win_h));
    }
    if f == ACTION_EXTRA && active_tab() == 5 {
        return Some(rescan_btn_rect(win_x, win_y, win_w, win_h));
    }
    None
}

/// Odaklı denetimin Enter ile tetiklediği eylem.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpAction {
    None,
    OpenSettings,
    NetTest,
    PerfTest,
    RescanDrivers,
}

/// Odaklı denetimi etkinleştirir; ana eylemler `event_loop` tarafından yürütülür.
pub fn activate_focused() -> CpAction {
    let f = focused_control();
    if f < TAB_COUNT {
        select_tab(f);
        return CpAction::None;
    }
    if f == ACTION_SETTINGS {
        return CpAction::OpenSettings;
    }
    if f == ACTION_EXTRA && active_tab() == 1 {
        return CpAction::NetTest;
    }
    if f == ACTION_EXTRA && active_tab() == 3 {
        return CpAction::PerfTest;
    }
    if f == ACTION_EXTRA && active_tab() == 5 {
        return CpAction::RescanDrivers;
    }
    CpAction::None
}

/// Panelin standart klavye tuşları.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpKey {
    Tab,
    BackTab,
    Enter,
    Space,
    Escape,
}

/// Tuş olayı sonucu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpKeyResult {
    /// Tuş panel tarafından tüketilmedi (ana döngü işlemeye devam eder).
    Ignored,
    /// Odak/sekme değişti; yeniden çizim gerekir.
    Redraw,
    /// Enter/Space → ana eylem (ana döngü tarafından yürütülür).
    Action(CpAction),
}

/// Kontrol Paneli'nin standart klavye sözleşmesi:
///
/// * `Tab` / `Shift+Tab` → odağı sıradaki denetime taşır (başa döner).
/// * `Enter` / `Space` → odaklı denetimi etkinleştirir.
/// * `Esc` → birincil durum varsa (`primary_present`) yalnızca onu kapatır ve
///   `Ignored` döner; aksi halde panel varsayılanına döner (Sekme 0,
///   odak "Ayarlar" düğmesi).
pub fn handle_key(key: CpKey, primary_present: bool) -> CpKeyResult {
    match key {
        CpKey::Tab => {
            focus_next_control(false);
            CpKeyResult::Redraw
        }
        CpKey::BackTab => {
            focus_next_control(true);
            CpKeyResult::Redraw
        }
        CpKey::Enter | CpKey::Space => CpKeyResult::Action(activate_focused()),
        CpKey::Escape => {
            if primary_present {
                return CpKeyResult::Ignored;
            }
            select_tab(0);
            FOCUSED_CONTROL.store(ACTION_SETTINGS, Ordering::Relaxed);
            CpKeyResult::Redraw
        }
    }
}

/// Pencere kapatıldığında sekmeyi başa alır.
pub fn reset() {
    ACTIVE_TAB.store(0, Ordering::Relaxed);
    FOCUSED_CONTROL.store(0, Ordering::Relaxed);
    NET_TEST_STATE.store(0, Ordering::Relaxed);
    REFRESHED_AT_MS.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Panel durumu global olduğu için testler sıralı çalıştırılır.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn tab_and_focus_state_machine() {
        let _g = lock();
        reset();
        assert_eq!(active_tab(), 0);
        assert_eq!(focused_control(), 0);
        // Tab: sekmeler → Ayarlar düğmesi → sekmeye döner (Sistem'de ekstra eylem yok).
        for _ in 0..TAB_COUNT {
            assert_eq!(handle_key(CpKey::Tab, false), CpKeyResult::Redraw);
        }
        assert_eq!(focused_control(), ACTION_SETTINGS);
        assert!(focus_on_action());
        match handle_key(CpKey::Enter, false) {
            CpKeyResult::Action(CpAction::OpenSettings) => {}
            _ => panic!("Enter, Ayarlar düğmesini etkinleştirmeli"),
        }
        assert_eq!(handle_key(CpKey::Tab, false), CpKeyResult::Redraw);
        assert_eq!(focused_control(), 0, "odak başa dönmeli");
        // Shift+Tab geriye sarar.
        handle_key(CpKey::BackTab, false);
        assert_eq!(focused_control(), ACTION_SETTINGS);
    }

    #[test]
    fn extra_action_only_on_supported_tabs() {
        let _g = lock();
        reset();
        // Sistem: odak Ayarlar'a alındığında Tab listesinde ekstra yok.
        handle_key(CpKey::Tab, false);
        assert_eq!(focused_control(), ACTION_SETTINGS);
        handle_key(CpKey::Tab, false);
        assert_eq!(focused_control(), 0);

        // Ağ sekmesi: Ayarlar → Ağ Testi → sekme başı.
        select_tab(1);
        focus_tab(ACTION_SETTINGS);
        handle_key(CpKey::Tab, false);
        assert_eq!(focused_control(), ACTION_EXTRA);
        match activate_focused() {
            CpAction::NetTest => {}
            _ => panic!("Ağ Testi bekleniyordu"),
        }
        // Testler: Performans testi.
        select_tab(3);
        focus_tab(ACTION_SETTINGS);
        handle_key(CpKey::Tab, false);
        assert!(matches!(activate_focused(), CpAction::PerfTest));
        // Sürücüler: Yeniden tara.
        select_tab(5);
        focus_tab(ACTION_SETTINGS);
        handle_key(CpKey::Tab, false);
        assert!(matches!(activate_focused(), CpAction::RescanDrivers));

        // Ekstra eylemi olmayan sekmeye geçince odak Ayarlar'a döner.
        focus_tab(ACTION_EXTRA);
        select_tab(2);
        assert_eq!(focused_control(), ACTION_SETTINGS);
    }

    #[test]
    fn escape_closes_primary_or_resets_panel() {
        let _g = lock();
        reset();
        select_tab(3);
        focus_tab(ACTION_EXTRA);
        // Birincil durum (onay/overlay) varsa Esc panelin kendi durumunu bozmaz.
        assert_eq!(handle_key(CpKey::Escape, true), CpKeyResult::Ignored);
        assert_eq!(active_tab(), 3);
        // Birincil durum yoksa panel varsayılanına döner.
        assert_eq!(handle_key(CpKey::Escape, false), CpKeyResult::Redraw);
        assert_eq!(active_tab(), 0);
        assert_eq!(focused_control(), ACTION_SETTINGS);
    }

    #[test]
    fn enter_on_tab_selects_it() {
        let _g = lock();
        reset();
        focus_tab(5);
        assert_eq!(
            handle_key(CpKey::Enter, false),
            CpKeyResult::Action(CpAction::None)
        );
        assert_eq!(active_tab(), 5);
        assert_eq!(
            handle_key(CpKey::Space, false),
            CpKeyResult::Action(CpAction::None)
        );
    }
}

/// Panel verilerini yeniler (canli degerler bir sonraki cizimde okunur).
pub fn refresh() {
    REFRESHED_AT_MS.store(crate::monotonic_ms(), Ordering::Relaxed);
    NET_TEST_STATE.store(0, Ordering::Relaxed);
}

/// Ağ geçidine ICMP ping testi başlatır (UI kilitlemeden).
pub fn start_network_test() {
    if !crate::drivers::e1000::is_ready() {
        NET_TEST_STATE.store(3, Ordering::Relaxed);
        return;
    }
    crate::drivers::netstack::clear_ping_reply();
    let sent = crate::drivers::netstack::ping(crate::drivers::netstack::gateway_ip());
    NET_TEST_STATE.store(if sent { 1 } else { 3 }, Ordering::Relaxed);
    NET_TEST_START_MS.store(crate::monotonic_ms(), Ordering::Relaxed);
}

fn poll_network_test() {
    if NET_TEST_STATE.load(Ordering::Relaxed) != 1 {
        return;
    }
    let now = crate::monotonic_ms();
    if crate::drivers::netstack::has_ping_reply() {
        let _ = crate::drivers::netstack::take_ping_reply();
        NET_TEST_STATE.store(2, Ordering::Relaxed);
    } else if now.wrapping_sub(NET_TEST_START_MS.load(Ordering::Relaxed)) >= 2_500 {
        NET_TEST_STATE.store(3, Ordering::Relaxed);
    }
}

/// Yazı yakınlaştırmasına göre satır/sekme yüksekliği artışı (100/125/150 -> 0/5/10).
fn zoom_extra() -> usize {
    (crate::gui::font_zoom() as usize).saturating_sub(100) / 5
}

/// Sekme düğmesinin tıklanabilir dikdörtgeni.
pub fn tab_rect(
    win_x: usize,
    win_y: usize,
    win_w: usize,
    index: usize,
) -> (usize, usize, usize, usize) {
    let tab_w = win_w.saturating_sub(20) / TAB_COUNT;
    (
        win_x + 10 + index * tab_w,
        win_y + 32,
        tab_w.saturating_sub(4),
        24 + zoom_extra(),
    )
}

/// Tema değiştirme düğmesinin tıklanabilir dikdörtgeni.
pub fn theme_btn_rect(
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
) -> (usize, usize, usize, usize) {
    let w = 158 + zoom_extra() * 8;
    (
        win_x + win_w.saturating_sub(w + 12),
        win_y + win_h.saturating_sub(38),
        w,
        26,
    )
}

/// "Yenile" düğmesinin tıklanabilir dikdörtgeni.
pub fn refresh_btn_rect(
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
) -> (usize, usize, usize, usize) {
    let theme = theme_btn_rect(win_x, win_y, win_w, win_h);
    (theme.0.saturating_sub(100 + 8), theme.1, 100, theme.3)
}

/// "Ağ Testi" düğmesinin tıklanabilir dikdörtgeni.
pub fn net_test_btn_rect(
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
) -> (usize, usize, usize, usize) {
    let anchor = perf_btn_rect(win_x, win_y, win_w, win_h);
    let bw = 124.min(win_w.saturating_sub(24));
    (anchor.0, anchor.1, bw, anchor.3)
}

/// "Yeniden Tara" düğmesinin tıklanabilir dikdörtgeni (Sürücüler sekmesi).
pub fn rescan_btn_rect(
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
) -> (usize, usize, usize, usize) {
    perf_btn_rect(win_x, win_y, win_w, win_h)
}

/// Sürücüleri yeniden tarar (donanıma dokunmayan güvenli algılama turu).
pub fn rescan_drivers() {
    crate::boot::devices::rescan_drivers();
    refresh();
}

fn draw_row(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    label: &str,
    value: &str,
    color: u32,
) {
    MinGuiCompositor::draw_string(fb, x, y, label, Color::TEXT_MUTED);
    let value_x = x + crate::gui::sp(150);
    let clip = (
        value_x,
        y.saturating_sub(2),
        w.saturating_sub(crate::gui::sp(150)),
        crate::gui::sp(20),
    );
    MinGuiCompositor::draw_string_clipped(fb, value_x, y, value, color, clip);
}

/// Kart başlığı çubuğu: ilgili satır grubunu bir başlıkla görsel olarak ayırır.
/// Dönen değer, başlık çubuğunun altındaki ilk satırın y konumudur.
fn draw_section(fb: &mut Framebuffer, x: usize, y: usize, w: usize, title: &str) -> usize {
    let h = sp(20);
    fb.draw_rect(x, y, w, h, Color::TITLE_BAR_INACTIVE);
    fb.draw_rect_outline(x, y, w, h, Color::BORDER);
    MinGuiCompositor::draw_string(
        fb,
        x + sp(8),
        y + (h.saturating_sub(MinGuiCompositor::text_height())) / 2,
        title,
        Color::ACCENT_BLUE,
    );
    y.saturating_add(h).saturating_add(sp(6))
}

fn push_time(buf: &mut TextBuf, now: &DateTime) {
    buf.push_u2(now.hour as u32);
    buf.push_str(":");
    buf.push_u2(now.minute as u32);
    buf.push_str(":");
    buf.push_u2(now.second as u32);
}

fn push_date(buf: &mut TextBuf, now: &DateTime) {
    buf.push_u2(now.day as u32);
    buf.push_str(".");
    buf.push_u2(now.month as u32);
    buf.push_str(".");
    buf.push_u4(now.year as u32);
}

fn push_uptime(buf: &mut TextBuf, total_seconds: usize) {
    let mut text = [0u8; crate::gui::timefmt::MAX_TEXT];
    let len = crate::gui::timefmt::format_duration(
        total_seconds as u64,
        crate::gui::ui_english(),
        &mut text,
    );
    for &byte in &text[..len] {
        buf.push_byte(byte);
    }
}

fn push_ip(buf: &mut TextBuf, ip: crate::drivers::netstack::Ipv4Address) {
    buf.push_usize(ip.0[0] as usize);
    buf.push_str(".");
    buf.push_usize(ip.0[1] as usize);
    buf.push_str(".");
    buf.push_usize(ip.0[2] as usize);
    buf.push_str(".");
    buf.push_usize(ip.0[3] as usize);
}

/// Kontrol Paneli içeriğini çizer.
pub fn render_content(
    fb: &mut Framebuffer,
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
    now: &DateTime,
    total_ram_bytes: u64,
    used_ram_bytes: u64,
    kbd_layout: KeyboardLayout,
) {
    poll_network_test();
    let names_tr = ["Sistem", "Ağ", "Hakkında", "Testler", "İzleme", "Sürücüler"];
    let names_en = [
        "System",
        "Network",
        "About",
        "Tests",
        "Monitoring",
        "Drivers",
    ];

    for i in 0..TAB_COUNT {
        let r = tab_rect(win_x, win_y, win_w, i);
        let is_active = i == active_tab();
        fb.draw_rect(
            r.0,
            r.1,
            r.2,
            r.3,
            if is_active {
                Color::TITLE_BAR
            } else {
                Color::TITLE_BAR_INACTIVE
            },
        );
        let name = crate::gui::ui_text(names_tr[i], names_en[i]);
        let tw = MinGuiCompositor::text_width(name, 1);
        let th = MinGuiCompositor::text_height();
        MinGuiCompositor::draw_string(
            fb,
            r.0 + r.2.saturating_sub(tw) / 2,
            r.1 + r.3.saturating_sub(th) / 2,
            name,
            if is_active {
                Color::ACCENT_BLUE
            } else {
                Color::TEXT_MUTED
            },
        );
    }

    let panel_x = win_x + 10;
    let panel_y = win_y + 62 + zoom_extra();
    let panel_w = win_w.saturating_sub(20);
    let panel_h = win_h.saturating_sub(62 + zoom_extra() + 52);
    fb.draw_rect(
        panel_x,
        panel_y,
        panel_w,
        panel_h,
        Color::TITLE_BAR_INACTIVE,
    );

    let left = panel_x + 16;
    let row_w = panel_w.saturating_sub(32);
    let mut y = panel_y + 12;
    let step = 24 + zoom_extra();

    match active_tab() {
        0 => {
            // === Üst kısım: Durum kartları (RAM, Disk, Sürücüler) ===
            let card_w = row_w / 3;
            let card_h = sp(72);
            let gap = sp(8);
            let cards_y = panel_y + 12;

            // RAM Kartı
            let ram_card_x = left;
            fb.draw_rect(ram_card_x, cards_y, card_w, card_h, Color::WINDOW_BG);
            fb.draw_rect_outline(ram_card_x, cards_y, card_w, card_h, Color::BORDER);

            let total_mb = total_ram_bytes / (1024 * 1024);
            let used_mb = used_ram_bytes / (1024 * 1024);
            let free_mb = total_mb.saturating_sub(used_mb);
            let ram_percent = if total_mb > 0 {
                (used_mb * 100).checked_div(total_mb).unwrap_or(0)
            } else {
                0
            };

            let ram_title = crate::gui::ui_text("🧠 RAM", "🧠 RAM");
            MinGuiCompositor::draw_string(
                fb,
                ram_card_x + sp(8),
                cards_y + sp(6),
                ram_title,
                Color::ACCENT_BLUE,
            );

            let mut ram_val = TextBuf::new();
            ram_val.push_usize(used_mb as usize);
            ram_val.push_str(" / ");
            ram_val.push_usize(total_mb as usize);
            ram_val.push_str(" MB");
            MinGuiCompositor::draw_string(
                fb,
                ram_card_x + sp(8),
                cards_y + sp(24),
                ram_val.as_str(),
                Color::TEXT_MAIN,
            );

            let mut ram_free = TextBuf::new();
            ram_free.push_str(crate::gui::ui_text("Boş: ", "Free: "));
            ram_free.push_usize(free_mb as usize);
            ram_free.push_str(" MB  ");
            ram_free.push_str(crate::gui::ui_text("%", "%"));
            ram_free.push_usize(ram_percent as usize);
            MinGuiCompositor::draw_string(
                fb,
                ram_card_x + sp(8),
                cards_y + sp(40),
                ram_free.as_str(),
                if ram_percent >= 90 {
                    Color::ACCENT_RED
                } else if ram_percent >= 70 {
                    Color::ACCENT_YELLOW
                } else {
                    Color::ACCENT_GREEN
                },
            );

            // RAM doluluk çubuğu
            let bar_x = ram_card_x + sp(8);
            let bar_y = cards_y + card_h - sp(18);
            let bar_w = card_w - sp(16);
            fb.draw_rect(bar_x, bar_y, bar_w, sp(10), Color::DARK_BG);
            let fill = bar_w.saturating_mul(ram_percent.min(100) as usize) / 100;
            let bar_color = if ram_percent >= 90 {
                Color::ACCENT_RED
            } else if ram_percent >= 70 {
                Color::ACCENT_YELLOW
            } else {
                Color::ACCENT_GREEN
            };
            fb.draw_rect(bar_x, bar_y, fill, sp(10), bar_color);

            // Disk Kartı
            let disk_card_x = ram_card_x + card_w + gap;
            fb.draw_rect(disk_card_x, cards_y, card_w, card_h, Color::WINDOW_BG);
            fb.draw_rect_outline(disk_card_x, cards_y, card_w, card_h, Color::BORDER);

            let sectors = crate::drivers::ata::drive_capacity_sectors(0);
            let disk_mb = sectors.saturating_mul(512) / (1024 * 1024);

            let disk_title = crate::gui::ui_text("💾 Disk", "💾 Disk");
            MinGuiCompositor::draw_string(
                fb,
                disk_card_x + sp(8),
                cards_y + sp(6),
                disk_title,
                Color::ACCENT_GREEN,
            );

            let mut disk_val = TextBuf::new();
            if sectors == 0 {
                disk_val.push_str(crate::gui::ui_text("Sürücü yok", "No drive"));
            } else {
                disk_val.push_usize(disk_mb as usize);
                disk_val.push_str(" MB");
            }
            MinGuiCompositor::draw_string(
                fb,
                disk_card_x + sp(8),
                cards_y + sp(24),
                disk_val.as_str(),
                if sectors == 0 {
                    Color::ACCENT_RED
                } else {
                    Color::TEXT_MAIN
                },
            );

            let mut disk_detail = TextBuf::new();
            if sectors > 0 {
                disk_detail.push_str(crate::gui::ui_text(
                    "ATA0 • FAT32/ext4",
                    "ATA0 • FAT32/ext4",
                ));
            } else {
                disk_detail.push_str(crate::gui::ui_text("Bağlı değil", "Not connected"));
            }
            MinGuiCompositor::draw_string(
                fb,
                disk_card_x + sp(8),
                cards_y + sp(40),
                disk_detail.as_str(),
                Color::TEXT_MUTED,
            );

            // Disk çubuğu (sadece kapasite gösterimi)
            let disk_bar_x = disk_card_x + sp(8);
            let disk_bar_y = cards_y + card_h - sp(18);
            let disk_bar_w = card_w - sp(16);
            fb.draw_rect(disk_bar_x, disk_bar_y, disk_bar_w, sp(10), Color::DARK_BG);
            if sectors > 0 {
                fb.draw_rect(
                    disk_bar_x,
                    disk_bar_y,
                    disk_bar_w,
                    sp(10),
                    Color::ACCENT_GREEN,
                );
            }

            // Sürücüler Kartı
            let drv_card_x = disk_card_x + card_w + gap;
            fb.draw_rect(drv_card_x, cards_y, card_w, card_h, Color::WINDOW_BG);
            fb.draw_rect_outline(drv_card_x, cards_y, card_w, card_h, Color::BORDER);

            use crate::drivers::status;
            let (n_ready, n_unavail, n_err, n_other) = status::summary();

            let drv_title = crate::gui::ui_text("⚙️ Sürücüler", "⚙️ Drivers");
            MinGuiCompositor::draw_string(
                fb,
                drv_card_x + sp(8),
                cards_y + sp(6),
                drv_title,
                Color::ACCENT_PINK,
            );

            let mut drv_val = TextBuf::new();
            drv_val.push_str(crate::gui::ui_text("Hazır: ", "Ready: "));
            drv_val.push_usize(n_ready);
            drv_val.push_str("  ");
            drv_val.push_str(crate::gui::ui_text("Kullanılamıyor: ", "Unavail: "));
            drv_val.push_usize(n_unavail);
            MinGuiCompositor::draw_string(
                fb,
                drv_card_x + sp(8),
                cards_y + sp(24),
                drv_val.as_str(),
                Color::TEXT_MAIN,
            );

            let mut drv_err = TextBuf::new();
            drv_err.push_str(crate::gui::ui_text("Hata: ", "Error: "));
            drv_err.push_usize(n_err);
            if n_other > 0 {
                drv_err.push_str("  ");
                drv_err.push_str(crate::gui::ui_text("Bilinmeyen: ", "Unknown: "));
                drv_err.push_usize(n_other);
            }
            MinGuiCompositor::draw_string(
                fb,
                drv_card_x + sp(8),
                cards_y + sp(40),
                drv_err.as_str(),
                if n_err > 0 {
                    Color::ACCENT_RED
                } else {
                    Color::TEXT_MUTED
                },
            );

            // Sürücüler çubuğu
            let drv_bar_x = drv_card_x + sp(8);
            let drv_bar_y = cards_y + card_h - sp(18);
            let drv_bar_w = card_w - sp(16);
            fb.draw_rect(drv_bar_x, drv_bar_y, drv_bar_w, sp(10), Color::DARK_BG);
            let total_drv = n_ready + n_unavail + n_err + n_other;
            if total_drv > 0 {
                let ready_w = drv_bar_w
                    .saturating_mul(n_ready)
                    .checked_div(total_drv)
                    .unwrap_or(0);
                let unavail_w = drv_bar_w
                    .saturating_mul(n_unavail)
                    .checked_div(total_drv)
                    .unwrap_or(0);
                let err_w = drv_bar_w
                    .saturating_mul(n_err)
                    .checked_div(total_drv)
                    .unwrap_or(0);
                let mut x = drv_bar_x;
                if ready_w > 0 {
                    fb.draw_rect(x, drv_bar_y, ready_w, sp(10), Color::ACCENT_GREEN);
                    x += ready_w;
                }
                if unavail_w > 0 {
                    fb.draw_rect(x, drv_bar_y, unavail_w, sp(10), Color::ACCENT_YELLOW);
                    x += unavail_w;
                }
                if err_w > 0 {
                    fb.draw_rect(x, drv_bar_y, err_w, sp(10), Color::ACCENT_RED);
                }
            }

            // Kartlar altına boşluk
            let mut y = cards_y + card_h + sp(16);
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("İşletim Sistemi:", "Operating System:"),
                "minOs (x86_64)",
                Color::TEXT_MAIN,
            );
            y += step;
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Çekirdek:", "Kernel:"),
                "Rust no_std • Limine v8",
                Color::TEXT_MAIN,
            );
            y += step;

            let mut cpu = TextBuf::new();
            cpu.push_str(crate::boot::cpu::cpu_vendor());
            cpu.push_str(" • ");
            cpu.push_usize(crate::boot::cpu::cpu_logical_count() as usize);
            cpu.push_str(crate::gui::ui_text(" mantıksal çekirdek", " logical cores"));
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("İşlemci:", "Processor:"),
                cpu.as_str(),
                Color::ACCENT_GREEN,
            );
            y += step;

            y = draw_section(fb, left, y, row_w, crate::gui::ui_text("Bellek", "Memory"));

            let total_mb = total_ram_bytes / (1024 * 1024);
            let used_mb = used_ram_bytes / (1024 * 1024);
            let percent = used_ram_bytes
                .saturating_mul(100)
                .checked_div(total_ram_bytes)
                .map(|v| v as usize)
                .unwrap_or(0);
            let mut ram = TextBuf::new();
            ram.push_usize(used_mb as usize);
            ram.push_str(" / ");
            ram.push_usize(total_mb as usize);
            ram.push_str(" MB (%");
            ram.push_usize(percent);
            ram.push_str(")");
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Bellek:", "Memory:"),
                ram.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            // Bellek doluluk çubuğu.
            let bar_x = left + crate::gui::sp(150);
            let bar_space = (panel_x + panel_w).saturating_sub(16 + bar_x);
            let bar_w = row_w.min(360).min(bar_space);
            fb.draw_rect(bar_x, y - 4, bar_w, 8, Color::DARK_BG);
            let fill = bar_w.saturating_mul(percent.min(100)) / 100;
            let bar_color = if percent >= 90 {
                Color::ACCENT_RED
            } else if percent >= 70 {
                Color::ACCENT_YELLOW
            } else {
                Color::ACCENT_GREEN
            };
            fb.draw_rect(bar_x, y - 4, fill, 8, bar_color);
            y += step;

            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Depolama", "Storage"),
            );
            let sectors = crate::drivers::ata::drive_capacity_sectors(0);
            let megabytes = sectors.saturating_mul(512) / (1024 * 1024);
            let mut storage = TextBuf::new();
            if sectors == 0 {
                storage.push_str(crate::gui::ui_text("Sürücü bulunamadı", "Drive not found"));
            } else {
                storage.push_usize(megabytes as usize);
                storage.push_str(" MB");
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Disk:", "Disk:"),
                storage.as_str(),
                if sectors == 0 {
                    Color::ACCENT_RED
                } else {
                    Color::TEXT_MAIN
                },
            );
            y += step;

            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Ekran · Tema · Klavye", "Display · Theme · Keyboard"),
            );

            let mut screen = TextBuf::new();
            screen.push_usize(fb.width);
            screen.push_str("x");
            screen.push_usize(fb.height);
            screen.push_str(", 32-bit");
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Ekran:", "Display:"),
                screen.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Tema:", "Theme:"),
                crate::gui::theme::current().name(),
                Color::ACCENT_PINK,
            );
            y += step;

            let kb = match kbd_layout {
                KeyboardLayout::TurkishQ => crate::gui::ui_text("Türkçe Q (TR)", "Turkish Q (TR)"),
                KeyboardLayout::EnglishUS => crate::gui::ui_text("İngilizce (US)", "English (US)"),
            };
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Klavye:", "Keyboard:"),
                kb,
                Color::TEXT_MAIN,
            );
            y += step;

            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Süre / Tarih", "Uptime / Date"),
            );

            let mut info = TextBuf::new();
            push_uptime(&mut info, (crate::monotonic_ms() / 1000) as usize);
            info.push_str(" • ");
            push_date(&mut info, now);
            info.push_str(" ");
            push_time(&mut info, now);
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Süre / Tarih:", "Uptime / Date:"),
                info.as_str(),
                Color::TEXT_MAIN,
            );
        }
        1 => {
            y = draw_section(fb, left, y, row_w, crate::gui::ui_text("Ağ", "Network"));

            let nic_ok = crate::drivers::e1000::is_ready();
            let mut nic = TextBuf::new();
            if nic_ok {
                if crate::drivers::net::is_isa() {
                    nic.push_str("NE2000 (ISA)");
                } else {
                    nic.push_str(crate::drivers::e1000::nic_name());
                }
                nic.push_str(crate::gui::ui_text(" (hazır)", " (ready)"));
            } else {
                nic.push_str(crate::gui::ui_text("Sürücü bulunamadı", "Driver not found"));
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Ağ sürücüsü:", "Network driver:"),
                nic.as_str(),
                if nic_ok {
                    Color::ACCENT_GREEN
                } else {
                    Color::ACCENT_RED
                },
            );
            y += step;

            let link = if !nic_ok {
                crate::gui::ui_text("Yok", "None")
            } else if crate::drivers::e1000::link_up() {
                crate::gui::ui_text("Bağlı (link up)", "Connected (link up)")
            } else {
                crate::gui::ui_text("Kopuk (link down)", "Disconnected (link down)")
            };
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Bağlantı:", "Link:"),
                link,
                if nic_ok && crate::drivers::e1000::link_up() {
                    Color::ACCENT_GREEN
                } else {
                    Color::ACCENT_RED
                },
            );
            y += step;

            let ready = crate::drivers::netstack::dhcp_ready();
            let mut ip = TextBuf::new();
            if nic_ok {
                push_ip(&mut ip, crate::drivers::netstack::local_ip());
            } else {
                ip.push_str("—");
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("IP adresi:", "IP address:"),
                ip.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            let mut mask = TextBuf::new();
            push_ip(&mut mask, crate::drivers::netstack::subnet_mask());
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Alt ağ maskesi:", "Subnet mask:"),
                mask.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            let mut gw = TextBuf::new();
            push_ip(&mut gw, crate::drivers::netstack::gateway_ip());
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Ağ geçidi:", "Gateway:"),
                gw.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            let mut dns = TextBuf::new();
            push_ip(&mut dns, crate::drivers::netstack::dns_server());
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("DNS sunucusu:", "DNS server:"),
                dns.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("DHCP:", "DHCP:"),
                if ready {
                    crate::gui::ui_text("Hazır", "Ready")
                } else {
                    crate::gui::ui_text("Bekliyor", "Pending")
                },
                if ready {
                    Color::ACCENT_GREEN
                } else {
                    Color::ACCENT_YELLOW
                },
            );
            y += step;

            let (audio, audio_ok) = if crate::drivers::hda::is_ready() {
                if crate::drivers::hda::is_dma_progressing() {
                    (
                        crate::gui::ui_text("Intel HDA (DMA aktif)", "Intel HDA (DMA active)"),
                        true,
                    )
                } else {
                    (
                        crate::gui::ui_text("Intel HDA (DMA beklemede)", "Intel HDA (DMA standby)"),
                        true,
                    )
                }
            } else {
                (
                    crate::gui::ui_text("PC Speaker (PWM yedeği)", "PC Speaker (PWM fallback)"),
                    false,
                )
            };
            y = draw_section(fb, left, y, row_w, crate::gui::ui_text("Ses", "Audio"));

            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Ses:", "Audio:"),
                audio,
                if audio_ok {
                    Color::ACCENT_GREEN
                } else {
                    Color::ACCENT_YELLOW
                },
            );
            y += step;

            y = draw_section(fb, left, y, row_w, crate::gui::ui_text("Test", "Test"));

            let (net_test, net_color) = match NET_TEST_STATE.load(Ordering::Relaxed) {
                1 => (
                    crate::gui::ui_text(
                        "Ağ testi: yanıt bekleniyor...",
                        "Network test: waiting for reply...",
                    ),
                    Color::ACCENT_YELLOW,
                ),
                2 => (
                    crate::gui::ui_text(
                        "Ağ testi: BAŞARILI (ICMP yanıtı alındı)",
                        "Network test: SUCCESS (ICMP reply received)",
                    ),
                    Color::ACCENT_GREEN,
                ),
                3 => (
                    crate::gui::ui_text(
                        "Ağ testi: BAŞARISIZ (yanıt yok)",
                        "Network test: FAILED (no reply)",
                    ),
                    Color::ACCENT_RED,
                ),
                _ => (
                    crate::gui::ui_text("Ağ testi: hazır", "Network test: ready"),
                    Color::TEXT_MUTED,
                ),
            };
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Test:", "Test:"),
                net_test,
                net_color,
            );

            // Ağ testi düğmesi (sadece Ağ sekmesinde görünür).
            let net_btn = net_test_btn_rect(win_x, win_y, win_w, win_h);
            fb.draw_rect(
                net_btn.0,
                net_btn.1,
                net_btn.2,
                net_btn.3,
                Color::ACCENT_GREEN,
            );
            let net_label = crate::gui::ui_text("Ağ Testi", "Network Test");
            let ntw = MinGuiCompositor::text_width(net_label, 1);
            MinGuiCompositor::draw_string(
                fb,
                net_btn.0 + net_btn.2.saturating_sub(ntw) / 2,
                net_btn.1 + 7,
                net_label,
                MinGuiCompositor::text_on(Color::ACCENT_GREEN),
            );
        }
        3 => {
            // Testler sekmesi: performans testi.
            let phase = perf_phase();
            let title = match phase {
                PerfPhase::Idle => {
                    crate::gui::ui_text("Performans testi hazır", "Performance test ready")
                }
                PerfPhase::Benchmark => {
                    crate::gui::ui_text("Donanım ölçülüyor...", "Measuring hardware...")
                }
                PerfPhase::Collecting => crate::gui::ui_text("Ölçülüyor...", "Measuring..."),
                PerfPhase::Finished => crate::gui::ui_text("Sonuç", "Result"),
            };
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Test:", "Test:"),
                title,
                Color::TEXT_MAIN,
            );
            y += step;

            // İlerleme çubuğu.
            let bar_x = left + crate::gui::sp(150);
            let bar_space = (panel_x + panel_w).saturating_sub(16 + bar_x);
            let bar_w = row_w.min(360).min(bar_space);
            fb.draw_rect(bar_x, y - 4, bar_w, 8, Color::DARK_BG);
            let pct = match phase {
                PerfPhase::Benchmark => 5,
                PerfPhase::Collecting => {
                    (PERF_SUM_US.load(Ordering::Relaxed) * 100 / PERF_DURATION_US).min(100) as usize
                }
                PerfPhase::Finished => 100,
                _ => 0,
            };
            fb.draw_rect(bar_x, y - 4, bar_w * pct / 100, 8, Color::ACCENT_GREEN);
            y += step + 6;

            if phase == PerfPhase::Collecting {
                let mut t = TextBuf::new();
                t.push_str(crate::gui::ui_text("İlerleme: %", "Progress: %"));
                t.push_usize(pct);
                t.push_str(crate::gui::ui_text("   - Ölçülüyor", "   - Measuring"));
                MinGuiCompositor::draw_string(fb, left, y, t.as_str(), Color::TEXT_MUTED);
                y += step;
            }

            if phase == PerfPhase::Finished {
                let res = perf_results();
                let mut r = TextBuf::new();
                r.push_str(crate::gui::ui_text("Kare hızı: ", "Frame rate: "));
                r.push_usize(res.fps as usize);
                r.push_str(" FPS");
                draw_row(fb, left, y, row_w, "", r.as_str(), Color::TEXT_MAIN);
                y += step;
                let mut r = TextBuf::new();
                r.push_str(crate::gui::ui_text(
                    "Render kapasitesi: ",
                    "Render capacity: ",
                ));
                r.push_usize(res.capacity_fps as usize);
                r.push_str(" FPS");
                draw_row(fb, left, y, row_w, "", r.as_str(), Color::TEXT_MAIN);
                y += step;
                let mut r = TextBuf::new();
                r.push_str(crate::gui::ui_text("CPU: ", "CPU: "));
                r.push_usize(res.cpu_mops as usize);
                r.push_str(" Mops/s");
                draw_row(fb, left, y, row_w, "", r.as_str(), Color::TEXT_MAIN);
                y += step;
                let mut r = TextBuf::new();
                r.push_str(crate::gui::ui_text("Bellek: ", "Memory: "));
                r.push_usize(res.mem_mbps as usize);
                r.push_str(" MB/s");
                draw_row(fb, left, y, row_w, "", r.as_str(), Color::TEXT_MAIN);
                y += step;
                let mut r = TextBuf::new();
                r.push_str(crate::gui::ui_text("Grafik: ", "Graphics: "));
                r.push_usize(res.gfx_mpix as usize);
                r.push_str(" Mpix/s");
                draw_row(fb, left, y, row_w, "", r.as_str(), Color::TEXT_MAIN);
                y += step;
                let mut r = TextBuf::new();
                r.push_str(crate::gui::ui_text("Kare sapması: ", "Frame jitter: "));
                r.push_usize(res.jitter_us as usize);
                r.push_str(" µs");
                draw_row(fb, left, y, row_w, "", r.as_str(), Color::TEXT_MAIN);
                y += step;

                let mut score_txt = TextBuf::new();
                score_txt.push_str(crate::gui::ui_text("Puan: ", "Score: "));
                score_txt.push_usize(res.score as usize);
                score_txt.push_str(" / 100 - ");
                score_txt.push_str(perf_grade(res.score));
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    left,
                    y,
                    score_txt.as_str(),
                    Color::ACCENT_GREEN,
                    (left, y, row_w, sp(22)),
                );
            } else if phase == PerfPhase::Idle {
                MinGuiCompositor::draw_string(
                    fb,
                    left,
                    y,
                    crate::gui::ui_text(
                        "CPU, bellek ve grafik hızı ölçülür; ardından 3 saniye boyunca kare hızı ve kararlılık ölçülür.",
                        "CPU, memory and graphics speed are measured, then frame rate and stability for 3 seconds.",
                    ),
                    Color::TEXT_MUTED,
                );
            }
        }
        4 => {
            // İzleme (kullanım) sekmesi: bellek baskısı, son 30 saniye
            // grafiği, sürücü başlatma süreleri ve bu ekranın render maliyeti.
            let render_start_us = now_us();

            y = draw_section(fb, left, y, row_w, crate::gui::ui_text("Bellek", "Memory"));

            let total_mb = total_ram_bytes / (1024 * 1024);
            let used_mb = used_ram_bytes / (1024 * 1024);
            let free_mb = total_mb.saturating_sub(used_mb);
            let free_pct = if total_mb > 0 {
                (free_mb * 100).checked_div(total_mb).unwrap_or(0)
            } else {
                0
            };
            let pressure = monitor::pressure_level(
                total_ram_bytes,
                total_ram_bytes.saturating_sub(used_ram_bytes),
            );
            let under_pressure = pressure.is_pressured();

            // Bellek baskısı uyarısı (seviyeye göre renk ve metin).
            if under_pressure {
                let (title, body) = monitor::pressure_message(pressure, crate::gui::ui_english());
                let (bg, fg) = if pressure == monitor::PressureLevel::Critical {
                    (Color::ACCENT_RED, Color::WHITE)
                } else {
                    (Color::ACCENT_YELLOW, Color::BLACK)
                };
                fb.draw_rect(left, y, row_w, sp(22), bg);
                let mut warn = TextBuf::new();
                warn.push_str(title);
                warn.push_str(": ");
                warn.push_str(body);
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    left + sp(8),
                    y + (sp(22).saturating_sub(MinGuiCompositor::text_height())) / 2,
                    warn.as_str(),
                    fg,
                    (left, y, row_w, sp(22)),
                );
                y += sp(22) + sp(6);
            }

            let mut ram = TextBuf::new();
            ram.push_usize(used_mb as usize);
            ram.push_str(" / ");
            ram.push_usize(total_mb as usize);
            ram.push_str(" MB (");
            ram.push_usize(free_pct as usize);
            ram.push_str("% ");
            ram.push_str(crate::gui::ui_text("boş", "free"));
            ram.push_str(")");
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Toplam / Boş RAM:", "Total / Free RAM:"),
                ram.as_str(),
                if under_pressure {
                    Color::ACCENT_RED
                } else {
                    Color::TEXT_MAIN
                },
            );
            y += step;

            // Kernel heap.
            let heap = heap_profile();
            let heap_committed_kb = heap.committed_bytes / 1024;
            let heap_mapped_kb = heap.mapped_bytes / 1024;
            let mut heap_txt = TextBuf::new();
            heap_txt.push_usize(heap_committed_kb as usize);
            heap_txt.push_str(" / ");
            heap_txt.push_usize(heap_mapped_kb as usize);
            heap_txt.push_str(" KB (commit / mapped)");
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Kernel Heap:", "Kernel Heap:"),
                heap_txt.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            // Disk (RAM'den ayrı gösterilir).
            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Depolama", "Storage"),
            );
            let sectors = crate::drivers::ata::drive_capacity_sectors(0);
            let disk_mb = sectors.saturating_mul(512) / (1024 * 1024);
            let mut disk_txt = TextBuf::new();
            if sectors == 0 {
                disk_txt.push_str(crate::gui::ui_text("Sürücü bulunamadı", "Drive not found"));
            } else {
                disk_txt.push_usize(disk_mb as usize);
                disk_txt.push_str(" MB");
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Disk:", "Disk:"),
                disk_txt.as_str(),
                if sectors == 0 {
                    Color::ACCENT_RED
                } else {
                    Color::TEXT_MAIN
                },
            );
            y += step;

            // Ağ paket ve hata sayaçları.
            y = draw_section(fb, left, y, row_w, crate::gui::ui_text("Ağ", "Network"));
            let net = diag::net_stats();
            let mut net_txt = TextBuf::new();
            net_txt.push_str(crate::gui::ui_text("Paket G/Ç:", "Packets RX/TX:"));
            net_txt.push_usize(net.rx_packets as usize);
            net_txt.push_str(" / ");
            net_txt.push_usize(net.tx_packets as usize);
            net_txt.push_str("  ");
            net_txt.push_str(crate::gui::ui_text("Bayt G/Ç:", "Bytes RX/TX:"));
            net_txt.push_usize(net.rx_bytes as usize);
            net_txt.push_str(" / ");
            net_txt.push_usize(net.tx_bytes as usize);
            draw_row(fb, left, y, row_w, "", net_txt.as_str(), Color::TEXT_MAIN);
            y += step;

            let mut err_txt = TextBuf::new();
            err_txt.push_str(crate::gui::ui_text("Hata G/Ç:", "Errors RX/TX:"));
            err_txt.push_usize(net.rx_errors as usize);
            err_txt.push_str(" / ");
            err_txt.push_usize(net.tx_errors as usize);
            err_txt.push_str("  ");
            err_txt.push_str(crate::gui::ui_text("Düşürülen:", "Dropped:"));
            err_txt.push_usize(net.dropped as usize);
            draw_row(
                fb,
                left,
                y,
                row_w,
                "",
                err_txt.as_str(),
                if net.rx_errors > 0 || net.tx_errors > 0 || net.dropped > 0 {
                    Color::ACCENT_YELLOW
                } else {
                    Color::TEXT_MAIN
                },
            );
            y += step;

            // Sürücü başlatma süreleri (en yavaştan hızlıya).
            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Sürücü Başlatma", "Driver Init"),
            );
            let raw_timings = diag::driver_init_timings();
            if raw_timings.is_empty() {
                draw_row(
                    fb,
                    left,
                    y,
                    row_w,
                    crate::gui::ui_text("Veri yok", "No data"),
                    "",
                    Color::TEXT_MUTED,
                );
                y += step;
            } else {
                let mut rows: alloc::vec::Vec<monitor::DriverTiming> =
                    raw_timings.iter().map(timing_row).collect();
                let summary = monitor::timing_summary(&rows, DRIVER_INIT_BUDGET_US);

                // Özet satırı: toplam süre, en yavaş sürücü ve bütçe oranı.
                let mut sum_txt = TextBuf::new();
                sum_txt.push_str(crate::gui::ui_text("Toplam: ", "Total: "));
                push_ms(&mut sum_txt, summary.total_us);
                sum_txt.push_str(crate::gui::ui_text(" • En yavaş: ", " • Slowest: "));
                sum_txt.push_str(if summary.slowest.is_empty() {
                    "-"
                } else {
                    summary.slowest
                });
                sum_txt.push_str(" ");
                push_ms(&mut sum_txt, summary.slowest_us);
                sum_txt.push_str(" ");
                let mut aux = TextBuf::new();
                aux.push_usize(summary.budget_pct);
                aux.push_str("% ");
                aux.push_str(crate::gui::ui_text("bütçe", "of budget"));
                sum_txt.push_str(aux.as_str());
                draw_row(
                    fb,
                    left,
                    y,
                    row_w,
                    "",
                    sum_txt.as_str(),
                    if summary.failed > 0 {
                        Color::ACCENT_RED
                    } else if summary.budget_pct > 100 {
                        Color::ACCENT_YELLOW
                    } else {
                        Color::TEXT_MUTED
                    },
                );
                y += step;

                rows = monitor::sort_slowest_first(&rows);
                for t in rows.iter() {
                    let mut t_txt = TextBuf::new();
                    t_txt.push_str(t.name);
                    t_txt.push_str(": ");
                    push_ms(&mut t_txt, t.us);
                    t_txt.push_str(" (");
                    t_txt.push_usize(t.us as usize);
                    t_txt.push_str(" µs)");
                    if !t.ok {
                        t_txt.push_str(" ");
                        t_txt.push_str(crate::gui::ui_text("BAŞARISIZ", "FAILED"));
                    }
                    draw_row(
                        fb,
                        left,
                        y,
                        row_w,
                        "",
                        t_txt.as_str(),
                        if t.ok {
                            Color::TEXT_MAIN
                        } else {
                            Color::ACCENT_RED
                        },
                    );
                    y += step;
                }
            }

            // Son 30 saniyelik kullanım grafiği (metin tabanlı sparkline).
            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Kullanım Geçmişi (30s)", "Usage History (30s)"),
            );
            let hist = monitor::history();
            if hist.len == 0 {
                draw_row(
                    fb,
                    left,
                    y,
                    row_w,
                    crate::gui::ui_text("Henüz örnek yok", "No samples yet"),
                    "",
                    Color::TEXT_MUTED,
                );
                y += step;
            } else {
                let count = hist.len;
                let mut spark = [0u8; memory::HISTORY_CAPACITY];

                // RAM ölçeği: kullanım dar bir bantta dalgalanıyorsa
                // (ör. 128–130 MB) tabanı düşür; düz çizgi çizilmesin.
                let max_mem = monitor::history_max(hist.mem());
                let min_mem = hist.mem().iter().copied().min().unwrap_or(0);
                let baseline = if max_mem.saturating_sub(min_mem) * 4 < max_mem {
                    min_mem.saturating_mul(9) / 10
                } else {
                    0
                };
                let shifted: alloc::vec::Vec<usize> = hist
                    .mem()
                    .iter()
                    .map(|&v| v.saturating_sub(baseline))
                    .collect();
                let n = memory::render_sparkline_ascii(
                    &shifted,
                    max_mem.saturating_sub(baseline),
                    &mut spark,
                );
                let spark_str = core::str::from_utf8(&spark[..n]).unwrap_or("");
                draw_row(
                    fb,
                    left,
                    y,
                    row_w,
                    crate::gui::ui_text("RAM:", "RAM:"),
                    spark_str,
                    Color::ACCENT_BLUE,
                );
                y += step;

                let max_cpu = monitor::history_max(hist.cpu());
                let n = memory::render_sparkline_ascii(hist.cpu(), max_cpu.max(1), &mut spark);
                let spark_str = core::str::from_utf8(&spark[..n]).unwrap_or("");
                draw_row(
                    fb,
                    left,
                    y,
                    row_w,
                    crate::gui::ui_text("CPU:", "CPU:"),
                    spark_str,
                    Color::ACCENT_GREEN,
                );
                y += step;

                // Özet: örnek sayısı ve tepe değerler.
                let mut foot = TextBuf::new();
                foot.push_usize(count);
                foot.push_str(" ");
                foot.push_str(crate::gui::ui_text(
                    "örnek • RAM tepe ",
                    "samples • RAM peak ",
                ));
                foot.push_usize(max_mem);
                foot.push_str(" MB ");
                let mut cpu_peak = TextBuf::new();
                cpu_peak.push_str(crate::gui::ui_text("CPU tepe %", "CPU peak %"));
                cpu_peak.push_usize(monitor::clamp_pct(max_cpu));
                foot.push_str(cpu_peak.as_str());
                draw_row(fb, left, y, row_w, "", foot.as_str(), Color::TEXT_MUTED);
                y += step;
            }

            // İzleme ekranının kendi render maliyeti (kare bütçesine oran).
            let render_us = now_us().saturating_sub(render_start_us);
            MONITOR_RENDER_US.store(render_us, Ordering::Relaxed);
            monitor::record_render(render_us, DRIVER_INIT_BUDGET_US);
            let mut perf_txt = TextBuf::new();
            perf_txt.push_str(crate::gui::ui_text(
                "Bu ekranın render süresi: ",
                "This screen's render time: ",
            ));
            perf_txt.push_usize(render_us as usize);
            perf_txt.push_str(" µs ");
            let mut aux = TextBuf::new();
            aux.push_usize(monitor::render_budget_pct());
            aux.push_str("% ");
            aux.push_str(crate::gui::ui_text("bütçe", "budget"));
            perf_txt.push_str(aux.as_str());
            draw_row(
                fb,
                left,
                y,
                row_w,
                "",
                perf_txt.as_str(),
                if monitor::render_budget_pct() > 100 {
                    Color::ACCENT_YELLOW
                } else {
                    Color::TEXT_MUTED
                },
            );
        }
        5 => {
            // Sürücüler sekmesi: ortak durum modeli (Ready/Unavailable/Error)
            // kayıt tablosu + alt sistem ayrıntıları + "Yeniden Tara".
            use crate::drivers::status::{self, DriverId, DriverPhase, Severity};

            let eng = crate::gui::ui_english();

            // Özet satırı.
            let (n_ready, n_unavail, n_err, n_other) = status::summary();
            let mut sum = TextBuf::new();
            sum.push_str(crate::gui::ui_text("Hazır: ", "Ready: "));
            sum.push_usize(n_ready);
            sum.push_str("  ");
            sum.push_str(crate::gui::ui_text("Kullanılamıyor: ", "Unavailable: "));
            sum.push_usize(n_unavail);
            sum.push_str("  ");
            sum.push_str(crate::gui::ui_text("Hata: ", "Error: "));
            sum.push_usize(n_err);
            if n_other > 0 {
                sum.push_str("  ");
                sum.push_str(crate::gui::ui_text("Bilinmiyor: ", "Unknown: "));
                sum.push_usize(n_other);
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                "",
                sum.as_str(),
                if n_err > 0 {
                    Color::ACCENT_RED
                } else {
                    Color::TEXT_MAIN
                },
            );
            y += step;

            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Donanım Sürücüleri", "Hardware Drivers"),
            );

            for id in DriverId::ALL {
                let e = status::entry(id);
                let mut val = TextBuf::new();
                val.push_str(e.phase.label(eng));
                val.push_str(" • ");
                val.push_usize(e.elapsed_ms as usize);
                val.push_str(" ms");
                if e.phase == DriverPhase::Error {
                    val.push_str(" • ");
                    val.push_str(status::error_desc(e.error_code, eng));
                }
                if !e.detail.is_empty() {
                    val.push_str(" — ");
                    val.push_str(e.detail);
                }
                let color = match e.phase.severity() {
                    Severity::Ok => Color::ACCENT_GREEN,
                    Severity::Warn => Color::ACCENT_YELLOW,
                    Severity::Fail => Color::ACCENT_RED,
                    Severity::Info => Color::TEXT_MUTED,
                };
                draw_row(fb, left, y, row_w, id.label(eng), val.as_str(), color);
                y += step;
            }

            // --- Alt sistem ayrıntıları -------------------------------------
            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Ağ Ayrıntıları", "Network Details"),
            );

            let mut nic = TextBuf::new();
            if crate::drivers::e1000::is_ready() {
                nic.push_str(crate::drivers::e1000::nic_label(eng));
            } else {
                nic.push_str(crate::gui::ui_text("Adaptör yok", "No adapter"));
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Adaptör:", "Adapter:"),
                nic.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            // MAC adresi: "52:54:00:12:34:56" biçiminde.
            let mut mac = TextBuf::new();
            if crate::drivers::e1000::is_ready() {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                let m = unsafe { crate::drivers::e1000::MAC_ADDR };
                for (i, b) in m.iter().enumerate() {
                    if i > 0 {
                        mac.push_str(":");
                    }
                    mac.push_byte(HEX[(*b >> 4) as usize]);
                    mac.push_byte(HEX[(*b & 0x0F) as usize]);
                }
            } else {
                mac.push_str("-");
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("MAC:", "MAC:"),
                mac.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            let mut link = TextBuf::new();
            link.push_str(if crate::drivers::e1000::is_ready() {
                if crate::drivers::e1000::link_up() {
                    crate::gui::ui_text("Bağlı (link up)", "Connected (link up)")
                } else {
                    crate::gui::ui_text("Kopuk (link down)", "Disconnected (link down)")
                }
            } else {
                crate::gui::ui_text("Yok", "None")
            });
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Bağlantı:", "Link:"),
                link.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            let mut ip = TextBuf::new();
            if crate::drivers::e1000::is_ready() {
                push_ip(&mut ip, crate::drivers::netstack::local_ip());
                ip.push_str("  ");
                ip.push_str(crate::gui::ui_text("DHCP:", "DHCP:"));
                ip.push_str(if crate::drivers::netstack::dhcp_ready() {
                    crate::gui::ui_text("hazır", "ready")
                } else {
                    crate::gui::ui_text("bekliyor", "pending")
                });
            } else {
                ip.push_str("-");
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("IP adresi:", "IP address:"),
                ip.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            // --- Grafik -----------------------------------------------------
            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Grafik Ayrıntıları", "Graphics Details"),
            );
            let mut res = TextBuf::new();
            res.push_usize(fb.width);
            res.push_str("x");
            res.push_usize(fb.height);
            res.push_str("  ");
            res.push_str(crate::gui::ui_text("Format:", "Format:"));
            res.push_str(match fb.bpp {
                32 => "32bpp XRGB8888",
                24 => "24bpp RGB888",
                16 => "16bpp RGB565",
                8 => "8bpp Indexed",
                _ => "?bpp",
            });
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Framebuffer:", "Framebuffer:"),
                res.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            // --- Ses ---------------------------------------------------------
            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Ses Ayrıntıları", "Audio Details"),
            );
            let mut dev = TextBuf::new();
            if crate::drivers::hda::is_ready() {
                dev.push_str("Intel HDA");
            } else {
                dev.push_str(crate::gui::ui_text(
                    "PC Speaker (yedek)",
                    "PC Speaker (fallback)",
                ));
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Aygıt:", "Device:"),
                dev.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            let mut rate = TextBuf::new();
            if crate::drivers::hda::is_ready() {
                rate.push_str("48000 Hz / 16-bit / Stereo");
            } else {
                rate.push_str("-");
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Örnekleme:", "Sample rate:"),
                rate.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            let mut dma = TextBuf::new();
            if crate::drivers::hda::is_ready() {
                dma.push_str(if crate::drivers::hda::is_dma_stalled() {
                    crate::gui::ui_text("Takıldı", "Stalled")
                } else if crate::drivers::hda::is_dma_progressing() {
                    crate::gui::ui_text("İlerliyor", "Progressing")
                } else {
                    crate::gui::ui_text("Beklemede", "Standby")
                });
            } else {
                dma.push_str(crate::gui::ui_text("Pasif", "Inactive"));
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("DMA:", "DMA:"),
                dma.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            // --- Depolama ----------------------------------------------------
            y = draw_section(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Depolama Ayrıntıları", "Storage Details"),
            );

            // ATA/ATAPI: model + kapasite.
            let mut ata_txt = TextBuf::new();
            if crate::drivers::ata::drive_count() > 0 {
                let model = crate::drivers::ata::model_of(0);
                if !model.is_empty() {
                    if let Ok(m) = core::str::from_utf8(model) {
                        ata_txt.push_str(m.trim_end_matches('\0').trim());
                        ata_txt.push_str("  ");
                    }
                }
                let mb = crate::drivers::ata::drive_capacity_sectors(0).saturating_mul(512)
                    / (1024 * 1024);
                ata_txt.push_usize(mb as usize);
                ata_txt.push_str(" MB");
            } else {
                ata_txt.push_str(crate::gui::ui_text("Aygıt yok", "No device"));
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("ATA/ATAPI:", "ATA/ATAPI:"),
                ata_txt.as_str(),
                Color::TEXT_MAIN,
            );
            y += step;

            // Diğer disk denetleyicileri: ad + (varsa) kapasite.
            let storage_rows: [StorageDriver; 4] = [
                StorageDriver {
                    id: DriverId::Ahci,
                    detected: crate::drivers::ahci::is_detected,
                    name_tr: crate::drivers::ahci::name_tr,
                    capacity_bytes: || 0,
                },
                StorageDriver {
                    id: DriverId::Nvme,
                    detected: crate::drivers::nvme::is_detected,
                    name_tr: crate::drivers::nvme::name_tr,
                    capacity_bytes: || 0,
                },
                StorageDriver {
                    id: DriverId::Sdmmc,
                    detected: crate::drivers::sdmmc::is_detected,
                    name_tr: crate::drivers::sdmmc::name_tr,
                    capacity_bytes: crate::drivers::sdmmc::capacity,
                },
                StorageDriver {
                    id: DriverId::VirtioBlk,
                    detected: crate::drivers::virtio_blk::is_detected,
                    name_tr: crate::drivers::virtio_blk::name_tr,
                    capacity_bytes: crate::drivers::virtio_blk::capacity,
                },
            ];
            for row in storage_rows {
                let mut t = TextBuf::new();
                if (row.detected)() {
                    t.push_str((row.name_tr)());
                    let cap = (row.capacity_bytes)();
                    if cap > 0 {
                        t.push_str("  ");
                        t.push_usize((cap / (1024 * 1024)) as usize);
                        t.push_str(" MB");
                    }
                } else {
                    t.push_str(crate::gui::ui_text("Algılanmadı", "Not detected"));
                }
                draw_row(
                    fb,
                    left,
                    y,
                    row_w,
                    row.id.label(eng),
                    t.as_str(),
                    Color::TEXT_MAIN,
                );
                y += step;
            }
        }
        _ => {
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Sürüm:", "Version:"),
                "minOs",
                Color::TEXT_MAIN,
            );
            y += step;
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Lisans:", "License:"),
                crate::gui::ui_text(
                    "Açık kaynak (eğitim/araştırma)",
                    "Open source (education/research)",
                ),
                Color::TEXT_MAIN,
            );
            y += step;
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Kaynak:", "Source:"),
                "github.com/tbagriyanik/minos",
                Color::ACCENT_BLUE,
            );
            y += step;
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Kısayollar:", "Shortcuts:"),
                "",
                Color::TEXT_MUTED,
            );
            y += step;
            let shortcut_rows: [(&str, &str, &str); 9] = [
                (
                    "Alt+Tab",
                    "Pencere değiştir • Shift+Alt+Tab geri • Alt+F4 kapat",
                    "Switch windows • Shift+Alt+Tab back • Alt+F4 close",
                ),
                (
                    "Super+←/→",
                    "Workspace değiştir • Shift: pencereyi taşı",
                    "Switch workspace • Shift: move window",
                ),
                (
                    "Super",
                    "Başlat • D masaüstü • M küçült • E Dosyalar • R Çalıştır",
                    "Start • D desktop • M minimize • E Files • R Run",
                ),
                (
                    "F2 • F5 • F12",
                    "Yeniden adlandır • Yenile • Ekran görüntüsü",
                    "Rename • Refresh • Screenshot",
                ),
                (
                    "Ctrl+C/X/V/A/Z/Y",
                    "Kopyala • Kes • Yapıştır • Seç • Geri/İleri al",
                    "Copy • Cut • Paste • Select • Undo/Redo",
                ),
                (
                    "Ctrl+S/O/N/F/H",
                    "Kaydet • Aç • Yeni • Bul • Değiştir",
                    "Save • Open • New • Find • Replace",
                ),
                (
                    "Ctrl+L/R • Alt+←/→",
                    "Tarayıcı: adres • yenile • geri/ileri",
                    "Browser: address • reload • back/fwd",
                ),
                ("Ctrl+Shift+D", "Tarayıcı DevTools", "Browser DevTools"),
                ("Ctrl+Alt+Del", "Görev Yöneticisi", "Task Manager"),
            ];
            for (keys, desc_tr, desc_en) in shortcut_rows {
                draw_row(
                    fb,
                    left,
                    y,
                    row_w,
                    keys,
                    crate::gui::ui_text(desc_tr, desc_en),
                    Color::TEXT_MAIN,
                );
                y += step;
            }
            draw_row(
                fb,
                left,
                y,
                row_w,
                crate::gui::ui_text("Kılavuz:", "Docs:"),
                "docs/usage.md • README.md",
                Color::TEXT_MUTED,
            );
        }
    }

    // Yenile kısayolu (her sekmede görünür).
    let refresh_btn = refresh_btn_rect(win_x, win_y, win_w, win_h);
    fb.draw_rect(
        refresh_btn.0,
        refresh_btn.1,
        refresh_btn.2,
        refresh_btn.3,
        Color::WINDOW_BG,
    );
    fb.draw_rect_outline(
        refresh_btn.0,
        refresh_btn.1,
        refresh_btn.2,
        refresh_btn.3,
        Color::BORDER,
    );
    let refresh_label = crate::gui::ui_text("Ayarlar", "Settings");
    MinGuiCompositor::draw_string(
        fb,
        refresh_btn.0 + 10,
        refresh_btn.1 + 7,
        refresh_label,
        Color::TEXT_MAIN,
    );

    // Performans testi düğmesi (Testler sekmesinde görünür).
    if active_tab() == 3 {
        let (bx, by, bw, bh) = perf_btn_rect(win_x, win_y, win_w, win_h);
        let bg = if perf_phase() == PerfPhase::Collecting {
            Color::TITLE_BAR_INACTIVE
        } else {
            Color::ACCENT_GREEN
        };
        fb.draw_rect(bx, by, bw, bh, bg);
        MinGuiCompositor::draw_string(
            fb,
            bx + sp(12),
            by + (bh.saturating_sub(MinGuiCompositor::text_height())) / 2,
            crate::gui::ui_text("Performans Testini Başlat", "Start Performance Test"),
            MinGuiCompositor::text_on(bg),
        );
    }

    // Yeniden Tara düğmesi (Sürücüler sekmesinde görünür).
    if active_tab() == 5 {
        let (bx, by, bw, bh) = rescan_btn_rect(win_x, win_y, win_w, win_h);
        fb.draw_rect(bx, by, bw, bh, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(
            fb,
            bx + sp(12),
            by + (bh.saturating_sub(MinGuiCompositor::text_height())) / 2,
            crate::gui::ui_text("Sürücüleri Yeniden Tara", "Rescan Drivers"),
            MinGuiCompositor::text_on(Color::ACCENT_BLUE),
        );
    }

    // Klavye odağı (Tab) göstergesi: odaklı denetimin çevresine halka çiz.
    if let Some(r) = focused_rect(win_x, win_y, win_w, win_h) {
        MinGuiCompositor::draw_focus_ring(fb, r.0, r.1, r.2, r.3);
    }
}
