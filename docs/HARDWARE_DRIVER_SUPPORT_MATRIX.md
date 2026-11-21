# 🖥️ Hardware & Driver Support Matrix

Türkçe donanım ve sürücü uyumluluk tablosu.

Bu belge, minOs'un **QEMU emülasyonunda** ve **gerçek donanımda** desteklenen/desteklenen
olmayan bileşenleri özetler. Sürücü olgunluk seviyeleri için
[`driver-maturity.md`](./driver-maturity.md) dosyasına bakın.

## 🏷️ Durum Göstergeleri

| Durum | Anlamı |
| :--- | :--- |
| ✅ **Doğrulandı** | QEMU profilinde otomatik testle doğrulandı (`test_qemu.ps1`) |
| 🟢 **Destekleniyor** | Sürücü tam; gerçek donanımda çalışması beklenir (manuel doğrulama bekliyor) |
| 🟡 **Kısmi** | Temel destek var; bazı özellikler eksik veya donanıma bağlı |
| 🔴 **Deneysel / Yok** | İskelet sürücü veya destek yok |

> Not: ✅ yalnızca **otomatik QEMU testi** anlamına gelir. Gerçek donanım
> sonuçları için aşağıdaki **Gerçek Donanım Doğrulama** bölümünü doldurun.

## 📌 Son QEMU Doğrulaması

Kaynak: [`../test-report.md`](../test-report.md) — `test_qemu.ps1` tarafından
üretilen son rapor. Bu tablo **yalnızca o raporda gerçekten çalışan** kontrolleri
listeler; profil dışı sonuçlar taşınmaz.

| Öğe | Sonuç |
| :--- | :--- |
| Tarih / profil | 2026-11-17 20:05:39 UTC · `default` |
| Sanal donanım | 512 MB RAM · 2 vCPU · virtio VGA · USB HID kapalı · PS/2 açık |
| Toplam | 36/36 QEMU kontrolü ✅ GEÇTİ |
| CPU & Çekirdek Donanım Başlatımı | ✅ GEÇTİ (BSP GDT/IDT, tek CPU yolu) |
| Gerçek ses çıkışı (Intel HDA / WAV) | ✅ GEÇTİ (44 bayt WAV başlığı; 0 bayt PCM) |
| `brk`/`mmap`/`munmap`, FD yaşam döngüsü, kaynak limitleri, COW | ✅ GEÇTİ |
| GUI duman testi (`[GUI-SMOKE]`), `[APP-MEM]` seri raporu | ✅ GEÇTİ |

> ⚠️ **Profil bazlı uyarı:** 2026-11-17 19:35 UTC civarındaki bir `high`
> profili denemesinde `CPU & Cekirdek Donanım Başlatimi` ❌ **BAŞARISIZ** oldu.
> 20:05 UTC `default` profil raporunda aynı kontrol ✅ GEÇTİ. Bu iki sonuç aynı
> donanım profiline ait değildir; `high` profili sonucu ayrıca doğrulanmadan
> genel destek iddiası yapılmaz.

---

## 🧪 QEMU Test Profilleri

`test_qemu.ps1` üç profil kullanır. Tüm profiller tek çekirdek (BSP) açılır;
ağ ve ses aygıtları yalnızca `minimal` dışındaki profillerde eklenir.

| Profil | RAM | vCPU | VGA | Ağ | Ses | USB HID | Amaç |
| :--- | :---: | :---: | :--- | :---: | :---: | :---: | :--- |
| `minimal` | 128 MB | 1 | std | — | — | — | En düşük donanım, boot kararlılığı |
| `default` | 512 MB | 2 | virtio/std | e1000 (user-net) | Intel HDA → WAV | — | Dengeli doğrulama |
| `high` | 8 GB | 1 | std | e1000 (user-net) | Intel HDA → WAV | qemu-xhci + usb-kbd + usb-tablet | Yüksek donanım, USB HID yolu |

---

## 🔌 Bileşen Uyumluluk Matrisi

### CPU / Çekirdek

| Bileşen | Gereksinim | Durum | Not |
| :--- | :--- | :---: | :--- |
| Mimari | x86_64 (Long Mode) | 🟢 | BIOS/UEFI, Limine önyükleyici |
| Zamanlayıcı | TSC (ana), PIT/HPET/APIC | 🟢 | SMP/APIC bazı dizüstelerde devre dışı |
| Çok çekirdek (SMP) | APIC + AP'ler | 🟡 | Bazı gerçek donanımlarda reset riski; BSP'de açılır |
| ACPI | DSDT/SSDT, güç, pil | 🟢 | `_BST/_BIX/_BIF`, `_S0.._S5` |

### Ekran / GPU

