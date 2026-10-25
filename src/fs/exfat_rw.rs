//! exFAT gerçek okuma/yazma sürücüsü (device-agnostic, `no_std`).
//!
//! `src/fs/exfat.rs` biçimlendiricisinin ürettiği yapıyı (ve başka araçlarla
//! hazırlanmış düz exFAT bölümlerini) okur/yazar. Blok aygıtı `BlockDevice`
//! arayüzünden gelir, böylece hem ATA hem bellek içi aygıtla çalışır ve host
//! testlerinde uçtan uca doğrulanır.
//!
//! Desteklenen: kök dizin okuma/yazma, alt dizin açma, dosya oluşturma,
//! tek seferde yazma, okuma, silme (küme zinciri FAT üzerinden çözülür).
//!
//! Sınırlar: uzun adlı (dizi hâlinde) dosyalar da desteklenir ama ad aramada
//! sadece ilk 255 UTF-16 birimine bakılır; Timestamp/ACL girdileri
//! yazılmaz; fragmented (parçalı) dosya yazımı desteklenmez (yeni yazımda
//! kümeler her zaman FAT zinciri olarak birbirine bağlanır).

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

use super::block::{BlockDevice, SECTOR};

const ENTRY_SIZE: usize = 32;
const DIR_ENTRY_END: u8 = 0x00;
const ENTRY_FILE: u8 = 0x85; // File Directory Entry (bit 6 = in use)
const ENTRY_STREAM: u8 = 0xC0; // Stream Extension Entry
const ENTRY_NAME: u8 = 0xC1; // File Name Entry
const ENTRY_BITMAP: u8 = 0x81;
const ENTRY_UPCASE: u8 = 0x82;
const ENTRY_VOLUME_LABEL: u8 = 0x83;

const ATTR_READ_ONLY: u16 = 0x0001;
const ATTR_HIDDEN: u16 = 0x0002;
const ATTR_DIRECTORY: u16 = 0x0010;
const ATTR_ARCHIVE: u16 = 0x0020;
const FLAG_NO_FAT_CHAIN: u8 = 0x02; // AllocationPossible = 0 (0x01 yok) + NoFatChain = 1

pub const MAX_NAME_UNITS: usize = 255;

#[derive(Clone, Debug)]
pub struct ExfatEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub first_cluster: u32,
    pub attributes: u16,
}

/// Bağlı exFAT birimi.
pub struct ExfatFs<D: BlockDevice> {
    dev: D,
    pub base_lba: u64,
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub cluster_count: u32,
    pub fat_offset: u32,
    pub cluster_heap_offset: u32,
    pub root_cluster: u32,
    pub bitmap_cluster: u32,
    pub upcase_cluster: u32,
    pub cluster_size: u32,
}

fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn le64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([
        b[o],
        b[o + 1],
        b[o + 2],
        b[o + 3],
        b[o + 4],
        b[o + 5],
        b[o + 6],
        b[o + 7],
    ])
}
fn wle16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn wle32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn wle64(b: &mut [u8], o: usize, v: u64) {
    b[o..o + 8].copy_from_slice(&v.to_le_bytes());
}

/// UTF-16 ad birimlerini kayipsiz metne cevir (bilinmeyen birimler U+FFFD).
pub fn units_to_name(units: &[u16]) -> String {
    let mut s = String::with_capacity(units.len());
    for &u in units {
        match core::char::from_u32(u as u32) {
            Some(c) => s.push(c),
            None => s.push('\u{FFFD}'),
        }
    }
    s
}

/// UTF-16 dizisi olarak dosya adı baytları.
pub fn name_to_units(name: &str) -> Vec<u16> {
    name.encode_utf16().collect()
}

/// exFAT ad karşılaştırması: büyük/küçük harf duyarsız (basit ASCII katlama).
fn names_equal(a: &[u16], b: &[u16]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        let ux = super::exfat::to_upper_exfat(*x);
        let uy = super::exfat::to_upper_exfat(*y);
        if ux != uy {
            return false;
        }
    }
    true
}

