# 🐱 MinOS - Sarman Edition

[![Rust](https://img.shields.io/badge/Language-Rust%20Nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Display](https://img.shields.io/badge/Resolution-1280x720%2032--bit%20RGB-purple.svg)]()
[![GitHub Release](https://img.shields.io/github/v/release/tbagriyanik/minos?color=brightgreen)](https://github.com/tbagriyanik/minos/releases)

**MinOS (Sarman Edition)**, Rust dili ile sıfır dış bağımlılık (`no_std`, `no_main`) prensibiyle geliştirilmiş, modern grafik arayüzüne (GUI) ve zengin masaüstü uygulama ekosistemine sahip bağımsız bir 64-bit (`x86_64`) işletim sistemi çekirdeğidir.

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

MinOS, tam teşekküllü bir masaüstü deneyimi sunan 8 adet yerleşik uygulama ve pencere yöneticisi içerir:

| Uygulama | Açıklama |
| :--- | :--- |
| **🔘 Başlat Menüsü** | Görev çubuğundaki kedi ikonlu **MinOS** butonuyla açılır; tüm uygulamaları numaralı kısayollarla (`1-8`) listeler. |
| **📁 Dosya Yöneticisi** | Sanal dosya sistemi (VFS) gezgini, dosya türü simgeleri (`[LOG]`, `[TXT]`, `[IMG]`, `[SND]`, `[VID]`), boyutlar ve konum çubuğu. |
| **⚙️ Kontrol Paneli** | Sistem Bilgisi, Ekran & Ses kaydırıcıları, Ağ Durumu (IP/DNS/Gateway) ve Dil/Klavye yönetim paneli. |
| **📝 Not Defteri** | Doğrudan klavye ile yazı yazılabilen metin editörü. Türkçe karakter desteği (`ç`, `ğ`, `ı`, `ö`, `ş`, `ü`), `Backspace`, `Enter`, satır numaraları (01-09) ve yanıp sönen imleç. |
| **🌐 Web Tarayıcısı (SarmanNet)** | Sekmeler, URL çubuğu (`https://catnet.os/portal`), SSL rozeti ve stilize edilmiş kart görünümleri. |
| **🖼️ Resim Gösterici (MinViewer)** | Şeffaf dama tahtası tuvali üzerinde 4x yakınlaştırmalı 32-bit renkli piksel sanatı kedi maskotu ve dosya meta verileri. |
| **🎵 Müzik Çalar (SarmanBeats)** | Müzik ritmine göre dalgalanan dinamik 18 bantlı renkli spektrum ekolayzerı (yeşil/sarı/pembe), tepe ışıkları, parça scrubber'ı ve çalma listesi. |
| **🎬 Video Player (MinPlayer)** | Canlı synthwave ufuk sahnesi, neon güneş scanline'ları, aşağı doğru akan 3D perspektif grid yol ve siber araç simülasyonu. |
| **📊 Görev Yöneticisi** | Çekirdek bellek kullanımı, CPU durumu, görev tablosu ve çalışma süresi. |

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
  - Sarı `[-]` butonu: Pencereyi simge durumuna küçülterek görev çubuğuna indirir.
- **Görev Çubuğu:** Açık olan pencerelerin sekmeleri (`[Dosya]`, `[Not]`, `[Müzik]`, vb.) görev çubuğunda listelenir. Tıklayarak pencereleri öne getirebilir veya simge durumuna küçültebilirsiniz.
- **Klavye Düzeni Değiştirme:** Görev çubuğunun sağ altındaki `[TR]` / `[EN]` rozetine tıklayarak veya `Tab` tuşuna basarak Türkçe Q ve Amerikan US klavye düzenleri arasında geçiş yapabilirsiniz.
- **Hızlı Uygulama Başlatma:** Başlat menüsü açıkken klavyeden `1` ile `8` arasındaki sayılara basarak ilgili uygulamayı anında açabilirsiniz.

---

## 📂 Proje Dizin Yapısı

```
minos/
├── src/
│   ├── arch/x86_64/         # SSE, IDT kesmeleri ve PIC maskeleme
│   ├── drivers/             # PS/2 fare, klavye, framebuffer video ve donanım sürücüleri
│   ├── gui/                 # Pencere yöneticisi, font motoru ve masaüstü kompozitörü
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