| Bileşen | Sürücü | Durum | Not |
| :--- | :--- | :---: | :--- |
| Bochs BGA / QEMU std VGA | `gpu.rs` | ✅ | Limine framebuffer, dinamik çözünürlük |
| VirtIO-GPU | `virtio_gpu.rs` | ✅ | 2D komut seti, scanout |
| VMware SVGA-II | `gpu.rs` | 🟡 | PCI algılama, temel çözünürlük |
| AMD Radeon / NVIDIA | `amdgpu.rs`, `nvidia.rs` | 🟡 | PCI tespit + 2D iskelet (3D yok) |

### Ağ

| Bileşen | Sürücü | Durum | Son Doğrulama | Not |
| :--- | :--- | :---: | :--- | :--- |
| Intel e1000/e1000e | `e1000.rs` | ✅ | QEMU `default` 2026-11-17 | DMA ring, DHCP, DNS, TCP/UDP, IPv4/IPv6, TLS |
| Intel I219-V | `i219.rs` | 🟢 | — | Gigabit Ethernet |
| Realtek RTL8139 | `e1000.rs` | 🟢 | — | I/O port, C+ modu |
| Realtek PCIe (RTL8168/8169/8125/8101) | `e1000.rs` | 🟢 | — | C+ modu, 64-bit RX/TX |
| NE2000 / RTL8019AS (ISA PnP) | `ne2000.rs` | 🟢 | — | ISA PnP, uzak DMA |
| NE2000 PCI (RTL8029AS) | `ne2000.rs` | 🟢 | — | PCI I/O BAR |
| Broadcom NetXtreme (BCM57xx) | `e1000.rs` | 🟢 | — | MAC hash, RX/TX |
| VirtIO-net | `virtio_net.rs` | ✅ | QEMU `default` 2026-11-17 | Sanal ağ |
| VMware VMXNET3 | `e1000.rs` | 🟢 | — | PCI 0x15AD |
| Atheros / Marvell Ethernet | `atheros.rs`, `marvell.rs` | 🟡 | — | RX/TX yolu yok; BDF düzeltmesi + filtre/checksum iskeleti eklendi, donanım doğrulaması bekliyor |
| Wi-Fi 802.11 | `wifi.rs` | 🔴 | — | WPA2 el sıkışması + Ayarlar'da simüle ağ listesi UI'ı var; gerçek PHY/RX-TX yok |
| Bluetooth HCI | `bluetooth.rs` | 🔴 | — | HCI paketleri; gerçek denetleyici yok |

### Ses

| Bileşen | Sürücü | Durum | Son Doğrulama | Not |
| :--- | :--- | :---: | :--- | :--- |
| Intel HDA | `hda.rs`, `audio.rs` | ✅ | QEMU `default` 2026-11-17 | 48 kHz stereo DMA, WAV/MP3 |
| PC Speaker (PWM) | `audio.rs` | 🟢 | — | Zamanlayıcı bip ve ton |
| USB Audio | `usb_audio.rs` | 🟡 | — | UAC1/UAC2 arayüz + iso-chronous endpoint keşfi (xHCI enumerasyonundan beslenir); streaming yok |

### Depolama

| Bileşen | Sürücü | Durum | Son Doğrulama | Not |
| :--- | :--- | :---: | :--- | :--- |
| ATA / IDE / ATAPI | `ata.rs` | 🟢 | — | LBA28/48, CD/DVD-ROM |
| AHCI / SATA | `ahci.rs` | 🟡 | — | DMA komut listesi/FIS/PRDT tahsisi ve gerçek sektör okuma-yazma uygulandı; QEMU AHCI ve gerçek donanım doğrulaması bekliyor |
| NVMe | `nvme.rs` | 🟢 | — | Komut kuyrukları |
| VirtIO-blk | `virtio_blk.rs` | ✅ | QEMU `default` 2026-11-17 | Sanal blok |
| USB Mass Storage | `usb_mass_storage.rs` | 🟡 | — | Gerçek enumerasyon olmadan "bulundu" raporlamaz; yazma desteklenmiyor olarak işaretli |
| SD / MMC | `sdmmc.rs` | 🟡 | — | SDHCI PIO okuma + CSD kapasite ayrıştırma; yazma bilinçli olarak reddedilir (desteklenmiyor) |

### Giriş

| Bileşen | Sürücü | Durum | Not |
| :--- | :--- | :---: | :--- |
| PS/2 klavye & fare | `ps2.rs`, `keyboard.rs`, `mouse.rs` | 🟢 | Kesme tabanlı, TR/EN |
| USB HID klavye & fare | `usb_hid.rs` | ✅ | QEMU `high` profili | xHCI (EHCI/UHCI/OHCI handoff), hotplug |
| USB dokunmatik ekran | `usb_touchscreen.rs` | 🟡 | — | HID Digitizer, çoklu dokunma; kalibrasyon yok |

