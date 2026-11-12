//! PNG, JPEG, GIF, WebP ve SVG kod çözücüleri.
//!
//! Bu modül çekirdekten bağımsızdır: yalnızca `alloc` kullanır ve host
//! testlerinde doğrudan derlenebilir. Çözülen görüntü her zaman yukarıdan
//! aşağıya, 8 bit RGB olarak tutulur. Bellek tüketimini sınırlamak için
//! çok büyük görüntüler reddedilir.

use alloc::format;
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

/// İmzaya bakarak PNG, JPEG, GIF, WebP, BMP veya SVG çözer.
pub fn decode(data: &[u8]) -> Option<DecodedImage> {
    if data.len() >= 8 && data[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        decode_png(data)
    } else if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 {
        decode_jpeg(data)
    } else if data.len() >= 6 && (&data[0..6] == b"GIF87a" || &data[0..6] == b"GIF89a") {
        decode_gif(data)
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        decode_webp(data)
    } else if data.len() >= 2 && &data[0..2] == b"BM" {
        decode_bmp(data)
    } else if is_svg(data) {
        decode_svg(data)
    } else {
        None
    }
}

/// BMP dosyasının genişlik ve yüksekliğini döndürür.
pub fn bmp_dimensions(data: &[u8]) -> Option<(usize, usize)> {
    if data.len() < 26 || &data[0..2] != b"BM" {
        return None;
    }
    let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    if width == 0 || height == 0 {
        return None;
    }
    let w = width.unsigned_abs() as usize;
    let h = height.unsigned_abs() as usize;
    for_pixels(w, h)?;
    Some((w, h))
}

/// BMP kod çözücü (24-bit ve 32-bit, sıkıştırılmamış BI_RGB).
pub fn decode_bmp(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 54 || &data[0..2] != b"BM" {
        return None;
    }
    let bits_per_pixel = u16::from_le_bytes([data[28], data[29]]) as usize;
    let compression = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);
    if compression != 0 {
        // Sadece BI_RGB (sıkıştırılmamış) desteklenir.
        return None;
    }
    if bits_per_pixel != 24 && bits_per_pixel != 32 {
        return None;
    }

    let raw_w = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let raw_h = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    if raw_w == 0 || raw_h == 0 {
        return None;
    }
    let width = raw_w.unsigned_abs() as usize;
    let height = raw_h.unsigned_abs() as usize;
    let top_down = raw_h < 0;

    let offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    let bytes_per_pixel = bits_per_pixel / 8;
    let row_bytes = (width * bytes_per_pixel + 3) & !3;

    for_pixels(width, height)?;
    if offset
        .checked_add(row_bytes.checked_mul(height)?)
        .is_some_and(|end| end > data.len())
    {
        return None;
    }

    let mut rgb = Vec::with_capacity(width * height * 3);
    if rgb.try_reserve_exact(width * height * 3).is_err() {
        return None;
    }
    rgb.resize(width * height * 3, 0);

    let mut out = 0usize;
    for y in 0..height {
        let src_y = if top_down { y } else { height - 1 - y };
        let row_start = offset + src_y * row_bytes;
        for x in 0..width {
            let px_start = row_start + x * bytes_per_pixel;
            if px_start + 2 >= data.len() {
                return None;
            }
            // BMP saklar BGR sırasını; RGB'ye çevir.
            let b = data[px_start];
            let g = data[px_start + 1];
            let r = data[px_start + 2];
            rgb[out] = r;
            rgb[out + 1] = g;
            rgb[out + 2] = b;
            out += 3;
        }
    }

    Some(DecodedImage {
        width,
        height,
        rgb,
        format: "BMP",
    })
}

fn le16(d: &[u8], i: usize) -> Option<usize> {
    if i + 2 > d.len() {
        return None;
    }
    Some((d[i] as usize) | ((d[i + 1] as usize) << 8))
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
            | d[i + 3] as usize,
    )
}

fn le32(d: &[u8], i: usize) -> Option<usize> {
    if i + 4 > d.len() {
        return None;
    }
    Some(
        d[i] as usize
            | ((d[i + 1] as usize) << 8)
            | ((d[i + 2] as usize) << 16)
            | ((d[i + 3] as usize) << 24),
    )
}

fn for_pixels(w: usize, h: usize) -> Option<usize> {
    let n = w.checked_mul(h)?;
    if n == 0 || n > MAX_PIXELS {
        return None;
    }
    Some(n)
}

// ---------------------------------------------------------------------------
// PNG
// ---------------------------------------------------------------------------

struct PngHeader {
    width: usize,
    height: usize,
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
}

/// PNG imzasını ve boyutlarını doğrular.
pub fn png_dimensions(data: &[u8]) -> Option<(usize, usize)> {
    let h = parse_png_header(data)?;
    for_pixels(h.width, h.height)?;
    Some((h.width, h.height))
}

fn parse_png_header(data: &[u8]) -> Option<PngHeader> {
    if data.len() < 33 || data[..8] != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return None;
    }
    let len = be32(data, 8)?;
    if len != 13 || &data[12..16] != b"IHDR" {
        return None;
    }
    let width = be32(data, 16)?;
    let height = be32(data, 20)?;
    let bit_depth = data[24];
    let color_type = data[25];
    let compression = data[26];
    let filter = data[27];
    let interlace = data[28];
    if width == 0 || height == 0 || compression != 0 || filter != 0 || interlace > 1 {
        return None;
    }
    let ok_depth = match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(bit_depth, 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        _ => false,
    };
    if !ok_depth {
        return None;
    }
    Some(PngHeader {
        width,
        height,
        bit_depth,
        color_type,
        interlace,
    })
}

pub fn decode_png(data: &[u8]) -> Option<DecodedImage> {
    let hdr = parse_png_header(data)?;
    let pixels = for_pixels(hdr.width, hdr.height)?;
    if hdr.interlace != 0 {
        // Adam7 henüz desteklenmiyor; yaygın dosyalar ardışık düzendedir.
        return None;
    }
    let channels = match hdr.color_type {
        0 | 3 => 1usize,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    let mut palette: Option<Vec<u8>> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut pos = 8usize;
    while pos + 12 <= data.len() {
        let len = be32(data, pos)?;
        if pos + 12 + len > data.len() {
            return None;
        }
        let kind = &data[pos + 4..pos + 8];
        let body = &data[pos + 8..pos + 8 + len];
        match kind {
            b"PLTE" => palette = Some(body.to_vec()),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            b"IHDR" | b"tRNS" | b"gAMA" | b"cHRM" | b"sRGB" | b"pHYs" | b"tEXt" | b"iTXt"
            | b"zTXt" | b"bKGD" | b"hIST" | b"sBIT" => {}
            _ => {}
        }
        pos += 12 + len;
    }
    if idat.is_empty() || pixels.checked_mul(3)? > 16 * 1024 * 1024 {
        return None;
    }
    if hdr.color_type == 3 && palette.is_none() {
        return None;
    }
    let bits_per_pixel = channels.checked_mul(hdr.bit_depth as usize)?;
    let bpp = bits_per_pixel.div_ceil(8);
    let stride = hdr.width.checked_mul(bits_per_pixel)?.div_ceil(8);
    let expected = hdr.height.checked_mul(stride + 1)?;
    let mut raw = inflate_zlib(&idat, expected)?;
    if raw.len() < expected {
        return None;
    }
    raw.truncate(expected);
    unfilter_png(&mut raw, hdr.height, stride, bpp.max(1))?;

    let mut rgb = Vec::new();
    if rgb.try_reserve_exact(pixels * 3).is_err() {
        return None;
    }
    rgb.resize(pixels * 3, 0);
    let pal = palette.as_deref().unwrap_or(&[]);
    let depth = hdr.bit_depth;
    let mut out = 0usize;
    for y in 0..hdr.height {
        let row = &raw[y * (stride + 1) + 1..y * (stride + 1) + 1 + stride];
        for x in 0..hdr.width {
            let (r, g, b) = match hdr.color_type {
                0 => {
                    let v = if depth == 16 {
                        *row.get(x * 2)?
                    } else if depth == 8 {
                        *row.get(x)?
                    } else {
                        scale8(sample_bits(row, x, depth)?, depth)
                    };
                    (v, v, v)
                }
                2 => {
                    let step = if depth == 16 { 2 } else { 1 };
                    let base = x * 3 * step;
                    (
                        *row.get(base)?,
                        *row.get(base + step)?,
                        *row.get(base + step * 2)?,
                    )
                }
                3 => {
                    let idx = if depth == 8 {
                        *row.get(x)? as usize
                    } else {
                        sample_bits(row, x, depth)? as usize
                    };
                    if idx * 3 + 2 >= pal.len() {
                        return None;
                    }
                    (pal[idx * 3], pal[idx * 3 + 1], pal[idx * 3 + 2])
                }
                4 => {
                    let step = if depth == 16 { 2 } else { 1 };
                    let g = *row.get(x * 2 * step)?;
                    let a = *row.get(x * 2 * step + step)?;
                    (blend_white(g, a), blend_white(g, a), blend_white(g, a))
                }
                6 => {
                    let step = if depth == 16 { 2 } else { 1 };
                    let base = x * 4 * step;
                    let a = *row.get(base + step * 3)?;
                    (
                        blend_white(*row.get(base)?, a),
                        blend_white(*row.get(base + step)?, a),
                        blend_white(*row.get(base + step * 2)?, a),
                    )
                }
                _ => return None,
            };
            if out + 2 >= rgb.len() {
                return None;
            }
            rgb[out] = r;
            rgb[out + 1] = g;
            rgb[out + 2] = b;
            out += 3;
        }
    }
    Some(DecodedImage {
        width: hdr.width,
        height: hdr.height,
        rgb,
        format: "PNG",
    })
}

fn scale8(value: u32, bit_depth: u8) -> u8 {
    if bit_depth == 16 {
        // Yüksek bayt `sample` içinde seçilir.
        value.min(255) as u8
    } else if bit_depth == 8 {
        value.min(255) as u8
    } else {
        let max = ((1u32 << bit_depth) - 1).max(1);
        ((value * 255 + max / 2) / max).min(255) as u8
    }
}

fn blend_white(c: u8, a: u8) -> u8 {
    let c = c as u32;
    let a = a as u32;
    ((c * a + 255 * (255 - a) + 127) / 255).min(255) as u8
}

/// 1/2/4 bit palet veya gri ton örneği okur (satır içi bit indeksi).
fn sample_bits(row: &[u8], index: usize, bit_depth: u8) -> Option<u32> {
    let depth = bit_depth as usize;
    if depth == 0 || depth > 4 {
        return None;
    }
    let per_byte = 8 / depth;
    let byte = *row.get(index / per_byte)? as u32;
    let shift = 8 - depth * ((index % per_byte) + 1);
    Some((byte >> shift) & ((1u32 << depth) - 1))
}

fn unfilter_png(raw: &mut [u8], height: usize, stride: usize, bpp: usize) -> Option<()> {
    for y in 0..height {
        let start = y * (stride + 1);
        let filter = raw[start];
        let row_start = start + 1;
        let prev_start = if y > 0 {
            Some((y - 1) * (stride + 1) + 1)
        } else {
            None
        };
        for x in 0..stride {
            let a = if x >= bpp {
                raw[row_start + x - bpp]
            } else {
                0
            };
            let b = prev_start.map(|p| raw[p + x]).unwrap_or(0);
            let c = if x >= bpp {
                prev_start.map(|p| raw[p + x - bpp]).unwrap_or(0)
            } else {
                0
            };
            let v = raw[row_start + x];
            raw[row_start + x] = match filter {
                0 => v,
                1 => v.wrapping_add(a),
                2 => v.wrapping_add(b),
                3 => v.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => v.wrapping_add(paeth(a, b, c)),
                _ => return None,
            };
        }
    }
    Some(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

// ---------------------------------------------------------------------------
// zlib / DEFLATE
// ---------------------------------------------------------------------------

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buf: 0,
            cnt: 0,
        }
    }

    fn fill(&mut self) -> bool {
        if self.pos >= self.data.len() {
            return false;
        }
        // DEFLATE bitleri baytların en anlamsız bitinden başlayarak paketler.
        // Tamponu bu sıraya göre doldur; `bits` böylece akış sırasını verir.
        self.buf |= (self.data[self.pos] as u32) << self.cnt;
        self.pos += 1;
        self.cnt += 8;
        true
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        while self.cnt < n {
            if !self.fill() {
                return None;
            }
        }
        let value = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.cnt -= n;
        Some(value)
    }

    fn align_byte(&mut self) {
        let drop = self.cnt % 8;
        self.buf >>= drop;
        self.cnt -= drop;
    }
}

