# 🐱 minOs - Sarman Edition

[![Rust](https://img.shields.io/badge/Language-Rust%20Nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Display](https://img.shields.io/badge/Resolution-1280x720%2032--bit%20RGB-purple.svg)]()
[![Min RAM](https://img.shields.io/badge/Min%20RAM-4%20MB-red.svg)]()
[![Min CPU](https://img.shields.io/badge/Min%20CPU-x86__64%20SSE2-red.svg)]()
[![Lines of Code](https://img.shields.io/badge/Kod%20Satırı-~16.400%20(Rust)-lightgrey.svg)]()
[![GitHub Release](https://img.shields.io/github/v/release/tbagriyanik/minos?color=brightgreen)](https://github.com/tbagriyanik/minos/releases)

**minOs (Sarman Edition)**, çekirdek tarafında `no_std` ve `no_main` kullanan, Rust ile geliştirilmiş bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Derleme sırasında `fontdue` kullanılarak TTF yazı tiplerinden bitmap atlasları üretilir; çekirdek bu atlasları grafik arayüzünde kullanır.

---

## 📥 Hazır ISO İndirme (Download)

minOs'u derlemeden doğrudan QEMU, VirtualBox (64bit) veya gerçek donanımda test etmek için hazır ISO imajı:

🔗 **Repo Üzerinden Doğrudan İndir:**  
[https://github.com/tbagriyanik/minos/blob/main/minos.iso](https://github.com/tbagriyanik/minos/blob/main/minos.iso)

---

## 📸 Ekran Görüntüleri (Screenshots)

### Welcome Ekranı
![Welcome](docs/screenshots/welcome.png)

### Tema 1
![Tema 1](docs/screenshots/tema1.png)

### Tema 2
![Tema 2](docs/screenshots/tema2.png)

---

## ✨ Özellikler ve Uygulama Paketi

minOs, tam teşekküllü bir masaüstü deneyimi sunan **11 adet yerleşik uygulama** ve pencere yöneticisi içerir:

| Uygulama | Açıklama |
| :--- | :--- |
| **🔘 Başlat Menüsü** | Kategori menüleri (`Sistem`, `Üretkenlik`, `Medya`, `Güç`) altında simgeli grafik listeleme. Klavye ok tuşları, Enter, fare tekerleği gezintisi ve arama. |
| **📁 Dosya Yöneticisi** | Sanal dosya sistemi (VFS/TarFS) gezgini; dosya türü simgeleri (`[LOG]`, `[TXT]`, `[IMG]`, `[SND]`, `[VID]`), boyutlar ve konum çubuğu. `-UsbDisks` ile bağlanan USB belleğin **FAT32** ve CD'nin **ISO9660** içeriği de listelenir. |
| **⚙️ Sistem Ayarları** | Dört sekme (Temel / İleri / Ağ / Hakkında): beş tema, ses düzeyi, gece ışığı, dil (TR/EN), klavye düzeni, yazı yakınlaştırma, çözünürlük ve bölge seçimi; pencere düzeni kaydetme ve **`settings.cfg` üzerinden kalıcı ayar saklama**. |
| **📝 Not Defteri** | Üst menü çubuğu (`Dosya`, `Düzen`, `Yardım`), metin temizleme/kaydetme, **Kes / Kopyala / Yapıştır / Tümünü Seç**, **Geri Al / İleri Al** (Ctrl+Z / Ctrl+Y). Türkçe karakter desteği, `Backspace`, `Enter`, **Home / End**, **Shift+ok tuşlarıyla seçim**, satır numaraları ve imleç. |
| **🌐 Web Tarayıcısı** | Yerel `minos://help` ve `minos://cats` sayfaları, sekmeli (tab) gezinme, `file://` adresleriyle yerel dosya görüntüleme ve e1000 üzerinden ARP/ping/DNS test paneli. |
| **🖼️ Resim Gösterici** | Şeffaf dama tahtası tuvali üzerinde 4x yakınlaştırmalı 32-bit renkli piksel sanatı kedi maskotu ve dosya meta verileri. |
| **🎵 Müzik Çalar** | HDA sürücüsü üzerinden PC hoparlöründen melodi çıkışı, animasyonlu spektrum ekolayzerı ve çalma listesi. |
| **🎬 Video Oynatıcı** | Canlı synthwave ufuk sahnesi, neon güneş scanline'ları, aşağı doğru akan 3D perspektif grid yol ve siber araç simülasyonu. |
| **📊 Görev Yöneticisi** | Çekirdek bellek kullanımı, CPU dalgalanma grafiği, görev tablosu, süreç sonlandırma (`Kill`) ve RAM grafik çubuğu. |
| **🧮 Hesap Makinesi** | Klavyeyle giriş yapılabilen ifade hesaplayıcı (`+`, `-`, `*`, `/`), `Backspace` ve `Esc` ile temizleme. |
| **🎨 Çizim Tahtası** | Piksel tabanlı serbest çizim tuvali, renk paleti ve dairesel silgi aracı. Fare basılı tutuldukça **sürekli/akıcı çizim** (Windows Paint benzeri), fırça kenarının otomatik kırpılması ve 4096 noktalık tuval geçmişi. |
| **⌨️ Komut İstemi (MinShell)** | `help`, `ls`, `cat`, `echo`, `clear`, `mem`, `ps`, `apps`, `ipconfig`, `ping`, `dns`, `date`, `uname`, `whoami`, `theme`, `open`/`run` komutları; Tab ile otomatik tamamlama ve yukarı/aşağı oklarla komut geçmişi. |

> **Not:** `src/apps/` altındaki **Kontrol Paneli** (`controlpanel.rs`) tanımlıdır ancak şu anda Başlat menüsünde kayıtlı olmadığı için çalıştırılamaz.

### 🆕 Modern Arayüz ve Kullanılabilirlik Yenilikleri

- **👋 Hoş Geldin Ekranı:** İlk masaüstü kullanımından önce arayüz dilini (Türkçe/English) ve beş masaüstü temasından birini seçin. Seçimler kalıcı ayarlara kaydedilir.
- **🇹🇷 Tam Türkçe Karakter Desteği:** Sistem Ayarları, Görev Yöneticisi, Not Defteri, Dosya Yöneticisi, Başlat menüsü ve durum çubukları dahil arayüzdeki tüm metinler Türkçe karakterlerle (`ç`, `Ç`, `ğ`, `Ğ`, `ı`, `I`, `İ`, `ö`, `Ö`, `ş`, `Ş`, `ü`, `Ü`) işlenir.
- **🔤 TTF Tabanlı Fontlar:** Genel arayüz Inria Sans, komut istemi Geist Mono kullanır. Derleme betiği fontların sunduğu karakterlerden bitmap atlas oluşturur; yazı yakınlaştırma ayarı arayüzde uygulanır.
- **⌨️ Klavyeyle Menü Gezintisi (Ok Tuşları & Enter):** Başlat menüsü ve bağlam menülerinde **Yukarı/Aşağı/Sol/Sağ ok tuşları** ile gezinebilir, **Enter** tuşuna basarak seçili uygulamayı veya komutu çalıştırabilirsiniz.
- **🖱️ Fare Tekerleği (Scroll Wheel) Desteği:** Başlat menüsü açıkken fare tekerleğiyle liste seçenekleri arasında geçiş yapılabilir. Tüm pencere içerikleri (Not Defteri, Dosya Yöneticisi listeleri) dikey olarak yumuşak şekilde kaydırılır.
- **📐 Daha Geniş Kenar Tutamaçları (12px Resize Grip):** Pencereleri kenarlarından tutup boyutlandırma alanı 8 pikselden **12 piksele** çıkarıldı. Artık pencerelerin kenar ve köşelerinden tutulması ve boyutlandırılması çok daha kolaydır.
- **🖼️ Masaüstü Simgesi Tıklama Koruması:** Açık ve görünür bir pencere masaüstü simgelerinin üzerine geldiğinde, pencereye tıklanıldığında altındaki masaüstü simgelerinin istemeden açılması engellendi.
- **🎨 Dinamik Tema ve Kontrast:** Kadife Gece, Gün Işığı, Kutup Mavisi, Yeşil Terminal ve Gün Batımı temaları arasında canlı geçiş. Düğme ve seçili öğe metinleri zemin rengine göre siyah veya beyaz seçilerek kontrast korunur.
- **🪟 Akıllı Pencere Yönetimi:** Çift tıkla maksimize etme, ekranın en üstüne sürükleyerek otomatik maksimize (snap) etme, pencere ortalama, simge durumuna küçültme ve yumuşak z-sırası odak yönetimi.
- **🗔 Çoklu Pencere:** Dosya yöneticisi, not defteri ve diğer desteklenen uygulamalar birden fazla pencerede açılabilir. Sistem Ayarları ve Görev Yöneticisi tek örnek olarak çalışır.
- **📌 Menülerin Dış Tıklamayla Kapanması:** Başlat, sağ tık, ses, takvim ve Görev Yöneticisi açılır menüsünün dışına tıklayınca menü kapanır; tıklama hedefteki pencere veya görev çubuğu öğesine iletilir.
- **📋 Not Defteri Açılır Menü Komutları:**
  - **Dosya:** Yeni Belge, Kaydet, Temizle
  - **Düzen:** Kes, Kopyala, Yapıştır, Tümünü Seç
  - **Yardım:** Not Defteri Hakkında
- **📝 Windows Notepad Benzeri Metin Düzenleme:**
  - **Çift tıkla kelime seçimi:** Herhangi bir kelimeye çift tıklayarak o kelimeyi seçin.
  - **Sürükleyerek seçim:** Sol tuşu basılı tutup fareyi sürükleyerek metin seçin; tıklanan karaktere doğru imleç konumlanır (satır kaydırma ve dikey kaydırma hesaba katılır).
  - **Home / End:** İmleci satır başına/sonuna taşır.
  - **Shift + Ok Tuşları:** Yukarı/Aşağı/Sol/Sağ ile seçimi genişletir.
  - **Geri Al / İleri Al:** `Ctrl+Z` (Geri Al) ve `Ctrl+Y` (İleri Al) — 32 seviyelik düzenleme geçmişi.
- **🎨 Windows Paint Benzeri Çizim:**
  - **Basılı Tutup Çizme:** Sol tuşu basılı tuttukça fare hareketi sürekli çizer; hızlı hareketlerde iki nokta arası enterpolasyonla doldurulup kesik çizgi oluşmaz.
  - **Otomatik Vuruş Bittiği:** Tuval dışına çıkıldığında veya tuş bırakıldığında kalem yolu otomatik sonlanır.
  - **Renk Paleti ve Silgi:** Mavi/Kırmızı/Yeşil/Siyah renk örnekleri, dairesel **Silgi** ve **Temizle** düğmeleri.

---

## 🖥️ Minimum Donanım Gereksinimleri (Minimum Hardware Requirements)

| Bileşen | Minimum | Önerilen |
| :--- | :--- | :--- |
| **CPU** | x86_64 (64-bit), SSE2 desteği | x86_64, SSE4.1+ |
| **RAM** | **4 MB** | 32 MB – 64 MB |
| **Depolama** | Yalnızca CD/ISO (RAM tabanlı) | 64 MB+ qcow2 disk |
| **Ekran** | 1280×720 32-bit framebuffer | 1280×720 veya daha yüksek |
| **Ses** | — (isteğe bağlı) | Intel HDA uyumlu |
| **Ağ** | — (isteğe bağlı) | Intel e1000 uyumlu |

> **Not:** minOs 4 MB RAM ile stabil çalışır; grafik arayüzü, font atlasları ve çekirdek tamponları bu alanda sığar. QEMU'da test için `-Memory 4M` parametresi yeterlidir. FAT32 disk yazma gibi özellikler için 32+ MB önerilir.

---

## 🛠️ Sistem Gereksinimleri (Prerequisites)

minOs'u kaynak kodundan derlemek ve çalıştırmak için aşağıdaki araçlar gereklidir:

1. **Rust Nightly Toolchain:**
   ```powershell
   rustup default nightly
   rustup component add rust-src
   ```
2. **QEMU x86_64 Emulator:**
   - [QEMU Resmi İndirme Sayfası](https://www.qemu.org/download/#windows)
   - Varsayılan kurulum yolu: `C:\Program Files\qemu\qemu-system-x86_64.exe`
3. **ISO Oluşturma Aracı:**
   - Windows için: `oscdimg.exe` (Windows ADK veya projedeki paketleyici).
   - Linux / macOS için: `xorriso` veya `mkisofs`.
4. **PowerShell 5.1+ (Windows):** Derleme ve ISO paketleme otomasyonları için.

---

## 🚀 Derleme ve Çalıştırma Adımları (Build & Run)

### Tek Komutla Derle ve Başlat (Önerilen)
minOs, çekirdeği derleyen, ISO dosyasını paketleyen ve QEMU'yu başlatan hazır bir betik içerir:

```powershell
# Depoyu klonlayın ve dizine girin
git clone https://github.com/tbagriyanik/minos.git
cd minos

# Derleyin ve QEMU üzerinde çalıştırın
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

#### `run.ps1` Seçenekleri (Switches)

`run.ps1`, çekirdeği `cargo build` ile derledikten sonra ISO'yu paketler ve QEMU'yu başlatır. Aşağıdaki anahtarlar ile davranışı özelleştirebilirsiniz:

| Anahtar | Açıklama |
| :--- | :--- |
| `-BuildOnly` | Yalnızca derler; QEMU'yu başlatmadan çıkar. Hızlı derleme kontrolü için idealdir. |
| `-Debug` | Debug profilinde derler (`target\x86_64-minos\debug\minos-kernel`). Varsayılan **release**'dir. |
| `-UsbDisks` | Sistemdeki USB diskleri **salt okunur** olarak QEMU'ya `usb-storage` aygıtı olarak bağlar. |
| `-UsbDiskReadWrite` | USB diskleri **yazma izniyle** bağlar (dikkatli kullanın). |
| `-CpuCount N` | QEMU'ya verilecek sanal CPU sayısı (varsayılan `2`, aralık `1–64`). |
| `-Memory 1G` | Konuk RAM miktarı (`512M`, `1G` gibi; varsayılan `512M`). |
| `-DiskImage dosya.qcow2` | Mevcut qcow2 sanal disk imajını IDE disk olarak bağlar. Oluşturma gerekiyorsa `qemu-img create -f qcow2 disk.qcow2 8G` kullanın. |

```powershell
# Yalnızca derle (QEMU başlatma)
powershell -ExecutionPolicy Bypass -File .\run.ps1 -BuildOnly

# Debug profili + USB diskleri salt okunur ekle
powershell -ExecutionPolicy Bypass -File .\run.ps1 -Debug -UsbDisks

# 4 sanal çekirdek, 2 GB RAM ve 8 GB qcow2 diskle çalıştır
powershell -ExecutionPolicy Bypass -File .\run.ps1 -CpuCount 4 -Memory 2G -DiskImage .\disk.qcow2
```

`-CpuCount` QEMU'nun SMP ayarını yapar. minOs çekirdeği şu anda çok çekirdekli (SMP) iş planlaması uygulamıyor; ek çekirdekler konukta paralel uygulama çalıştırma sağlamaz. RAM değeri QEMU'ya ayrılan miktardır. Disk imajı ayrıca oluşturulur ve `-DiskImage` ile bağlanır; USB disk seçenekleri fiziksel USB aygıtları içindir.

> **Ses:** `run.ps1` ses çıkışı için QEMU `sdl` audio sürücüsünü (`-audiodev driver=sdl,id=snd0`) ve yalnızca çıkış amaçlı `hda-output` codec'ini kullanır. Bu, DirectSound'un zorunlu mikrofon/capture başlatma hatasını önler.

> **Ağ:** QEMU kullanıcı modu (SLIRP) ve `e1000` ile ARP/ping/TCP çalışır; `8080` portu konuk makinenin `80` portuna yönlendirilir (`hostfwd=tcp::8080-:80`).

---

### Adım Adım Manuel Derleme (Manual Steps)

#### 1. Çekirdeği Derleme (Kernel Build)
minOs çekirdeği, özel hedef mimarisi ve `-Zbuild-std` ile gereken çekirdek kütüphanelerini kaynak kodundan derleyerek oluşturulur:

```powershell
cargo build --release -Zjson-target-spec --target x86_64-minos.json `
  -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
```

#### 2. Boot Edilebilir ISO İmajını Paketleme (ISO Creation)
Limine bootloader dosyalarını çekirdekle birleştirmek ve El Torito tablosunu yamalamak için:

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```
> *İşlem tamamlandığında kök dizinde `minos.iso` dosyası oluşturulur.*

#### 3. QEMU ile Test Etme (Run in QEMU)
```powershell
& 'C:\Program Files\qemu\qemu-system-x86_64.exe' -cdrom minos.iso -m 512M -vga std `
  -audiodev driver=sdl,id=snd0 -device intel-hda -device hda-output,audiodev=snd0 `
  -netdev user,id=n0,hostfwd=tcp::8080-:80 -device e1000,netdev=n0
```

#### 4. VirtualBox Kurulumu & Ayarları (VirtualBox Setup)
VirtualBox'ta **`PANIC: limine: This CPU does not support 64-bit mode`** hatası alıyorsanız aşağıdaki 2 ayarı kontrol edin:

1. **Sanal Makine Türü:** Sanal makineyi oluştururken **İşletim Sistemi Türü** olarak **`Other / Other (64-bit)`** seçildiğinden emin olun.
2. **Sanal Donanım İvmelendirmesi (VT-x / AMD-V):**
   - Windows kullanıyorsanız ve VirtualBox 64-bit seçeneği çıkmıyorsa, **Hyper-V**'nin açık olmadığından ve BIOS'ta **Intel VT-x / AMD-V** donanım sanallaştırmasının aktif olduğundan emin olun.
   - VM Ayarları -> **Sistem** -> **Hızlandırma** sekmesinde **Sanallaştırma Arabirimi** varsayılan/KVM olarak ayarlanmalıdır.
3. **Ses:** VM Ayarları -> **Ses** bölümünde sesi etkinleştirin ve **Ses Denetleyicisi** olarak **Intel HD Audio** seçin. minOs'un HDA sürücüsü bu denetleyiciyi kullanır.

---

## 🎮 Masaüstü Kullanımı ve Kısayollar

- **Başlat Menüsünde Gezinme:** Klavye **Yukarı / Aşağı / Sol / Sağ ok tuşları** ile menüler arasında dolaşın, **Enter** ile seçili uygulamayı çalıştırın veya **fare tekerleğini** çevirerek kaydırın.
- **Pencere Boyutlandırma:** Pencerelerin herhangi bir kenarından veya köşesinden basılı tutarak boyutlandırın (tutma alanı 12px'e çıkarılmıştır).
- **Pencere Taşıma (Drag & Drop):** Herhangi bir pencerenin üst başlık çubuğuna sol tıklayıp basılı tutarak ekran üzerinde sürükleyin.
- **Pencere Kontrolleri:**
  - Kırmızı `[x]` butonu: Pencereyi kapatır.
  - Yeşil `[□]` butonu: Pencereyi maksimize eder (tekrar tıklayınca geri yükler). Başlık çubuğuna **çift tıklamak** da aynı işi yapar.
  - Sarı `[-]` butonu: Pencereyi simge durumuna küçülterek görev çubuğuna indirir.
- **Pencere Yerleştirme (Snap):** Başlık çubuğunu ekranın en üstüne sürükleyip bırakırsanız pencere otomatik olarak maksimize edilir.
- **Pencere Değiştirme:** `Alt+Tab` pencereler arasında soldan sağa ilerler; `Shift+Alt+Tab` aynı sırada geriye gider. Alt tuşunu bırakınca seçili pencere öne gelir.
- **Sistem Tepsisi (Sağ Alt):** Sırasıyla **klavye rozeti (`TR`/`EN`)**, **tema düğmesi**, **SES** düğmesi ve **canlı saat** bulunur. Saate tıklayınca canlı takvim, **SES**'e tıklayınca ses paneli açılır.
- **Tema Değiştirme:** Görev çubuğundaki renk örneğine, Sistem Ayarları > **Değiştir** düğmesine, **tema palet şeridindeki renk örneklerine** veya masaüstü sağ tık > **Tema Değiştir**'e tıklayın.
- **Bağlama Duyarlı Sağ Tık:** Masaüstünde, Not Defteri'nde, Dosya Yöneticisi'nde veya pencere başlığında özel sağ tık menüleri açılır.
- **Not Defteri Düzenleme Kısayolları:** `Ctrl+C` (Kopyala), `Ctrl+X` (Kes), `Ctrl+V` (Yapıştır), `Ctrl+A` (Tümünü Seç), `Ctrl+Z` (Geri Al), `Ctrl+Y` (İleri Al), `Delete` (İleri sil), `Home`/`End` (satır başı/sonu) ve `Shift+Ok tuşları` (seçim genişletme).
- **Çizim Tahtası Kullanımı:** Araç çubuğundan renk veya **Silgi** seçin; tuval üzerinde sol tuşu basılı tutarak çizin. `C` tuşu veya **Temizle** düğmesi tuvali temizler.

---

## 📂 Proje Dizin Yapısı

```
minos/
├── src/
│   ├── arch/x86_64/         # SSE, IDT kesmeleri ve PIC maskeleme
│   ├── drivers/             # video (framebuffer), PS/2 fare & klavye, RTC (CMOS) saat,
│   │                        #   e1000 + TCP/IP yığını, HDA ses, ATA/ATAPI (PIO) ve USB durum katmanı
│   ├── gui/                 # MinGUI pencere yöneticisi, font motoru, tema motoru (theme.rs),
│   │                        #   kaydırma çubuğu ve pano (clipboard)
│   ├── apps/                # 11 Masaüstü uygulaması (Not Defteri, Dosya Yöneticisi, Görev Yöneticisi...)
│   ├── mm/                  # Fiziksel bellek yöneticisi (PMM)
│   ├── fs/                  # Sanal dosya sistemi (vfs.rs / TarFS) ve disk okuyucuları (diskfs.rs: FAT32 + ISO9660)
│   └── main.rs              # Çekirdek giriş noktası (_start), Limine istekleri ve ana olay döngüsü
├── boot/                    # Limine bootloader yapılandırma dosyaları (limine.cfg)
├── docs/screenshots/        # Masaüstü ve başlat menüsü ekran görüntüleri
├── build_iso.ps1            # El Torito yamalı hibrit ISO oluşturucu
├── run.ps1                  # Tek tıkla derleme ve QEMU başlatma betiği (-Debug, -BuildOnly, -UsbDisks, -UsbDiskReadWrite, -CpuCount, -Memory, -DiskImage)
├── build.rs                 # fontdue ile TTF → bitmap font atlası üretimi
└── x86_64-minos.json        # Hedef x86_64 Long Mode tanımlama dosyası
```

---

## 📊 Kod İstatistikleri (Code Statistics)

> Son güncelleme: Ekim 2026

| Ölçüt | Değer |
| :--- | :--- |
| **Toplam Rust Kaynak Dosyası** | 40 dosya (`src/**/*.rs`) |
| **Toplam Kod Satırı (ham)** | ~16.400 satır |
| **Etkin Kod Satırı** (boş/yorum hariç) | ~13.960 satır |
| **Dil** | Rust (Nightly, `no_std`) |
| **Derleme Hedefi** | `x86_64-minos` (özel bare-metal) |
| **Yerleşik Uygulama Sayısı** | 11 masaüstü uygulaması |
| **Kernel Boyutu (ISO)** | ~7.4 MB (fontlar dahil) |

---

## 📄 Lisans

Bu proje açık kaynaklıdır ve eğitim/araştırma amaçlı geliştirilmektedir.
Katkıda bulunmaktan ve geliştirmeler önermekten çekinmeyin! 🐾
