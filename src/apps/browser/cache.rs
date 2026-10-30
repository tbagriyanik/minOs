//! Web tarayıcı önbellek modülü.
//!
//! CSS, resim ve JS dosyaları VFS üzerinden `/browser/cache/` altında saklanır.
//! Aynı URL'den ikinci kez istek yapıldığında ağa gitmeden önbellekten okunur.
//!
//! HTTP önbellek kontrolü desteklenir: Cache-Control, Expires, ETag, Last-Modified.
#![allow(static_mut_refs)]

use crate::fs::vfs::VirtualFileSystem;
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// Önbellek meta verileri.
#[derive(Clone)]
pub struct CacheMetadata {
    /// Son kullanma zamanı (epoch seconds, 0 = sonsuz)
    pub expires: u64,
    /// ETag değeri (sunucu tarafı sürüm kontrolü)
    pub etag: Option<String>,
    /// Last-Modified değeri (epoch seconds)
    pub last_modified: Option<u64>,
    /// Cache-Control: max-age (saniye)
    pub max_age: Option<u32>,
    /// Kaynak ne zaman önbelleğe alındı (epoch seconds)
    pub cached_at: u64,
}

impl CacheMetadata {
    pub fn new() -> Self {
        Self {
            expires: 0,
            etag: None,
            last_modified: None,
            max_age: None,
            cached_at: 0,
        }
    }

    /// Kaynak hala geçerli mi?
    pub fn is_fresh(&self, current_time: u64) -> bool {
        if self.expires == 0 {
            // Sonsuz cache
            return true;
        }
        current_time < self.expires
    }

    /// Revalidation gerekiyor mu?
    pub fn needs_revalidation(&self, current_time: u64) -> bool {
        if self.expires == 0 {
            // Sonsuz cache, revalidation gerekmez
            return false;
        }
        current_time >= self.expires
    }
}

/// Önbellek kök dizini (VFS yolu).
const CACHE_DIR: &str = "/browser/cache";

/// Maksimum önbellek boyutu (varsayılan 10 MB).
const MAX_CACHE_SIZE: usize = 10 * 1024 * 1024;

/// Geçerli önbellek boyutu (bayt cinsinden).
static mut CACHE_SIZE: usize = 0;

/// Önbellekteki dosya boyutları (LRU eviction için).
struct CacheEntrySize {
    path: String,
    size: usize,
}
static mut CACHE_ENTRY_SIZES: [Option<CacheEntrySize>; 64] = [const { None }; 64];

/// Önbellek boyutunu günceller ve gerekiyorsa LRU tabanlı eviction yapar.
fn ensure_cache_size(vfs: &mut VirtualFileSystem, needed: usize) {
    unsafe {
        let current = CACHE_SIZE;
        if current + needed <= MAX_CACHE_SIZE {
            CACHE_SIZE = current + needed;
            return;
        }

        // LRU: en eski eklenen girdiyi kaldır (basit FIFO eviction)
        let mut removed = 0usize;
        for entry in CACHE_ENTRY_SIZES.iter_mut() {
            if let Some(ref e) = entry {
                let _ = vfs.delete_file(&e.path);
                let meta_path = format!("{}.meta", e.path);
                let _ = vfs.delete_file(&meta_path);
                removed = e.size;
                *entry = None;
                break;
            }
        }
        CACHE_SIZE = current.saturating_sub(removed) + needed;
    }
}

/// Önbellekteki bir girdinin boyutunu kaydeder.
fn record_cache_entry(path: &str, size: usize) {
    unsafe {
        for entry in CACHE_ENTRY_SIZES.iter_mut() {
            if entry.is_none() {
                *entry = Some(CacheEntrySize {
                    path: path.to_string(),
                    size,
                });
                return;
            }
        }
        // Tümü dolu: ilkini değiştir
        if let Some(first) = CACHE_ENTRY_SIZES.iter_mut().next() {
            *first = Some(CacheEntrySize {
                path: path.to_string(),
                size,
            });
        }
    }
}

/// Önbellekteki bir girdinin boyutunu günceller.
#[allow(dead_code)]
fn update_cache_entry_size(path: &str, _old_size: usize, new_size: usize) {
    unsafe {
        for entry in CACHE_ENTRY_SIZES.iter_mut() {
            if let Some(ref mut e) = entry {
                if e.path == path {
                    e.size = new_size;
                    return;
                }
            }
        }
    }
}

