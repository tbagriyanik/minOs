# 🐱 minOs - Sarman Edition

[![Rust](https://img.shields.io/badge/Language-Rust%20Nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Display](https://img.shields.io/badge/Resolution-1280x720%2032--bit%20RGB-purple.svg)]()
[![Min RAM](https://img.shields.io/badge/Min%20RAM-4%20MB-red.svg)]()
[![Min CPU](https://img.shields.io/badge/Min%20CPU-x86__64%20SSE2-red.svg)]()
[![Lines of Code](https://img.shields.io/badge/Kod%20Satırı-48.500%20(Rust)-lightgrey.svg)]()
[![GitHub Release](https://img.shields.io/badge/Release-v1.8.0-brightgreen.svg)](https://github.com/tbagriyanik/minos/releases)
[![Wiki](https://img.shields.io/badge/Docs-GitHub%20Wiki-blueviolet.svg)](https://github.com/tbagriyanik/minOs/wiki)

**minOs (Sarman Edition) v1.8.0**, çekirdek tarafında `no_std` ve `no_main` kullanan, Rust ile geliştirilmiş bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Derleme sırasında `fontdue` kullanılarak TTF yazı tiplerinden bitmap atlasları üretilir; çekirdek bu atlasları grafik arayüzünde kullanır.

> 📚 **Resmi Kılavuz ve Dokümantasyon:** Tüm donanım sürücüleri, uygulama kılavuzları, ağ/TLS yapılandırması ve geliştirici belgeleri için **[minOs GitHub Wiki](https://github.com/tbagriyanik/minOs/wiki)** sayfasını ziyaret edebilirsiniz.

---

## Yenilikler (v1.8.0)

- **X.509 Sertifika Doğrulama (Zaman & SNI/CN):** TLS el sıkışmasında sunucu sertifikasının geçerlilik tarih aralığı (`not_before` / `not_after`) CMOS RTC saatiyle, hedef ana makine adı ise CN (`CommonName`) ve SAN (`Subject Alternative Name`) DNS kayıtları üzerinden RFC 6125 kurallarıyla (tek düzeyli joker `*.domain.com` dahil) doğrulanır.
- **VMware VMXNET3 & Genişletilmiş Ağ Desteği:** Intel e1000 ailesine ek olarak VMware PCI vendor `0x15AD` ve aygıt kimlikleri (`0x07B0` VMXNET3, `0x0720` VMXNET) algılanır; MAC adresi, bağlantı (link) yoklaması ve donanım adı raporlaması entegre edildi.
- **minPak / minpkg SHA-256 Bütünlük Doğrulaması:** Paket yöneticisine indirilen ve kurulan ikili paketlerin SHA-256 özetini kontrol etme özelliği eklendi (`minPak install <paket> [hash]`, `minPak sha256 <dosya>`, `minPak verify <dosya> <hash>`).
- **USB Klavye ve Fare İyileştirmeleri (xHCI):** xHCI başlatmasındaki temel hatalar giderildi: `HCSPARAMS1.MaxPorts` alanı yanlış bit aralığından okunuyordu, MMIO yazmaç erişimleri taban adresi iki kez ekliyordu, port kayıtları 4 bayt yerine 16 bayt adımla okunmalıydı ve `Address Device` bağlamında A0 (Slot Context) bayrağı eksikti. Bu düzeltmelerle USB klavye/fare QEMU xHCI üzerinde başarıyla numaralandırılıyor. Ayrıca HID kullanım→PS/2 eşlemesindeki kayma düzeltildi (yön tuşları, Home/End, Insert/Delete ve sayısal tuş takımı artık doğru çalışıyor); rapor tanımlayıcısı (Report Descriptor) doğru çözümlenerek boot protokolü sunmayan farelerde mutlak/göreli ayrımı, 12/16-bit eksenler, Report ID ve tekerlek desteği eklendi. Kablosuz klavye+fare alıcıları gibi çok arayüzlü aygıtların tüm HID arayüzleri aynı xHCI slotunda çalışıyor; Caps Lock/Num Lock LED'leri aygıta geri bildiriliyor ve tak/çıkarma sırasında DMA bellek sızıntısı önlendi.
- **Açılış Konsolunda USB HID & PS/2 Fallback Teşhisi:** Açılışta yerel xHCI USB HID denetleyicisi veya aygıtı bulunamadığında önyükleme konsoluna ve seri porta net teşhis günlüğü basılır (`[INPUT] USB HID bulunamadı, PS/2 fallback kullanılıyor`).
- **Açılışta Otomatik Çözünürlük ve Kalıcılık:** Limine önyükleme menüsünden seçilen ekran çözünürlüğü (`kernel_cmdline: res=WxH`) ve Ayarlar uygulamasından diske kaydedilen tercihler (`settings.cfg`), masaüstü açılmadan önce donanıma (BGA) otomatik olarak uygulanır; sistem doğrudan seçili çözünürlükle açılır.
- **Dinamik Çözünürlük ve Masaüstü Uyumlanması:** Çalışma zamanında çözünürlük değiştirildiğinde masaüstü sınırları, imleç hareket alanı, pencere konumları (`keep_windows_on_screen`) ve duvar kağıdı yeni çözünürlüğe anında uyarlanır; ekran tamamen yeniden çizilir (`force_render`).
- **QEMU SDL / GTK Desteği & 64 MB VRAM:** `run.ps1` betiğine `-Display sdl` parametresi eklendi (GTK pixbuf uyarıları engellenir, daha düşük fare gecikmesi sunar). QEMU video belleği 64 MB'a yükseltildi (`-global VGA.vgamem_mb=64`).
- **Statik BSS Bellek Mimarisi (CodeQL Alert #25 Düzeltmesi):** Çekirdek bellek yöneticisi (`MemoryManager`) statik BSS depolama alanına taşınarak yığın belleğin kaçışı ve raw pointer çözümlenmesi güvenli hale getirildi.
- **Genişletilmiş Kripto & TLS 1.3:** SHA-384, SHA-512 ve ECDSA (secp256r1) imza doğrulama/üretme ilkelleri eklendi (57 host testi geçerli); TLS 1.3 tam el sıkışma döngüsü (RSA & ECDSA CertificateVerify, HKDF anahtar planı ve şifreli kayıt işleme) entegre edildi.
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
  - 📦 **[Yerleşik 15 Uygulama](https://github.com/tbagriyanik/minOs/wiki/Uygulamalar)**: Web Tarayıcısı, Not Defteri, Dosya Yöneticisi, Kontrol Paneli, Paket Yöneticisi vb.
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

---

### 💾 Dosya Sistemleri Desteği ve Format Sınırları

minOs VFS katmanı farklı dosya sistemlerini destekler; ancak her dosya sisteminin yetenek düzeyi farklıdır:

| Dosya Sistemi | Destek Düzeyi | Açıklama |
| :--- | :--- | :--- |
| **FAT32** | **Tam Okuma / Yazma / Biçimlendirme / Kalıcılık** | MBR/GPT disklerde tam dosya okuma, yazma, oluşturma, silme; `settings.cfg` ve `VFS.DAT` sistem kalıcılığı. |
| **Ext2** | **Okuma / Yazma / Biçimlendirme** | `src/fs/ext2.rs` üzerinden ATA disk bölümlerine mount etme, dizin listeleme ve dosya okuma/yazma. |
| **Ext4** | **Yalnızca Biçimlendirme (Format-only)** | `src/fs/ext4.rs` ile disk ext4 süperblokuyla biçimlendirilebilir. Temel ext2 geriye dönük uyumluluğu haricinde ext4 extents ve journaling okuma/yazma sürücüsü **bulunmamaktadır** (yalnızca format). |
| **exFAT** | **Yalnızca Biçimlendirme (Format-only)** | `src/fs/exfat.rs` ile bölüm biçimlendirme desteklenir; henüz VFS dosya okuma/yazma sürücüsü **bulunmamaktadır**. |
| **ISO9660** | **Salt Okunur (Read-only)** | Canlı CD/DVD ve kurulum medyası içeriğini okuma desteği. |

---

### ⚡ Çekirdek Mimarisi ve Donanım Durumu (UEFI, USB-HID, SMP)

- **UEFI & BIOS Dual-Boot:** Limine v8 önyükleyici sayesinde sistem hem modern x86_64 UEFI sistemlerde hem de geleneksel BIOS (Legacy) ortamlarında sorunsuz önyüklenir.
- **Yerel xHCI USB HID & PS/2 Fallback:** xHCI (USB 3.x) denetleyicileri üzerinden yerel USB klavye ve fare sürücüsü (`src/drivers/usb_hid.rs`) aktiftir. USB aygıtı bulunamadığında veya BIOS CSM/Legacy USB kapalı olduğunda sistem kararlılık için otomatik olarak PS/2 fallback moduna geçer ve önyükleme konsoluna bilgilendirme günlüğü düşer. QEMU'da PS/2'yi tamamen kapatıp girdiyi yalnızca USB klavye/fare üzerinden test etmek için `.\run.ps1 -NoPs2`; QMP ile otomatik klavye/fare enjeksiyonu dahil uçtan uca doğrulama için `.\test_qemu.ps1 -NoPs2` kullanılabilir.
- **SMP (Symmetric Multiprocessing) Altyapısı:** Çekirdekte Limine SMP protokolü, Local APIC timer/IPI, TTAS Spinlock senkronizasyonu ve Görev Yöneticisi per-core CPU izleme çubukları mevcuttur; fiziksel laptop açılış kararlılığı için varsayılan profilde tek çekirdek emniyetiyle başlar.

---

### 💻 Doğrudan QEMU Çalıştırma Örnekleri (Ses ve Ağ Dahil)

`run.ps1` otomasyonunu kullanmadan doğrudan QEMU komut satırıyla çalıştırmak isterseniz, ses (HDA) ve ağ (e1000) bayraklarının eksiksiz verilmesi gerekir:

#### Linux (PulseAudio + Intel HDA Duplex + e1000):
```bash
qemu-system-x86_64 -m 512M -smp 2 \
  -cdrom minos.iso -boot d \
  -vga std -global VGA.vgamem_mb=64 \
  -audiodev pa,id=snd0 \
  -device intel-hda -device hda-duplex,audiodev=snd0 \
  -netdev user,id=n0 -device e1000,netdev=n0
```

#### Windows (SDL veya DirectSound + Intel HDA Output + e1000):
```powershell
# SDL ses arka ucu DirectSoundCapture (mikrofon açma) hatasını önler:
qemu-system-x86_64.exe -m 512M -smp 2 `
  -cdrom .\minos.iso -boot d `
  -vga std -global VGA.vgamem_mb=64 `
  -audiodev driver=sdl,id=snd0 `
  -device intel-hda -device hda-output,audiodev=snd0 `
  -netdev user,id=n0 -device e1000,netdev=n0
```
> 💡 *Not: DirectSound kullanıyorsanız `hda-duplex` yerine `hda-output` veya `-audiodev driver=sdl` kullanmak mikrofon yakalama aygıtı eksikliğinden kaynaklanan QEMU açılış çökmelerini önler.*

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

## 🆕 Yeni Özellikler (v1.8.1)

- **Not Defteri – Bul ve Değiştir:** `Ctrl+F` ile **Bul** çubuğu, `Ctrl+H` ile **Değiştir** çubuğu açılır. Metin girişi aktif alanda yönlendirilir, `Esc` ile çubuk kapanır. Gelişmiş arama seçenekleri ve otomatik vurgulama desteklenir.
- **Klavye Kısayolları Geliştirildi:** `FindText` ve `ReplaceText` eylemleri `src/main.rs` içinde tam olarak işlenmiştir; artık seçili metin içinde hızlı arama / değiştirme mümkündür.
- **Dokümantasyon Güncellendi:** README ve `docs/features.md` dosyalarına yeni özellikler eklendi.
