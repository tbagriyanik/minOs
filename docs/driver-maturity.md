# 📊 Sürücü Maturite Matrisi (Driver Maturity Matrix)

Bu belge, `src/drivers/` altındaki tüm sürücülerin uygulama durumunu ve maturite seviyesini listeler.

## 🏷️ Durum Göstergeleri

| Durum | Anlamı |
| :--- | :--- |
| 🟢 **Tam** | Gerçek donanım tespiti ve işlevsellik eksiksiz, production'da kullanılabilir |
| 🟡 **Kısmi** | Temel tespit ve API var, ama gerçek donanım entegrasyonu eksik veya sınırlı |
| 🔴 **İskelet / WIP** | Sadece sabitler, state machine ve API imzaları var; gerçek donanım tespiti yok |

---

## 📋 Sürücü Listesi

### Grafik & Ekran

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| Bochs BGA / QEMU VGA | `gpu.rs` | 🟢 Tam | Limine framebuffer, 1080p+, dinamik çözünürlük |
| VirtIO-GPU | `virtio_gpu.rs` | 🟢 Tam | Split VirtQueue, 2D komut seti, scanout |
| AMD Radeon | `amdgpu.rs` | 🟡 Kısmi | PCI tespit + 2D mode-set iskeleti, 3D yok |
| NVIDIA GeForce | `nvidia.rs` | 🟡 Kısmi | PCI tespit + 2D mode-set iskeleti, 3D yok |
| VMware SVGA-II | `gpu.rs` | 🟡 Kısmi | PCI algılama, temel çözünürlük okuma |

### Ağ & İletişim

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| Intel e1000/e1000e | `e1000.rs` | 🟢 Tam | DMA ring, DHCP, DNS, TCP/UDP, IPv4/IPv6, TLS 1.3 |
| Intel I219-V | `i219.rs` | 🟢 Tam | Gigabit Ethernet |
| Realtek PCIe | `net.rs` | 🟢 Tam | C+ modu, 64-bit RX/TX, PHYSTATUS |
| Broadcom NetXtreme | `net.rs` | 🟢 Tam | MAC hash, RX/TX, PHY link |
| VirtIO-net | `virtio_net.rs` | 🟢 Tam | Sanallaştırılmış ağ adaptörü |
| VMware VMXNET3 | `net.rs` | 🟢 Tam | PCI vendor 0x15AD, MAC ve link |
| Atheros Ethernet | `atheros.rs` | 🔴 İskelet | PCI vendor/device ID listesi, gerçek RX/TX yok |
| Marvell Ethernet | `marvell.rs` | 🔴 İskelet | PCI vendor/device ID listesi, gerçek RX/TX yok |
| Wi-Fi 802.11 | `wifi.rs` | 🔴 İskelet | 802.11 çerçeve yapıları, WPA2-PSK anahtar türetimi, gerçek PHY yok |
| Bluetooth HCI | `bluetooth.rs` | 🟡 Kısmi | HCI paketleme/ayrıştırma, synthetic yanıtlar, gerçek USB/PCI yok |

### Giriş Aygıtları

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| PS/2 Klavye & Fare | `ps2.rs`, `keyboard.rs`, `mouse.rs` | 🟢 Tam | Kesme tabanlı girdi, Türkçe Q / İngilizce |
| USB HID Klavye & Fare | `usb_hid.rs`, `usb_keyboard.rs`, `usb_mouse.rs` | 🟢 Tam | xHCI/EHCI/UHCI/OHCI, 16-bit eksen, tekerlek, hotplug |
| USB Dokunmatik Ekran | `usb_touchscreen.rs` | 🔴 İskelet | HID sabitleri ve state machine, gerçek Interrupt IN yok |

### Depolama

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| ATA / IDE / ATAPI | `ata.rs` | 🟢 Tam | LBA28/48, sabit disk ve CD/DVD-ROM |
| AHCI / SATA | `ahci.rs` | 🟢 Tam | SATA SSD ve HDD blok I/O |
| NVMe SSD | `nvme.rs` | 🟢 Tam | NVMe komut kuyrukları ve blok okuma/yazma |
| VirtIO-blk | `virtio_blk.rs` | 🟢 Tam | Sanal blok aygıtı |
| USB Mass Storage | `usb_mass_storage.rs` | 🟢 Tam | SCSI BOT transfer protokolü |
| SD / MMC | `sdmmc.rs` | 🟡 Kısmi | Sektör okuma aktif, CSD ayrıştırma ve yazma WIP |

