# Desteklenen Donanım Listesi

minOs çekirdeği bünyesindeki yerel sürücülerin ve donanım katmanlarının güncel durum tablosu.

> **Durum Göstergeleri:**
> - 🟢 **Tam Destek:** Günlük kullanımda kararlı, tüm temel ve gelişmiş işlevler eksiksiz devrede.
> - 🟡 **Kısmi Destek:** Temel işlevsellik ve sürücü devrede; bazı ileri seviye profiller/özellikler geliştirme aşamasında.
> - 🔴 **İskelet / WIP:** Aygıt algılama veya altyapı taslağı mevcut; tam protokol entegrasyonu planlama aşamasında.

---

## Grafik & Ekran

| Donanım / Aygıt | Sürücü Modülü | Durum | Kapsam & Yetenekler |
| :--- | :--- | :---: | :--- |
| Bochs BGA / QEMU Standard VGA | `gpu.rs` | 🟢 Tam | 1080p+, dinamik çözünürlük değişimi, LFB |
| VirtIO-GPU (QEMU/KVM) | `virtio_gpu.rs` | 🟢 Tam | Split VirtQueue, 2D komut seti, scanout, masaüstü varsayılan donanımsal 2D render & flush yolu, mode-set |
| NVIDIA GeForce GPU | `nvidia.rs` | 🟡 Kısmi | BAR0/BAR1 VRAM LFB, 2D çizim, mode-set (3D hızlandırma planlanıyor) |
| AMD Radeon GPU | `amdgpu.rs` | 🟡 Kısmi | BAR0 64-bit VRAM LFB, 2D çizim, mode-set (3D hızlandırma planlanıyor) |
| VMware SVGA-II | `gpu.rs` | 🟡 Kısmi | PCI algılama, temel çözünürlük okuma |

---

## Ağ & İletişim

| Donanım / Aygıt | Sürücü Modülü | Durum | Kapsam & Yetenekler |
| :--- | :--- | :---: | :--- |
| Intel e1000 / e1000e (Gigabit) | `e1000.rs` | 🟢 Tam | DMA ring, DHCP, DNS, TCP/UDP, IPv4/IPv6, ICMPv6, TLS 1.3 |
| Intel I219-V Ethernet | `i219.rs` | 🟢 Tam | Gigabit Ethernet bağlantısı ve paket aktarımı |
| Realtek PCIe (RTL8168/8169/8125) | `net.rs` | 🟢 Tam | C+ modu, 64-bit RX/TX halkaları, PHYSTATUS link takibi |
| Broadcom NetXtreme (BCM57xx) | `net.rs` | 🟢 Tam | MAC hash, RX/TX, PHY link kontrolü |
| VirtIO-net (QEMU/KVM/Cloud) | `virtio_net.rs` | 🟢 Tam | Yüksek hızlı sanallaştırılmış ağ adaptörü |
| Atheros Ethernet | `atheros.rs` | 🟡 Kısmi | Temel bağlantı ve paket transferi (offload filtreleri TODO) |
| Marvell Ethernet | `marvell.rs` | 🟡 Kısmi | Temel RX/TX paket işleme |
| Bluetooth HCI | `bluetooth.rs` | 🟡 Kısmi | Komut (0x01), ACL (0x02), Olay (0x04), Inquiry, bağlantı yönetimi (A2DP/GATT TODO) |
| Wi-Fi (802.11 Kablosuz) | `wifi.rs` | 🔴 İskelet | PCI/USB arayüz tespiti (WPA el sıkışması WIP) |
| VMware VMXNET3 | `e1000.rs` | 🟢 Tam | VMware PCI vendor `0x15AD`, MAC adresi, bağlantı yoklaması |

---

## Giriş Aygıtları

| Donanım / Aygıt | Sürücü Modülü | Durum | Kapsam & Yetenekler |
| :--- | :--- | :---: | :--- |
| PS/2 Klavye & Fare | `ps2.rs`, `keyboard.rs`, `mouse.rs` | 🟢 Tam | Kesme tabanlı girdi, Türkçe Q / İngilizce tuş eşleme |
| USB HID Klavye & Fare | `usb_hid.rs`, `usb_transfer.rs` | 🟢 Tam | xHCI/EHCI/UHCI/OHCI tam transfer katmanı, 16-bit eksen, tekerlek, hotplug |
| USB Dokunmatik Ekran | `usb_touchscreen.rs` | 🟡 Kısmi | Tek parmak dokunma/tıklama (çoklu dokunma analizi TODO) |

---

## Depolama

| Donanım / Aygıt | Sürücü Modülü | Durum | Kapsam & Yetenekler |
| :--- | :--- | :---: | :--- |
| ATA / IDE / ATAPI | `ata.rs` | 🟢 Tam | LBA28/48, sabit disk ve CD/DVD-ROM |
| AHCI / SATA | `ahci.rs` | 🟢 Tam | SATA SSD ve HDD blok I/O |
| NVMe SSD (PCIe M.2) | `nvme.rs` | 🟢 Tam | NVMe komut kuyrukları ve blok okuma/yazma |
| VirtIO-blk (QEMU/KVM) | `virtio_blk.rs` | 🟢 Tam | Sanal blok aygıtı entegrasyonu |
| USB Mass Storage (Flash Disk) | `usb_mass_storage.rs` | 🟢 Tam | SCSI BOT transfer protokolü ile depolama |
| SD / MMC Kart | `sdmmc.rs` | 🟡 Kısmi | Sektör okuma aktif (CSD ayrıştırma ve güvenilir yazma WIP) |