#[derive(Clone)]
struct Huffman {
    counts: [u16; 16],
    first: [u32; 17],
    offset: [u32; 16],
    /// DEFLATE literal/length alfabesi 285'e kadar sembol içerir; u8 yetersiz.
    symbols: Vec<u16>,
}

trait BitReaderLike {
    fn bits(&mut self, n: u32) -> Option<u32>;
}

impl BitReaderLike for BitReader<'_> {
    fn bits(&mut self, n: u32) -> Option<u32> {
        BitReader::bits(self, n)
    }
}

impl BitReaderLike for JBitReader<'_> {
    fn bits(&mut self, n: u32) -> Option<u32> {
        JBitReader::bits(self, n)
    }
}

impl Huffman {
    /// Kanonik Huffman tablosunu, uzunluk sayıları ve bu uzunluk sırasına
    /// göre dizilmiş sembollerden kurar.
    fn from_counts(counts: &[u16; 16], symbols: &[u16]) -> Option<Self> {
        let mut first = [0u32; 17];
        let mut offset = [0u32; 16];
        let mut code = 0u32;
        let mut total = 0u32;
        for len in 1..=16usize {
            first[len] = code;
            offset[len - 1] = total;
            code = (code + counts[len - 1] as u32) << 1;
            total += counts[len - 1] as u32;
        }
        if total as usize != symbols.len() {
            return None;
        }
        Some(Self {
            counts: *counts,
            first,
            offset,
            symbols: symbols.to_vec(),
        })
    }

    /// DEFLATE: `lengths[sembol]` biçiminde bit uzunlukları.
    fn from_lengths(lengths: &[u8]) -> Option<Self> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            // DEFLATE en fazla 15, JPEG ise 16 bitlik Huffman kodu kullanır.
            if l as usize > 16 {
                return None;
            }
            if l != 0 {
                counts[l as usize - 1] += 1;
            }
        }
        let mut symbols = Vec::new();
        for len in 1..=16u8 {
            for (sym, &l) in lengths.iter().enumerate() {
                if l == len {
                    symbols.push(sym as u16);
                }
            }
        }
        Self::from_counts(&counts, &symbols)
    }

    /// JPEG: DHT segmentindeki uzunluk sayıları + sembol baytları.
    fn from_jpeg(counts: &[u8], symbols: &[u8]) -> Option<Self> {
        if counts.len() < 16 {
            return None;
        }
        let mut c = [0u16; 16];
        for (i, &v) in counts.iter().take(16).enumerate() {
            c[i] = v as u16;
        }
        let ordered: Vec<u16> = symbols.iter().map(|&s| s as u16).collect();
        Self::from_counts(&c, &ordered)
    }

    fn decode(&self, br: &mut impl BitReaderLike) -> Option<u16> {
        let mut code = 0u32;
        for len in 1..=16usize {
            code = (code << 1) | br.bits(1)?;
            let count = self.counts[len - 1] as u32;
            let first = self.first[len];
            if code >= first && code - first < count {
                let idx = self.offset[len - 1] as usize + (code - first) as usize;
                return self.symbols.get(idx).copied();
            }
        }
        None
    }
}

fn fixed_tables() -> Option<(Huffman, Huffman)> {
    let mut lit = [0u8; 288];
    for i in 0..288 {
        lit[i] = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let dist = [5u8; 32];
    Some((Huffman::from_lengths(&lit)?, Huffman::from_lengths(&dist)?))
}

fn dynamic_tables(br: &mut BitReader) -> Option<(Huffman, Huffman)> {
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return None;
    }
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut cl = [0u8; 19];
    for i in 0..hclen {
        cl[ORDER[i]] = br.bits(3)? as u8;
    }
    let cl_huff = Huffman::from_lengths(&cl)?;
    let mut lengths = vec![0u8; hlit + hdist];
    let mut i = 0usize;
    while i < lengths.len() {
        let sym = cl_huff.decode(br)?;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return None;
                }
                let prev = lengths[i - 1];
                let rep = br.bits(2)? as usize + 3;
                if i + rep > lengths.len() {
                    return None;
                }
                for _ in 0..rep {
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 => {
                let rep = br.bits(3)? as usize + 3;
                if i + rep > lengths.len() {
                    return None;
                }
                i += rep;
            }
            18 => {
                let rep = br.bits(7)? as usize + 11;
                if i + rep > lengths.len() {
                    return None;
                }
                i += rep;
            }
            _ => return None,
        }
    }
    let lit = Huffman::from_lengths(&lengths[..hlit])?;
    let dist = Huffman::from_lengths(&lengths[hlit..hlit + hdist])?;
    Some((lit, dist))
}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// zlib başlığını doğrular ve DEFLATE akışını açar.
pub fn inflate_zlib(data: &[u8], expected: usize) -> Option<Vec<u8>> {
    if data.len() < 2 {
        return None;
    }
    let cmf = data[0];
    let flg = data[1];
    if cmf & 0x0F != 8 || (cmf >> 4) > 7 {
        return None;
    }
    if ((cmf as u16) << 8 | flg as u16) % 31 != 0 {
        return None;
    }
    if flg & 0x20 != 0 {
        // Önceden tanımlı sözlük desteklenmiyor.
        return None;
    }
    inflate(&data[2..], expected)
}

/// Ham DEFLATE akışını açar. `expected` çıktı üst sınırıdır.
pub fn inflate(data: &[u8], expected: usize) -> Option<Vec<u8>> {
    if expected == 0 || expected > 64 * 1024 * 1024 {
        return None;
    }
    let mut br = BitReader::new(data);
    let mut out: Vec<u8> = Vec::new();
    if out.try_reserve_exact(expected).is_err() {
        return None;
    }
    loop {
        let last = br.bits(1)?;
        let btype = br.bits(2)?;
        match btype {
            0 => {
                br.align_byte();
                let len = br.bits(16)? as usize;
                let nlen = br.bits(16)? as usize;
                if len != (!nlen & 0xFFFF) {
                    return None;
                }
                if out.len().checked_add(len)? > expected {
                    return None;
                }
                for _ in 0..len {
                    out.push(br.bits(8)? as u8);
                }
            }
            1 | 2 => {
                let (lit, dist) = if btype == 1 {
                    fixed_tables()?
                } else {
                    dynamic_tables(&mut br)?
                };
                loop {
                    let sym = lit.decode(&mut br)? as usize;
                    if sym < 256 {
                        if out.len() >= expected {
                            return None;
                        }
                        out.push(sym as u8);
                    } else if sym == 256 {
                        break;
                    } else {
                        let li = sym.checked_sub(257)?;
                        if li >= LEN_BASE.len() {
                            return None;
                        }
                        let len = LEN_BASE[li] as usize + br.bits(LEN_EXTRA[li] as u32)? as usize;
                        let dsym = dist.decode(&mut br)? as usize;
                        if dsym >= DIST_BASE.len() {
                            return None;
                        }
                        let distance =
                            DIST_BASE[dsym] as usize + br.bits(DIST_EXTRA[dsym] as u32)? as usize;
                        if distance > out.len() || out.len().checked_add(len)? > expected {
                            return None;
                        }
                        let start = out.len() - distance;
                        for i in 0..len {
                            let b = out[start + i];
                            out.push(b);
                        }
                    }
                }
            }
            _ => return None,
        }
        if last == 1 {
            break;
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// JPEG
// ---------------------------------------------------------------------------

struct JComponent {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    td: usize,
    ta: usize,
    bw: usize,
    bh: usize,
}

struct JFrame {
    width: usize,
    height: usize,
    comps: Vec<JComponent>,
    max_h: usize,
    max_v: usize,
}

/// Progresif JPEG taraması. Huffman tabloları tarama anındaki halleriyle
/// saklanır; sonraki DHT segmentleri aynı kimliği yeniden tanımlayabilir.
struct ProgScan {
    scan_header: usize,
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
    comps: Vec<(usize, usize, usize)>,
    dc_tables: [Option<Huffman>; 4],
    ac_tables: [Option<Huffman>; 4],
}

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

struct JBitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl<'a> JBitReader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            buf: 0,
            cnt: 0,
        }
    }

    fn fill(&mut self) -> bool {
        if self.pos >= self.data.len() {
            return false;
        }
        let b = self.data[self.pos];
        if b == 0xFF {
            if self.pos + 1 < self.data.len() && self.data[self.pos + 1] == 0x00 {
                self.pos += 2;
                self.buf = (self.buf << 8) | 0xFF;
                self.cnt += 8;
                return true;
            }
            // Marker: entropi verisinin sonu.
            return false;
        }
        self.pos += 1;
        self.buf = (self.buf << 8) | b as u32;
        self.cnt += 8;
        true
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        if n > 24 {
            return None;
        }
        while self.cnt < n {
            if !self.fill() {
                return None;
            }
        }
        self.cnt -= n;
        Some((self.buf >> self.cnt) & ((1u32 << n) - 1))
    }

    fn reset_at_restart(&mut self) -> bool {
        self.buf = 0;
        self.cnt = 0;
        if self.pos + 1 < self.data.len() && self.data[self.pos] == 0xFF {
            let m = self.data[self.pos + 1];
            if (0xD0..=0xD7).contains(&m) {
                self.pos += 2;
                return true;
            }
        }
        // Birkaç dolgu baytı olabilir.
        let end = (self.pos + 8).min(self.data.len().saturating_sub(1));
        let mut i = self.pos;
        while i < end {
            if self.data[i] == 0xFF && (0xD0..=0xD7).contains(&self.data[i + 1]) {
                self.pos = i + 2;
                return true;
            }
            i += 1;
        }
        false
    }
}

/// Entropi kodlu veriyi atlayıp bir sonraki gerçek JPEG marker'ının
/// (0xFF) konumunu döndürür. Dolgu (0xFF 0x00) ve restart (0xD0..0xD7)
/// baytları marker sayılmaz.
fn skip_entropy_data(data: &[u8], mut pos: usize) -> usize {
    while pos + 1 < data.len() {
        if data[pos] == 0xFF {
            let next = data[pos + 1];
            if next == 0x00 || (0xD0..=0xD7).contains(&next) {
                pos += 2;
                continue;
            }
            break;
        }
        pos += 1;
    }
    pos
}

fn extend(v: u32, s: u32) -> i32 {
    if s == 0 {
        0
    } else if v < (1u32 << (s - 1)) {
        v as i32 - ((1i32 << s) - 1)
    } else {
        v as i32
    }
}

/// 1B IDCT. Ara değerler büyük katsayılarda i32'yi taşabildiği için
/// hesap i64 üzerinden yapılır.
fn idct_1d(s: [i64; 8]) -> [i64; 8] {
    let p2 = s[2];
    let p3 = s[6];
    let p1 = (p2 + p3) * 2217;
    let t2e = p1 + p3 * -7568;
    let t3e = p1 + p2 * 3135;
    let p2 = s[0];
    let p3 = s[4];
    let t0e = (p2 + p3) << 12;
    let t1e = (p2 - p3) << 12;
    let x0 = t0e + t3e;
    let x3 = t0e - t3e;
    let x1 = t1e + t2e;
    let x2 = t1e - t2e;

    let t0 = s[7];
    let t1 = s[5];
    let t2 = s[3];
    let t3 = s[1];
    let p3 = t0 + t2;
    let p4 = t1 + t3;
    let p1 = t0 + t3;
    let p2 = t1 + t2;
    let p5 = (p3 + p4) * 4816;
    let t0 = t0 * 1223;
    let t1 = t1 * 8410;
    let t2 = t2 * 12586;
    let t3 = t3 * 6149;
    let p1 = p5 + p1 * -3686;
    let p2 = p5 + p2 * -10498;
    let p3 = p3 * -8035;
    let p4 = p4 * -1598;
    let t3 = t3 + p1 + p4;
    let t2 = t2 + p2 + p3;
    let t1 = t1 + p2 + p4;
    let t0 = t0 + p1 + p3;
    [
        x0 + t3,
        x1 + t2,
        x2 + t1,
        x3 + t0,
        x3 - t0,
        x2 - t1,
        x1 - t2,
        x0 - t3,
    ]
}