impl<D: BlockDevice> ExfatFs<D> {
    /// Bölüm başındaki VBR'yi okuyup doğrula ve birimi bağla.
    pub fn mount(dev: D, base_lba: u64) -> Option<Self> {
        let mut vbr = [0u8; SECTOR];
        if !dev.read(base_lba, &mut vbr) {
            return None;
        }
        if &vbr[3..11] != super::exfat::EXFAT_OEM_NAME {
            return None;
        }
        if le16(&vbr, 510) != super::exfat::EXFAT_SIGNATURE {
            return None;
        }
        let bps_shift = vbr[108];
        let spc_shift = vbr[109];
        if bps_shift < 9 || bps_shift > 12 || spc_shift > 25 - bps_shift {
            return None;
        }
        let bytes_per_sector = 1u32 << bps_shift;
        let sectors_per_cluster = 1u32 << spc_shift;
        let cluster_count = le32(&vbr, 92);
        if cluster_count < 8 {
            return None;
        }
        let fat_offset = le32(&vbr, 80);
        let cluster_heap_offset = le32(&vbr, 88);
        let root_cluster = le32(&vbr, 96);
        if fat_offset < 24 || cluster_heap_offset <= fat_offset || root_cluster < 2 {
            return None;
        }
        let cluster_size = bytes_per_sector.saturating_mul(sectors_per_cluster);
        let mut fs = Self {
            dev,
            base_lba,
            bytes_per_sector,
            sectors_per_cluster,
            cluster_count,
            fat_offset,
            cluster_heap_offset,
            root_cluster,
            bitmap_cluster: 0,
            upcase_cluster: 0,
            cluster_size,
        };
        // Kök dizinden bitmap/upcase kümelerini bul
        let root = fs.read_chain(root_cluster)?;
        let mut off = 0usize;
        while off + ENTRY_SIZE <= root.bytes.len() {
            let e = &root.bytes[off..off + ENTRY_SIZE];
            if e[0] == DIR_ENTRY_END {
                break;
            }
            if e[0] == ENTRY_BITMAP {
                fs.bitmap_cluster = le32(e, 20);
            } else if e[0] == ENTRY_UPCASE {
                fs.upcase_cluster = le32(e, 20);
            }
            off += ENTRY_SIZE;
        }
        if fs.bitmap_cluster < 2 {
            return None;
        }
        Some(fs)
    }

    /// Birimin ham sektör okuma/yazma erişimi.
    pub fn device(&self) -> &D {
        &self.dev
    }
    pub fn device_mut(&mut self) -> &mut D {
        &mut self.dev
    }

    fn sectors_per_cluster_u64(&self) -> u64 {
        self.sectors_per_cluster as u64
    }

    /// Kümenin başlangıç LBA'si (bölüm başına göreli).
    pub fn cluster_lba(&self, cluster: u32) -> u64 {
        self.cluster_heap_offset as u64 + (cluster as u64 - 2) * self.sectors_per_cluster_u64()
    }

    /// FAT girdisi.
    pub fn next_cluster(&self, cluster: u32) -> Option<u32> {
        let off = self.fat_offset as u64 * self.bytes_per_sector as u64 + cluster as u64 * 4;
        let mut buf = [0u8; SECTOR];
        if !self.dev.read(self.base_lba + off / SECTOR as u64, &mut buf) {
            return None;
        }
        let inner = (off % SECTOR as u64) as usize;
        let v = le32(&buf, inner);
        Some(v & 0x0FFF_FFFF)
    }

    fn set_fat_entry(&mut self, cluster: u32, value: u32) -> bool {
        let off = self.fat_offset as u64 * self.bytes_per_sector as u64 + cluster as u64 * 4;
        let lba = self.base_lba + off / SECTOR as u64;
        let inner = (off % SECTOR as u64) as usize;
        let mut buf = [0u8; SECTOR];
        if !self.dev.read(lba, &mut buf) {
            return false;
        }
        wle32(&mut buf, inner, value);
        self.dev.write(lba, &buf)
    }

    /// Bitmap'teki biti oku.
    fn bitmap_get(&self, cluster: u32) -> bool {
        let bit = (cluster - 2) as u64;
        let byte = bit / 8;
        let chunks = self.chain_clusters(self.bitmap_cluster).unwrap_or_default();
        let bytes_per_cluster = self.cluster_size as u64;
        let idx = (byte / bytes_per_cluster) as usize;
        if idx >= chunks.len() {
            return true; // tanımsız alan dolu sayılır
        }
        let within = (byte % bytes_per_cluster) as usize;
        let mut buf = [0u8; SECTOR];
        let lba = self.base_lba + self.cluster_lba(chunks[idx]) + (within / SECTOR as usize) as u64;
        if !self.dev.read(lba, &mut buf) {
            return true;
        }
        let b = buf[within % SECTOR as usize];
        b & (1 << (bit % 8)) != 0
    }

