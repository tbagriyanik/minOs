#![allow(dead_code, static_mut_refs)]
//! Minimal Ext2 read-write (kalıcı depolama): superblock + BGDT + inode +
//! dizin girdileri + direct/single-indirect bloklar. Tüm disk G/Ç ATA PIO.
//!
//! Kapsam: kök dizin listeleme, dosya okuma/yazma/oluşturma/silme, mkdir.
//! Sınırlama: symlink yok, 8-bit isim (255B), tek grup varsayımı yok — çok
//! grup desteklenir; indirect çift/üçlü desteklenmez (büyük dosya > ~268 blok
//! için indirect zinciri tek seviyede tutulur).

extern crate alloc;
use alloc::vec::Vec;
use crate::drivers::ata;

const MAGIC: u16 = 0xEF53;

struct Sb {
    base_lba: u32, // bölüm başlangıcı (512B sektör)
    block_size: u32,
    blocks_per_group: u32,
    inodes_per_group: u32,
    first_data_block: u32,
    groups: u32,
    inode_size: u32,
    sec_per_block: u32,
}

static mut SB: Option<Sb> = None;
static mut DRIVE: u8 = 0;

fn le16(b: &[u8], o: usize) -> u16 { u16::from_le_bytes([b[o],b[o+1]]) }
fn le32(b: &[u8], o: usize) -> u32 { u32::from_le_bytes([b[o],b[o+1],b[o+2],b[o+3]]) }
fn wle16(b: &mut [u8], o: usize, v: u16) { b[o..o+2].copy_from_slice(&v.to_le_bytes()); }
fn wle32(b: &mut [u8], o: usize, v: u32) { b[o..o+4].copy_from_slice(&v.to_le_bytes()); }

fn block_lba(sb: &Sb, block: u32) -> u32 { sb.base_lba + block * sb.sec_per_block }

fn read_block(sb: &Sb, drive: u8, block: u32, out: &mut Vec<u8>) -> bool {
    out.clear();
    if out.try_reserve_exact(sb.block_size as usize).is_err() { return false; }
    out.resize(sb.block_size as usize, 0);
    let lba = block_lba(sb, block);
    let n = sb.sec_per_block as usize;
    ata::read_sectors(drive, lba, n, out) == n
}

fn write_block(sb: &Sb, drive: u8, block: u32, data: &[u8]) -> bool {
    let bs = sb.block_size as usize;
    let mut buf = Vec::new();
    if buf.try_reserve_exact(bs).is_err() { return false; }
    buf.resize(bs, 0);
    let n = data.len().min(bs);
    buf[..n].copy_from_slice(&data[..n]);
    let lba = block_lba(sb, block);
    let c = sb.sec_per_block as usize;
    if ata::write_sectors_unflushed(drive, lba, c, &buf) != c { return false; }
    ata::flush_cache(drive);
    true
}

fn sync_sb(drive: u8, sb_raw: &[u8; 1024]) -> bool {
    // superblock 1024. bayttan başlar = base+2 sektör
    unsafe {
        let base = SB.as_ref().map(|s| s.base_lba).unwrap_or(0);
        if ata::write_sectors_unflushed(drive, base + 2, 2, sb_raw) != 2 { return false; }
        ata::flush_cache(drive);
        true
    }
}

/// Bölüm başlangıcındaki Ext2 imzasını doğrula ve mount et.
pub fn mount(drive: u8, part_lba: u32) -> bool {
    let mut raw = [0u8; 1024];
    let mut tmp = [0u8; 512];
    // superblock = offset 1024 => base+2, 2 sektör
    if !ata::read_sector(drive, part_lba + 2, &mut tmp) { return false; }
    raw[..512].copy_from_slice(&tmp);
    if !ata::read_sector(drive, part_lba + 3, &mut tmp) { return false; }
    raw[512..].copy_from_slice(&tmp);
    if le16(&raw, 56) != MAGIC { return false; }
    let log_bs = le32(&raw, 24);
    if log_bs > 3 { return false; }
    let bs = 1024u32 << log_bs;
    if bs % 512 != 0 { return false; }
    let sb = Sb {
        base_lba: part_lba,
        block_size: bs,
        blocks_per_group: le32(&raw, 32),
        inodes_per_group: le32(&raw, 40),
        first_data_block: le32(&raw, 20),
        groups: (le32(&raw, 4).saturating_add(le32(&raw, 32)).saturating_sub(1) / le32(&raw, 32).max(1)).max(1),
        inode_size: { let v = le16(&raw, 88) as u32; if v == 0 { 128 } else { v } },
        sec_per_block: bs / 512,
    };
    if sb.blocks_per_group == 0 || sb.inodes_per_group == 0 { return false; }
    unsafe { SB = Some(sb); DRIVE = drive; }
    true
}

