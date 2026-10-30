//! Web tarayıcı önbellek modülü.
//!
//! CSS, resim ve JS dosyaları VFS üzerinden `/browser/cache/` altında saklanır.
//! Aynı URL'den ikinci kez istek yapıldığında ağa gitmeden önbellekten okunur.

use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::VirtualFileSystem;

/// Önbellek kök dizini (VFS yolu).
const CACHE_DIR: &str = "/browser/cache";

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
pub fn cache_get(vfs: Option<&VirtualFileSystem>, url: &str) -> Option<Vec<u8>> {
    let vfs = vfs?;
    let path = url_to_cache_path(url);
    vfs.read_file(&path).map(|d| d.to_vec())
}

/// Önbelleğe dosya yazar (zaten varsa üzerine yazar).
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
