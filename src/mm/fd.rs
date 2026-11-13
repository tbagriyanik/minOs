//! Dosya Tanıtıcısı (File Descriptor) Yaşam Döngüsü / File Descriptor Lifecycle.
//!
//! Her süreç sabit boyutlu bir fd tablosu taşır (`MAX_FDS`); tablo `Process`
//! içinde `Copy` kalır, çünkü girdiler yalnızca nesne indeksi + offset tutar.
//! Dosya **içeriği** çekirdek genelinde tek bir `FileTable` içinde saklanır ve
//! referans sayılır:
//!
//! * `SYS_OPEN` yolu çözer, içeriği çekirdek tamponuna kopyalar ve bir nesne
//!   açar (aynı yol ikinci kez açılırsa referans artar).
//! * `fork` fd tablosunu devralır; her dolu fd için nesne referansı artar.
//! * `exec` tüm fd'leri kapatır ve 0/1/2 standart tanıtıcılarını yeniden kurar.
//! * `SYS_EXIT` / hata / `wait` ile reaping sırasında kalan fd'ler kapanır;
//!   son referans bırakıldığında içerik serbest kalır.
//!
//! All file contents live in one kernel-side reference-counted table. Process
//! fd tables are fixed-size and `Copy`, so fork/exec/reap can move them
//! without allocating; closing the last reference releases the buffer.
//!
//! Kaynak limitleri: süreç başına `MAX_FDS`, çekirdek genelinde
//! `MAX_OPEN_FILES` nesne ve nesne başına `MAX_FILE_BYTES` içerik.

use alloc::vec::Vec;

/// Süreç başına azami fd sayısı (0/1/2 standart tanıtıcılar dahil).
pub const MAX_FDS: usize = 16;
/// Çekirdek genelinde azami açık dosya nesnesi.
pub const MAX_OPEN_FILES: usize = 32;
/// fd yolu için azami bayt (NUL hariç).
pub const MAX_FD_PATH: usize = 96;
/// Tek bir dosya nesnesinin azami içerik boyu.
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// VFS açılış önbelleği için toplam azami bayt (çekirdek heap koruması).
pub const MAX_CACHE_BYTES: usize = 2 * 1024 * 1024;

/// Standart tanıtıcılar.
pub const FD_STDIN: usize = 0;
pub const FD_STDOUT: usize = 1;
pub const FD_STDERR: usize = 2;
/// Kullanıcı `open` çağrılarının başladığı ilk fd.
pub const FIRST_USER_FD: usize = 3;

/// `open` bayrakları (yalnızca erişim modu; yazma henüz desteklenmiyor).
pub mod open_flags {
    pub const O_RDONLY: u32 = 0;
    pub const O_WRONLY: u32 = 1;
    pub const O_RDWR: u32 = 2;
    pub const O_ACCMODE: u32 = 3;
}

/// fd türü / descriptor kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// Standart girdi (şimdilik EOF).
    Stdin,
    /// Standart çıktı (seri konsol).
    Stdout,
    /// Standart hata (seri konsol).
    Stderr,
    /// Çekirdek dosya nesnesi (`FileTable` indeksi).
    Regular,
}

/// Süreç başına tek bir fd girdisi (küçük ve `Copy`).
#[derive(Debug, Clone, Copy)]
pub struct FileDescriptor {
    pub used: bool,
    pub kind: FileKind,
    /// `FileTable` nesne indeksi (yalnızca `Regular`).
    pub object: u16,
    /// Okuma imleci (bayt).
    pub offset: u64,
    /// `open_flags` erişim modu.
    pub flags: u32,
}

impl FileDescriptor {
    pub const fn empty() -> Self {
        Self {
            used: false,
            kind: FileKind::Regular,
            object: 0,
            offset: 0,
            flags: open_flags::O_RDONLY,
        }
    }

    pub const fn standard(kind: FileKind) -> Self {
        Self {
            used: true,
            kind,
            object: 0,
            offset: 0,
            flags: open_flags::O_RDONLY,
        }
    }
}

/// Süreç fd tablosu tipi.
pub type FdTable = [FileDescriptor; MAX_FDS];

/// fd hata nedenleri / descriptor errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdError {
    /// Tablo dolu.
    NoFreeFd,
    /// Nesne tablosu dolu.
    NoFreeObject,
    /// Yol geçersiz (boş, göreli, `..` içeriyor veya çok uzun).
    InvalidPath,
    /// Dosya bulunamadı.
    NotFound,
    /// Desteklenmeyen erişim modu.
    Unsupported,
    /// Dosya boyutu `MAX_FILE_BYTES` sınırını aşıyor.
    TooLarge,
    /// Bellek yetersiz.
    OutOfMemory,
}

