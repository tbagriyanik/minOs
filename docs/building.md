# Donanım ve Derleme Rehberi

Desteklenen donanım, sistem gereksinimleri ve çalıştırma adımları.

## ⚠️ Donanım Desteği ve Güncel Açılış Yapılandırması

- **Boot ekranı ve hata davranışı:** Limine framebuffer doğrulandıktan sonra `serial_print` kayıtları metin konsoluna da aktarılır. Framebuffer geçersizse ya da kritik bir açılış hatası oluşursa GUI açılmaz. Panik mesajı ve son boot logu ekranda bırakılır; otomatik reset yapılmaz.
- **CPU/APIC:** Mevcut laptop tanılama yapılandırması AP başlatımını ve APIC timer'ı atlar; sistem tek CPU modunda TSC saatiyle devam eder. `src/arch/x86_64/smp.rs` içinde Limine SMP/AP başlatma kodu mevcut olsa da varsayılan açılış akışında kullanılmıyor.
- **PS/2 fare/touchpad:** `src/drivers/ps2.rs` PS/2 8042 yardımcı portunu yoklayarak standart fare paketlerini okur. Dahili touchpad'in bu arayüzde sunulması firmware/ACPI ayarına bağlıdır; I2C-HID touchpad sürücüsü henüz bulunmuyor.
- **USB klavye/fare:** `src/drivers/usb_hid.rs`, xHCI denetleyicilerinde yerel HID Boot Protocol desteği sunar: aygıt numaralandırma, yapılandırma seçimi, kesme IN uç noktası ve rapor çözümleme uygulanmıştır. Klavye HID kullanım kimlikleri PS/2 set-1 tarama kodlarına, fare raporları ise mevcut `MouseState` paketlerine çevrilir. Sınırlar: yalnızca HID sınıfı/alt sınıf 1 (Boot Interface) ve boot protokolü desteklenir; EHCI/UHCI/OHCI için denetleyici başlatma/handoff ve transfer katmanı (`src/drivers/usb.rs`) mevcuttur ancak bu denetleyicilerde HID rapor ayrıştırma henüz yoktur — firmware'in USB Legacy/8042 emülasyonu yedek yol olarak kullanılır. `src/drivers/usb.rs` ayrıca PCI denetleyici tanısı ve legacy capability raporlamasını sürdürür.
- **USB depolama:** USB depolama desteği, mevcut ATA/ATAPI ve diskfs yolları üzerinden sunulur; bu, USB HID giriş desteğinden ayrıdır.
- **SMP ve Ring 3:** Kod tabanında SMP ve Ring 3 bileşenleri/testleri vardır; fiziksel laptop açılışında kararlılık için Ring3 demosu ve AP başlatımı varsayılan boot yolunda çalıştırılmaz. Bileşenlerin bulunması, her donanımda açılışta etkin oldukları anlamına gelmez.

### Son değişikliklerin kaynak referansları

