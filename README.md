# 🐱 minOs - Sarman Edition

[![Rust](https://img.shields.io/badge/Language-Rust%20Nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Display](https://img.shields.io/badge/Resolution-1280x720%2032--bit%20RGB-purple.svg)]()
[![Min RAM](https://img.shields.io/badge/Min%20RAM-4%20MB-red.svg)]()
[![Min CPU](https://img.shields.io/badge/Min%20CPU-x86__64%20SSE2-red.svg)]()
[![Lines of Code](https://img.shields.io/badge/Kod%20Satırı-41.703%20(Rust)-lightgrey.svg)]()
[![GitHub Release](https://img.shields.io/badge/Release-v1.7.0-brightgreen.svg)](https://github.com/tbagriyanik/minos/releases)
[![Wiki](https://img.shields.io/badge/Docs-GitHub%20Wiki-blueviolet.svg)](https://github.com/tbagriyanik/minOs/wiki)

**minOs (Sarman Edition) v1.7.0**, çekirdek tarafında `no_std` ve `no_main` kullanan, Rust ile geliştirilmiş bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Derleme sırasında `fontdue` kullanılarak TTF yazı tiplerinden bitmap atlasları üretilir; çekirdek bu atlasları grafik arayüzünde kullanır.

> 📚 **Resmi Kılavuz ve Dokümantasyon:** Tüm donanım sürücüleri, uygulama kılavuzları, ağ/TLS yapılandırması ve geliştirici belgeleri için **[minOs GitHub Wiki](https://github.com/tbagriyanik/minOs/wiki)** sayfasını ziyaret edebilirsiniz.

---

## Yenilikler (v1.7.0)

- **Açılışta Otomatik Çözünürlük ve Kalıcılık:** Limine önyükleme menüsünden seçilen ekran çözünürlüğü (`kernel_cmdline: res=WxH`) ve Ayarlar uygulamasından diske kaydedilen tercihler (`settings.cfg`), masaüstü açılmadan önce donanıma (BGA) otomatik olarak uygulanır; sistem doğrudan seçili çözünürlükle açılır.
- **Dinamik Çözünürlük ve Masaüstü Uyumlanması:** Çalışma zamanında çözünürlük değiştirildiğinde masaüstü sınırları, imleç hareket alanı, pencere konumları (`keep_windows_on_screen`) ve duvar kağıdı yeni çözünürlüğe anında uyarlanır; ekran tamamen yeniden çizilir (`force_render`).
- **QEMU SDL / GTK Desteği & 64 MB VRAM:** `run.ps1` betiğine `-Display sdl` parametresi eklendi (GTK pixbuf uyarıları engellenir, daha düşük fare gecikmesi sunar). QEMU video belleği 64 MB'a yükseltildi (`-global VGA.vgamem_mb=64`).
- **Statik BSS Bellek Mimarisi (CodeQL Alert #25 Düzeltmesi):** Çekirdek bellek yöneticisi (`MemoryManager`) statik BSS depolama alanına taşınarak yığın belleğin kaçışı ve raw pointer çözümlenmesi güvenli hale getirildi.
- **Genişletilmiş Kripto & TLS 1.3:** SHA-384, SHA-512 ve ECDSA (secp256r1) imza doğrulama/üretme ilkelleri eklendi (54/54 test geçerli); TLS 1.3 tam el sıkışma döngüsü (RSA & ECDSA CertificateVerify, HKDF anahtar planı ve şifreli kayıt işleme) entegre edildi.
- **Erken İstisna Yakalama & Donanımsal Triple Fault Koruması:** Çekirdeğin açılışının ilk anından itibaren BSP GDT+TSS (`IST1`) ve IDT istisna kapıları (`#DE`, `#UD`, `#DF`, `#SS`, `#GP`, `#PF`) yüklenir. Sürücü başlatma aşamasında oluşabilecek donanım hatalarında anakartın ani yeniden başlaması önlenir.
- **Etkileşimli Metin Kurtarma Konsolu (`recovery#`):** Açılışta veya kritik sürücülerde hata meydana geldiğinde grafik karşılama ekranına geçilmeden önce ekranda hata ayrıntıları (vektör, RIP, CR2, hata kodu) gösterilir ve kullanıcıya interaktif komut istemi sunulur (`restart` / `reboot` / `r`, `halt` / `h`, `help`).
- **Genişletilmiş Yüksek Çözünürlük ve 16:10 Desteği:** Çekirdek arka tamponu (backbuffer) `2560x1600` boyutuna genişletildi. 1920x1080 (FHD), 1920x1200 (16:10 WUXGA), 2560x1440 (2K) ve 2560x1600 ekranlar tam desteklenir; 1920 sınır doğrulama algoritması düzeltildi.
- **Dinamik VRAM MMIO Haritalaması:** Açılışta 16 MB VRAM aralığının tamamı sayfa tablolarına 2MB huge page olarak haritalanır; çözünürlük veya pencere çizimlerinde VRAM sınır aşımından kaynaklı sayfa hataları engellenir.
- **Intel HDA & Modern Laptop Uyumluluğu:** Intel Smart Sound (DSP/cAVS) yonga setleri için zaman aşımı ve PCI multi-function denetimleri eklendi; donanım yanıt vermediğinde sistem çökmeden güvenle PC Speaker PWM moduna geri düşer.
- **Ayarlar Penceresi Tıklama İzolasyonu:** Yazı yakınlaştırma (%100, %125, %150) butonlarına tıklandığında çözünürlük butonlarının yanlışlıkla tetiklenmesi önlendi; gerçek donanımda BGA yazımları kısıtlanarak güvenli ekran modu yönetimi sağlandı.
- **Varsayılan Donanım Sürücü Profili:** Normal Live ve Installed açılışlarında tam sürücü profili kullanılır; sürücüsüz açılış için Limine menüsündeki Safe Mode seçeneği korunur.
- **Daha Okunabilir Boot Günlüğü:** Metin konsolu her tamamlanan satırı ekrana aktarır ve zaman tabanı hazır olduktan sonra sonraki satıra geçmeden önce 45 ms bekler. Böylece açılış tanılama mesajları daha rahat izlenir; zaman tabanı hazır olmadan yazılan erken mesajlar geciktirilmez.
- **Tarayıcı Sık Kullanılanlar (Yer İmleri Çubuğu):** Web tarayıcısına interaktif yer imleri çubuğu eklendi; saf HTTP test sitesi, `minos://cats`, `minos://net`, `minos://history` ve `minos://demo` hızlı erişim butonları ile `minos://bookmarks` sayfası entegre edildi.

### 🛡️ Açılış Kurtarma Konsolu (`recovery#`)

Açılışta veya kritik sürücülerde donanım istisnası (`#PF`, `#GP`, `#DF`) veya arıza tespit edilirse grafik karşılama ekranına geçilmeden önce ekranda hata detayları gösterilir ve etkileşimli komut istemi açılır:

- **`[Enter]`** veya **`restart`** / **`reboot`** / **`r`**: Sistemi hemen güvenli şekilde yeniden başlatır.
- **`halt`** / **`shutdown`** / **`h`**: Sistemi durdurur (`hlt`).
- **`help`**: Kullanılabilir komutları listeler.
- Ayrıntılı kılavuz için: [Açılış Hata ve Kurtarma Konsolu Kılavuzu](docs/usage.md#️-açılış-hata-ve-kurtarma-konsolu-boot-recovery-console)

## 📥 Hazır ISO İndirme (Download)

minOs'u derlemeden doğrudan QEMU, VirtualBox (64bit) veya gerçek donanımda test etmek için hazır ISO imajı:

🔗 **Repo Üzerinden Doğrudan İndir:**
[https://github.com/tbagriyanik/minos/blob/main/minos.iso](https://github.com/tbagriyanik/minos/blob/main/minos.iso)

---

## 🎥 Tanıtım Videosu

[![minOs tanıtım videosunu izleyin](https://img.youtube.com/vi/fB1MQPZTMEg/hqdefault.jpg)](https://www.youtube.com/watch?v=fB1MQPZTMEg)

▶️ [Videoyu YouTube'da izleyin](https://youtu.be/fB1MQPZTMEg)

---

## 📸 Ekran Görüntüleri (Screenshots)

### Welcome Ekranı
![Welcome](docs/screenshots/welcome.png)

### Tema 1
![Tema 1](docs/screenshots/tema1.png)

### Tema 2
![Tema 2](docs/screenshots/tema2.png)

---

## 📚 Dokümantasyon ve Wiki

Ayrıntılı rehberler ve teknik başvuru kaynakları:

- 📖 **[minOs GitHub Wiki (Ana Sayfa)](https://github.com/tbagriyanik/minOs/wiki)**
  - ℹ️ **[Hakkında & Mimari](https://github.com/tbagriyanik/minOs/wiki/Hakkinda)**: Tasarım felsefesi, `no_std` modeli, PMM ve IDT yapısı.
  - 🚀 **[Kurulum ve Çalıştırma](https://github.com/tbagriyanik/minOs/wiki/Kurulum-ve-Calistirma)**: `run.ps1` seçenekleri, Live USB oluşturma ve diske kurulum.
  - 💻 **[Desteklenen Donanım Listesi](https://github.com/tbagriyanik/minOs/wiki/Desteklenen-Donanimlar)**: xHCI USB HID, Touchpad (Tap-to-click), UVC Kamera, e1000 ve Wi-Fi.
  - 📦 **[Yerleşik 14 Uygulama](https://github.com/tbagriyanik/minOs/wiki/Uygulamalar)**: Web Tarayıcısı, Not Defteri, Dosya Yöneticisi, Paket Yöneticisi vb.
  - 🌐 **[Ağ ve İnternet](https://github.com/tbagriyanik/minOs/wiki/Ag-ve-Internet)**: TCP/IP yığını, DNS, HTTP/1.1 ve TLS 1.2 (HTTPS).
  - 💾 **[Dosya Sistemleri](https://github.com/tbagriyanik/minOs/wiki/Dosya-Sistemleri)**: VFS, FAT32 (r/w/format), Ext2, ISO9660 ve kalıcılık (`VFS.DAT`).
  - ⌨️ **[Kullanıcı Rehberi ve Kısayollar](https://github.com/tbagriyanik/minOs/wiki/Kullanici-Rehberi)**: Masaüstü kısayolları ve MinShell komut referansı.
  - 🛠️ **[Geliştirici Rehberi](https://github.com/tbagriyanik/minOs/wiki/Gelistirici-Rehberi)**: Kaynak kod dizin haritası, birim testleri (`test_host.ps1`) ve derleme.
  - 🗺️ **[Sürüm Geçmişi ve Yol Haritası](https://github.com/tbagriyanik/minOs/wiki/Surum-Gecmisi)**: Sürüm notları ve gelecek planları.

Yerel Markdown Dokümanları:
- [Özellikler ve Yol Haritası](docs/features.md)
- [Komutlar, Kısayollar ve Proje Yapısı](docs/usage.md)
- [Donanım, Gereksinimler ve Derleme](docs/building.md)
- [Önceki Sürüm Notları](docs/releases.md)

---

### 🚀 Hızlı Başlangıç

```powershell
git clone https://github.com/tbagriyanik/minOs.git
cd minOs
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

---

## 📄 Lisans

Bu proje açık kaynaklıdır ve eğitim/araştırma amaçlı geliştirilmektedir.
Katkıda bulunmaktan ve geliştirmeler önermekten çekinmeyin! 🐾