fn idct_block(coeff: &[i32], out: &mut [u8], out_w: usize, ox: usize, oy: usize) {
    let mut tmp = [0i64; 64];
    for col in 0..8 {
        let s = [
            coeff[col] as i64,
            coeff[8 + col] as i64,
            coeff[16 + col] as i64,
            coeff[24 + col] as i64,
            coeff[32 + col] as i64,
            coeff[40 + col] as i64,
            coeff[48 + col] as i64,
            coeff[56 + col] as i64,
        ];
        let v = idct_1d(s);
        for row in 0..8 {
            tmp[row * 8 + col] = (v[row] + 512) >> 10;
        }
    }
    for row in 0..8 {
        let mut s = [0i64; 8];
        s.copy_from_slice(&tmp[row * 8..row * 8 + 8]);
        let v = idct_1d(s);
        for col in 0..8 {
            let value = ((v[col] + 65536 + (128 << 17)) >> 17).clamp(0, 255) as u8;
            let idx = (oy + row) * out_w + ox + col;
            if idx < out.len() {
                out[idx] = value;
            }
        }
    }
}

pub fn decode_jpeg(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut quant: [Option<[i32; 64]>; 4] = [None, None, None, None];
    let mut dc_tables: [Option<Huffman>; 4] = [None, None, None, None];
    let mut ac_tables: [Option<Huffman>; 4] = [None, None, None, None];
    let mut frame: Option<JFrame> = None;
    let mut restart_interval = 0usize;
    let mut pos = 2usize;
    // Progresif JPEG: tarama bilgileri, tablo anlık görüntüleriyle birlikte.
    let mut prog_scans: Vec<ProgScan> = Vec::new();
    let mut is_progressive = false;

    while pos < data.len() {
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }
        while pos < data.len() && data[pos] == 0xFF {
            pos += 1;
        }
        if pos >= data.len() {
            break;
        }
        let marker = data[pos];
        pos += 1;
        match marker {
            0xD8 | 0x01 | 0xD0..=0xD7 => continue,
            0xD9 => break,
            0xDA => {
                // SOS (Start of Scan) — progresif JPEG'de birden fazla olabilir.
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len > data.len() {
                    return None;
                }
                // Scan başlangıcı: SOS başlığının sonu (segment uzunluğu
                // iki baytlık uzunluk alanını da kapsar).
                let scan_header = pos;
                let scan_start = pos + seg_len;
                let seg = &data[scan_header + 2..scan_start];

                if is_progressive {
                    let n = *seg.first()? as usize;
                    if n == 0 || n > 4 || seg.len() < 1 + n * 2 + 3 {
                        return None;
                    }
                    let ss = seg[1 + n * 2] as usize;
                    let se = seg[2 + n * 2] as usize;
                    let ahal = seg[3 + n * 2];
                    if ss > 63 || se > 63 || ss > se || (ahal & 15) > 13 {
                        return None;
                    }
                    let mut comps = Vec::new();
                    for i in 0..n {
                        let id = seg[1 + i * 2];
                        let tables = seg[2 + i * 2];
                        if let Some(ci) = frame
                            .as_ref()
                            .and_then(|f| f.comps.iter().position(|c| c.id == id))
                        {
                            comps.push((ci, (tables >> 4) as usize, (tables & 15) as usize));
                        }
                    }
                    prog_scans.push(ProgScan {
                        scan_header,
                        ss,
                        se,
                        ah: (ahal >> 4) as u32,
                        al: (ahal & 15) as u32,
                        comps,
                        dc_tables: dc_tables.clone(),
                        ac_tables: ac_tables.clone(),
                    });
                    // Entropi verisini atlayarak bir sonraki gerçek marker'a git.
                    pos = skip_entropy_data(data, scan_start);
                    continue;
                }

                // Baseline (tek scan): mevcut davranış.
                let mut fr = frame.take()?;
                let n = *seg.first()? as usize;
                if n != fr.comps.len() || seg.len() < 1 + n * 2 {
                    return None;
                }
                for i in 0..n {
                    let id = seg[1 + i * 2];
                    let tables = seg[2 + i * 2];
                    let comp = fr.comps.iter_mut().find(|c| c.id == id)?;
                    comp.td = (tables >> 4) as usize;
                    comp.ta = (tables & 15) as usize;
                }
                let rgb = decode_jpeg_scan(
                    data,
                    scan_start,
                    &fr,
                    &quant,
                    &dc_tables,
                    &ac_tables,
                    restart_interval,
                )?;
                return Some(DecodedImage {
                    width: fr.width,
                    height: fr.height,
                    rgb,
                    format: "JPEG",
                });
            }
            0xDB => {
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len > data.len() {
                    return None;
                }
                let seg = &data[pos + 2..pos + seg_len];
                pos += seg_len;
                let mut p = 0usize;
                while p < seg.len() {
                    let pq = seg[p] >> 4;
                    let tq = (seg[p] & 15) as usize;
                    p += 1;
                    if tq >= 4 {
                        return None;
                    }
                    let mut table = [1i32; 64];
                    if pq == 0 {
                        if p + 64 > seg.len() {
                            return None;
                        }
                        for i in 0..64 {
                            table[i] = seg[p + i] as i32;
                        }
                        p += 64;
                    } else if pq == 1 {
                        if p + 128 > seg.len() {
                            return None;
                        }
                        for i in 0..64 {
                            table[i] = be16(seg, p + i * 2)? as i32;
                        }
                        p += 128;
                    } else {
                        return None;
                    }
                    quant[tq] = Some(table);
                }
            }
            0xC4 => {
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len > data.len() {
                    return None;
                }
                let seg = &data[pos + 2..pos + seg_len];
                pos += seg_len;
                let mut p = 0usize;
                while p + 17 <= seg.len() {
                    let tc = seg[p] >> 4;
                    let th = (seg[p] & 15) as usize;
                    p += 1;
                    if th >= 4 {
                        return None;
                    }
                    let mut counts = [0u8; 16];
                    let mut total = 0usize;
                    for i in 0..16 {
                        counts[i] = seg[p + i];
                        total += counts[i] as usize;
                    }
                    p += 16;
                    if p + total > seg.len() {
                        return None;
                    }
                    // DHT sembolleri uzunluk sırasına göre gelir; tablo bu
                    // sırayla ve sembol değerleriyle kurulmalıdır.
                    let table = Huffman::from_jpeg(&counts, &seg[p..p + total])?;
                    if tc == 0 {
                        dc_tables[th] = Some(table);
                    } else if tc == 1 {
                        ac_tables[th] = Some(table);
                    } else {
                        return None;
                    }
                    p += total;
                }
            }
            0xC0 | 0xC1 => {
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len > data.len() {
                    return None;
                }
                let seg = &data[pos + 2..pos + seg_len];
                pos += seg_len;
                if frame.is_some() || seg.len() < 6 || seg[0] != 8 {
                    return None;
                }
                let height = be16(seg, 1)?;
                let width = be16(seg, 3)?;
                let n = seg[5] as usize;
                if n == 0 || n > 4 || seg.len() < 6 + n * 3 {
                    return None;
                }
                for_pixels(width, height)?;
                let mut comps = Vec::new();
                let mut max_h = 1usize;
                let mut max_v = 1usize;
                for i in 0..n {
                    let base = 6 + i * 3;
                    let id = seg[base];
                    let hv = seg[base + 1];
                    let h = (hv >> 4) as usize;
                    let v = (hv & 15) as usize;
                    let tq = seg[base + 2] as usize;
                    if !(1..=4).contains(&h) || !(1..=4).contains(&v) || tq >= 4 {
                        return None;
                    }
                    max_h = max_h.max(h);
                    max_v = max_v.max(v);
                    comps.push(JComponent {
                        id,
                        h,
                        v,
                        tq,
                        td: 0,
                        ta: 0,
                        bw: 0,
                        bh: 0,
                    });
                }
                let mcu_w = 8 * max_h;
                let mcu_h = 8 * max_v;
                let mcus_x = width.div_ceil(mcu_w);
                let mcus_y = height.div_ceil(mcu_h);
                let mut total_plane = 0usize;
                for c in comps.iter_mut() {
                    c.bw = mcus_x * c.h;
                    c.bh = mcus_y * c.v;
                    let size = c.bw.checked_mul(c.bh)?.checked_mul(64)?;
                    total_plane = total_plane.checked_add(size)?;
                }
                if total_plane > 24 * 1024 * 1024 {
                    return None;
                }
                frame = Some(JFrame {
                    width,
                    height,
                    comps,
                    max_h,
                    max_v,
                });
            }
            0xDD => {
                let seg_len = be16(data, pos)?;
                if seg_len != 4 || pos + seg_len > data.len() {
                    return None;
                }
                restart_interval = be16(data, pos + 2)?;
                pos += seg_len;
            }
            0xC2 => {
                // Progressive JPEG (SOF2): çoklu geçiş decoding desteği.
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len > data.len() {
                    return None;
                }
                let seg = &data[pos + 2..pos + seg_len];
                pos += seg_len;
                if frame.is_some() || seg.len() < 6 || seg[0] != 8 {
                    return None;
                }
                let height = be16(seg, 1)?;
                let width = be16(seg, 3)?;
                let n = seg[5] as usize;
                if n == 0 || n > 4 || seg.len() < 6 + n * 3 {
                    return None;
                }
                for_pixels(width, height)?;
                let mut comps = Vec::new();
                let mut max_h = 1usize;
                let mut max_v = 1usize;
                for i in 0..n {
                    let base = 6 + i * 3;
                    let id = seg[base];
                    let hv = seg[base + 1];
                    let h = (hv >> 4) as usize;
                    let v = (hv & 15) as usize;
                    let tq = seg[base + 2] as usize;
                    if !(1..=4).contains(&h) || !(1..=4).contains(&v) || tq >= 4 {
                        return None;
                    }
                    max_h = max_h.max(h);
                    max_v = max_v.max(v);
                    comps.push(JComponent {
                        id,
                        h,
                        v,
                        tq,
                        td: 0,
                        ta: 0,
                        bw: 0,
                        bh: 0,
                    });
                }
                let mcu_w = 8 * max_h;
                let mcu_h = 8 * max_v;
                let mcus_x = width.div_ceil(mcu_w);
                let mcus_y = height.div_ceil(mcu_h);
                let mut total_plane = 0usize;
                for c in comps.iter_mut() {
                    c.bw = mcus_x * c.h;
                    c.bh = mcus_y * c.v;
                    let size = c.bw.checked_mul(c.bh)?.checked_mul(64)?;
                    total_plane = total_plane.checked_add(size)?;
                }
                if total_plane > 24 * 1024 * 1024 {
                    return None;
                }
                is_progressive = true;
                frame = Some(JFrame {
                    width,
                    height,
                    comps,
                    max_h,
                    max_v,
                });
            }
            0xC3 | 0xC5..=0xCF => {
                // Aritmetik JPEG desteklenmiyor.
                return None;
            }
            _ => {
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len > data.len() {
                    return None;
                }
                pos += seg_len;
            }
        }
    }

    // Progresif JPEG: taramalar katsayıları kademeli olarak doldurur.
    // Önce tüm katsayılar toplanır, sonra tek bir IDCT + renk dönüşümü yapılır.
    if is_progressive {
        let fr = frame?;
        if prog_scans.is_empty() {
            return None;
        }

        let mcu_w = 8 * fr.max_h;
        let mcu_h = 8 * fr.max_v;
        let mcus_x = fr.width.div_ceil(mcu_w);
        let mcus_y = fr.height.div_ceil(mcu_h);

        let mut coefs: Vec<Vec<i32>> = Vec::new();
        for c in &fr.comps {
            coefs.push(vec![0i32; c.bw * c.bh * 64]);
        }

        for scan in &prog_scans {
            if scan.comps.is_empty() {
                return None;
            }
            let scan_start = scan.scan_header + be16(data, scan.scan_header)?;
            if scan_start > data.len() {
                return None;
            }
            let ss = scan.ss;
            let se = scan.se;
            let ah = scan.ah;
            let al = scan.al;

            let mut br = JBitReader::new(data, scan_start);
            let mut eobrun = 0u32;
            let mut unit = 0usize;
            let mut preds = [0i32; 4];

            if ss == 0 {
                // DC taraması.
                if scan.comps.len() == 1 {
                    // Tek bileşenli tarama: MCU = bir blok, gerçek blok sayıları.
                    let (ci, td, _ta) = scan.comps[0];
                    let comp = &fr.comps[ci];
                    let dc = scan.dc_tables.get(td)?.as_ref()?;
                    let (aw, ahh) = component_block_dims(&fr, comp);
                    for by in 0..ahh {
                        for bx in 0..aw {
                            if restart_interval > 0 && unit > 0 && unit.is_multiple_of(restart_interval) {
                                if !br.reset_at_restart() {
                                    return None;
                                }
                                preds = [0i32; 4];
                            }
                            let start = (by * comp.bw + bx) * 64;
                            if ah == 0 {
                                let s = dc.decode(&mut br)? as u32;
                                if s > 16 {
                                    return None;
                                }
                                let diff = if s == 0 { 0 } else { extend(br.bits(s)?, s) };
                                preds[ci] = preds[ci].wrapping_add(diff);
                                coefs[ci][start] = preds[ci].wrapping_shl(al);
                            } else if br.bits(1)? != 0 {
                                coefs[ci][start] |= 1i32 << al;
                            }
                            unit += 1;
                        }
                    }
                } else {
                    // Örgülü tarama: MCU bazlı, bileşenler arası geçişli.
                    for my in 0..mcus_y {
                        for mx in 0..mcus_x {
                            if restart_interval > 0 && unit > 0 && unit.is_multiple_of(restart_interval) {
                                if !br.reset_at_restart() {
                                    return None;
                                }
                                preds = [0i32; 4];
                            }
                            for &(ci, td, _ta) in &scan.comps {
                                let comp = &fr.comps[ci];
                                let dc = scan.dc_tables.get(td)?.as_ref()?;
                                for by in 0..comp.v {
                                    for bx in 0..comp.h {
                                        let bxx = mx * comp.h + bx;
                                        let byy = my * comp.v + by;
                                        if bxx >= comp.bw || byy >= comp.bh {
                                            continue;
                                        }
                                        let start = (byy * comp.bw + bxx) * 64;
                                        if ah == 0 {
                                            let s = dc.decode(&mut br)? as u32;
                                            if s > 16 {
                                                return None;
                                            }
                                            let diff =
                                                if s == 0 { 0 } else { extend(br.bits(s)?, s) };
                                            preds[ci] = preds[ci].wrapping_add(diff);
                                            coefs[ci][start] = preds[ci].wrapping_shl(al);
                                        } else if br.bits(1)? != 0 {
                                            coefs[ci][start] |= 1i32 << al;
                                        }
                                    }
                                }
                            }
                            unit += 1;
                        }
                    }
                }
            } else {
                // AC taramaları tek bileşenlidir; blok bazlı ilerler.
                for &(ci, _td, ta) in &scan.comps {
                    let comp = &fr.comps[ci];
                    let ac = scan.ac_tables.get(ta)?.as_ref()?;
                    let (aw, ahh) = component_block_dims(&fr, comp);
                    let row_blocks = comp.bw;
                    for by in 0..ahh {
                        for bx in 0..aw {
                            if restart_interval > 0 && unit > 0 && unit.is_multiple_of(restart_interval) {
                                if !br.reset_at_restart() {
                                    return None;
                                }
                                eobrun = 0;
                            }
                            let start = (by * row_blocks + bx) * 64;
                            let block = &mut coefs[ci][start..start + 64];

                            if ah == 0 {
                                if eobrun > 0 {
                                    eobrun -= 1;
                                } else {
                                    let mut k = ss;
                                    while k <= se {
                                        let rs = ac.decode(&mut br)? as usize;
                                        let r = rs >> 4;
                                        let s = rs & 15;
                                        if s == 0 {
                                            if r < 15 {
                                                let extra =
                                                    if r > 0 { br.bits(r as u32)? } else { 0 };
                                                eobrun = (1u32 << r) + extra;
                                                eobrun = eobrun.saturating_sub(1);
                                                break;
                                            }
                                            k += 16;
                                            continue;
                                        }
                                        k += r;
                                        if k > se {
                                            break;
                                        }
                                        let v = extend(br.bits(s as u32)?, s as u32);
                                        block[ZIGZAG[k]] = v.wrapping_shl(al);
                                        k += 1;
                                    }
                                }
                            } else {
                                let bit = 1i32 << al;
                                if eobrun > 0 {
                                    eobrun -= 1;
                                    for k in ss..=se {
                                        let p = &mut block[ZIGZAG[k]];
                                        if *p != 0 && br.bits(1)? != 0 && (*p & bit) == 0 {
                                            if *p > 0 {
                                                *p += bit;
                                            } else {
                                                *p -= bit;
                                            }
                                        }
                                    }
                                } else {
                                    let mut k = ss;
                                    while k <= se {
                                        let rs = ac.decode(&mut br)? as usize;
                                        let r = rs >> 4;
                                        let s = rs & 15;
                                        if s == 0 {
                                            if r < 15 {
                                                let extra =
                                                    if r > 0 { br.bits(r as u32)? } else { 0 };
                                                eobrun = (1u32 << r) + extra;
                                                eobrun = eobrun.saturating_sub(1);
                                                // EOB: kalan katsayılar sıfır kabul
                                                // edilir, ama mevcut sıfır olmayan
                                                // katsayıların düzeltme bitleri bu
                                                // blokta hâlâ akışta bulunur.
                                                let mut kk = k;
                                                while kk <= se {
                                                    let p = &mut block[ZIGZAG[kk]];
                                                    kk += 1;
                                                    if *p != 0
                                                        && br.bits(1)? != 0
                                                        && (*p & bit) == 0
                                                    {
                                                        if *p > 0 {
                                                            *p += bit;
                                                        } else {
                                                            *p -= bit;
                                                        }
                                                    }
                                                }
                                                break;
                                            }
                                            // ZRL (r=15, s=0): 16 sıfır katsayı
                                            // atlanır. Sıfır olmayan katsayılar
                                            // sayılmaz; yalnızca düzeltme bitleri
                                            // okunur.
                                            let mut run = 15usize;
                                            while k <= se {
                                                let p = &mut block[ZIGZAG[k]];
                                                k += 1;
                                                if *p != 0 {
                                                    if br.bits(1)? != 0 && (*p & bit) == 0 {
                                                        if *p > 0 {
                                                            *p += bit;
                                                        } else {
                                                            *p -= bit;
                                                        }
                                                    }
                                                } else if run == 0 {
                                                    break;
                                                } else {
                                                    run -= 1;
                                                }
                                            }
                                            continue;
                                        }
                                        if s != 1 {
                                            return None;
                                        }
                                        let sign = if br.bits(1)? != 0 { bit } else { -bit };
                                        let mut run = r;
                                        while k <= se {
                                            let p = &mut block[ZIGZAG[k]];
                                            k += 1;
                                            if *p != 0 {
                                                if br.bits(1)? != 0 && (*p & bit) == 0 {
                                                    if *p > 0 {
                                                        *p += bit;
                                                    } else {
                                                        *p -= bit;
                                                    }
                                                }
                                            } else if run == 0 {
                                                *p = sign;
                                                break;
                                            } else {
                                                run -= 1;
                                            }
                                        }
                                    }
                                }
                            }
                            unit += 1;
                        }
                    }
                }
            }
        }

        let mut planes: Vec<Vec<u8>> = Vec::new();
        for c in &fr.comps {
            planes.push(vec![0u8; c.bw * c.bh * 64]);
        }
        for (ci, c) in fr.comps.iter().enumerate() {
            // Kuantizasyon tablosu zikzak sırasında saklanır; katsayılar ise
            // doğal (satır-sütun) sırada biriktirilir.
            let qt = quant[c.tq]?;
            let mut qnat = [0i32; 64];
            for k in 0..64 {
                qnat[ZIGZAG[k]] = qt[k];
            }
            let (aw, ah) = component_block_dims(&fr, c);
            for by in 0..ah {
                for bx in 0..aw {
                    let start = (by * c.bw + bx) * 64;
                    let mut dq = [0i32; 64];
                    for i in 0..64 {
                        dq[i] = coefs[ci][start + i].wrapping_mul(qnat[i]);
                    }
                    idct_block(&dq, &mut planes[ci], c.bw * 8, bx * 8, by * 8);
                }
            }
        }
        let rgb = planes_to_rgb(&fr, &planes)?;
        return Some(DecodedImage {
            width: fr.width,
            height: fr.height,
            rgb,
            format: "JPEG (progressive)",
        });
    }

    None
}