---

## ✅ Otomatik QEMU Doğrulama Kapsamı

`test_qemu.ps1` aşağıdaki kontrolleri yapar ve `test-report.md` üretir:

- PMM (4 KB çerçeve tahsis/iade)
- VMM (PML4, USER|WRITABLE|NX)
- Gelişmiş allocator (bump + FSB)
- CPU çekirdek açılışı
- Ağ adaptörü link & ring (e1000/RTL8139/NE2000/VMXNET3)
- Ağ yığını özyin testi (boot sırasında): ARP TX, ICMP Echo, TCP SYN, soket/HTTP mantık
- Giriş sürücüleri (USB HID / PS/2 fallback)
- Intel HDA (48 kHz DMA) + gerçek WAV çıkışı
- Masaüstü olay döngüsü
- Uygulama bazlı bellek raporu (`[APP-MEM]`) — ✅ 20:05 UTC raporunda doğrulandı
- GUI ekran geometrisi / duman testi (`[GUI-SMOKE]`) — ✅ 20:05 UTC raporunda doğrulandı
- USB HID numaralandırma ve QMP ile klavye/fare raporu (yalnızca `high`/`UsbHid`)

---

## 🧷 Gerçek Donanım Doğrulama

Aşağıdaki tablolar **gerçek donanım ölçümü değildir**; doğrulama yapılmadığı
için tüm hücreler açıkça `DOĞRULANMADI` olarak işaretlenmiştir. Gerçek bir
sonuç elde edildiğinde ilgili hücreyi `✅` / `❌` ve tarihle güncelleyin.
`minos.iso`'yu USB'ye yazdırıp (Rufus/Ventoy) sistemi başlatın.

### CPU / SMP (gerçek donanım)

| Donanım | CPU | BSP açılışı | AP/SMP | Tarih | Not |
| :--- | :--- | :---: | :---: | :---: | :--- |
| Doğrulanmadı | Doğrulanmadı | DOĞRULANMADI | DOĞRULANMADI | — | QEMU `default` sonucu gerçek donanıma taşınamaz |

### Ağ (ARP / ICMP / TCP)

| Donanım | NIC | ARP | ICMP (ping) | TCP (HTTP) | Tarih | Not |
| :--- | :--- | :---: | :---: | :---: | :---: | :--- |
| Doğrulanmadı | Doğrulanmadı | DOĞRULANMADI | DOĞRULANMADI | DOĞRULANMADI | — | — |

### Ses (HDA)

| Donanım | Denetleyici | Çıkış | Tarih | Not |
| :--- | :--- | :---: | :---: | :--- |
| Doğrulanmadı | Doğrulanmadı | DOĞRULANMADI | — | — |

### Giriş (USB klavye / fare)

| Donanım | Denetleyici | Klavye | Fare | Tarih | Not |
| :--- | :--- | :---: | :---: | :---: | :--- |
| Doğrulanmadı | Doğrulanmadı | DOĞRULANMADI | DOĞRULANMADI | — | — |

### Depolama / Önyükleme

| Donanım | Depolama | Önyükleme | Tarih | Not |
| :--- | :--- | :---: | :---: | :--- |
| Doğrulanmadı | Doğrulanmadı | DOĞRULANMADI | — | — |

---

## ⚠️ Bilinen Sınırlamalar

- **SMP:** Çoğu gerçek dizüstünde APIC/AP başlatma reset ürettiği için BSP'de (tek çekirdek) açılır.
- **Wi-Fi / Bluetooth:** WPA2 el sıkışması ve Ayarlar'da ağ listesi UI'ı var; gerçek PHY/RX-TX yok (ağ listesi simüle edilir).
- **USB Audio / Printer / Kamera:** USB Audio'da arayüz/endpoint keşfi var; streaming, yazıcı ve kamera iskelet.
- **SD/MMC yazma:** Bilinçli olarak desteklenmiyor; yazma istekleri açık hata döndürür.
- **AHCI:** DMA blok I/O kodu mevcut; henüz QEMU/gerçek donanımda doğrulanmadı.
- **GPU 3D:** AMD/NVIDIA yalnızca 2D mode-set iskeleti.
- **Fan/EC profilleri:** Örnek ofsetler; anakart özel doğrulama gerekir.

---

## 🔗 İlgili Belgeler

- [`driver-maturity.md`](./driver-maturity.md) — sürücü olgunluk matrisi
- [`RELEASE_AND_TESTING.md`](./RELEASE_AND_TESTING.md) — sürüm ve test süreci
- [`../ISO_GUIDE.md`](../ISO_GUIDE.md) — ISO üretimi
- [`../test_qemu.ps1`](../test_qemu.ps1) — QEMU entegrasyon testi