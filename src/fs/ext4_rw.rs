//! ext4 okuma/yazma sürücüsü (extent + directory htree olmadan, ext2 uyumlu
//! blok eşleme; 256 baytlık ext4 inode'ları).
//!
//! `src/fs/ext2.rs` çekirdek sürücüsü doğrudan ATA üzerinden çalışır; bu modül
//! aynı mantığı `BlockDevice` arayüzü üzerinden, ek özelliklerle sunar:
//!
//! - Çoklu blok grubu (her grubun kendi bitmap'leri ve inode tablosu)
//! - 256 baytlık inode'lar, `i_size_high`, `i_blocks` (512B birim)
//! - Direct + single + double indirect blok eşleme (`i_flags` içinde EXTENTS
//!   kapalı olan ext4 birimleri; `mke2fs -O ^extent` veya minOs'un kendi
//!   biçimlendiricisi)
//! - Kök ve alt dizinlerde oluşturma/silme/listeleme, dosya oluşturma/yazma/okuma
//!
//! Sınırlar: extent ağacı (`EXT4_EXTENTS_FL`), journal (JBD2), htree (dir_index),
//! inline data ve 64-bit grup tanımlayıcıları desteklenmez; bu birimler
//! `mount` tarafından reddedilir. Symlink, xattr, ACL yoktur.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

use super::block::{BlockDevice, SECTOR};

const EXT2_MAGIC: u16 = 0xEF53;
/// ext4_extent_header imzası (0xF30A) — desteklenen birimlerde bulunmamalı.
pub const EXTENT_MAGIC: u16 = 0xF30A;

pub const FEATURE_INCOMPAT_FILETYPE: u32 = 0x0002;
pub const FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
pub const FEATURE_INCOMPAT_64BIT: u32 = 0x0080;
pub const FEATURE_INCOMPAT_INLINE_DATA: u32 = 0x8000;
pub const FEATURE_COMPAT_HAS_JOURNAL: u32 = 0x0004;

const S_IFDIR: u16 = 0x4000;
const S_IFREG: u16 = 0x8000;
const DEFAULT_DIR_MODE: u16 = 0x41ED; // 0755
const DEFAULT_FILE_MODE: u16 = 0x81A4; // 0644

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ext4Error {
    /// VBR/süperblok imzası yok.
    BadSuperblock,
    /// Bu sürücün desteklemediği ext4 özellikleri etkin.
    UnsupportedFeatures(u32),
    /// Desteklenen blok boyutu değil (1024..4096, 512'nin katı).
    BadBlockSize,
    /// İstenen inode/dizin bulunamadı.
    NotFound,
    /// Alt aygıt G/Ç hatası.
    Io,
    /// Boş yer yok.
    NoSpace,
    /// Dizin dolu / ad çok uzun.
    Invalid,
}

impl Ext4Error {
    pub fn message(&self) -> &'static str {
        match self {
            Ext4Error::BadSuperblock => "ext4 superblock imzasi gecersiz",
            Ext4Error::UnsupportedFeatures(_) => "ext4 ozellikleri desteklenmiyor (extent/journal)",
            Ext4Error::BadBlockSize => "ext4 blok boyutu desteklenmiyor",
            Ext4Error::NotFound => "yol bulunamadi",
            Ext4Error::Io => "disk okuma/yazma hatasi",
            Ext4Error::NoSpace => "bos blok/inode kalmadi",
            Ext4Error::Invalid => "gecersiz istek",
        }
    }
}

fn le16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o], b[o + 1]]) }
fn le32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) }
fn wle16(b: &mut [u8], o: usize, v: u16) { b[o..o + 2].copy_from_slice(&v.to_le_bytes()); }
fn wle32(b: &mut [u8], o: usize, v: u32) { b[o..o + 4].copy_from_slice(&v.to_le_bytes()); }

#[derive(Clone, Debug)]
pub struct Superblock {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub first_data_block: u32,
    pub block_size: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub inode_size: u32,
    pub desc_size: u32,
    pub feature_incompat: u32,
    pub groups: u32,
}

pub struct Ext4Fs<D: BlockDevice> {
    dev: D,
    pub base_lba: u64,
    pub sb: Superblock,
    pub sectors_per_block: u32,
}