pub fn mounted() -> bool { unsafe { SB.is_some() } }

fn group_of(sb: &Sb, inode: u32) -> (u32, u32) {
    let g = (inode - 1) / sb.inodes_per_group;
    let idx = (inode - 1) % sb.inodes_per_group;
    (g, idx)
}

fn bgdt_block(sb: &Sb) -> u32 { if sb.block_size == 1024 { 2 } else { 1 } }

fn read_gd(sb: &Sb, drive: u8, group: u32, out: &mut [u8; 32]) -> bool {
    let per = sb.block_size / 32;
    let blk = bgdt_block(sb) + group / per;
    let mut b = Vec::new();
    if !read_block(sb, drive, blk, &mut b) { return false; }
    let off = (group % per) as usize * 32;
    out.copy_from_slice(&b[off..off+32]);
    true
}

fn write_gd(sb: &Sb, drive: u8, group: u32, gd: &[u8; 32]) -> bool {
    let per = sb.block_size / 32;
    let blk = bgdt_block(sb) + group / per;
    let mut b = Vec::new();
    if !read_block(sb, drive, blk, &mut b) { return false; }
    let off = (group % per) as usize * 32;
    b[off..off+32].copy_from_slice(gd);
    write_block(sb, drive, blk, &b)
}

fn read_inode_raw(sb: &Sb, drive: u8, inode: u32, out: &mut Vec<u8>) -> bool {
    let (g, idx) = group_of(sb, inode);
    let mut gd = [0u8; 32];
    if !read_gd(sb, drive, g, &mut gd) { return false; }
    let table = le32(&gd, 8);
    let per_block = sb.block_size / sb.inode_size;
    let blk = table + idx / per_block;
    let mut b = Vec::new();
    if !read_block(sb, drive, blk, &mut b) { return false; }
    let off = (idx % per_block) as usize * sb.inode_size as usize;
    out.clear();
    out.extend_from_slice(&b[off..off+sb.inode_size as usize]);
    true
}

fn write_inode_raw(sb: &Sb, drive: u8, inode: u32, data: &[u8]) -> bool {
    let (g, idx) = group_of(sb, inode);
    let mut gd = [0u8; 32];
    if !read_gd(sb, drive, g, &mut gd) { return false; }
    let table = le32(&gd, 8);
    let per_block = sb.block_size / sb.inode_size;
    let blk = table + idx / per_block;
    let mut b = Vec::new();
    if !read_block(sb, drive, blk, &mut b) { return false; }
    let off = (idx % per_block) as usize * sb.inode_size as usize;
    let n = data.len().min(sb.inode_size as usize);
    b[off..off+n].copy_from_slice(&data[..n]);
    write_block(sb, drive, blk, &b)
}

/// inode'un veri blok listesi (direct 0..11 + single indirect blok 12).
fn inode_blocks(sb: &Sb, drive: u8, ino: &[u8]) -> Vec<u32> {
    let mut v = Vec::new();
    for i in 0..12 {
        let b = le32(ino, 40 + i*4);
        if b != 0 { v.push(b); }
    }
    let ind = le32(ino, 40 + 12*4);
    if ind != 0 {
        let mut b = Vec::new();
        let sbc = unsafe { SB.as_ref().unwrap() };
        if read_block(sbc, drive, ind, &mut b) {
            for i in 0..sb.block_size/4 {
                let x = le32(&b, i as usize*4);
                if x != 0 && v.len() < 65536 { v.push(x); }
            }
        }
    }
    v
}

