//! minOs Paket Yöneticisi (`minPak` / `pkgmgr`)
//!
//! Bu modül:
//! 1. VFS `/bin` dizinindeki çalıştırılabilir ve kurulu paketleri yönetir.
//! 2. Depo (repo) kataloglarını ve yerel paket listesini sunar.
//! 3. Hem komut satırı (minPak) hem de grafiksel pencere arayüzü (PackageManagerApp) sağlar.
//! 4. Paket arama, kurma (yerleşik ELF veya HTTP GET) ve kaldırma işlevlerini destekler.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::video::Framebuffer;
use crate::fs::vfs::VirtualFileSystem;
use crate::gui::{Color, MinGuiCompositor, TextBuf};

pub const DEFAULT_REPO_URL: &str = "http://repo.minos.org/packages";

/// Paket Kaydı
#[derive(Clone)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub install_path: String,
    pub size_bytes: usize,
}

pub struct MinPkg;

impl MinPkg {
    /// VFS üzerinde kurulu paketleri arar ve listeler.
    pub fn list_installed(vfs: &VirtualFileSystem) -> Vec<PackageMeta> {
        let mut list = Vec::new();
        let bin_items = vfs.list_dir("/bin");
        for it in bin_items {
            if it.node_type == crate::fs::vfs::NodeType::File {
                let name = String::from(it.name.as_str());
                let install_path = format!("/bin/{}", name);
                let size_bytes = it.size;
                list.push(PackageMeta {
                    name,
                    version: String::from("1.0.0"),
                    description: String::from("Sistem uygulamasi / ikili"),
                    install_path,
                    size_bytes,
                });
            }
        }
        list
    }

    /// Verilen bayt dizisinin SHA-256 özetini küçük harfli onaltılık dize olarak döner.
    pub fn compute_sha256(data: &[u8]) -> String {
        let hash = crate::drivers::crypto::sha256::Sha256::hash(data);
        let mut s = String::with_capacity(64);
        for b in hash {
            let hi = b >> 4;
            let lo = b & 0x0F;
            s.push(if hi < 10 { (b'0' + hi) as char } else { (b'a' + (hi - 10)) as char });
            s.push(if lo < 10 { (b'0' + lo) as char } else { (b'a' + (lo - 10)) as char });
        }
        s
    }

    /// Belirtilen paketi uzak repodan veya URL'den HTTP ile indirir, opsiyonel
    /// beklenen SHA-256 checksum'ını kontrol eder ve VFS'e kurar.
    pub fn install_from_url_with_checksum(
        pkg_name_or_url: &str,
        expected_sha256: Option<&str>,
        vfs: &mut VirtualFileSystem,
    ) -> Result<String, &'static str> {
        if !crate::drivers::e1000::is_ready() {
            return Err("Ag bagdastiricisi (e1000) hazir degil");
        }

        let url = if pkg_name_or_url.starts_with("http://") {
            String::from(pkg_name_or_url)
        } else {
            format!("{}/{}.elf", DEFAULT_REPO_URL, pkg_name_or_url)
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

                // Checksum kontrolü
                let expected = expected_sha256.or_else(|| {
                    for pkg in REPO_CATALOG {
                        if pkg_name_or_url == pkg.name || pkg_name_or_url.ends_with(&format!("/{}.elf", pkg.name)) {
                            return pkg.sha256;
                        }
                    }
                    None
                });

                if let Some(exp) = expected {
                    let actual = Self::compute_sha256(body);
                    if !actual.eq_ignore_ascii_case(exp.trim()) {
                        crate::serial::print("[PKG] Checksum uyuşmazlığı! Beklenen: ");
                        crate::serial::print(exp);
                        crate::serial::print(", Hesaplanan: ");
                        crate::serial::print(&actual);
                        crate::serial::print("\n");
                        return Err("Paket dogrulama hatasi (SHA-256 checksum uyusmuyor)");
                    }
                    crate::serial::print("[PKG] SHA-256 checksum dogrulandi.\n");
                }

                let base_name = if let Some(idx) = pkg_name_or_url.rfind('/') {
                    &pkg_name_or_url[idx + 1..]
                } else {
                    pkg_name_or_url
                };

                let target_path = if base_name.ends_with(".elf") {
                    format!("/bin/{}", base_name)
                } else {
                    format!("/bin/{}.elf", base_name)
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

    /// Belirtilen paketi uzak repodan veya URL'den HTTP ile indirip VFS'e kurar.
    pub fn install_from_url(
        pkg_name_or_url: &str,
        vfs: &mut VirtualFileSystem,
    ) -> Result<String, &'static str> {
        Self::install_from_url_with_checksum(pkg_name_or_url, None, vfs)
    }

    /// VFS'teki bir dosyanın SHA-256 özetini doğrular.
    pub fn verify_file_checksum(
        path: &str,
        expected_sha256: &str,
        vfs: &VirtualFileSystem,
    ) -> Result<bool, &'static str> {
        let Some(data) = vfs.read_file(path) else {
            return Err("Dosya bulunamadi");
        };
        let actual = Self::compute_sha256(&data);
        Ok(actual.eq_ignore_ascii_case(expected_sha256.trim()))
    }

