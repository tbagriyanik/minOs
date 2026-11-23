# Kurulum ve Çalıştırma

minOs'u derlemek, ISO oluşturmak ve QEMU veya gerçek donanımda çalıştırmak için gereken tüm adımlar.

---

## Ön Koşullar

| Araç | Açıklama |
| :--- | :--- |
| **Rust Nightly** | `rustup default nightly` ve `rustup component add rust-src` |
| **QEMU x86_64** | [QEMU Resmi İndirme](https://www.qemu.org/download/#windows) |
| **ISO Oluşturma** | Windows: `oscdimg.exe`; Linux/macOS: `xorriso` veya `mkisofs` |
| **PowerShell 5.1+** | Derleme ve ISO paketleme otomasyonu için |

---

## Tek Komutla Derle ve Başlat (Önerilen)

```powershell
git clone https://github.com/tbagriyanik/minos.git
cd minos
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

### `run.ps1` Seçenekleri

| Anahtar | Açıklama |
| :--- | :--- |
| `-BuildOnly` | Yalnızca derler; QEMU başlatılmaz |
| `-Debug` | Debug profilinde derler (varsayılan release) |
| `-NoIso` | ISO bağlamaz; doğrudan sabit diskten başlatır |
| `-Live` | Canlı ortamı zorlar (`-boot d`) |
| `-ReprepareDisk` | Sabit diski yeniden biçimlendirir |
| `-DiskImage dosya.qcow2` | Sanal disk imajı yolu (varsayılan `.\disk.qcow2`) |
| `-UsbDisks` | USB diskleri salt okunur olarak bağlar |
| `-UsbDiskReadWrite` | USB diskleri yazma izniyle bağlar |
| `-UsbHid` | QEMU'ya `usb-kbd` ve `usb-tablet` ekler |
| `-NoPs2` | PS/2 denetleyicisini kapatır, `-UsbHid` otomatik açar |
| `-Display <sdl\|gtk\|default>` | QEMU grafik arka ucu |
| `-CpuCount N` | Sanal CPU sayısı (varsayılan 2, max 8) |
| `-Memory 1G` | Konuk RAM miktarı (varsayılan 512M) |

---

## Sabit Diskten Boot Etme (QCOW2 / RAW)

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1 -NoIso -DiskImage .\disk.qcow2
```

Disk boş veya formatsızsa QEMU açılmaz ve güvenli hata mesajı gösterilir. Önce ISO ile başlatıp "Kur" uygulamasını çalıştırın veya `build_disk.ps1` ile diski hazırlayın.

### Disk Hazırlama (Yöntem B - PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -File .\build_disk.ps1 -DiskImage .\disk.qcow2 -Force
```

---

## Manuel Derleme Adımları

### 1. Çekirdeği Derle

```powershell
cargo build --release -Zjson-target-spec --target x86_64-minos.json `
  -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
```

### 2. Birim Testlerini Çalıştır

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

### 3. ISO İmajını Oluştur

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```

Kökte `minos.iso` dosyası oluşturulur.

### 4. QEMU ile Çalıştır

> **Ses notu:** Sürücü yalnızca **Intel HDA** (codec NID 1/2/3) destekler.
> QEMU'da `-device intel-hda` tek başına **codec'sizdir** ve ses üretmez;
> HDA codec'i ayrıca eklenmelidir ve sıra önemlidir (`intel-hda` önce).
> Windows'ta `wasapi`/`pa` backend'i yoktur; `sdl` veya `dsound` kullanılır.
> `test_qemu.ps1` ise sesi `driver=wav` ile **dosyaya** yazar (host'tan
> duyulmaz).

**Linux (PulseAudio + Intel HDA + e1000):**
```bash
qemu-system-x86_64 -cdrom minos.iso -m 512M -smp 2 -vga std -global VGA.vgamem_mb=64 \
  -audiodev pa,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \
  -netdev user,id=n0 -device e1000,netdev=n0
```

**Windows (SDL + Intel HDA Output + e1000):**
```powershell
qemu-system-x86_64.exe -cdrom .\minos.iso -m 512M -smp 2 -vga std -global VGA.vgamem_mb=64 `
  -audiodev driver=sdl,id=snd0 -device intel-hda -device hda-output,audiodev=snd0 `
  -netdev user,id=n0 -device e1000,netdev=n0
```

### 5. Açılış Profilleri (Limine menüsü)

| Menü girdisi | Komut satırı | Davranış |
| :--- | :--- | :--- |
| minOs Live Environment | `live full` | Tüm sürücüler + kendi kendine test bataryası |
| **minOs Fast Boot** | `live full fast` | Tüm sürücüler, **test bataryası atlanır** (en hızlı açılış) |
| minOs Safe Mode | `live safe` | Donanım sürücüleri yok |
| minOs Audio Test | `live audio` | Yalnızca HDA ses |
| minOs Hardware Test | `live hardware` | USB + ağ (ses yok) |

> `fast` profili, açılışta çalışan bellek/süreç/Ring3 test bataryasını atlar;
> sanal makinede ve yavaş disklerde açılış süresini belirgin şekilde kısaltır.
> Hangi aşamanın ne kadar sürdüğünü seri konsoldaki `[BOOT-TIME]` satırları
> gösterir.

---

## VirtualBox Kurulumu

| Ayar | Önerilen Değer |
| :--- | :--- |
| İşletim Sistemi | Other / Other (64-bit) |
| Bellek | 128 MB – 512 MB |
| İşlemci | 1 vCPU |
| Ekran Belleği | 16 MB – 32 MB |
| Grafik Denetleyicisi | VBoxVGA veya VMSVGA (3D kapalı) |
| Ağ Bağdaştırıcısı | Intel PRO/1000 MT Desktop (82540EM) |
| Ses Denetleyicisi | Intel HD Audio |

> **Sorun Giderme:**
> - `PANIC: limine: This CPU does not support 64-bit mode`: Sanal makine türünün `Other (64-bit)` olduğundan emin olun.
> - Siyah ekran: Grafik denetleyicisini `VBoxVGA` yapın, 3D hızlandırmayı kapatın, VRAM en az 16 MB olmalı.
> - UEFI: `VBoxManage modifyvm "minOs" --firmware efi`.
> - **Ekran yırtılması:** VirtualBox konuk framebuffer'ını kendi taramasıyla
>   ekrana kopyalar; yazılım, vblank'e kilitlenmediği için kare ortasında
>   yazma yapabilir. minOs kare başına tek yazma yapar ve vblank'i destekleyen
>   donanımda yazma öncesi bekler. Yine de yırtılma fark ederseniz çözünürlüğü
>   1024×768'e sabitleyin (kopya süresi kısalır) ve VirtualBox 7.0+ ile
>   **VirtIO-GPU** deneyin (host tarafı atomik kopyalar).

---

## Gerçek Donanım Sorun Giderme

| Belirti | Olası Neden | Yapılacak |
| :--- | :--- | :--- |
| Ağ kartı görünmüyor | Kart, desteklenmeyen bir aile (Marvell/Atheros) veya firmware BAR atamamış | Seri logda `[NET] Desteklenmeyen ag aygiti: PCI ...` satırına bakın; Intel I219/I226 ve Realtek RTL8168 artık `e1000` sürücüsüyle sürülür |
| USB fare görünmüyor | BIOS'ta Legacy USB kapalı ve xHCI'nin USB2 portu yok | BIOS'ta "Legacy USB Support"u açın veya yalnızca USB3 Type-C portlu bir dizüstünde HID çalışmaz |
| Fare yavaş / takılıyor | iki fare kaynağı (USB HID + PS/2 8042 emülasyonu) aynı anda işleniyordu | Artık BIOS legacy devri tamamlanamazsa PS/2 fare yolu otomatik kapatılır; fare hızını Ayarlar'dan yükseltebilirsiniz |
| İlk açılış çok yavaş | ISO'nun diskten ilk okunuşu + her boot'ta test bataryası | Limine menüsünden **minOs Fast Boot** girdisini seçin (`live full fast`) |

---

## Minimum Donanım Gereksinimleri

| Bileşen | Minimum | Önerilen |
| :--- | :--- | :--- |
| CPU | x86_64, SSE2 | x86_64, SSE4.1+ |
| RAM | 4 MB | 32 MB – 64 MB |
| Depolama | Yalnızca CD/ISO | 64 MB+ qcow2 disk |
| Ekran | 1024×768 32-bit | 1024×768 veya daha yüksek |
| Ses | — (isteğe bağlı) | HDA uyumlu |
| Ağ | — (isteğe bağlı) | Realtek PCIe / Intel e1000 / VMware VMXNET3 |
