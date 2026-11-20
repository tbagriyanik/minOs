//! Performans ölçüm ve puanlama çekirdeği.
//!
//! Bu modül bilerek çekirdek bağımlılıklarından arındırılmıştır (`no_std`,
//! `alloc` dışında bağımlılık yok). Böylece **aynı** iş yükleri:
//!
//! * çekirdekte — kontrol paneli "Testler" sekmesindeki performans testi,
//! * host'ta — `tools/perfhost.rs` ile gerçek masaüstü referans ölçümü,
//!
//! üzerinde çalıştırılabilir. Puanlama çapaları bu iki ölçümden kalibre
//! edilmiştir.
#![allow(dead_code)]
//!
//! Ölçülen dört büyüklük:
//!
//! | Ölçüm      | Birim       | Ne ölçer                                   |
//! |------------|-------------|-------------------------------------------|
//! | CPU        | Mops/s      | Tamsayı karıştırma iş yükü (bağımlılık zinciri) |
//! | Bellek     | MB/s        | Ardışık okuma + yazma bant genişliği      |
//! | Grafik     | Mpix/s      | Piksel başına alfa harmanlama (framebuffer ile aynı formül) |
//! | Kare       | FPS         | Gözlenen kare teslim hızı ve kararlılığı   |

/// CPU iş yükü: xorshift64 + çarpma karışımı.
/// Zincir bağımlılığı sayesinde derleyici işi optimize edip silemez.
pub const CPU_ITERATIONS: u64 = 200_000;

/// Tek çağrıda çalıştırılan CPU iş yükü parçası.
pub const CPU_CHUNK: u64 = 50_000;

/// Bir ölçümün en az sürmesi gereken süre (µs).
///
/// Kısa ölçümlerde zamanlayıcı granülaritesi sonucu bozar (özellikle yazılım
/// emülasyonunda). İş yükü bu süreye ulaşana kadar tekrarlanır; böylece
/// ölçülen hız kararlı ve güvenilir olur.
pub const MIN_BENCH_US: u64 = 2_000;

/// Bellek iş yükü tamponu: 1 MiB.
pub const MEM_BYTES: usize = 1 << 20;
/// Bellek iş yükü geçiş sayısı (her geçiş tamponun tamamını okur ve yazar).
pub const MEM_PASSES: u32 = 4;

/// Grafik iş yükü piksel sayısı: 256K piksel (1 MiB).
pub const GFX_PIXELS: usize = 256 * 1024;
/// Grafik iş yükü geçiş sayısı.
pub const GFX_PASSES: u32 = 4;

/// 1280x720 = 921_600 piksel. Render kapasitesi karşılaştırma tabanı.
pub const FRAME_PIXELS: u64 = 1280 * 720;

// ---------------------------------------------------------------------------
// İş yükleri
// ---------------------------------------------------------------------------

/// Tamsayı karıştırma iş yükü. Dönen değer bir "sağlık" toplamıdır; iş
/// yapılmadığını doğrulamak için kullanılır.
pub fn cpu_workload(state: &mut u64, iterations: u64) -> u64 {
    let mut x = *state | 1;
    let mut acc = 0u64;
    for _ in 0..iterations {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        acc = acc.wrapping_add(v ^ (v >> 29));
    }
    *state = x;
    acc
}

/// Ardışık bellek okuma + yazma iş yükü (bant genişliği ölçümü).
pub fn mem_workload(buf: &mut [u32], passes: u32) -> u64 {
    let mut acc = 0u64;
    for _ in 0..passes {
        for (i, slot) in buf.iter_mut().enumerate() {
            let v = *slot;
            *slot = v.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            acc = acc.wrapping_add(u64::from(v ^ *slot) ^ (i as u64));
        }
    }
    acc
}

/// Alfa harmanlama iş yükü. Formül framebuffer'ın metin/gölge harmanlamasıyla
/// aynıdır: `(b * (255 - a) + f * a + 127) / 255`.
pub fn gfx_workload(buf: &mut [u32], passes: u32) -> u64 {
    let mut acc = 0u64;
    for _ in 0..passes {
        for (i, slot) in buf.iter_mut().enumerate() {
            let bg = *slot;
            let fg = 0x30_40_50u32.wrapping_add(i as u32);
            let a = 0x80u32;
            let mut out = 0u32;
            for shift in [16, 8, 0] {
                let f = (fg >> shift) & 0xFF;
                let b = (bg >> shift) & 0xFF;
                out |= ((b * (255 - a) + f * a + 127) / 255) << shift;
            }
            *slot = out;
            acc = acc.wrapping_add(u64::from(out));
        }
    }
    acc
}