/// Çekirdek genelinde açık dosya nesnesi.
pub struct FileObject {
    pub used: bool,
    /// VFS önbelleği: referans sıfır olsa da yerinde kalır, yer gerekirse
    /// tahliye edilebilir.
    pub pinned: bool,
    /// Kaç süreç fd'si bu nesneyi gösteriyor.
    pub refs: u16,
    pub path_len: u16,
    pub path: [u8; MAX_FD_PATH],
    pub data: Vec<u8>,
}

impl FileObject {
    /// `const` başlatma için boş nesne. `Vec::new()` const olduğundan statik
    /// `ProcessManager` başlatımında kullanılabilir.
    pub const fn empty() -> Self {
        Self {
            used: false,
            pinned: false,
            refs: 0,
            path_len: 0,
            path: [0; MAX_FD_PATH],
            data: Vec::new(),
        }
    }

    pub fn path_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.path[..self.path_len as usize]).ok()
    }
}

/// Çekirdek genelinde dosya nesnesi tablosu.
pub struct FileTable {
    pub objects: [FileObject; MAX_OPEN_FILES],
}

impl FileTable {
    pub const fn new() -> Self {
        Self {
            objects: [const { FileObject::empty() }; MAX_OPEN_FILES],
        }
    }

    /// Açık nesne sayısı.
    pub fn open_count(&self) -> usize {
        self.objects.iter().filter(|object| object.used).count()
    }

    /// Toplam referans sayısı (teşhis).
    pub fn reference_count(&self) -> usize {
        self.objects
            .iter()
            .filter(|object| object.used)
            .map(|object| object.refs as usize)
            .sum()
    }

    /// Yolu normalize eder: mutlak olmalı, `..` ve boş bileşen içermemeli.
    fn normalize_path(path: &str) -> Result<&str, FdError> {
        if path.len() > MAX_FD_PATH || !path.starts_with('/') {
            return Err(FdError::InvalidPath);
        }
        let mut trimmed = path;
        while trimmed.len() > 1 && trimmed.ends_with('/') {
            trimmed = &trimmed[..trimmed.len() - 1];
        }
        if trimmed.is_empty() {
            return Err(FdError::InvalidPath);
        }
        for component in trimmed[1..].split('/') {
            if component == ".." || component.is_empty() {
                return Err(FdError::InvalidPath);
            }
        }
        Ok(trimmed)
    }

    /// Yolu diskten/VFS önbelleğinden/yerleşik tablodan yükler.
    fn load_path(path: &str) -> Result<Vec<u8>, FdError> {
        for &(builtin_path, data) in BUILTIN_FILES {
            if builtin_path == path {
                return Ok(data.to_vec());
            }
        }
        if let Some(data) = crate::fs::diskfs::fat_read_file_path(path) {
            return if data.len() <= MAX_FILE_BYTES {
                Ok(data)
            } else {
                Err(FdError::TooLarge)
            };
        }
        let name = path.rsplit('/').next().unwrap_or(path);
        if let Some(data) = crate::fs::diskfs::load_file_alloc(name) {
            return if data.len() <= MAX_FILE_BYTES {
                Ok(data)
            } else {
                Err(FdError::TooLarge)
            };
        }
        Err(FdError::NotFound)
    }

    /// Yolu açar ve nesne indeksi döndürür. Aynı yol zaten açıksa içerik
    /// yeniden okunmaz, yalnızca referans artırılır.
    pub fn open(&mut self, path: &str, flags: u32) -> Result<usize, FdError> {
        if flags & open_flags::O_ACCMODE != open_flags::O_RDONLY {
            return Err(FdError::Unsupported);
        }
        let path = Self::normalize_path(path)?;

        if let Some(index) = self
            .objects
            .iter()
            .position(|object| object.used && object.path_str().is_some_and(|name| name == path))
        {
            self.objects[index].refs = self.objects[index].refs.saturating_add(1);
            return Ok(index);
        }

        let slot = self
            .objects
            .iter()
            .position(|object| !object.used)
            // Yer yoksa referanssız (VFS önbelleği dahil) girdiyi tahliye et.
            .or_else(|| {
                self.objects
                    .iter()
                    .position(|object| object.used && object.refs == 0)
            })
            .ok_or(FdError::NoFreeObject)?;

        let data = Self::load_path(path)?;
        let object = &mut self.objects[slot];
        object.used = true;
        object.pinned = false;
        object.refs = 1;
        object.path.fill(0);
        let bytes = path.as_bytes();
        object.path_len = bytes.len() as u16;
        object.path[..bytes.len()].copy_from_slice(bytes);
        object.data = data;
        Ok(slot)
    }

