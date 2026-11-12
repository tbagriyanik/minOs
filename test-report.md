# minOs QEMU Entegrasyon Test Raporu

| Alan | Değer |
| :--- | :--- |
| Zaman | 2026-11-12 17:39:56 UTC |
| ISO | `D:\minos\minos.iso` |
| QEMU profili | default |
| Bellek / vCPU | 512 MB / 2 |
| VGA | virtio |
| USB HID / PS/2 | kapalı / açık |
| HDA WAV çıktısı | yok |

## Kontroller

| Kontrol | Durum | Açıklama |
| :--- | :---: | :--- |
| PMM (Fiziksel Bellek Tahsis & Iade) | ❌ BAŞARISIZ | 4KB hizali cerceve tahsis ve sizintisiz iade |
| VMM (Sayfalama & Ring-3 Izolasyon) | ❌ BAŞARISIZ | PML4 haritalama, ceviri ve USER|WRITABLE|NX bayraklari |
| Gelismis Allocator (Bump & FSB) | ❌ BAŞARISIZ | Tasma korumali align_up, O(1) sabit blok ve buyuk blok havuzlama |
| CPU & Cekirdek Donanim Baslatimi | ❌ BAŞARISIZ | BSP GDT, IDT ve islemci mimari yapisi |
| Ag Adaptoru (e1000 / RTL8139 / NE2000 Link & Ring Hazir) | ❌ BAŞARISIZ | PCI/ISA donanim tespiti, MAC cozumu ve RX/TX halkalarinin aktivasyonu |
| Giris Suruculeri (USB-HID / PS-2 Fallback) | ❌ BAŞARISIZ | xHCI taramasi veya basarili PS/2 fallback aktivasyonu |
| Ses Denetleyicisi (Intel HDA Surucusu) | ❌ BAŞARISIZ | PCI BAR0 MMIO, 48kHz stereo DMA akisi |
| Cekirdek Surucu Profili | ❌ BAŞARISIZ | Tum donanim suruculerinin eksiksiz baslatimi |
| Ag Yigini On Testi (ARP TX) | ❌ BAŞARISIZ | Boot sirasinda ARP istegi TX halkasindan iletildi |
| ICMP (Ping) Testi | ❌ BAŞARISIZ | Gateway'e ICMP Echo Request gonderildi |
| TCP 3-Way Handshake Testi | ❌ BAŞARISIZ | Gateway HTTP portuna SYN paketi gonderildi |
| Soket + HTTP Mantik Testleri | ❌ BAŞARISIZ | NIC'den bagimsiz soket ve HTTP istemci mantigi dogrulandi |
| FPS 3D Uygulama Acilisi | ❌ BAŞARISIZ | Baslat menusu aramasi ile FPS 3D penceresi gercekten acildi |
| FPS 3D Metrik Akisi | ❌ BAŞARISIZ | Tamamlanan karelerden uretilen gercek FPS/min-FPS/frame-time ornekleri |
| Gerçek Ses Çıkışı (Intel HDA / WAV) | ❌ BAŞARISIZ | 48 kHz DMA akışı ve WAV dosyası |
| Masaüstü Olay Döngüsü | ❌ BAŞARISIZ | Ana etkileşim döngüsüne giriş |
| FPS 3D Stres Testi | ❌ BAŞARISIZ | açılış=False metrik_ornegi=0 son_fps=0 min_fps=0 max_frame_ms=0 max_interval_ms=0 frames=0 |

## Sonuç

**BAŞARISIZ** — bir veya daha fazla kontrol başarısız oldu.

> Bu rapor 	est_qemu.ps1 tarafından otomatik üretilmiştir.