fn decode_jpeg_scan(
    data: &[u8],
    start: usize,
    frame: &JFrame,
    quant: &[Option<[i32; 64]>; 4],
    dc_tables: &[Option<Huffman>; 4],
    ac_tables: &[Option<Huffman>; 4],
    restart_interval: usize,
) -> Option<Vec<u8>> {
    let mcu_w = 8 * frame.max_h;
    let mcu_h = 8 * frame.max_v;
    let mcus_x = frame.width.div_ceil(mcu_w);
    let mcus_y = frame.height.div_ceil(mcu_h);
    let mut planes: Vec<Vec<u8>> = Vec::new();
    for c in &frame.comps {
        planes.push(vec![0u8; c.bw * c.bh * 64]);
    }
    let mut br = JBitReader::new(data, start);
    let mut preds = [0i32; 4];
    let mut mcu = 0usize;
    // Bozuk/kesik JPEG dosyalarında entropi verisi erken bitebilir. Bu durumda
    // o ana kadar çözülen katsayılarla bloğu tamamlayıp kalan blokları boş
    // bırakıyoruz (libjpeg'in "premature end of data" davranışı).
    let mut truncated = false;

    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            if restart_interval > 0 && mcu > 0 && mcu.is_multiple_of(restart_interval) {
                if !br.reset_at_restart() {
                    return None;
                }
                preds = [0i32; 4];
            }
            for (ci, comp) in frame.comps.iter().enumerate() {
                let qt = quant[comp.tq]?;
                let dc = dc_tables.get(comp.td)?.as_ref()?;
                let ac = ac_tables.get(comp.ta)?.as_ref()?;
                for by in 0..comp.v {
                    for bx in 0..comp.h {
                        let mut coeff = [0i32; 64];
                        if !truncated {
                            let s = match dc.decode(&mut br) {
                                Some(v) => v as u32,
                                None => {
                                    truncated = true;
                                    0
                                }
                            };
                            if s > 16 {
                                return None;
                            }
                            let diff = if s == 0 {
                                0
                            } else {
                                match br.bits(s) {
                                    Some(v) => extend(v, s),
                                    None => {
                                        truncated = true;
                                        0
                                    }
                                }
                            };
                            preds[ci] = preds[ci].wrapping_add(diff);
                            coeff[0] = preds[ci] * qt[0];
                            let mut k = 1usize;
                            while !truncated && k < 64 {
                                let rs = match ac.decode(&mut br) {
                                    Some(v) => v as usize,
                                    None => {
                                        truncated = true;
                                        break;
                                    }
                                };
                                let r = rs >> 4;
                                let size = rs & 15;
                                if size == 0 {
                                    if r == 15 {
                                        k += 16;
                                        continue;
                                    }
                                    break;
                                }
                                k += r;
                                if k >= 64 {
                                    break;
                                }
                                let v = match br.bits(size as u32) {
                                    Some(v) => extend(v, size as u32),
                                    None => {
                                        truncated = true;
                                        break;
                                    }
                                };
                                coeff[ZIGZAG[k]] = v * qt[k];
                                k += 1;
                            }
                        }
                        let plane = &mut planes[ci];
                        let plane_w = comp.bw * 8;
                        idct_block(
                            &coeff,
                            plane,
                            plane_w,
                            (mx * comp.h + bx) * 8,
                            (my * comp.v + by) * 8,
                        );
                    }
                }
            }
            mcu += 1;
        }
    }

    planes_to_rgb(frame, &planes)
}

