//! PNG ve JPEG (baseline) kod çözücüleri.
//!
//! Bu modül çekirdekten bağımsızdır: yalnızca `alloc` kullanır ve host
//! testlerinde doğrudan derlenebilir. Çözülen görüntü her zaman yukarıdan
//! aşağıya, 8 bit RGB olarak tutulur. Bellek tüketimini sınırlamak için
//! çok büyük görüntüler reddedilir.

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
    Some(((d[i] as usize) << 24) | ((d[i + 1] as usize) << 16) | ((d[i + 2] as usize) << 8) | d[i + 3] as usize)
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
    Some(PngHeader { width, height, bit_depth, color_type, interlace })
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
            b"IHDR" | b"tRNS" | b"gAMA" | b"cHRM" | b"sRGB" | b"pHYs" | b"tEXt" | b"iTXt" | b"zTXt" | b"bKGD" | b"hIST" | b"sBIT" => {}
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
    let bpp = (bits_per_pixel + 7) / 8;
    let stride = (hdr.width.checked_mul(bits_per_pixel)? + 7) / 8;
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
                    let g = if depth == 16 { g } else { g };
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
    Some(DecodedImage { width: hdr.width, height: hdr.height, rgb, format: "PNG" })
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
        let prev_start = if y > 0 { Some((y - 1) * (stride + 1) + 1) } else { None };
        for x in 0..stride {
            let a = if x >= bpp { raw[row_start + x - bpp] } else { 0 };
            let b = prev_start.map(|p| raw[p + x]).unwrap_or(0);
            let c = if x >= bpp { prev_start.map(|p| raw[p + x - bpp]).unwrap_or(0) } else { 0 };
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
    if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
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
        Self { data, pos: 0, buf: 0, cnt: 0 }
    }

    fn fill(&mut self) -> bool {
        if self.pos >= self.data.len() {
            return false;
        }
        self.buf = (self.buf << 8) | self.data[self.pos] as u32;
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
        self.cnt -= n;
        Some((self.buf >> self.cnt) & ((1u32 << n) - 1))
    }

    fn align_byte(&mut self) {
        self.cnt -= self.cnt % 8;
    }
}

struct Huffman {
    counts: [u16; 16],
    first: [u32; 17],
    offset: [u32; 16],
    symbols: Vec<u8>,
}

trait BitReaderLike {
    fn bits(&mut self, n: u32) -> Option<u32>;
}

impl BitReaderLike for BitReader<'_> {
    fn bits(&mut self, n: u32) -> Option<u32> { BitReader::bits(self, n) }
}

impl BitReaderLike for JBitReader<'_> {
    fn bits(&mut self, n: u32) -> Option<u32> { JBitReader::bits(self, n) }
}

impl Huffman {
    fn from_lengths(lengths: &[u8]) -> Option<Self> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l as usize > 15 {
                return None;
            }
            if l != 0 {
                counts[l as usize - 1] += 1;
            }
        }
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
        let mut symbols = vec![0u8; total as usize];
        let mut next = [0u32; 16];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let li = l as usize - 1;
                let idx = offset[li] + next[li];
                next[li] += 1;
                if idx as usize >= symbols.len() {
                    return None;
                }
                symbols[idx as usize] = sym as u8;
            }
        }
        Some(Self { counts, first, offset, symbols })
    }

    fn decode(&self, br: &mut impl BitReaderLike) -> Option<u8> {
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
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
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
                lengths[i] = sym;
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
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
    131, 163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
    13, 13,
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
                let (lit, dist) = if btype == 1 { fixed_tables()? } else { dynamic_tables(&mut br)? };
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
                        let distance = DIST_BASE[dsym] as usize + br.bits(DIST_EXTRA[dsym] as u32)? as usize;
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
    plane: Vec<u8>,
}

struct JFrame {
    width: usize,
    height: usize,
    comps: Vec<JComponent>,
    max_h: usize,
    max_v: usize,
}

const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27,
    20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

struct JBitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl<'a> JBitReader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos, buf: 0, cnt: 0 }
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

fn extend(v: u32, s: u32) -> i32 {
    if s == 0 {
        0
    } else if v < (1u32 << (s - 1)) {
        v as i32 - ((1i32 << s) - 1)
    } else {
        v as i32
    }
}

fn idct_1d(s: [i32; 8]) -> [i32; 8] {
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
    [x0 + t3, x1 + t2, x2 + t1, x3 + t0, x3 - t0, x2 - t1, x1 - t2, x0 - t3]
}

