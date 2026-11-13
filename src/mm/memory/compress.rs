//! Bellek sıkıştırma (depo dışı / swap'sız yaklaşım — zswap benzeri).
//!
//! Gerçek donanımda disk takası (swap) yoktur; belleğe sıkışma baskısı
//! geldiğinde sayfalar diske yazılmak yerine **sıkıştırılmış** biçimde
//! tutulur. Bu modül iki katman sunar:
//!
//! 1. **Bellek içi sıkıştırma** (`compress` / `decompress`): `no_std`,
//!    ayırıcı kullanmayan, sabit tampona yazan LZ77 türü bir kodlayıcı.
//!    - Sıfır sayfaları 1 baytlık çalıştırma (RLE) ile temsil edilir.
//!    - 3–258 baytlık eşleşmeler 16 bit geri mesafe + uzunluk ile kodlanır.
//!    - Çıktı sıkıştırmadan büyükse `None` döner (çağıran ham yolu seçer).
//! 2. **Sıkıştırılmış sayfa havuzu** (`store_page` / `retrieve_page`):
//!    sayfa başına kayıt tutan, dolduğunda en eski (FIFO/LRU yaklaşımlı)
//!    kaydı çıkaran sabit kapasiteli depo. Sıkıştırma sonucu ham sayfadan
//!    küçük değilse sayfa **depoya alınmaz** (kazanç yoksa maliyet yok).
//!
//! Belirteç biçimi (baytlar):
//!
//! | Belirteç | Anlam |
//! | :--- | :--- |
//! | `0x00 ..= 0x7F` | Sonraki `n + 1` bayt birebir (literal) |
//! | `0x80 ..= 0xBF` | `value = token & 0x3F`; sonraki 1 bayt sayı `n` ise `n + 3` adet `value` baytı (RLE) |
//! | `0xC0 ..= 0xFF` | `len = (token & 0x3F) + 3`; sonraki 2 bayt LE geri mesafe; bu uzunlukta kopya |
//!
//! Bu biçim deterministiktir ve host testleri (saf Rust) ile doğrulanır.

use crate::arch::x86_64::sync::SpinMutex;

/// Sıkıştırılmış sayfa havuzu kapasitesi (kayıt sayısı).
pub const MAX_STORED_PAGES: usize = 64;
/// Kayıt başına azami sıkıştırılmış boy (4 KiB sayfa sınırı).
pub const MAX_COMPRESSED_BYTES: usize = 4096;
/// Eşleşme aramasında taranacak azami geri mesafe.
const MAX_DISTANCE: usize = 4096;
/// Azami eşleşme uzunluğu (belirteç 6 bit uzunluk taşır: 3..=66).
const MAX_MATCH: usize = 66;
/// Azami literal çalıştırma uzunluğu.
const MAX_LITERAL: usize = 128;
/// Azami tek RLE çalıştırma uzunluğu (1 baytlık sayı + 3).
const MAX_RLE: usize = 258;

/// Sıkıştırma hatası.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressError {
    /// Çıktı tamponu yetersiz.
    OutputTooSmall,
    /// Bozuk/kesilmiş belirteç akışı.
    Corrupt,
    /// Sıkıştırma kazanç sağlamadı (çıktı ≥ girdi).
    NoGain,
}