/// Önbelleği temizler (tüm girdileri kaldırır).
#[allow(dead_code)]
pub fn cache_clear(vfs: &mut VirtualFileSystem) {
    unsafe {
        for entry in CACHE_ENTRY_SIZES.iter_mut() {
            if let Some(ref e) = entry.take() {
                let _ = vfs.delete_file(&e.path);
                let meta_path = format!("{}.meta", e.path);
                let _ = vfs.delete_file(&meta_path);
            }
        }
        CACHE_SIZE = 0;
    }
}

/// Belirtilen URL'yi önbellekten kaldırır (invalidation).
#[allow(dead_code)]
pub fn cache_invalidate(vfs: &mut VirtualFileSystem, url: &str) -> bool {
    let path = url_to_cache_path(url);
    unsafe {
        for entry in CACHE_ENTRY_SIZES.iter_mut() {
            if let Some(ref mut e) = entry {
                if e.path == path {
                    let size = e.size;
                    *entry = None;
                    let _ = vfs.delete_file(&path);
                    let meta_path = format!("{}.meta", path);
                    let _ = vfs.delete_file(&meta_path);
                    CACHE_SIZE = CACHE_SIZE.saturating_sub(size);
                    return true;
                }
            }
        }
    }
    // Entry sizes'de yoksa doğrudan dosyayı silmeye çalış
    let _ = vfs.delete_file(&path);
    let meta_path = format!("{}.meta", path);
    let _ = vfs.delete_file(&meta_path);
    true
}

/// URL'yi VFS dosya yolu olarak temizler (özel karakterler `_` ile değiştirilir).
pub fn url_to_cache_path(url: &str) -> String {
    let mut path = String::from(CACHE_DIR);
    path.push('/');

    // Şemayı kaldır (http://, https://)
    let rest = if url.starts_with("http://") {
        &url[7..]
    } else if url.starts_with("https://") {
        &url[8..]
    } else {
        url
    };

    // Tüm karakterleri geçerli dosya adı karakterlerine dönüştür
    for ch in rest.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_' | '~' => {
                path.push(ch);
            }
            '/' | '?' | ':' | '@' | '[' | ']' | '#' => {
                path.push('_');
            }
            _ => {
                path.push('_');
            }
        }
    }

    path
}

/// Önbellekten dosya okur. Bulamazsa `None` döner.
#[allow(dead_code)]
pub fn cache_get(vfs: Option<&VirtualFileSystem>, url: &str) -> Option<Vec<u8>> {
    let vfs = vfs?;
    let path = url_to_cache_path(url);
    vfs.read_file(&path).map(|d| d.to_vec())
}

/// Önbelleğe dosya yazar (zaten varsa üzerine yazar).
#[allow(dead_code)]
pub fn cache_store(vfs: Option<&mut VirtualFileSystem>, url: &str, data: &[u8]) -> bool {
    let Some(vfs) = vfs else {
        return false;
    };
    let path = url_to_cache_path(url);
    // Üst dizinin var olduğundan emin ol
    if !vfs.is_dir(CACHE_DIR) {
        vfs.create_dir(CACHE_DIR);
    }
    vfs.create_file(&path, data)
}

/// Önbellek dizinini başlatır (zaten varsa işlem yapmaz).
#[allow(dead_code)]
pub fn init_cache(vfs: &mut VirtualFileSystem) {
    if !vfs.is_dir(CACHE_DIR) {
        vfs.create_dir(CACHE_DIR);
    }
}

/// HTTP yanıt başlıklarından önbellek meta verilerini çıkarır.
pub fn parse_cache_headers(headers: &str, current_time: u64) -> CacheMetadata {
    let mut meta = CacheMetadata::new();
    meta.cached_at = current_time;

    for line in headers.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();

            match key.as_str() {
                "cache-control" => {
                    // max-age=N, no-cache, no-store, private, public
                    for directive in value.split(',') {
                        let directive = directive.trim();
                        if directive.starts_with("max-age=") {
                            if let Ok(seconds) = directive[8..].parse::<u32>() {
                                meta.max_age = Some(seconds);
                                meta.expires = current_time + seconds as u64;
                            }
                        } else if directive == "no-cache" || directive == "no-store" {
                            // Cache'e alma
                            meta.expires = current_time; // Hemen expire
                        }
                    }
                }
                "expires" => {
                    // RFC 1123 date format: Wed, 21 Oct 2015 07:28:00 GMT
                    if let Some(epoch) = parse_http_date(value) {
                        meta.expires = epoch;
                    }
                }
                "etag" => {
                    meta.etag = Some(value.to_string());
                }
                "last-modified" => {
                    if let Some(epoch) = parse_http_date(value) {
                        meta.last_modified = Some(epoch);
                    }
                }
                _ => {}
            }
        }
    }

    meta
}

