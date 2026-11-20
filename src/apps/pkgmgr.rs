//! minOs Paket Yöneticisi (`minPak` / `pkgmgr`)
//!
//! Bu modül:
//! 1. VFS `/bin` dizinindeki kurulu paketleri yönetir.
//! 2. Depo (repo) kataloglarını ve yerel paket listesini sunar.
//! 3. Hem komut satırı (minPak) hem de grafiksel pencere arayüzü (PackageManagerApp) sağlar.
//! 4. Paket arama, kurma (yerleşik araç manifesti veya HTTP GET) ve kaldırma işlevlerini destekler.

use crate::apps::pkg_deps::{plan_names, resolve, InstallPlan, PkgEntry, ResolveError};
use crate::drivers::video::Framebuffer;
use crate::fs::vfs::VirtualFileSystem;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

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
    /// VFS `/bin` dizinindeki kurulu araçları ve ikilileri listeler.
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
            s.push(if hi < 10 {
                (b'0' + hi) as char
            } else {
                (b'a' + (hi - 10)) as char
            });
            s.push(if lo < 10 {
                (b'0' + lo) as char
            } else {
                (b'a' + (lo - 10)) as char
            });
        }
        s
    }

    /// İndirilmiş gövdeyi doğrular (opsiyonel SHA-256) ve VFS'e kurar.
    /// Senkron `install_from_url_with_checksum` ve asenkron `PkgDownload`
    /// tamamlanması ortak bu yardımcıyı kullanır.
    fn install_body(
        pkg_name_or_url: &str,
        expected_sha256: Option<&str>,
        body: &[u8],
        vfs: &mut VirtualFileSystem,
    ) -> Result<String, &'static str> {
        if body.is_empty() {
            return Err("Indirilen paket bos");
        }

        // Checksum kontrolü
        let expected = expected_sha256.or_else(|| {
            for pkg in REPO_CATALOG {
                if pkg_name_or_url == pkg.name
                    || pkg_name_or_url.ends_with(&format!("/{}.elf", pkg.name))
                {
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
            crate::serial::print("[PKG] SHA-256 checksum doğrulandı.\n");
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

    /// Belirtilen paketi uzak repodan veya URL'den HTTP ile indirir, opsiyonel
    /// beklenen SHA-256 checksum'ını kontrol eder ve VFS'e kurar.
    /// NOT: Bu yol ana döngüyü bloklar; GUI kullanımı `PkgDownload` (asenkron)
    /// yolunu tercih etmelidir. Bu işlev komut satırı/tek seferlik çağrılar
    /// için korunmuştur.
    pub fn install_from_url_with_checksum(
        pkg_name_or_url: &str,
        expected_sha256: Option<&str>,
        vfs: &mut VirtualFileSystem,
    ) -> Result<String, &'static str> {
        if !crate::drivers::e1000::is_ready() {
            return Err("Ag bagdastiricisi (e1000/NE2000) hazir degil");
        }

        let url = if pkg_name_or_url.starts_with("http://") {
            String::from(pkg_name_or_url)
        } else {
            format!("{}/{}.elf", DEFAULT_REPO_URL, pkg_name_or_url)
        };

        // Paket indirme tamponu (32 KB)
        let mut buf = alloc::vec![0; 32768];

        match crate::drivers::http::http_get(&url, &mut buf) {
            Ok((status, hlen, total)) => {
                if status >= 400 {
                    return Err("Paket sunucuda bulunamadi (HTTP 4xx/5xx)");
                }
                let body = &buf[hlen.min(total)..total];
                Self::install_body(pkg_name_or_url, expected_sha256, body, vfs)
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
        let actual = Self::compute_sha256(data);
        Ok(actual.eq_ignore_ascii_case(expected_sha256.trim()))
    }

    /// Depo kapalıyken kullanılabilen yerleşik araç paketini kurar.
    pub fn install_builtin(
        name: &str,
        vfs: &mut VirtualFileSystem,
    ) -> Result<String, &'static str> {
        let clean = name.strip_suffix(".elf").unwrap_or(name);
        let version = REPO_CATALOG
            .iter()
            .find(|pkg| pkg.name == clean)
            .map(|pkg| pkg.version)
            .ok_or("Yerlesik paket katalogda bulunamadi")?;
        let target_path = format!("/bin/{}.pkg", clean);
        let manifest = format!("MINOS-BUILTIN\n{}\n{}\n", clean, version);
        if vfs.write_file(&target_path, manifest.as_bytes()) {
            Ok(target_path)
        } else {
            Err("VFS uzerine yerlesik paket bilgisi yazilamadi")
        }
    }

    /// Kurulu paketi siler.
    pub fn remove(pkg_name: &str, vfs: &mut VirtualFileSystem) -> bool {
        let path1 = format!("/bin/{}", pkg_name);
        let path2 = format!("/bin/{}.elf", pkg_name);
        let path3 = format!("/bin/{}.pkg", pkg_name);
        vfs.delete_file(&path1) || vfs.delete_file(&path2) || vfs.delete_file(&path3)
    }

    /// Yerleşik depo kataloğunu bağımlılık çözümleyicinin girdi biçimine çevir.
    pub fn catalog_entries() -> Vec<PkgEntry> {
        REPO_CATALOG
            .iter()
            .map(|p| PkgEntry::new(p.name, p.version).with_deps(p.dependencies))
            .collect()
    }

    /// VFS `/bin` içeriğinden kurulu paket listesi (ad, sürüm).
    /// `.pkg` manifestinin 2. satırı paket adı, 3. satırı sürümdür.
    pub fn installed_packages(vfs: &VirtualFileSystem) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for it in vfs.list_dir("/bin") {
            if it.node_type != crate::fs::vfs::NodeType::File {
                continue;
            }
            let name = it.name.as_str();
            let (base, version) = if let Some(n) = name.strip_suffix(".pkg") {
                let path = format!("/bin/{}", name);
                let version = vfs
                    .read_file(&path)
                    .and_then(|data| {
                        let text = core::str::from_utf8(data).ok()?;
                        let mut lines = text.lines();
                        if lines.next()? != "MINOS-BUILTIN" {
                            return None;
                        }
                        let _pkg = lines.next()?;
                        Some(String::from(lines.next()?.trim()))
                    })
                    .unwrap_or_default();
                (n, version)
            } else if let Some(n) = name.strip_suffix(".elf") {
                (n, String::new())
            } else {
                (name, String::new())
            };
            if base.is_empty() || out.iter().any(|(n, _)| n == base) {
                continue;
            }
            out.push((String::from(base), version));
        }
        out
    }

    /// Paketin bağımlılık çözüm planını döner (kurulum yapmaz).
    pub fn resolve_plan(
        pkg_name: &str,
        vfs: &VirtualFileSystem,
    ) -> Result<InstallPlan, ResolveError> {
        let installed = Self::installed_packages(vfs);
        resolve(&Self::catalog_entries(), &installed, pkg_name)
    }

    /// Bağımlılıkları önce olacak şekilde, plana göre paketleri kurar.
    /// Dönüş: kurulan yollar ve zaten kurulu olduğu için atlanan paketler.
    pub fn install_with_dependencies(
        pkg_name: &str,
        vfs: &mut VirtualFileSystem,
    ) -> Result<(Vec<String>, Vec<String>), String> {
        let plan = match Self::resolve_plan(pkg_name, vfs) {
            Ok(p) => p,
            Err(e) => {
                let msg = e.message();
                crate::serial::print("[PKG] Bagimlilik hatasi: ");
                crate::serial::print(&msg);
                crate::serial::print("\n");
                return Err(msg);
            }
        };
        let names = plan_names(&plan);
        let mut installed = Vec::new();
        for name in names {
            match Self::install_builtin(&name, vfs) {
                Ok(path) => installed.push(path),
                Err(_) => return Err(format!("{} paketi kurulamadi", name)),
            }
        }
        Ok((installed, plan.already_installed))
    }
}

/// Uzak paket indirmesinin azami tamponu (senkron yolla aynı sınır).
const PKG_DL_MAX: usize = 32768;
/// İndirme başına toplam bekleme süresi (tarayıcı ile aynı pencere).
const PKG_DL_TIMEOUT_MS: u64 = 15_000;
/// `netstack` tek-bağlantılı TCP yolundaki sahiplik kimliği (tarayıcı=1).
const PKG_NET_OWNER: u8 = 2;

/// `http://` paket indirmesini ana döngüyü bloklamadan yürüten durum makinesi.
/// `poll()` her karede bir kez çağrılır; tamamlandığında gövde doğrulanıp
/// VFS'e kurulur. HTTPS henüz desteklenmez (senkron TLS yolundan farklı
/// olarak eşzamanlı TLS durumu gerekir).
pub struct PkgDownload {
    /// 0 = başlat/sahiplen, 1 = DNS bekle, 2 = TCP bağlan, 3 = istek gönder,
    /// 4 = yanıt al, 5 = yeniden yönlendirme sonrası başlat.
    state: u8,
    url: String,
    host: String,
    path: String,
    port: u16,
    request: Vec<u8>,
    req_sent: usize,
    buf: Vec<u8>,
    status: u16,
    /// Başlık bloğunun bittiği ofset ("\r\n\r\n" sonrası); 0 = henüz yok.
    hdr_end: usize,
    content_length: Option<usize>,
    chunked: bool,
    hops: u8,
    deadline: u64,
    pkg_name: String,
    expected_sha: Option<String>,
    /// İndirme boyunca kum saati imlecini açık tutan kapsam koruyucu.
    _busy: Option<crate::gui::screens::BusyScope>,
    /// Son kullanıcıya gösterilecek aşama mesajı.
    pub status_msg: (&'static str, &'static str),
    /// Görev başarıyla tamamlandıysa `true` (poll_download bunu okur).
    pub succeeded: bool,
}

impl PkgDownload {
    /// `pkg_name_or_url` `http://` ile başlıyorsa indirme görevini kurar.
    /// Diğer adlar (katalog adları) `install_builtin` yoluna aittir.
    pub fn start(
        pkg_name_or_url: &str,
        expected_sha256: Option<&str>,
        pkg_name: &str,
    ) -> Option<Self> {
        let url = if pkg_name_or_url.starts_with("http://") {
            String::from(pkg_name_or_url)
        } else {
            return None;
        };
        Some(Self {
            state: 0,
            url,
            host: String::new(),
            path: String::new(),
            port: 80,
            request: Vec::new(),
            req_sent: 0,
            buf: Vec::new(),
            status: 0,
            hdr_end: 0,
            content_length: None,
            chunked: false,
            hops: 0,
            deadline: 0,
            pkg_name: String::from(pkg_name),
            expected_sha: expected_sha256.map(String::from),
            _busy: Some(crate::gui::screens::BusyScope::new()),
            status_msg: ("Paket indiriliyor...", "Downloading package..."),
            succeeded: false,
        })
    }

    /// Görev ilerlemesini bir kare ilerletir. `true` dönerse hâlâ aktif;
    /// `false` döndüğünde sonuç `self.status_msg` üzerinden bildirilmiştir.
    /// `status_msg`/`status_is_err` uygulama tarafından yansıtılır.
    pub fn poll(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        let now = crate::monotonic_ms();
        if self.deadline == 0 {
            self.deadline = now.wrapping_add(PKG_DL_TIMEOUT_MS);
        }
        if now.wrapping_sub(self.deadline) < u64::MAX / 2 {
            self.fail_now("Ag indirme zaman asimina ugradi.", "Download timed out.");
            return false;
        }
        if self.state == 9 {
            return false;
        }
        match self.state {
            0 => self.step_start(),
            1 => self.step_dns(),
            2 => self.step_connect(),
            3 => self.step_send(),
            _ => self.step_recv(vfs),
        }
    }

    fn set_msg(&mut self, tr: &'static str, en: &'static str) {
        self.status_msg = (tr, en);
    }

    /// Adres çözümleme + TCP yolunu sahiplenme; her iki yola da ayrılır.
    fn step_start(&mut self) -> bool {
        use crate::drivers::netstack as ns;
        // Tarayıcı/paket yöneticisi dışındaki istemciler TCP yolunu tutuyorsa
        // bekle; sahiplik boşalınca (kira ~5 sn) yeniden denenir.
        if !ns::tcp_claim(PKG_NET_OWNER) {
            self.set_msg("Ag mesgul, bekleniyor...", "Network busy, waiting...");
            return true;
        }
        let rest = match self.url.strip_prefix("http://") {
            Some(r) => r,
            None => {
                self.fail_now(
                    "Yalnizca http:// adresleri desteklenir.",
                    "Only http:// URLs are supported.",
                );
                return false;
            }
        };
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        let suffix = &rest[authority_end..];
        let suffix = suffix.split('#').next().unwrap_or("");
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => match p.parse::<u16>() {
                Ok(n) if n > 0 => (h, n),
                _ => ("", 0),
            },
            None => (authority, 80),
        };
        let path_len = if suffix.is_empty() { 1 } else { suffix.len() };
        if host.is_empty() || host.len() > 96 || path_len > 200 || port == 0 {
            self.fail_now("Gecersiz paket adresi.", "Invalid package URL.");
            return false;
        }
        self.host = String::from(host);
        self.port = port;
        self.path = if suffix.is_empty() {
            String::from("/")
        } else if suffix.starts_with('?') {
            let mut p = String::from("/");
            p.push_str(suffix);
            p
        } else {
            String::from(suffix)
        };
        if let Some(ip) = ns::parse_ipv4(&self.host) {
            return self.begin_tcp(ip);
        }
        if ns::dns_query(&self.host) {
            self.state = 1;
            self.deadline = 0;
            self.set_msg("DNS cozumleniyor...", "Resolving DNS...");
            return true;
        }
        self.fail_now("DNS sorgusu gonderilemedi.", "Could not send DNS query.");
        false
    }

    fn step_dns(&mut self) -> bool {
        use crate::drivers::netstack as ns;
        if let Some(ip) = ns::take_dns_result() {
            return self.begin_tcp(ip);
        }
        true
    }

    /// TCP SYN gönderildi; SYN-ACK bekleniyor.
    fn begin_tcp(&mut self, ip: crate::drivers::netstack::Ipv4Address) -> bool {
        use crate::drivers::netstack as ns;
        if ns::tcp_begin_owned(ip, self.port, 49160, PKG_NET_OWNER) {
            self.state = 2;
            self.deadline = 0;
            self.set_msg(
                "TCP baglantisi kuruluyor...",
                "Establishing TCP connection...",
            );
            true
        } else {
            // Sahiplik az önce alınmış olmalı; yine de yabancı sahip varsa bekle.
            self.set_msg("Ag mesgul, bekleniyor...", "Network busy, waiting...");
            true
        }
    }

    fn step_connect(&mut self) -> bool {
        use crate::drivers::netstack as ns;
        if ns::tcp_reset_received() {
            self.fail_now(
                "Sunucu baglantiyi reddetti.",
                "Server rejected the connection.",
            );
            return false;
        }
        if ns::tcp_syn_acked() {
            self.build_request();
            self.state = 3;
            self.deadline = 0;
            self.set_msg("Istek gonderiliyor...", "Sending request...");
        }
        true
    }

    fn build_request(&mut self) {
        let mut req = Vec::with_capacity(512);
        req.extend_from_slice(b"GET ");
        req.extend_from_slice(self.path.as_bytes());
        req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
        req.extend_from_slice(self.host.as_bytes());
        if self.port != 80 {
            req.push(b':');
            let mut n = self.port;
            let mut digits = [0u8; 5];
            let mut idx = digits.len();
            loop {
                idx -= 1;
                digits[idx] = b'0' + (n % 10) as u8;
                n /= 10;
                if n == 0 || idx == 0 {
                    break;
                }
            }
            req.extend_from_slice(&digits[idx..]);
        }
        req.extend_from_slice(b"\r\nUser-Agent: ");
        req.extend_from_slice(crate::user_agent::USER_AGENT.as_bytes());
        req.extend_from_slice(
            b"\r\nAccept: application/octet-stream,*/*\r\nConnection: close\r\n\r\n",
        );
        self.request = req;
        self.req_sent = 0;
    }

    fn step_send(&mut self) -> bool {
        use crate::drivers::netstack as ns;
        while self.req_sent < self.request.len() {
            let end = (self.req_sent + 1300).min(self.request.len());
            if !ns::tcp_send(&self.request[self.req_sent..end]) {
                // Bağlantı henüz tam oturmamış olabilir; sonraki karede dene.
                return true;
            }
            self.req_sent = end;
        }
        self.state = 4;
        self.deadline = 0;
        self.set_msg("Yanit bekleniyor...", "Waiting for response...");
        true
    }

    /// Yanıtı parça parça `buf`'a boşaltır; başlıklar ve gövde tamamlanınca
    /// yönlendirme veya kurulumu yapar.
    fn step_recv(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        use crate::drivers::netstack as ns;
        if self.buf.len() < PKG_DL_MAX {
            let mut tmp = [0u8; 2048];
            let n = ns::take_tcp_data(&mut tmp);
            if n > 0 {
                self.deadline = 0;
                let room = PKG_DL_MAX - self.buf.len();
                let take = n.min(room);
                self.buf.extend_from_slice(&tmp[..take]);
                self.parse_headers();
            }
        }
        if self.hdr_end == 0 {
            // Başlıklar henüz tamamlanmadı; FIN geldiyse eksik yanıt say.
            if ns::tcp_finished() {
                self.fail_now("Eksik HTTP yaniti.", "Incomplete HTTP response.");
                return false;
            }
            return true;
        }
        // Yönlendirme: gövde gerekmez, Location varsa yeni adrese geç.
        if (300..400).contains(&self.status) {
            if self.hops >= 3 {
                self.fail_now("Cok fazla yonlendirme.", "Too many redirects.");
                return false;
            }
            let headers = &self.buf[..self.hdr_end];
            if let Some(loc) = crate::drivers::http::header_value(headers, b"location") {
                if let Ok(loc_str) = core::str::from_utf8(loc) {
                    let next = if loc_str.starts_with("http://") {
                        String::from(loc_str)
                    } else if loc_str.starts_with('/') {
                        let scheme_end = self.url.find("://").map(|i| i + 3).unwrap_or(7);
                        let mut u = String::from(&self.url[..scheme_end]);
                        u.push_str(loc_str);
                        u
                    } else {
                        String::new()
                    };
                    if !next.is_empty() {
                        self.hops += 1;
                        self.url = next;
                        self.buf.clear();
                        self.hdr_end = 0;
                        self.status = 0;
                        self.content_length = None;
                        self.chunked = false;
                        self.state = 0;
                        self.deadline = 0;
                        self.set_msg("Yonlendiriliyor...", "Following redirect...");
                        return true;
                    }
                }
            }
            self.fail_now("Desteklenmeyen yonlendirme.", "Unsupported redirect.");
            return false;
        }
        if self.status >= 400 {
            self.fail_now(
                "Paket sunucuda bulunamadi (HTTP 4xx/5xx).",
                "Package not found on server (HTTP 4xx/5xx).",
            );
            return false;
        }
        // Gövde bütünlüğü: Content-Length biliniyorsa ona göre, chunked/bağlantı
        // kapanışıyla sınırlı yanıtlar FIN'i bekler.
        let body_have = self.buf.len().saturating_sub(self.hdr_end);
        let complete = if self.chunked || self.content_length.is_none() {
            ns::tcp_finished()
        } else {
            body_have >= self.content_length.unwrap_or(0) || ns::tcp_finished()
        };
        if !complete {
            return true;
        }
        self.finish_download(vfs)
    }

    /// Başlık bloğu tamamlandıysa durum satırını ve Content-Length/chunked
    /// bilgisini çözer.
    fn parse_headers(&mut self) {
        if self.hdr_end != 0 {
            return;
        }
        let Some(pos) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") else {
            return;
        };
        self.hdr_end = pos + 4;
        let headers = &self.buf[..self.hdr_end];
        // Durum kodu: "HTTP/1.1 200 OK"
        if let Some(sp) = headers.iter().position(|&b| b == b' ') {
            let mut code: u32 = 0;
            for &b in &headers[sp + 1..self.hdr_end.min(sp + 5)] {
                if b.is_ascii_digit() {
                    code = code * 10 + (b - b'0') as u32;
                } else {
                    break;
                }
            }
            self.status = code as u16;
        }
        if let Some(cl) = crate::drivers::http::header_value(headers, b"content-length") {
            if let Ok(s) = core::str::from_utf8(cl) {
                if let Ok(n) = s.trim().parse::<usize>() {
                    self.content_length = Some(n);
                }
            }
        }
        self.chunked = crate::drivers::http::header_value(headers, b"transfer-encoding")
            .map(|v| {
                core::str::from_utf8(v)
                    .map(|s| s.to_ascii_lowercase().contains("chunked"))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
    }

    /// Yanıt gövdesini çıkarır, chunked kodlamayı çözer ve kurulumu bitirir.
    fn finish_download(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        use crate::drivers::netstack as ns;
        let _ = ns::tcp_close();
        ns::tcp_release(PKG_NET_OWNER);
        let mut body_len = self.buf.len().saturating_sub(self.hdr_end);
        if self.chunked {
            let mut tmp = self.buf[self.hdr_end..].to_vec();
            let chunked_len = tmp.len();
            if let Some(decoded) = crate::drivers::http::decode_chunked(&mut tmp[..], chunked_len) {
                tmp.truncate(decoded);
                self.buf.truncate(self.hdr_end);
                self.buf.extend_from_slice(&tmp);
                body_len = tmp.len();
            } else {
                self.fail_now("Bozuk chunked yanit.", "Malformed chunked response.");
                return false;
            }
        }
        if let Some(cl) = self.content_length {
            body_len = body_len.min(cl);
        }
        let body = &self.buf[self.hdr_end..self.hdr_end + body_len];
        match MinPkg::install_body(&self.pkg_name, self.expected_sha.as_deref(), body, vfs) {
            Ok(path) => {
                self.set_msg(
                    "Paket basariyla kuruldu.",
                    "Package installed successfully.",
                );
                self.succeeded = true;
                crate::serial::print("[PKG] ");
                crate::serial::print(&path);
                crate::serial::print(" kuruldu.\n");
                false
            }
            Err(e) => {
                let tr = match e {
                    "Indirilen paket bos" => "Indirilen paket bos.",
                    "Paket dogrulama hatasi (SHA-256 checksum uyusmuyor)" => {
                        "Paket dogrulama hatasi (SHA-256)."
                    }
                    "VFS uzerine yazilamadi" => "VFS uzerine yazilamadi.",
                    _ => "Kurulum basarisiz.",
                };
                let en = match e {
                    "Indirilen paket bos" => "Downloaded package is empty.",
                    "Paket dogrulama hatasi (SHA-256 checksum uyusmuyor)" => {
                        "Package verification failed (SHA-256)."
                    }
                    "VFS uzerine yazilamadi" => "Could not write to VFS.",
                    _ => "Installation failed.",
                };
                self.set_msg(tr, en);
                false
            }
        }
    }

    /// Başarısızlığı sonlandırır: bağlantıyı ve sahipliği temizler.
    fn fail_now(&mut self, tr: &'static str, en: &'static str) {
        use crate::drivers::netstack as ns;
        let _ = ns::tcp_close();
        ns::tcp_release(PKG_NET_OWNER);
        self.set_msg(tr, en);
        self.state = 9;
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
    /// Bağımlılık kuralları (`"ssl-lite>=1.2"` biçiminde).
    pub dependencies: &'static [&'static str],
}

pub const REPO_CATALOG: &[RepoPackage] = &[
    RepoPackage {
        name: "sarman-cli",
        version: "1.2.0",
        description_tr: "Sistem durumunu raporlar veya vecize gösterir",
        description_en: "Reports system status or shows a quote",
        category: "Sistem",
        size_kb: 0,
        sha256: None,
        dependencies: &[],
    },
    RepoPackage {
        name: "netcat-lite",
        version: "0.9.1",
        description_tr: "TCP bağlantısına metin gönderip yanıt okur",
        description_en: "Sends text to a TCP connection and reads a reply",
        category: "Ağ",
        size_kb: 0,
        sha256: None,
        dependencies: &[],
    },
    RepoPackage {
        name: "hexdump",
        version: "1.0.4",
        description_tr: "İkili dosya onaltılık görüntüleyici",
        description_en: "Binary file hexadecimal inspector",
        category: "Geliştirici",
        size_kb: 0,
        sha256: None,
        dependencies: &[],
    },
    RepoPackage {
        name: "tiny-editor",
        version: "2.1.0",
        description_tr: "Not Defteri uygulamasını açar",
        description_en: "Opens the Notepad application",
        category: "Üretkenlik",
        size_kb: 0,
        sha256: None,
        dependencies: &["hexdump>=1.0"],
    },
    RepoPackage {
        name: "benchmark",
        version: "1.0.0",
        description_tr: "CPU döngüsü ve boş bellek özetini gösterir",
        description_en: "Reports a CPU loop timing and free memory summary",
        category: "Sistem",
        size_kb: 0,
        sha256: None,
        dependencies: &["sarman-cli"],
    },
    RepoPackage {
        name: "fortune",
        version: "1.1.0",
        description_tr: "Günün vecizelerinden birini gösterir",
        description_en: "Shows one of the daily quotes",
        category: "Eğlence",
        size_kb: 0,
        sha256: None,
        dependencies: &[],
    },
];

/// Paket Yöneticisi GUI Penceresi
pub struct PackageManagerApp {
    pub active_tab: usize, // 0: Depo Kataloğu, 1: Kurulu Paketler
    pub selected_index: usize,
    pub search_input: [u8; 32],
    pub search_len: usize,
    pub search_focused: bool,
    /// Tab ile gezinen odaklı denetim: 0 = liste, 1 = arama, 2 = birincil eylem
    /// (Kur / Aç), 3 = ikincil eylem (Yenile / Kaldır), 4 = Yenile (kurulu sekmesi).
    pub focused_control: u8,
    /// Liste dikey kaydırma ofseti (klavye ile seçimi görünür tutar).
    list_scroll: usize,
    pub status_msg: (&'static str, &'static str),
    pub status_is_err: bool,
    pub installed_cache: Vec<PackageMeta>,
    pub pending_launch: Option<String>,
    /// Devam eden uzak paket indirmesi (ana döngüyü bloklamaz).
    pub download: Option<PkgDownload>,
}

impl PackageManagerApp {
    pub const fn new() -> Self {
        Self {
            active_tab: 0,
            selected_index: 0,
            search_input: [0; 32],
            search_len: 0,
            search_focused: false,
            focused_control: 0,
            list_scroll: 0,
            status_msg: (
                "minPak Paket Yöneticisi hazır.",
                "minPak Package Manager ready.",
            ),
            status_is_err: false,
            installed_cache: Vec::new(),
            pending_launch: None,
            download: None,
        }
    }

    /// Oynatılmakta olan paket indirmesi var mı?
    pub fn download_active(&self) -> bool {
        self.download.is_some()
    }

    /// İndirme durum makinesini bir kare ilerletir; tamamlanınca VFS'e kurar
    /// ve durum çubuğunu günceller. Olay döngüsünden her karede çağrılır.
    pub fn poll_download(&mut self, vfs: &mut VirtualFileSystem) {
        let Some(mut dl) = self.download.take() else {
            return;
        };
        let still_active = dl.poll(vfs);
        let msg = dl.status_msg;
        let succeeded = dl.succeeded;
        if still_active {
            self.status_msg = msg;
            self.status_is_err = false;
            self.download = Some(dl);
        } else {
            // poll() false döndüğünde status_msg son sonucu taşır;
            // hata durumları 'status_is_err' ile ayırt edilir.
            self.status_msg = msg;
            self.status_is_err = !succeeded;
            if succeeded {
                self.refresh_installed(vfs);
            }
        }
    }

    pub fn take_pending_launch(&mut self) -> Option<String> {
        self.pending_launch.take()
    }

    pub fn refresh_installed(&mut self, vfs: &VirtualFileSystem) {
        let _busy = crate::gui::screens::BusyScope::new();
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

    /// Klavye (Tab / Shift+Tab) ile odaklı denetime geçer.
    /// Sıra: liste -> arama -> birincil eylem -> ikincil eylem -> yenile (varsa).
    pub fn focus_next_control(&mut self, reverse: bool) {
        // Sekmeye göre gezilebilir denetim sayısı:
        // Depo: liste, arama, Kur, Yenile (4)
        // Kurulu: liste, arama, Aç, Kaldır, Yenile (5)
        let count = if self.active_tab == 0 { 4usize } else { 5usize };
        let cur = (self.focused_control as usize).min(count - 1);
        self.focused_control = if reverse {
            (cur + count - 1) % count
        } else {
            (cur + 1) % count
        } as u8;
        self.search_focused = self.focused_control == 1;
    }

    /// Görünür satır sayısı (pencere yüksekliğinden).
    fn visible_rows(win_h: usize) -> usize {
        let la_h = win_h.saturating_sub(136);
        la_h.saturating_sub(26) / 28
    }

    /// Yukarı/aşağı ok ile liste seçiminde gezinir; seçim her zaman görünür kalır.
    pub fn move_selection(&mut self, delta: isize, win_h: usize) {
        let count = if self.active_tab == 0 {
            self.filtered_repo_count()
        } else {
            self.filtered_installed_count()
        };
        if count == 0 {
            return;
        }
        let cur = self.selected_index.min(count - 1) as isize;
        let next = (cur + delta).clamp(0, count as isize - 1) as usize;
        self.selected_index = next;
        let vis = Self::visible_rows(win_h).max(1);
        if next < self.list_scroll {
            self.list_scroll = next;
        } else if next >= self.list_scroll + vis {
            self.list_scroll = next + 1 - vis;
        }
        let max_scroll = count.saturating_sub(vis);
        if self.list_scroll > max_scroll {
            self.list_scroll = max_scroll;
        }
    }

    /// Enter ile odaklı denetimi etkinleştirir; fare tıklamasıyla aynı sonucu verir.
    pub fn activate_focused(&mut self, win_w: usize, win_h: usize, vfs: &mut VirtualFileSystem) {
        let (cx, cy) = match (self.active_tab, self.focused_control) {
            // Birincil eylem: Depo -> Kur, Kurulu -> Aç
            (0, 0 | 2) => {
                let r = Self::action_button_rect(0, 0, win_w, win_h);
                (r.0 + r.2 / 2, r.1 + r.3 / 2)
            }
            (1, 0 | 2) => {
                let r = Self::open_button_rect(0, 0, win_w, win_h);
                (r.0 + r.2 / 2, r.1 + r.3 / 2)
            }
            // İkincil eylem: Kurulu -> Kaldır
            (1, 3) => {
                let r = Self::remove_button_rect(0, 0, win_w, win_h);
                (r.0 + r.2 / 2, r.1 + r.3 / 2)
            }
            // Yenile: depo sekmesinde özel konum, kurulu sekmesinde ortak dikdörtgen
            (0, 3) => {
                let r = (win_w.saturating_sub(252), win_h.saturating_sub(40), 110, 28);
                (r.0 + r.2 / 2, r.1 + r.3 / 2)
            }
            (1, 4) => {
                let r = Self::refresh_button_rect(0, 0, win_w, win_h);
                (r.0 + r.2 / 2, r.1 + r.3 / 2)
            }
            _ => return,
        };
        self.handle_click(cx, cy, win_w, win_h, vfs);
    }

    // Geometri ve tıklama alanları
    pub fn tab_rect(win_x: usize, win_y: usize, tab_idx: usize) -> (usize, usize, usize, usize) {
        let x = win_x + 14 + tab_idx * 136;
        let y = win_y + 32;
        (x, y, 128, 26)
    }

    pub fn search_bar_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        let x = win_x + win_w.saturating_sub(230);
        let y = win_y + 32;
        (x, y, 214, 26)
    }

    pub fn list_area_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let x = win_x + 14;
        let y = win_y + 68;
        let w = win_w.saturating_sub(28);
        let h = win_h.saturating_sub(136);
        (x, y, w, h)
    }

    pub fn open_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let w = 100;
        let h = 28;
        let x = win_x + win_w.saturating_sub(w + 14);
        let y = win_y + win_h.saturating_sub(h + 12);
        (x, y, w, h)
    }

    pub fn remove_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let w = 100;
        let h = 28;
        let x = win_x + win_w.saturating_sub(w + 122);
        let y = win_y + win_h.saturating_sub(h + 12);
        (x, y, w, h)
    }

    pub fn action_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let w = 120;
        let h = 28;
        let x = win_x + win_w.saturating_sub(w + 14);
        let y = win_y + win_h.saturating_sub(h + 12);
        (x, y, w, h)
    }

    pub fn refresh_button_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let w = 100;
        let h = 28;
        let x = win_x + win_w.saturating_sub(w + 230);
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
            self.focused_control = 0;
            self.list_scroll = 0;
            return;
        }
        if crate::gui::in_rect(abs_x, abs_y, t1) {
            self.active_tab = 1;
            self.refresh_installed(vfs);
            self.selected_index = 0;
            self.search_focused = false;
            self.focused_control = 0;
            self.list_scroll = 0;
            return;
        }

        // Arama kutusu
        let sb = Self::search_bar_rect(0, 0, win_w);
        if crate::gui::in_rect(abs_x, abs_y, sb) {
            self.search_focused = true;
            self.focused_control = 1;
            return;
        } else {
            self.search_focused = false;
        }

        // Liste satır seçimi veya satırdaki [▶ Aç] rozeti tıklaması
        let la = Self::list_area_rect(0, 0, win_w, win_h);
        if crate::gui::in_rect(abs_x, abs_y, la) {
            let row_h = 28usize;
            let rel_item_y = abs_y.saturating_sub(la.1 + 24);
            let clicked_row = rel_item_y / row_h;
            let visible_rows = la.3.saturating_sub(26) / row_h;
            let total_count = if self.active_tab == 0 {
                self.filtered_repo_count()
            } else {
                self.filtered_installed_count()
            };
            if clicked_row < visible_rows && self.list_scroll + clicked_row < total_count {
                let item_idx = self.list_scroll + clicked_row;
                self.selected_index = item_idx.min(total_count - 1);
                self.focused_control = 0;
                // Kurulu sekmesinde sağdaki [▶ Aç] butonuna tıklandıysa doğrudan başlat
                if self.active_tab == 1 && abs_x >= la.0 + la.2.saturating_sub(75) {
                    if let Some(pkg) = self.filtered_installed_item(item_idx) {
                        self.pending_launch = Some(pkg.name.clone());
                        self.status_msg = ("Uygulama başlatılıyor...", "Launching application...");
                        self.status_is_err = false;
                    }
                }
            }
            return;
        }

        // Yenile Butonu
        let rf = if self.active_tab == 0 {
            let rx = win_w.saturating_sub(252);
            let ry = win_h.saturating_sub(40);
            (rx, ry, 110, 28)
        } else {
            Self::refresh_button_rect(0, 0, win_w, win_h)
        };
        if crate::gui::in_rect(abs_x, abs_y, rf) {
            self.refresh_installed(vfs);
            self.status_msg = ("Paket listesi güncellendi.", "Package list refreshed.");
            self.status_is_err = false;
            return;
        }

        if self.active_tab == 0 {
            // Depodan kur butonu
            let ab = Self::action_button_rect(0, 0, win_w, win_h);
            if crate::gui::in_rect(abs_x, abs_y, ab) {
                if let Some(pkg) = self.filtered_repo_item(self.selected_index) {
                    if pkg.name.starts_with("http://") {
                        // Uzak indirme: ana döngüyü bloklamadan kare kare ilerler.
                        if self.download_active() {
                            self.status_msg = (
                                "Zaten bir indirme suruyor.",
                                "A download is already in progress.",
                            );
                            self.status_is_err = false;
                            return;
                        }
                        match PkgDownload::start(pkg.name, pkg.sha256, pkg.name) {
                            Some(dl) => {
                                self.download = Some(dl);
                                self.status_msg =
                                    ("Paket indiriliyor...", "Downloading package...");
                                self.status_is_err = false;
                            }
                            None => {
                                self.status_msg =
                                    ("Gecersiz paket adresi.", "Invalid package URL.");
                                self.status_is_err = true;
                            }
                        }
                        return;
                    }
                    let _busy = crate::gui::screens::BusyScope::new();
                    // Örnek kurulabilir paketler doğrudan dahili kaynaktan kurulur
                    let install_res = MinPkg::install_builtin(pkg.name, vfs);
                    match install_res {
                        Ok(_) => {
                            self.status_msg = (
                                "Paket başarıyla /bin altına kuruldu!",
                                "Package installed successfully to /bin!",
                            );
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
            }
        } else {
            // Kurulu sekmesi: [▶ Aç] ve [Kaldır] butonları
            let ob = Self::open_button_rect(0, 0, win_w, win_h);
            if crate::gui::in_rect(abs_x, abs_y, ob) {
                if let Some(pkg) = self.filtered_installed_item(self.selected_index) {
                    self.pending_launch = Some(pkg.name.clone());
                    self.status_msg = ("Uygulama başlatılıyor...", "Launching application...");
                    self.status_is_err = false;
                    return;
                }
            }

            let rb = Self::remove_button_rect(0, 0, win_w, win_h);
            if crate::gui::in_rect(abs_x, abs_y, rb) {
                if let Some(pkg) = self.filtered_installed_item(self.selected_index) {
                    let name = String::from(pkg.name.as_str());
                    let _busy = crate::gui::screens::BusyScope::new();
                    if MinPkg::remove(&name, vfs) {
                        self.status_msg =
                            ("Paket VFS'ten kaldırıldı.", "Package removed from VFS.");
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
            REPO_CATALOG
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&q)
                        || p.description_tr.to_lowercase().contains(&q)
                        || p.description_en.to_lowercase().contains(&q)
                        || p.category.to_lowercase().contains(&q)
                })
                .count()
        }
    }

    fn filtered_repo_item(&self, idx: usize) -> Option<RepoPackage> {
        let q = self.search_str().to_lowercase();
        if q.is_empty() {
            REPO_CATALOG.get(idx).copied()
        } else {
            REPO_CATALOG
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&q)
                        || p.description_tr.to_lowercase().contains(&q)
                        || p.description_en.to_lowercase().contains(&q)
                        || p.category.to_lowercase().contains(&q)
                })
                .nth(idx)
                .copied()
        }
    }

    fn filtered_installed_count(&self) -> usize {
        let q = self.search_str().to_lowercase();
        if q.is_empty() {
            self.installed_cache.len()
        } else {
            self.installed_cache
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&q) || p.description.to_lowercase().contains(&q)
                })
                .count()
        }
    }

    fn filtered_installed_item(&self, idx: usize) -> Option<PackageMeta> {
        let q = self.search_str().to_lowercase();
        if q.is_empty() {
            self.installed_cache.get(idx).cloned()
        } else {
            self.installed_cache
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&q) || p.description.to_lowercase().contains(&q)
                })
                .nth(idx)
                .cloned()
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

        let bg_t0 = if self.active_tab == 0 {
            Color::ACCENT_BLUE
        } else {
            Color::TASKBAR_BG
        };
        let bg_t1 = if self.active_tab == 1 {
            Color::ACCENT_BLUE
        } else {
            Color::TASKBAR_BG
        };

        fb.draw_rounded_rect(t0.0, t0.1, t0.2, t0.3, 4, bg_t0);
        fb.draw_rounded_rect(t1.0, t1.1, t1.2, t1.3, 4, bg_t1);
        if self.active_tab == 0 {
            fb.draw_rounded_rect_outline(t0.0, t0.1, t0.2, t0.3, 4, Color::WHITE);
        } else {
            fb.draw_rounded_rect_outline(t1.0, t1.1, t1.2, t1.3, 4, Color::WHITE);
        }

        let l0 = if english {
            "Repo Catalog"
        } else {
            "Depo Kataloğu"
        };
        let l1 = if english {
            "Installed (VFS)"
        } else {
            "Kurulu Paketler"
        };
        MinGuiCompositor::draw_string(
            fb,
            t0.0 + 12,
            t0.1 + 6,
            l0,
            MinGuiCompositor::text_on(bg_t0),
        );
        MinGuiCompositor::draw_string(
            fb,
            t1.0 + 12,
            t1.1 + 6,
            l1,
            MinGuiCompositor::text_on(bg_t1),
        );

        // 2. Arama Çubuğu
        let sb = Self::search_bar_rect(win_x, win_y, win_w);
        fb.draw_rounded_rect(sb.0, sb.1, sb.2, sb.3, 4, Color::DARK_BG);
        let sb_border = if self.search_focused || self.focused_control == 1 {
            Color::ACCENT_BLUE
        } else {
            Color::BORDER
        };
        fb.draw_rounded_rect_outline(sb.0, sb.1, sb.2, sb.3, 4, sb_border);

        let search_display = if self.search_len > 0 {
            self.search_str()
        } else {
            if english {
                "Search package..."
            } else {
                "Paket ara..."
            }
        };
        let search_color = if self.search_len > 0 {
            Color::TEXT_MAIN
        } else {
            Color::TEXT_MUTED
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            sb.0 + 8,
            sb.1 + 6,
            search_display,
            search_color,
            sb,
        );

        // 3. Liste Alanı
        let la = Self::list_area_rect(win_x, win_y, win_w, win_h);
        fb.draw_rounded_rect(la.0, la.1, la.2, la.3, 4, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(la.0, la.1, la.2, la.3, 4, Color::BORDER);

        // Tablo Başlığı
        let th_bg = Color::TITLE_BAR;
        fb.draw_rounded_rect(la.0 + 1, la.1 + 1, la.2 - 2, 22, 3, th_bg);
        let col1 = if english {
            "Package Name"
        } else {
            "Paket Adı"
        };
        let col2 = if english { "Version" } else { "Sürüm" };
        let col3 = if english {
            "Category / Path"
        } else {
            "Kategori / Yol"
        };
        let col4 = if english { "Size" } else { "Boyut" };

        MinGuiCompositor::draw_string(fb, la.0 + 10, la.1 + 4, col1, Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, la.0 + 160, la.1 + 4, col2, Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(fb, la.0 + 240, la.1 + 4, col3, Color::TEXT_MAIN);
        MinGuiCompositor::draw_string(
            fb,
            la.0 + la.2.saturating_sub(80),
            la.1 + 4,
            col4,
            Color::TEXT_MAIN,
        );

        // Satırlar
        let row_h = 28usize;
        let visible_rows = (la.3.saturating_sub(26)) / row_h;

        if self.active_tab == 0 {
            let count = self.filtered_repo_count();
            let scroll = self.list_scroll.min(count.saturating_sub(visible_rows));
            for i in 0..visible_rows.min(count.saturating_sub(scroll)) {
                let idx = scroll + i;
                if let Some(p) = self.filtered_repo_item(idx) {
                    let ry = la.1 + 25 + i * row_h;
                    let is_sel = idx == self.selected_index;
                    let row_bg = if is_sel {
                        Color::ACCENT_BLUE
                    } else if i % 2 == 1 {
                        Color::TASKBAR_BG
                    } else {
                        Color::WINDOW_BG
                    };
                    fb.draw_rect(la.0 + 2, ry, la.2 - 4, row_h - 1, row_bg);

                    let txt_col = if is_sel {
                        Color::WHITE
                    } else {
                        Color::TEXT_MAIN
                    };
                    let desc = if english {
                        p.description_en
                    } else {
                        p.description_tr
                    };

                    MinGuiCompositor::draw_string(fb, la.0 + 10, ry + 6, p.name, txt_col);
                    MinGuiCompositor::draw_string(
                        fb,
                        la.0 + 160,
                        ry + 6,
                        p.version,
                        Color::ACCENT_PINK,
                    );
                    MinGuiCompositor::draw_string_clipped(
                        fb,
                        la.0 + 240,
                        ry + 6,
                        desc,
                        Color::TEXT_MUTED,
                        (la.0 + 240, ry, la.2.saturating_sub(330), row_h),
                    );

                    let mut sz_buf = TextBuf::new();
                    if p.size_kb == 0 {
                        sz_buf.push_str(if english { "Built-in" } else { "Yerleşik" });
                    } else {
                        sz_buf.push_usize(p.size_kb);
                        sz_buf.push_str(" KB");
                    }
                    MinGuiCompositor::draw_string(
                        fb,
                        la.0 + la.2.saturating_sub(80),
                        ry + 6,
                        sz_buf.as_str(),
                        Color::ACCENT_GREEN,
                    );
                }
            }
            if count == 0 {
                let empty_msg = if english {
                    "No matching packages found in repository."
                } else {
                    "Depoda eşleşen paket bulunamadı."
                };
                MinGuiCompositor::draw_string(
                    fb,
                    la.0 + 16,
                    la.1 + 40,
                    empty_msg,
                    Color::TEXT_MUTED,
                );
            }
        } else {
            let count = self.filtered_installed_count();
            let scroll = self.list_scroll.min(count.saturating_sub(visible_rows));
            for i in 0..visible_rows.min(count.saturating_sub(scroll)) {
                let idx = scroll + i;
                if let Some(p) = self.filtered_installed_item(idx) {
                    let ry = la.1 + 25 + i * row_h;
                    let is_sel = idx == self.selected_index;
                    let row_bg = if is_sel {
                        Color::ACCENT_BLUE
                    } else if i % 2 == 1 {
                        Color::TASKBAR_BG
                    } else {
                        Color::WINDOW_BG
                    };
                    fb.draw_rect(la.0 + 2, ry, la.2 - 4, row_h - 1, row_bg);

                    let txt_col = if is_sel {
                        Color::WHITE
                    } else {
                        Color::TEXT_MAIN
                    };
                    MinGuiCompositor::draw_string(fb, la.0 + 10, ry + 6, &p.name, txt_col);
                    MinGuiCompositor::draw_string(
                        fb,
                        la.0 + 160,
                        ry + 6,
                        &p.version,
                        Color::ACCENT_PINK,
                    );
                    MinGuiCompositor::draw_string_clipped(
                        fb,
                        la.0 + 240,
                        ry + 6,
                        &p.install_path,
                        Color::ACCENT_YELLOW,
                        (la.0 + 240, ry, la.2.saturating_sub(380), row_h),
                    );

                    let mut sz_buf = TextBuf::new();
                    if p.size_bytes >= 1024 * 1024 {
                        sz_buf.push_usize(p.size_bytes / (1024 * 1024));
                        sz_buf.push_str(" MB");
                    } else if p.size_bytes >= 1024 {
                        sz_buf.push_usize(p.size_bytes / 1024);
                        sz_buf.push_str(" KB");
                    } else {
                        sz_buf.push_usize(p.size_bytes);
                        sz_buf.push_str(" B");
                    }
                    MinGuiCompositor::draw_string(
                        fb,
                        la.0 + la.2.saturating_sub(130),
                        ry + 6,
                        sz_buf.as_str(),
                        Color::ACCENT_GREEN,
                    );

                    // Aç rozeti (satır içi hızlı çalıştırma)
                    let badge_x = la.0 + la.2.saturating_sub(62);
                    let badge_y = ry + 3;
                    fb.draw_rounded_rect(badge_x, badge_y, 54, 20, 3, Color::ACCENT_GREEN);
                    let badge_txt = if english { "▶ Open" } else { "▶ Aç" };
                    MinGuiCompositor::draw_string(
                        fb,
                        badge_x + 5,
                        badge_y + 3,
                        badge_txt,
                        Color::WHITE,
                    );
                }
            }
            if count == 0 {
                let empty_msg = if english {
                    "No installed packages found in /bin."
                } else {
                    "/bin dizininde kurulu paket bulunamadı."
                };
                MinGuiCompositor::draw_string(
                    fb,
                    la.0 + 16,
                    la.1 + 40,
                    empty_msg,
                    Color::TEXT_MUTED,
                );
            }
        }

        // 4. Durum Çubuğu & Butonlar
        let st_col = if self.status_is_err {
            Color::ACCENT_RED
        } else {
            Color::ACCENT_GREEN
        };
        let status_text = if english {
            self.status_msg.1
        } else {
            self.status_msg.0
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            win_x + 16,
            win_y + win_h.saturating_sub(32),
            status_text,
            st_col,
            (
                win_x + 16,
                win_y + win_h.saturating_sub(36),
                win_w.saturating_sub(340),
                28,
            ),
        );

        if self.active_tab == 1 {
            // Aç Butonu
            let ob = Self::open_button_rect(win_x, win_y, win_w, win_h);
            fb.draw_rounded_rect(ob.0, ob.1, ob.2, ob.3, 4, Color::ACCENT_GREEN);
            fb.draw_rounded_rect_outline(ob.0, ob.1, ob.2, ob.3, 4, Color::WHITE);
            let ob_txt = if english { "▶ Open" } else { "▶ Aç" };
            let off = (ob.2.saturating_sub(ob_txt.len() * 8)) / 2;
            MinGuiCompositor::draw_string(fb, ob.0 + off, ob.1 + 6, ob_txt, Color::WHITE);

            // Kaldır Butonu
            let rb = Self::remove_button_rect(win_x, win_y, win_w, win_h);
            fb.draw_rounded_rect(rb.0, rb.1, rb.2, rb.3, 4, Color::ACCENT_RED);
            fb.draw_rounded_rect_outline(rb.0, rb.1, rb.2, rb.3, 4, Color::WHITE);
            let rb_txt = if english { "Remove" } else { "Kaldır" };
            let off_r = (rb.2.saturating_sub(rb_txt.len() * 8)) / 2;
            MinGuiCompositor::draw_string(fb, rb.0 + off_r, rb.1 + 6, rb_txt, Color::WHITE);

            // Yenile Butonu
            let rf = Self::refresh_button_rect(win_x, win_y, win_w, win_h);
            fb.draw_rounded_rect(rf.0, rf.1, rf.2, rf.3, 4, Color::TASKBAR_BG);
            fb.draw_rounded_rect_outline(rf.0, rf.1, rf.2, rf.3, 4, Color::BORDER);
            let rf_txt = if english { "Refresh" } else { "Yenile" };
            let off_rf = (rf.2.saturating_sub(rf_txt.len() * 8)) / 2;
            MinGuiCompositor::draw_string(fb, rf.0 + off_rf, rf.1 + 6, rf_txt, Color::TEXT_MAIN);
        } else {
            // Depo sekmesi: Kur ve Yenile Butonları
            let rf_x = win_x + win_w.saturating_sub(252);
            let rf_y = win_y + win_h.saturating_sub(40);
            fb.draw_rounded_rect(rf_x, rf_y, 110, 28, 4, Color::TASKBAR_BG);
            fb.draw_rounded_rect_outline(rf_x, rf_y, 110, 28, 4, Color::BORDER);
            let rf_txt = if english { "Refresh" } else { "Yenile" };
            MinGuiCompositor::draw_string(fb, rf_x + 24, rf_y + 6, rf_txt, Color::TEXT_MAIN);

            let ab = Self::action_button_rect(win_x, win_y, win_w, win_h);
            fb.draw_rounded_rect(ab.0, ab.1, ab.2, ab.3, 4, Color::ACCENT_GREEN);
            fb.draw_rounded_rect_outline(ab.0, ab.1, ab.2, ab.3, 4, Color::WHITE);
            let ab_txt = if english { "Install" } else { "Kur" };
            let text_off = (ab.2.saturating_sub(ab_txt.len() * 8)) / 2;
            MinGuiCompositor::draw_string(fb, ab.0 + text_off, ab.1 + 6, ab_txt, Color::WHITE);
        }

        // Klavye odağı (Tab) göstergesi: odaklı eylem düğmesinin çevresine halka çiz.
        let focus_rect = match (self.active_tab, self.focused_control) {
            (0, 2) => Some(Self::action_button_rect(win_x, win_y, win_w, win_h)),
            (0, 3) => Some((
                win_x + win_w.saturating_sub(252),
                win_y + win_h.saturating_sub(40),
                110,
                28,
            )),
            (1, 2) => Some(Self::open_button_rect(win_x, win_y, win_w, win_h)),
            (1, 3) => Some(Self::remove_button_rect(win_x, win_y, win_w, win_h)),
            (1, 4) => Some(Self::refresh_button_rect(win_x, win_y, win_w, win_h)),
            _ => None,
        };
        if let Some(r) = focus_rect {
            MinGuiCompositor::draw_focus_ring(fb, r.0, r.1, r.2, r.3);
        }
    }
}

impl Default for PackageManagerApp {
    fn default() -> Self {
        Self::new()
    }
}