fn alloc_block(sb: &Sb, drive: u8) -> Option<u32> {
    for g in 0..sb.groups {
        let mut gd = [0u8; 32];
        if !read_gd(sb, drive, g, &mut gd) { return None; }
        if le16(&gd, 12) == 0 { continue; }
        let bmp = le32(&gd, 0);
        let mut b = Vec::new();
        if !read_block(sb, drive, bmp, &mut b) { return None; }
        for (bi, byte) in b.iter_mut().enumerate() {
            if *byte != 0xFF {
                for bit in 0..8 {
                    if *byte & (1 << bit) == 0 {
                        *byte |= 1 << bit;
                        if !write_block(sb, drive, bmp, &b) { return None; }
                        let blk = g * sb.blocks_per_group + (bi as u32 * 8 + bit as u32) + sb.first_data_block;
                        let mut gd2 = gd;
                        wle16(&mut gd2, 12, le16(&gd, 12) - 1);
                        let _ = write_gd(sb, drive, g, &gd2);
                        let mut z = Vec::new(); z.resize(sb.block_size as usize, 0);
                        let _ = write_block(sb, drive, blk, &z);
                        return Some(blk);
                    }
                }
            }
        }
    }
    None
}

fn free_block(sb: &Sb, drive: u8, block: u32) {
    if block < sb.first_data_block { return; }
    let n = block - sb.first_data_block;
    let g = n / sb.blocks_per_group;
    let bit = n % sb.blocks_per_group;
    let mut gd = [0u8; 32];
    if !read_gd(sb, drive, g, &mut gd) { return; }
    let bmp = le32(&gd, 0);
    let mut b = Vec::new();
    if !read_block(sb, drive, bmp, &mut b) { return; }
    let i = (bit / 8) as usize;
    if i < b.len() {
        b[i] &= !(1 << (bit % 8));
        let _ = write_block(sb, drive, bmp, &b);
        let mut gd2 = gd;
        wle16(&mut gd2, 12, le16(&gd, 12) + 1);
        let _ = write_gd(sb, drive, g, &gd2);
    }
}

fn alloc_inode(sb: &Sb, drive: u8, is_dir: bool) -> Option<u32> {
    for g in 0..sb.groups {
        let mut gd = [0u8; 32];
        if !read_gd(sb, drive, g, &mut gd) { return None; }
        if le16(&gd, 14) == 0 { continue; }
        let bmp = le32(&gd, 4);
        let mut b = Vec::new();
        if !read_block(sb, drive, bmp, &mut b) { return None; }
        for (bi, byte) in b.iter_mut().enumerate() {
            if *byte != 0xFF {
                for bit in 0..8 {
                    if *byte & (1 << bit) == 0 {
                        *byte |= 1 << bit;
                        if !write_block(sb, drive, bmp, &mut b) { return None; }
                        let ino = g * sb.inodes_per_group + bi as u32 * 8 + bit as u32 + 1;
                        let mut gd2 = gd;
                        wle16(&mut gd2, 14, le16(&gd, 14) - 1);
                        if is_dir { wle16(&mut gd2, 16, le16(&gd, 16) + 1); }
                        let _ = write_gd(sb, drive, g, &gd2);
                        return Some(ino);
                    }
                }
            }
        }
    }
    None
}

#[derive(Clone)]
pub struct DirEnt { pub inode: u32, pub name: Vec<u8>, pub is_dir: bool }

fn dir_entries(sb: &Sb, drive: u8, ino_raw: &[u8]) -> Vec<DirEnt> {
    let mut out = Vec::new();
    let blocks = inode_blocks(sb, drive, ino_raw);
    let mut b = Vec::new();
    for blk in blocks {
        if !read_block(sb, drive, blk, &mut b) { continue; }
        let mut off = 0usize;
        while off + 8 <= b.len() {
            let ino = le32(&b, off);
            let reclen = le16(&b, off+4) as usize;
            let namelen = b[off+6] as usize;
            let ftype = b[off+7];
            if reclen < 8 || off + reclen > b.len() { break; }
            if ino != 0 && namelen > 0 && off+8+namelen <= b.len() {
                let nm = b[off+8..off+8+namelen].to_vec();
                if !(nm == b"." || nm == b"..") {
                    out.push(DirEnt { inode: ino, name: nm, is_dir: ftype == 2 });
                }
            }
            off += reclen;
        }
    }
    out
}

fn find_in_dir(sb: &Sb, drive: u8, dir_ino: &[u8], name: &[u8]) -> Option<(u32, bool)> {
    for e in dir_entries(sb, drive, dir_ino) {
        if e.name == name { return Some((e.inode, e.is_dir)); }
    }
    None
}