/// `src` baytlarını `dst` tamponuna sıkıştırır; sıkıştırılmış uzunluğu döner.
///
/// Sıkıştırma kazancı yoksa `Err(NoGain)` döner.
pub fn compress(src: &[u8], dst: &mut [u8]) -> Result<usize, CompressError> {
    if src.is_empty() {
        return Ok(0);
    }
    let mut out = 0usize;
    let mut position = 0usize;
    let mut literal_start = 0usize;
    let mut literal_len = 0usize;

    // Bekleyen literal çalıştırmayı boşaltır (128 baytlık parçalar hâlinde).
    let flush_literals = |dst: &mut [u8], out: &mut usize, start: usize, len: usize| {
        let mut emitted = 0usize;
        while emitted < len {
            let chunk = (len - emitted).min(MAX_LITERAL);
            if *out + 1 + chunk > dst.len() {
                return Err(CompressError::OutputTooSmall);
            }
            dst[*out] = (chunk - 1) as u8;
            *out += 1;
            dst[*out..*out + chunk].copy_from_slice(&src[start + emitted..start + emitted + chunk]);
            *out += chunk;
            emitted += chunk;
        }
        Ok(())
    };

    while position < src.len() {
        // Sıfır çalıştırması: RLE belirteci literal akışından daha verimli.
        if src[position] == 0 {
            let mut run = 1usize;
            while position + run < src.len() && src[position + run] == 0 && run < MAX_RLE {
                run += 1;
            }
            if run >= 3 {
                if literal_len > 0 {
                    flush_literals(dst, &mut out, literal_start, literal_len)?;
                    literal_len = 0;
                }
                if out + 2 > dst.len() {
                    return Err(CompressError::OutputTooSmall);
                }
                dst[out] = 0x80;
                out += 1;
                dst[out] = (run - 3) as u8;
                out += 1;
                position += run;
                literal_start = position;
                continue;
            }
        }

        // En iyi eşleşmeyi ara. Pencerede yüksek maliyetli aramayı sınırlamak
        // için yalnız son `MAX_DISTANCE` bayta bakılır.
        let mut best_len = 0usize;
        let mut best_distance = 0usize;
        if position >= 1 {
            let window_start = position.saturating_sub(MAX_DISTANCE);
            let max_len = MAX_MATCH.min(src.len() - position);
            let mut candidate = window_start;
            while candidate < position {
                if src[candidate] == src[position] {
                    let mut len = 1usize;
                    while len < max_len && src[candidate + len] == src[position + len] {
                        len += 1;
                    }
                    if len > best_len {
                        best_len = len;
                        best_distance = position - candidate;
                    }
                }
                candidate += 1;
            }
        }

        if best_len >= 3 {
            if literal_len > 0 {
                flush_literals(dst, &mut out, literal_start, literal_len)?;
                literal_len = 0;
            }
            if out + 3 > dst.len() {
                return Err(CompressError::OutputTooSmall);
            }
            dst[out] = 0xC0 | ((best_len - 3) as u8 & 0x3F);
            out += 1;
            let distance = best_distance as u16;
            dst[out..out + 2].copy_from_slice(&distance.to_le_bytes());
            out += 2;
            position += best_len;
            literal_start = position;
            continue;
        }

        // Literal biriktir.
        if literal_len == 0 {
            literal_start = position;
        }
        literal_len += 1;
        position += 1;
    }

    if literal_len > 0 {
        flush_literals(dst, &mut out, literal_start, literal_len)?;
    }
    if out >= src.len() {
        return Err(CompressError::NoGain);
    }
    Ok(out)
}

/// `src` belirteç akışını `dst` tamponuna açar; açılan uzunluğu döner.
pub fn decompress(src: &[u8], dst: &mut [u8]) -> Result<usize, CompressError> {
    let mut input = 0usize;
    let mut out = 0usize;
    while input < src.len() {
        let token = src[input];
        input += 1;
        match token {
            0x00..=0x7F => {
                let len = token as usize + 1;
                if input + len > src.len() || out + len > dst.len() {
                    return Err(CompressError::Corrupt);
                }
                dst[out..out + len].copy_from_slice(&src[input..input + len]);
                input += len;
                out += len;
            }
            0x80..=0xBF => {
                let value = token & 0x3F;
                if input >= src.len() {
                    return Err(CompressError::Corrupt);
                }
                let len = src[input] as usize + 3;
                input += 1;
                if out + len > dst.len() {
                    return Err(CompressError::OutputTooSmall);
                }
                dst[out..out + len].fill(value);
                out += len;
            }
            _ => {
                let len = (token & 0x3F) as usize + 3;
                if input + 2 > src.len() {
                    return Err(CompressError::Corrupt);
                }
                let distance = u16::from_le_bytes([src[input], src[input + 1]]) as usize;
                input += 2;
                if distance == 0 || distance > out || out + len > dst.len() {
                    return Err(CompressError::Corrupt);
                }
                // Kopyalama örtüşebilir (RLE benzeri tekrar); bayt bayt ilerle.
                for _ in 0..len {
                    dst[out] = dst[out - distance];
                    out += 1;
                }
            }
        }
    }
    Ok(out)
}