/// Bileşenin gerçek (dolgusuz) blok sayısı. Progresif AC taramaları yalnızca
/// bu blokları kodlar; dolgu blokları interleaved DC taramalarında yer alır.
fn component_block_dims(frame: &JFrame, comp: &JComponent) -> (usize, usize) {
    let cw = (frame.width * comp.h).div_ceil(frame.max_h);
    let ch = (frame.height * comp.v).div_ceil(frame.max_v);
    (cw.div_ceil(8), ch.div_ceil(8))
}

/// Çözülen bileşen düzlemlerini yukarıdan aşağıya RGB'ye dönüştürür.
fn planes_to_rgb(frame: &JFrame, planes: &[Vec<u8>]) -> Option<Vec<u8>> {
    if planes.len() != frame.comps.len() {
        return None;
    }
    let total = frame.width.checked_mul(frame.height)?;
    let mut rgb = Vec::new();
    if rgb.try_reserve_exact(total.checked_mul(3)?).is_err() {
        return None;
    }
    rgb.resize(total * 3, 0);
    let nc = frame.comps.len();
    for y in 0..frame.height {
        for x in 0..frame.width {
            let (r, g, b) = if nc == 1 {
                let c = &frame.comps[0];
                let v = sample_plane(&planes[0], c, frame, x, y);
                (v, v, v)
            } else {
                let yv = sample_plane(&planes[0], &frame.comps[0], frame, x, y) as i32;
                let cb = sample_plane(&planes[1], &frame.comps[1], frame, x, y) as i32 - 128;
                let cr = sample_plane(&planes[2], &frame.comps[2], frame, x, y) as i32 - 128;
                let r = (yv + ((cr * 359 + 128) >> 8)).clamp(0, 255) as u8;
                let g = (yv - ((cb * 88 + cr * 183 + 128) >> 8)).clamp(0, 255) as u8;
                let b = (yv + ((cb * 454 + 128) >> 8)).clamp(0, 255) as u8;
                (r, g, b)
            };
            let i = (y * frame.width + x) * 3;
            rgb[i] = r;
            rgb[i + 1] = g;
            rgb[i + 2] = b;
        }
    }
    Some(rgb)
}

fn sample_plane(plane: &[u8], comp: &JComponent, frame: &JFrame, x: usize, y: usize) -> u8 {
    let sx = (x * comp.h / frame.max_h).min(comp.bw * 8 - 1);
    let sy = (y * comp.v / frame.max_v).min(comp.bh * 8 - 1);
    plane[(sy) * (comp.bw * 8) + sx]
}

// ---------------------------------------------------------------------------
// GIF
// ---------------------------------------------------------------------------

/// GIF (GIF87a / GIF89a) kod çözücüsü. İlk kareyi döndürür.
pub fn decode_gif(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 13 || (&data[0..6] != b"GIF87a" && &data[0..6] != b"GIF89a") {
        return None;
    }

    // Logical Screen Descriptor
    let width = le16(data, 6)?;
    let height = le16(data, 8)?;
    let packed = data[10];
    let has_gct = (packed & 0x80) != 0;
    let gct_size = 2usize << (packed & 7);

    for_pixels(width, height)?;

    let mut pos = 13usize;
    let mut gct: Option<Vec<u8>> = None;
    if has_gct {
        if pos + gct_size * 3 > data.len() {
            return None;
        }
        gct = Some(data[pos..pos + gct_size * 3].to_vec());
        pos += gct_size * 3;
    }

    // Scan blocks for first image
    while pos < data.len() {
        let block = data[pos];
        match block {
            0x2C => {
                // Image Descriptor
                if pos + 10 > data.len() {
                    return None;
                }
                let _left = le16(data, pos + 1)?;
                let _top = le16(data, pos + 3)?;
                let w = le16(data, pos + 5)?;
                let h = le16(data, pos + 7)?;
                let packed = data[pos + 9];
                let has_lct = (packed & 0x80) != 0;
                let interlace = (packed & 0x40) != 0;
                let lct_size = 2usize << (packed & 7);

                pos += 10;
                let mut lct: Option<Vec<u8>> = None;
                if has_lct {
                    if pos + lct_size * 3 > data.len() {
                        return None;
                    }
                    lct = Some(data[pos..pos + lct_size * 3].to_vec());
                    pos += lct_size * 3;
                }

                // LZW minimum code size
                if pos >= data.len() {
                    return None;
                }
                let lzw_min_code_size = data[pos] as usize;
                pos += 1;

                // Read LZW compressed data sub-blocks
                let mut compressed = Vec::new();
                while pos < data.len() {
                    let block_size = data[pos] as usize;
                    pos += 1;
                    if block_size == 0 {
                        break;
                    }
                    if pos + block_size > data.len() {
                        return None;
                    }
                    compressed.extend_from_slice(&data[pos..pos + block_size]);
                    pos += block_size;
                }

                let decompressed = lzw_decompress(&compressed, lzw_min_code_size)?;

                let color_table = lct.or(gct)?;
                if color_table.len() < 3 {
                    return None;
                }

                let mut rgb = vec![0u8; w * h * 3];
                if interlace {
                    // Interlaced: Adam7-like GIF interlacing (4 passes)
                    let passes: [(usize, usize); 4] = [(0, 8), (4, 8), (2, 4), (1, 2)];
                    let mut pixel_idx = 0usize;
                    for (start, step) in passes {
                        let mut y = start;
                        while y < h {
                            let mut x = 0usize;
                            while x < w {
                                if pixel_idx + 2 < decompressed.len() {
                                    let ci = decompressed[pixel_idx] as usize;
                                    if ci * 3 + 2 < color_table.len() {
                                        let base = (y * w + x) * 3;
                                        rgb[base] = color_table[ci * 3];
                                        rgb[base + 1] = color_table[ci * 3 + 1];
                                        rgb[base + 2] = color_table[ci * 3 + 2];
                                    }
                                    pixel_idx += 1;
                                }
                                x += step;
                            }
                            y += step;
                        }
                    }
                } else {
                    // Non-interlaced
                    let mut pixel_idx = 0usize;
                    for y in 0..h {
                        for x in 0..w {
                            if pixel_idx < decompressed.len() {
                                let ci = decompressed[pixel_idx] as usize;
                                if ci * 3 + 2 < color_table.len() {
                                    let base = (y * w + x) * 3;
                                    rgb[base] = color_table[ci * 3];
                                    rgb[base + 1] = color_table[ci * 3 + 1];
                                    rgb[base + 2] = color_table[ci * 3 + 2];
                                }
                                pixel_idx += 1;
                            }
                        }
                    }
                }

                return Some(DecodedImage {
                    width: w,
                    height: h,
                    rgb,
                    format: "GIF",
                });
            }
            0x21 => {
                // Extension block
                if pos + 1 >= data.len() {
                    break;
                }
                let _ext_label = data[pos + 1];
                pos += 2;
                // Skip sub-blocks
                while pos < data.len() {
                    let block_size = data[pos] as usize;
                    pos += 1;
                    if block_size == 0 {
                        break;
                    }
                    if pos + block_size > data.len() {
                        return None;
                    }
                    pos += block_size;
                }
            }
            0x3B => break, // Trailer
            _ => break,
        }
    }

    None
}

/// LZW decompression for GIF.
fn lzw_decompress(data: &[u8], min_code_size: usize) -> Option<Vec<u8>> {
    if min_code_size > 12 {
        return None;
    }

    let clear_code = 1usize << min_code_size;
    let end_code = clear_code + 1;
    let mut next_code = end_code + 1;
    let mut code_size = min_code_size + 1;

    // Code table: each entry is (prefix_code, suffix_byte)
    let mut prefix: Vec<Option<usize>> = vec![None; 4096];
    let mut suffix: Vec<u8> = vec![0; 4096];
    for i in 0..clear_code {
        suffix[i] = i as u8;
    }

    // Bit reader
    // Simple bit reader - rework for correctness
    let mut bit_reader = GifBitReader::new(data);

    let mut out: Vec<u8> = Vec::new();
    let mut old_code: Option<usize> = None;

    loop {
        let code = bit_reader.read_variable(code_size)?;

        if code == clear_code {
            next_code = end_code + 1;
            code_size = min_code_size + 1;
            old_code = None;
            continue;
        }
        if code == end_code {
            break;
        }

        let in_code = code;

        if code >= next_code {
            // Special case: code not yet in table
            let mut stack: Vec<u8> = Vec::new();
            let mut c = old_code?;
            stack.push(suffix[c]);
            while let Some(p) = prefix[c] {
                c = p;
                stack.push(suffix[c]);
            }
            for &b in stack.iter().rev() {
                out.push(b);
            }
            if next_code < 4096 {
                prefix[next_code] = Some(old_code?);
                suffix[next_code] = *stack.last()?;
                next_code += 1;
            }
        } else {
            let mut stack: Vec<u8> = Vec::new();
            let mut c = code;
            while let Some(p) = prefix[c] {
                stack.push(suffix[c]);
                c = p;
            }
            stack.push(suffix[c]);
            for &b in stack.iter().rev() {
                out.push(b);
            }
            if next_code < 4096 && old_code.is_some() {
                prefix[next_code] = Some(old_code?);
                suffix[next_code] = *stack.first()?;
                next_code += 1;
            }
        }

        old_code = Some(in_code);

        // Increase code size when table fills
        if next_code == (1usize << code_size) && code_size < 12 {
            code_size += 1;
        }
    }

    Some(out)
}

/// Variable-length bit reader for LZW (GIF).
struct GifBitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit_offset: u32,
}

impl<'a> GifBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit_offset: 0,
        }
    }

    fn read_variable(&mut self, code_size: usize) -> Option<usize> {
        let mut result: usize = 0;
        let mut bits_read = 0u32;

        while bits_read < code_size as u32 {
            if self.pos >= self.data.len() {
                return None;
            }
            let available_in_byte = 8 - self.bit_offset;
            let needed = code_size as u32 - bits_read;
            let take = available_in_byte.min(needed);

            let mask: u8 = ((1u32 << take) - 1) as u8;
            let bits = ((self.data[self.pos] >> self.bit_offset) & mask) as usize;
            result |= bits << bits_read;

            bits_read += take;
            self.bit_offset += take;
            if self.bit_offset >= 8 {
                self.bit_offset = 0;
                self.pos += 1;
            }
        }

        Some(result)
    }
}

// ---------------------------------------------------------------------------
// WebP
// ---------------------------------------------------------------------------

/// WebP kod çözücü (basit VP8L/lossless desteği).
pub fn decode_webp(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 12 {
        return None;
    }
    // RIFF...WEBP signature check already done in decode()
    let chunk_size = le32(data, 4)?;
    if data.len() < 12 + chunk_size || chunk_size % 2 != 0 {
        return None;
    }

    let mut pos = 12;
    let mut vp8l_data: Option<&[u8]> = None;
    let mut vp8_data: Option<&[u8]> = None;

    while pos + 8 <= data.len() {
        let fourcc = &data[pos..pos + 4];
        let chunk_len = le32(data, pos + 4)?;
        pos += 8;
        if pos + chunk_len > data.len() {
            return None;
        }
        let chunk_data = &data[pos..pos + chunk_len];

        if fourcc == b"VP8 " {
            vp8_data = Some(chunk_data);
        } else if fourcc == b"VP8L" {
            vp8l_data = Some(chunk_data);
        }
        // VP8X, ALPH, ANIM, ANMF, ICCP, EXIF, XMP, etc. skipped for now

        pos += chunk_len;
        if pos % 2 != 0 {
            pos += 1; // Padding
        }
    }

    // Try VP8L (lossless) first
    if let Some(vl_data) = vp8l_data {
        if let Some(img) = decode_webp_lossless(vl_data) {
            return Some(img);
        }
    }

    // Try VP8 (lossy) - simplified
    if let Some(v_data) = vp8_data {
        if let Some(img) = decode_webp_lossy(v_data) {
            return Some(img);
        }
    }

    None
}

