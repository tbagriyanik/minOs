use minos_compress::compress;
use minos_compress::decompress;

fn main() {
    let mut pattern = [0u8; 4096];
    for (i, b) in pattern.iter_mut().enumerate() {
        *b = b'a' + (i % 4) as u8;
    }

    let mut packed = [0u8; 4096];
    let len = compress(&pattern, &mut packed).expect("compress");
    println!("compressed len = {}", len);

    let mut restored = [0u8; 4096];
    let opened = decompress(&packed[..len], &mut restored).expect("decompress");
    println!("decompressed len = {}", opened);

    let mut mismatches = 0;
    for i in 0..pattern.len() {
        if restored[i] != pattern[i] {
            if mismatches < 16 {
                println!("mismatch at {}: expected {} got {}", i, pattern[i], restored[i]);
            }
            mismatches += 1;
        }
    }
    println!("total mismatches = {}", mismatches);
}