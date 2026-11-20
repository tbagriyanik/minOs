//! Kontrol Paneli (Control Panel)
//!
//! Sistem, ağ ve sürüm bilgilerini canlı çekirdek API'lerinden okuyup gösteren
//! salt okunur panel; alt kısımdaki düğme ile tema değiştirilebilir.
//! Eski sürümdeki sabit/sahte değerler (128 MB RAM, sabit 1280x720, çelişkili
//! ağ durumu) yerine gerçek veriler kullanılır.

use crate::drivers::keyboard::KeyboardLayout;
use crate::drivers::rtc::DateTime;
use crate::drivers::video::Framebuffer;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// Sekme sayısı: Sistem, Ağ, Hakkında, Testler.
pub const TAB_COUNT: usize = 4;

static ACTIVE_TAB: AtomicUsize = AtomicUsize::new(0);
static REFRESHED_AT_MS: AtomicU64 = AtomicU64::new(0);
/// Klavye (Tab / Shift+Tab) ile gezilen odaklı denetim:
/// 0..TAB_COUNT sekmeler, TAB_COUNT "Ayarlar" düğmesi,
/// TAB_COUNT+1 "Ağ Testi" (yalnız Ağ sekmesi), TAB_COUNT+2 "Performans Testi" (yalnız Testler sekmesi).
static FOCUSED_CONTROL: AtomicUsize = AtomicUsize::new(0);
/// 0: bos, 1: yanit bekleniyor, 2: basarili, 3: basarisiz.
static NET_TEST_STATE: AtomicUsize = AtomicUsize::new(0);
static NET_TEST_START_MS: AtomicU64 = AtomicU64::new(0);

// --- Performans testi (Testler sekmesi) ---

use crate::apps::perfbench::{self, PerfRates};

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

/// Sekmeyi seçer.
pub fn select_tab(tab: usize) {
    if tab < TAB_COUNT {
        ACTIVE_TAB.store(tab, Ordering::Relaxed);
    }
}

/// Odaklı denetim sayısı; yalnızca etkin sekmede görünen düğmeler sayılır.
///
/// Sekmeler 0..TAB_COUNT, ardından "Yenile" düğmesi (TAB_COUNT) ve en
/// sonda etkin sekmeye özgü tek düğme (TAB_COUNT+1). Ağ sekmesinde bu
/// düğme "Ağ Testi", Testler sekmesinde "Performans Testi" olur.
fn focusable_count() -> usize {
    TAB_COUNT + 1 + usize::from(active_tab() == 1) + usize::from(active_tab() == 3)
}

/// Odaklı denetim indeksini döndürür.
pub fn focused_control() -> usize {
    FOCUSED_CONTROL
        .load(Ordering::Relaxed)
        .min(focusable_count().saturating_sub(1))
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
    if f == TAB_COUNT {
        return Some(refresh_btn_rect(win_x, win_y, win_w, win_h));
    }
    if f == TAB_COUNT + 1 && active_tab() == 1 {
        return Some(net_test_btn_rect(win_x, win_y, win_w, win_h));
    }
    if f == TAB_COUNT + 1 && active_tab() == 3 {
        return Some(perf_btn_rect(win_x, win_y, win_w, win_h));
    }
    None
}

/// Odaklı denetimin Enter ile tetiklediği eylem.
pub enum CpAction {
    None,
    OpenSettings,
    NetTest,
    PerfTest,
}

/// Odaklı denetimi etkinleştirir; ana eylemler `event_loop` tarafından yürütülür.
pub fn activate_focused() -> CpAction {
    let f = focused_control();
    if f < TAB_COUNT {
        select_tab(f);
        return CpAction::None;
    }
    if f == TAB_COUNT {
        return CpAction::OpenSettings;
    }
    if f == TAB_COUNT + 1 && active_tab() == 1 {
        return CpAction::NetTest;
    }
    if f == TAB_COUNT + 1 && active_tab() == 3 {
        return CpAction::PerfTest;
    }
    CpAction::None
}

/// Pencere kapatıldığında sekmeyi başa alır.
pub fn reset() {
    ACTIVE_TAB.store(0, Ordering::Relaxed);
    FOCUSED_CONTROL.store(0, Ordering::Relaxed);
    NET_TEST_STATE.store(0, Ordering::Relaxed);
    REFRESHED_AT_MS.store(0, Ordering::Relaxed);
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
    let names_tr = ["Sistem", "Ağ", "Hakkında", "Testler"];
    let names_en = ["System", "Network", "About", "Tests"];

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

    // Klavye odağı (Tab) göstergesi: odaklı denetimin çevresine halka çiz.
    if let Some(r) = focused_rect(win_x, win_y, win_w, win_h) {
        fb.draw_rect_outline(
            r.0.saturating_sub(1),
            r.1.saturating_sub(1),
            r.2.saturating_add(2),
            r.3.saturating_add(2),
            Color::ACCENT_BLUE,
        );
    }
}