/// "/a/b/c" yolunu inode'a çöz (2 = kök).
pub fn lookup(path: &str) -> Option<(u32, Vec<u8>)> {
    let (sb, drive) = unsafe { (SB.as_ref()?, DRIVE) };
    let mut ino_raw = Vec::new();
    if !read_inode_raw(sb, drive, 2, &mut ino_raw) { return None; }
    if path == "/" || path.is_empty() { return Some((2, ino_raw)); }
    let mut cur = 2u32;
    for comp in path.split('/').filter(|s| !s.is_empty()) {
        if !read_inode_raw(sb, drive, cur, &mut ino_raw) { return None; }
        if le16(&ino_raw, 0) & 0x4000 == 0 { return None; }
        let (next, _) = find_in_dir(sb, drive, &ino_raw, comp.as_bytes())?;
        cur = next;
    }
    if !read_inode_raw(sb, drive, cur, &mut ino_raw) { return None; }
    Some((cur, ino_raw))
}

pub fn list_dir(path: &str) -> Vec<(Vec<u8>, bool, u32)> {
    let Some((_, ino)) = lookup(path) else { return Vec::new() };
    let (sb, drive) = unsafe { (SB.as_ref().unwrap(), DRIVE) };
    dir_entries(sb, drive, &ino).into_iter().map(|e| {
        let sz = read_inode_raw(sb, drive, e.inode, &mut Vec::new()).then(|| 0).unwrap_or(0);
        let _ = sz;
        let mut raw = Vec::new();
        let size = if read_inode_raw(sb, drive, e.inode, &mut raw) { le32(&raw, 4) } else { 0 };
        (e.name, e.is_dir, size)
    }).collect()
}

pub fn read_file(path: &str, out: &mut Vec<u8>) -> bool {
    let Some((_, ino)) = lookup(path) else { return false };
    let (sb, drive) = unsafe { (SB.as_ref().unwrap(), DRIVE) };
    if le16(&ino, 0) & 0x8000 == 0 { return false; }
    let size = le32(&ino, 4) as usize;
    out.clear();
    if out.try_reserve_exact(size).is_err() { return false; }
    out.resize(size, 0);
    let blocks = inode_blocks(sb, drive, &ino);
    let bs = sb.block_size as usize;
    let mut b = Vec::new();
    let mut done = 0usize;
    for blk in blocks {
        if done >= size { break; }
        if !read_block(sb, drive, blk, &mut b) { return false; }
        let take = (size - done).min(bs);
        out[done..done+take].copy_from_slice(&b[..take]);
        done += take;
    }
    done == size
}

/// inode veri bloklarını `data` ile değiştirir (gerekirse blok ayırır/boşaltır).
fn write_inode_data(sb: &Sb, drive: u8, inode: u32, ino_raw: &mut Vec<u8>, data: &[u8]) -> bool {
    let bs = sb.block_size as usize;
    let need = data.len().max(1).div_ceil(bs);
    let mut blocks = inode_blocks(sb, drive, ino_raw);
    while blocks.len() < need {
        let Some(b) = alloc_block(sb, drive) else { return false };
        blocks.push(b);
    }
    while blocks.len() > need {
        let b = blocks.pop().unwrap();
        free_block(sb, drive, b);
    }
    // direct + single indirect yaz
    let direct_n = need.min(12);
    for i in 0..12 {
        let v = if i < direct_n { blocks[i] } else { 0 };
        wle32(ino_raw, 40+i*4, v);
    }
    let old_ind = le32(ino_raw, 40+12*4);
    if need > 12 {
        let ind = if old_ind != 0 { old_ind } else {
            let Some(b) = alloc_block(sb, drive) else { return false };
            wle32(ino_raw, 40+12*4, b);
            b
        };
        let mut ib = alloc::vec![0u8; 0];
        ib.resize(bs, 0);
        for (i, blk) in blocks.iter().skip(12).enumerate() {
            ib[i*4..i*4+4].copy_from_slice(&blk.to_le_bytes());
        }
        if !write_block(sb, drive, ind, &ib) { return false; }
    } else if old_ind != 0 {
        wle32(ino_raw, 40+12*4, 0);
        free_block(sb, drive, old_ind);
    }
    // veri yaz
    let mut off = 0usize;
    for blk in blocks.iter().take(need) {
        let take = (data.len() - off).min(bs);
        let mut tmp = Vec::new(); tmp.resize(bs, 0);
        if take > 0 { tmp[..take].copy_from_slice(&data[off..off+take]); }
        if !write_block(sb, drive, *blk, &tmp) { return false; }
        off += take;
    }
    let secs = (data.len() as u32).div_ceil(512).max(1);
    wle32(ino_raw, 4, data.len() as u32);
    wle32(ino_raw, 28, secs);
    // blok sayısını güncelle (512B birimi)
    wle32(ino_raw, 28, (need as u32) * (bs as u32 / 512));
    write_inode_raw(sb, drive, inode, ino_raw)
}