fn idct_block(coeff: &[i32; 64], out: &mut [u8], out_w: usize, ox: usize, oy: usize) {
    let mut tmp = [0i32; 64];
    for col in 0..8 {
        let s = [
            coeff[col], coeff[8 + col], coeff[16 + col], coeff[24 + col],
            coeff[32 + col], coeff[40 + col], coeff[48 + col], coeff[56 + col],
        ];
        let v = idct_1d(s);
        for row in 0..8 {
            tmp[row * 8 + col] = (v[row] + 512) >> 10;
        }
    }
    for row in 0..8 {
        let mut s = [0i32; 8];
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
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len - 2 > data.len() {
                    return None;
                }
                let seg = &data[pos..pos + seg_len - 2];
                pos += seg_len - 2;
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
                let rgb = decode_jpeg_scan(data, pos, &fr, &quant, &dc_tables, &ac_tables, restart_interval)?;
                return Some(DecodedImage { width: fr.width, height: fr.height, rgb, format: "JPEG" });
            }
            0xDB => {
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len - 2 > data.len() {
                    return None;
                }
                let seg = &data[pos..pos + seg_len - 2];
                pos += seg_len - 2;
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
                if seg_len < 2 || pos + seg_len - 2 > data.len() {
                    return None;
                }
                let seg = &data[pos..pos + seg_len - 2];
                pos += seg_len - 2;
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
                    let mut lengths = Vec::new();
                    for (len, &count) in counts.iter().enumerate() {
                        for _ in 0..count {
                            lengths.push(len as u8 + 1);
                        }
                    }
                    let table = Huffman::from_lengths(&lengths)?;
                    let _ = &mut lengths;
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
                if seg_len < 2 || pos + seg_len - 2 > data.len() {
                    return None;
                }
                let seg = &data[pos..pos + seg_len - 2];
                pos += seg_len - 2;
                if frame.is_some() || seg.len() < 6 || seg[0] != 8 {
                    return None;
                }
                let height = be16(seg, 1)?;
                let width = be16(seg, 3)?;
                let n = seg[5] as usize;
                if n == 0 || n > 4 || seg.len() < 6 + n * 3 {
                    return None;
                }
                if for_pixels(width, height).is_none() {
                    return None;
                }
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
                    comps.push(JComponent { id, h, v, tq, td: 0, ta: 0, bw: 0, bh: 0, plane: Vec::new() });
                }
                let mcu_w = 8 * max_h;
                let mcu_h = 8 * max_v;
                let mcus_x = (width + mcu_w - 1) / mcu_w;
                let mcus_y = (height + mcu_h - 1) / mcu_h;
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
                frame = Some(JFrame { width, height, comps, max_h, max_v });
            }
            0xDD => {
                let seg_len = be16(data, pos)?;
                if seg_len != 4 || pos + 2 > data.len() {
                    return None;
                }
                restart_interval = be16(data, pos + 2)?;
                pos += seg_len - 2;
            }
            0xC2 | 0xC3 | 0xC5..=0xCF => {
                // Aşamalı (progressive) ve aritmetik JPEG desteklenmiyor.
                return None;
            }
            _ => {
                let seg_len = be16(data, pos)?;
                if seg_len < 2 || pos + seg_len - 2 > data.len() {
                    return None;
                }
                pos += seg_len - 2;
            }
        }
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
    let mcus_x = (frame.width + mcu_w - 1) / mcu_w;
    let mcus_y = (frame.height + mcu_h - 1) / mcu_h;
    let total = frame.width.checked_mul(frame.height)?;
    let mut planes: Vec<Vec<u8>> = Vec::new();
    for c in &frame.comps {
        planes.push(vec![0u8; c.plane.len()]);
    }
    let mut br = JBitReader::new(data, start);
    let mut preds = [0i32; 4];
    let mut mcu = 0usize;

    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            if restart_interval > 0 && mcu > 0 && mcu % restart_interval == 0 {
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
                        let s = dc.decode(&mut br)? as u32;
                        if s > 16 {
                            return None;
                        }
                        let diff = extend(br.bits(s)?, s);
                        preds[ci] = preds[ci].wrapping_add(diff);
                        coeff[0] = preds[ci] * qt[0];
                        let mut k = 1usize;
                        while k < 64 {
                            let rs = ac.decode(&mut br)? as usize;
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
                            let v = extend(br.bits(size as u32)?, size as u32);
                            coeff[ZIGZAG[k]] = v * qt[k];
                            k += 1;
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

    let mut rgb = Vec::new();
    if rgb.try_reserve_exact(total * 3).is_err() {
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
