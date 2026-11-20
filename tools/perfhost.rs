// File: tools/perfhost.rs
// minOs performans testi — host (gerçek masaüstü) referans ölçüm aracı.
//
// Kontrol panelindeki performans testi `src/apps/perfbench.rs` içindeki
// iş yüklerini kullanır. Bu araç **aynı** dosyayı doğrudan derleyerek gerçek
// bir PC'de aynı iş yüklerini çalıştırır ve puanlama çapalarının kalibrasyonu
// için referans değerleri üretir.
//
// Kullanım:
//     rustc -O tools/perfhost.rs -o target/perfhost.exe
//     target/perfhost.exe
//
// Çıktı satırı, QEMU'daki seri çıktısıyla aynı alanları içerir:
//     PERF-HOST cpu_mops=... mem_mbps=... gfx_mpix=... cap_fps=... score=...

#[path = "../src/apps/perfbench.rs"]
mod perfbench;

use std::time::Instant;

fn main() {
    // Isınma (ilk çağrıda sayfa hatası / önbellek soğukluğu olmasın).
    {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        perfbench::cpu_workload(&mut state, 50_000);
        let mut buf = vec![0u32; perfbench::MEM_BYTES / 4];
        perfbench::mem_workload(&mut buf, 1);
        let mut gbuf = vec![0u32; perfbench::GFX_PIXELS];
        perfbench::gfx_workload(&mut gbuf, 1);
    }

    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let t0 = Instant::now();
    let cpu_sum = perfbench::cpu_workload(&mut state, perfbench::CPU_ITERATIONS);
    let cpu_us = t0.elapsed().as_micros() as u64;

    let mut mem_buf = vec![0u32; perfbench::MEM_BYTES / 4];
    let t1 = Instant::now();
    let mem_sum = perfbench::mem_workload(&mut mem_buf, perfbench::MEM_PASSES);
    let mem_us = t1.elapsed().as_micros() as u64;

    let mut gfx_buf = vec![0u32; perfbench::GFX_PIXELS];
    let t2 = Instant::now();
    let gfx_sum = perfbench::gfx_workload(&mut gfx_buf, perfbench::GFX_PASSES);
    let gfx_us = t2.elapsed().as_micros() as u64;

    let rates = perfbench::PerfRates {
        cpu_mops: perfbench::PerfRates::cpu_mops(perfbench::CPU_ITERATIONS, cpu_us),
        mem_mbps: perfbench::PerfRates::mem_mbps(
            (perfbench::MEM_BYTES as u64) * u64::from(perfbench::MEM_PASSES),
            mem_us,
        ),
        gfx_mpix: perfbench::PerfRates::gfx_mpix(
            (perfbench::GFX_PIXELS as u64) * u64::from(perfbench::GFX_PASSES),
            gfx_us,
        ),
    };

    // Puanlama çapalarının doğruluğunu görmek için "ideal" bir kare teslimi
    // varsayılır (60 FPS, 1 ms sapma).
    let scores = perfbench::score(rates, 60, 1_000);

    println!(
        "PERF-HOST cpu_mops={} mem_mbps={} gfx_mpix={} cap_fps={} score={} (cpu={} mem={} gfx={} pacing={})",
        rates.cpu_mops,
        rates.mem_mbps,
        rates.gfx_mpix,
        rates.capacity_fps(),
        scores.total,
        scores.cpu,
        scores.mem,
        scores.gfx,
        scores.pacing
    );
    println!(
        "PERF-HOST-RAW cpu_us={} mem_us={} gfx_us={} sums={}/{}/{}",
        cpu_us, mem_us, gfx_us, cpu_sum, mem_sum, gfx_sum
    );
}
