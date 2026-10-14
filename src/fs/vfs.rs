#![allow(dead_code, static_mut_refs)]

//! Sanal Dosya Sistemi (VFS) — Klasör Desteği, Dosya Açma, Kaydetme, Silme ve Disk Entegrasyonu.
//!
//! Bu modül tam bir dizin hiyerarşisi (/root, /root/Documents, /media/usb0, vb.)
//! ve dinamik dosya/klasör yönetimi sağlar. `/media/usb0` ve `/media/cdrom` yolları
//! otomatik olarak ATA/FAT32 ve ISO9660 sürücülerine yönlendirilir.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::diskfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
}

/// Geri Dönüşüm Kutusu sanal dizin yolu. Silinen öğeler buraya taşınır.
pub const TRASH_DIR: &str = "/root/Trash";

pub struct VfsNode {
    pub name: &'static str,
    pub node_type: NodeType,
    pub size: usize,
    pub data: &'static [u8],
}

#[derive(Clone)]
pub struct VfsItem {
    pub path: String,
    pub name: String,
    pub node_type: NodeType,
    pub size: usize,
    pub data: Vec<u8>,
    pub is_readonly: bool,
}

pub struct VirtualFileSystem {
    pub nodes: [Option<VfsNode>; 16],
    pub items: Vec<VfsItem>,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        let mut vfs = Self {
            nodes: [
                Some(VfsNode { name: "kernel.log", node_type: NodeType::File, size: 34, data: b"[minOs] Kernel booted successfully\n" }),
                Some(VfsNode { name: "readme.txt", node_type: NodeType::File, size: 47, data: b"Welcome to minOs - Fast & Lightweight x86_64 OS\n" }),
                Some(VfsNode { name: "settings.cfg", node_type: NodeType::File, size: 24, data: b"theme=velvet\nvolume=80\n" }),
                None, None, None, None, None,
                None, None, None, None, None, None, None, None,
            ],
            items: Vec::new(),
        };

