# minOs QEMU Entegrasyon Test Raporu

| Alan | Değer |
| :--- | :--- |
| Zaman | 2026-11-17 17:59:12 UTC |
| ISO | `D:\minos\minos.iso` |
| QEMU profili | default |
| Bellek / vCPU | 512 MB / 2 |
| VGA | vmware |
| USB HID / PS/2 | açık / devre dışı |
| HDA WAV çıktısı | 0 bayt PCM / dosya 44 bayt |

## Kontroller

| Kontrol | Durum | Açıklama |
| :--- | :---: | :--- |
| PMM (Fiziksel Bellek Tahsis & Iade) | ✅ GEÇTİ | 4KB hizali cerceve tahsis ve sizintisiz iade |
| VMM (Sayfalama & Ring-3 Izolasyon) | ✅ GEÇTİ | PML4 haritalama, ceviri ve USER|WRITABLE|NX bayraklari |
| Sayfa Tablosu Geri Kazanimi (PT/PD/PDPT) | ✅ GEÇTİ | Unmap sonrasi bosalan alt seviye tablo cercevelerinin PMM'e iadesi (sizintisiz) |
| Gelismis Allocator (Bump & FSB) | ✅ GEÇTİ | Tasma korumali align_up, bump LIFO ve FSB free-list yeniden kullanimi |
| Buyuk Sayfa (2MB Huge Page) | ✅ GEÇTİ | 2MB hizali tahsis, PD seviyesinde HUGE_PAGE esleme ve sizintisiz iade |
| Bellek sıkıstirma (Swap'siz MZRAM) | ✅ GEÇTİ | Sifir sayfa RLE, desen eslesmesi, geri acma ve sıkıstirilamaz veri reddi |
| Kernel Heap Buyutme ve Tembel Doldurma | ✅ GEÇTİ | Heap genisletme (commit) ve buyuk tahsisatlarin tembel doldurulmasi |
| Bellek Kullanim Profili | ✅ GEÇTİ | PMM/kernel/kullanici bellek profili ve COW paylasim sayaclari |
| CPU & Cekirdek Donanim Baslatimi | ✅ GEÇTİ | BSP GDT, IDT ve coklu/tek CPU baslatma durumu |
| Ag Adaptoru (e1000 / RTL8139 / NE2000 Link & Ring Hazir) | ✅ GEÇTİ | PCI/ISA donanim tespiti, MAC cozumu ve RX/TX halkalarinin aktivasyonu |
| Giris Suruculeri (USB-HID / PS-2 Fallback) | ✅ GEÇTİ | xHCI taramasi veya basarili PS/2 fallback aktivasyonu |
| Ses Denetleyicisi (Intel HDA Surucusu) | ✅ GEÇTİ | PCI BAR0 MMIO, 48kHz stereo DMA akisi |
| Cekirdek Surucu Profili | ✅ GEÇTİ | Tum donanim suruculerinin eksiksiz baslatimi |
| Ring3 Surec Zamanlayici (CR3 + Context Switch) | ✅ GEÇTİ | APIC timer onalimli gercek CR3/context-switch ile kullanici sureci dilimi |
| Ring3 Izolasyon & Yigin Guard Testi | ✅ GEÇTİ | Kernel adresi erisimi reddi (SIGSEGV), kullanici yigin tasmasi ve yield/exit dogrulamasi |
| Cok Cekirdekli Scheduler Stres Testi | ✅ GEÇTİ | CPU'lar arasi gorev dagilimi, notify/iptal yarisi ve kuyruk derinligi (RUN_QUEUE_DEPTH) sizinti denetimi |
| Ring3 Syscall Register Restore | ✅ GEÇTİ | GPR ve XMM durumunun int 0x80 donusunde korunmasi |
| Ring3 Syscall Return | ✅ GEÇTİ | getpid sistem cagrisi ve Ring 3'ten Ring 0'a EXIT donusu |
| Ring3 Kernel Memory Denial | ✅ GEÇTİ | Kullanici modundan cekirdek bellegi okuma girisiminin engellenmesi |
| Ring3 Stack Guard Fault | ✅ GEÇTİ | Kullanici yigin guard sayfasi tasmasinin yakalanmasi |
| Process Fork Exec Wait | ❌ BAŞARISIZ | Copy-on-write fork paylasimi, exec imaj degisimi, bloklanan wait ve kaynak iadesi |
| Kullanici Heap ve Mmap (brk/mmap) | ✅ GEÇTİ | brk buyut/kucult, mmap/munmap, MEMINFO (R10 ABI) ve sizintisiz iade |
| Dosya Taniticisi Yasam Dongusu | ❌ BAŞARISIZ | open/read/lseek/close, nesne referans sayimi, fork devralma, exec sifirlama ve fd kotasi |
| Surec Kaynak Limitleri | ❌ BAŞARISIZ | RESINFO raporu, heap/mmap/fd/cerceve kotasi ve fork ile limit mirasi |
| Copy-on-Write (Ring3 yazma hatasi) | ✅ GEÇTİ | Fork sonrasi #PF ile COW kopyasi; cocuk yazari ebeveyne sizmaz |
| ASLR (yigin + mmap tabani) | ✅ GEÇTİ | Ring3 yigin ustu her demo/surecte rastgele kaydirilir; mmap tabani ASLR |
| Calisan Sistem Scheduler/Surec Izleme | ✅ GEÇTİ | Calisan sistemde dilim sayaci, surec durumlari, heap/mmap ve COW sayaclari |
| Ag Yigini On Testi (ARP TX) | ❌ BAŞARISIZ | Boot sirasinda ARP istegi TX halkasindan iletildi |
| ICMP (Ping) Testi | ✅ GEÇTİ | Gateway'e ICMP Echo Request gonderildi |
| TCP 3-Way Handshake Testi | ✅ GEÇTİ | Gateway HTTP portuna SYN paketi gonderildi |
| Soket + HTTP Mantik Testleri | ✅ GEÇTİ | NIC'den bagimsiz soket ve HTTP istemci mantigi dogrulandi |
| USB HID Numaralandirma (xHCI) | ✅ GEÇTİ | qemu-xhci uzerinde usb-kbd ve usb-mouse numaralandirildi |
| USB HID Etkin Girdi Yolu | ✅ GEÇTİ | Masaustu girdisi USB HID surucusune baglandi |
| PS/2 Devre Disi (i8042 yok) | ✅ GEÇTİ | Girdi yalnizca USB HID uzerinden geliyor |
| USB Klavye Raporu (QMP enjeksiyonu) | ❌ BAŞARISIZ | QMP 'a' tusu xHCI HID raporu olarak islendi |
| USB Fare Raporu (QMP enjeksiyonu) | ❌ BAŞARISIZ | QMP fare hareketi/tiki xHCI HID raporu olarak islendi |
| Gerçek Ses Çıkışı (Intel HDA / WAV) | ✅ GEÇTİ | 48 kHz DMA akışı ve WAV dosyası |
| Masaüstü Olay Döngüsü | ❌ BAŞARISIZ | Ana etkileşim döngüsüne giriş |

## Sonuç

**BAŞARISIZ** — bir veya daha fazla kontrol başarısız oldu.

> Bu rapor 	est_qemu.ps1 tarafından otomatik üretilmiştir.