    /// Yerleşik paket oluşturucu (Repo kapalıyken veya test için dahili paket kurulumu).
    pub fn install_builtin(name: &str, vfs: &mut VirtualFileSystem) -> Result<String, &'static str> {
        let target_path = if name.ends_with(".elf") {
            format!("/bin/{}", name)
        } else {
            format!("/bin/{}.elf", name)
        };
        let sample_elf = crate::mm::loader::AppLoader::create_sample_elf();
        if vfs.write_file(&target_path, &sample_elf) {
            Ok(target_path)
        } else {
            Err("VFS uzerine yerlesik paket yazilamadi")
        }
    }

    /// Kurulu paketi siler.
    pub fn remove(pkg_name: &str, vfs: &mut VirtualFileSystem) -> bool {
        let path1 = format!("/bin/{}", pkg_name);
        let path2 = format!("/bin/{}.elf", pkg_name);
        vfs.delete_file(&path1) || vfs.delete_file(&path2)
    }
}

/// Depo Katalog Paketi
#[derive(Clone, Copy)]
pub struct RepoPackage {
    pub name: &'static str,
    pub version: &'static str,
    pub description_tr: &'static str,
    pub description_en: &'static str,
    pub category: &'static str,
    pub size_kb: usize,
    pub sha256: Option<&'static str>,
}

pub const REPO_CATALOG: &[RepoPackage] = &[
    RepoPackage {
        name: "sarman-cli",
        version: "1.2.0",
        description_tr: "Gelişmiş kedi temalı kabuk yardımcıları",
        description_en: "Advanced cat themed shell utilities",
        category: "Sistem",
        size_kb: 14,
        sha256: Some("a1b2c3d4e5f60718293a4b5c6d7e8f90123456789abcdef0123456789abcdef0"),
    },
    RepoPackage {
        name: "netcat-lite",
        version: "0.9.1",
        description_tr: "Hafif soket test ve veri aktarım aracı",
        description_en: "Lightweight socket test and transfer tool",
        category: "Ağ",
        size_kb: 18,
        sha256: None,
    },
    RepoPackage {
        name: "hexdump",
        version: "1.0.4",
        description_tr: "İkili dosya onaltılık görüntüleyici",
        description_en: "Binary file hexadecimal inspector",
        category: "Geliştirici",
        size_kb: 12,
        sha256: None,
    },
    RepoPackage {
        name: "tiny-editor",
        version: "2.1.0",
        description_tr: "Hızlı komut satırı metin düzenleyici",
        description_en: "Fast command line text editor",
        category: "Üretkenlik",
        size_kb: 22,
        sha256: None,
    },
    RepoPackage {
        name: "benchmark",
        version: "1.0.0",
        description_tr: "CPU ve PMM bellek hız ölçüm aracı",
        description_en: "CPU and PMM memory benchmark suite",
        category: "Sistem",
        size_kb: 16,
        sha256: None,
    },
    RepoPackage {
        name: "fortune",
        version: "1.1.0",
        description_tr: "Günün minOs bilge kedi vecizesi",
        description_en: "Wise minOs cat quote of the day",
        category: "Eğlence",
        size_kb: 8,
        sha256: None,
    },
];

/// Paket Yöneticisi GUI Penceresi
pub struct PackageManagerApp {
    pub active_tab: usize, // 0: Depo Kataloğu, 1: Kurulu Paketler
    pub selected_index: usize,
    pub search_input: [u8; 32],
    pub search_len: usize,
    pub search_focused: bool,
    pub status_msg: (&'static str, &'static str),
    pub status_is_err: bool,
    pub installed_cache: Vec<PackageMeta>,
}

