# Sürüm Geçmişi ve Yol Haritası

minOs sürüm notları ve gelecek planları.

---

## v2.2.0 (Güncel)

### Yenilikler

- **🔐 TLS 1.3 HTTPS El Sıkışması Tamamlandı:** Tarayıcının `https://` bağlantıları artık TLS 1.3 el sıkışmasını uçtan uca tamamlıyor. RFC 8446'ya uygun üç düzeltme: AAD uzunluğu 5 bayt (§5.2), `finished_key` doğrudan `server/client_hs_traffic_secret` üzerinden türetiliyor (§4.4.4), `close_notify` artık fatal hata değil (§6.1).
- **🌐 Tarayıcı Geliştirmeleri (Önbellek ve Görselleştirme):** VFS tabanlı önbellek (`/browser/cache/`); CSS `transform`, `transition`/`animation`, `flexbox`/`grid`, gölgeler, border-radius, opacity desteği; `imgdecode` iyileştirmeleri ve SVG temsili.
- **🌍 Progresif JPEG & @font-face Desteği:** Çok taramalı progresif JPEG (SOF2) çözümlemesi; CSS `@font-face` ve `@keyframes` (`color`/`background-color` interpolasyonu).
- **⏱️ IPv6 Öncelikli HTTP (Dual-Stack):** Önce AAAA çözülür, IPv6 bağlantısı denenir; başarısız olursa IPv4'ye düşülür. 3xx yönlendirme ve `chunked` çözümü IPv6'de uygulanır.
- **🧩 HTTP/2 İstemci Çekirdeği:** RFC 7540 çerçeve katmanı (`h2`), HPACK kodlayıcısı, `Http2Connection`, `http2_get()`. Altyapı hazır; tarayıcı hâlâ HTTP/1.1 kullanır.
- **🐾 Tarayıcı Demo Sayfaları:** `minos://demo`, `minos://demo-img`, `minos://demo-video`.
- **🚫 Tarayıcı Donma Düzeltmesi:** Harici kaynaklar `pending_resources` listesine alınır ve `poll_network()` ile kare başına asenkron getirilir; ana olay döngüsü bloke edilmez.
- **🛠️ Tarayıcı Geliştirici Araçları:** `Ctrl+Shift+D` ile DevTools (Elements, Console, Network, Storage).
- **Genişletilmiş Donanım Sürücüleri (20+):** Ağ (VirtIO-net, Intel I219-V, Atheros, Marvell, Realtek RTL8139, NE2000), Bluetooth HCI, Depolama (AHCI/SATA, NVMe, VirtIO-blk, SD/MMC), USB (Mass Storage, Printer, Audio, Touchscreen), Grafik (VirtIO-gpu, AMD Radeon, NVIDIA GeForce), Sensör & Güç (ACPI AML, pil, uyku, ThermalZone, fan), Veri Yolu & Zaman (PCIe, PCI, PIT/HPET/APIC/TSC).
- **📶 Wi-Fi (802.11) WPA 4-Way El Sıkışması:** `wifi.rs` WPA2-PSK PBKDF2-HMAC-SHA1 PMK/PTK türetimi, WPA 4-way el sıkışması (EAPOL-Key, KCK/KEK/TK, MIC doğrulama) ve PCI/USB arayüz tespiti eklendi.
- **🌀 EC Fan Profilleri:** `fan_temp.rs` için EC (0x66/0x62) fan profilleri (Silent/Normal/Performance/Full/Auto) ve sıcaklığa göre otomatik profil geçişi eklendi.
- **🖼️ Ekran Ayarları Küçük Resimleri:** PNG duvar kağıdı önizlemeleri önyükleme sırasında önceden çözülür; uzun işlemlerde kum saati imleci gösterilir.
- **OHCI & UHCI & EHCI Transfer Katmanı:** Tam transfer katmanı ve port handoff altyapısı.
- **minJs İleri Düzey JavaScript Çalışma Zamanı:** İkili aritmetik, döngüler, değişken mutasyonları, dizi metotları, nesne özellikleri, JSON, matematiksel/dize yardımcıları, global dönüşümler.
- **minJs Closure'lar ve Zamanlayıcılar:** Closure desteği (fonksiyon ifadeleri, ok fonksiyonları, kapsam yakalama, closure başına gizli durum), `setTimeout`/`setInterval` zamanlayıcıları `clearTimeout`/`clearInterval` ile, `Object.keys()`/`Object.values()`/`Object.entries()`, dizi geri çağırmalı metotları (`forEach`, `map`, `filter`, `find`, `findIndex`, `some`, `every`, `reduce`) ve IIFE desteği.
- **IPv6 Protokol & Soket Altyapısı:** `Ipv6Address`, `IpAddress`, RFC 4291, RFC 2464, RFC 8200, ICMPv6 (`ping6`), Neighbor Discovery, `ETHERTYPE_IPV6`; `ipconfig` IPv6 ve `ping`/`ping6` desteği.
- **exFAT ve ext4 Parçalanma & Derin Dizin Bütünlüğü:** Host stres testleriyle doğrulandı.
- **Genişletilmiş Güven Deposu (Root CA):** 51 güvenilen kök CA sertifikası çekirdeğe gömüldü.
- **ISRG Root X1 Kök CA & TLS Zincir Doğrulaması:** ISRG Root X1 gömüldü; zincir RFC 5280 ile doğrulanır.
- **USB Hub Arkası Cihaz Desteği (xHCI):** Çok katmanlı USB hub mimarisi desteklenir.
- **EHCI, UHCI ve OHCI Denetleyici Desteği:** Eski anakartlarda USB portları ve BIOS legacy emülasyonu.
- **Açılış Konsolunda Akıllı Hata ve Uyarı Renklendirmesi:** `[BOOT]`, `[INPUT]`, `[DRIVER]` satırları kırmızı (`#FF6B6B`) ile vurgulanır.
- **Not Defteri – Bul ve Değiştir & Taşma Menüsü:** `Ctrl+F` Bul, `Ctrl+H` Değiştir, `Esc` kapatma.
- **Realtek PCIe Ağ Kartı Desteği (RTL8168/8169/8125/8101):** Yerel sürücü, çoklu PCI bus tarama, 64-bit RX/TX, PHYSTATUS link takibi.
- **Broadcom NetXtreme Ağ Kartı Desteği (BCM57xx, PCI 0x14E4):** Yerel sürücü eklendi.
- **Yüksek Çözünürlüklü Ekranlarda (1080p+) Akıcı Fare & Balistik İvmelenme:** 12/16-bit hareket verilerinin kırpılması engellendi; çözünürlük ölçekli dinamik taban hızı ve 4 kademeli balistik ivmelenme.
- **Yenilenen Orta Boyutlu & Yüksek Kontrastlı Fare İmleci:** 15 piksellik keskin ok, 8 yönlü tam kutu konturu, VRAM yedekleme arabelleği 32x32 piksel.
- **X.509 Sertifika Doğrulama (Zaman & SNI/CN):** `not_before`/`not_after` CMOS RTC ile, CN/SAN DNS kayıtlarıyla RFC 6125 doğrulaması.
- **Sertifika Zinciri Derinlik Sınırı:** `MAX_CHAIN_DEPTH` (6); daha uzun zincirler `ChainTooDeep` ile reddedilir.
- **Sertifika İptal Kontrolü (CRL & OCSP ayrıştırma):** DER CRL/OCSP ayrıştırma, fail-closed politika.
- **VMware VMXNET3 & Genişletilmiş Ağ Desteği:** VMware PCI vendor `0x15AD` (`0x07B0` VMXNET3, `0x0720` VMXNET) algılanır.
- **minPak / minpkg SHA-256 Bütünlük Doğrulaması:** `minPak install <paket> [hash]`, `minPak sha256 <dosya>`, `minPak verify <dosya> <hash>`.
- **minpkg Bağımlılık Çözümleme:** Sürüm kuralları, geçişli sıralama, çakışma ve döngü tespiti.
- **USB Klavye ve Fare İyileştirmeleri (xHCI):** Başlatma hataları giderildi; HID kullanım→PS/2 eşlemesi, rapor tanımlayıcısı, çok arayüzlü aygıt desteği.
- **Açılış Konsolunda USB HID & PS/2 Fallback Teşhisi:** `[INPUT] USB HID bulunamadı, PS/2 fallback kullanılıyor`.
- **Açılışta Otomatik Çözünürlük ve Kalıcılık:** Limine `res=WxH` ve `settings.cfg` tercihleri otomatik uygulanır.
- **Dinamik Çözünürlük ve Masaüstü Uyumlanması:** Çalışma zamanında çözünürlük değişimi anında uyarlanır (`force_render`).
- **QEMU SDL / GTK Desteği & 64 MB VRAM:** `-Display sdl` parametresi, video belleği 64 MB.
- **Statik BSS Bellek Mimarisi (CodeQL Alert #25 Düzeltmesi):** `MemoryManager` statik BSS depolama alanına taşınmıştır.
- **Genişletilmiş Kripto & TLS 1.3:** SHA-384, SHA-512, ECDSA (secp256r1) imza doğrulama/üretme (190 host testi).
- **Erken İstisna Yakalama & Donanımsal Triple Fault Koruması:** BSP GDT+TSS (`IST1`) ve IDT istisna kapıları (`#DE`, `#UD`, `#DF`, `#SS`, `#GP`, `#PF`).
- **Etkileşimli Metin Kurtarma Konsolu (`recovery#`):** Hata ayrıntıları (vektör, RIP, CR2, hata kodu) ve `restart`/`halt`/`help` komutları.
- **Genişletilmiş Yüksek Çözünürlük ve 16:10 Desteği:** Çekirdek arka tamponu `2560x1600`; 1920x1080, 1920x1200, 2560x1440, 2560x1600 desteklenir.
- **Dinamik VRAM MMIO Haritalaması:** 16 MB VRAM aralığı 2MB huge page olarak haritalanır.
- **Intel HDA & Modern Laptop Uyumluluğu:** Zaman aşımı ve PCI multi-function denetimleri; PC Speaker PWM geri dönüşü.
- **Ayarlar Penceresi Tıklama İzolasyonu:** Yazı yakınlaştırma butonları çözünürlük butonlarını tetiklemez.
- **Varsayılan Donanım Sürücü Profili:** Normal Live ve Installed açılışlarında tam sürücü profili; Safe Mode seçeneği korunur.
- **Daha Okunabilir Boot Günlüğü:** Her tamamlanan satır ekrana aktarılır, zaman tabanı hazır olduğunda 45 ms bekleme.
- **Tarayıcı Sık Kullanılanlar (Yer İmleri Çubuğu):** İnteraktif yer imleri çubuğu ve `minos://bookmarks` sayfası.

---

## v2.1.0 (Önceki)

### Yenilikler

- **🛠️ Tarayıcı DevTools Paneli:** `Ctrl+Shift+D` kısayoluyla açılan geliştirici araçları; Elements, Console, Network ve Storage sekmeleri üzerinden DOM öğe sayaçlarını, JavaScript hata günlüğünü, ağ/indirme durumunu, çerezleri ve form otomatik doldurma kayıtlarını gösterir.
- **🔐 TLS 1.3 HTTPS El Sıkışması Tamamlandı:** Tarayıcının `https://` bağlantıları artık TLS 1.3 el sıkışmasını uçtan uca tamamlayıp şifreli HTTP yanıtını işleyebiliyor. RFC 8446'ya uygun üç kritik düzeltme yapıldı:
  - **AAD uzunluğu (§5.2):** Şifreli kayıtların Ek Kimlik Doğrulama Verisi tam olarak 5 bayt olarak hesaplanıyor.
  - **Finished anahtar türetimi (§4.4.4):** `finished_key` doğrudan `server/client_hs_traffic_secret` üzerinden türetiliyor.
  - **`close_notify` zarif kapanış (§6.1):** Sunucunun gönderdiği `close_notify` artık fatal hata sayılmıyor.
- **🌐 Tarayıcı Sayfa İndirme ve HTML Entegrasyonu:** İndirilen sayfalar `/root/Downloads/` altına `.html` uzantısıyla kaydedilir. Dosya Yöneticisi'nde `.html`, `.htm`, `.xhtml` ve `.url` dosyalarına çift tıklandığında tarayıcıda açılır.
- **🌍 Progresif JPEG, `@font-face` & `@keyframes` Renk Animasyonu:** Görsel çözücü çok taramalı **progresif JPEG** (SOF2) dosyalarını katsayı biriktirerek çözer; CSS motoru `@font-face` kurallarını ayrıştırıp `font-family` bilgisini `ElementStyle` üzerinde taşır; `@keyframes` animasyonları `opacity` yanında `color`/`background-color` değerlerini de interpolasyonla canlandırır. Sınır: uzak TTF/WOFF verisi indirilip rasterleştirilmiyor.
- **⏱️ IPv6 Öncelikli HTTP & HTTP/2 İstemci Altyapısı:** `http_get_impl` önce AAAA çözüp IPv6 bağlantısı kurar (HTTPS'te TLS el sıkışması IPv6 üzerinden), başarısız olursa IPv4 yoluna düşer. `http.rs` içine RFC 7540 çerçeve katmanı, HPACK istek kodlayıcısı ve `Http2Connection` (`http2_get`) eklendi. Sınır: HTTPS'te çerçeve şifrelemesi, ALPN `h2` pazarlığı ve HPACK çözümlemesi eksik olduğu için tarayıcı hâlâ HTTP/1.1 kullanır.
- **🐾 Tarayıcı Demo Sayfaları:** `minos://demo`, `minos://demo-img`, `minos://demo-video` yerel demo sayfaları eklendi.
- **Genişletilmiş Donanım Sürücüleri (20+ Yeni Sürücü):** Ağ, Bluetooth HCI, Depolama, USB, Grafik, Sensör & Güç, Veri Yolu & Zaman.
- **Canlı USB Tak/Çıkar:** Çalışma sırasında USB cihazlarının anında algılanıp toast bildirimleri gösteriliyor.
- **OHCI & UHCI & EHCI Transfer Katmanı:** Tam transfer katmanı ve port handoff altyapısı entegre edildi.
- **minJs İleri Düzey JavaScript Çalışma Zamanı:** İkili aritmetik, döngüler, değişken mutasyonları, dizi metotları, nesne özellikleri, JSON, matematiksel ve dize yardımcıları, global dönüşümler.
- **IPv6 Protokol & Soket Altyapısı:** `Ipv6Address`, `IpAddress`, RFC 4291, RFC 2464, RFC 8200, ICMPv6, Neighbor Discovery, `ETHERTYPE_IPV6`.
- **exFAT ve ext4 Parçalanma & Derin Dizin Bütünlüğü:** Host stres testleriyle doğrulandı.
- **Genişletilmiş Güven Deposu (Root CA):** 51 güvenilen kök CA sertifikası çekirdeğe gömüldü.
- **ISRG Root X1 Kök CA & TLS Zincir Doğrulaması:** Let's Encrypt ISRG Root X1 kök sertifikası gömüldü.
- **USB Hub Arkası Cihaz Desteği (xHCI):** Çok katmanlı USB hub mimarisi yerel olarak desteklenir.
- **EHCI, UHCI ve OHCI Denetleyici Desteği:** Eski anakart ve laptop sistemlerinde USB portlarının sağlıklı çalışması.
- **Açılış Konsolunda Akıllı Hata ve Uyarı Renklendirmesi:** `[BOOT]`, `[INPUT]`, `[DRIVER]` satırları kırmızı renkle vurgulanır.
- **Not Defteri – Bul ve Değiştir & Taşma Menüsü:** `Ctrl+F` ile Bul, `Ctrl+H` ile Değiştir çubuğu.
- **Realtek PCIe Ağ Kartı Desteği (RTL8168/8169/8125/8101):** Yerel sürücü desteği entegre edildi.
- **Broadcom NetXtreme Ağ Kartı Desteği (BCM57xx, PCI 0x14E4):** Yerel sürücü eklendi.
- **Yüksek Çözünürlüklü Ekranlarda (1080p+) Akıcı Fare & Balistik İvmelenme:** 12/16-bit hareket verilerinin dar aralığa kırpılması engellendi.
- **Yenilenen Orta Boyutlu & Yüksek Kontrastlı Fare İmleci:** 15 piksellik keskin ok imleci, 8 yönlü tam kutu konturu.
- **X.509 Sertifika Doğrulama (Zaman & SNI/CN):** Geçerlilik tarih aralığı ve hedef ana makine adı doğrulanır.
- **Sertifika Zinciri Derinlik Sınırı:** `MAX_CHAIN_DEPTH` (6) sertifika sınırı.
- **Sertifika İptal Kontrolü (CRL & OCSP ayrıştırma):** Fail-closed politika uygulanır.
- **VMware VMXNET3 & Genişletilmiş Ağ Desteği:** VMware PCI vendor `0x15AD` algılanır.
- **minPak / minpkg SHA-256 Bütünlük Doğrulaması:** Paket yöneticisine SHA-256 özet kontrolü eklendi.
- **minpkg Bağımlılık Çözümleme:** Sürüm kuralları, geçişli bağımlılık sıralaması, çakışma ve döngü tespiti.
- **USB Klavye ve Fare İyileştirmeleri (xHCI):** xHCI başlatmasındaki temel hatalar giderildi.
- **Açılış Konsolunda USB HID & PS/2 Fallback Teşhisi:** Net teşhis günlüğü basılır.
- **Açılışta Otomatik Çözünürlük ve Kalıcılık:** Limine menüsünden seçilen çözünürlük ve `settings.cfg` tercihleri otomatik uygulanır.
- **Dinamik Çözünürlük ve Masaüstü Uyumlanması:** Çalışma zamanında çözünürlük değişimi anında uyarlanır.
- **QEMU SDL / GTK Desteği & 64 MB VRAM:** `-Display sdl` parametresi eklendi, video belleği 64 MB'a yükseltildi.
- **Statik BSS Bellek Mimarisi (CodeQL Alert #25 Düzeltmesi):** `MemoryManager` statik BSS depolama alanına taşınmıştır.
- **Genişletilmiş Kripto & TLS 1.3:** SHA-384, SHA-512 ve ECDSA (secp256r1) imza doğrulama/üretme ilkelleri eklendi.
- **Erken İstisna Yakalama & Donanımsal Triple Fault Koruması:** BSP GDT+TSS (`IST1`) ve IDT istesna kapıları yüklenir.
- **Etkileşimli Metin Kurtarma Konsolu (`recovery#`):** Hata ayrıntıları gösterilir ve interaktif komut istemi sunulur.
- **Genişletilmiş Yüksek Çözünürlük ve 16:10 Desteği:** Çekirdek arka tamponu `2560x1600` boyutuna genişletildi.
- **Dinamik VRAM MMIO Haritalaması:** 16 MB VRAM aralığı 2MB huge page olarak haritalanır.
- **Intel HDA & Modern Laptop Uyumluluğu:** Zaman aşımı ve PCI multi-function denetimleri eklendi.
- **Ayarlar Penceresi Tıklama İzolasyonu:** Yazı yakınlaştırma butonları çözünürlük butonlarını tetiklemez.
- **Varsayılan Donanım Sürücü Profili:** Normal Live ve Installed açılışlarında tam sürücü profili kullanılır.
- **Daha Okunabilir Boot Günlüğü:** Metin konsolu her tamamlanan satırı ekrana aktarır ve 45 ms bekler.
- **Tarayıcı Sık Kullanılanlar (Yer İmleri Çubuğu):** İnteraktif yer imleri çubuğu eklendi.

---

## Yol Haritası (Roadmap)

- **Bluetooth Üst Katman Profilleri:** A2DP ses akışı ve BLE GATT profilleri planlanmaktadır.
- **NVIDIA / AMD 3D Hızlandırma:** Vulkan/Gallium ayrı bir aşamadır.
- **Sensör & Fan Yönetimi:** Donanıma özel EC fan profilleri.
- **Atheros / Marvell Ethernet:** Donanımsal checksum offload ve gelişmiş paket filtreleri planlanmaktadır.
- **USB Audio Kayıt:** Mikrofon üzerinden PCM kayıt desteği TODO.
- **SD/MMC Karta Yazma:** CSD kayıt ayrıştırması ve blok yazma güvenilirliği WIP.
- **CRL/OCSP İptal Kontrolü:** Otomatik HTTP/OCSP indirme isteği planlanmaktadır.
- **IPv6 DNS (AAAA) Çözümlemesi:** AAAA sorgusu gönderilmez ve AAAA yanıtları ayrıştırılmaz.
- **minJs JavaScript Sınırları:** Karmaşık closures/lexical scoping, prototype chain, `class` kalıtımı ve `async`/`await`/Promise henüz tam olarak yer almamaktadır.