/// Süperblok baytlarını çöz (imza ve özellik kontrolü yapmaz).
pub fn parse_superblock(raw: &[u8]) -> Option<Superblock> {
    if raw.len() < 1024 || le16(raw, 56) != EXT2_MAGIC {
        return None;
    }
    let log_bs = le32(raw, 24);
    if log_bs > 6 {
        return None;
    }
    // 1024 * 2^log_bs = 512 * 2^(log_bs+1); 4096 bayttan büyük blok desteklenmez.
    if log_bs > 2 {
        return None;
    }
    let block_size = 1024u32 << log_bs;
    let blocks_count = le32(raw, 4);
    let blocks_per_group = le32(raw, 32);
    let inodes_per_group = le32(raw, 40);
    if blocks_per_group == 0 || inodes_per_group == 0 {
        return None;
    }
    let inode_size = {
        let v = le16(raw, 88) as u32;
        if v < 128 { 128 } else { v }
    };
    let desc_size = {
        let v = le16(raw, 254) as u32;
        if v < 32 { 32 } else { v }
    };
    Some(Superblock {
        inodes_count: le32(raw, 0),
        blocks_count,
        free_blocks_count: le32(raw, 12),
        free_inodes_count: le32(raw, 16),
        first_data_block: le32(raw, 20),
        block_size,
        blocks_per_group,
        inodes_per_group,
        inode_size,
        desc_size,
        feature_incompat: le32(raw, 96),
        groups: (blocks_count + blocks_per_group - 1) / blocks_per_group,
    })
}

impl<D: BlockDevice> Ext4Fs<D> {
    /// Bölümü bağla; desteklenmeyen özelliklerde `UnsupportedFeatures` döner.
    pub fn mount(dev: D, base_lba: u64) -> Result<Self, Ext4Error> {
        let mut raw = [0u8; 1024];
        // Süperblok her zaman 1024. bayttan başlar (base + 2 sektör).
        if !dev.read(base_lba + 2, &mut raw[..SECTOR]) || !dev.read(base_lba + 3, &mut raw[SECTOR..]) {
            return Err(Ext4Error::Io);
        }
        let sb = parse_superblock(&raw).ok_or(Ext4Error::BadSuperblock)?;
        if sb.block_size < 1024 || sb.block_size % SECTOR as u32 != 0 || sb.block_size > 4096 {
            return Err(Ext4Error::BadBlockSize);
        }
        // 64-bit grup tanımlayıcıları ve inline data bu sürücüde yok.
        let unsupported = sb.feature_incompat
            & (FEATURE_INCOMPAT_EXTENTS | FEATURE_INCOMPAT_64BIT | FEATURE_INCOMPAT_INLINE_DATA);
        if unsupported != 0 {
            return Err(Ext4Error::UnsupportedFeatures(unsupported));
        }
        let sectors_per_block = sb.block_size / SECTOR as u32;
        Ok(Self { dev, base_lba, sb, sectors_per_block })
    }

    pub fn device(&self) -> &D {
        &self.dev
    }
    pub fn device_mut(&mut self) -> &mut D {
        &mut self.dev
    }

    pub fn block_lba(&self, block: u32) -> u64 {
        self.base_lba + block as u64 * self.sectors_per_block as u64
    }

    pub fn read_block(&self, block: u32) -> Result<Vec<u8>, Ext4Error> {
        if block >= self.sb.blocks_count {
            return Err(Ext4Error::Io);
        }
        let mut buf = alloc::vec![0u8; self.sb.block_size as usize];
        if !self.dev.read(self.block_lba(block), &mut buf) {
            return Err(Ext4Error::Io);
        }
        Ok(buf)
    }

    pub fn write_block(&mut self, block: u32, data: &[u8]) -> Result<(), Ext4Error> {
        if block >= self.sb.blocks_count {
            return Err(Ext4Error::Io);
        }
        let bs = self.sb.block_size as usize;
        let mut buf = alloc::vec![0u8; bs];
        let n = data.len().min(bs);
        buf[..n].copy_from_slice(&data[..n]);
        if !self.dev.write(self.block_lba(block), &buf) {
            return Err(Ext4Error::Io);
        }
        self.dev.flush();
        Ok(())
    }

    fn gdt_block(&self) -> u32 {
        if self.sb.block_size == 1024 { 2 } else { 1 }
    }