    /// VFS dosyalarını syscall katmanına önbellekler. Açılışta bir kez
    /// çağrılır; içerik kopyalanır, böylece VFS'nin sahipliği paylaşılmaz.
    pub fn cache_vfs(&mut self, vfs: &crate::fs::vfs::VirtualFileSystem) -> usize {
        let mut cached = 0usize;
        let mut cached_bytes = 0usize;
        for item in &vfs.items {
            if item.node_type != crate::fs::vfs::NodeType::File || item.data.is_empty() {
                continue;
            }
            if item.path.len() > MAX_FD_PATH || item.data.len() > MAX_FILE_BYTES {
                continue;
            }
            if item.path.contains("..") {
                continue;
            }
            let Some(next_total) = cached_bytes.checked_add(item.data.len()) else {
                break;
            };
            if next_total > MAX_CACHE_BYTES {
                continue;
            }
            if self
                .objects
                .iter()
                .any(|object| object.used && object.path_str() == Some(item.path.as_str()))
            {
                continue;
            }
            let Some(slot) = self.objects.iter().position(|object| !object.used) else {
                break;
            };
            let object = &mut self.objects[slot];
            object.used = true;
            object.pinned = true;
            object.refs = 0;
            object.path.fill(0);
            let bytes = item.path.as_bytes();
            object.path_len = bytes.len() as u16;
            object.path[..bytes.len()].copy_from_slice(bytes);
            object.data = item.data.clone();
            cached += 1;
            cached_bytes = next_total;
        }
        cached
    }

    /// Bir nesne referansını bırakır; son referans ise içeriği serbest bırakır.
    pub fn release(&mut self, index: usize) -> bool {
        let Some(object) = self.objects.get_mut(index) else {
            return false;
        };
        if !object.used {
            return false;
        }
        object.refs = object.refs.saturating_sub(1);
        if object.refs == 0 && !object.pinned {
            *object = FileObject::empty();
        }
        true
    }

    /// `object` içeriğinden `offset` konumundan en fazla `dst.len()` bayt okur.
    pub fn read_at(&self, object: usize, offset: u64, dst: &mut [u8]) -> usize {
        let Some(object) = self.objects.get(object) else {
            return 0;
        };
        if !object.used {
            return 0;
        }
        let start = offset.min(object.data.len() as u64) as usize;
        let available = object.data.len() - start;
        let count = available.min(dst.len());
        dst[..count].copy_from_slice(&object.data[start..start + count]);
        count
    }

    /// `object` içeriğinin boyu.
    pub fn object_size(&self, object: usize) -> Option<u64> {
        self.objects
            .get(object)
            .filter(|object| object.used)
            .map(|object| object.data.len() as u64)
    }
}

impl Default for FileTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Yerleşik, deterministik dosyalar (disk/VFS olmadan syscall testleri için).
const BUILTIN_FILES: &[(&str, &[u8])] = &[
    (
        "/etc/fd-test",
        b"minOs file descriptor lifecycle test\nsecond line\n",
    ),
    ("/dev/null", b""),
];

/// Süreç fd tablosunu 0/1/2 standart tanıtıcılarıyla başlatır.
pub fn init_standard_fds(table: &mut FdTable) {
    for entry in table.iter_mut() {
        *entry = FileDescriptor::empty();
    }
    table[FD_STDIN] = FileDescriptor::standard(FileKind::Stdin);
    table[FD_STDOUT] = FileDescriptor::standard(FileKind::Stdout);
    table[FD_STDERR] = FileDescriptor::standard(FileKind::Stderr);
}

/// Boş bir fd bulur ve nesneyi bağlar.
pub fn alloc_fd(table: &mut FdTable, object: usize, flags: u32) -> Result<usize, FdError> {
    let fd = table
        .iter()
        .enumerate()
        .skip(FIRST_USER_FD)
        .find(|(_, entry)| !entry.used)
        .map(|(index, _)| index)
        .ok_or(FdError::NoFreeFd)?;
    table[fd] = FileDescriptor {
        used: true,
        kind: FileKind::Regular,
        object: object as u16,
        offset: 0,
        flags,
    };
    Ok(fd)
}

/// fd'yi kapatır ve varsa bağlı nesne indeksini döndürür.
pub fn close_fd(table: &mut FdTable, fd: usize) -> Option<usize> {
    let entry = table.get_mut(fd)?;
    if !entry.used || entry.kind != FileKind::Regular {
        return None;
    }
    let object = entry.object as usize;
    *entry = FileDescriptor::empty();
    Some(object)
}

/// Tüm kullanıcı fd'lerini kapatır ve nesne indekslerini döndürür.
pub fn close_all_user_fds(table: &mut FdTable) -> Vec<usize> {
    let mut released = Vec::new();
    for fd in FIRST_USER_FD..MAX_FDS {
        if let Some(object) = close_fd(table, fd) {
            released.push(object);
        }
    }
    released
}

/// Kullanılan fd sayısı (standart tanıtıcılar dahil).
pub fn used_fd_count(table: &FdTable) -> usize {
    table.iter().filter(|entry| entry.used).count()
}