### Ses & Multimedya

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| Intel HDA | `hda.rs`, `audio.rs` | 🟢 Tam | DMA akış halkaları, mikser, WAV/MP3 |
| PC Speaker (PWM) | `audio.rs` | 🟢 Tam | Donanımsal zamanlayıcı bip ve PWM tonları |
| USB Audio | `usb_audio.rs` | 🔴 İskelet | Audio Class sabitleri, state machine, gerçek USB yok |
| USB Yazıcı | `usb_printer.rs` | 🔴 İskelet | Printer Class sabitleri, state machine, gerçek USB yok |
| USB Kamera (UVC) | `camera.rs` | 🔴 İskelet | UVC sabitleri, YUY2→RGB32 dönüşüm, gerçek cihaz yok |

### Güç & Sensör

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| ACPI AML & Pil | `acpi_battery.rs` | 🟢 Tam | DSDT/SSDT AML, _BST/_BIX/_BIF |
| ACPI Güç Yönetimi | `acpi_power.rs` | 🟢 Tam | _S0_.._S5, PowerResource |
| Fan & Sıcaklık | `fan_temp.rs` | 🟡 Kısmi | ThermalZone ayrıştırma, fan RPM izleme |
| PCI / PCIe | `pci.rs`, `pcie.rs` | 🟢 Tam | Konfigürasyon altyapısı |
| Zamanlayıcı | `timer.rs` | 🟢 Tam | PIT/HPET/APIC/TSC |
| RTC | `rtc.rs` | 🟢 Tam | Gerçek zamanlı saat |

### Sistem

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| USB Çekirdek | `usb.rs`, `usb_transfer.rs` | 🟢 Tam | xHCI/EHCI/UHCI/OHCI transfer katmanı |
| I2C | `i2c.rs` | 🔴 İskelet | (henüz eklenmemiş / planlama aşamasında) |

---

## 📈 Özet

| Maturite | Sayı | Yüzde |
| :--- | :---: | :---: |
| 🟢 Tam | 22 | ~61% |
| 🟡 Kısmi | 7 | ~19% |
| 🔴 İskelet / WIP | 7 | ~19% |
| **Toplam** | **36** | **100%** |

---

## 🔧 İskelet Sürücüleri için Gelecek Adımlar

Aşağıdaki sürücüler **işkelet (skeleton)** durumundadır; gerçek donanım entegrasyonu için aşağıdaki işler yapılması gerekiyor:

1. **`atheros.rs`** — Gerçek MAC adresi okuma, RX/TX descriptor halkaları, interrupt handling
2. **`marvell.rs`** — Gerçek MAC adresi okuma, RX/TX descriptor halkaları, interrupt handling
3. **`wifi.rs`** — Gerçek 802.11 PHY tespiti, beacon/probe yanıtı, WPA 4-way handshake entegrasyonu
4. **`bluetooth.rs`** — Gerçek USB/PCI Bluetooth denetleyicisi tespiti, HCI komut gönderimi
5. **`usb_audio.rs`** — USB Audio Class interface enum'ası, isokronik endpoint tespiti, DMA akışı
6. **`usb_printer.rs`** — USB Bulk IN/OUT endpoint tespiti, IEEE 1284 Device ID, yazdırma akışı
7. **`usb_touchscreen.rs`** — USB HID Interrupt IN endpoint, Report Descriptor ayrıştırma, multitouch
8. **`camera.rs`** — Gerçek UVC cihaz tespiti, video akışı başlatma, kare yakalama
9. **`amdgpu.rs`** — 3D hızlandırma (Vulkan/Gallium), güç yönetimi
10. **`nvidia.rs`** — 3D hızlandırma (Vulkan/Gallium), güç yönetimi