    /// Grup tanımlayıcısı (en az 32 bayt; 64-bit alanlar kullanılmaz).
    pub fn read_group_desc(&self, group: u32) -> Result<[u8; 32], Ext4Error> {
        let per_block = self.sb.block_size / self.sb.desc_size;
        let blk = self.gdt_block() + group / per_block;
        let data = self.read_block(blk)?;
        let off = (group % per_block) as usize * self.sb.desc_size as usize;
        let mut out = [0u8; 32];
        out.copy_from_slice(&data[off..off + 32]);
        Ok(out)
    }

    fn write_group_desc(&mut self, group: u32, gd: &[u8; 32]) -> Result<(), Ext4Error> {
        let per_block = self.sb.block_size / self.sb.desc_size;
        let blk = self.gdt_block() + group / per_block;
        let mut data = self.read_block(blk)?;
        let off = (group % per_block) as usize * self.sb.desc_size as usize;
        data[off..off + 32].copy_from_slice(gd);
        self.write_block(blk, &data)
    }

    /// İnodenin ham baytlarını oku.
    pub fn read_inode(&self, inode: u32) -> Result<Vec<u8>, Ext4Error> {
        if inode == 0 || inode > self.sb.inodes_count {
            return Err(Ext4Error::NotFound);
        }
        let group = (inode - 1) / self.sb.inodes_per_group;
        let index = (inode - 1) % self.sb.inodes_per_group;
        let gd = self.read_group_desc(group)?;
        let table = le32(&gd, 8);
        let per_block = self.sb.block_size / self.sb.inode_size;
        let blk = table + index / per_block;
        let data = self.read_block(blk)?;
        let off = (index % per_block) as usize * self.sb.inode_size as usize;
        Ok(data[off..off + self.sb.inode_size as usize].to_vec())
    }

    pub fn write_inode(&mut self, inode: u32, raw: &[u8]) -> Result<(), Ext4Error> {
        let group = (inode - 1) / self.sb.inodes_per_group;
        let index = (inode - 1) % self.sb.inodes_per_group;
        let gd = self.read_group_desc(group)?;
        let table = le32(&gd, 8);
        let per_block = self.sb.block_size / self.sb.inode_size;
        let blk = table + index / per_block;
        let mut data = self.read_block(blk)?;
        let off = (index % per_block) as usize * self.sb.inode_size as usize;
        let n = raw.len().min(self.sb.inode_size as usize);
        data[off..off + n].copy_from_slice(&raw[..n]);
        self.write_block(blk, &data)
    }

    pub fn inode_mode(&self, inode: &[u8]) -> u16 {
        le16(inode, 0)
    }

    pub fn inode_is_dir(&self, inode: &[u8]) -> bool {
        le16(inode, 0) & 0xF000 == S_IFDIR
    }

    pub fn inode_size_bytes(&self, inode: &[u8]) -> u64 {
        let lo = le32(inode, 4) as u64;
        let hi = le32(inode, 108) as u64;
        lo | (hi << 32)
    }

    fn inode_set_size(&self, inode: &mut [u8], size: u64) {
        wle32(inode, 4, size as u32);
        wle32(inode, 108, (size >> 32) as u32);
    }

    fn inode_add_links(&self, inode: &mut [u8], delta: i32) {
        let v = le16(inode, 26) as i32 + delta;
        wle16(inode, 26, v.max(0) as u16);
    }

    /// İnodenin veri bloğu listesi (direct + single + double indirect).
    pub fn inode_blocks(&self, inode: &[u8]) -> Result<Vec<u32>, Ext4Error> {
        let mut out = Vec::new();
        for i in 0..12 {
            let b = le32(inode, 40 + i * 4);
            if b != 0 {
                out.push(b);
            }
        }
        let per_block = self.sb.block_size / 4;
        let single = le32(inode, 40 + 12 * 4);
        if single != 0 {
            let data = self.read_block(single)?;
            for i in 0..per_block as usize {
                let b = le32(&data, i * 4);
                if b != 0 {
                    out.push(b);
                }
            }
        }
        let double = le32(inode, 40 + 13 * 4);
        if double != 0 {
            let data = self.read_block(double)?;
            for i in 0..per_block as usize {
                let ind = le32(&data, i * 4);
                if ind == 0 {
                    continue;
                }
                let l2 = self.read_block(ind)?;
                for j in 0..per_block as usize {
                    let b = le32(&l2, j * 4);
                    if b != 0 {
                        out.push(b);
                    }
                }
            }
        }
        Ok(out)
    }

