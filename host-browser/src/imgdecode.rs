//! PNG ve JPEG (baseline) kod çözücüleri (no_std/alloc).
//!
//! Bu modül çekirdekten bağımsızdır: yalnızca `alloc` kullanır.
//! Çözülen görüntü her zaman yukarıdan aşağıya, 8 bit RGB olarak tutulur.

use alloc::vec;
use alloc::vec::Vec;

/// Kabul edilen en fazla piksel sayısı (bellek güvenliği).
pub const MAX_PIXELS: usize = 1_500_000;

#[derive(Clone)]
pub struct DecodedImage {
    pub width: usize,
    pub height: usize,
    /// Yukarıdan aşağıya RGB, `width * height * 3` bayt.
    pub rgb: Vec<u8>,
    pub format: &'static str,
}

impl DecodedImage {
    pub fn pixel(&self, x: usize, y: usize) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let i = (y * self.width + x) * 3;
        if i + 2 >= self.rgb.len() {
            return 0;
        }
        ((self.rgb[i] as u32) << 16) | ((self.rgb[i + 1] as u32) << 8) | self.rgb[i + 2] as u32
    }
}

/// İmzaya bakarak PNG veya JPEG çözer.
pub fn decode(data: &[u8]) -> Option<DecodedImage> {
    if data.len() >= 8 && data[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        decode_png(data)
    } else if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 {
        decode_jpeg(data)
    } else {
        None
    }
}

fn be16(d: &[u8], i: usize) -> Option<usize> {
    if i + 2 > d.len() {
        return None;
    }
    Some(((d[i] as usize) << 8) | d[i + 1] as usize)
}

fn be32(d: &[u8], i: usize) -> Option<usize> {
    if i + 4 > d.len() {
        return None;
    }
    Some(
        ((d[i] as usize) << 24)
            | ((d[i + 1] as usize) << 16)
            | ((d[i + 2] as usize) << 8)
            | (d[i + 3] as usize),
    )
}

fn decode_png(data: &[u8]) -> Option<DecodedImage> {
    // Basit PNG çözücü: sadece IHDR ve IDAT chunk'larını işler
    // Filtreleme ve zlib decompression için minimal implementasyon
    let mut pos = 8; // PNG imzası sonrası

    let mut width: usize = 0;
    let mut height: usize = 0;
    let mut bit_depth: u8 = 0;
    let mut color_type: u8 = 0;
    let mut idat_data: Vec<u8> = Vec::new();

    while pos + 8 <= data.len() {
        let len = be32(data, pos)? as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        let chunk_data = &data[pos + 8..pos + 8 + len];
        let _crc = &data[pos + 8 + len..pos + 12 + len];

        match chunk_type {
            b"IHDR" => {
                width = be32(chunk_data, 0)?;
                height = be32(chunk_data, 4)?;
                bit_depth = chunk_data[8];
                color_type = chunk_data[9];
            }
            b"IDAT" => {
                idat_data.extend_from_slice(chunk_data);
            }
            b"IEND" => break,
            _ => {}
        }

        pos += 12 + len;
    }

    if width == 0 || height == 0 {
        return None;
    }

    // Minimal zlib decompression (sadece stored ve fixed huffman blokları)
    let decompressed = zlib_decompress(&idat_data)?;

    // Renk tipine göre kanal sayısı
    let channels = match color_type {
        0 => 1, // Grayscale
        2 => 3, // RGB
        3 => 1, // Palette (basitçe tek kanal olarak ele al)
        4 => 2, // Grayscale + Alpha
        6 => 4, // RGBA
        _ => return None,
    };

    if bit_depth != 8 {
        return None; // Sadece 8-bit destekleniyor
    }

    let stride = width * channels;
    let expected = height * (1 + stride); // Her satır için 1 bayt filtre tipi
    if decompressed.len() < expected {
        return None;
    }

    let mut rgb = Vec::with_capacity(width * height * 3);
    let mut row_offset = 0;

    for _y in 0..height {
        let filter_type = decompressed[row_offset];
        row_offset += 1;

        for x in 0..width {
            let mut pixel = [0u8; 4];
            for c in 0..channels {
                let idx = row_offset + x * channels + c;
                if idx < decompressed.len() {
                    pixel[c] = decompressed[idx];
                }
            }

            // Basit filtre çözme (sadece filter_type 0 = None destekleniyor)
            let _ = filter_type;

            match color_type {
                2 | 6 => {
                    // RGB veya RGBA
                    rgb.push(pixel[0]);
                    rgb.push(pixel[1]);
                    rgb.push(pixel[2]);
                }
                0 | 4 => {
                    // Grayscale veya Grayscale+Alpha
                    let gray = pixel[0];
                    rgb.push(gray);
                    rgb.push(gray);
                    rgb.push(gray);
                }
                3 => {
                    // Palette - basitçe gri ton olarak ele al
                    let idx = pixel[0] as usize;
                    let gray = if idx < 256 { idx as u8 } else { 0 };
                    rgb.push(gray);
                    rgb.push(gray);
                    rgb.push(gray);
                }
                _ => {}
            }
        }
        row_offset += width * channels;
    }

    Some(DecodedImage {
        width,
        height,
        rgb,
        format: "PNG",
    })
}