fn dir_add_entry(sb: &Sb, drive: u8, dir_inode: u32, dir_raw: &mut Vec<u8>, child: u32, name: &[u8], is_dir: bool) -> bool {
    let bs = sb.block_size as usize;
    let blocks = inode_blocks(sb, drive, dir_raw);
    let mut b = Vec::new();
    // boş slot ara
    for blk in blocks.clone() {
        if !read_block(sb, drive, blk, &mut b) { return false; }
        let mut off = 0usize;
        while off + 8 <= bs {
            let ino = le32(&b, off);
            let reclen = le16(&b, off+4) as usize;
            if reclen < 8 || off + reclen > bs { break; }
            if ino == 0 {
                let need = ((8 + name.len() + 3) / 4) * 4;
                if reclen >= need {
                    wle32(&mut b, off, child);
                    wle16(&mut b, off+4, reclen as u16);
                    b[off+6] = name.len() as u8;
                    b[off+7] = if is_dir { 2 } else { 1 };
                    b[off+8..off+8+name.len()].copy_from_slice(name);
                    return write_block(sb, drive, blk, &b);
                }
            } else {
                let namelen = b[off+6] as usize;
                let ideal = ((8 + namelen + 3) / 4) * 4;
                if reclen > ideal {
                    let need = ((8 + name.len() + 3) / 4) * 4;
                    if reclen - ideal >= need {
                        wle16(&mut b, off+4, ideal as u16);
                        let noff = off + ideal;
                        wle32(&mut b, noff, child);
                        wle16(&mut b, noff+4, (reclen - ideal) as u16);
                        b[noff+6] = name.len() as u8;
                        b[noff+7] = if is_dir { 2 } else { 1 };
                        b[noff+8..noff+8+name.len()].copy_from_slice(name);
                        return write_block(sb, drive, blk, &b);
                    }
                }
            }
            off += reclen;
        }
    }
    // yeni blok ekle
    let Some(nb) = alloc_block(sb, drive) else { return false };
    let mut nb_data = Vec::new(); nb_data.resize(bs, 0);
    let need = ((8 + name.len() + 3) / 4) * 4;
    wle32(&mut nb_data, 0, child);
    wle16(&mut nb_data, 4, bs as u16);
    nb_data[6] = name.len() as u8;
    nb_data[7] = if is_dir { 2 } else { 1 };
    nb_data[8..8+name.len()].copy_from_slice(name);
    let _ = need;
    if !write_block(sb, drive, nb, &nb_data) { return false; }
    // blok listesine ekle
    let mut nb2 = Vec::new(); nb2.resize(bs, 0);
    // basit: mevcut blok verisini oku, sonuna ekle — bunun yerine inode blok listesini genişlet
    let size = le32(dir_raw, 4) as usize;
    let mut all = Vec::new();
    let bl = inode_blocks(sb, drive, dir_raw);
    let mut tmp = Vec::new();
    for blk in bl { if read_block(sb, drive, blk, &mut tmp) { all.extend_from_slice(&tmp); } }
    all.resize(size + bs, 0);
    all[size..size+bs].copy_from_slice(&nb_data);
    // blokları yeniden yaz: son blok nb olmalı
    let mut blocks2 = inode_blocks(sb, drive, dir_raw);
    blocks2.push(nb);
    // direct yaz (basit yol: sadece son bloğu bağla)
    for (i, blk) in blocks2.iter().take(12).enumerate() {
        wle32(dir_raw, 40+i*4, *blk);
    }
    wle32(dir_raw, 4, (size + bs) as u32);
    let _ = nb2;
    write_inode_raw(sb, drive, dir_inode, dir_raw)
}