// ---------------------------------------------------------------------------
// Hız hesabı
// ---------------------------------------------------------------------------

/// Ölçülen hızlar (ölçüm biriminde).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct PerfRates {
    /// Milyon işlem / saniye.
    pub cpu_mops: u32,
    /// Megabayt / saniye.
    pub mem_mbps: u32,
    /// Milyon piksel / saniye.
    pub gfx_mpix: u32,
}

impl PerfRates {
    /// `iterations` işin `elapsed_us` mikrosaniyede tamamlanmasından CPU hızı.
    pub fn cpu_mops(iterations: u64, elapsed_us: u64) -> u32 {
        rate(iterations, elapsed_us)
    }

    /// `bytes` baytın `elapsed_us` mikrosaniyede işlenmesinden bellek hızı.
    pub fn mem_mbps(bytes: u64, elapsed_us: u64) -> u32 {
        rate(bytes, elapsed_us)
    }

    /// `pixels` pikselin `elapsed_us` mikrosaniyede işlenmesinden grafik hızı.
    pub fn gfx_mpix(pixels: u64, elapsed_us: u64) -> u32 {
        rate(pixels, elapsed_us)
    }

    /// 1280x720 kare başına kaç kare çizilebileceği (render kapasitesi).
    pub fn capacity_fps(&self) -> u32 {
        if self.gfx_mpix == 0 {
            return 0;
        }
        // gfx_mpix (Mpix/s) * 1e6 / 921_600 piksel
        (u64::from(self.gfx_mpix) * 1_000_000 / FRAME_PIXELS) as u32
    }
}

/// "Birim / mikrosaniye" -> "birim / saniye / 1e6" dönüşümü.
fn rate(units: u64, elapsed_us: u64) -> u32 {
    if elapsed_us == 0 {
        return 0;
    }
    (units / elapsed_us).min(u64::from(u32::MAX)) as u32
}

// ---------------------------------------------------------------------------
// Puanlama
// ---------------------------------------------------------------------------

/// Ölçek çapaları: `lo` = 0 puan, `mid` = 70 puan, `hi` = 100 puan.
///
/// Aradaki değerler logaritmik (log2) interpolasyonla doldurulur; böylece
/// "bir katına çıkan hız" her metrikte kabaca aynı puan artışına denk gelir.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Anchors {
    pub lo: u32,
    pub mid: u32,
    pub hi: u32,
}

impl Anchors {
    /// Değeri 0..100 puana eşler.
    pub fn score(&self, value: u32) -> u32 {
        let v = u64::from(value.max(1));
        let lo = u64::from(self.lo.max(1));
        let mid = u64::from(self.mid.max(self.lo.saturating_add(1)));
        let hi = u64::from(self.hi.max(self.mid.saturating_add(1)));
        if v <= lo {
            return 0;
        }
        if v >= hi {
            return 100;
        }
        let lv = log2_q10(v);
        if v <= mid {
            let a = log2_q10(lo);
            let b = log2_q10(mid);
            let span = (b - a).max(1);
            (((lv - a) * 70) / span).clamp(0, 70) as u32
        } else {
            let a = log2_q10(mid);
            let b = log2_q10(hi);
            let span = (b - a).max(1);
            (70 + ((lv - a) * 30) / span).clamp(70, 100) as u32
        }
    }
}

// --- Puanlama çapaları -----------------------------------------------------
//
// Referanslar (ölçüm birimiyle):
//
// * `lo`  : sistemin "yetersiz" sayıldığı alt sınır.
// * `mid` : "kabul edilebilir" seviye — yazılım emülasyonlu (TCG) tek
//           çekirdekli sanal makine sınıfı (70 puan).
// * `hi`  : modern masaüstü sınıfı (100 puan).
//
// Referans değerler `tools/perfhost.rs` (gerçek PC) ve QEMU TCG ölçümleri
// ile kalibre edilmiştir.

/// CPU hızı çapaları (Mops/s).
pub const CPU_ANCHORS: Anchors = Anchors {
    lo: 20,
    mid: 150,
    hi: 1_500,
};

/// Bellek bant genişliği çapaları (MB/s).
pub const MEM_ANCHORS: Anchors = Anchors {
    lo: 40,
    mid: 400,
    hi: 6_000,
};

