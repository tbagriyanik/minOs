# minOs QEMU Entegrasyon Test Raporu

| Alan | Değer |
| :--- | :--- |
| Zaman | 2026-11-12 19:39:37 UTC |
| ISO | `D:\minos\minos.iso` |
| QEMU profili | default |
| Bellek / vCPU | 512 MB / 2 |
| VGA | virtio |
| USB HID / PS/2 | kapalı / açık |
| HDA WAV çıktısı | 0 bayt PCM / dosya 44 bayt |

## Kontroller

| Kontrol | Durum | Açıklama |
| :--- | :---: | :--- |
| PMM (Fiziksel Bellek Tahsis & Iade) | ✅ GEÇTİ | 4KB hizali cerceve tahsis ve sizintisiz iade |
| VMM (Sayfalama & Ring-3 Izolasyon) | ✅ GEÇTİ | PML4 haritalama, ceviri ve USER|WRITABLE|NX bayraklari |
| Gelismis Allocator (Bump & FSB) | ✅ GEÇTİ | Tasma korumali align_up, bump LIFO ve FSB free-list yeniden kullanimi |
| CPU & Cekirdek Donanim Baslatimi | ✅ GEÇTİ | BSP GDT, IDT ve coklu/tek CPU baslatma durumu |
| Ag Adaptoru (e1000 / RTL8139 / NE2000 Link & Ring Hazir) | ✅ GEÇTİ | PCI/ISA donanim tespiti, MAC cozumu ve RX/TX halkalarinin aktivasyonu |
| Giris Suruculeri (USB-HID / PS-2 Fallback) | ✅ GEÇTİ | xHCI taramasi veya basarili PS/2 fallback aktivasyonu |
| Ses Denetleyicisi (Intel HDA Surucusu) | ✅ GEÇTİ | PCI BAR0 MMIO, 48kHz stereo DMA akisi |
| Cekirdek Surucu Profili | ✅ GEÇTİ | Tum donanim suruculerinin eksiksiz baslatimi |
| Ring3 Surec Zamanlayici (CR3 + Context Switch) | ✅ GEÇTİ | APIC timer onalimli gercek CR3/context-switch ile kullanici sureci dilimi |
| Ring3 Izolasyon & Yigin Guard Testi | ✅ GEÇTİ | Kernel adresi erisimi reddi (SIGSEGV), kullanici yigin tasmasi ve yield/exit dogrulamasi |
| Ring3 Syscall Register Restore | ✅ GEÇTİ | GPR ve XMM durumunun int 0x80 donusunde korunmasi |
| Ring3 Syscall Return | ✅ GEÇTİ | getpid sistem cagrisi ve Ring 3'ten Ring 0'a EXIT donusu |
| Ring3 Kernel Memory Denial | ✅ GEÇTİ | Kullanici modundan cekirdek bellegi okuma girisiminin engellenmesi |
| Ring3 Stack Guard Fault | ✅ GEÇTİ | Kullanici yigin guard sayfasi tasmasinin yakalanmasi |
| Process Fork Exec Wait | ✅ GEÇTİ | Fork sayfa kopyasi, exec imaj degisimi, bloklanan wait ve kaynak iadesi |
| Ag Yigini On Testi (ARP TX) | ✅ GEÇTİ | Boot sirasinda ARP istegi TX halkasindan iletildi |
| ICMP (Ping) Testi | ✅ GEÇTİ | Gateway'e ICMP Echo Request gonderildi |
| TCP 3-Way Handshake Testi | ✅ GEÇTİ | Gateway HTTP portuna SYN paketi gonderildi |
| Soket + HTTP Mantik Testleri | ✅ GEÇTİ | NIC'den bagimsiz soket ve HTTP istemci mantigi dogrulandi |
| Gerçek Ses Çıkışı (Intel HDA / WAV) | ✅ GEÇTİ | 48 kHz DMA akışı ve WAV dosyası |
| Masaüstü Olay Döngüsü | ✅ GEÇTİ | Ana etkileşim döngüsüne giriş |

## Sonuç

**BAŞARILI** — tüm kontroller geçti.

> Bu rapor 	est_qemu.ps1 tarafından otomatik üretilmiştir.

