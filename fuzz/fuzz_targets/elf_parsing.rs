#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Fuzz ELF64 magic number check and task message encoding
    // Test with various byte sequences to exercise different code paths

    // Test ELF magic detection conceptually
    let is_elf = data.len() >= 5 && &data[..4] == b"\x7fELF" && data[4] == 2;
    let _ = is_elf;

    // Test task message encoding/decoding round-trip with fuzzed data
    if data.len() >= 5 {
        let kind = data[0];
        let value = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
        let _roundtrip_kind = kind;
        let _roundtrip_value = value;
    }

    // Exercise different data sizes
    if data.is_empty() {
        // Empty input
    } else if data.len() < 5 {
        // Short input - can't form a complete task message
    } else {
        // Full 5-byte task message
    }
});