    /// Dosya içeriğini oku.
    pub fn read_inode_data(&self, inode: &[u8]) -> Result<Vec<u8>, Ext4Error> {
        let size = self.inode_size_bytes(inode);
        if size == 0 {
            return Ok(Vec::new());
        }
        let blocks = self.inode_blocks(inode)?;
        let bs = self.sb.block_size as usize;
        let mut out = alloc::vec![0u8; size as usize];
        let mut done = 0usize;
        for b in blocks {
            if done >= out.len() {
                break;
            }
            let data = self.read_block(b)?;
            let take = (out.len() - done).min(bs);
            out[done..done + take].copy_from_slice(&data[..take]);
            done += take;
        }
        Ok(out)
    }

    /// Dizin girdileri: (inode, ad, dizin mi)
    pub fn dir_entries(&self, inode: &[u8]) -> Result<Vec<(u32, String, bool)>, Ext4Error> {
        let mut out = Vec::new();
        let bs = self.sb.block_size as usize;
        for blk in self.inode_blocks(inode)? {
            let data = self.read_block(blk)?;
            let mut off = 0usize;
            while off + 8 <= bs {
                let ino = le32(&data, off);
                let reclen = le16(&data, off + 4) as usize;
                let namelen = data[off + 6] as usize;
                if reclen < 8 || off + reclen > bs {
                    break;
                }
                if ino != 0 && namelen > 0 && off + 8 + namelen <= bs {
                    let name = String::from_utf8_lossy(&data[off + 8..off + 8 + namelen]).into_owned();
                    if name != "." && name != ".." {
                        let is_dir = data[off + 7] == 2;
                        out.push((ino, name, is_dir));
                    }
                }
                off += reclen;
            }
        }
        Ok(out)
    }