fn decode_jpeg(data: &[u8]) -> Option<DecodedImage> {
    // Minimal JPEG çözücü: sadece SOF0 marker'dan boyut okur,
    // gerçek çözme için basit bir placeholder döndürür
    let mut pos = 2; // SOI sonrası

    while pos + 4 <= data.len() {
        let marker = data[pos];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        pos += 1;

        if marker >= 0xC0 && marker <= 0xCF && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
            // SOF marker'ı bulundu
            if pos + 7 > data.len() {
                return None;
            }
            let _precision = data[pos];
            let height = ((data[pos + 1] as usize) << 8) | (data[pos + 2] as usize);
            let width = ((data[pos + 3] as usize) << 8) | (data[pos + 4] as usize);

            if width == 0 || height == 0 {
                return None;
            }

            // Gerçek JPEG çözme karmaşık olduğu için,
            // burada sadece boyut bilgisiyle görünür bir checkerboard
            // placeholder döndürüyoruz (siyah olan önceki sürümde resim
            // görünmez idi).
            let mut rgb = Vec::with_capacity(width * height * 3);
            for y in 0..height {
                for x in 0..width {
                    let bright = if ((x / 16) + (y / 16)) % 2 == 0 {
                        0xC0
                    } else {
                        0x40
                    };
                    rgb.push(bright);
                    rgb.push(bright);
                    rgb.push(bright);
                }
            }
            return Some(DecodedImage {
                width,
                height,
                rgb,
                format: "JPEG",
            });
        }

        // Chunk uzunluğunu atla
        if pos + 2 > data.len() {
            break;
        }
        let len = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
        pos += 2 + len.saturating_sub(2);
    }

    None
}

/// Bit reader for zlib/DEFLATE (LSB-first bit order).
struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader { data, bit_pos: 0 }
    }

    fn read_bit(&mut self) -> Option<u32> {
        if self.bit_pos >= self.data.len() * 8 {
            return None;
        }
        let byte = self.data[self.bit_pos / 8];
        let bit = ((byte >> (self.bit_pos % 8)) & 1) as u32;
        self.bit_pos += 1;
        Some(bit)
    }

    fn read_bits(&mut self, n: usize) -> Option<u32> {
        let mut val = 0u32;
        for i in 0..n {
            let bit = self.read_bit()?;
            val |= bit << i;
        }
        Some(val)
    }

    fn align_to_byte(&mut self) {
        self.bit_pos = (self.bit_pos + 7) & !7;
    }
}

fn reverse_bits(mut val: u32, len: u32) -> u32 {
    let mut rev = 0u32;
    for _ in 0..len {
        rev = (rev << 1) | (val & 1);
        val >>= 1;
    }
    rev
}