        // Standart Kök Dizin Hiyerarşisi
        vfs.init_default_hierarchy();
        vfs
    }

    fn init_default_hierarchy(&mut self) {
        // Klasörler
        self.create_dir_internal("/");
        self.create_dir_internal("/root");
        self.create_dir_internal("/root/Documents");
        self.create_dir_internal("/root/Media");
        self.create_dir_internal("/root/Downloads");
        self.create_dir_internal("/boot");
        self.create_dir_internal("/etc");
        self.create_dir_internal("/var");
        self.create_dir_internal("/var/log");
        self.create_dir_internal("/media");
        self.create_dir_internal("/media/usb0");
        self.create_dir_internal("/media/hdd0");
        self.create_dir_internal("/media/cdrom");
        // Geri Dönüşüm Kutusu (Trash) — silinen öğeler buraya taşınır.
        self.create_dir_internal(TRASH_DIR);

        // Örnek Belgeler ve Sistem Dosyaları
        self.create_file_internal(
            "/root/Documents/notlar.txt",
            b"minOs Not Defteri\n================\nBu dosya dogrudan VFS ve disk uzerinde saklanmaktadir.\nMetin duzenleyip [Kaydet] butonuyla kalici hale getirebilirsiniz.\n"
        );
        self.create_file_internal(
            "/root/Documents/readme.txt",
            b"minOs x86_64 Sarman Edition v2.0\n--------------------------------\n* Hizli ve hafif mikrocekirdek mimarisi\n* Gelismis dosya yoneticisi ve klasor destegi\n* USB, HDD ve CD-ROM suruculeri aktif\n"
        );
        self.create_file_internal(
            "/root/Documents/proje_fikirleri.txt",
            b"Gelecek Surum Planlari:\n1. FAT32 kalici dizin agaci gelistirmesi\n2. TCP/IP istemci socket API\n3. Coklu gorev zamanlayici\n"
        );
        self.create_file_internal(
            "/etc/settings.cfg",
            b"theme=velvet;volume=80;keyboard=tr;language=tr;res=1280x720;"
        );
        self.create_file_internal(
            "/var/log/kernel.log",
            b"[minOs] Kernel v2.0 online.\n[Storage] ATA 4 channels initialized.\n[VFS] Virtual File System mounted.\n[GUI] Compositor active.\n"
        );
        self.create_file_internal(
            "/boot/limine.cfg",
            b"TIMEOUT=3\n:minOs\nPROTOCOL=limine\nKERNEL_PATH=boot:///minos-kernel\n"
        );
        self.create_file_internal(
            "/root/Media/kedi_maskotu.bmp",
            b"BM\x36\x00\x00\x00\x00\x00\x00\x00\x36\x00\x00\x00\x28\x00\x00\x00"
        );
        self.create_file_internal(
            "/root/Media/lofi_ring0.wav",
            b"RIFF\x24\x00\x00\x00WAVEfmt \x10\x00\x00\x00\x01\x00\x02\x00\x44\xAC\x00\x00"
        );
        self.create_file_internal(
            "/root/Media/retro_drive.mp4",
            b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2avc1mp41"
        );
    }

    fn normalize_path(path: &str) -> String {
        let p = path.trim();
        if p.is_empty() {
            return String::from("/");
        }
        let mut norm = String::new();
        if !p.starts_with('/') {
            norm.push('/');
        }
        norm.push_str(p);
        while norm.len() > 1 && norm.ends_with('/') {
            norm.pop();
        }
        norm
    }

    fn file_name_from_path(path: &str) -> String {
        let p = path.trim_end_matches('/');
        if let Some(pos) = p.rfind('/') {
            String::from(&p[pos + 1..])
        } else {
            String::from(p)
        }
    }

    fn parent_path_from_path(path: &str) -> String {
        let p = path.trim_end_matches('/');
        if let Some(pos) = p.rfind('/') {
            if pos == 0 {
                String::from("/")
            } else {
                String::from(&p[..pos])
            }
        } else {
            String::from("/")
        }
    }

    fn create_dir_internal(&mut self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        for it in &self.items {
            if it.path == norm {
                return false;
            }
        }
        let name = Self::file_name_from_path(&norm);
        self.items.push(VfsItem {
            path: norm,
            name,
            node_type: NodeType::Directory,
            size: 0,
            data: Vec::new(),
            is_readonly: false,
        });
        true
    }

    fn create_file_internal(&mut self, path: &str, data: &[u8]) -> bool {
        let norm = Self::normalize_path(path);
        let name = Self::file_name_from_path(&norm);
        for it in &mut self.items {
            if it.path == norm {
                it.data = data.to_vec();
                it.size = data.len();
                return true;
            }
        }
        self.items.push(VfsItem {
            path: norm,
            name,
            node_type: NodeType::File,
            size: data.len(),
            data: data.to_vec(),
            is_readonly: false,
        });
        true
    }

    /// Yeni bir klasör oluşturur.
    pub fn create_dir(&mut self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        let parent = Self::parent_path_from_path(&norm);
        if !self.is_dir(&parent) && parent != "/" {
            self.create_dir(&parent);
        }
        self.create_dir_internal(&norm)
    }

    /// Yeni dosya oluşturur veya üzerine yazar.
    pub fn create_file(&mut self, path: &str, data: &[u8]) -> bool {
        let norm = Self::normalize_path(path);
        let parent = Self::parent_path_from_path(&norm);
        if !self.is_dir(&parent) && parent != "/" {
            self.create_dir(&parent);
        }
        self.create_file_internal(&norm, data)
    }

    /// Dosyaya yazar veya üzerine kaydeder.
    pub fn write_file(&mut self, path_or_name: &str, data: &[u8]) -> bool {
        let norm = Self::normalize_path(path_or_name);

        // USB ve Sabit Disk ortamına yazma kontrolü
        if norm.starts_with("/media/usb0/") || norm.starts_with("/media/hdd0/") {
            let disk_file = Self::file_name_from_path(&norm);
            // diskfs üzerinden de diske/qcow2'ye senkronize et
            diskfs::save_file_by_name(&disk_file, data);
        }

        for it in &mut self.items {
            if it.path == norm || it.name == path_or_name {
                if it.node_type == NodeType::Directory {
                    return false;
                }
                it.data = data.to_vec();
                it.size = data.len();
                return true;
            }
        }

        // Bulunamadıysa yeni dosya olarak oluştur
        self.create_file(&norm, data)
    }

    /// Büyük dosyalar için baytları çağıran uygulamada tutup VFS'de yalnızca
    /// dosya kaydı oluşturur/günceller. Çağıran tekrar okuma için sahibi olur.
    pub fn write_large_file_metadata(&mut self, path: &str, size: usize) -> bool {
        let norm = Self::normalize_path(path);
        for item in &mut self.items {
            if item.path == norm {
                if item.node_type == NodeType::Directory { return false; }
                item.size = size;
                item.data.clear();
                return true;
            }
        }
        let parent = Self::parent_path_from_path(&norm);
        if !self.is_dir(&parent) && parent != "/" { self.create_dir(&parent); }
        self.items.push(VfsItem {
            name: Self::file_name_from_path(&norm),
            path: norm,
            node_type: NodeType::File,
            size,
            data: Vec::new(),
            is_readonly: false,
        });
        true
    }

    pub fn rename(&mut self, old_path: &str, new_path: &str) -> bool {
        let old = Self::normalize_path(old_path);
        let new = Self::normalize_path(new_path);
        if old == "/" || new == "/" || self.exists(&new) { return false; }
        if !self.exists(&old) { return false; }
        let prefix = if old.ends_with('/') { old.clone() } else { let mut p = old.clone(); p.push('/'); p };
        let mut changed = false;
        for item in &mut self.items {
            if item.path == old {
                item.path = new.clone();
                item.name = Self::file_name_from_path(&new);
                changed = true;
            } else if item.path.starts_with(&prefix) {
                let suffix = &item.path[prefix.len()..];
                let mut child = new.clone(); child.push('/'); child.push_str(suffix);
                item.path = child;
                changed = true;
            }
        }
        changed
    }

    /// Dosya okur (tam yol veya dosya adı ile).
    pub fn read_file(&self, path_or_name: &str) -> Option<&[u8]> {
        let norm = Self::normalize_path(path_or_name);

        // Önce doğrudan VFS kayıtlarında ara
        for it in &self.items {
            if (it.path == norm || it.name == path_or_name) && it.node_type == NodeType::File {
                return Some(&it.data);
            }
        }

        // Sabit Disk (HDD/QCOW2), USB veya CD-ROM ortamından okuma
        if norm.starts_with("/media/usb0/") || norm.starts_with("/media/hdd0/") || norm.starts_with("/media/cdrom/") {
            let filename = Self::file_name_from_path(&norm);
            if diskfs::load_file(&filename) > 0 {
                return Some(diskfs::content());
            }
        }

        // Geriye dönük uyumluluk: static nodes tablosu
        for opt in self.nodes.iter() {
            if let Some(node) = opt {
                if node.node_type == NodeType::File && (node.name == path_or_name || norm.ends_with(node.name)) {
                    return Some(node.data);
                }
            }
        }

        None
    }

    /// Bir büyük dosyanın baytları onu oluşturan uygulama tarafından tutulur;
    /// bu kayıt okunmaya çalışılırsa veri olmadığını açıkça bildir.
    pub fn is_large_file_metadata(&self, path_or_name: &str) -> bool {
        let norm = Self::normalize_path(path_or_name);
        self.items.iter().any(|item| {
            (item.path == norm || item.name == path_or_name)
                && item.node_type == NodeType::File
                && item.size > 0
                && item.data.is_empty()
        })
    }

    /// Dosya siler.
    pub fn delete_file(&mut self, path_or_name: &str) -> bool {
        let norm = Self::normalize_path(path_or_name);

        if norm.starts_with("/media/usb0/") || norm.starts_with("/media/hdd0/") {
            let disk_file = Self::file_name_from_path(&norm);
            diskfs::delete_file_by_name(&disk_file);
        }

        let mut found_idx = None;
        for (i, it) in self.items.iter().enumerate() {
            if (it.path == norm || it.name == path_or_name) && it.node_type == NodeType::File {
                found_idx = Some(i);
                break;
            }
        }
        if let Some(idx) = found_idx {
            self.items.remove(idx);
            return true;
        }
        false
    }

    /// Klasör siler (içi boş olmalıdır).
    pub fn delete_dir(&mut self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        if norm == "/" {
            return false;
        }
        // İçinde çocuk var mı kontrolü
        for it in &self.items {
            if it.path != norm && it.path.starts_with(&norm) {
                return false;
            }
        }
        let mut found_idx = None;
        for (i, it) in self.items.iter().enumerate() {
            if it.path == norm && it.node_type == NodeType::Directory {
                found_idx = Some(i);
                break;
            }
        }
        if let Some(idx) = found_idx {
            self.items.remove(idx);
            return true;
        }
        false
    }

    /// Silinen bir öğeyi kalıcı olarak silmek yerine Geri Dönüşüm Kutusu'na taşır.
    /// Dosya veya (boş) klasör olabilir. Aynı isimde öğe varsa sonuna numara eklenir.
    /// Başarılıysa `true` döner ve öğe artık `TRASH_DIR` altında yaşar.
    pub fn move_to_trash(&mut self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        // Zaten çöp kutusunda olan ya da kök dizini taşımaya çalışmak anlamsız.
        if norm == "/" || norm == TRASH_DIR || Self::is_in_trash_path(&norm) {
            return false;
        }
        // Öğe var mı?
        let src = match self.items.iter().find(|it| it.path == norm) {
            Some(it) => it.clone(),
            None => return false,
        };
        // Klasörse içi boş olmalı (iç içe taşıma karmaşıklığını önlemek için).
        if src.node_type == NodeType::Directory {
            for it in &self.items {
                if it.path != norm && it.path.starts_with(&norm) {
                    return false;
                }
            }
        }
        // Çöp kutusunda çakışmayan bir hedef isim üret.
        let base_name = src.name.clone();
        let mut target = String::from(TRASH_DIR);
        target.push('/');
        target.push_str(&base_name);
        let mut counter = 1u32;
        while self.items.iter().any(|it| it.path == target) {
            target = String::from(TRASH_DIR);
            target.push('/');
            target.push_str(&base_name);
            target.push('_');
            Self::push_u32(&mut target, counter);
            counter += 1;
        }
        // Öğeyi çöp kutusuna taşı (yol ve isim güncellenir).
        for it in &mut self.items {
            if it.path == norm {
                it.path = target.clone();
                it.name = Self::file_name_from_path(&target);
                return true;
            }
        }
        false
    }

    /// Geri Dönüşüm Kutusu içindeki öğeleri listeler.
    pub fn list_trash(&self) -> Vec<VfsItem> {
        self.list_dir(TRASH_DIR)
    }

    /// Geri Dönüşüm Kutusu'ndaki bir öğeyi hedef klasöre geri yükler.
    /// `trash_path` çöp kutusundaki tam yoldur, `dest_dir` ise hedef klasör yoludur.
    pub fn restore_from_trash(&mut self, trash_path: &str, dest_dir: &str) -> bool {
        let src_norm = Self::normalize_path(trash_path);
        if !src_norm.starts_with(TRASH_DIR) {
            return false;
        }
        let name = Self::file_name_from_path(&src_norm);
        let dest_norm = Self::normalize_path(dest_dir);
        let mut target = dest_norm.clone();
        if !target.ends_with('/') {
            target.push('/');
        }
        target.push_str(&name);
        // Hedefte aynı isim varsa numaralandır.
        let mut counter = 1u32;
        while self.items.iter().any(|it| it.path == target) {
            target = dest_norm.clone();
            if !target.ends_with('/') {
                target.push('/');
            }
            target.push_str(&name);
            target.push('_');
            Self::push_u32(&mut target, counter);
            counter += 1;
        }
        for it in &mut self.items {
            if it.path == src_norm {
                it.path = target.clone();
                it.name = Self::file_name_from_path(&target);
                return true;
            }
        }
        false
    }

    /// Geri Dönüşüm Kutusu'nu tamamen boşaltır (kalıcı silme).
    pub fn empty_trash(&mut self) -> usize {
        let before = self.items.len();
        self.items.retain(|it| !it.path.starts_with(TRASH_DIR));
        before - self.items.len()
    }

    /// Geri Dönüşüm Kutusu'nda öğe olup olmadığını döndürür.
    pub fn trash_has_items(&self) -> bool {
        self.items.iter().any(|it| it.path.starts_with(TRASH_DIR) && it.path != TRASH_DIR)
    }

    /// Geri Dönüşüm Kutusu'ndaki öğe sayısı.
    pub fn trash_count(&self) -> usize {
        self.items.iter().filter(|it| it.path.starts_with(TRASH_DIR) && it.path != TRASH_DIR).count()
    }

    /// Verilen normalize edilmiş yolun çöp kutusu altında olup olmadığını döndürür.
    fn is_in_trash_path(norm: &str) -> bool {
        if let Some(rest) = norm.strip_prefix(TRASH_DIR) {
            return rest.is_empty() || rest.starts_with('/');
        }
        false
    }

    /// `String`'e u32 değeri (ondalık) ekler.
    fn push_u32(s: &mut String, mut v: u32) {
        let mut buf = [0u8; 10];
        let mut n = 0usize;
        if v == 0 {
            s.push('0');
            return;
        }
        while v > 0 && n < buf.len() {
            buf[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
        }
        while n > 0 {
            n -= 1;
            s.push(buf[n] as char);
        }
    }
    pub fn list_dir(&self, dir_path: &str) -> Vec<VfsItem> {
        let norm = Self::normalize_path(dir_path);
        let mut results = Vec::new();

        // Sabit Disk (HDD/QCOW2) ve USB disk görünümü
        if (norm == "/media/usb0" || norm == "/media/hdd0") && diskfs::usb_available() {
            if !diskfs::listed() {
                diskfs::refresh();
            }
            for i in 0..diskfs::entry_count() {
                if let Some(entry) = diskfs::entry(i) {
                    let mut path = norm.clone();
                    path.push('/');
                    path.push_str(entry.as_str());
                    results.push(VfsItem {
                        path,
                        name: String::from(entry.as_str()),
                        node_type: if entry.is_dir { NodeType::Directory } else { NodeType::File },
                        size: entry.size,
                        data: Vec::new(),
                        is_readonly: false,
                    });
                }
            }
            return results;
        }

        // CD-ROM görünümü
        if norm == "/media/cdrom" && diskfs::cd_available() {
            if !diskfs::listed() {
                diskfs::refresh();
            }
            for i in 0..diskfs::entry_count() {
                if let Some(entry) = diskfs::entry(i) {
                    let mut path = norm.clone();
                    path.push('/');
                    path.push_str(entry.as_str());
                    results.push(VfsItem {
                        path,
                        name: String::from(entry.as_str()),
                        node_type: if entry.is_dir { NodeType::Directory } else { NodeType::File },
                        size: entry.size,
                        data: Vec::new(),
                        is_readonly: true,
                    });
                }
            }
            return results;
        }

        // VFS dizin taraması
        for it in &self.items {
            if it.path == norm {
                continue;
            }
            let parent = Self::parent_path_from_path(&it.path);
            if parent == norm {
                results.push(it.clone());
            }
        }

        results
    }

    pub fn exists(&self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        self.items.iter().any(|it| it.path == norm)
    }

    pub fn is_dir(&self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        self.items.iter().any(|it| it.path == norm && it.node_type == NodeType::Directory)
    }

    pub fn find_file(&self, name: &str) -> Option<&VfsNode> {
        for node_opt in self.nodes.iter() {
            if let Some(node) = node_opt {
                if node.node_type == NodeType::File && node.name == name {
                    return Some(node);
                }
            }
        }
        None
    }
}