    fn bitmap_set(&mut self, cluster: u32, used: bool) -> bool {
        let bit = (cluster - 2) as u64;
        let byte = bit / 8;
        let chunks = self.chain_clusters(self.bitmap_cluster).unwrap_or_default();
        let bytes_per_cluster = self.cluster_size as u64;
        let idx = (byte / bytes_per_cluster) as usize;
        if idx >= chunks.len() {
            return false;
        }
        let within = (byte % bytes_per_cluster) as usize;
        let mut buf = [0u8; SECTOR];
        let lba = self.base_lba + self.cluster_lba(chunks[idx]) + (within / SECTOR as usize) as u64;
        if !self.dev.read(lba, &mut buf) {
            return false;
        }
        let mask = 1u8 << (bit % 8);
        if used {
            buf[within % SECTOR as usize] |= mask;
        } else {
            buf[within % SECTOR as usize] &= !mask;
        }
        self.dev.write(lba, &buf)
    }

    /// Kümeyi boşalt (yeni dosya/dizin için).
    fn zero_cluster(&mut self, cluster: u32) -> bool {
        let zero = alloc::vec![0u8; self.cluster_size as usize];
        self.dev
            .write(self.base_lba + self.cluster_lba(cluster), &zero)
    }

    /// FAT zincirini (veya süreklisini) küme listesine çöz.
    fn chain_clusters(&self, first: u32) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        let mut c = first;
        while c >= 2 && c < self.cluster_count + 2 && out.len() <= self.cluster_count as usize {
            out.push(c);
            let next = self.next_cluster(c)?;
            if next == 0x0FFF_FFFF || next < 2 {
                break;
            }
            if next >= self.cluster_count + 2 {
                break;
            }
            c = next;
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    /// Bir küme zincirinin tüm baytlarını oku.
    fn read_chain(&self, first: u32) -> Option<ClusterBuf> {
        let chunks = self.chain_clusters(first)?;
        let mut bytes = alloc::vec![0u8; chunks.len() * self.cluster_size as usize];
        for (i, c) in chunks.iter().enumerate() {
            let off = i * self.cluster_size as usize;
            if !self.dev.read(
                self.base_lba + self.cluster_lba(*c),
                &mut bytes[off..off + self.cluster_size as usize],
            ) {
                return None;
            }
        }
        Some(ClusterBuf { chunks, bytes })
    }

    /// İstenen uzunluk kadar veriyi (FAT zinciri veya sürekli) oku.
    fn read_cluster_data(&self, first: u32, size: u64, no_fat_chain: bool) -> Option<Vec<u8>> {
        let need_clusters =
            ((size + self.cluster_size as u64 - 1) / self.cluster_size as u64).max(1) as usize;
        let mut chunks = Vec::with_capacity(need_clusters);
        if no_fat_chain {
            for i in 0..need_clusters {
                chunks.push(first + i as u32);
            }
        } else {
            chunks = self.chain_clusters(first)?;
        }
        let mut bytes = alloc::vec![0u8; size as usize];
        let mut written = 0usize;
        for c in chunks {
            if written >= bytes.len() {
                break;
            }
            let take = (bytes.len() - written).min(self.cluster_size as usize);
            if !self.dev.read(
                self.base_lba + self.cluster_lba(c),
                &mut bytes[written..written + take],
            ) {
                return None;
            }
            written += take;
        }
        Some(bytes)
    }

    /// Boş bir küme ayır (bitmap + FAT).
    fn alloc_cluster(&mut self) -> Option<u32> {
        for c in 2..self.cluster_count + 2 {
            if !self.bitmap_get(c) {
                if !self.bitmap_set(c, true) {
                    return None;
                }
                if !self.set_fat_entry(c, 0x0FFF_FFFF) {
                    return None;
                }
                if !self.zero_cluster(c) {
                    return None;
                }
                return Some(c);
            }
        }
        None
    }

    fn free_chain(&mut self, first: u32) {
        let chunks = self.chain_clusters(first).unwrap_or_default();
        for c in chunks {
            let _ = self.set_fat_entry(c, 0);
            let _ = self.bitmap_set(c, false);
        }
    }

    /// Kök dizin girdilerini ayrıştır.
    pub fn list_dir(&self, dir_cluster: u32) -> Vec<ExfatEntry> {
        let Some(root) = self.read_chain(dir_cluster) else {
            return Vec::new();
        };
        parse_dir_entries(&root.bytes)
    }

    pub fn list_root(&self) -> Vec<ExfatEntry> {
        self.list_dir(self.root_cluster)
    }

    /// Adı verilen girdiyi bul.
    pub fn lookup_in(&self, dir_cluster: u32, name: &str) -> Option<ExfatEntry> {
        let wanted = name_to_units(name);
        self.list_dir(dir_cluster)
            .into_iter()
            .find(|e| names_equal(&name_to_units(&e.name), &wanted))
    }

    pub fn lookup(&self, path: &str) -> Option<ExfatEntry> {
        let mut cluster = self.root_cluster;
        let mut found: Option<ExfatEntry> = None;
        for comp in path.split('/').filter(|s| !s.is_empty()) {
            found = self.lookup_in(cluster, comp);
            let e = found.as_ref()?;
            if e.is_dir {
                cluster = e.first_cluster;
            } else {
                return Some(e.clone());
            }
        }
        found.or_else(|| {
            Some(ExfatEntry {
                name: String::from("/"),
                is_dir: true,
                size: 0,
                first_cluster: self.root_cluster,
                attributes: ATTR_DIRECTORY,
            })
        })
    }

    /// Dosyanın tüm içeriğini oku.
    pub fn read_file_at(&self, entry: &ExfatEntry) -> Option<Vec<u8>> {
        if entry.is_dir {
            return None;
        }
        if entry.size == 0 {
            return Some(Vec::new());
        }
        let no_fat_chain = self
            .stream_flags(entry)
            .map(|f| f & FLAG_NO_FAT_CHAIN != 0)
            .unwrap_or(false);
        self.read_cluster_data(entry.first_cluster, entry.size, no_fat_chain)
    }

    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        let e = self.lookup(path)?;
        self.read_file_at(&e)
    }