pub fn create_file(path: &str, data: &[u8]) -> bool {
    let (sb, drive) = unsafe { (SB.as_ref().unwrap() as *const Sb, DRIVE) };
    let sb = unsafe { &*sb };
    let (parent, name) = match path.rfind('/') { Some(i) => (&path[..i.max(1)], &path[i+1..]), None => ("/", path) };
    let parent = if parent.is_empty() { "/" } else { parent };
    let Some((pino, mut praw)) = lookup(parent) else { return false };
    if find_in_dir(sb, drive, &praw, name.as_bytes()).is_some() {
        return write_file(path, data);
    }
    let Some(nino) = alloc_inode(sb, drive, false) else { return false };
    let mut raw = alloc::vec![0u8; sb.inode_size as usize];
    wle16(&mut raw, 0, 0x81A4);
    wle16(&mut raw, 2, 0);
    wle32(&mut raw, 4, 0);
    wle16(&mut raw, 24, 1);
    if !write_inode_raw(sb, drive, nino, &raw) { return false; }
    let mut raw2 = raw.clone();
    if !write_inode_data(sb, drive, nino, &mut raw2, data) { return false; }
    dir_add_entry(sb, drive, pino, &mut praw, nino, name.as_bytes(), false)
}

pub fn write_file(path: &str, data: &[u8]) -> bool {
    let Some((ino, mut raw)) = lookup(path) else { return create_file(path, data) };
    let (sb, drive) = unsafe { (SB.as_ref().unwrap() as *const Sb, DRIVE) };
    let sb = unsafe { &*sb };
    write_inode_data(sb, drive, ino, &mut raw, data)
}

pub fn mkdir(path: &str) -> bool {
    let (sb, drive) = unsafe { (SB.as_ref().unwrap() as *const Sb, DRIVE) };
    let sb = unsafe { &*sb };
    let (parent, name) = match path.rfind('/') { Some(i) => (&path[..i.max(1)], &path[i+1..]), None => ("/", path) };
    let parent = if parent.is_empty() { "/" } else { parent };
    let Some((pino, mut praw)) = lookup(parent) else { return false };
    if find_in_dir(sb, drive, &praw, name.as_bytes()).is_some() { return false; }
    let Some(nino) = alloc_inode(sb, drive, true) else { return false };
    let Some(dblk) = alloc_block(sb, drive) else { return false };
    let bs = sb.block_size as usize;
    let mut dd = Vec::new(); dd.resize(bs, 0);
    wle32(&mut dd, 0, nino); wle16(&mut dd, 4, 12); dd[6]=1; dd[7]=2; dd[8]=b'.';
    wle32(&mut dd, 12, pino); wle16(&mut dd, 16, bs as u16 - 12); dd[18]=2; dd[19]=2; dd[20]=b'.'; dd[21]=b'.';
    if !write_block(sb, drive, dblk, &dd) { return false; }
    let mut raw = alloc::vec![0u8; sb.inode_size as usize];
    wle16(&mut raw, 0, 0x41ED);
    wle32(&mut raw, 4, bs as u32);
    wle32(&mut raw, 40, dblk);
    wle16(&mut raw, 24, 2);
    wle32(&mut raw, 28, bs as u32 / 512);
    if !write_inode_raw(sb, drive, nino, &raw) { return false; }
    if !dir_add_entry(sb, drive, pino, &mut praw, nino, name.as_bytes(), true) { return false; }
    // link sayısını artır
    let mut pr = Vec::new();
    if read_inode_raw(sb, drive, pino, &mut pr) {
        let n = le16(&pr, 24) + 1;
        wle16(&mut pr, 24, n);
        let _ = write_inode_raw(sb, drive, pino, &pr);
    }
    true
}