| Özellik | Kaynak konumu |
| :--- | :--- |
| Erken framebuffer doğrulaması ve tek CPU açılış kararı | [`src/main.rs:102`](../src/main.rs#L102), [`src/main.rs:286`](../src/main.rs#L286) |
| PS/2 fare/touchpad başlatma ve paket yoklama | [`src/drivers/ps2.rs:73`](../src/drivers/ps2.rs#L73), [`src/drivers/ps2.rs:163`](../src/drivers/ps2.rs#L163) |
| USB controller ve legacy capability tanılama | [`src/drivers/usb.rs:280`](../src/drivers/usb.rs#L280), [`src/drivers/usb.rs:336`](../src/drivers/usb.rs#L336) |
| Yerel xHCI USB HID klavye/fare sürücüsü | [`src/drivers/usb_hid.rs:1`](../src/drivers/usb_hid.rs#L1), [`src/drivers/usb_hid.rs:1731`](../src/drivers/usb_hid.rs#L1731) |
| Boot log metin çizimi, ayna font ve fatal stop | [`src/boot/text_console.rs:197`](../src/boot/text_console.rs#L197), [`src/boot/text_console.rs:269`](../src/boot/text_console.rs#L269), [`src/boot/text_console.rs:335`](../src/boot/text_console.rs#L335) |
| Panic ekranında otomatik resetin engellenmesi | [`src/panic_handler.rs:51`](../src/panic_handler.rs#L51) |


---


## 🖥️ Minimum Donanım Gereksinimleri (Minimum Hardware Requirements)

| Bileşen | Minimum | Önerilen |
| :--- | :--- | :--- |
| **CPU** | x86_64 (64-bit), SSE2 desteği | x86_64, SSE4.1+ |
| **RAM** | **4 MB** | 32 MB – 64 MB |
| **Depolama** | Yalnızca CD/ISO (RAM tabanlı) | 64 MB+ qcow2 disk |
| **Ekran** | 1024×768 32-bit framebuffer | 1024×768 veya daha yüksek |
| **Ses** | — (isteğe bağlı) | HDA uyumlu |
| **Ağ** | — (isteğe bağlı) | Realtek PCIe (RTL8168/8169) / Intel e1000 / VMware VMXNET3 |

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
| `-NoIso` | ISO dosyasını bağlamaz (`-cdrom` argümanını iptal eder); sistemi doğrudan sabit diskten (`-boot c`) başlatır. |
| `-Live` | Canlı ortamı zorlar; sabit disk kurulu olsa bile ISO üzerinden başlatır (`-boot d`). |
| `-ReprepareDisk` | Sabit diski yeniden biçimlendirip MBR ve VFS ağacını sıfırdan kurar (`build_disk.ps1 -Force`). |
| `-DiskImage dosya.qcow2` | Bağlanacak sanal disk imajının yolu (varsayılan `.\disk.qcow2`). |
| `-UsbDisks` | Sistemdeki USB diskleri **salt okunur** olarak QEMU'ya `usb-storage` aygıtı olarak bağlar. |
| `-UsbDiskReadWrite` | USB diskleri **yazma izniyle** bağlar (dikkatli kullanın). |
| `-UsbHid` | Yerel xHCI HID sürücüsünü test etmek için QEMU'ya `qemu-xhci` denetleyicisi ile `usb-kbd` ve `usb-mouse` ekler. |
| `-NoPs2` | PS/2 (i8042) denetleyicisini tamamen kapatır (`-machine pc,i8042=off`) ve `-UsbHid`'i otomatik açar. Girdi yalnızca USB klavye/fare üzerinden gelir; USB HID sürücüsünü PS/2 yedeği olmadan test etmek için kullanın. |
| `-Display <sdl\|gtk\|default>` | QEMU grafik arayüz arka ucunu belirler. `-Display sdl` seçildiğinde Windows GTK pixbuf/tema uyarıları engellenir ve daha düşük fare gecikmesi elde edilir. |
| `-CpuCount N` | QEMU'ya verilecek sanal CPU sayısı (varsayılan `2`, aralık `1–64`, çekirdek üst sınırı 8). |
| `-Memory 1G` | Konuk RAM miktarı (`512M`, `1G` gibi; varsayılan `512M`). |

---

### 💽 ISO Olmadan Sabit Diskten (QCOW2 / RAW) Boot Etme

minOs, canlı ISO olmadan doğrudan kurulu bir sabit disk üzerinden açılabilir.

#### 1. Doğrudan Sabit Diskten Başlatma:
```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1 -NoIso -DiskImage .\disk.qcow2
```

#### 2. Akıllı Boot Doğrulaması:
- Eğer `-NoIso` ile başlatırsanız veya ISO ortamı yoksa, `run.ps1` diskin önyüklenebilirliğini otomatik denetler:
  - **Disk Boş / Formatsız ise:** QEMU açılmaz; kullanıcıya güvenli bir hata mesajı verilir:
    ```text
    [HATA] Disk formatli/onyuklenebilir degil (.\disk.qcow2). Sistem boot edilemez!
    [IPUCU] Once ISO ile baslatip 'Kur' uygulamasini calistirin veya build_disk.ps1 ile diski hazirlayin.
    ```
  - **Disk Formatlı ve MBR Önyükleyicisi Kurulu ise:** Doğrudan sabit diskten (`-boot c`) açılır.

#### 3. Bağımsız Disk Hazırlama Aşamaları:
Bir diski ISO olmadan boot edilebilir hale getirmek için iki yöntem vardır:

- **Yöntem A (minOs İçinden - Kurulum Sihirbazı):**
  1. `.\run.ps1` ile ISO canlı ortamında başlatın.
  2. Masaüstündeki **"Kur" (Kurulum Sihirbazı)** uygulamasını açın.
  3. Hedef diski seçip **"Tek Tıkla Kur"** düğmesine basın.
  4. minOs çekirdeği diske otomatik olarak:
     - 1 MiB hizalı MBR bölüm tablosu (`LBA 2048`, `0x80 Active`, `FAT32 LBA`),
     - Limine MBR Stage 1 & Stage 2 önyükleyici kodlarını,
     - `/boot/limine/limine.conf`, `limine-bios.sys`, `minos-kernel` ve sistem dosyalarını yazar.
  5. Makineyi kapatıp `.\run.ps1 -NoIso` ile ISO'suz doğrudan açabilirsiniz.

- **Yöntem B (Windows PowerShell ile Hazırlama):**
  ```powershell
  # 256 MB boyutunda önyüklenebilir disk oluşturur ve Limine BIOS'u kurar:
  powershell -ExecutionPolicy Bypass -File .\build_disk.ps1 -DiskImage .\disk.qcow2 -Force
  ```

---

### Adım Adım Manuel Derleme (Manual Steps)

#### 1. Çekirdeği Derleme (Kernel Build)
minOs çekirdeği, özel hedef mimarisi ve `-Zbuild-std` ile gereken çekirdek kütüphanelerini kaynak kodundan derleyerek oluşturulur:

```powershell
cargo build --release -Zjson-target-spec --target x86_64-minos.json `
  -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
```

#### 2. Birim ve Regresyon Testlerini Çalıştırma (Host Unit Tests)
Kriptografi (AES, GCM, RSA, ECC, ECDSA, SHA-256, SHA-384, SHA-512), X.509 sertifika, zincir derinliği ve geçerlilik süresi/hostname doğrulama, TLS 1.2 / TLS 1.3 el sıkışması, JavaScript motoru, HTML ayrıştırıcı ve GUI metin tamponu (`TextBuf`) testlerini içeren 146 adet birim testini host ortamında yürütmek için:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

#### 3. Boot Edilebilir ISO İmajını Paketleme (ISO Creation)
Limine bootloader dosyalarını çekirdekle birleştirmek ve El Torito tablosunu yamalamak için:

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```
> *İşlem tamamlandığında kök dizinde `minos.iso` dosyası oluşturulur.*

#### 4. QEMU ile Test Etme (Run in QEMU)

##### Linux (PulseAudio + Intel HDA Duplex + e1000):
`ash
qemu-system-x86_64 -cdrom minos.iso -m 512M -smp 2 -vga std -global VGA.vgamem_mb=64 \
  -audiodev pa,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \
  -netdev user,id=n0 -device e1000,netdev=n0
`

##### Windows (SDL veya DirectSound + Intel HDA Output + e1000):
`powershell
& 'C:\Program Files\qemu\qemu-system-x86_64.exe' -cdrom minos.iso -m 512M -smp 2 -vga std -global VGA.vgamem_mb=64 
  -audiodev driver=sdl,id=snd0 -device intel-hda -device hda-output,audiodev=snd0 
  -netdev user,id=n0 -device e1000,netdev=n0
`

#### 5. VirtualBox Kurulumu & Temel Ayarlar (VirtualBox Setup)

VirtualBox üzerinde minOs çalıştırmak için gereken temel ve önerilen sanal makine yapılandırması:

| Ayar Kategorisi | Önerilen / Minimum Değer | Açıklama |
| :--- | :--- | :--- |
| **İşletim Sistemi Türü** | **Other / Other (64-bit)** | Çekirdek x86_64 long modda çalıştığı için mutlaka 64-bit seçilmelidir. |
| **Temel Bellek (RAM)** | **128 MB – 512 MB** *(Min: 4 MB)* | Canlı masaüstü, font atlasları ve çoklu pencere için 256 MB veya 512 MB idealdir. |
| **İşlemci (CPU)** | **1 vCPU önerilir** | Güncel fiziksel laptop açılışında AP/SMP ve APIC timer devre dışıdır; TSC tabanlı saat kullanılır. SMP kodu kaynakta bulunsa da bu boot profilinde etkin değildir. |
| **Ekran Belleği (VRAM)** | **16 MB – 32 MB** | 1280x720 32-bit çözünürlük için en az 16 MB ayrılmalıdır. |
| **Grafik Denetleyicisi** | **VBoxVGA** veya **VMSVGA** | 3D hızlandırmayı kapatın (yazılımsal framebuffer kullanılır). |
| **Sabit Disk (İsteğe Bağlı)** | **256 MB – 1 GB** *(VDI / RAW)* | Canlı ISO testi için disk gerekmez. Kalıcı kurulum için SATA veya IDE olarak ekleyin. |
| **Ses Denetleyicisi** | **Intel HD Audio** | minOs HDA ses çıkış codec'i ile tam uyumludur. |
| **Ağ Bağdaştırıcısı** | **Intel PRO/1000 MT Desktop (82540EM)** | e1000 sürücüsüyle DHCP, ping ve HTTP desteği sağlar. |

> **⚠️ Önemli VirtualBox Sorun Giderme Notları:**
> 1. **`PANIC: limine: This CPU does not support 64-bit mode`:** Sanal makine türünün `Other (64-bit)` olduğundan ve BIOS'ta işlemci sanallaştırmasının (VT-x / AMD-V) açık olduğundan emin olun.
> 2. **Siyah Ekran Hatası:** Sanal makine ayarları -> **Ekran** -> **Grafik Denetleyicisi**'ni **VBoxVGA** olarak ayarlayın ve **3B Hızlandırmayı** devre dışı bırakın. VRAM en az 16 MB olmalı.
> 3. **UEFI / EFI:** `build_iso.ps1`, Limine UEFI El Torito imajı mevcutsa BIOS + UEFI hibrit ISO üretir. VirtualBox'ta **Sistem -> EFI'yi Etkinleştir** seçeneğini açın. Komut satırıyla: `VBoxManage modifyvm "minOs" --firmware efi`. UEFI açılmazsa ISO'yu `oscdimg`, `xorriso` veya `mkisofs` araçlarından biriyle yeniden oluşturun; yerel IMAPI2FS fallback'i yalnızca Legacy BIOS ISO üretir.
> 4. **Çözünürlük:** Varsayılan açılış çözünürlüğü evrensel VESA/GOP modu **1024x768x32**'dir. `1280x720` / `1920x1080`, VirtualBox'un standart mod listesinde yoktur; bu modlar için ya Limine menüsündeki `Display Resolutions` girdisini kullanın ya da `VBoxManage setextradata <vm> "CustomVideoMode1" "1280x720x32"` ile özel mod ekleyin.
> 5. **ISO Olmadan Başlatma:** Diske kurulum yaptıysanız (veya `build_disk.ps1` ile hazırladıysanız), CD/DVD sürücüsünden `minos.iso` dosyasını çıkarın. Sistem doğrudan MBR üzerinden açılacaktır.

---