    /// Girdinin stream bayrakları (NoFatChain).
    fn stream_flags(&self, entry: &ExfatEntry) -> Option<u8> {
        let dir = self.find_parent_dir(entry)?;
        let root = self.read_chain(dir)?;
        let wanted = name_to_units(&entry.name);
        let mut off = 0usize;
        while off + ENTRY_SIZE * 3 <= root.bytes.len() {
            let e = &root.bytes[off..off + ENTRY_SIZE];
            if e[0] == DIR_ENTRY_END {
                break;
            }
            if e[0] & 0x7F == (ENTRY_FILE & 0x7F) {
                if let Some((name, stream_off)) = read_entry_name(&root.bytes, off) {
                    if names_equal(&name, &wanted) {
                        return Some(root.bytes[stream_off + 1]);
                    }
                }
            }
            off += ENTRY_SIZE;
        }
        None
    }

    /// Girdinin bulunduğu dizin kümesi (kök ise kök kümesi).
    fn find_parent_dir(&self, entry: &ExfatEntry) -> Option<u32> {
        // Doğrudan kökte mi?
        if self
            .list_dir(self.root_cluster)
            .iter()
            .any(|e| e.name == entry.name)
        {
            return Some(self.root_cluster);
        }
        for e in self.list_dir(self.root_cluster) {
            if e.is_dir
                && self
                    .list_dir(e.first_cluster)
                    .iter()
                    .any(|x| x.name == entry.name)
            {
                return Some(e.first_cluster);
            }
        }
        None
    }

    /// Dizin kümesinde boş girdi alanı bul: (giriş ofseti, gereken ardışık girdi sayısı)
    fn free_entries(bytes: &[u8], needed: usize) -> Option<usize> {
        let mut run = 0usize;
        let mut start = 0usize;
        let mut off = 0usize;
        while off + ENTRY_SIZE <= bytes.len() {
            let in_use = bytes[off] & 0x80 != 0 && bytes[off] != DIR_ENTRY_END;
            let end = bytes[off] == DIR_ENTRY_END;
            if !in_use || end {
                if run == 0 {
                    start = off;
                }
                run += 1;
                if run >= needed {
                    return Some(start);
                }
            } else {
                run = 0;
            }
            off += ENTRY_SIZE;
        }
        None
    }

