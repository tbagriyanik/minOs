//! MZRAM compress/decompress round-trip host testleri.
//!
//! `src/mm/memory/compress.rs` modülü `crate::boot::console::serial_print` kullanır;
//! host harness bu fonksiyonları sağladığı için modül aynen bağlanır.

use crate::mzram_compress::{
    compress, decompress, retrieve_page, store_page, CompressError, MAX_COMPRESSED_BUFFER,
};

const PAGE: usize = 4096;

#[test]
fn zero_page_roundtrips_tiny() {
    let zeros = [0u8; PAGE];
    let mut packed = [0u8; MAX_COMPRESSED_BUFFER];
    let len = compress(&zeros, &mut packed).expect("compress");
    assert!(len < 64, "zero page compressed too large: {}", len);
    let mut restored = [0u8; PAGE];
    let opened = decompress(&packed[..len], &mut restored).expect("decompress");
    assert_eq!(opened, PAGE);
    assert_eq!(&restored[..opened], &zeros[..opened]);
}

#[test]
fn repeating_pattern_roundtrips() {
    let mut pattern = [0u8; PAGE];
    for (i, b) in pattern.iter_mut().enumerate() {
        *b = b'a' + (i % 4) as u8;
    }
    let mut packed = [0u8; MAX_COMPRESSED_BUFFER];
    let len = compress(&pattern, &mut packed).expect("compress");
    assert!(
        len < PAGE,
        "compressed len {} not smaller than {}",
        len,
        PAGE
    );
    let mut restored = [0u8; PAGE];
    let opened = decompress(&packed[..len], &mut restored).expect("decompress");
    assert_eq!(opened, PAGE);
    assert_eq!(
        &restored[..opened],
        &pattern[..opened],
        "round-trip mismatch"
    );
}

#[test]
fn mixed_page_roundtrips() {
    let mut data = [0u8; PAGE];
    for (i, b) in data.iter_mut().enumerate() {
        let block = i / 64;
        *b = match block % 4 {
            0 => 0,
            1 => (i % 7) as u8,
            2 => 0x5A,
            _ => (i * 31 % 251) as u8,
        };
    }
    let mut packed = [0u8; MAX_COMPRESSED_BUFFER];
    let len = compress(&data, &mut packed).expect("compress");
    let mut restored = [0u8; PAGE];
    let opened = decompress(&packed[..len], &mut restored).expect("decompress");
    assert_eq!(opened, PAGE);
    assert_eq!(
        &restored[..opened],
        &data[..opened],
        "mixed round-trip mismatch"
    );
}

#[test]
fn noise_is_rejected() {
    let mut noise = [0u8; PAGE];
    let mut seed: u64 = 0x1234_5678_9ABC_DEF0;
    for b in noise.iter_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *b = seed as u8;
    }
    let mut out = [0u8; MAX_COMPRESSED_BUFFER];
    assert_eq!(compress(&noise, &mut out), Err(CompressError::NoGain));
}

#[test]
fn corrupt_stream_detected() {
    let stream = [0xC0u8, 0x00, 0x00];
    let mut dst = [0u8; 16];
    assert_eq!(decompress(&stream, &mut dst), Err(CompressError::Corrupt));
}

#[test]
fn output_buffer_bounds_checked() {
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

#[test]
fn store_and_retrieve_roundtrip() {
    let mut pattern = [0u8; PAGE];
    for (i, b) in pattern.iter_mut().enumerate() {
        *b = b'a' + (i % 4) as u8;
    }
    let stored = store_page(&pattern).expect("store_page");
    assert!(stored > 0);
    let mut fetched = [0u8; PAGE];
    let fetched_len = retrieve_page(&mut fetched).expect("retrieve_page");
    assert_eq!(fetched_len, PAGE);
    assert_eq!(&fetched[..fetched_len], &pattern[..fetched_len]);
}