---

## Güç & Sensör

| Donanım / Aygıt | Sürücü Modülü | Durum | Kapsam & Yetenekler |
| :--- | :--- | :---: | :--- |
| ACPI AML Yorumlayıcısı & Pil | `acpi_battery.rs` | 🟢 Tam | DSDT/SSDT AML ayrıştırma, `_BST`/`_BIX`/`_BIF` canlı yüzde, voltaj, akım, döngü |
| ACPI Güç Yönetimi | `acpi_power.rs` | 🟢 Tam | `_S0`..`_S5` uyku durumları, PowerResource, dinamik AC/Pil durum izleme |
| Fan & Sıcaklık Sensörleri | `fan_temp.rs` | 🟡 Kısmi | ACPI ThermalZone ayrıştırma, Celsius dönüşümü, fan RPM izleme |
| Veri Yolu & Zamanlayıcı | `pci.rs`, `pcie.rs`, `timer.rs` | 🟢 Tam | PCI/PCIe konfigürasyonu, TSC/HPET/APIC zamanlayıcıları |

---

## Ses & Multimedya

| Donanım / Aygıt | Sürücü Modülü | Durum | Kapsam & Yetenekler |
| :--- | :--- | :---: | :--- |
| Intel High Definition Audio (HDA) | `hda.rs`, `audio.rs` | 🟢 Tam | DMA akış halkaları, mikser, WAV/MP3 oynatma |
| PC Speaker (PWM) | `audio.rs` | 🟢 Tam | Donanımsal zamanlayıcı bip ve PWM tonları |
| USB Audio | `usb_audio.rs` | 🟡 Kısmi | Hoparlör/kulaklık çıkışı (mikrofon kaydı TODO) |

---

## Dosya Sistemleri

| Dosya Sistemi | Destek Düzeyi | Açıklama |
| :--- | :--- | :--- |
| **FAT32** | Tam Okuma / Yazma / Biçimlendirme / Kalıcılık | MBR/GPT disklerde tam dosya okuma, yazma, oluşturma, silme |
| **Ext2** | Okuma / Yazma / Biçimlendirme | ATA disk bölümlerine mount etme, dizin listeleme, dosya okuma/yazma |
| **Ext4** | Okuma / Yazma / Biçimlendirme | 1024 baytlık blok, 256 baytlık inode, çoklu blok grubu (extents, journal, htree desteklenmez) |
| **exFAT** | Okuma / Yazma / Biçimlendirme | VBR, FAT, allocation bitmap, up-case tablosu, Unicode adlar |
| **ISO9660** | Salt Okunur | Canlı CD/DVD ve kurulum medyası içeriği okuma |

---

## ⚠️ Bilinen Sınırlamalar ve Spesifasyon Uyumluluğu

Aşağıdaki donanım spesifikasyonları minOs'da **kısmi veya eksik** olarak uygulanmıştır. Bu bölüm, şeffaflık ve planlama amacıyla bilinmeyen/eksik alanları belgelemektedir.

| Spesifikasyon | Durum | Açıklama |
| :--- | :---: | :--- |
| **ACPI AML Yorumlayıcısı** | 🟡 Kısım | Sadece pil/güç metodlarına özel opcode desteği (`_BST`, `_BIX`, `_BIF`, `_S0`–`_S5`, ThermalZone). Microsoft ACPICA'nın kapsadığı yüzlerce opcode'dan yalnızca bir alt kümesi desteklenir. Gerçek ACPI uyumluluğu (tüm AML opcode'ları, yöntemler, cihazlar, kapsamlar) sağlanmamıştır. |
| **USB 3.x SuperSpeed+ (10/20 Gbps)** | 🟡 Kısım | xHCI denetleyicisi mevcut ve temel USB 3.0/3.1 işlevleri çalışmaktadır, ancak USB 3.2 multi-lane (10/20 Gbps) protokol özellikleri kapsanmıyor mu belirsizdir. Tam USB 3.2 spesifikasyonu doğrulanmamıştır. |
| **PCIe AER (Advanced Error Reporting)** | 🔴 Yok | PCI-SIG'in hata toleransı ve gelişmiş hata raporlama standardı uygulanmamıştır. Kurumsal/sunucu donanımında beklenebilecek AER desteği yoktur. |

### Notlar

- Bu sınırlamalar, minOs'un hobi/ağaç odaklı bir işletim sistemi olduğu ve üretim/sunucu ortamı için tasarlanmadığını göstermektedir.
- ACPI AML yorumlayıcısı, laptop pil/güç yönetimi için yeterli düzeyde çalışmaktadır; genel amaçlı ACPI uyumluluğu hedeflenmemiştir.
- USB 3.x desteği, QEMU xHCI ve yaygın USB klavye/fare/masaüstü depolama aygıtları üzerinde test edilmiştir.
- PCIe AER, gelecek sürümlerde planlanabilir ancak şu anda öncelikli değildir.