    /// Dizin kümesine yeni bir dosya girdisi kümesi yaz.
    fn write_entry_set(
        &mut self,
        dir_cluster: u32,
        name_units: &[u16],
        is_dir: bool,
        size: u64,
        first_cluster: u32,
        no_fat_chain: bool,
    ) -> bool {
        let Some(mut dir) = self.read_chain(dir_cluster) else {
            return false;
        };
        let name_entries = (name_units.len() + 14) / 15;
        let needed = 2 + name_entries;
        let Some(start) = Self::free_entries(&dir.bytes, needed) else {
            return false;
        };
        let b = &mut dir.bytes;
        // File Directory Entry
        let o = start;
        b[o] = ENTRY_FILE;
        b[o + 1] = 2; // SecondaryCount
        wle16(b, o + 2, if is_dir { ATTR_DIRECTORY } else { ATTR_ARCHIVE });
        // Stream Extension
        let s = o + ENTRY_SIZE;
        b[s] = ENTRY_STREAM;
        b[s + 1] = if no_fat_chain { FLAG_NO_FAT_CHAIN } else { 0 };
        b[s + 3] = name_units.len().min(MAX_NAME_UNITS) as u8;
        wle16(b, s + 4, 0); // NameHash (arama için kullanılmıyor)
        wle64(b, s + 8, size);
        wle32(b, s + 20, first_cluster);
        wle64(b, s + 24, size.max(1)); // ValidDataLength
                                       // File Name Entries
        for (i, chunk) in name_units.chunks(15).enumerate() {
            let n = s + ENTRY_SIZE * (i + 1);
            b[n] = ENTRY_NAME;
            for (k, unit) in chunk.iter().enumerate() {
                wle16(b, n + 2 + k * 2, *unit);
            }
        }
        // Kalan girdiler boş (in-use=0, EndOfDirectory değil) → yazıldıktan sonra
        // arama fonksiyonları bunları "boş" sayar.
        self.write_chain(&mut dir)
    }

    fn write_chain(&mut self, buf: &mut ClusterBuf) -> bool {
        for (i, c) in buf.chunks.iter().enumerate() {
            let off = i * self.cluster_size as usize;
            if !self.dev.write(
                self.base_lba + self.cluster_lba(*c),
                &buf.bytes[off..off + self.cluster_size as usize],
            ) {
                return false;
            }
        }
        self.dev.flush();
        true
    }

    /// Yeni dosya oluştur ve içeriğini yaz (varsa üzerine yaz).
    pub fn write_file(&mut self, dir_cluster: u32, name: &str, data: &[u8]) -> bool {
        let units = name_to_units(name);
        if units.is_empty() || units.len() > MAX_NAME_UNITS {
            return false;
        }
        // Var olan dosya: içeriği değiştir (küme zinciri yeniden kurulur).
        let existing = self.lookup_in(dir_cluster, name);
        if let Some(e) = existing {
            if e.is_dir {
                return false;
            }
            if e.attributes & ATTR_READ_ONLY != 0 {
                return false;
            }
            self.free_chain(e.first_cluster);
            self.delete_entry(dir_cluster, &units);
        }
        let clusters_needed =
            ((data.len() as u64 + self.cluster_size as u64 - 1) / self.cluster_size as u64).max(1);
        let mut chain: Vec<u32> = Vec::new();
        for _ in 0..clusters_needed {
            let Some(c) = self.alloc_cluster() else {
                return false;
            };
            chain.push(c);
        }
        for i in 0..chain.len().saturating_sub(1) {
            if !self.set_fat_entry(chain[i], chain[i + 1]) {
                return false;
            }
        }
        // Veriyi yaz
        let mut off = 0usize;
        for c in &chain {
            let mut buf = alloc::vec![0u8; self.cluster_size as usize];
            let take = (data.len() - off).min(self.cluster_size as usize);
            if take > 0 {
                buf[..take].copy_from_slice(&data[off..off + take]);
            }
            if !self.dev.write(self.base_lba + self.cluster_lba(*c), &buf) {
                return false;
            }
            off += take;
        }
        self.write_entry_set(
            dir_cluster,
            &units,
            false,
            data.len() as u64,
            chain.first().copied().unwrap_or(0),
            false,
        )
    }

    /// Kökte dosya yaz (kısayol).
    pub fn write_root_file(&mut self, name: &str, data: &[u8]) -> bool {
        let root = self.root_cluster;
        self.write_file(root, name, data)
    }

