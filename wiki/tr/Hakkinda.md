# Hakkında & Mimari

minOs (Sarman Edition) v2.1.0, Rust programlama diliyle geliştirilmiş, `no_std` ve `no_main` özelliklerini kullanan bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Limine v8 önyükleyici ile UEFI ve BIOS (Legacy) ortamlarında sorunsuz önyüklenir.

---

## Tasarım Felsefesi

- **Minimal ve Bağımsız:** Hiçbir mevcut işletim sistemi üzerine kurulmamıştır; donanıma doğrudan erişim sağlar.
- **Rust Güvenliği:** `no_std` ortamında bellek güvenliği, tür güvenliği ve veri çakışmalarından korunma sağlanır.
- **Modüler Mimari:** Sürücüler, dosya sistemleri, GUI ve uygulamalar ayrı modüller olarak organize edilmiştir.
- **Eğitim ve Araştırma:** Açık kaynaklı, öğrenme ve deneme amaçlı geliştirilmiştir.

---

## Çekirdek Bileşenleri

| Bileşen | Dosya | Açıklama |
| :--- | :--- | :--- |
| **Çekirdek Giriş** | `src/main.rs` | `_start` giriş noktası, Limine istekleri, donanım kurulumu |
| **Mimari** | `src/arch/x86_64/` | SSE, IDT kesmeleri, PIC maskeleme, GDT/TSS |
| **Önyükleme** | `src/boot/` | Limine konsolu, metin çizimi, ilerleme çubuğu |
| **Sürücüler** | `src/drivers/` | GPU, ağ, depolama, giriş, ses, USB, ACPI, kripto |
| **GUI** | `src/gui/` | Pencere yöneticisi, font motoru, tema motoru |
| **Uygulamalar** | `src/apps/` | 15 masaüstü uygulaması |
| **Masaüstü** | `src/desktop/` | Oturum yönetimi, ana olay döngüsü |
| **Bellek** | `src/mm/` | Fiziksel bellek yöneticisi (PMM), ELF yükleyici |
| **Dosya Sistemi** | `src/fs/` | VFS, FAT32, Ext2, Ext4, exFAT, ISO9660 |

---

## `no_std` Modeli

Çekirdek Rust standart kitaplığı olmadan derlenir. Bellek tahsisi için `linked_list_allocator` kullanılır. TTF yazı tiplerinden bitmap atlasları derleme sırasında `fontdue` ile üretilir ve çekirdek grafik arayüzünde kullanılır.

---

## Bellek Yönetimi (PMM)

- **Statik BSS Mimarisi:** `MemoryManager` statik `.bss` segmentine taşınmıştır (CodeQL Alert #25 düzeltmesi).
- **Yığın Kaçışı Önleme:** Raw pointer çözümlenmesi güvenli hale getirilmiştir.
- **Sayfa Tabloları:** VRAM aralığı 2 MB huge page olarak haritalanır.

---

## Kesme ve İstisna Yönetimi (IDT)

Açılışın ilk anından itibaren BSP GDT+TSS (`IST1`) ve IDT istesna kapıları yüklenir:

- `#DE` (Bölme Hatası)
- `#UD` (Geçersiz Talimat)
- `#DF` (İki Katlı Hata)
- `#SS` (Yığın Segment Hatası)
- `#GP` (Genel Koruma Hatası)
- `#PF` (Sayfa Hatası)

Sürücü başlatma sırasında donanım hatalarında anakartın ani yeniden başlaması (triple fault) önlenir.

---

## SMP (Çoklu İşlemci)

- Limine SMP protokolü mevcuttur.
- Varsayılan profil tek çekirdek modunda başlar (fiziksel laptop kararlılığı için).
- AP başlatma kodu `src/arch/x86_64/smp.rs` içinde mevcuttur ancak varsayılan akışta kullanılmaz.
- TTAS Spinlock senkronizasyonu ve per-core CPU izleme çubukları mevcuttur.
