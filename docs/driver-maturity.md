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
| Realtek RTL8139 | `e1000.rs` | 🟢 Tam | I/O portu, C+ modu, 4 TX kanalı, 8 KiB RX halkası |
| NE2000 / RTL8019AS (ISA PnP) | `ne2000.rs` | 🟢 Tam | ISA PnP, uzak DMA, 8K RX halkası |
| NE2000 uyumlu PCI (RTL8029AS) | `ne2000.rs` | 🟢 Tam | PCI I/O BAR, uzak DMA |
| Realtek PCIe | `e1000.rs` | 🟢 Tam | C+ modu, 64-bit RX/TX, PHYSTATUS |
| Broadcom NetXtreme | `e1000.rs` | 🟢 Tam | MAC hash, RX/TX, PHY link |
| VirtIO-net | `virtio_net.rs` | 🟢 Tam | Sanallaştırılmış ağ adaptörü |
| VMware VMXNET3 | `e1000.rs` | 🟢 Tam | PCI vendor 0x15AD, MAC ve link |
| Ağ otobüsü seçimi (PCI / ISA) | `net.rs` | 🟢 Tam | `BusKind`, ISA PnP izolasyonu, NE2000 DMA veri portu |
| Atheros Ethernet | `atheros.rs` | 🟡 Kısmi | PCI tespit + 2D mode-set iskeleti, RX/TX basic; checksum offload ve filtreler TODO |
| Marvell Ethernet | `marvell.rs` | 🟡 Kısmi | PCI tespit + 2D mode-set iskeleti, RX/TX basic; checksum offload ve filtreler TODO |
| Wi-Fi 802.11 | `wifi.rs` | 🟡 Kısmi | 802.11 çerçeve yapıları, WPA2-PSK PBKDF2-HMAC-SHA1 PMK/PTK türetimi, WPA 4-way el sıkışması, PCI/USB arayüz tespiti; gerçek PHY/RX-TX yok |
| Bluetooth HCI | `bluetooth.rs` | 🟡 Kısmi | HCI paketleme/ayrıştırma, synthetic yanıtlar, gerçek USB/PCI yok; A2DP/BLE GATT yok |

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
| SD / MMC | `sdmmc.rs` | 🟡 Kısmi | Sektör okuma aktif, CSD ayrıştırma ve yazma WIP; yazma yolu güvenilir değil |

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
| Fan & Sıcaklık | `fan_temp.rs` | 🟡 Kısmi | ACPI ThermalZone okuma, fan RPM izleme, EC (0x66/0x62) fan profilleri (Silent/Normal/Performance/Full/Auto) ve sıcaklığa göre otomatik geçiş; EC profilleri varsayımsal/örnek ofsetler, gerçek EC kayıt ofsetleri placa özel |
| PCI / PCIe | `pci.rs`, `pcie.rs` | 🟢 Tam | Konfigürasyon altyapısı |
| Zamanlayıcı | `timer.rs` | 🟢 Tam | PIT/HPET/APIC/TSC |
| RTC | `rtc.rs` | 🟢 Tam | Gerçek zamanlı saat |

### Sistem

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| USB Çekirdek | `usb.rs`, `usb_transfer.rs` | 🟢 Tam | xHCI/EHCI/UHCI/OHCI transfer katmanı |
| I2C | `i2c.rs` | 🔴 İskelet | (henüz eklenmemiş / planlama aşamasında) |
| Preemptive Scheduler | `task/mod.rs` | 🟢 Tam | Preemptive kernel task scheduler, APIC timer 1000 Hz tick, per-CPU run queue, work-stealing, callback-safe boundaries |

### Güvenlik

| Sürücü | Dosya | Durum | Açıklama |
| :--- | :--- | :---: | :--- |
| CRL/OCSP İndirme | `drivers/crypto/revocation_fetch.rs` | 🟡 Kısmi | CRL/OCSP HTTP GET indirme iskeleti; OCSP imza doğrulaması yok, HTTPS indirme TLS'ten geçmiyor, otomatik indirme sınırlı |

---

## 📈 Özet

| Maturite | Sayı | Yüzde |
| :--- | :---: | :---: |
| 🟢 Tam | 27 | ~69% |
| 🟡 Kısmi | 9 | ~23% | (Atheros ve Marvell Ethernet güncellendi) |
| 🔴 İskelet / WIP | 3 | ~8% | |
| **Toplam** | **39** | **100%** |

---

## 🔧 İskelet Sürücüleri için Gelecek Adımlar

Aşağıdaki sürücüler **işkelet (skeleton)** durumundadır; gerçek donanım entegrasyonu için aşağıdaki işler yapılması gerekiyor:

1. **`wifi.rs`** — Gerçek 802.11 PHY/RX-TX entegrasyonu, beacon/probe yanıtı, kesme işleme ve donanımdan MAC adresi okuma (WPA 4-way el sıkışması ve PCI/USB tespiti eklendi)
4. **`bluetooth.rs`** — Gerçek USB/PCI Bluetooth denetleyicisi tespiti, HCI komut gönderimi
5. **`usb_audio.rs`** — USB Audio Class interface enum'ası, isokronik endpoint tespiti, DMA akışı
6. **`usb_printer.rs`** — USB Bulk IN/OUT endpoint tespiti, IEEE 1284 Device ID, yazdırma akışı
7. **`usb_touchscreen.rs`** — USB HID Interrupt IN endpoint, Report Descriptor ayrıştırma, multitouch
8. **`camera.rs`** — Gerçek UVC cihaz tespiti, video akışı başlatma, kare yakalama
9. **`amdgpu.rs`** — 3D hızlandırma (Vulkan/Gallium), güç yönetimi
10. **`nvidia.rs`** — 3D hızlandırma (Vulkan/Gallium), güç yönetimi