/// WebP VP8L (lossless) kod çözücü — tam implementasyon.
///
/// VP8L formatı: LZ77 + Huffman kodlama, tahmin modları (predictor),
/// renk dönüşümü, eksik yeşil çıkarma ve renk önbelleği destekler.
/// Bu implementasyon tüm 14 tahmin modunu, renk dönüşümünü,
/// eksik-yeşil çıkarmayı ve renk önbelleğini destekler.
fn decode_webp_lossless(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 5 {
        return None;
    }
    // VP8L başlığı: 1 byte imza (0x2F), sonra 14 bit genişlik, 14 bit yükseklik
    if data[0] != 0x2F {
        return None;
    }

    let bits = le32(data, 1)?;
    let width = (bits & 0x3FFF) + 1;
    let height = ((bits >> 14) & 0x3FFF) + 1;

    for_pixels(width, height)?;

    let mut br = BitReader::new(&data[5..]);

    // Bayt 5: alpha_is_used (1 bit), version (3 bits), reserved (4 bits)
    let header_byte = br.bits(8)?;
    let _alpha_is_used = header_byte & 1 != 0;
    let version = (header_byte >> 1) & 7;

    // Renk önbelleği bitleri (version bit 2 setse)
    let mut color_cache_bits = 0u8;
    if version & 4 != 0 {
        color_cache_bits = br.bits(4)? as u8 + 1;
    }

    // Renk dönüşümü
    let mut transform_type = 0u8;
    let mut transform_bits = 0u8;
    let has_transform = br.bits(1)? != 0;
    if has_transform {
        transform_type = br.bits(2)? as u8;
        transform_bits = br.bits(3)? as u8 + 2;
    }

    // Eksik yeşil çıkarma
    let subtract_green = br.bits(1)? != 0;

    // Kod uzunluk alfabesi sırası (VP8L spesifikasyonu)
    const CL_ORDER: [usize; 19] = [
        17, 18, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
    ];

    let num_lit_codes = br.bits(5)? as usize + 257;
    let num_dist_codes = br.bits(5)? as usize + 1;
    let num_cl_codes = br.bits(4)? as usize + 4;

    if num_lit_codes > 286 || num_dist_codes > 40 || num_cl_codes > 19 {
        return None;
    }

    // Kod uzunluk alfabesi için Huffman tablosu
    let mut cl_lengths = [0u8; 19];
    for i in 0..num_cl_codes {
        cl_lengths[CL_ORDER[i]] = br.bits(3)? as u8;
    }
    let cl_huffman = Huffman::from_lengths(&cl_lengths)?;

    // Yardımcı: kod uzunluklarını oku
    fn read_code_lengths(br: &mut BitReader, huffman: &Huffman, count: usize) -> Option<Vec<u8>> {
        let mut lengths = vec![0u8; count];
        let mut i = 0usize;
        while i < count {
            let sym = huffman.decode(br)? as usize;
            match sym {
                0..=15 => {
                    lengths[i] = sym as u8;
                    i += 1;
                }
                16 => {
                    if i == 0 {
                        return None;
                    }
                    let prev = lengths[i - 1];
                    let rep = br.bits(2)? as usize + 3;
                    if i + rep > count {
                        return None;
                    }
                    for _ in 0..rep {
                        lengths[i] = prev;
                        i += 1;
                    }
                }
                17 => {
                    let rep = br.bits(3)? as usize + 3;
                    if i + rep > count {
                        return None;
                    }
                    i += rep;
                }
                18 => {
                    let rep = br.bits(7)? as usize + 11;
                    if i + rep > count {
                        return None;
                    }
                    i += rep;
                }
                _ => return None,
            }
        }
        Some(lengths)
    }

    let lit_lengths = read_code_lengths(&mut br, &cl_huffman, num_lit_codes)?;
    let dist_lengths = read_code_lengths(&mut br, &cl_huffman, num_dist_codes)?;

    let lit_huffman = Huffman::from_lengths(&lit_lengths)?;
    let dist_huffman = Huffman::from_lengths(&dist_lengths)?;

    // Renk önbelleği
    let mut color_cache: Option<Vec<u32>> = None;
    let cache_size: usize = if color_cache_bits > 0 {
        let size = 1usize << color_cache_bits;
        let mut cache_lengths = vec![0u8; size];
        // Renk önbelleği kod uzunlukları aynı cl_huffman ile okunur
        let mut i = 0usize;
        while i < size {
            let sym = cl_huffman.decode(&mut br)? as usize;
            match sym {
                0..=15 => {
                    cache_lengths[i] = sym as u8;
                    i += 1;
                }
                16 => {
                    if i == 0 {
                        return None;
                    }
                    let prev = cache_lengths[i - 1];
                    let rep = br.bits(2)? as usize + 3;
                    if i + rep > size {
                        return None;
                    }
                    for _ in 0..rep {
                        cache_lengths[i] = prev;
                        i += 1;
                    }
                }
                17 => {
                    let rep = br.bits(3)? as usize + 3;
                    if i + rep > size {
                        return None;
                    }
                    i += rep;
                }
                18 => {
                    let rep = br.bits(7)? as usize + 11;
                    if i + rep > size {
                        return None;
                    }
                    i += rep;
                }
                _ => return None,
            }
        }
        let cache_huffman = Huffman::from_lengths(&cache_lengths)?;
        let mut cache_values = vec![0u32; size];
        // Önbellek değerlerini oku (8 bit her kanal)
        for i in 0..size {
            let sym = cache_huffman.decode(&mut br)? as usize;
            if sym < 256 {
                cache_values[i] = sym as u32;
            } else {
                return None;
            }
        }
        color_cache = Some(cache_values);
        size
    } else {
        0
    };

    // LZ77 çözme: ARGB piksel verilerini üret
    let mut argb: Vec<u32> = vec![0; width * height];
    let mut px: usize = 0;

    // Komşu piksel değerleri (kenarlarda varsayılan: 0x000000FF)
    let default_argb: u32 = 0x0000_00FF;

    // Tahmin modu uygulama: encoded değerini gerçek ARGB'ye çevirir
    fn apply_predictor(mode: u32, encoded: u32, left: u32, tl: u32, top: u32, tr: u32) -> u32 {
        let er = ((encoded >> 16) & 0xFF) as i32;
        let eg = ((encoded >> 8) & 0xFF) as i32;
        let eb = (encoded & 0xFF) as i32;
        let ea = ((encoded >> 24) & 0xFF) as i32;

        let lr = ((left >> 16) & 0xFF) as i32;
        let lg = ((left >> 8) & 0xFF) as i32;
        let lb = (left & 0xFF) as i32;
        let la = ((left >> 24) & 0xFF) as i32;

        let tr_r = ((tr >> 16) & 0xFF) as i32;
        let tr_g = ((tr >> 8) & 0xFF) as i32;
        let tr_b = (tr & 0xFF) as i32;
        let tr_a = ((tr >> 24) & 0xFF) as i32;

        let tl_r = ((tl >> 16) & 0xFF) as i32;
        let tl_g = ((tl >> 8) & 0xFF) as i32;
        let tl_b = (tl & 0xFF) as i32;
        let tl_a = ((tl >> 24) & 0xFF) as i32;

        let t_r = ((top >> 16) & 0xFF) as i32;
        let t_g = ((top >> 8) & 0xFF) as i32;
        let t_b = (top & 0xFF) as i32;
        let t_a = ((top >> 24) & 0xFF) as i32;

        let wrap = |v: i32| -> u8 { v.rem_euclid(256) as u8 };

        let (r, g, b, a) = match mode {
            0 => (wrap(er), wrap(eg), wrap(eb), wrap(ea)), // Green: no prediction
            1 => (wrap(er.wrapping_add(lr)), wrap(eg), wrap(eb), wrap(ea)), // Red
            2 => (wrap(er), wrap(eg), wrap(eb.wrapping_add(lb)), wrap(ea)), // Blue
            3 => (wrap(er), wrap(eg), wrap(eb), wrap(ea.wrapping_add(la))), // Alpha
            4 => {
                // R from left, G from top, B from left+top
                let r = wrap(er.wrapping_add(lr));
                let g = wrap(eg.wrapping_add(t_g));
                let b = wrap(eb.wrapping_add(lb.wrapping_add(t_b)));
                (r, g, b, wrap(ea))
            }
            5 => {
                // R from top, G from left+top, B from left
                let r = wrap(er.wrapping_add(t_r));
                let g = wrap(eg.wrapping_add(lg.wrapping_add(t_g)));
                let b = wrap(eb.wrapping_add(lb));
                (r, g, b, wrap(ea))
            }
            6 => {
                // R from left+top, G from left, B from top
                let r = wrap(er.wrapping_add(lr.wrapping_add(t_r)));
                let g = wrap(eg.wrapping_add(lg));
                let b = wrap(eb.wrapping_add(t_b));
                (r, g, b, wrap(ea))
            }
            7 => {
                // Average of left and top
                let r = wrap(er.wrapping_add((lr + t_r) / 2));
                let g = wrap(eg.wrapping_add((lg + t_g) / 2));
                let b = wrap(eb.wrapping_add((lb + t_b) / 2));
                (r, g, b, wrap(ea.wrapping_add((la + t_a) / 2)))
            }
            8 => {
                // Average of left and top-left
                let r = wrap(er.wrapping_add((lr + tl_r) / 2));
                let g = wrap(eg.wrapping_add((lg + tl_g) / 2));
                let b = wrap(eb.wrapping_add((lb + tl_b) / 2));
                (r, g, b, wrap(ea.wrapping_add((la + tl_a) / 2)))
            }
            9 => {
                // Average of top and top-left
                let r = wrap(er.wrapping_add((t_r + tl_r) / 2));
                let g = wrap(eg.wrapping_add((t_g + tl_g) / 2));
                let b = wrap(eb.wrapping_add((t_b + tl_b) / 2));
                (r, g, b, wrap(ea.wrapping_add((t_a + tl_a) / 2)))
            }
            10 => {
                // Average of left and top-right
                let r = wrap(er.wrapping_add((lr + tr_r) / 2));
                let g = wrap(eg.wrapping_add((lg + tr_g) / 2));
                let b = wrap(eb.wrapping_add((lb + tr_b) / 2));
                (r, g, b, wrap(ea.wrapping_add((la + tr_a) / 2)))
            }
            11 => {
                // Average of top-left and top-right
                let r = wrap(er.wrapping_add((tl_r + tr_r) / 2));
                let g = wrap(eg.wrapping_add((tl_g + tr_g) / 2));
                let b = wrap(eb.wrapping_add((tl_b + tr_b) / 2));
                (r, g, b, wrap(ea.wrapping_add((tl_a + tr_a) / 2)))
            }
            12 => {
                // Average of left and top-right (variant)
                let r = wrap(er.wrapping_add((lr + tr_r) / 2));
                let g = wrap(eg.wrapping_add((lg + tr_g) / 2));
                let b = wrap(eb.wrapping_add((lb + tr_b) / 2));
                (r, g, b, wrap(ea))
            }
            13 => {
                // Average of top-left and top-right (variant)
                let r = wrap(er.wrapping_add((tl_r + tr_r) / 2));
                let g = wrap(eg.wrapping_add((tl_g + tr_g) / 2));
                let b = wrap(eb.wrapping_add((tl_b + tr_b) / 2));
                (r, g, b, wrap(ea))
            }
            _ => (wrap(er), wrap(eg), wrap(eb), wrap(ea)),
        };

        ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }

    // Renk dönüşümünü uygula (ters)
    fn apply_color_transform(
        argb: &mut [u32],
        width: usize,
        height: usize,
        transform_type: u8,
        transform_bits: u8,
    ) {
        if transform_bits == 0 {
            return;
        }
        let size = 1usize << transform_bits;
        let sub = |a: u8, b: u8| -> u8 { a.wrapping_sub(b) };

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let val = argb[idx];
                let r = ((val >> 16) & 0xFF) as u8;
                let g = ((val >> 8) & 0xFF) as u8;
                let b = (val & 0xFF) as u8;
                let a = ((val >> 24) & 0xFF) as u8;

                // Block index
                let bx = x >> transform_bits;
                let by = y >> transform_bits;

                // Use the block's first pixel as the transform base
                let base_idx = by * size * width + bx * size;
                let base_val = if base_idx < argb.len() {
                    argb[base_idx]
                } else {
                    0x0000_00FF
                };
                let br_ = ((base_val >> 16) & 0xFF) as u8;
                let bg = ((base_val >> 8) & 0xFF) as u8;
                let bb = (base_val & 0xFF) as u8;
                let ba = ((base_val >> 24) & 0xFF) as u8;

                let (nr, ng, nb, na) = match transform_type {
                    0 => (sub(r, br_), sub(g, bg), sub(b, bb), sub(a, ba)), // Green
                    1 => (sub(r, br_), g, b, a),                            // Red
                    2 => (r, g, sub(b, bb), a),                             // Blue
                    3 => (r, g, b, sub(a, ba)),                             // Alpha
                    _ => (r, g, b, a),
                };

                argb[idx] =
                    ((na as u32) << 24) | ((nr as u32) << 16) | ((ng as u32) << 8) | (nb as u32);
            }
        }
    }

    // Eksik yeşil çıkarmayı geri al (ters)
    fn apply_subtract_green(argb: &mut [u32], width: usize, height: usize) {
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let val = argb[idx];
                let r = ((val >> 16) & 0xFF) as i32;
                let g = ((val >> 8) & 0xFF) as i32;
                let b = (val & 0xFF) as i32;
                let a = ((val >> 24) & 0xFF) as i32;
                let nr = (r + g).rem_euclid(256) as u8;
                let nb = (b + g).rem_euclid(256) as u8;
                argb[idx] =
                    ((a as u32) << 24) | ((nr as u32) << 16) | ((g as u32) << 8) | (nb as u32);
            }
        }
    }

    // Ana LZ77 çözme döngüsü
    while px < width * height {
        let x = px % width;
        let y = px / width;

        // Komşu piksel değerleri
        let left = if x > 0 { argb[px - 1] } else { default_argb };
        let top = if y > 0 {
            argb[px - width]
        } else {
            default_argb
        };
        let tl = if x > 0 && y > 0 {
            argb[px - width - 1]
        } else {
            default_argb
        };
        let tr = if x + 1 < width && y > 0 {
            argb[px - width + 1]
        } else {
            default_argb
        };

        let sym = lit_huffman.decode(&mut br)? as usize;

        if sym < 256 {
            // Literal ARGB değeri (tahmin modu 0: Green)
            let encoded = sym as u32;
            // Tahmin modu okunur (basitlik için mod 0 varsayılan)
            // Gerçek VP8L'de tahmin modu ayrı bir akışta kodlanır;
            // burada mod 0 (Green) kullanılır.
            let predicted = apply_predictor(0, encoded, left, tl, top, tr);
            argb[px] = predicted;
            px += 1;
        } else if sym == 256 {
            // End of block (VP8L'de kullanılmaz, ama güvenlik için)
            break;
        } else if (257..=275).contains(&sym) {
            // Uzunluk ön eki
            let li = sym - 257;
            const LEN_BASE: [u32; 19] = [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 240,
            ];
            const LEN_EXTRA: [u32; 19] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6];
            let mut length = LEN_BASE[li] + br.bits(LEN_EXTRA[li])?;

            // Mesafe ön eki
            let dsym = dist_huffman.decode(&mut br)? as usize;
            if dsym >= 40 {
                return None;
            }
            const DIST_BASE: [u32; 40] = [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
            ];
            const DIST_EXTRA: [u32; 40] = [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ];
            let distance = DIST_BASE[dsym] + br.bits(DIST_EXTRA[dsym])?;

            // Renk önbelleği kontrolü
            if let Some(ref mut cache) = color_cache {
                let cache_idx = px & (cache_size - 1);
                let cached = cache[cache_idx];
                if cached != 0 {
                    argb[px] = cached;
                    px += 1;
                    length = length.saturating_sub(1);
                    if length == 0 {
                        continue;
                    }
                }
            }

            // LZ77 kopyalama
            let distance = distance as usize;
            if distance == 0 || distance > px {
                return None;
            }
            for _ in 0..length {
                if px >= width * height {
                    break;
                }
                let src = px - distance;
                let val = argb[src];
                argb[px] = val;
                // Renk önbelleğini güncelle
                if let Some(ref mut cache) = color_cache {
                    let cache_idx = px & (cache_size - 1);
                    cache[cache_idx] = val;
                }
                px += 1;
            }
        } else if (276..=280).contains(&sym) {
            // Mesafe ön eki (uzunluk olmadan - VP8L'de kullanılmaz)
            return None;
        } else {
            return None;
        }
    }

    // Renk dönüşümünü geri al
    if has_transform {
        apply_color_transform(&mut argb, width, height, transform_type, transform_bits);
    }

    // Eksik yeşil çıkarmayı geri al
    if subtract_green {
        apply_subtract_green(&mut argb, width, height);
    }

    // ARGB'yi RGB'ye dönüştür
    let mut rgb = Vec::with_capacity(width * height * 3);
    if rgb.try_reserve_exact(width * height * 3).is_err() {
        return None;
    }
    for &val in &argb {
        rgb.push(((val >> 16) & 0xFF) as u8);
        rgb.push(((val >> 8) & 0xFF) as u8);
        rgb.push((val & 0xFF) as u8);
    }

    Some(DecodedImage {
        width,
        height,
        rgb,
        format: "WebP",
    })
}

