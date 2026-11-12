//! Uygulama kayıt defteri: dinamik kayıt ve `/bin` üzerinden hot-load iskeleti.
//!
//! Mevcut yerleşik uygulamalar `gui::setup::register_default_windows` ile
//! başlatılır; bu modül yeni/harici uygulamaların çalışma zamanında
//! keşfedilmesi ve kaydedilmesi için genişletilebilir bir çerçeve sağlar.

extern crate alloc;

use crate::fs::vfs::{NodeType, VirtualFileSystem};
use alloc::string::String;
use alloc::vec::Vec;

use super::Application;

/// Kayıtlı bir uygulama girdisi.
pub struct AppEntry {
    /// Benzersiz uygulama kimliği (yerleşikler için sabit, hariciler için dinamik).
    pub id: u32,
    /// Görünen ad.
    pub name: String,
    /// Çalıştırılabilir yol (`/bin/foo.elf` gibi) veya boş (yerleşik).
    pub path: String,
    /// Yerleşik (çekirdeğe gömülü) mi?
    pub builtin: bool,
}

/// Uygulama kayıt defteri.
pub struct AppRegistry {
    entries: Vec<AppEntry>,
    next_dynamic_id: u32,
}

impl AppRegistry {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_dynamic_id: 1000,
        }
    }

    /// Yerleşik bir uygulamayı kaydet.
    pub fn register_builtin(&mut self, id: u32, name: &str) {
        if self.entries.iter().any(|e| e.id == id) {
            return;
        }
        self.entries.push(AppEntry {
            id,
            name: String::from(name),
            path: String::new(),
            builtin: true,
        });
    }

    /// `/bin` içeriğini tarayıp yeni çalıştırılabilirleri kaydeder (hot-load keşfi).
    /// Dönen değer yeni eklenen giriş sayısıdır.
    pub fn scan_bin(&mut self, vfs: &VirtualFileSystem) -> usize {
        let mut added = 0usize;
        let items = vfs.list_dir("/bin");
        for it in items {
            if it.node_type != NodeType::File {
                continue;
            }
            let name = it.name.as_str();
            let clean = name.strip_suffix(".elf").unwrap_or(name);
            let full_path = if name.starts_with("/bin/") {
                String::from(name)
            } else {
                let mut p = String::from("/bin/");
                p.push_str(name);
                p
            };
            if self
                .entries
                .iter()
                .any(|e| e.path == full_path || (!e.builtin && e.name == clean))
            {
                continue;
            }
            let id = self.next_dynamic_id;
            self.next_dynamic_id = self.next_dynamic_id.saturating_add(1);
            self.entries.push(AppEntry {
                id,
                name: String::from(clean),
                path: full_path,
                builtin: false,
            });
            added += 1;
        }
        added
    }

    /// İsme veya yola göre uygulama bul.
    pub fn find(&self, query: &str) -> Option<&AppEntry> {
        self.entries
            .iter()
            .find(|e| e.name == query || e.path == query)
    }

    /// Kimliğe göre uygulama bul.
    pub fn find_by_id(&self, id: u32) -> Option<&AppEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Harici bir uygulamayı `/bin` yolundan yükleyip çalıştırır.
    ///
    /// Yerleşik girdiler için `None` döner; harici ELF/ikili dosyalar
    /// `mm::loader` + `ring3` üzerinden izole biçimde çalıştırılır.
    pub fn load_and_run(
        &self,
        id: u32,
        vfs: &VirtualFileSystem,
        pmm: &mut crate::mm::pmm::PhysicalMemoryManager,
    ) -> Option<Result<u64, &'static str>> {
        let entry = self.find_by_id(id)?;
        if entry.builtin {
            return None;
        }

        let data = match vfs.read_file(&entry.path) {
            Some(data) => data,
            None => return Some(Err("uygulama dosyasi bulunamadi")),
        };

        let mut address_space = crate::mm::vmm::AddressSpace::current();
        let loaded =
            match crate::mm::loader::AppLoader::load_binary(data, &mut address_space, pmm) {
                Ok(loaded) => loaded,
                Err(err) => return Some(Err(err)),
            };

        let entry_point = loaded.entry_point;
        let ok = crate::arch::x86_64::ring3::execute_loaded_program(&loaded);
        loaded.unmap_and_free(&mut address_space, pmm);

        if ok {
            Some(Ok(entry_point))
        } else {
            Some(Err("uygulama calisirken hata olustu"))
        }
    }

    pub fn entries(&self) -> &[AppEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for AppRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Çalışma zamanı uygulama örneği sarmalayıcısı (trait-object tabanlı genişletme noktası).
pub struct RegisteredApp {
    pub entry: AppEntry,
    pub instance: Option<alloc::boxed::Box<dyn Application>>,
}

impl RegisteredApp {
    pub fn new(entry: AppEntry) -> Self {
        Self {
            entry,
            instance: None,
        }
    }
}
