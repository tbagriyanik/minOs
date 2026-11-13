//! Brotli (RFC 7932) çözücüsü — `no_std` + `alloc`.
//!
//! `brotli-decompressor` crate'i `default-features = false` ile derlendiğinde
//! `std` kullanmaz ve tüm bellek tahsislerini `alloc-no-stdlib` üzerinden
//! yapar. Çekirdekte `std` bulunmadığı için bu modül, `Vec<T>` tabanlı basit bir
//! `Allocator` uygulaması sağlar ve `BrotliDecompressStream` API'sini büyüyen
//! bir çıktı tamponuyla sürer.

use alloc::vec;
use alloc::vec::Vec;
use core::ops::{Index, IndexMut, Range};

use brotli_decompressor::{
    Allocator, BrotliDecompressStream, BrotliResult, BrotliState, HuffmanCode, SliceWrapper,
    SliceWrapperMut,
};

/// `alloc::vec::Vec` üzerinde `alloc-no-stdlib::Allocator` uygulaması.
///
/// `brotli-decompressor` crate'i `Default` ile boş bir tahsis bekler; `Vec`
/// tabanlı bu tür, `free_cell` çağrısında `Vec`'i düşürerek belleği serbest
/// bırakır (Rust'ın kendi allocator'ına devredilir).
#[derive(Debug)]
pub struct VecAlloc<T: Clone + Default>(Vec<T>);

impl<T: Clone + Default> Default for VecAlloc<T> {
    fn default() -> Self {
        VecAlloc(Vec::new())
    }
}

impl<T: Clone + Default> SliceWrapper<T> for VecAlloc<T> {
    fn slice(&self) -> &[T] {
        &self.0
    }
}

impl<T: Clone + Default> SliceWrapperMut<T> for VecAlloc<T> {
    fn slice_mut(&mut self) -> &mut [T] {
        &mut self.0
    }
}

impl<T: Clone + Default> Index<usize> for VecAlloc<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        &self.0[index]
    }
}

impl<T: Clone + Default> IndexMut<usize> for VecAlloc<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.0[index]
    }
}

impl<T: Clone + Default> Index<Range<usize>> for VecAlloc<T> {
    type Output = [T];
    fn index(&self, index: Range<usize>) -> &[T] {
        &self.0[index]
    }
}

impl<T: Clone + Default> IndexMut<Range<usize>> for VecAlloc<T> {
    fn index_mut(&mut self, index: Range<usize>) -> &mut [T] {
        &mut self.0[index]
    }
}

impl<T: Clone + Default> Allocator<T> for VecAlloc<T> {
    type AllocatedMemory = Self;

    fn alloc_cell(&mut self, len: usize) -> Self {
        VecAlloc(vec![T::default(); len])
    }

    fn free_cell(&mut self, _data: Self) {}
}

/// Tek seferde üretilecek en fazla çıktı miktarı. Brotli akışları keyfi
/// boyutta olabildiği için çıktı tamponu `NeedsMoreOutput` döndükçe büyütülür.
const OUTPUT_CHUNK: usize = 16 * 1024;

/// Brotli (RFC 7932) sıkıştırılmış veriyi çözer.
///
/// Bozuk/eksik akışlarda `None` döner. Çıktı boyutu `usize` sınırına ulaşırsa
/// veya bellek tahsisi başarısız olursa da `None` döner.
pub fn decompress(input: &[u8]) -> Option<Vec<u8>> {
    if input.is_empty() {
        return Some(Vec::new());
    }

    let mut state = BrotliState::new(
        VecAlloc::default(),
        VecAlloc::default(),
        VecAlloc::default(),
    );

    let mut available_in = input.len();
    let mut input_offset = 0usize;
    let mut output: Vec<u8> = Vec::new();

    loop {
        let start = output.len();
        let new_len = start.checked_add(OUTPUT_CHUNK)?;
        output.resize(new_len, 0);

        let mut available_out = OUTPUT_CHUNK;
        let mut output_offset = 0usize;
        let mut total_out = 0usize;

        let result = BrotliDecompressStream(
            &mut available_in,
            &mut input_offset,
            input,
            &mut available_out,
            &mut output_offset,
            &mut output[start..],
            &mut total_out,
            &mut state,
        );

        // `output_offset` bu çağrıda yazılan bayt sayısıdır.
        output.truncate(start + output_offset);

        match result {
            BrotliResult::ResultSuccess => return Some(output),
            // Girdinin tamamı tüketildi ama akış bitmedi: eksik/bozuk veri.
            BrotliResult::NeedsMoreInput => return None,
            BrotliResult::ResultFailure => return None,
            BrotliResult::NeedsMoreOutput => {
                // Tampon doldu; bir sonraki turda büyütülmüş çıktıya devam et.
                continue;
            }
        }
    }
}

/// `HuffmanCode` türünün `Default` olması, `VecAlloc<HuffmanCode>` için
/// gereklidir; derleme zamanında doğrulanır.
const _: fn() = || {
    fn assert_default<T: Default>() {}
    assert_default::<HuffmanCode>();
};

#[cfg(test)]
mod tests {
    use super::*;

    /// "hello world" metninin brotli (quality 11, window 22) çıktısı.
    /// `printf 'hello world' | brotli -q 11` ile üretilmiştir.
    const HELLO_WORLD_BR: &[u8] = &[
        0x0b, 0x05, 0x80, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x77, 0x6f, 0x72, 0x6c, 0x64, 0x03,
    ];

    #[test]
    fn test_brotli_roundtrip_hello_world() {
        assert_eq!(
            decompress(HELLO_WORLD_BR).as_deref(),
            Some(&b"hello world"[..])
        );
    }

    #[test]
    fn test_brotli_empty_input() {
        assert_eq!(decompress(&[]).as_deref(), Some(&b""[..]));
    }

    #[test]
    fn test_brotli_corrupt_input_rejected() {
        assert_eq!(decompress(&[0x00]), None);
        assert_eq!(decompress(&[0xff, 0xff, 0xff, 0xff]), None);
    }
}