impl PackageManagerApp {
    pub const fn new() -> Self {
        Self {
            active_tab: 0,
            selected_index: 0,
            search_input: [0; 32],
            search_len: 0,
            search_focused: false,
            status_msg: ("minPak Paket Yöneticisi hazır.", "minPak Package Manager ready."),
            status_is_err: false,
            installed_cache: Vec::new(),
        }
    }

    pub fn refresh_installed(&mut self, vfs: &VirtualFileSystem) {
        self.installed_cache = MinPkg::list_installed(vfs);
        if self.active_tab == 1 && self.selected_index >= self.installed_cache.len() {
            self.selected_index = self.installed_cache.len().saturating_sub(1);
        }
    }

    pub fn search_str(&self) -> &str {
        core::str::from_utf8(&self.search_input[..self.search_len]).unwrap_or("")
    }

    pub fn insert_search_char(&mut self, ch: char) {
        if ch.is_ascii_graphic() || ch == ' ' {
            let mut b = [0u8; 4];
            let s = ch.encode_utf8(&mut b);
            if self.search_len + s.len() <= self.search_input.len() {
                for &byte in s.as_bytes() {
                    self.search_input[self.search_len] = byte;
                    self.search_len += 1;
                }
                self.selected_index = 0;
            }
        }
    }

    pub fn backspace_search(&mut self) {
        if self.search_len > 0 {
            self.search_len -= 1;
            self.search_input[self.search_len] = 0;
            self.selected_index = 0;
        }
    }

    pub fn clear_search(&mut self) {
        self.search_len = 0;
        self.search_input = [0; 32];
        self.selected_index = 0;
    }

    // Geometri ve tıklama alanları
    pub fn tab_rect(win_x: usize, win_y: usize, tab_idx: usize) -> (usize, usize, usize, usize) {
        let x = win_x + 14 + tab_idx * 136;
        let y = win_y + 32;
        (x, y, 128, 26)
    }

