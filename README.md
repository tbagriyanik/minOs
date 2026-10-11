# 🐱 MinOS - Sarman Edition

[![Rust](https://img.shields.io/badge/Language-Rust%20Nightly-orange.svg)](https://www.rust-lang.org/)
[![Kernel](https://img.shields.io/badge/Linux_Kernel-7.2.9_Stable_(2026--10--03)-blue.svg)](https://kernel.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Display](https://img.shields.io/badge/Resolution-1280x720%2032--bit%20RGB-purple.svg)]()
[![GitHub Release](https://img.shields.io/github/v/release/tbagriyanik/minos?color=brightgreen)](https://github.com/tbagriyanik/minos/releases)

**MinOS (Sarman Edition)**, Rust dili ile sıfır dış bağımlılık (`no_std`, `no_main`) prensibiyle geliştirilmiş, modern grafik arayüzüne (GUI) ve zengin masaüstü uygulama ekosistemine sahip bağımsız bir 64-bit (`x86_64`) işletim sistemi ve masaüstü ortamıdır. 

MinOS, donanım uyumluluğu ve sürücü desteği için resmi **Linux 7.2.9 Stable (2026-10-03)** çekirdeği donanım motorunu ve **Limine Bootloader** altyapısını kullanır. Ses (ALSA/HDA), USB çevre birimleri (evdev), tam ekran sürücüleri (DRM/Framebuffer) ve TCP/IP ağ stack'i (Intel e1000 & Linux soketleri) Linux çekirdeğinin gücüyle desteklenir.

Sistem iki temel çalışma moduna sahiptir:
1. **MinOS Live CD (Grafik Masaüstü):** Doğrudan grafik masaüstüne anında boot eden tak-çalıştır Live CD modu (Varsayılan).
2. **Install MinOS to Disk (Linux Donanım Yükleyicisi):** MinOS'u sabit diske kurmak için Linux çekirdeği tabanlı yükleyici modu.

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

### 3. Sistem Ayarları, Görev Yöneticisi & Uygulama Ekosistemi
![MinOS Uygulamalar ve Ayarlar](docs/screenshots/minos_apps.png)

### 4. Açık Tema (Daylight Theme)
![MinOS Açık Tema](docs/screenshots/acikTema.png)

### 5. Mor / Kadife Gece Teması (Velvet Dark Theme)
![MinOS Mor Tema](docs/screenshots/morTema.png)

---

## ✨ Özellikler ve Uygulama Paketi

MinOS, tam teşekküllü bir masaüstü deneyimi sunan 11 adet yerleşik uygulama ve pencere yöneticisi içerir:

| Uygulama | Açıklama |
| :--- | :--- |
| **🔘 Başlat Menüsü** | Kategori menüleri (`Sistem`, `Üretkenlik`, `Medya`, `Güç`) altında simgeli grafik listeleme. Klavye ok tuşları, Enter ve fare tekerleği gezintisi. |
| **📁 Dosya Yöneticisi** | Sanal dosya sistemi (VFS) gezgini, dosya türü simgeleri (`[LOG]`, `[TXT]`, `[IMG]`, `[SND]`, `[VID]`), boyutlar ve konum çubuğu. |
| **⚙️ Sistem Ayarları** | Tema seçimi/değiştirme, canlı saat & tarih, ses düzeyi kaydırıcısı, ağ durumu (IP/DNS/Gateway), dil/klavye ve pencere düzeni kaydetme paneli. |
| **📝 Not Defteri** | İşlevsel üst menü çubuğu (`Dosya`, `Düzen`, `Biçim`, `Yardım`). Metin temizleme, dosya kaydetme, **Kes / Kopyala / Yapıştır / Tümünü Seç**, dinamik font modları (`Piksel`, `Sıkıştırılmış`, `ClearType`). Tam Türkçe karakter desteği (`ç`, `ğ`, `ı`, `İ`, `ö`, `ş`, `ü`), `Backspace`, `Enter`, satır numaraları ve yanıp sönen imleç. |
| **🌐 Web Tarayıcısı** | Yerel `minos://help`, `minos://cats` ve donanım test paneli `minos://net`. `http://` protokolü üzerinden canlı HTML DOM/sayfa render motoru, DNS ve TCP/IP istek desteği. |
| **🖼️ Resim Gösterici** | Şeffaf dama tahtası tuvali üzerinde 4x yakınlaştırmalı 32-bit renkli piksel sanatı kedi maskotu ve dosya meta verileri. |
| **🎵 Müzik Çalar** | PC hoparlöründen ve Intel HDA PCM üzerinden melodi çıkışı, animasyonlu spektrum ekolayzerı ve çalma listesi. |
| **🎬 Video Oynatıcı** | Canlı synthwave ufuk sahnesi, neon güneş scanline'ları, aşağı doğru akan 3D perspektif grid yol ve siber araç simülasyonu. |
| **📊 Görev Yöneticisi** | Çekirdek bellek kullanımı, CPU dalgalanma grafiği, görev tablosu, süreç sonlandırma (`Kill`) ve RAM grafik çubuğu. |
| **🧮 Hesap Makinesi** | Klavyeyle giriş yapılabilen ifade hesaplayıcı (`+`, `-`, `*`, `/`), `Backspace` ve `Esc` ile temizleme. |
| **🎨 Çizim Tahtası** | Piksel tabanlı serbest çizim tuvali, renk paleti ve silgi aracı. |
| **⌨️ Komut İstemi** | Klavyeyle komut ve metin girişi kabul eden retro terminal görünümü (`ipconfig`, `ping`, `dns`, `mem`, `ls`, `cat`, `open` vb.). |

### 🆕 Modern Arayüz ve Sistem Mimarisi
 
- **🐧 Linux Çekirdeği Donanım Motoru (Linux 7.2 Hardware Engine):** MinOS, tüm donanım sürücüsü gereksinimlerinde güncel **Linux 7.2** çekirdeğinin sürücü ekosistemini (USB fare/klavyeler için `evdev`, ses için ALSA/Intel HDA, modern ekran kartları ve ağ bağdaştırıcıları) temel alır.
- **📀 İki Aşamalı Boot Menüsü:**
  - **1. MinOS Live CD (Grafik Masaüstü):** Doğrudan grafik arayüzüne giriş yapar, konsol ekranında takılmaz.
  - **2. Install MinOS to Disk (Linux 7.2):** Linux 7.2 çekirdeği ortamında çalışan MinOS donanım kurulum yöneticisidir.
- **🖥️ Çoklu Çözünürlük Desteği (Full HD Ready):** Açılış menüsünden ve donanımdan **1920x1080 (Full HD)**, **1280x720**, **1024x768** ve **800x600** çözünürlükleri tam arka tampon (backbuffer) uyumluluğuyla çalışır.
- **🌐 Çift Ağ Yığını (Intel e1000 & Linux TCP/IP):** Intel e1000 ve Linux soket arayüzü desteğiyle canlı HTML DOM Web Gezgini, HTTP istemcisi, `ping` ve `dns` araçları.
- **🇹🇷 Tam Türkçe Karakter Desteği:** Sistem Ayarları, Görev Yöneticisi, Not Defteri, Dosya Yöneticisi, Başlat menüsü ve durum çubukları dahil arayüzdeki tüm metinler Türkçe karakterlerle (`ç`, `Ç`, `ğ`, `Ğ`, `ı`, `I`, `İ`, `ö`, `Ö`, `ş`, `Ş`, `ü`, `Ü`) işlenir.
- **⌨️ Klavyeyle Menü Gezintisi (Ok Tuşları & Enter):** Başlat menüsü ve bağlam menülerinde **Yukarı/Aşağı/Sol/Sağ ok tuşları** ile gezinebilir, **Enter** tuşuna basarak seçili uygulamayı veya komutu çalıştırabilirsiniz.
- **🖱️ Fare Tekerleği (Scroll Wheel) Desteği:** Başlat menüsü açıkken fare tekerleğiyle liste seçenekleri arasında geçiş yapılabilir. Tüm pencere içerikleri (Not Defteri, Dosya Yöneticisi listeleri, Terminal geçmişi) dikey olarak yumuşak şekilde kaydırılır.
- **📐 Daha Geniş Kenar Tutamaçları (12px Resize Grip):** Pencereleri kenarlarından tutup boyutlandırma alanı 8 pikselden **12 piksele** çıkarıldı.
- **🎨 Dinamik Tema Motoru (5 Tema):** Kadife Gece, Gün Işığı, Kutup Mavisi, Yeşil Terminal ve Gün Batımı temaları arasında tek tıkla canlı geçiş.
- **🪟 Akıllı Pencere Yönetimi:** Çift tıkla maksimize etme, ekranın en üstüne sürükleyerek otomatik maksimize (snap) etme, pencere ortalama, simge durumuna küçültme ve yumuşak z-sırası odak yönetimi.
- **📋 Not Defteri Açılır Menü Komutları:**
  - **Dosya:** Yeni Belge, Kaydet, Temizle
  - **Düzen:** Kes, Kopyala, Yapıştır, Tümünü Seç
  - **Biçim:** Piksel Font, Sıkıştırılmış Font, ClearType Font
  - **Yardım:** Not Defteri Hakkında

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
   - Windows için: `oscdimg.exe` (Windows ADK veya projedeki paketleyici).
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

## 💿 Kolay Kurulum ve Kullanım Adımları (Easy Setup Guide)

MinOS ISO'su açıldığında boot menüsünde sunulan seçenekler:

```
+-------------------------------------------------------------+
| 1. Boot Installed MinOS from Hard Disk (HDD/SSD) [Kuruluysa] |
| 2. MinOS Live CD (Graphical Desktop)         [Varsayılan]   |
| 3. MinOS Live CD - 1920x1080 (Full HD)                      |
| 4. MinOS Live CD - 1024x768                                 |
+-------------------------------------------------------------+
```

### 1. Canlı Masaüstü ile Başlatma (Live CD Modu - Sıfır Kurulum)
- ISO boot ettiğinde doğrudan **`MinOS Live CD (Graphical Desktop)`** seçilerek MinOS masaüstü ortamı açılır.
- Herhangi bir disk alanı veya ön kurulum gerektirmez, doğrudan RAM üzerinde çalışır.

### 2. Masaüstünden Kolay Kurulum (Masaüstü Sihirbazı)
MinOS'u sabit diske (HDD/SSD) kurmak için konsol veya Linux komutlarıyla uğraşmanıza gerek yoktur:
1. MinOS masaüstü açıldığında sol taraftaki hızlı erişim simgelerinden **`Diske Kur (Setup)`** simgesine tıklayın.
2. Açılan grafiksel **MinOS Diske Kurulum Sihirbazı** penceresinde **`Şimdi Kur`** butonuna tıklayın.
3. Sihirbaz hedef sabit diski (`/dev/sda` / 512MB+) otomatik hazırlar, MBR bölümünü açar ve MinOS dosyalarını yükler.
4. Kurulum tamamlandığında **`Yeniden Başlat`** butonuna tıklayın.

### 3. Kurulu Sistemden Başlatma (Hard Disk Boot)
- Kurulum tamamlandıktan sonra bilgisayarınızı açtığınızda boot menüsünün en başında yer alan **`Boot Installed MinOS from Hard Disk (HDD/SSD)`** seçeneğini seçerek işletim sistemini doğrudan sabit diskinizden başlatabilirsiniz.

---

### Adım Adım Manuel Derleme (Manual Steps)

#### 1. Çekirdeği Derleme (Kernel Build)
MinOS çekirdeği, özel hedef mimarisi ve sıfır bağımlılık bayraklarıyla derlenir:

```powershell
cargo build --release --target x86_64-minos.json
```

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

1. **Sanal Makine Türü:** Sanal makineyi oluştururken **İşletim Sistemi Türü** olarak **`Other / Other (64-bit)`** seçildiğinden emin olun.
2. **Sanal Donanım İvmelendirmesi (VT-x / AMD-V):**
   - Windows kullanıyorsanız ve VirtualBox 64-bit seçeneği çıkmıyorsa, **Hyper-V**'nin açık olmadığından ve BIOS'ta **Intel VT-x / AMD-V** donanım sanallaştırmasının aktif olduğundan emin olun.
   - VM Ayarları -> **Sistem** -> **Hızlandırma** sekmesinde **Sanallaştırma Arabirimi** varsayılan/KVM olarak ayarlanmalıdır.

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
- **Sistem Tepsisi (Sağ Alt):** Sırasıyla **klavye rozeti (`TR`/`EN`)**, **tema düğmesi**, **SES** düğmesi ve **canlı saat** bulunur. Saate tıklayınca canlı takvim, **SES**'e tıklayınca ses paneli açılır.
- **Tema Değiştirme:** Görev çubuğundaki renk örneğine, Sistem Ayarları > **Değiştir** düğmesine, **tema palet şeridindeki renk örneklerine** veya masaüstü sağ tık > **Tema Değiştir**'e tıklayın.
- **Ağ Komutları ve İnternet Gezgini:**
  - **Komut İstemi (CMD):** `ipconfig` (IP/MAC/Gateway bilgileri), `ping 10.0.2.2` (ICMP Echo testi), `dns <host>` (DNS sorgusu), `open browser` (CMD içinden tarayıcı başlatma).
  - **Web Gezgini:** `minos://net` (donanım/ağ tanı paneli), `http://<alan-adı>` (canlı HTML web belgesi ve HTTP GET isteği). F6 tuşu ile doğrudan adres çubuğuna odaklanma, F5 ile sayfayı yenileme.
- **Bağlama Duyarlı Sağ Tık:** Masaüstünde, Not Defteri'nde, Dosya Yöneticisi'nde veya pencere başlığında özel sağ tık menüleri açılır.

---

## 📂 Proje Dizin Yapısı

```
minos/
├── src/
│   ├── arch/x86_64/         # SSE, IDT kesmeleri ve PIC maskeleme
│   ├── platform/linux/      # Linux donanım köprüsü (DRM/fb0, evdev, sockets, ALSA)
│   ├── drivers/             # PS/2 fare, klavye, framebuffer video, RTC (CMOS) saat ve e1000/HDA donanım sürücüleri
│   ├── gui/                 # MinGUI pencere yöneticisi, font motoru, tema motoru (theme.rs) ve masaüstü kompozitörü
│   ├── apps/                # 11 Masaüstü uygulaması (Not Defteri, Dosya Yöneticisi, Görev Yöneticisi...)
│   ├── mm/                  # Fiziksel bellek yöneticisi (PMM)
│   ├── fs/                  # Sanal dosya sistemi (VFS / TarFS)
│   └── main.rs              # Çekirdek giriş noktası (_start) ve ana olay döngüsü
├── boot/                    # Limine bootloader dosyaları (limine.cfg) ve Linux çekirdeği (vmlinuz/initramfs)
├── docs/screenshots/        # Masaüstü ve başlat menüsü ekran görüntüleri
├── build_iso.ps1            # El Torito yamalı hibrit ISO oluşturucu
├── run.ps1                  # Tek tıkla derleme ve QEMU başlatma betiği
└── x86_64-minos.json        # Hedef x86_64 Long Mode tanımlama dosyası
```

---

## 📄 Lisans

Bu proje açık kaynaklıdır ve eğitim/araştırma amaçlı geliştirilmektedir.
Katkıda bulunmaktan ve geliştirmeler önermekten çekinmeyin! 🐾