/// Grafik dolgu hızı çapaları (Mpix/s).
pub const GFX_ANCHORS: Anchors = Anchors {
    lo: 10,
    mid: 60,
    hi: 800,
};

/// Gözlenen kare hızı çapaları (FPS): 15 = kullanılamaz, 30 = akıcı masaüstü,
/// 60 = akıcı.
pub const FPS_ANCHORS: Anchors = Anchors {
    lo: 15,
    mid: 30,
    hi: 60,
};

/// Ağırlıklar (toplam 100).
pub const WEIGHT_CPU: u32 = 30;
pub const WEIGHT_MEM: u32 = 25;
pub const WEIGHT_GFX: u32 = 25;
pub const WEIGHT_PACING: u32 = 20;

/// Bu hızın altında kalan sistem "kullanılamaz" kabul edilir; puan tavanlanır.
pub const MIN_USABLE_FPS: u32 = 15;
/// Kullanılamaz sayılan sistemler için puan tavanı.
pub const UNUSABLE_SCORE_CAP: u32 = 45;

/// Alt bileşen puanları ve toplam.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct PerfScores {
    pub cpu: u32,
    pub mem: u32,
    pub gfx: u32,
    pub pacing: u32,
    pub total: u32,
}

/// Kare teslimi puanı: kararlılık (sapma) ve gözlenen hızın ağırlıklı ortalaması.
///
/// `jitter_us` kare süresi standart sapmasıdır (mikrosaniye).
pub fn pacing_score(delivered_fps: u32, jitter_us: u32) -> u32 {
    // Kararlılık: 1 ms sapma 10 puan, 6 ms ve üzeri 60 puan düşürür.
    let stability = 100u32.saturating_sub((jitter_us / 100).min(60));
    let rate = FPS_ANCHORS.score(delivered_fps);
    stability * 60 / 100 + rate * 40 / 100
}

/// Hız ve kare ölçümlerinden toplam puanı hesaplar.
pub fn score(rates: PerfRates, delivered_fps: u32, jitter_us: u32) -> PerfScores {
    let cpu = CPU_ANCHORS.score(rates.cpu_mops);
    let mem = MEM_ANCHORS.score(rates.mem_mbps);
    let gfx = GFX_ANCHORS.score(rates.gfx_mpix);
    let pacing = pacing_score(delivered_fps, jitter_us);
    let mut total =
        (cpu * WEIGHT_CPU + mem * WEIGHT_MEM + gfx * WEIGHT_GFX + pacing * WEIGHT_PACING) / 100;
    if delivered_fps < MIN_USABLE_FPS {
        total = total.min(UNUSABLE_SCORE_CAP);
    }
    PerfScores {
        cpu,
        mem,
        gfx,
        pacing,
        total,
    }
}

/// Puanı derece etiketine çevirir (arayüz dili ayrıca seçilir).
pub fn grade(score: u32) -> Grade {
    match score {
        90..=u32::MAX => Grade::Excellent,
        75..=89 => Grade::VeryGood,
        55..=74 => Grade::Good,
        35..=54 => Grade::Fair,
        _ => Grade::Poor,
    }
}

/// Puan dereceleri.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Excellent,
    VeryGood,
    Good,
    Fair,
    Poor,
}

// ---------------------------------------------------------------------------
// Sabit nokta log2
// ---------------------------------------------------------------------------

/// Tam sayı karekök (aşağı yuvarlama).
pub fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = 1u64 << ((63 - n.leading_zeros()) / 2 + 1);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            break;
        }
        x = y;
    }
    x
}

/// `log2(x) * 1024` (x >= 1). Seri açılımıyla ~0.005 hata.
pub fn log2_q10(x: u64) -> i64 {
    if x == 0 {
        return i64::MIN / 4;
    }
    let msb = 63 - x.leading_zeros() as i64;
    // Mantisi [1,2) aralığına Q10 ölçeğinde indir.
    let mant = (((x as u128) << 10) >> msb) as u64;
    let u = mant - 1024;
    // log2(1+u) = 2/ln2 * (t + t^3/3 + ...), t = u / (2 + u)
    let t = (u * 1024 / (2048 + u)) as i64;
    let t3 = ((t as u64).saturating_mul(t as u64).saturating_mul(t as u64)) >> 20;
    let sum = t + (t3 as i64) / 3;
    // 2/ln2 = 2.88539...
    let frac = (sum * 28854) / 10000;
    msb * 1024 + frac
}