    pub fn search_bar_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        let x = win_x + win_w.saturating_sub(230);
        let y = win_y + 32;
        (x, y, 214, 26)
    }

    pub fn list_area_rect(win_x: usize, win_y: usize, win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
        let x = win_x + 14;
        let y = win_y + 68;
        let w = win_w.saturating_sub(28);
        let h = win_h.saturating_sub(136);
        (x, y, w, h)
    }

    pub fn action_button_rect(win_x: usize, win_y: usize, win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
        let w = 120;
        let h = 28;
        let x = win_x + win_w.saturating_sub(w + 14);
        let y = win_y + win_h.saturating_sub(h + 12);
        (x, y, w, h)
    }

    pub fn refresh_button_rect(win_x: usize, win_y: usize, win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
        let w = 110;
        let h = 28;
        let x = win_x + win_w.saturating_sub(w + 142);
        let y = win_y + win_h.saturating_sub(h + 12);
        (x, y, w, h)
    }

    pub fn handle_click(
        &mut self,
        rel_x: usize,
        rel_y: usize,
        win_w: usize,
        win_h: usize,
        vfs: &mut VirtualFileSystem,
    ) {
        let abs_x = rel_x;
        let abs_y = rel_y;

        // Sekme değişimi
        let t0 = Self::tab_rect(0, 0, 0);
        let t1 = Self::tab_rect(0, 0, 1);
        if crate::gui::in_rect(abs_x, abs_y, t0) {
            self.active_tab = 0;
            self.selected_index = 0;
            self.search_focused = false;
            return;
        }
        if crate::gui::in_rect(abs_x, abs_y, t1) {
            self.active_tab = 1;
            self.refresh_installed(vfs);
            self.selected_index = 0;
            self.search_focused = false;
            return;
        }

        // Arama kutusu
        let sb = Self::search_bar_rect(0, 0, win_w);
        if crate::gui::in_rect(abs_x, abs_y, sb) {
            self.search_focused = true;
            return;
        } else {
            self.search_focused = false;
        }

        // Liste satır seçimi
        let la = Self::list_area_rect(0, 0, win_w, win_h);
        if crate::gui::in_rect(abs_x, abs_y, la) {
            let row_h = 28usize;
            let rel_item_y = abs_y.saturating_sub(la.1 + 24);
            let clicked_row = rel_item_y / row_h;
            let total_count = if self.active_tab == 0 {
                self.filtered_repo_count()
            } else {
                self.filtered_installed_count()
            };
            if clicked_row < total_count {
                self.selected_index = clicked_row;
            }
            return;
        }

        // Yenile Butonu
        let rf = Self::refresh_button_rect(0, 0, win_w, win_h);
        if crate::gui::in_rect(abs_x, abs_y, rf) {
            self.refresh_installed(vfs);
            self.status_msg = ("Paket listesi güncellendi.", "Package list refreshed.");
            self.status_is_err = false;
            return;
        }

        // Aksiyon Butonu: Kur (Depo) veya Kaldır (Kurulu)
        let ab = Self::action_button_rect(0, 0, win_w, win_h);
        if crate::gui::in_rect(abs_x, abs_y, ab) {
            if self.active_tab == 0 {
                // Depodan kur
                if let Some(pkg) = self.filtered_repo_item(self.selected_index) {
                    let install_res = if crate::drivers::e1000::is_ready() {
                        MinPkg::install_from_url(pkg.name, vfs)
                    } else {
                        MinPkg::install_builtin(pkg.name, vfs)
                    };
                    match install_res {
                        Ok(_) => {
                            self.status_msg = ("Paket başarıyla /bin altına kuruldu!", "Package installed successfully to /bin!");
                            self.status_is_err = false;
                            self.refresh_installed(vfs);
                        }
                        Err(e) => {
                            let _ = e;
                            self.status_msg = ("Kurulum başarısız oldu!", "Installation failed!");
                            self.status_is_err = true;
                        }
                    }
                }
            } else {
                // Kurulu olanı kaldır
                if let Some(pkg) = self.filtered_installed_item(self.selected_index) {
                    let name = String::from(pkg.name.as_str());
                    if MinPkg::remove(&name, vfs) {
                        self.status_msg = ("Paket VFS'ten kaldırıldı.", "Package removed from VFS.");
                        self.status_is_err = false;
                        self.refresh_installed(vfs);
                    } else {
                        self.status_msg = ("Paket kaldırılamadı!", "Failed to remove package!");
                        self.status_is_err = true;
                    }
                }
            }
        }
    }

    fn filtered_repo_count(&self) -> usize {
        let q = self.search_str().to_lowercase();
        if q.is_empty() {
            REPO_CATALOG.len()
        } else {
            REPO_CATALOG.iter().filter(|p| {
                p.name.to_lowercase().contains(&q)
                    || p.description_tr.to_lowercase().contains(&q)
                    || p.description_en.to_lowercase().contains(&q)
                    || p.category.to_lowercase().contains(&q)
            }).count()
        }
    }

    fn filtered_repo_item(&self, idx: usize) -> Option<RepoPackage> {
        let q = self.search_str().to_lowercase();
        if q.is_empty() {
            REPO_CATALOG.get(idx).copied()
        } else {
            REPO_CATALOG.iter().filter(|p| {
                p.name.to_lowercase().contains(&q)
                    || p.description_tr.to_lowercase().contains(&q)
                    || p.description_en.to_lowercase().contains(&q)
                    || p.category.to_lowercase().contains(&q)
            }).nth(idx).copied()
        }
    }

    fn filtered_installed_count(&self) -> usize {
        let q = self.search_str().to_lowercase();
        if q.is_empty() {
            self.installed_cache.len()
        } else {
            self.installed_cache.iter().filter(|p| {
                p.name.to_lowercase().contains(&q) || p.description.to_lowercase().contains(&q)
            }).count()
        }
    }

    fn filtered_installed_item(&self, idx: usize) -> Option<PackageMeta> {
        let q = self.search_str().to_lowercase();
        if q.is_empty() {
            self.installed_cache.get(idx).cloned()
        } else {
            self.installed_cache.iter().filter(|p| {
                p.name.to_lowercase().contains(&q) || p.description.to_lowercase().contains(&q)
            }).nth(idx).cloned()
        }
    }

    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let english = crate::gui::ui_english();

        // 1. Üst Sekmeler
        let t0 = Self::tab_rect(win_x, win_y, 0);
        let t1 = Self::tab_rect(win_x, win_y, 1);

        let bg_t0 = if self.active_tab == 0 { Color::ACCENT_BLUE } else { Color::TASKBAR_BG };
        let bg_t1 = if self.active_tab == 1 { Color::ACCENT_BLUE } else { Color::TASKBAR_BG };

        fb.draw_rounded_rect(t0.0, t0.1, t0.2, t0.3, 4, bg_t0);
        fb.draw_rounded_rect(t1.0, t1.1, t1.2, t1.3, 4, bg_t1);
        if self.active_tab == 0 {
            fb.draw_rounded_rect_outline(t0.0, t0.1, t0.2, t0.3, 4, Color::WHITE);
        } else {
            fb.draw_rounded_rect_outline(t1.0, t1.1, t1.2, t1.3, 4, Color::WHITE);
        }

        let l0 = if english { "Repo Catalog" } else { "Depo Kataloğu" };
        let l1 = if english { "Installed (VFS)" } else { "Kurulu Paketler" };
        MinGuiCompositor::draw_string(fb, t0.0 + 12, t0.1 + 6, l0, MinGuiCompositor::text_on(bg_t0));
        MinGuiCompositor::draw_string(fb, t1.0 + 12, t1.1 + 6, l1, MinGuiCompositor::text_on(bg_t1));

        // 2. Arama Çubuğu
        let sb = Self::search_bar_rect(win_x, win_y, win_w);
        fb.draw_rounded_rect(sb.0, sb.1, sb.2, sb.3, 4, Color::DARK_BG);
        let sb_border = if self.search_focused { Color::ACCENT_BLUE } else { Color::BORDER };
        fb.draw_rounded_rect_outline(sb.0, sb.1, sb.2, sb.3, 4, sb_border);

        let search_display = if self.search_len > 0 {
            self.search_str()
        } else {
            if english { "Search package..." } else { "Paket ara..." }
        };
        let search_color = if self.search_len > 0 { Color::TEXT_MAIN } else { Color::TEXT_MUTED };
        MinGuiCompositor::draw_string_clipped(fb, sb.0 + 8, sb.1 + 6, search_display, search_color, sb);

        // 3. Liste Alanı
        let la = Self::list_area_rect(win_x, win_y, win_w, win_h);
        fb.draw_rounded_rect(la.0, la.1, la.2, la.3, 4, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(la.0, la.1, la.2, la.3, 4, Color::BORDER);

        // Tablo Başlığı
        let th_bg = Color::TITLE_BAR;
        fb.draw_rounded_rect(la.0 + 1, la.1 + 1, la.2 - 2, 22, 3, th_bg);
        let col1 = if english { "Package Name" } else { "Paket Adı" };
        let col2 = if english { "Version" } else { "Sürüm" };
        let col3 = if english { "Category / Path" } else { "Kategori / Yol" };
        let col4 = if english { "Size" } else { "Boyut" };

        MinGuiCompositor::draw_string(fb, la.0 + 10, la.1 + 4, col1, Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, la.0 + 160, la.1 + 4, col2, Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, la.0 + 240, la.1 + 4, col3, Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, la.0 + la.2.saturating_sub(80), la.1 + 4, col4, Color::TEXT_MAIN);

        // Satırlar
        let row_h = 28usize;
        let visible_rows = (la.3.saturating_sub(26)) / row_h;

        if self.active_tab == 0 {
            let count = self.filtered_repo_count();
            for i in 0..visible_rows.min(count) {
                if let Some(p) = self.filtered_repo_item(i) {
                    let ry = la.1 + 25 + i * row_h;
                    let is_sel = i == self.selected_index;
                    let row_bg = if is_sel { Color::ACCENT_BLUE } else if i % 2 == 1 { Color::TASKBAR_BG } else { Color::WINDOW_BG };
                    fb.draw_rect(la.0 + 2, ry, la.2 - 4, row_h - 1, row_bg);

                    let txt_col = if is_sel { Color::WHITE } else { Color::TEXT_MAIN };
                    let desc = if english { p.description_en } else { p.description_tr };

                    MinGuiCompositor::draw_string(fb, la.0 + 10, ry + 6, p.name, txt_col);
                    MinGuiCompositor::draw_string(fb, la.0 + 160, ry + 6, p.version, Color::ACCENT_PINK);
                    MinGuiCompositor::draw_string_clipped(fb, la.0 + 240, ry + 6, desc, Color::TEXT_MUTED, (la.0 + 240, ry, la.2.saturating_sub(330), row_h));

                    let mut sz_buf = TextBuf::new();
                    sz_buf.push_usize(p.size_kb);
                    sz_buf.push_str(" KB");
                    MinGuiCompositor::draw_string(fb, la.0 + la.2.saturating_sub(80), ry + 6, sz_buf.as_str(), Color::ACCENT_GREEN);
                }
            }
            if count == 0 {
                let empty_msg = if english { "No matching packages found in repository." } else { "Depoda eşleşen paket bulunamadı." };
                MinGuiCompositor::draw_string(fb, la.0 + 16, la.1 + 40, empty_msg, Color::TEXT_MUTED);
            }
        } else {
            let count = self.filtered_installed_count();
            for i in 0..visible_rows.min(count) {
                if let Some(p) = self.filtered_installed_item(i) {
                    let ry = la.1 + 25 + i * row_h;
                    let is_sel = i == self.selected_index;
                    let row_bg = if is_sel { Color::ACCENT_BLUE } else if i % 2 == 1 { Color::TASKBAR_BG } else { Color::WINDOW_BG };
                    fb.draw_rect(la.0 + 2, ry, la.2 - 4, row_h - 1, row_bg);

                    let txt_col = if is_sel { Color::WHITE } else { Color::TEXT_MAIN };
                    MinGuiCompositor::draw_string(fb, la.0 + 10, ry + 6, &p.name, txt_col);
                    MinGuiCompositor::draw_string(fb, la.0 + 160, ry + 6, &p.version, Color::ACCENT_PINK);
                    MinGuiCompositor::draw_string_clipped(fb, la.0 + 240, ry + 6, &p.install_path, Color::ACCENT_YELLOW, (la.0 + 240, ry, la.2.saturating_sub(330), row_h));

                    let mut sz_buf = TextBuf::new();
                    sz_buf.push_usize(p.size_bytes / 1024);
                    sz_buf.push_str(" KB");
                    MinGuiCompositor::draw_string(fb, la.0 + la.2.saturating_sub(80), ry + 6, sz_buf.as_str(), Color::ACCENT_GREEN);
                }
            }
            if count == 0 {
                let empty_msg = if english { "No installed packages found in /bin." } else { "/bin dizininde kurulu paket bulunamadı." };
                MinGuiCompositor::draw_string(fb, la.0 + 16, la.1 + 40, empty_msg, Color::TEXT_MUTED);
            }
        }

        // 4. Durum Çubuğu & Butonlar
        let st_col = if self.status_is_err { Color::ACCENT_RED } else { Color::ACCENT_GREEN };
        let status_text = if english { self.status_msg.1 } else { self.status_msg.0 };
        MinGuiCompositor::draw_string_clipped(
            fb,
            win_x + 16,
            win_y + win_h.saturating_sub(32),
            status_text,
            st_col,
            (win_x + 16, win_y + win_h.saturating_sub(36), win_w.saturating_sub(280), 28),
        );

        // Yenile Butonu
        let rf = Self::refresh_button_rect(win_x, win_y, win_w, win_h);
        fb.draw_rounded_rect(rf.0, rf.1, rf.2, rf.3, 4, Color::TASKBAR_BG);
        fb.draw_rounded_rect_outline(rf.0, rf.1, rf.2, rf.3, 4, Color::BORDER);
        let rf_txt = if english { "Refresh" } else { "Yenile" };
        MinGuiCompositor::draw_string(fb, rf.0 + 24, rf.1 + 6, rf_txt, Color::TEXT_MAIN);

        // Aksiyon Butonu
        let ab = Self::action_button_rect(win_x, win_y, win_w, win_h);
        let (ab_bg, ab_txt) = if self.active_tab == 0 {
            (Color::ACCENT_GREEN, if english { "Install" } else { "Kur" })
        } else {
            (Color::ACCENT_RED, if english { "Remove" } else { "Kaldır" })
        };
        fb.draw_rounded_rect(ab.0, ab.1, ab.2, ab.3, 4, ab_bg);
        fb.draw_rounded_rect_outline(ab.0, ab.1, ab.2, ab.3, 4, Color::WHITE);
        let text_off = (ab.2.saturating_sub(ab_txt.len() * 8)) / 2;
        MinGuiCompositor::draw_string(fb, ab.0 + text_off, ab.1 + 6, ab_txt, MinGuiCompositor::text_on(ab_bg));
    }
}