/// Sıkıştırılmış depo istatistikleri.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompressionStats {
    /// Depoda tutulan sayfa sayısı.
    pub pages: u64,
    /// Depodaki sıkıştırılmış bayt toplamı.
    pub stored_bytes: u64,
    /// Depodaki sayfaların ham bayt toplamı.
    pub raw_bytes: u64,
    /// Tamamen sıfır olup depoya alınmayan sayfa sayısı.
    pub zero_pages: u64,
    /// Kapasite nedeniyle en eski kaydın çıkarılma sayısı.
    pub evictions: u64,
    /// Depoya girmeyen (kazanç yok) sayfa sayısı.
    pub rejected: u64,
}

impl CompressionStats {
    /// Sıkıştırma oranı (yüzde; 100 = sıkışma yok).
    pub fn ratio_percent(&self) -> u32 {
        if self.raw_bytes == 0 {
            return 100;
        }
        ((self.stored_bytes * 100) / self.raw_bytes) as u32
    }
}

/// Tek bir depo kaydı.
#[derive(Clone, Copy)]
struct StoredPage {
    /// Sıkıştırılmış veri (kullanılan kısım `len` bayt).
    data: [u8; MAX_COMPRESSED_BYTES],
    len: usize,
    /// Açılan (ham) sayfa boyu.
    raw_len: usize,
    used: bool,
}

impl StoredPage {
    const fn empty() -> Self {
        Self {
            data: [0; MAX_COMPRESSED_BYTES],
            len: 0,
            raw_len: 0,
            used: false,
        }
    }
}

struct CompressedPool {
    pages: [StoredPage; MAX_STORED_PAGES],
    next_slot: usize,
    stored_bytes: u64,
    raw_bytes: u64,
    zero_pages: u64,
    evictions: u64,
    rejected: u64,
    count: u64,
}

impl CompressedPool {
    const fn new() -> Self {
        Self {
            pages: [StoredPage::empty(); MAX_STORED_PAGES],
            next_slot: 0,
            stored_bytes: 0,
            raw_bytes: 0,
            zero_pages: 0,
            evictions: 0,
            rejected: 0,
            count: 0,
        }
    }
}

static POOL: SpinMutex<CompressedPool> = SpinMutex::new(CompressedPool::new());

/// Bir sayfayı sıkıştırıp depoya ekler; sıkıştırılmış boyu döner.
///
/// * Tamamen sıfır sayfa depoya alınmaz (`Ok(0)`; `zero_pages` artar).
/// * Sıkıştırma kazanç sağlamazsa sayfa depoya alınmaz (`Err(NoGain)`).
pub fn store_page(raw: &[u8]) -> Result<usize, CompressError> {
    if raw.is_empty() || raw.len() > MAX_COMPRESSED_BYTES {
        return Err(CompressError::Corrupt);
    }
    if raw.iter().all(|byte| *byte == 0) {
        let mut pool = POOL.lock();
        pool.zero_pages += 1;
        return Ok(0);
    }
    let mut scratch = [0u8; MAX_COMPRESSED_BYTES];
    let compressed = match compress(raw, &mut scratch) {
        Ok(len) => len,
        Err(CompressError::NoGain) => {
            POOL.lock().rejected += 1;
            return Err(CompressError::NoGain);
        }
        Err(other) => return Err(other),
    };
    let mut pool = POOL.lock();
    let slot = pool.next_slot % MAX_STORED_PAGES;
    if pool.pages[slot].used {
        pool.stored_bytes = pool
            .stored_bytes
            .saturating_sub(pool.pages[slot].len as u64);
        pool.raw_bytes = pool
            .raw_bytes
            .saturating_sub(pool.pages[slot].raw_len as u64);
        pool.evictions += 1;
        pool.count = pool.count.saturating_sub(1);
    }
    let page = &mut pool.pages[slot];
    page.data[..compressed].copy_from_slice(&scratch[..compressed]);
    page.len = compressed;
    page.raw_len = raw.len();
    page.used = true;
    pool.next_slot = (slot + 1) % MAX_STORED_PAGES;
    pool.stored_bytes += compressed as u64;
    pool.raw_bytes += raw.len() as u64;
    pool.count += 1;
    Ok(compressed)
}

