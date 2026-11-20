// Host birim testleri: performans ölçüm ve puanlama çekirdeği.
//
// `src/apps/perfbench.rs` saf Rust'tır (çekirdek bağımlılığı yoktur); hem
// çekirdekte (kontrol paneli performans testi) hem host'ta çalışır.

#[path = "../src/apps/perfbench.rs"]
pub mod perfbench;

#[cfg(test)]
mod tests {
    use super::perfbench::*;

    #[test]
    fn log2_q10_known_values() {
        assert_eq!(log2_q10(1), 0);
        assert_eq!(log2_q10(1024), 10 * 1024);
        assert_eq!(log2_q10(1 << 20), 20 * 1024);
        let v = log2_q10(1536);
        assert!((v - (10 * 1024 + 599)).abs() <= 6, "log2_q10(1536) = {v}");
        let v = log2_q10(10);
        assert!((v - 3401).abs() <= 6, "log2_q10(10) = {v}");
    }

    #[test]
    fn isqrt_matches_known_values() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(1_000_000), 1_000);
        assert_eq!(isqrt(999_999), 999);
        assert_eq!(isqrt(1 << 40), 1 << 20);
    }

    #[test]
    fn anchors_map_endpoints_and_are_monotonic() {
        let a = Anchors {
            lo: 10,
            mid: 100,
            hi: 1000,
        };
        assert_eq!(a.score(5), 0);
        assert_eq!(a.score(10), 0);
        assert_eq!(a.score(100), 70);
        assert_eq!(a.score(1000), 100);
        assert_eq!(a.score(5000), 100);
        let mut prev = 0;
        for v in [12, 20, 40, 80, 100, 200, 400, 800, 1000] {
            let s = a.score(v);
            assert!(s >= prev, "v={v} score={s} < {prev}");
            prev = s;
        }
    }

    #[test]
    fn rate_conversion_is_microsecond_based() {
        assert_eq!(PerfRates::cpu_mops(200_000, 100), 2_000);
        assert_eq!(PerfRates::mem_mbps(8 * 1_048_576, 1_000), 8_388);
        assert_eq!(PerfRates::gfx_mpix(1_000_000, 1_000), 1_000);
        assert_eq!(PerfRates::cpu_mops(200_000, 0), 0);
    }

    #[test]
    fn capacity_fps_uses_720p_frame() {
        let rates = PerfRates {
            cpu_mops: 0,
            mem_mbps: 0,
            gfx_mpix: 921,
        };
        assert_eq!(rates.capacity_fps(), 999);
    }

    #[test]
    fn score_is_bounded_and_weighted() {
        let rates = PerfRates {
            cpu_mops: 1_000,
            mem_mbps: 3_000,
            gfx_mpix: 400,
        };
        let s = score(rates, 60, 500);
        assert!(s.total >= 70 && s.total <= 100, "total={}", s.total);
        assert!(s.cpu >= 70 && s.mem >= 70 && s.gfx >= 70);
        let slow = PerfRates {
            cpu_mops: 5,
            mem_mbps: 10,
            gfx_mpix: 2,
        };
        assert!(score(slow, 60, 0).total < 30);
    }

    #[test]
    fn unusable_frame_rate_caps_total() {
        let rates = PerfRates {
            cpu_mops: 5_000,
            mem_mbps: 20_000,
            gfx_mpix: 5_000,
        };
        let s = score(rates, 5, 100);
        assert!(s.total <= UNUSABLE_SCORE_CAP, "total={}", s.total);
    }

    #[test]
    fn workloads_are_deterministic_and_non_trivial() {
        let mut s1 = 0x1234_5678_9ABC_DEF0u64;
        let mut s2 = 0x1234_5678_9ABC_DEF0u64;
        let a = cpu_workload(&mut s1, 1_000);
        let b = cpu_workload(&mut s2, 1_000);
        assert_eq!(a, b);
        assert_ne!(a, 0);
        assert_eq!(s1, s2);

        let mut buf = vec![7u32; 1_024];
        let checksum = mem_workload(&mut buf, 2);
        assert_ne!(checksum, 0);
        // Tampon işlendiği için değerleri değişmiş olmalı.
        assert!(buf.iter().any(|&v| v != 7));

        let mut gbuf = vec![0u32; 1_024];
        let checksum = gfx_workload(&mut gbuf, 1);
        assert_ne!(checksum, 0);
        assert!(gbuf.iter().any(|&v| v != 0));
    }
}
