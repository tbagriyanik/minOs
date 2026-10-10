# 🐱 MinOS - Sarman Edition

[![Rust](https://img.shields.io/badge/Language-Rust%20Nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Display](https://img.shields.io/badge/Resolution-1280x720%2032--bit%20RGB-purple.svg)]()
[![GitHub Release](https://img.shields.io/github/v/release/tbagriyanik/minos?color=brightgreen)](https://github.com/tbagriyanik/minos/releases)

**MinOS (Sarman Edition)**, Rust dili ile sıfır dış bağımlılık (`no_std`, `no_main`) prensibiyle geliştirilmiş, modern grafik arayüzüne (GUI) ve zengin masaüstü uygulama ekosistemine sahip bağımsız bir 64-bit (`x86_64`) işletim sistemi çekirdeğidir.

### 📏 Kod İstatistikleri

Depodaki Rust kaynaklarında **4.379 satır kod** bulunmaktadır. Bu sayı `src/` altındaki tüm `.rs` dosyalarının satır toplamıdır; derleme çıktıları (`target/`) dahil değildir.

---

## 📥 Hazır ISO İndirme (Download)

MinOS'u derlemeden doğrudan QEMU, VirtualBox (64bit) veya gerçek donanımda test etmek için hazır ISO imajı:

🔗 **Repo Üzerinden Doğrudan İndir:**  
[https://github.com/tbagriyanik/minos/blob/main/minos.iso](https://github.com/tbagriyanik/minos/blob/main/minos.iso)

---

## 📸 Ekran Görüntüleri (Screenshots)

### 1. Başlat Menüsü & Çoklu Pencere Ortamı
![MinOS Başlat Menüsü](docs/screenshots/minos_startmenu.png)

### 2. Etkileşimli Not Defteri, Müzik Çalar Ekolayzerı & Dosya Yöneticisi
![MinOS Masaüstü](docs/screenshots/minos_desktop.png)

---

## ✨ Özellikler ve Uygulama Paketi

MinOS, tam teşekküllü bir masaüstü deneyimi sunan 11 adet yerleşik uygulama ve pencere yöneticisi içerir:

| Uygulama | Açıklama |
| :--- | :--- |
| **🔘 Başlat Menüsü** | Görev çubuğundaki kedi ikonlu **MinOS** butonuyla açılır; tüm uygulamaları numaralı kısayollarla (`1-10`) listeler. |
| **📁 Dosya Yöneticisi** | Sanal dosya sistemi (VFS) gezgini, dosya türü simgeleri (`[LOG]`, `[TXT]`, `[IMG]`, `[SND]`, `[VID]`), boyutlar ve konum çubuğu. |
| **⚙️ Sistem Ayarları** | Tema seçimi/değiştirme, canlı saat & tarih, ses düzeyi kaydırıcısı, ağ durumu (IP/DNS/Gateway), dil/klavye ve pencere düzeni kaydetme paneli. |
| **📝 Not Defteri** | Doğrudan klavye ile yazı yazılabilen metin editörü. Türkçe karakter desteği (`ç`, `ğ`, `ı`, `ö`, `ş`, `ü`), `Backspace`, `Enter`, satır numaraları (01-09) ve yanıp sönen imleç. Sağ tık menüsüyle **Kes / Kopyala / Yapıştır / Sil / Tümünü Seç**. |
| **🌐 Web Tarayıcısı (SarmanNet)** | Sekmeler, URL çubuğu (`https://catnet.os/portal`), SSL rozeti ve stilize edilmiş kart görünümleri. |
| **🖼️ Resim Gösterici (MinViewer)** | Şeffaf dama tahtası tuvali üzerinde 4x yakınlaştırmalı 32-bit renkli piksel sanatı kedi maskotu ve dosya meta verileri. |
| **🎵 Müzik Çalar (SarmanBeats)** | Müzik ritmine göre dalgalanan dinamik 18 bantlı renkli spektrum ekolayzerı (yeşil/sarı/pembe), tepe ışıkları, parça scrubber'ı ve çalma listesi. |
| **🎬 Video Player (MinPlayer)** | Canlı synthwave ufuk sahnesi, neon güneş scanline'ları, aşağı doğru akan 3D perspektif grid yol ve siber araç simülasyonu. |
| **📊 Görev Yöneticisi** | Çekirdek bellek kullanımı, CPU durumu, görev tablosu ve çalışma süresi. |
| **🧮 Hesap Makinesi (MinCalc)** | Klavyeyle giriş yapılabilen basit ifade hesaplayıcı (`+`, `-`), `Backspace` ve `Esc` ile temizleme. |
| **🎨 Çizim Tahtası (MinPaint)** | Piksel tabanlı serbest çizim tuvali ve renk paleti. |
| **⌨️ Komut İstemi (MinShell)** | Klavyeyle komut/metin girişi kabul eden retro terminal görünümü. |

### 🆕 Modern Arayüz Yenilikleri (v1.1)

- **🎨 Dinamik Tema Motoru (5 Tema):** Tüm arayüz *renk jetonları* (token) üzerinden çizilir. Görev çubuğundaki renk örneğine, sistem ayarlarındaki **Değiştir** düğmesine, **tema palet şeridindeki renk örneklerine** veya masaüstü sağ tık menüsüne tıklayarak anında **Kadife Gece (Velvet Dark)**, **Gün Işığı (Light)**, **Kutup Mavisi (Nord)**, **Yeşil Terminal** ve **Gün Batımı** temaları arasında geçiş yapabilirsiniz. Tek tıkla masaüstü, pencereler ve tüm uygulamalar uyum sağlar.
- **🪟 Gelişmiş Pencere Davranışları:** Pencereler artık **tüm kenar ve köşelerden** yeniden boyutlandırılabilir (önceden yalnızca sağ alt köşe). Kenar/köşe üzerine gelindiğinde fare imleci **yönlü yeniden boyutlandırma** biçimine döner (↔ ↕ ⤡ ⤢). Başlık çubuğu **ekranın en üstüne sürüklenip bırakıldığında** pencere otomatik maksimize edilir (snap). Çift tık, küçültme, kapatma ve z-sırası odak davranışları korunur.
- **🔤 İyileştirilmiş Yazı Tipi Motoru:** Yüksek kontrastlı metin renkleri ve gölgeli başlık çizimi. Türkçe karakterlere ek olarak para birimleri (₺ € £), yön okları (← ↑ ↓ →), madde işareti (•) ve onay (✓) glyphleri eklendi. Ayarlar > **Font** düğmesiyle **Türkçe Piksel** ve **Sıkıştırılmış (Compact)** modları arasında geçiş yapılabilir; Compact mod daha sıkı harf aralığıyla uzun satırların pencereye sığmasını sağlar.
- **🕓 Gerçek Zamanlı Saat & Takvim:** CMOS/RTC sürücüsü (`0x70`/`0x71`) üzerinden gerçek saat okunur ve görev çubuğunda canlı `SS:DD:SS` olarak gösterilir. Saate tıklayınca içinde bulunulan ayı, haftanın günlerini ve **bugünü** vurgulayan, `<` / `>` ile aylar arasında gezilebilen bir **takvim** açılır.
- **🔊 Çalışan Ses Kontrolü:** Görev çubuğundaki **SES** düğmesi ses düzeyi panelini açar; kaydırıcıyı sürükleyerek ve **Sustur** düğmesiyle sesi ayarlayabilirsiniz. Değer `AudioDevice`'a uygulanır ve ayarlar penceresindeki kaydırıcıyla senkronize çalışır.
- **🖱️ Bağlama Duyarlı Sağ Tık Menüsü:** Sağ tık menüsü tıklanan yere göre değişir:
  - **Masaüstü:** Dosya Yöneticisini Aç, Yeni Not, Sistem Ayarları, Görev Yöneticisi, Tema Değiştir, Masaüstünü Yenile.
  - **Not Defteri:** Kes, Kopyala, Yapıştır, Sil, Tümünü Seç, Tümünü Temizle.
  - **Dosya Yöneticisi:** İlişkili Uygulamayla Aç, Not Defterinde Düzenle, Resim Gösterici, Müzik Çalar, Video Oynatıcı, Dosya Özellikleri.
  - **Pencere Başlığı:** Küçült, Maksimize Et / Geri Yükle, Pencereyi Ortala, Kapat.
- **🪟 ZENGİN PENCERE DAVRANIŞLARI:** Pencereler düzgün bir z-sırası yığınıyla yönetilir (odaklanan pencere daima en üste gelir). Başlık çubuğunda **yeşil maksimize/geri yükle** düğmesi bulunur; başlık çubuğuna **çift tıklayarak** da maksimize edebilir, **ekranın en üstüne sürükleyip bırakarak** da otomatik maksimize edebilirsiniz. Pencereler **dört bir yanından ve köşelerinden** yeniden boyutlandırılabilir; kenar/köşe üzerine gelindiğinde yönlü fare imleci görünür. Dış çerçeve, yumuşak gölge ve gradyan başlık çubuğuyla çizilir; düğmeler üzerine gelince parlar.
- **🔤 İyileştirilmiş Yazı Okunabilirliği:** Yüksek kontrastlı metin renkleri, başlıklar için gölgeli metin çizimi, gradyan paneller ve ölçeklenebilir/merkezlenebilir metin yardımcıları. Para birimi, ok ve sembol glyphleri ile **Piksel/Sıkıştırılmış** font modu desteği.
- **📜 Taşma Koruması, Kaydırma Çubukları (Scrollbar) ve İçerik Kırpma:** Tüm pencere içeriği, pencere çerçevesine **kırpılır (clip)** — içerik hiçbir koşulda pencere dışına taşmaz. İçerik pencereye sığmadığında **dikey ve yatay kaydırma çubukları** otomatik belirir. Kaydırma çubuğu tutamacını fare ile sürükleyebilir, track üzerine tıklayarak zıplayabilir veya **fare tekerleği** ile kaydırabilirsiniz. **Not Defteri** metni pencere genişliğine göre otomatik satır kaydırma (word-wrap) yapar ve satır numaraları (gutter) kaydırma konumuna göre dinamik güncellenir; **Dosya Yöneticisi** listesi dikey olarak kaydırılır. Pencereler yeniden boyutlandırıldıkça içerik alanı canlı olarak yeniden hesaplanır.

---

## 🛠️ Sistem Gereksinimleri (Prerequisites)

MinOS'u kaynak kodundan derlemek ve çalıştırmak için aşağıdaki araçlar gereklidir:

1. **Rust Nightly Toolchain:**
   ```powershell
   rustup default nightly
   rustup component add rust-src
   ```
2. **QEMU x86_64 Emulator:**
   - [QEMU Resmi İndirme Sayfası](https://www.qemu.org/download/#windows)
   - Varsayılan kurulum yolu: `C:\Program Files\qemu\qemu-system-x86_64.exe`
3. **ISO Oluşturma Aracı:**
   - Windows için: `oscdimg.exe` (Windows ADK ile birlikte gelir veya `tools/` içinde yer alır).
   - Linux / macOS için: `xorriso` veya `mkisofs`.
4. **PowerShell 5.1+ (Windows):** Derleme ve ISO paketleme otomasyonları için.

---

## 🚀 Derleme ve Çalıştırma Adımları (Build & Run)

### Tek Komutla Derle ve Başlat (Önerilen)
MinOS, çekirdeği derleyen, ISO dosyasını paketleyen ve QEMU'yu başlatan hazır bir betik içerir:

```powershell
# Depoyu klonlayın ve dizine girin
git clone https://github.com/tbagriyanik/minos.git
cd minos

# Derleyin ve QEMU üzerinde çalıştırın
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

---

### Adım Adım Manuel Derleme (Manual Steps)

#### 1. Çekirdeği Derleme (Kernel Build)
MinOS çekirdeği, özel hedef mimarisi ve sıfır bağımlılık bayraklarıyla derlenir:

```powershell
cargo build --release
```
> *Not: Çekirdek ikilisi `target/x86_64-minos/release/minos-kernel` konumunda üretilir.*

#### 2. Boot Edilebilir ISO İmajını Paketleme (ISO Creation)
Limine bootloader dosyalarını çekirdekle birleştirmek ve El Torito tablosunu yamalamak için:

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```
> *İşlem tamamlandığında kök dizinde `minos.iso` dosyası oluşturulur.*

#### 3. QEMU ile Test Etme (Run in QEMU)
```powershell
& 'C:\Program Files\qemu\qemu-system-x86_64.exe' -cdrom minos.iso -m 512M -vga std
```

#### 4. VirtualBox Kurulumu & Ayarları (VirtualBox Setup)
VirtualBox'ta **`PANIC: limine: This CPU does not support 64-bit mode`** hatası alıyorsanız aşağıdaki 2 ayarı kontrol edin:

1. **Sanal Makine Türü:** Sanal makineyi oluştururken **İşletim Sistemi Türü** olarak **`Other / Other (64-bit)`** seçildiğinden emin olun (Varsayılan 32-bit seçildiğinde 64-bit mod çalışmaz).
2. **Sanal Donanım İvmelendirmesi (VT-x / AMD-V):**
   - Windows kullanıyorsanız ve VirtualBox 64-bit seçeneği çıkmıyorsa, **Hyper-V**'nin açık olmadığından ve BIOS'ta **Intel VT-x / AMD-V** donanım sanallaştırmasının aktif olduğundan emin olun.
   - VM Ayarları -> **Sistem** -> **Hızlandırma** sekmesinde **Sanallaştırma Arabirimi** varsayılan/KVM olarak ayarlanmalıdır.

---

## 🎮 Masaüstü Kullanımı ve Kısayollar

- **Pencere Taşıma (Drag & Drop):** Herhangi bir pencerenin üst başlık çubuğuna sol tıklayıp basılı tutarak ekran üzerinde sürükleyebilirsiniz.
- **Pencere Kontrolleri:**
  - Kırmızı `[x]` butonu: Pencereyi kapatır.
  - Yeşil `[□]` butonu: Pencereyi maksimize eder (tekrar tıklayınca geri yükler). Başlık çubuğuna **çift tıklamak** da aynı işi yapar.
  - Sarı `[-]` butonu: Pencereyi simge durumuna küçülterek görev çubuğuna indirir.
  - Sağ alt köşedeki tutamaçtan **veya pencerenin herhangi bir kenarından/köşesinden** pencereyi yeniden boyutlandırabilirsiniz. Kenar/köşe üzerine gelince imleç yönlü ok biçimine döner.
- **Pencere Yerleştirme (Snap):** Başlık çubuğunu ekranın en üstüne sürükleyip bırakırsanız pencere otomatik olarak maksimize edilir.
- **Görev Çubuğu:** Açık olan pencerelerin sekmeleri (`[Dosya]`, `[Not]`, `[Müzik]`, vb.) görev çubuğunda listelenir. Tıklayarak pencereleri öne getirebilir veya simge durumuna küçültebilirsiniz.
- **Sistem Tepsisi (Sağ Alt):** Sırasıyla **klavye rozeti**, **tema düğmesi**, **SES** düğmesi ve **canlı saat** bulunur. Saate tıklayınca takvim, **SES**'e tıklayınca ses paneli açılır.
- **Tema Değiştirme:** Görev çubuğundaki renk örneğine, Sistem Ayarları > **Değiştir** düğmesine, **tema palet şeridindeki renk örneklerine** veya masaüstü sağ tık > **Tema Değiştir**'e tıklayın. Beş hazır tema arasında geçiş yapılır.
- **Bağlama Duyarlı Sağ Tık:** Masaüstünde, Not Defteri'nde, Dosya Yöneticisi'nde veya pencere başlığında farklı menüler açılır (bkz. yukarıdaki yenilikler).
- **Klavye Düzeni Değiştirme:** Görev çubuğunun sağ altındaki `[TR]` / `[EN]` rozetine tıklayarak veya `Tab` tuşuna basarak Türkçe Q ve Amerikan US klavye düzenleri arasında geçiş yapabilirsiniz.
- **Kaydırma (Scroll):** İçerik pencereye sığmadığında sağda/altta kaydırma çubukları belirir. **Fare tekerleği** ile kaydırın, çubuğun tutamacını sürükleyin veya çubuk üzerine tıklayın. İçerik asla pencere dışına taşmaz.
- **Hızlı Uygulama Başlatma:** Başlat menüsü açıkken klavyeden `1` ile `8` arasındaki sayılara basarak ilgili uygulamayı anında açabilirsiniz.

---

## 📂 Proje Dizin Yapısı

```
minos/
├── src/
│   ├── arch/x86_64/         # SSE, IDT kesmeleri ve PIC maskeleme
│   ├── drivers/             # PS/2 fare, klavye, framebuffer video, RTC (CMOS) saat ve donanım sürücüleri
│   ├── gui/                 # Pencere yöneticisi, font motoru, tema motoru (theme.rs) ve masaüstü kompozitörü
│   ├── apps/                # 8 Masaüstü uygulaması (Not Defteri, Müzik Çalar, Tarayıcı...)
│   ├── mm/                  # Fiziksel bellek yöneticisi (PMM)
│   ├── fs/                  # Sanal dosya sistemi (VFS / TarFS)
│   └── main.rs              # Çekirdek giriş noktası (_start) ve olay döngüsü
├── boot/                    # Limine bootloader yapılandırma dosyaları (limine.cfg)
├── docs/screenshots/        # Masaüstü ve başlat menüsü ekran görüntüleri
├── build_iso.ps1            # El Torito yamalı hibrit ISO oluşturucu
├── run.ps1                  # Tek tıkla derleme ve QEMU başlatma betiği
└── x86_64-minos.json        # Hedef x86_64 Long Mode tanımlama dosyası
```

---

## 📄 Lisans

Bu proje açık kaynaklıdır ve eğitim/araştırma amaçlı geliştirilmektedir.
Katkıda bulunmaktan ve geliştirmeler önermekten çekinmeyin! 🐾