    /// Alt dizin oluştur.
    pub fn create_dir(&mut self, parent_cluster: u32, name: &str) -> bool {
        let units = name_to_units(name);
        if units.is_empty() || units.len() > MAX_NAME_UNITS {
            return false;
        }
        if self.lookup_in(parent_cluster, name).is_some() {
            return false;
        }
        let Some(c) = self.alloc_cluster() else {
            return false;
        };
        self.write_entry_set(parent_cluster, &units, true, 0, c, false)
    }

    /// Dizinden girdi kümesini işaretle silinmiş (in-use biti = 0).
    fn delete_entry(&mut self, dir_cluster: u32, name_units: &[u16]) -> bool {
        let Some(mut dir) = self.read_chain(dir_cluster) else {
            return false;
        };
        let mut off = 0usize;
        while off + ENTRY_SIZE * 3 <= dir.bytes.len() {
            if dir.bytes[off] & 0x7F == (ENTRY_FILE & 0x7F) {
                if let Some((name, _)) = read_entry_name(&dir.bytes, off) {
                    if names_equal(&name, name_units) {
                        let secondaries = dir.bytes[off + 1] as usize;
                        for i in 0..=secondaries {
                            let o = off + i * ENTRY_SIZE;
                            if o < dir.bytes.len() {
                                dir.bytes[o] &= 0x7F;
                            }
                        }
                        return self.write_chain(&mut dir);
                    }
                }
            }
            off += ENTRY_SIZE;
        }
        false
    }

    /// Dosyayı sil (kümeleri FAT/bitmap üzerinden boşalt).
    pub fn remove(&mut self, dir_cluster: u32, name: &str) -> bool {
        let units = name_to_units(name);
        let Some(e) = self.lookup_in(dir_cluster, name) else {
            return false;
        };
        if e.is_dir {
            return false;
        }
        if e.first_cluster >= 2 {
            self.free_chain(e.first_cluster);
        }
        self.delete_entry(dir_cluster, &units)
    }
}

/// Küme zincirinin okunmuş hâli (düzenleme + geri yazma için).
struct ClusterBuf {
    chunks: Vec<u32>,
    bytes: Vec<u8>,
}

/// Dizin girdi dizisini yüksek seviye kayıtlara çevir.
pub fn parse_dir_entries(bytes: &[u8]) -> Vec<ExfatEntry> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + ENTRY_SIZE <= bytes.len() {
        let tag = bytes[off];
        if tag == DIR_ENTRY_END {
            break;
        }
        if tag & 0x80 != 0 && tag & 0x7F == (ENTRY_FILE & 0x7F) {
            if let Some((units, stream_off)) = read_entry_name(bytes, off) {
                let s = stream_off;
                let attrs = le16(bytes, off + 2);
                let size = le64(bytes, s + 8);
                let first_cluster = le32(bytes, s + 20);
                out.push(ExfatEntry {
                    name: units_to_name(&units),
                    is_dir: attrs & ATTR_DIRECTORY != 0,
                    size,
                    first_cluster,
                    attributes: attrs,
                });
            }
        }
        off += ENTRY_SIZE;
    }
    out
}

/// Girdi kümesinden UTF-16 adı ve stream girdisinin ofsetini çıkar.
fn read_entry_name(bytes: &[u8], file_off: usize) -> Option<(Vec<u16>, usize)> {
    if file_off + ENTRY_SIZE * 2 > bytes.len() {
        return None;
    }
    let secondaries = bytes[file_off + 1] as usize;
    if secondaries < 2 {
        return None;
    }
    let stream_off = file_off + ENTRY_SIZE;
    if bytes[stream_off] != ENTRY_STREAM {
        return None;
    }
    let name_len = bytes[stream_off + 3] as usize;
    let mut units: Vec<u16> = Vec::with_capacity(name_len);
    for i in 0..secondaries - 1 {
        let n = stream_off + ENTRY_SIZE * (i + 1);
        if n + ENTRY_SIZE > bytes.len() || bytes[n] != ENTRY_NAME {
            break;
        }
        for k in 0..15 {
            if units.len() >= name_len {
                break;
            }
            units.push(le16(bytes, n + 2 + k * 2));
        }
    }
    if units.is_empty() {
        None
    } else {
        Some((units, stream_off))
    }
}