/// Depodaki en eski kaydı açar ve `dst` tamponuna yazar; açılan uzunluğu döner.
pub fn retrieve_page(dst: &mut [u8]) -> Result<usize, CompressError> {
    let mut pool = POOL.lock();
    let Some(slot) = pool.pages.iter().position(|page| page.used) else {
        return Err(CompressError::Corrupt);
    };
    let (len, raw_len) = (pool.pages[slot].len, pool.pages[slot].raw_len);
    let mut scratch = [0u8; MAX_COMPRESSED_BYTES];
    scratch[..len].copy_from_slice(&pool.pages[slot].data[..len]);
    pool.stored_bytes = pool.stored_bytes.saturating_sub(len as u64);
    pool.raw_bytes = pool.raw_bytes.saturating_sub(raw_len as u64);
    pool.pages[slot].used = false;
    pool.count = pool.count.saturating_sub(1);
    drop(pool);
    let opened = decompress(&scratch[..len], dst)?;
    Ok(opened)
}

/// Anlık sıkıştırma istatistikleri.
pub fn stats() -> CompressionStats {
    let pool = POOL.lock();
    CompressionStats {
        pages: pool.count,
        stored_bytes: pool.stored_bytes,
        raw_bytes: pool.raw_bytes,
        zero_pages: pool.zero_pages,
        evictions: pool.evictions,
        rejected: pool.rejected,
    }
}

/// Bellek baskısında çağrılır: sıkıştırılabilir sıfır sayfaları raporlar ve
/// sıkıştırma oranını seri konsola yazar. Gerçek sayfa seçimi (süreç RSS'i)
/// `profile` modülündeki aday taramasıyla yapılır.
pub fn pressure_report() {
    let stats = stats();
    crate::boot::console::serial_print("[MZRAM] pages=");
    crate::boot::console::serial_print_dec(stats.pages);
    crate::boot::console::serial_print(" raw_kb=");
    crate::boot::console::serial_print_dec(stats.raw_bytes / 1024);
    crate::boot::console::serial_print(" stored_kb=");
    crate::boot::console::serial_print_dec(stats.stored_bytes / 1024);
    crate::boot::console::serial_print(" ratio=%");
    crate::boot::console::serial_print_dec(stats.ratio_percent() as u64);
    crate::boot::console::serial_print(" zero=");
    crate::boot::console::serial_print_dec(stats.zero_pages);
    crate::boot::console::serial_print(" evict=");
    crate::boot::console::serial_print_dec(stats.evictions);
    crate::boot::console::serial_print("\n");
}