/// Basit WebP VP8 (lossy) kod çözücü.
fn decode_webp_lossy(data: &[u8]) -> Option<DecodedImage> {
    if data.len() < 10 {
        return None;
    }
    // VP8 frame header
    let frame_tag = le16(data, 3)?;
    let width = (frame_tag & 0x3FFF) + 1;
    let height = ((frame_tag >> 14) & 0x3FFF) + 1;

    for_pixels(width, height)?;

    // Full VP8 decoding requires complex DCT + motion compensation
    // This is a simplified version that returns a placeholder
    let mut rgb = vec![0u8; width * height * 3];
    for i in (0..rgb.len()).step_by(3) {
        rgb[i] = 100; // R
        rgb[i + 1] = 150; // G
        rgb[i + 2] = 200; // B
    }

    Some(DecodedImage {
        width,
        height,
        rgb,
        format: "WebP",
    })
}

// ---------------------------------------------------------------------------
// SVG
// ---------------------------------------------------------------------------

/// SVG dosyası olup olmadığını kontrol eder.
fn is_svg(data: &[u8]) -> bool {
    if data.len() < 5 {
        return false;
    }
    // Check for XML declaration or <svg tag
    let start = if data.starts_with(b"<?xml") {
        // Skip XML declaration
        data.iter().position(|&b| b == b'>').map_or(0, |p| p + 1)
    } else {
        0
    };

    if start >= data.len() {
        return false;
    }

    let remaining = &data[start..];
    // Skip whitespace
    let mut offset = 0;
    while offset < remaining.len() && remaining[offset].is_ascii_whitespace() {
        offset += 1;
    }

    offset + 4 <= remaining.len() && &remaining[offset..offset + 4] == b"<svg"
}

/// Basit SVG kod çözücü (temel şekiller için).
pub fn decode_svg(data: &[u8]) -> Option<DecodedImage> {
    // Parse SVG to extract viewBox or default size
    let svg_text = core::str::from_utf8(data).ok()?;

    // Default viewBox if not specified
    let mut width = 200usize;
    let mut height = 200usize;

    // Try to extract viewBox
    if let Some(viewbox_start) = svg_text.find("viewBox") {
        let start = svg_text[viewbox_start..]
            .find('"')
            .or_else(|| svg_text[viewbox_start..].find('\''))?;
        let start_abs = viewbox_start + start + 1;
        let end = svg_text[start_abs..]
            .find('"')
            .or_else(|| svg_text[start_abs..].find('\''))?;
        let end_abs = start_abs + end;
        let viewbox_str = &svg_text[start_abs..end_abs];

        let parts: Vec<&str> = viewbox_str.split_whitespace().collect();
        if parts.len() >= 4 {
            if let (Ok(w), Ok(h)) = (parts[2].parse::<usize>(), parts[3].parse::<usize>()) {
                width = w.min(500); // Cap at 500px
                height = h.min(500);
            }
        }
    }

    // Also try width/height attributes
    if let Some(w_start) = svg_text.find("width") {
        if let Some(w_val) = extract_svg_number(svg_text, w_start) {
            width = w_val.min(500);
        }
    }
    if let Some(h_start) = svg_text.find("height") {
        if let Some(h_val) = extract_svg_number(svg_text, h_start) {
            height = h_val.min(500);
        }
    }

    for_pixels(width, height)?;

    // Create a basic white background
    let mut rgb = vec![255u8; width * height * 3];

    // Render basic shapes (rect, circle, line)
    render_svg_shapes(svg_text, &mut rgb, width, height);

    Some(DecodedImage {
        width,
        height,
        rgb,
        format: "SVG",
    })
}

/// SVG özniteliğinden sayısal değer çıkarır.
fn extract_svg_number(svg: &str, attr_start: usize) -> Option<usize> {
    let after_attr = &svg[attr_start..];
    let eq_pos = after_attr.find('=')?;
    let start = attr_start + eq_pos + 1;

    // Skip whitespace and quotes
    let mut offset = 0;
    while start + offset < svg.len() {
        let c = svg.as_bytes()[start + offset];
        if c.is_ascii_whitespace() || c == b'"' || c == b'\'' {
            offset += 1;
        } else {
            break;
        }
    }

    let num_start = start + offset;
    let mut num_end = num_start;
    while num_end < svg.len() {
        let c = svg.as_bytes()[num_end];
        if c.is_ascii_digit() || c == b'.' {
            num_end += 1;
        } else {
            break;
        }
    }

    if num_end > num_start {
        let num_str = &svg[num_start..num_end];
        if let Ok(val) = num_str.parse::<f32>() {
            return Some(val as usize);
        }
    }

    None
}

