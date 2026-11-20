# minOs Wiki – Hoş Geldiniz

minOs (Sarman Edition) v2.6.0, Rust ile geliştirilmiş, `no_std` ve `no_main` kullanan bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Limine v8 önyükleyici ile UEFI ve BIOS ortamlarında çalışır.

> 📄 **Proje:** [GitHub Repository](https://github.com/tbagriyanik/minos)  
> 📄 **Dil:** [English](../en/)

---

## Hızlı Erişim

| Sayfa | İçerik |
| :--- | :--- |
| [Hakkında & Mimari](Hakkinda) | Tasarım felsefesi, `no_std` modeli, PMM ve IDT yapısı |
| [Kurulum ve Çalıştırma](Kurulum-ve-Calistirma) | `run.ps1` seçenekleri, Live USB oluşturma, diske kurulum |
| [Desteklenen Donanım Listesi](Desteklenen-Donanimlar) | Sürücü matrisi, ağ, grafik, depolama, giriş aygıtları |
| [Yerleşik 17 Uygulama + Başlat Menüsü](Uygulamalar) | Web Tarayıcısı, Not Defteri, Dosya Yöneticisi, vb. |
| [Ağ ve İnternet](Ag-ve-Internet) | TCP/IP, DNS (A/AAAA), HTTP/1.1, TLS 1.3 (HTTPS), HTTP/2 altyapısı |
| [Dosya Sistemleri](Dosya-Sistemleri) | VFS, FAT32, Ext2, Ext4, exFAT, ISO9660 |
| [Kullanıcı Rehberi](Kullanici-Rehberi) | Masaüstü kısayolları, MinShell komutları |
| [Geliştirici Rehberi](Gelistirici-Rehberi) | Kaynak haritası, birim testleri, derleme |
| [Sürüm Geçmişi](Surum-Gecmisi) | Sürüm notları ve gelecek planları |

---

## Durum Özeti (v2.6.0)

| Ölçüm | Değer |
| :--- | :--- |
| **Rust Derleyici** | 1.102.0-nightly |
| **Toplam Kod Satırı (ham)** | 178.452 satır (Rust: `src/` + `tests/` + `build.rs`); boşlukları hariç `src/` 160.205 satır |
| **P0/P1 Tamamlanma** | 13/23 görev tamamlandı (%57) — P0 bellek-kaldırma grubu bitti |
| **Kritik Düzeltmeler** | Fork/COW delimiter, munmap gerçek sayfa kaldırma + PT/PD/PDPT iadesi, SHARED refcount, ARP table |
| **Güvenilen Kök CA** | 51 sertifika |
| **Yerleşik Uygulama** | 17 kayıtlı uygulama (app ID 0–16) + Başlat Menüsü |
| **Çekirdek / ISO Boyutu** | ~2.0 MB / ~6.0 MB |

---

## Özellik Özetleri

- **Sürücü Görünürlüğü ve Hata Ayıklama:** `drivers::status` her donanım sürücüsünü `Ready` / `Unavailable` (donanım yok veya profilde devre dışı) / `Error` (başlatma hatası) olarak kararlı hata kodlarıyla kaydeder; başlatma süresi ve sonucu seri günlüğe yazılır (`[DRIVER-STATUS]`/`[DRIVER-INIT]`). Kontrol Paneli **Sürücüler** sekmesi 11 sürücünün durumunu, ağ (adaptör/MAC/link/IP), grafik (framebuffer çözünürlük/format), ses (aygıt/örnekleme hızı/DMA) ve depolama (tür/kapasite) ayrıntılarını gösterir ve **Yeniden Tara** içerir. Sürücüsü hatalı uygulamalar açılışta güvenli biçimde engellenir.
- **Bellek Yönetimi:** `munmap` gerçek sayfa kaldırma yapar — gerçekleşmiş çerçeveler kaynak defteri üzerinden PMM'e iade edilir, boşalan PT/PD/PDPT sayfa tablosu çerçeveleri geri kazanılır, tembel sayfalar fiziksel bellek harcamaz ve 16 kayıtlı bölge tablosu doluyken ortadan bölme atomik olarak reddedilir. Süreç çıkışı ve fork hata yolları COW/paylaşımlı çerçeve referanslarını doğru bırakır (bayat kayıt ve erken iade yok).
- **TLS 1.3 HTTPS:** Tarayıcı `https://` bağlantılarını tamamlar; yaprak ve ara sertifika süreleri, CA BasicConstraints/pathLen ve varsa KeyUsage/EKU uzantıları doğrulanır. Host TLS testlerinde doğrulama kestirmesi kaldırılmıştır.
- **Tarayıcı Motoru:** Progresif (SOF2) JPEG çözme, `@font-face` kural ayrıştırma ve `@keyframes` renk/opaklık animasyonu.
- **minJs JavaScript Çalışma Zamanı:** Yerleşik `no_std` JavaScript motoru **closure** (fonksiyon ifadeleri, ok fonksiyonları, kapsam yakalama), **`setTimeout`/`setInterval`** zamanlayıcılarının `clearTimeout`/`clearInterval` ile iptali, **`Object.keys()`/`Object.values()`/`Object.entries()`** ve dizi geri çağırmalı metotları (`forEach`, `map`, `filter`, `find`, `findIndex`, `some`, `every`, `reduce`) ile genişletildi. Fonksiyon değerleri değişkene/nesne özelliğine atanıp `obj.method()` biçiminde çağrılabilir.
- **Dinamik Tema:** 5 tema (Kadife Gece, Gün Işığı, Kutup Mavisi, Yeşil Terminal, Gün Batımı) canlı geçiş.
- **xHCI USB HID:** Yerel USB klavye/fare desteği, PS/2 fallback ile. `run.ps1` varsayılan olarak `qemu-xhci + usb-kbd + usb-mouse` bağlar (QEMU 11'de varsayılan `pc` makinesine PS/2 klavye/fare aygıtı bağlı gelmediği için önceki sürümlerde masaüstü girişı çalışmıyordu; düzeltildi).
- **Ağ:** Intel PRO/1000, RTL8139, AMD PCnet algılama, NE2000, I219-V, Realtek PCIe, Broadcom NetXtreme, VirtIO-net ve VMware VMXNET3.
- **Depolama:** AHCI/SATA, NVMe, VirtIO-blk, USB Mass Storage, SD/MMC.
- **Dosya Sistemleri:** FAT32, Ext2, Ext4, exFAT (okuma/yazma/biçimlendirme).
- **Kripto:** SHA-256/384/512, HMAC, AES-GCM, RSA, ECDSA, X.509, CRL/OCSP yardımcıları.
- **Wi-Fi (802.11):** 802.11 çerçeve yapıları, WPA2-PSK PBKDF2-HMAC-SHA1 PMK/PTK türetimi, WPA 4-way el sıkışması (simüle edilen AP ile), PCI/USB arayüz tespiti; gerçek PHY/RX-TX yok.
- **EC Fan Profilleri:** `fan_temp.rs` için EC (0x66/0x62) fan profilleri (Silent/Normal/Performance/Full/Auto) ve sıcaklığa göre otomatik geçiş.
- **Ekran Ayarları:** PNG duvar kağıdı küçük resimleri önyükleme sırasında önceden çözülür; uzun işlemlerde kum saati imleci gösterilir.
- **Watchdog:** Donmuş uygulama tespiti APIC zamanlayıcı kesmesinde bağımsız çalışır; sonlandırma ana döngüde `drain_frozen()` ile ertelenir ve yalnızca takılan uygulama işaretlenir.
- **FPS 3D Performans Metrikleri:** FPS 3D Oyun tamamlanan karelerden gerçek `fps`/`min_fps`/`frame_ms` ölçer (`[FPS3D-METRICS]`); bütçe aşımında kısmi kare geri yüklenir. `test_qemu.ps1 -FpsStressTest` ile 10/30/60 dk stres doğrulaması.
- **FPS Oyunu Menüsü ve Klavye Erişimi:** FPS Oyunu başlangıç/sonuç menülerini sunar; Hoş Geldin ekranı ve desteklenen form kontrollerinde Tab gezinme, Enter onaylama ve Esc vazgeçme kısayolları kullanılabilir.
- **3D Oyun Nesneleri:** FPS Oyunu düşmanları ve Şehir Sürüş trafik araçları ile toplanabilir ödülleri artık düz billboard yerine ön/üst/yan yüzeyli 3D kutu olarak render eder; z-buffer testiyle duvarların arkasından sızdırılmazlar.
- **QEMU Klavye/Fare:** Varsayılan çalıştırmada girdi çalışmıyordu (QEMU 11 varsayılan `pc` makinesine PS/2 aygıtı bağlı gelmez); `run.ps1` artık `qemu-xhci + usb-kbd + usb-mouse` (USB HID) bağlar.