/// Boot doğrulama testi (`[MZRAM]` işareti).
pub fn self_test() -> bool {
    use crate::boot::console::{serial_print, serial_print_dec};

    // 1. Sıfır sayfa: RLE çalıştırması çok küçük bir çıktı üretmeli.
    let zeros = [0u8; 4096];
    let mut buffer = [0u8; 4096];
    let zero_len = match compress(&zeros, &mut buffer) {
        Ok(len) => len,
        Err(_) => {
            serial_print("[MZRAM] Sifir sayfa sikistirma: BASARISIZ\n");
            return false;
        }
    };
    let zero_ok = zero_len < 64;

    // 2. Tekrarlı desen: eşleşme kopyalama devreye girmeli.
    let mut pattern = [0u8; 4096];
    for (index, byte) in pattern.iter_mut().enumerate() {
        *byte = b'a' + (index % 4) as u8;
    }
    let pattern_len = match compress(&pattern, &mut buffer) {
        Ok(len) => len,
        Err(_) => {
            serial_print("[MZRAM] Desen sikistirma: BASARISIZ\n");
            return false;
        }
    };
    let mut restored = [0u8; 4096];
    let opened = decompress(&buffer[..pattern_len], &mut restored).unwrap_or(0);
    let pattern_ok = opened == pattern.len() && restored == pattern;

    // 3. Rastgele (sıkıştırılamaz) veri: `NoGain` ile reddedilmeli.
    let mut noise = [0u8; 4096];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    for byte in noise.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *byte = seed as u8;
    }
    let noise_rejected = matches!(compress(&noise, &mut buffer), Err(CompressError::NoGain));

    // 4. Depo: sıkıştırılabilir sayfa saklanır ve aynen geri açılır.
    let stored = store_page(&pattern).unwrap_or(0);
    let mut fetched = [0u8; 4096];
    let fetched_len = retrieve_page(&mut fetched).unwrap_or(0);
    let pool_ok = stored > 0 && fetched_len == pattern.len() && fetched == pattern;

    let ok = zero_ok && pattern_ok && noise_rejected && pool_ok;
    serial_print("[MZRAM] Sikistirma (sifir=");
    serial_print_dec(zero_len as u64);
    serial_print(" B, desen=");
    serial_print_dec(pattern_len as u64);
    serial_print(" B, geri acildi=");
    serial_print_dec(opened as u64);
    serial_print(" B)");
    serial_print(if ok { ": BASARILI\n" } else { ": BASARISIZ\n" });
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) -> usize {
        let mut packed = [0u8; MAX_COMPRESSED_BYTES];
        let len = compress(data, &mut packed).expect("sikistirma basarili olmali");
        assert!(len < data.len(), "cikti girdiden kucuk olmali");
        let mut opened = [0u8; MAX_COMPRESSED_BYTES];
        let opened_len = decompress(&packed[..len], &mut opened).expect("acilabilmeli");
        assert_eq!(opened_len, data.len());
        assert_eq!(&opened[..opened_len], data);
        len
    }

    #[test]
    fn zero_page_uses_rle() {
        let zeros = [0u8; 4096];
        let len = roundtrip(&zeros);
        assert!(len < 64, "sifir sayfa cok kucuk olmali: {len}");
    }

    #[test]
    fn repeating_pattern_roundtrips() {
        let mut pattern = [0u8; 4096];
        for (index, byte) in pattern.iter_mut().enumerate() {
            *byte = b'a' + (index % 4) as u8;
        }
        roundtrip(&pattern);
    }

    #[test]
    fn mixed_page_roundtrips() {
        let mut data = [0u8; 4096];
        for (index, byte) in data.iter_mut().enumerate() {
            let block = index / 64;
            *byte = match block % 4 {
                0 => 0,
                1 => (index % 7) as u8,
                2 => 0x5A,
                _ => (index * 31 % 251) as u8,
            };
        }
        roundtrip(&data);
    }

    #[test]
    fn already_compressed_payload_is_rejected() {
        let mut noise = [0u8; 4096];
        let mut seed: u64 = 0x1234_5678_9ABC_DEF0;
        for byte in noise.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *byte = seed as u8;
        }
        let mut out = [0u8; MAX_COMPRESSED_BYTES];
        assert_eq!(compress(&noise, &mut out), Err(CompressError::NoGain));
    }

    #[test]
    fn corrupt_stream_is_detected() {
        // Geri mesafe 0 olan esleme belirteci gecersizdir.
        let stream = [0xC0u8, 0x00, 0x00];
        let mut dst = [0u8; 16];
        assert_eq!(decompress(&stream, &mut dst), Err(CompressError::Corrupt));
    }

    #[test]
    fn output_buffer_is_bounds_checked() {
        let data = [7u8; 32];
        let mut small = [0u8; 4];
        assert!(compress(&data, &mut small).is_err());
    }

    #[test]
    fn empty_input_is_identity() {
        let mut out = [0u8; 8];
        assert_eq!(compress(&[], &mut out), Ok(0));
        assert_eq!(decompress(&[], &mut out), Ok(0));
    }
}