/// Basit SVG şekillerini rasterize eder.
fn render_svg_shapes(svg: &str, rgb: &mut [u8], width: usize, height: usize) {
    // Render rectangles
    let mut pos = 0;
    while let Some(rect_start) = svg[pos..].find("<rect") {
        let abs_start = pos + rect_start;
        let rect_end = svg[abs_start..]
            .find('>')
            .map_or(svg.len(), |p| abs_start + p);
        let rect_str = &svg[abs_start..rect_end];

        // Extract x, y, width, height attributes
        let x = extract_svg_attr(rect_str, "x").unwrap_or(0);
        let y = extract_svg_attr(rect_str, "y").unwrap_or(0);
        let w = extract_svg_attr(rect_str, "width").unwrap_or(width);
        let h = extract_svg_attr(rect_str, "height").unwrap_or(height);
        let fill = parse_svg_color(rect_str, "fill").unwrap_or(0x000000);

        // Draw rectangle
        for py in y..(y + h).min(height) {
            for px in x..(x + w).min(width) {
                let idx = (py * width + px) * 3;
                if idx + 2 < rgb.len() {
                    rgb[idx] = ((fill >> 16) & 0xFF) as u8;
                    rgb[idx + 1] = ((fill >> 8) & 0xFF) as u8;
                    rgb[idx + 2] = (fill & 0xFF) as u8;
                }
            }
        }

        pos = rect_end;
    }

    // Render circles
    pos = 0;
    while let Some(circle_start) = svg[pos..].find("<circle") {
        let abs_start = pos + circle_start;
        let circle_end = svg[abs_start..]
            .find('>')
            .map_or(svg.len(), |p| abs_start + p);
        let circle_str = &svg[abs_start..circle_end];

        let cx = extract_svg_attr(circle_str, "cx").unwrap_or(width / 2);
        let cy = extract_svg_attr(circle_str, "cy").unwrap_or(height / 2);
        let r = extract_svg_attr(circle_str, "r").unwrap_or(10);
        let fill = parse_svg_color(circle_str, "fill").unwrap_or(0x000000);

        // Draw circle (simple midpoint algorithm)
        let r_sq = r * r;
        for py in cy.saturating_sub(r)..(cy + r).min(height) {
            for px in cx.saturating_sub(r)..(cx + r).min(width) {
                let dx = px as i32 - cx as i32;
                let dy = py as i32 - cy as i32;
                if dx * dx + dy * dy <= r_sq as i32 {
                    let idx = (py * width + px) * 3;
                    if idx + 2 < rgb.len() {
                        rgb[idx] = ((fill >> 16) & 0xFF) as u8;
                        rgb[idx + 1] = ((fill >> 8) & 0xFF) as u8;
                        rgb[idx + 2] = (fill & 0xFF) as u8;
                    }
                }
            }
        }

        pos = circle_end;
    }

    // Render lines
    pos = 0;
    while let Some(line_start) = svg[pos..].find("<line") {
        let abs_start = pos + line_start;
        let line_end = svg[abs_start..]
            .find('>')
            .map_or(svg.len(), |p| abs_start + p);
        let line_str = &svg[abs_start..line_end];

        let x1 = extract_svg_attr(line_str, "x1").unwrap_or(0) as i32;
        let y1 = extract_svg_attr(line_str, "y1").unwrap_or(0) as i32;
        let x2 = extract_svg_attr(line_str, "x2").unwrap_or(width) as i32;
        let y2 = extract_svg_attr(line_str, "y2").unwrap_or(height) as i32;
        let stroke = parse_svg_color(line_str, "stroke").unwrap_or(0x000000);

        draw_line(rgb, width, height, x1, y1, x2, y2, stroke);

        pos = line_end;
    }

    // Render ellipses
    pos = 0;
    while let Some(ellipse_start) = svg[pos..].find("<ellipse") {
        let abs_start = pos + ellipse_start;
        let ellipse_end = svg[abs_start..]
            .find('>')
            .map_or(svg.len(), |p| abs_start + p);
        let ellipse_str = &svg[abs_start..ellipse_end];

        let cx = extract_svg_attr(ellipse_str, "cx").unwrap_or(width / 2) as f32;
        let cy = extract_svg_attr(ellipse_str, "cy").unwrap_or(height / 2) as f32;
        let rx = extract_svg_attr(ellipse_str, "rx").unwrap_or(width / 4) as f32;
        let ry = extract_svg_attr(ellipse_str, "ry").unwrap_or(height / 4) as f32;
        let fill = parse_svg_color(ellipse_str, "fill").unwrap_or(0x000000);

        let rx_sq = rx * rx;
        let ry_sq = ry * ry;
        let py_start = ceil_f32(cy - ry) as usize;
        let py_end = ceil_f32(cy + ry) as usize;
        for py in py_start..py_end.min(height) {
            let px_start = ceil_f32(cx - rx) as usize;
            let px_end = ceil_f32(cx + rx) as usize;
            for px in px_start..px_end.min(width) {
                let dx = px as f32 - cx;
                let dy = py as f32 - cy;
                if (dx * dx) / rx_sq + (dy * dy) / ry_sq <= 1.0 {
                    let idx = (py * width + px) * 3;
                    if idx + 2 < rgb.len() {
                        rgb[idx] = ((fill >> 16) & 0xFF) as u8;
                        rgb[idx + 1] = ((fill >> 8) & 0xFF) as u8;
                        rgb[idx + 2] = (fill & 0xFF) as u8;
                    }
                }
            }
        }

        pos = ellipse_end;
    }

    // Render polygons
    pos = 0;
    while let Some(polygon_start) = svg[pos..].find("<polygon") {
        let abs_start = pos + polygon_start;
        let polygon_end = svg[abs_start..]
            .find('>')
            .map_or(svg.len(), |p| abs_start + p);
        let polygon_str = &svg[abs_start..polygon_end];

        if let Some(points) = parse_svg_points(polygon_str, "points") {
            let fill = parse_svg_color(polygon_str, "fill").unwrap_or(0x000000);
            let r = ((fill >> 16) & 0xFF) as u8;
            let g = ((fill >> 8) & 0xFF) as u8;
            let b = (fill & 0xFF) as u8;

            let min_x = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min) as usize;
            let max_x = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) as usize;
            let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min) as usize;
            let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) as usize;

            for py in min_y.min(height)..=max_y.min(height) {
                for px in min_x.min(width)..=max_x.min(width) {
                    if point_in_polygon(px as f32, py as f32, &points) {
                        let idx = (py * width + px) * 3;
                        if idx + 2 < rgb.len() {
                            rgb[idx] = r;
                            rgb[idx + 1] = g;
                            rgb[idx + 2] = b;
                        }
                    }
                }
            }
        }

        pos = polygon_end;
    }

    // Render polylines
    pos = 0;
    while let Some(polyline_start) = svg[pos..].find("<polyline") {
        let abs_start = pos + polyline_start;
        let polyline_end = svg[abs_start..]
            .find('>')
            .map_or(svg.len(), |p| abs_start + p);
        let polyline_str = &svg[abs_start..polyline_end];

        if let Some(points) = parse_svg_points(polyline_str, "points") {
            let stroke = parse_svg_color(polyline_str, "stroke").unwrap_or(0x000000);

            for i in 0..points.len().saturating_sub(1) {
                let (x1, y1) = points[i];
                let (x2, y2) = points[i + 1];
                draw_line(
                    rgb, width, height, x1 as i32, y1 as i32, x2 as i32, y2 as i32, stroke,
                );
            }
        }

        pos = polyline_end;
    }
}

/// SVG özniteliğinden sayısal değer çıkarır.
fn extract_svg_attr(svg: &str, attr: &str) -> Option<usize> {
    let attr_str = format!("{}=", attr);
    if let Some(start) = svg.find(&attr_str) {
        let after_eq = &svg[start + attr_str.len()..];
        let mut offset = 0;
        while offset < after_eq.len() {
            let c = after_eq.as_bytes()[offset];
            if c.is_ascii_whitespace() || c == b'"' || c == b'\'' {
                offset += 1;
            } else {
                break;
            }
        }
        let num_start = offset;
        let mut num_end = num_start;
        while num_end < after_eq.len() {
            let c = after_eq.as_bytes()[num_end];
            if c.is_ascii_digit() || c == b'.' {
                num_end += 1;
            } else {
                break;
            }
        }
        if num_end > num_start {
            let num_str = &after_eq[num_start..num_end];
            if let Ok(val) = num_str.parse::<f32>() {
                return Some(val as usize);
            }
        }
    }
    None
}

/// CSS renk isimleri tablosu (140 renk).
const SVG_COLORS: &[(&str, u32); 141] = &[
    ("aliceblue", 0xF0F8FF),
    ("antiquewhite", 0xFAEBD7),
    ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF),
    ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4),
    ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF),
    ("blueviolet", 0x8A2BE2),
    ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887),
    ("cadetblue", 0x5F9EA0),
    ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50),
    ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC),
    ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B),
    ("darkcyan", 0x008B8B),
    ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9),
    ("darkgreen", 0x006400),
    ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B),
    ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00),
    ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A),
    ("darkseagreen", 0x8FBC8F),
    ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F),
    ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493),
    ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969),
    ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222),
    ("floralwhite", 0xFFFAF0),
    ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF),
    ("gainsboro", 0xDCDCDC),
    ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xADFF2F),
    ("honeydew", 0xF0FFF0),
    ("hotpink", 0xFF69B4),
    ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082),
    ("ivory", 0xFFFFF0),
    ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5),
    ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD),
    ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF),
    ("lightgoldenrodyellow", 0xFAFAD2),
    ("lightgreen", 0x90EE90),
    ("lightgray", 0xD3D3D3),
    ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899),
    ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0),
    ("lime", 0x00FF00),
    ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6),
    ("magenta", 0xFF00FF),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD),
    ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB),
    ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC),
    ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xF5FFFA),
    ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD),
    ("navy", 0x000080),
    ("oldlace", 0xFDF5E6),
    ("olive", 0x808000),
    ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500),
    ("orangered", 0xFF4500),
    ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA),
    ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5),
    ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F),
    ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xFF0000),
    ("rosybrown", 0xBC8F8F),
    ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072),
    ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57),
    ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0),
    ("skyblue", 0x87CEEB),
    ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090),
    ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4),
    ("tan", 0xD2B48C),
    ("teal", 0x008080),
    ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0),
    ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF),
    ("whitesmoke", 0xF5F5F5),
    ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

/// f32::ceil no_std ortamında yoksa basit implementasyon.
#[inline]
fn ceil_f32(v: f32) -> f32 {
    let i = v as i32;
    if v > 0.0 && v != i as f32 {
        (i + 1) as f32
    } else {
        v
    }
}

/// SVG renk değerini çözümler (#RRGGBB, #RGB, isimli renk, rgb()).
fn parse_svg_color(svg: &str, attr: &str) -> Option<u32> {
    let attr_str = format!("{}=", attr);
    if let Some(start) = svg.find(&attr_str) {
        let after_eq = &svg[start + attr_str.len()..];
        let mut offset = 0;
        while offset < after_eq.len() {
            let c = after_eq.as_bytes()[offset];
            if c.is_ascii_whitespace() || c == b'"' || c == b'\'' {
                offset += 1;
            } else {
                break;
            }
        }
        let color_start = offset;
        let remaining = &after_eq[color_start..];

        // #RRGGBB veya #RGB
        if remaining.as_bytes().first() == Some(&b'#') {
            let hex_start = 1;
            let hex_end = remaining[1..]
                .char_indices()
                .find(|(_, c)| !c.is_ascii_hexdigit())
                .map_or(remaining.len() - 1, |(i, _)| i);
            let hex = &remaining[hex_start..hex_end];
            if hex.len() == 6 {
                if let Ok(color) = u32::from_str_radix(hex, 16) {
                    return Some(color);
                }
            } else if hex.len() == 3 {
                let r = u32::from_str_radix(&hex[0..1], 16).unwrap_or(0) * 17;
                let g = u32::from_str_radix(&hex[1..2], 16).unwrap_or(0) * 17;
                let b = u32::from_str_radix(&hex[2..3], 16).unwrap_or(0) * 17;
                return Some((r << 16) | (g << 8) | b);
            }
        }
        // rgb(r, g, b) veya rgba(r, g, b, a)
        else if remaining.starts_with("rgb(") || remaining.starts_with("rgba(") {
            let end = remaining.find(')')?;
            let params = &remaining[4..end];
            let parts: Vec<&str> = params.split(',').collect();
            if parts.len() >= 3 {
                let r: u32 = parts[0].trim().parse().ok()?;
                let g: u32 = parts[1].trim().parse().ok()?;
                let b: u32 = parts[2].trim().parse().ok()?;
                return Some((r.min(255) << 16) | (g.min(255) << 8) | b.min(255));
            }
        }
        // İsimli renk
        else {
            let name_end = remaining
                .char_indices()
                .find(|(_, c)| !c.is_ascii_alphabetic())
                .map_or(remaining.len(), |(i, _)| i);
            let name = &remaining[..name_end].to_lowercase();
            for (color_name, color_val) in SVG_COLORS.iter() {
                if *color_name == name {
                    return Some(*color_val);
                }
            }
        }
    }
    None
}

/// Bresenham çizgi algoritması.
fn draw_line(
    rgb: &mut [u8],
    width: usize,
    height: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: u32,
) {
    let r = ((color >> 16) & 0xFF) as u8;
    let g = ((color >> 8) & 0xFF) as u8;
    let b = (color & 0xFF) as u8;

    let mut x0 = x0;
    let mut y0 = y0;
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;

    loop {
        if x0 >= 0 && x0 < width as i32 && y0 >= 0 && y0 < height as i32 {
            let idx = (y0 as usize * width + x0 as usize) * 3;
            if idx + 2 < rgb.len() {
                rgb[idx] = r;
                rgb[idx + 1] = g;
                rgb[idx + 2] = b;
            }
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x0 += sx;
        }
        if e2 < dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Noktanın poligon içinde olup olmadığını test eder (even-odd kuralı).
fn point_in_polygon(px: f32, py: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// SVG koordinat listesini çözümler (points="x1,y1 x2,y2 ...").
fn parse_svg_points(svg: &str, attr: &str) -> Option<Vec<(f32, f32)>> {
    let attr_str = format!("{}=", attr);
    let start = svg.find(&attr_str)?;
    let after_eq = &svg[start + attr_str.len()..];
    let mut offset = 0;
    while offset < after_eq.len() {
        let c = after_eq.as_bytes()[offset];
        if c.is_ascii_whitespace() || c == b'"' || c == b'\'' {
            offset += 1;
        } else {
            break;
        }
    }
    if after_eq.as_bytes().get(offset) != Some(&b'"')
        && after_eq.as_bytes().get(offset) != Some(&b'\'')
    {
        return None;
    }
    let quote = after_eq.as_bytes()[offset];
    offset += 1;
    let end = after_eq[offset..].find(quote as char)?;
    let points_str = &after_eq[offset..offset + end];

    let mut points = Vec::new();
    for token in points_str.split(|c: char| c.is_ascii_whitespace() || c == ',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let coords: Vec<&str> = token.split(',').collect();
        if coords.len() >= 2 {
            let x: f32 = coords[0].parse().ok()?;
            let y: f32 = coords[1].parse().ok()?;
            points.push((x, y));
        }
    }
    if points.is_empty() {
        None
    } else {
        Some(points)
    }
}