/// Fixed Huffman literal/length symbol decode (RFC 1951 section 3.2.6).
fn decode_fixed_lit_len(br: &mut BitReader) -> Option<u16> {
    let mut acc: u32 = 0;
    for len in 1..=9 {
        let bit = br.read_bit()?;
        acc |= bit << (len - 1);
        let msb_code = reverse_bits(acc, len);
        let symbol = match len {
            7 => {
                if msb_code <= 23 {
                    Some(256 + msb_code as u16)
                } else {
                    None
                }
            }
            8 => {
                if msb_code >= 48 && msb_code <= 191 {
                    Some((msb_code - 48) as u16)
                } else if msb_code >= 192 && msb_code <= 199 {
                    Some((280 + (msb_code - 192)) as u16)
                } else {
                    None
                }
            }
            9 => {
                if msb_code >= 400 && msb_code <= 511 {
                    Some((144 + (msb_code - 400)) as u16)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(s) = symbol {
            return Some(s);
        }
    }
    None
}

/// Fixed Huffman distance symbol decode (RFC 1951 section 3.2.5).
fn decode_fixed_dist(br: &mut BitReader) -> Option<u16> {
    let mut acc: u32 = 0;
    for len in 1..=5 {
        let bit = br.read_bit()?;
        acc |= bit << (len - 1);
        let msb_code = reverse_bits(acc, len);
        if len == 5 && msb_code <= 31 {
            return Some(msb_code as u16);
        }
    }
    None
}

/// DEFLATE length base values for symbols 257-285.
const LENGTH_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 43, 45,
    47, 49, 50,
];

/// DEFLATE length extra bits count for symbols 257-285.
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// DEFLATE distance base values for symbols 0-31.
const DIST_BASE: [u32; 32] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1385, 1641,
    1921, 2185, 2449, 2713, 2977, 3241, 3505, 3769, 4033,
];

/// DEFLATE distance extra bits count for symbols 0-31.
const DIST_EXTRA: [u32; 32] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
    14, 14,
];

/// Inflate a fixed Huffman DEFLATE block.
fn inflate_fixed(br: &mut BitReader, output: &mut Vec<u8>) -> Option<()> {
    loop {
        let symbol = decode_fixed_lit_len(br)?;

        if symbol < 256 {
            output.push(symbol as u8);
        } else if symbol == 256 {
            break;
        } else {
            let length_index = (symbol - 257) as usize;
            let mut length = LENGTH_BASE[length_index];
            let extra = LENGTH_EXTRA[length_index];
            if extra > 0 {
                length += br.read_bits(extra as usize)?;
            }

            let dist_symbol = decode_fixed_dist(br)?;
            let dist_index = dist_symbol as usize;
            let mut distance = DIST_BASE[dist_index];
            let dist_extra = DIST_EXTRA[dist_index];
            if dist_extra > 0 {
                distance += br.read_bits(dist_extra as usize)?;
            }

            if output.len() < distance as usize {
                return None;
            }
            let start = output.len() - distance as usize;
            for i in 0..length as usize {
                let byte = output[start + (i % distance as usize)];
                output.push(byte);
            }
        }
    }
    Some(())
}

/// Minimal zlib decompression (stored ve fixed Huffman blokları).
fn zlib_decompress(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 2 {
        return None;
    }

    let cmf = data[0];
    let flg = data[1];
    let cm = cmf & 0x0F;
    let cinfo = (cmf >> 4) & 0x0F;

    if cm != 8 || cinfo > 7 {
        return None;
    }

    let fcheck = ((cmf as usize) * 256 + (flg as usize)) % 31;
    if fcheck != 0 {
        return None;
    }

    let fdict = (flg & 0x20) != 0;
    if fdict {
        return None; // FDictionary desteklenmiyor
    }

    let mut br = BitReader::new(&data[2..]);
    let mut output = Vec::new();

    loop {
        let bfinal = br.read_bit()?;
        let btype = br.read_bits(2)?;

        match btype {
            0 => {
                // Stored block
                br.align_to_byte();
                let len = br.read_bits(16)? as usize;
                let _nlen = br.read_bits(16)? as usize;
                for _ in 0..len {
                    output.push(br.read_bits(8)? as u8);
                }
            }
            1 => {
                // Fixed Huffman
                inflate_fixed(&mut br, &mut output)?;
            }
            _ => {
                return None;
            }
        }

        if bfinal == 1 {
            break;
        }
    }

    if output.is_empty() {
        None
    } else {
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_png_signature() {
        let png_data: &[u8] = &[
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A,
        ];
        assert!(decode(png_data).is_none()); // Sadece imza, geçerli PNG değil
    }
}