    pub fn lookup(&self, path: &str) -> Result<(u32, Vec<u8>), Ext4Error> {
        let mut inode_no = 2u32;
        let mut raw = self.read_inode(inode_no)?;
        for comp in path.split('/').filter(|s| !s.is_empty()) {
            if !self.inode_is_dir(&raw) {
                return Err(Ext4Error::NotFound);
            }
            let entries = self.dir_entries(&raw)?;
            let found = entries.iter().find(|(_, n, _)| n == comp).ok_or(Ext4Error::NotFound)?;
            inode_no = found.0;
            raw = self.read_inode(inode_no)?;
        }
        Ok((inode_no, raw))
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, Ext4Error> {
        let (_, raw) = self.lookup(path)?;
        if self.inode_is_dir(&raw) {
            return Err(Ext4Error::Invalid);
        }
        self.read_inode_data(&raw)
    }

    /// Boş blok ayır (grup bitmap'leri sırayla).
    pub fn alloc_block(&mut self) -> Result<u32, Ext4Error> {
        for g in 0..self.sb.groups {
            let mut gd = self.read_group_desc(g)?;
            if le16(&gd, 12) == 0 {
                continue;
            }
            let bitmap_block = le32(&gd, 0);
            let mut bmp = self.read_block(bitmap_block)?;
            let mut found = None;
            for (bi, byte) in bmp.iter_mut().enumerate() {
                if *byte == 0xFF {
                    continue;
                }
                for bit in 0..8u32 {
                    if *byte & (1 << bit) == 0 {
                        *byte |= 1 << bit;
                        found = Some(
                            g * self.sb.blocks_per_group
                                + bi as u32 * 8
                                + bit
                                + self.sb.first_data_block,
                        );
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            if let Some(block) = found {
                self.write_block(bitmap_block, &bmp)?;
                let used = le16(&gd, 12);
                wle16(&mut gd, 12, used.saturating_sub(1));
                self.write_group_desc(g, &gd)?;
                if let Ok(mut sb_raw) = self.read_superblock_raw() {
                    let free = le32(&sb_raw, 12).saturating_sub(1);
                    wle32(&mut sb_raw, 12, free);
                    let _ = self.write_superblock_raw(&sb_raw);
                }
                let zero = alloc::vec![0u8; self.sb.block_size as usize];
                self.write_block(block, &zero)?;
                return Ok(block);
            }
        }
        Err(Ext4Error::NoSpace)
    }

    pub fn free_block(&mut self, block: u32) -> Result<(), Ext4Error> {
        if block < self.sb.first_data_block {
            return Ok(());
        }
        let n = block - self.sb.first_data_block;
        let g = n / self.sb.blocks_per_group;
        let bit = n % self.sb.blocks_per_group;
        let mut gd = self.read_group_desc(g)?;
        let bitmap_block = le32(&gd, 0);
        let mut bmp = self.read_block(bitmap_block)?;
        let idx = (bit / 8) as usize;
        if idx < bmp.len() {
            bmp[idx] &= !(1 << (bit % 8));
            self.write_block(bitmap_block, &bmp)?;
            let free = le16(&gd, 12) + 1;
            wle16(&mut gd, 12, free);
            self.write_group_desc(g, &gd)?;
        }
        Ok(())
    }

    /// Boş inode ayır.
    pub fn alloc_inode(&mut self, is_dir: bool) -> Result<u32, Ext4Error> {
        for g in 0..self.sb.groups {
            let mut gd = self.read_group_desc(g)?;
            if le16(&gd, 14) == 0 {
                continue;
            }
            let bitmap_block = le32(&gd, 4);
            let mut bmp = self.read_block(bitmap_block)?;
            for (bi, byte) in bmp.iter_mut().enumerate() {
                if *byte == 0xFF {
                    continue;
                }
                for bit in 0..8u32 {
                    if *byte & (1 << bit) == 0 {
                        let ino = g * self.sb.inodes_per_group + bi as u32 * 8 + bit + 1;
                        if ino <= 10 {
                            continue; // rezerve inode'lar
                        }
                        *byte |= 1 << bit;
                        self.write_block(bitmap_block, &bmp)?;
                        let free = le16(&gd, 14).saturating_sub(1);
                        wle16(&mut gd, 14, free);
                        if is_dir {
                            let dirs = le16(&gd, 16) + 1;
                            wle16(&mut gd, 16, dirs);
                        }
                        self.write_group_desc(g, &gd)?;
                        let blank = alloc::vec![0u8; self.sb.inode_size as usize];
                        self.write_inode(ino, &blank)?;
                        return Ok(ino);
                    }
                }
            }
        }
        Err(Ext4Error::NoSpace)
    }

    fn free_inode(&mut self, ino: u32) -> Result<(), Ext4Error> {
        let g = (ino - 1) / self.sb.inodes_per_group;
        let bit = (ino - 1) % self.sb.inodes_per_group;
        let mut gd = self.read_group_desc(g)?;
        let bitmap_block = le32(&gd, 4);
        let mut bmp = self.read_block(bitmap_block)?;
        let idx = (bit / 8) as usize;
        if idx < bmp.len() {
            bmp[idx] &= !(1 << (bit % 8));
            self.write_block(bitmap_block, &bmp)?;
            let free = le16(&gd, 14) + 1;
            wle16(&mut gd, 14, free);
            self.write_group_desc(g, &gd)?;
        }
        Ok(())
    }

    fn read_superblock_raw(&self) -> Result<[u8; 1024], Ext4Error> {
        let mut raw = [0u8; 1024];
        if !self.dev.read(self.base_lba + 2, &mut raw[..SECTOR])
            || !self.dev.read(self.base_lba + 3, &mut raw[SECTOR..])
        {
            return Err(Ext4Error::Io);
        }
        Ok(raw)
    }

    fn write_superblock_raw(&mut self, raw: &[u8; 1024]) -> Result<(), Ext4Error> {
        if !self.dev.write(self.base_lba + 2, &raw[..SECTOR])
            || !self.dev.write(self.base_lba + 3, &raw[SECTOR..])
        {
            return Err(Ext4Error::Io);
        }
        self.dev.flush();
        Ok(())
    }

    /// İnodenin veri bloklarını `data` ile değiştir (gerekirse blok ayır/boşalt).
    pub fn write_inode_data(&mut self, ino: u32, inode: &mut Vec<u8>, data: &[u8]) -> Result<(), Ext4Error> {
        let bs = self.sb.block_size as usize;
        let per_block = self.sb.block_size / 4;
        let need = if data.is_empty() { 0 } else { (data.len() + bs - 1) / bs };
        let single_capacity = 12 + per_block as usize;
        let double_capacity = single_capacity + per_block as usize * per_block as usize;
        if need > double_capacity {
            return Err(Ext4Error::NoSpace);
        }

        let mut blocks = self.inode_blocks(inode)?;
        while blocks.len() < need {
            let b = self.alloc_block()?;
            blocks.push(b);
        }
        while blocks.len() > need {
            let b = blocks.pop().unwrap();
            self.free_block(b)?;
        }

        // Direct
        for i in 0..12 {
            wle32(inode, 40 + i * 4, if i < blocks.len() { blocks[i] } else { 0 });
        }
        // Single indirect
        let old_single = le32(inode, 40 + 12 * 4);
        let single_blocks = blocks.len().saturating_sub(12).min(per_block as usize);
        if single_blocks > 0 {
            let ind = if old_single != 0 {
                old_single
            } else {
                let b = self.alloc_block()?;
                wle32(inode, 40 + 12 * 4, b);
                b
            };
            let mut ib = alloc::vec![0u8; bs];
            for (i, blk) in blocks.iter().skip(12).take(single_blocks).enumerate() {
                wle32(&mut ib, i * 4, *blk);
            }
            self.write_block(ind, &ib)?;
        } else if old_single != 0 {
            wle32(inode, 40 + 12 * 4, 0);
            let data_block = self.read_block(old_single)?;
            for i in 0..per_block as usize {
                let b = le32(&data_block, i * 4);
                if b != 0 {
                    self.free_block(b)?;
                }
            }
            self.free_block(old_single)?;
        }
        // Double indirect
        let old_double = le32(inode, 40 + 13 * 4);
        let double_blocks: Vec<u32> = blocks.iter().skip(single_capacity).copied().collect();
        if !double_blocks.is_empty() {
            let dbl = if old_double != 0 {
                old_double
            } else {
                let b = self.alloc_block()?;
                wle32(inode, 40 + 13 * 4, b);
                b
            };
            let mut top = alloc::vec![0u8; bs];
            for (chunk_index, chunk) in double_blocks.chunks(per_block as usize).enumerate() {
                let ind = self.alloc_block()?;
                let mut level2 = alloc::vec![0u8; bs];
                for (i, blk) in chunk.iter().enumerate() {
                    wle32(&mut level2, i * 4, *blk);
                }
                self.write_block(ind, &level2)?;
                wle32(&mut top, chunk_index * 4, ind);
            }
            self.write_block(dbl, &top)?;
        } else if old_double != 0 {
            wle32(inode, 40 + 13 * 4, 0);
            let top = self.read_block(old_double)?;
            for i in 0..per_block as usize {
                let ind = le32(&top, i * 4);
                if ind != 0 {
                    self.free_block(ind)?;
                }
            }
            self.free_block(old_double)?;
        }

        // Veri yazımı
        let mut off = 0usize;
        for b in blocks.iter().take(need) {
            let take = (data.len() - off).min(bs);
            self.write_block(*b, &data[off..off + take])?;
            off += take;
        }

        self.inode_set_size(inode, data.len() as u64);
        let sectors = ((need * bs) / SECTOR as usize) as u32;
        wle32(inode, 28, sectors);
        self.write_inode(ino, inode)
    }

    /// Dizine girdi ekle (boş alanı böler, yoksa yeni blok tahsis eder).
    fn dir_add_entry(
        &mut self,
        dir_ino: u32,
        dir_inode: &mut Vec<u8>,
        child: u32,
        name: &str,
        is_dir: bool,
    ) -> Result<(), Ext4Error> {
        let bs = self.sb.block_size as usize;
        let name_bytes = name.as_bytes();
        if name_bytes.is_empty() || name_bytes.len() > 255 {
            return Err(Ext4Error::Invalid);
        }
        let need = ((8 + name_bytes.len() + 3) / 4) * 4;
        if need > bs {
            return Err(Ext4Error::Invalid);
        }
        let blocks = self.inode_blocks(dir_inode)?;
        for blk in &blocks {
            let mut data = self.read_block(*blk)?;
            let mut off = 0usize;
            while off + 8 <= bs {
                let ino = le32(&data, off);
                let reclen = le16(&data, off + 4) as usize;
                if reclen < 8 || off + reclen > bs {
                    break;
                }
                if ino == 0 {
                    if reclen >= need {
                        wle32(&mut data, off, child);
                        wle16(&mut data, off + 4, reclen as u16);
                        data[off + 6] = name_bytes.len() as u8;
                        data[off + 7] = if is_dir { 2 } else { 1 };
                        data[off + 8..off + 8 + name_bytes.len()].copy_from_slice(name_bytes);
                        self.write_block(*blk, &data)?;
                        return Ok(());
                    }
                } else {
                    let namelen = data[off + 6] as usize;
                    let ideal = ((8 + namelen + 3) / 4) * 4;
                    if reclen >= ideal + need {
                        wle16(&mut data, off + 4, ideal as u16);
                        let new_off = off + ideal;
                        wle32(&mut data, new_off, child);
                        wle16(&mut data, new_off + 4, (reclen - ideal) as u16);
                        data[new_off + 6] = name_bytes.len() as u8;
                        data[new_off + 7] = if is_dir { 2 } else { 1 };
                        data[new_off + 8..new_off + 8 + name_bytes.len()].copy_from_slice(name_bytes);
                        self.write_block(*blk, &data)?;
                        return Ok(());
                    }
                }
                off += reclen;
            }
        }
        // Yeni blok
        let new_block = self.alloc_block()?;
        let mut data = alloc::vec![0u8; bs];
        wle32(&mut data, 0, child);
        wle16(&mut data, 4, bs as u16);
        data[6] = name_bytes.len() as u8;
        data[7] = if is_dir { 2 } else { 1 };
        data[8..8 + name_bytes.len()].copy_from_slice(name_bytes);
        self.write_block(new_block, &data)?;
        // İnodenin blok listesine ekle
        let mut new_blocks = blocks;
        new_blocks.push(new_block);
        let per_block = self.sb.block_size / 4;
        if new_blocks.len() <= 12 {
            wle32(dir_inode, 40 + (new_blocks.len() - 1) * 4, new_block);
        } else if new_blocks.len() <= 12 + per_block as usize {
            let ind = {
                let v = le32(dir_inode, 40 + 12 * 4);
                if v != 0 {
                    v
                } else {
                    let b = self.alloc_block()?;
                    wle32(dir_inode, 40 + 12 * 4, b);
                    b
                }
            };
            let mut ib = self.read_block(ind)?;
            wle32(&mut ib, (new_blocks.len() - 13) * 4, new_block);
            self.write_block(ind, &ib)?;
        } else {
            return Err(Ext4Error::NoSpace);
        }
        self.inode_set_size(dir_inode, (new_blocks.len() * bs) as u64);
        wle32(dir_inode, 28, (new_blocks.len() * bs / SECTOR as usize) as u32);
        self.write_inode(dir_ino, dir_inode)
    }

    fn split_path(path: &str) -> (&str, &str) {
        match path.rfind('/') {
            Some(i) => {
                let parent = if i == 0 { "/" } else { &path[..i] };
                (parent, &path[i + 1..])
            }
            None => ("/", path),
        }
    }

    /// Dosya oluştur (varsa içeriği değiştir).
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), Ext4Error> {
        let (parent_path, name) = Self::split_path(path);
        let (parent_ino, mut parent) = self.lookup(parent_path)?;
        if let Ok((ino, mut raw)) = self.lookup(path) {
            if self.inode_is_dir(&raw) {
                return Err(Ext4Error::Invalid);
            }
            return self.write_inode_data(ino, &mut raw, data);
        }
        let ino = self.alloc_inode(false)?;
        let mut raw = alloc::vec![0u8; self.sb.inode_size as usize];
        wle16(&mut raw, 0, DEFAULT_FILE_MODE);
        wle16(&mut raw, 26, 1);
        self.write_inode(ino, &raw)?;
        self.write_inode_data(ino, &mut raw, data)?;
        self.dir_add_entry(parent_ino, &mut parent, ino, name, false)
    }

    /// Alt dizin oluştur.
    pub fn mkdir(&mut self, path: &str) -> Result<u32, Ext4Error> {
        let (parent_path, name) = Self::split_path(path);
        let (parent_ino, mut parent) = self.lookup(parent_path)?;
        if self.lookup(path).is_ok() {
            return Err(Ext4Error::Invalid);
        }
        let ino = self.alloc_inode(true)?;
        let blk = self.alloc_block()?;
        let bs = self.sb.block_size as usize;
        let mut dd = alloc::vec![0u8; bs];
        wle32(&mut dd, 0, ino);
        wle16(&mut dd, 4, 12);
        dd[6] = 1;
        dd[7] = 2;
        dd[8] = b'.';
        wle32(&mut dd, 12, parent_ino);
        wle16(&mut dd, 16, (bs - 12) as u16);
        dd[18] = 2;
        dd[19] = 2;
        dd[20] = b'.';
        dd[21] = b'.';
        self.write_block(blk, &dd)?;
        let mut raw = alloc::vec![0u8; self.sb.inode_size as usize];
        wle16(&mut raw, 0, DEFAULT_DIR_MODE);
        self.inode_set_size(&mut raw, bs as u64);
        wle32(&mut raw, 40, blk);
        wle16(&mut raw, 26, 2);
        wle32(&mut raw, 28, (bs / SECTOR as usize) as u32);
        self.write_inode(ino, &raw)?;
        self.dir_add_entry(parent_ino, &mut parent, ino, name, true)?;
        self.inode_add_links(&mut parent, 1);
        self.write_inode(parent_ino, &parent)?;
        Ok(ino)
    }

    /// Dosyayı sil: küme/blokları boşalt, girdiyi dizinden kaldır.
    pub fn remove_file(&mut self, path: &str) -> Result<(), Ext4Error> {
        let (parent_path, name) = Self::split_path(path);
        let (parent_ino, mut parent) = self.lookup(parent_path)?;
        let (ino, raw) = self.lookup(path)?;
        if self.inode_is_dir(&raw) {
            return Err(Ext4Error::Invalid);
        }
        for b in self.inode_blocks(&raw)? {
            self.free_block(b)?;
        }
        self.free_inode(ino)?;
        self.dir_remove_entry(parent_ino, &mut parent, name)?;
        self.inode_add_links(&mut parent, -1);
        self.write_inode(parent_ino, &parent)
    }

    fn dir_remove_entry(
        &mut self,
        dir_ino: u32,
        dir_inode: &mut [u8],
        name: &str,
    ) -> Result<(), Ext4Error> {
        let bs = self.sb.block_size as usize;
        let name_bytes = name.as_bytes();
        for blk in self.inode_blocks(dir_inode)? {
            let mut data = self.read_block(blk)?;
            let mut off = 0usize;
            let mut prev: Option<usize> = None;
            while off + 8 <= bs {
                let ino = le32(&data, off);
                let reclen = le16(&data, off + 4) as usize;
                if reclen < 8 || off + reclen > bs {
                    break;
                }
                let namelen = data[off + 6] as usize;
                if ino != 0 && namelen == name_bytes.len()
                    && &data[off + 8..off + 8 + namelen] == name_bytes
                {
                    if let Some(p) = prev {
                        // Önceki girdinin rec_len'ini birleştir.
                        let prev_len = le16(&data, p + 4) as usize;
                        wle16(&mut data, p + 4, (prev_len + reclen) as u16);
                    } else {
                        // İlk girdi: inode = 0 yap (birleştirme mümkün değil).
                        wle32(&mut data, off, 0);
                    }
                    self.write_block(blk, &data)?;
                    return Ok(());
                }
                prev = Some(off);
                off += reclen;
            }
        }
        let _ = dir_ino;
        Err(Ext4Error::NotFound)
    }

    /// Dosya sistemi istatistikleri: (toplam blok, boş blok, toplam inode, boş inode)
    pub fn stats(&mut self) -> [u32; 4] {
        let mut free_blocks = 0u32;
        let mut free_inodes = 0u32;
        for g in 0..self.sb.groups {
            if let Ok(gd) = self.read_group_desc(g) {
                free_blocks += le16(&gd, 12) as u32;
                free_inodes += le16(&gd, 14) as u32;
            }
        }
        self.sb.free_blocks_count = free_blocks;
        self.sb.free_inodes_count = free_inodes;
        [self.sb.blocks_count, free_blocks, self.sb.inodes_count, free_inodes]
    }
}