/// HTTP date formatını epoch seconds'a çevirir.
/// Format: "Wed, 21 Oct 2015 07:28:00 GMT" veya "Wed, 21 Oct 2015 07:28:00 GMT"
#[allow(dead_code)]
fn parse_http_date(_date_str: &str) -> Option<u64> {
    // Basit implementasyon - yalnızca cache için yeterli
    // Tam RFC 1123/850 parsing karmaşık olabilir
    // Şimdilik placeholder - gerçek implementasyon calendar parsing gerektirir
    // Not: no_std ortamında zaman parsing zordur, basitleştirilmiş yaklaşım
    None
}

/// Cache validation için HTTP başlıkları oluşturur.
pub fn build_validation_headers(meta: &CacheMetadata) -> String {
    let mut headers = String::new();

    if let Some(ref etag) = meta.etag {
        headers.push_str("If-None-Match: ");
        headers.push_str(etag);
        headers.push_str("\r\n");
    }

    if let Some(last_modified) = meta.last_modified {
        headers.push_str("If-Modified-Since: ");
        // Tarih formatlaması gerekir - placeholder
        headers.push_str(&last_modified.to_string());
        headers.push_str("\r\n");
    }

    headers
}

/// Önbellekten dosya ve meta verilerini okur.
pub fn cache_get_with_metadata(
    vfs: Option<&VirtualFileSystem>,
    url: &str,
) -> Option<(Vec<u8>, CacheMetadata)> {
    let vfs = vfs?;
    let path = url_to_cache_path(url);
    let data = vfs.read_file(&path).map(|d| d.to_vec())?;

    // Meta veri dosyasını oku (.meta uzantısı)
    let meta_path = format!("{}.meta", path);
    let meta_data = vfs.read_file(&meta_path)?;
    let meta_str = core::str::from_utf8(&meta_data).ok()?;

    // Meta veriyi parse et
    let mut meta = CacheMetadata::new();
    for line in meta_str.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "expires" => meta.expires = value.parse().unwrap_or(0),
                "etag" => meta.etag = Some(value.to_string()),
                "last_modified" => meta.last_modified = value.parse().ok(),
                "max_age" => meta.max_age = value.parse().ok(),
                "cached_at" => meta.cached_at = value.parse().unwrap_or(0),
                _ => {}
            }
        }
    }

    Some((data, meta))
}

/// Önbelleğe dosya ve meta verilerini yazar.
pub fn cache_store_with_metadata(
    vfs: Option<&mut VirtualFileSystem>,
    url: &str,
    data: &[u8],
    meta: &CacheMetadata,
) -> bool {
    let Some(vfs) = vfs else {
        return false;
    };
    let path = url_to_cache_path(url);

    // Üst dizinin var olduğundan emin ol
    if !vfs.is_dir(CACHE_DIR) {
        vfs.create_dir(CACHE_DIR);
    }

    // Aynı URL zaten önbellekteyse boyutu güncelle
    let existing_size = unsafe {
        let mut size = 0usize;
        for entry in CACHE_ENTRY_SIZES.iter() {
            if let Some(ref e) = entry {
                if e.path == path {
                    size = e.size;
                    break;
                }
            }
        }
        size
    };

    // Önbellek boyutunu kontrol et ve gerekiyorsa eviction yap
    ensure_cache_size(vfs, data.len().saturating_sub(existing_size));

    // Dosyayı kaydet
    if !vfs.create_file(&path, data) {
        return false;
    }

    // Meta veriyi kaydet
    let meta_path = format!("{}.meta", path);
    let mut meta_str = String::new();
    meta_str.push_str(&format!("expires={}\n", meta.expires));
    if let Some(ref etag) = meta.etag {
        meta_str.push_str(&format!("etag={}\n", etag));
    }
    if let Some(last_modified) = meta.last_modified {
        meta_str.push_str(&format!("last_modified={}\n", last_modified));
    }
    if let Some(max_age) = meta.max_age {
        meta_str.push_str(&format!("max_age={}\n", max_age));
    }
    meta_str.push_str(&format!("cached_at={}\n", meta.cached_at));

    if !vfs.create_file(&meta_path, meta_str.as_bytes()) {
        return false;
    }

    // Önbellek girdisi boyutunu kaydet/güncelle
    if existing_size > 0 {
        update_cache_entry_size(&path, existing_size, data.len());
    } else {
        record_cache_entry(&path, data.len());
    }

    true
}