/// Yeni Ext2 dosya sistemi biçimlendir (tüm bölümü tek dosya sistemi yapar).
pub fn format(drive: u8, part_lba: u32, part_sectors: u64) -> bool {
    let total_blocks = (part_sectors / 2) as u32; // 1024B blok
    if total_blocks < 256 { return false; }
    let bs: u32 = 1024;
    let inodes = (total_blocks / 4).max(64);
    let groups = (total_blocks + 8191) / 8192;
    let _ = groups;
    // Tek grup basitliği: groups=1 kabulü yerine gerçek çok grup değil —
    // küçük diskler (<8MB blok) için 1 grup yeterli.
    let g = 1u32;
    let itable_blocks = (inodes * 128 + bs - 1) / bs;
    // blok yerleşimi (1K blok): 0=boot, 1=sb, 2=bgdt, 3=bblock-bmp, 4=ibmp, 5..=itable, sonra root
    let root_blk = 5 + itable_blocks;
    let first_free = root_blk + 1;
    let mut sb_raw = [0u8; 1024];
    wle32(&mut sb_raw, 0, inodes);
    wle32(&mut sb_raw, 4, total_blocks);
    wle32(&mut sb_raw, 12, total_blocks - first_free);
    wle32(&mut sb_raw, 16, inodes - 12);
    wle32(&mut sb_raw, 20, 1);
    wle32(&mut sb_raw, 24, 0);
    wle32(&mut sb_raw, 32, 8192);
    wle32(&mut sb_raw, 40, inodes);
    wle16(&mut sb_raw, 56, MAGIC);
    wle16(&mut sb_raw, 58, 1);
    wle16(&mut sb_raw, 62, 1);
    wle16(&mut sb_raw, 88, 128);
    // superblock yaz (base+2, base+3)
    let mut s0 = [0u8; 512]; s0.copy_from_slice(&sb_raw[..512]);
    let mut s1 = [0u8; 512]; s1.copy_from_slice(&sb_raw[512..]);
    if !ata::write_sector(drive, part_lba + 2, &s0) { return false; }
    if !ata::write_sector(drive, part_lba + 3, &s1) { return false; }
    // BGDT blok 2
    let mut gd = [0u8; 1024];
    wle32(&mut gd, 0, 3); wle32(&mut gd, 4, 4); wle32(&mut gd, 8, 5);
    wle16(&mut gd, 12, (8192 - (first_free - 1)) as u16);
    wle16(&mut gd, 14, (inodes - 12) as u16);
    wle16(&mut gd, 16, 2);
    let mut g0 = [0u8; 512]; g0.copy_from_slice(&gd[..512]);
    let mut g1 = [0u8; 512]; g1.copy_from_slice(&gd[512..]);
    let bl = |b: u32| part_lba + b * 2;
    if !ata::write_sector(drive, bl(2), &g0) { return false; }
    if !ata::write_sector(drive, bl(2) + 1, &g1) { return false; }
    // bitmapler
    let mut bmp = [0u8; 512];
    // ilk first_free bloğu dolu işaretle
    for b in 0..first_free { bmp[(b/8) as usize] |= 1 << (b % 8); }
    if !ata::write_sector(drive, bl(3), &bmp) { return false; }
    if !ata::write_sector(drive, bl(3) + 1, &bmp) { return false; }
    let mut ibmp = [0u8; 512];
    for i in 0..12 { ibmp[(i/8) as usize] |= 1 << (i % 8); } // 1..12 dolu (2=kök)
    ibmp[1] = ibmp[1]; // 9..12
    ibmp[1] |= 0x0F;
    if !ata::write_sector(drive, bl(4), &ibmp) { return false; }
    if !ata::write_sector(drive, bl(4) + 1, &ibmp) { return false; }
    // inode tablosunu sıfırla
    let z = [0u8; 512];
    for b in 5..5 + itable_blocks {
        if !ata::write_sector(drive, bl(b), &z) { return false; }
        if !ata::write_sector(drive, bl(b) + 1, &z) { return false; }
    }
    // kök inode (#2): blok 5'in ilk inode'u (offset 128)
    // inode tablosu blok 5'ten başlar; inode2 offset = 1*128
    let mut ib0 = [0u8; 512];
    // inode2 blok 5 sektör 0 içinde: offset 128
    let mut root_ino = [0u8; 128];
    wle16(&mut root_ino, 0, 0x41ED);
    wle32(&mut root_ino, 4, 1024);
    wle16(&mut root_ino, 24, 3); // nlink: . .. + parent
    wle32(&mut root_ino, 28, 2);
    wle32(&mut root_ino, 40, root_blk);
    // inode1 boş, inode2 yaz
    ib0[128..256].copy_from_slice(&root_ino);
    if !ata::write_sector(drive, bl(5), &ib0) { return false; }
    // kök dizin bloğu: . ve ..
    let mut dd = [0u8; 512];
    wle32(&mut dd, 0, 2); wle16(&mut dd, 4, 12); dd[6]=1; dd[7]=2; dd[8]=b'.';
    wle32(&mut dd, 12, 2); wle16(&mut dd, 16, 512-12); dd[18]=2; dd[19]=2; dd[20]=b'.'; dd[21]=b'.';
    if !ata::write_sector(drive, bl(root_blk), &dd) { return false; }
    if !ata::write_sector(drive, bl(root_blk) + 1, &dd) { return false; }
    let _ = g;
    ata::flush_cache(drive);
    mount(drive, part_lba)
}
