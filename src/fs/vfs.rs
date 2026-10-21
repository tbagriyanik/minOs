#![allow(dead_code, static_mut_refs)]

//! Sanal Dosya Sistemi (VFS) — Klasör Desteği, Dosya Açma, Kaydetme, Silme ve Disk Entegrasyonu.
//!
//! Bu modül tam bir dizin hiyerarşisi (/root, /root/Documents, /media/usb0, vb.)
//! ve dinamik dosya/klasör yönetimi sağlar. `/media/usb0` ve `/media/cdrom` yolları
//! otomatik olarak ATA/FAT32 ve ISO9660 sürücülerine yönlendirilir.

extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
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
    pub large_files: Vec<(String, Arc<[u8]>)>,
    /// Açılış sırasında (varsayılan ağaç kurulurken) anlık görüntünün
    /// diskteki kaydı ezmemesi için geçici olarak kapatılır.
    suppress_sync: bool,
}

/// Açılışlar arası kalıcılık: VFS ağacının tamamı (klasörler, dosyalar,
/// içerikler, çöp kutusu) FAT kök dizinindeki `VFS.DAT` dosyasına serileşir
/// ve açılışta geri yüklenir. Böylece tüm dosya/klasör işlemleri restart
/// sonrasında geri gelir.
const SNAPSHOT_NAME: &str = "VFS.DAT";
const SNAPSHOT_MAGIC: &[u8; 6] = b"MVFS01";
const SNAPSHOT_MAX: usize = 16 * 1024 * 1024;

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
            large_files: Vec::new(),
            suppress_sync: true,
        };

        // Standart Kök Dizin Hiyerarşisi
        vfs.init_default_hierarchy();
        // Önceki oturumdan kalma anlık görüntü varsa onu yükle (tüm
        // dosya/klasör işlemleri restart sonrasında geri gelir).
        let mut loaded_snapshot = false;
        if let Some(blob) = diskfs::load_file_alloc(SNAPSHOT_NAME) {
            if let Some((items, large)) = Self::parse_snapshot(&blob) {
                vfs.items = items;
                vfs.large_files = large;
                loaded_snapshot = true;
            }
        }
        // Anlık görüntü yoksa ama disk kuruluysa, gerçek FAT32 ağacından yükle.
        if !loaded_snapshot && diskfs::is_installed() {
            Self::load_tree_walk(&mut vfs, "/", "/", 0);
        }
        // Varsayılan ağaç/yükleme tamamlandı; bundan sonraki değişiklikler
        // anlık görüntüye (ve FAT32 ağacına) yazılır.
        vfs.suppress_sync = false;
        vfs
    }

    /// Diskteki FAT32 ağacını özyinelemeli olarak VFS'e yükler.
    fn load_tree_walk(vfs: &mut VirtualFileSystem, disk_path: &str, vfs_path: &str, depth: usize) {
        if depth > 8 { return; }
        let Some(entries) = diskfs::fat_list_dir_path(disk_path) else { return; };
        for (name, is_dir, size) in entries {
            if name.eq_ignore_ascii_case(SNAPSHOT_NAME) || name.eq_ignore_ascii_case("MINOS.SYS") { continue; }
            // Windows sistem dizinleri VFS'e alınmaz (geri yazımda çöp olmasın).
            if name.eq_ignore_ascii_case("System Volume Information")
                || name.eq_ignore_ascii_case("$RECYCLE.BIN")
                || name.eq_ignore_ascii_case("FOUND.000") { continue; }
            let mut child_vfs = String::from(vfs_path);
            if !child_vfs.ends_with('/') { child_vfs.push('/'); }
            child_vfs.push_str(&name);
            let mut child_disk = String::from(disk_path);
            if !child_disk.ends_with('/') { child_disk.push('/'); }
            child_disk.push_str(&name);
            if vfs.items.iter().any(|it| it.path == child_vfs) {
                if is_dir { Self::load_tree_walk(vfs, &child_disk, &child_vfs, depth + 1); }
                continue;
            }
            if is_dir {
                vfs.items.push(VfsItem { path: child_vfs.clone(), name, node_type: NodeType::Directory, size: 0, data: Vec::new(), is_readonly: false });
                Self::load_tree_walk(vfs, &child_disk, &child_vfs, depth + 1);
            } else if size <= 64 * 1024 {
                let data = diskfs::fat_read_file_path(&child_disk).unwrap_or_default();
                vfs.items.push(VfsItem { path: child_vfs, name, node_type: NodeType::File, size: data.len(), data, is_readonly: false });
            } else {
                vfs.items.push(VfsItem { path: child_vfs, name, node_type: NodeType::File, size, data: Vec::new(), is_readonly: false });
            }
        }
    }

    fn init_default_hierarchy(&mut self) {
        // Klasörler
        self.create_dir_internal("/");
        self.create_dir_internal("/root");
        self.create_dir_internal("/root/Documents");
        self.create_dir_internal("/root/Media");
        self.create_dir_internal("/root/Downloads");
        self.create_dir_internal("/boot");
        self.create_dir_internal("/bin");
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
            "/root/Documents/sekiller.3d",
            b"# minOs 3D Sahne Testi\n# Desteklenen sekiller: cube, cylinder, sphere, pyramid\nshape=cube\nauto_rotate=true\nwireframe=false\n"
        );
        self.create_file_internal(
            "/root/Documents/kup.3d",
            b"# minOs 3D Kup Modeli\nshape=cube\nauto_rotate=true\nwireframe=false\n"
        );
        self.create_file_internal(
            "/root/Documents/silindir.3d",
            b"# minOs 3D Silindir Modeli\nshape=cylinder\nauto_rotate=true\nwireframe=false\n"
        );
        self.create_file_internal(
            "/root/Documents/kure.3d",
            b"# minOs 3D Kure Modeli\nshape=sphere\nauto_rotate=true\nwireframe=false\n"
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
            "/boot/limine.conf",
            b"timeout: 5\n\n/minOs Installed System\n    protocol: limine\n    kernel_path: boot():/boot/minos-kernel\n    kernel_cmdline: installed\n    resolution: 1280x720x32\n"
        );
        let sample_elf = crate::mm::loader::AppLoader::create_sample_elf();
        self.create_file_internal("/bin/hello.elf", &sample_elf);
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

    /// VFS içeriğini tek bir blob'a serileştirir.
    fn build_snapshot(items: &[VfsItem], large_files: &[(String, Arc<[u8]>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(SNAPSHOT_MAGIC);
        let count = items.len().min(u32::MAX as usize) as u32;
        out.extend_from_slice(&count.to_le_bytes());
        for it in items.iter().take(count as usize) {
            let is_dir = it.node_type == NodeType::Directory;
            let mut kind = if is_dir { 1u8 } else { 0u8 };
            let mut data: &[u8] = &it.data;
            let owned: Option<Arc<[u8]>>;
            if !is_dir && data.is_empty() {
                if let Some((_, arc)) = large_files.iter().find(|(p, _)| *p == it.path) {
                    kind = 2;
                    owned = Some(arc.clone());
                    data = &owned.as_ref().unwrap();
                }
            }
            out.push(kind);
            let path_bytes = it.path.as_bytes();
            let plen = path_bytes.len().min(u16::MAX as usize) as u16;
            out.extend_from_slice(&plen.to_le_bytes());
            out.extend_from_slice(&path_bytes[..plen as usize]);
            out.extend_from_slice(&(it.size as u32).to_le_bytes());
            if is_dir {
                out.extend_from_slice(&0u32.to_le_bytes());
            } else {
                let dlen = data.len().min(u32::MAX as usize) as u32;
                out.extend_from_slice(&dlen.to_le_bytes());
                out.extend_from_slice(&data[..dlen as usize]);
            }
            if out.len() > SNAPSHOT_MAX {
                break;
            }
        }
        out
    }

    /// Anlık görüntü blob'unu çözer; bozuksa `None` döner.
    fn parse_snapshot(blob: &[u8]) -> Option<(Vec<VfsItem>, Vec<(String, Arc<[u8]>)>)> {
        if blob.len() < 10 || &blob[..6] != SNAPSHOT_MAGIC {
            return None;
        }
        let mut off = 6usize;
        let count = u32::from_le_bytes([blob[off], blob[off + 1], blob[off + 2], blob[off + 3]]) as usize;
        off += 4;
        if count > 4096 {
            return None;
        }
        let mut items = Vec::new();
        let mut large: Vec<(String, Arc<[u8]>)> = Vec::new();
        for _ in 0..count {
            if off + 1 + 2 + 4 + 4 > blob.len() {
                return None;
            }
            let kind = blob[off];
            off += 1;
            if kind > 2 {
                return None;
            }
            let plen = u16::from_le_bytes([blob[off], blob[off + 1]]) as usize;
            off += 2;
            if plen == 0 || plen > 256 || off + plen > blob.len() {
                return None;
            }
            let path = String::from(core::str::from_utf8(&blob[off..off + plen]).ok()?);
            off += plen;
            let size = u32::from_le_bytes([blob[off], blob[off + 1], blob[off + 2], blob[off + 3]]) as usize;
            off += 4;
            let dlen = u32::from_le_bytes([blob[off], blob[off + 1], blob[off + 2], blob[off + 3]]) as usize;
            off += 4;
            if off + dlen > blob.len() {
                return None;
            }
            let name = Self::file_name_from_path(&path);
            if kind == 1 {
                if dlen != 0 {
                    return None;
                }
                items.push(VfsItem { path, name, node_type: NodeType::Directory, size: 0, data: Vec::new(), is_readonly: false });
            } else {
                let bytes = blob[off..off + dlen].to_vec();
                off += dlen;
                if kind == 2 {
                    let arc: Arc<[u8]> = Arc::from(bytes.into_boxed_slice());
                    large.push((path.clone(), arc));
                    items.push(VfsItem { path, name, node_type: NodeType::File, size, data: Vec::new(), is_readonly: false });
                } else {
                    items.push(VfsItem { path, name, node_type: NodeType::File, size, data: bytes, is_readonly: false });
                }
            }
        }
        Some((items, large))
    }

    /// VFS'deki son değişikliği diske kalıcı olarak yazar.
    /// Disk yoksa/yazılamazsa sessizce vazgeçer (canlı ortam).
    fn sync_snapshot(&self) {
        if self.suppress_sync { return; }
        let _ = self.persist_snapshot();
    }

    /// Anlık görüntüyü hemen diske yazar; kurulum sırasında da kullanılır.
    /// Başarılıysa `true` döner.
    pub fn persist_snapshot(&self) -> bool {
        let blob = Self::build_snapshot(&self.items, &self.large_files);
        if blob.len() > SNAPSHOT_MAX {
            return false;
        }
        diskfs::save_file_by_name(SNAPSHOT_NAME, &blob)
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

    // -----------------------------------------------------------------------
    // Diske (FAT32 kök ağacına) yansıtma
    // -----------------------------------------------------------------------

    /// VFS yolunu disk üzerindeki FAT32 yoluna çevirir.
    /// `None` → bu yol diske yazılmaz (sanal/canlı ortam görünümü).
    fn disk_path_for(vfs_path: &str) -> Option<String> {
        let norm = Self::normalize_path(vfs_path);
        let n = norm.as_str();
        if n == "/media/usb0" || n.starts_with("/media/usb0/")
            || n == "/media/cdrom" || n.starts_with("/media/cdrom/") {
            return None;
        }
        if n == "/media/hdd0" {
            return Some(String::from("/"));
        }
        if let Some(rest) = n.strip_prefix("/media/hdd0/") {
            let mut p = String::from("/");
            p.push_str(rest);
            return Some(p);
        }
        Some(norm)
    }

    fn disk_create_dir(vfs_path: &str) -> bool {
        if !diskfs::fat_ready() { return true; }
        match Self::disk_path_for(vfs_path) {
            Some(dp) => dp == "/" || diskfs::fat_create_dir_all(&dp),
            None => true,
        }
    }

    fn disk_write_file(vfs_path: &str, data: &[u8]) -> bool {
        if !diskfs::fat_ready() { return true; }
        match Self::disk_path_for(vfs_path) {
            Some(dp) => dp != "/" && diskfs::fat_write_file_path(&dp, data),
            None => true,
        }
    }

    fn disk_delete(vfs_path: &str) -> bool {
        if !diskfs::fat_ready() { return true; }
        match Self::disk_path_for(vfs_path) {
            Some(dp) => dp != "/" && diskfs::fat_delete_path(&dp),
            None => true,
        }
    }

    fn disk_rename(from: &str, to: &str) -> bool {
        if !diskfs::fat_ready() { return true; }
        match (Self::disk_path_for(from), Self::disk_path_for(to)) {
            (Some(a), Some(b)) => a == "/" || b == "/" || diskfs::fat_rename_path(&a, &b),
            _ => true,
        }
    }

    /// Tam ağaç aynalaması sırasında diske yazılacak hedefi döndürür.
    /// `/media/*` sanal görünümler (disk köküne eşlenenler) atlanır.
    fn mirror_target(vfs_path: &str) -> Option<String> {
        let norm = Self::normalize_path(vfs_path);
        if norm == "/media/hdd0" || norm.starts_with("/media/hdd0/") {
            return None;
        }
        // Windows sistem dizinleri aynalanmaz.
        for comp in norm.split('/').filter(|c| !c.is_empty()) {
            if comp.eq_ignore_ascii_case("System Volume Information")
                || comp.eq_ignore_ascii_case("$RECYCLE.BIN")
                || comp.eq_ignore_ascii_case("FOUND.000") {
                return None;
            }
        }
        match Self::disk_path_for(&norm) {
            Some(dp) if dp != "/" => Some(dp),
            _ => None,
        }
    }

    /// VFS ağacının tamamını FAT32 kök ağacına yansıtır.
    /// Dönen değer: (yazılan klasör sayısı, yazılan dosya sayısı).
    pub fn sync_all_to_disk(&self) -> (usize, usize) {
        if !diskfs::fat_ready() { return (0, 0); }
        let mut dirs = 0usize;
        let mut files = 0usize;

        // Önce ana dizinler oluşsun diye klasörleri yol uzunluğuna göre sırala.
        let mut dir_paths: Vec<&str> = self.items.iter()
            .filter(|it| it.node_type == NodeType::Directory)
            .map(|it| it.path.as_str())
            .collect();
        dir_paths.sort_by_key(|p| p.len());
        dir_paths.dedup();
        for p in dir_paths {
            if Self::mirror_target(p).is_some() && Self::disk_create_dir(p) {
                dirs += 1;
            }
        }

        for it in &self.items {
            if it.node_type != NodeType::File { continue; }
            if Self::mirror_target(&it.path).is_none() { continue; }
            let data: &[u8] = if let Some((_, arc)) = self.large_files.iter().find(|(sp, _)| *sp == it.path) {
                arc
            } else {
                &it.data
            };
            // Verisi başka yerde tutulan büyük dosya meta kaydını atla.
            if data.is_empty() && it.size > 0 { continue; }
            if Self::disk_write_file(&it.path, data) {
                files += 1;
            }
        }
        (dirs, files)
    }

    /// Bir yolun (ve varsa alt ağacının) diskteki aynasını oluşturur.
    fn mirror_subtree_to_disk(&self, vfs_path: &str) {
        if !diskfs::fat_ready() { return; }
        let norm = Self::normalize_path(vfs_path);
        let mut prefix = norm.clone();
        if !prefix.ends_with('/') { prefix.push('/'); }

        let mut dir_paths: Vec<&str> = self.items.iter()
            .filter(|it| (it.path == norm || it.path.starts_with(&prefix)) && it.node_type == NodeType::Directory)
            .map(|it| it.path.as_str())
            .collect();
        dir_paths.sort_by_key(|p| p.len());
        for p in dir_paths {
            if Self::mirror_target(p).is_some() { Self::disk_create_dir(p); }
        }

        for it in &self.items {
            if it.node_type != NodeType::File { continue; }
            if !(it.path == norm || it.path.starts_with(&prefix)) { continue; }
            if Self::mirror_target(&it.path).is_none() { continue; }
            let data: &[u8] = if let Some((_, arc)) = self.large_files.iter().find(|(sp, _)| *sp == it.path) {
                arc
            } else {
                &it.data
            };
            if data.is_empty() && it.size > 0 { continue; }
            Self::disk_write_file(&it.path, data);
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
        self.sync_snapshot();
        true
    }

    fn create_file_internal(&mut self, path: &str, data: &[u8]) -> bool {
        let norm = Self::normalize_path(path);
        let name = Self::file_name_from_path(&norm);
        for it in &mut self.items {
            if it.path == norm {
                it.data = data.to_vec();
                it.size = data.len();
                self.sync_snapshot();
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
        self.sync_snapshot();
        true
    }

    /// Yeni bir klasör oluşturur.
    pub fn create_dir(&mut self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        let parent = Self::parent_path_from_path(&norm);
        if norm == "/" { return false; }
        if !self.is_dir(&parent) && parent != "/" {
            self.create_dir(&parent);
        }
        let already = self.is_dir(&norm);
        if !already {
            let name = Self::file_name_from_path(&norm);
            self.items.push(VfsItem {
                path: norm.clone(),
                name,
                node_type: NodeType::Directory,
                size: 0,
                data: Vec::new(),
                is_readonly: false,
            });
        }
        Self::disk_create_dir(&norm);
        self.sync_snapshot();
        true
    }

    /// Yeni dosya oluşturur veya üzerine yazar.
    pub fn create_file(&mut self, path: &str, data: &[u8]) -> bool {
        let norm = Self::normalize_path(path);
        if norm == "/" { return false; }
        let parent = Self::parent_path_from_path(&norm);
        if !self.is_dir(&parent) && parent != "/" {
            self.create_dir(&parent);
        }
        let name = Self::file_name_from_path(&norm);
        let mut found = false;
        for it in &mut self.items {
            if it.path == norm {
                if it.node_type == NodeType::Directory { return false; }
                it.data = data.to_vec();
                it.size = data.len();
                found = true;
                break;
            }
        }
        if !found {
            self.items.push(VfsItem {
                path: norm.clone(),
                name,
                node_type: NodeType::File,
                size: data.len(),
                data: data.to_vec(),
                is_readonly: false,
            });
        }
        self.large_files.retain(|(p, _)| *p != norm);
        Self::disk_write_file(&norm, data);
        self.sync_snapshot();
        true
    }

    /// Dosyaya yazar veya üzerine kaydeder.
    pub fn write_file(&mut self, path_or_name: &str, data: &[u8]) -> bool {
        let norm = Self::normalize_path(path_or_name);
        if norm == "/" { return false; }

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
                self.large_files.retain(|(p, _)| *p != it.path);
                let p = it.path.clone();
                Self::disk_write_file(&p, data);
                self.sync_snapshot();
                return true;
            }
        }

        // Bulunamadıysa yeni dosya olarak oluştur
        self.create_file(&norm, data)
    }

    /// Büyük dosyalar için baytları çağıran uygulamada tutup VFS'de yalnızca
    /// dosya kaydı oluşturur/günceller. Çağıran tekrar okuma için sahibi olur.
    pub fn write_large_file(&mut self, path: &str, data: Arc<[u8]>) -> bool {
        let norm = Self::normalize_path(path);
        if norm == "/" { return false; }
        let size = data.len();
        if let Some((_, existing)) = self.large_files.iter_mut().find(|(saved_path, _)| *saved_path == norm) {
            *existing = data.clone();
        } else {
            self.large_files.push((norm.clone(), data.clone()));
        }
        let mut found = false;
        for item in &mut self.items {
            if item.path == norm {
                if item.node_type == NodeType::Directory { return false; }
                item.size = size;
                item.data.clear();
                found = true;
                break;
            }
        }
        if !found {
            let parent = Self::parent_path_from_path(&norm);
            if !self.is_dir(&parent) && parent != "/" { self.create_dir(&parent); }
            self.items.push(VfsItem {
                name: Self::file_name_from_path(&norm),
                path: norm.clone(),
                node_type: NodeType::File,
                size,
                data: Vec::new(),
                is_readonly: false,
            });
        }
        Self::disk_write_file(&norm, &data);
        self.sync_snapshot();
        true
    }

    pub fn rename(&mut self, old_path: &str, new_path: &str) -> bool {
        let old = Self::normalize_path(old_path);
        let new = Self::normalize_path(new_path);
        if old == "/" || new == "/" || self.exists(&new) { return false; }
        let existed = self.exists(&old);
        Self::disk_rename(&old, &new);
        if !existed { return false; }
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
        if changed {
            for (saved_path, _) in &mut self.large_files {
                if *saved_path == old {
                    *saved_path = new.clone();
                } else if saved_path.starts_with(&prefix) {
                    let suffix = String::from(&saved_path[prefix.len()..]);
                    let mut child = new.clone(); child.push('/'); child.push_str(&suffix);
                    *saved_path = child;
                }
            }
            self.sync_snapshot();
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

        if Self::parent_path_from_path(&norm) == "/root/Documents" {
            let filename = Self::file_name_from_path(&norm);
            if diskfs::load_file(&filename) > 0 { return Some(diskfs::content()); }
        }

        // Sabit Disk (HDD/QCOW2), USB veya CD-ROM ortamından okuma
        if norm.starts_with("/media/usb0/") || norm.starts_with("/media/hdd0/") || norm.starts_with("/media/cdrom/") {
            let filename = Self::file_name_from_path(&norm);
            if diskfs::load_file(&filename) > 0 {
                return Some(diskfs::content());
            }
        }

        // Geriye dönük uyumlululuk: static nodes tablosu (önbellekli arama)
        for i in 0..16 {
            if let Some(node) = &self.nodes[i] {
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

    pub fn read_large_file(&self, path: &str) -> Option<Arc<[u8]>> {
        let norm = Self::normalize_path(path);
        self.large_files.iter().find(|(saved_path, _)| *saved_path == norm).map(|(_, data)| data.clone())
            .or_else(|| {
                Self::disk_path_for(&norm).and_then(|dp| {
                    if dp == "/" { None } else { diskfs::fat_read_file_path(&dp).map(Arc::from) }
                })
            })
    }

    /// Dosya siler.
    pub fn delete_file(&mut self, path_or_name: &str) -> bool {
        let norm = Self::normalize_path(path_or_name);

        if norm.starts_with("/media/usb0/") || norm.starts_with("/media/hdd0/") {
            let disk_file = Self::file_name_from_path(&norm);
            diskfs::delete_file_by_name(&disk_file);
        }
        Self::disk_delete(&norm);

        let mut found_idx = None;
        for (i, it) in self.items.iter().enumerate() {
            if (it.path == norm || it.name == path_or_name) && it.node_type == NodeType::File {
                found_idx = Some(i);
                break;
            }
        }
        if let Some(idx) = found_idx {
            self.items.remove(idx);
            self.large_files.retain(|(p, _)| *p != norm);
            self.sync_snapshot();
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
        Self::disk_delete(&norm);
        let mut found_idx = None;
        for (i, it) in self.items.iter().enumerate() {
            if it.path == norm && it.node_type == NodeType::Directory {
                found_idx = Some(i);
                break;
            }
        }
        if let Some(idx) = found_idx {
            self.items.remove(idx);
            self.sync_snapshot();
            return true;
        }
        false
    }

    /// Silinen bir öğeyi kalıcı olarak silmek yerine Geri Dönüşüm Kutusu'na taşır.
    /// Dosya veya klasör ağacı olabilir. Aynı isimde öğe varsa sonuna numara eklenir.
    /// Başarılıysa `true` döner ve öğe artık `TRASH_DIR` altında yaşar.
    pub fn move_to_trash(&mut self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        // Zaten çöp kutusunda olan ya da kök dizini taşımaya çalışmak anlamsız.
        if norm == "/" || norm == TRASH_DIR || Self::is_in_trash_path(&norm) {
            return false;
        }
        // Öğe var mı?
        let src = self.items.iter().find(|it| it.path == norm).cloned().or_else(|| {
            let parent = Self::parent_path_from_path(&norm);
            self.list_dir(&parent).into_iter().find(|it| it.path == norm)
        });
        let Some(src) = src else { return false; };
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
        // Disk aynasından sil; çöp kutusu girdisi RAM'de tutulur.
        Self::disk_delete(&norm);
        // Klasörün ve tüm alt öğelerinin yollarını birlikte güncelle.
        let prefix = if norm.ends_with('/') { norm.clone() } else { let mut p = norm.clone(); p.push('/'); p };
        let src_is_dir = src.node_type == NodeType::Directory;
        let mut moved_root = false;
        for it in &mut self.items {
            if it.path == norm {
                it.path = target.clone();
                it.name = Self::file_name_from_path(&target);
                moved_root = true;
            } else if src_is_dir && it.path.starts_with(&prefix) {
                let suffix = String::from(&it.path[norm.len()..]);
                let mut new_path = target.clone();
                new_path.push_str(&suffix);
                it.path = new_path;
            }
        }
        if !moved_root {
            let mut copy = src;
            copy.path = target.clone();
            copy.name = Self::file_name_from_path(&target);
            self.items.push(copy);
        }
        for (saved_path, _) in &mut self.large_files {
            if *saved_path == norm {
                *saved_path = target.clone();
            } else if src_is_dir && saved_path.starts_with(&prefix) {
                let suffix = String::from(&saved_path[norm.len()..]);
                let mut new_path = target.clone();
                new_path.push_str(&suffix);
                *saved_path = new_path;
            }
        }
        self.sync_snapshot();
        true
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
                for (saved_path, _) in &mut self.large_files {
                    if *saved_path == src_norm {
                        *saved_path = target.clone();
                    }
                }
                self.sync_snapshot();
                return true;
            }
        }
        false
    }

    /// Bir dosya adını kök ve uzantı olarak ayırır (örn: ("notlar", ".txt") veya ("klasor", "")).
    pub fn split_name_ext(name: &str) -> (&str, &str) {
        if let Some(dot_idx) = name.rfind('.') {
            if dot_idx > 0 {
                return (&name[..dot_idx], &name[dot_idx..]);
            }
        }
        (name, "")
    }

    /// Bir klasörü ve içindeki tüm alt öğeleri siler.
    pub fn delete_tree(&mut self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        if norm == "/" { return false; }
        let prefix = if norm.ends_with('/') { norm.clone() } else { let mut p = norm.clone(); p.push('/'); p };
        self.items.retain(|it| it.path != norm && !it.path.starts_with(&prefix));
        self.large_files.retain(|(p, _)| *p != norm && !p.starts_with(&prefix));
        Self::disk_delete(&norm);
        self.sync_snapshot();
        true
    }

    /// Dosya veya klasörü belirtilen hedef dizine kopyalar.
    /// Hedefte aynı isimde öğe varsa numaralandırılmış benzersiz bir isim üretilir.
    pub fn copy_item(&mut self, src_path: &str, dest_dir: &str) -> Result<String, &'static str> {
        let src_norm = Self::normalize_path(src_path);
        let dest_norm = Self::normalize_path(dest_dir);
        if src_norm == "/" || dest_norm.is_empty() {
            return Err("Gecersiz yol / Invalid path");
        }
        if !self.is_dir(&dest_norm) && dest_norm != "/" {
            return Err("Hedef klasor bulunamadi / Destination folder not found");
        }
        let src_is_dir = self.is_dir(&src_norm);
        if src_is_dir {
            if dest_norm == src_norm || dest_norm.starts_with(&(src_norm.clone() + "/")) {
                return Err("Hedef kaynak klasorun icinde / Destination is inside source");
            }
        }

        let base_name = Self::file_name_from_path(&src_norm);
        let (stem, ext) = Self::split_name_ext(&base_name);
        let same_parent = Self::parent_path_from_path(&src_norm) == dest_norm;

        let mut target = dest_norm.clone();
        if !target.ends_with('/') { target.push('/'); }
        if same_parent {
            target.push_str(stem);
            target.push_str("_kopya");
            target.push_str(ext);
        } else {
            target.push_str(&base_name);
        }

        let mut counter = 1u32;
        while self.exists(&target) {
            target = dest_norm.clone();
            if !target.ends_with('/') { target.push('/'); }
            target.push_str(stem);
            if same_parent { target.push_str("_kopya_"); } else { target.push('_'); }
            Self::push_u32(&mut target, counter);
            target.push_str(ext);
            counter += 1;
        }

        if !src_is_dir {
            if self.is_large_file_metadata(&src_norm) {
                if let Some(arc) = self.read_large_file(&src_norm) {
                    self.write_large_file(&target, arc);
                    return Ok(target);
                }
            }
            if let Some(data) = self.read_file(&src_norm).map(|b| b.to_vec()) {
                self.write_file(&target, &data);
                Ok(target)
            } else {
                Err("Dosya okunamadi / Could not read file")
            }
        } else {
            self.create_dir(&target);
            let src_prefix = if src_norm.ends_with('/') { src_norm.clone() } else { let mut p = src_norm.clone(); p.push('/'); p };
            let items_to_copy: Vec<(String, bool)> = self.items.iter()
                .filter(|it| it.path.starts_with(&src_prefix))
                .map(|it| (it.path.clone(), it.node_type == NodeType::Directory))
                .collect();

            for (child_path, is_child_dir) in items_to_copy {
                let rel = &child_path[src_prefix.len()..];
                let mut child_target = target.clone();
                child_target.push('/');
                child_target.push_str(rel);
                if is_child_dir {
                    self.create_dir(&child_target);
                } else if let Some(data) = self.read_file(&child_path).map(|b| b.to_vec()) {
                    self.write_file(&child_target, &data);
                }
            }
            Ok(target)
        }
    }

    /// Dosya veya klasörü belirtilen hedef dizine taşır.
    pub fn move_item(&mut self, src_path: &str, dest_dir: &str) -> Result<String, &'static str> {
        let src_norm = Self::normalize_path(src_path);
        let dest_norm = Self::normalize_path(dest_dir);
        if src_norm == "/" || dest_norm.is_empty() {
            return Err("Gecersiz yol / Invalid path");
        }
        if dest_norm == TRASH_DIR {
            if self.move_to_trash(&src_norm) {
                return Ok(String::from(TRASH_DIR));
            } else {
                return Err("Cop kutusuna tasinamadi / Could not move to trash");
            }
        }
        if !self.is_dir(&dest_norm) && dest_norm != "/" {
            return Err("Hedef klasor bulunamadi / Destination folder not found");
        }
        let src_is_dir = self.is_dir(&src_norm);
        if src_is_dir {
            if dest_norm == src_norm || dest_norm.starts_with(&(src_norm.clone() + "/")) {
                return Err("Hedef kaynak klasorun icinde / Destination is inside source");
            }
        }
        if Self::parent_path_from_path(&src_norm) == dest_norm {
            return Err("Zaten bu klasorde / Already in this folder");
        }

        let target = self.copy_item(&src_norm, &dest_norm)?;
        if src_is_dir {
            self.delete_tree(&src_norm);
        } else {
            self.delete_file(&src_norm);
        }
        Ok(target)
    }

    /// Geri Dönüşüm Kutusu'nu tamamen boşaltır (kalıcı silme).
    pub fn empty_trash(&mut self) -> usize {
        let before = self.items.len();
        self.items.retain(|it| !it.path.starts_with(TRASH_DIR));
        self.large_files.retain(|(p, _)| !p.starts_with(TRASH_DIR));
        let removed = before - self.items.len();
        if removed > 0 {
            self.sync_snapshot();
        }
        removed
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

        if norm == "/root/Documents" && diskfs::usb_available() {
            if !diskfs::listed() { diskfs::refresh(); }
            for i in 0..diskfs::entry_count() {
                let Some(entry) = diskfs::entry(i) else { continue; };
                let name = String::from(entry.as_str());
                if name.eq_ignore_ascii_case("settings.cfg") || name.eq_ignore_ascii_case("benioku.txt")
                    || name.eq_ignore_ascii_case(SNAPSHOT_NAME) || name.eq_ignore_ascii_case("MINOS.SYS") { continue; }
                if results.iter().any(|item: &VfsItem| item.name.eq_ignore_ascii_case(&name)) { continue; }
                let mut path = norm.clone(); path.push('/'); path.push_str(&name);
                results.push(VfsItem { path, name, node_type: if entry.is_dir { NodeType::Directory } else { NodeType::File }, size: entry.size,
                    data: Vec::new(), is_readonly: false });
            }
            // Gerçek iç içe ağaçtan ek girdiler (kök-düz liste + dedup).
            Self::inject_disk_entries(&mut results, "/root/Documents", &norm);
        }

        // Sabit Disk (HDD/QCOW2) ve USB disk görünümü
        if (norm == "/media/usb0" || norm == "/media/hdd0") && diskfs::usb_available() {
            if !diskfs::listed() {
                diskfs::refresh();
            }
            for i in 0..diskfs::entry_count() {
                if let Some(entry) = diskfs::entry(i) {
                    if entry.as_str().eq_ignore_ascii_case(SNAPSHOT_NAME)
                        || entry.as_str().eq_ignore_ascii_case("MINOS.SYS") { continue; }
                    if results.iter().any(|item: &VfsItem| item.name.eq_ignore_ascii_case(entry.as_str())) { continue; }
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
            if norm == "/media/hdd0" {
                // Kök dizin düz listesine ek olarak gerçek FAT32 kök ağacı.
                Self::inject_disk_entries(&mut results, "/", &norm);
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
            // Masaüstü, VFS'nin ayrı bir kullanıcı alanıdır; kök dizinde
            // sıradan klasör olarak listelenmez.
            if norm == "/" && it.path == "/desktop" { continue; }
            let parent = Self::parent_path_from_path(&it.path);
            if parent == norm && !results.iter().any(|existing| existing.name.eq_ignore_ascii_case(&it.name)) {
                results.push(it.clone());
            }
        }

        results
    }

    /// FAT32 ağacındaki girdileri sonuç listesine ekler (yinelenenleri atlar).
    fn inject_disk_entries(results: &mut Vec<VfsItem>, disk_path: &str, vfs_base: &str) {
        if !diskfs::fat_ready() { return; }
        let Some(entries) = diskfs::fat_list_dir_path(disk_path) else { return; };
        for (name, is_dir, size) in entries {
            if name.eq_ignore_ascii_case(SNAPSHOT_NAME)
                || name.eq_ignore_ascii_case("MINOS.SYS")
                || name.eq_ignore_ascii_case("settings.cfg")
                || name.eq_ignore_ascii_case("benioku.txt")
                || name.eq_ignore_ascii_case("System Volume Information")
                || name.eq_ignore_ascii_case("$RECYCLE.BIN")
                || name.eq_ignore_ascii_case("FOUND.000") { continue; }
            if results.iter().any(|item: &VfsItem| item.name.eq_ignore_ascii_case(&name)) { continue; }
            let mut path = String::from(vfs_base);
            if !path.ends_with('/') { path.push('/'); }
            path.push_str(&name);
            results.push(VfsItem { path, name,
                node_type: if is_dir { NodeType::Directory } else { NodeType::File },
                size, data: Vec::new(), is_readonly: false });
        }
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
        for i in 0..16 {
            if let Some(node) = &self.nodes[i] {
                if node.node_type == NodeType::File && node.name == name {
                    return Some(node);
                }
            }
        }
        None
    }
}
