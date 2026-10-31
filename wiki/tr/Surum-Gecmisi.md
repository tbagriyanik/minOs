# Sürüm Geçmişi ve Yol Haritası

minOs sürüm notları ve gelecek planları.

---

## v2.1.0 (Güncel)

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
