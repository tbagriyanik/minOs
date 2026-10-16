//! minOs Paket Yöneticisi (`minpkg`)
//!
//! Bu modül:
//! 1. minOs paket (.mpk) formatını ayrıştırır veya düz ikili paketleri işler.
//! 2. HTTP üzerinden uzak depodan (repository) paket indirir (`install <pkg>`).
//! 3. İndirilen paketi VFS üzerinde `/bin/<pkg>` veya `/share/<pkg>` hedeflerine kurar.
//! 4. Kurulu paketleri listeler (`list`) ve kaldırır (`remove <pkg>`).

use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::VirtualFileSystem;

pub const DEFAULT_REPO_URL: &str = "http://repo.minos.org/packages";

/// Paket Kaydı
#[derive(Clone)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub install_path: String,
}

pub struct MinPkg;

impl MinPkg {
    /// VFS üzerinde kurulu paketleri arar ve listeler.
    pub fn list_installed(vfs: &VirtualFileSystem) -> Vec<PackageMeta> {
        let mut list = Vec::new();
        // /bin dizinindeki çalıştırılabilirleri listele
        let bin_items = vfs.list_dir("/bin");
        for it in bin_items {
            if it.node_type == crate::fs::vfs::NodeType::File {
                let name = String::from(it.name.as_str());
                let install_path = alloc::format!("/bin/{}", name);
                list.push(PackageMeta {
                    name,
                    version: String::from("1.0.0"),
                    description: String::from("Sistem uygulamasi / ikili"),
                    install_path,
                });
            }
        }
        list
    }

    /// Belirtilen paketi uzak repodan veya URL'den HTTP ile indirip VFS'e kurar.
    pub fn install_from_url(
        pkg_name_or_url: &str,
        vfs: &mut VirtualFileSystem,
    ) -> Result<String, &'static str> {
        if !crate::drivers::e1000::is_ready() {
            return Err("Ag bagdastiricisi (e1000) hazir degil");
        }

        let url = if pkg_name_or_url.starts_with("http://") {
            String::from(pkg_name_or_url)
        } else {
            alloc::format!("{}/{}.elf", DEFAULT_REPO_URL, pkg_name_or_url)
        };

        // Paket indirme tamponu (32 KB)
        let mut buf = Vec::new();
        buf.resize(32768, 0);

        match crate::drivers::http::http_get(&url, &mut buf) {
            Ok((status, hlen, total)) => {
                if status >= 400 {
                    return Err("Paket sunucuda bulunamadi (HTTP 4xx/5xx)");
                }
                let body = &buf[hlen.min(total)..total];
                if body.is_empty() {
                    return Err("Indirilen paket bos");
                }

                // Paketi /bin/<ad> konumuna kaydet
                let base_name = if let Some(idx) = pkg_name_or_url.rfind('/') {
                    &pkg_name_or_url[idx + 1..]
                } else {
                    pkg_name_or_url
                };

                let target_path = if base_name.ends_with(".elf") {
                    alloc::format!("/bin/{}", base_name)
                } else {
                    alloc::format!("/bin/{}.elf", base_name)
                };

                if vfs.write_file(&target_path, body) {
                    Ok(target_path)
                } else {
                    Err("VFS uzerine yazilamadi")
                }
            }
            Err(_) => Err("Ag indirme veya baglanti hatasi"),
        }
    }

    /// Yerleşik paket oluşturucu (Repo kapalıyken veya test için dahili paket kurulumu).
    pub fn install_builtin(name: &str, vfs: &mut VirtualFileSystem) -> Result<String, &'static str> {
        let target_path = alloc::format!("/bin/{}.elf", name);
        let sample_elf = crate::mm::loader::AppLoader::create_sample_elf();
        if vfs.write_file(&target_path, &sample_elf) {
            Ok(target_path)
        } else {
            Err("VFS uzerine yerlesik paket yazilamadi")
        }
    }

    /// Kurulu paketi siler.
    pub fn remove(pkg_name: &str, vfs: &mut VirtualFileSystem) -> bool {
        let path1 = alloc::format!("/bin/{}", pkg_name);
        let path2 = alloc::format!("/bin/{}.elf", pkg_name);
        vfs.delete_file(&path1) || vfs.delete_file(&path2)
    }
}
