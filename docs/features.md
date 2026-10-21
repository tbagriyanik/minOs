# Özellikler ve Yol Haritası

Uygulama kataloğu ve proje yol haritası.

## ✨ Özellikler ve Uygulama Paketi

minOs, tam teşekküllü bir masaüstü deneyimi sunan **14 adet yerleşik uygulama** ve pencere yöneticisi içerir:

| Uygulama | Açıklama |
| :--- | :--- |
| **🔘 Başlat Menüsü** | Kategori menüleri (`Sistem`, `Üretkenlik`, `Medya`, `Güç`) altında simgeli grafik listeleme. Klavye ok tuşları, Enter, fare tekerleği gezintisi ve arama. |
| **📁 Dosya Yöneticisi** | Sanal dosya sistemi (VFS/TarFS) gezgini; dosya türü simgeleri (`[LOG]`, `[TXT]`, `[IMG]`, `[SND]`, `[VID]`), boyutlar ve konum çubuğu. `-UsbDisks` ile bağlanan USB belleğin **FAT32** ve CD'nin **ISO9660** içeriği de listelenir. Masaüstü (`/desktop`) gezgenden bağımsız, sabit bir alandır; iki görünüm birbirinin seçim ve gezinme durumunu etkilemez. |
| **⚙️ Sistem Ayarları** | Beş sekme (Temel / Ekran / Ağ / Aygıtlar / Testler): v1.7.0 sürüm bilgisi ve çekirdek durumu, beş tema, ses düzeyi, gece ışığı, dil (TR/EN), klavye düzeni, yazı yakınlaştırma, hedef çözünürlük ve bölge seçimi; seçilebilir arka plan/duvar kağıdı (tema rengi, tek renk seçenekleri ve 3 masaüstü resmi) ve anında önizleme; pencere düzeni kaydetme ve **`settings.cfg` üzerinden kalıcı ayar saklama**. Gerçek ekran modu Limine önyükleme menüsünden seçilir. |
| **📦 Paket Yöneticisi (`minPak`)** | Grafiksel ve komut satırı destekli paket yönetim arayüzü. Depo kataloglarını listeleme, paket arama, yerel/ağ üzerinden paket kurma ve kurulu paketleri yönetme. |
| **📝 Not Defteri** | Üst menü çubuğu (`Dosya`, `Düzen`, `Yardım`), metin temizleme/kaydetme, **Kes / Kopyala / Yapıştır / Tümünü Seç**, **Geri Al / İleri Al** (Ctrl+Z / Ctrl+Y). Türkçe karakter desteği, `Backspace`, `Enter`, **Home / End**, **Shift+ok tuşlarıyla seçim**, satır numaraları ve imleç. |
| **🌐 Web Tarayıcısı** | Sık kullanılanlar (Bookmarks) çubuğu, hafif HTTP test sitesi, minos://bookmarks, minos://help, minos://cats, minos://net, minos://history ve minos://demo sayfaları, sekmeli gezinme ve e1000 ARP/ping/DNS test paneli. https:// adresleri (443 portu) **TLS 1.2** el sıkışması ile güvenli bağlantı kurar; sunucu yanıtı AES-128-GCM ile şifresi çözülüp HTTP olarak işlenir. |
| **🖼️ Resim Gösterici** | Çizim Tahtası'ndan kaydedilen 24-bit `.bmp` çizimlerini ve sanal dosya sistemindeki resimleri açar, yakınlaştırma/uzaklaştırma ve ayrıntılı resim meta verisi paneli sunar; resim yüklenmemişken sevimli kedi maskotunu gösterir. |
| **🎵 Müzik Çalar** | Kullanıcı oynatmayı başlattığında HDA/PC hoparlörü üzerinden ses, animasyonlu spektrum ekolayzerı ve çalma listesi. Sistem açılışında otomatik ses çalınmaz. |
| **🎬 Video Oynatıcı** | Canlı synthwave ufuk sahnesi, neon güneş scanline'ları, aşağı doğru akan 3D perspektif grid yol ve siber araç simülasyonu. |
| **📊 Görev Yöneticisi** | Çekirdek bellek kullanımı, CPU dalgalanma grafiği, çok çekirdekli (SMP) per-core CPU çubukları, görev tablosu, süreç sonlandırma (`Kill`), alfabetik "Program Aç" başlatma menüsü ve RAM doluluk çubuğu. Pencere altına sabitlenmiş durum çubuğu ile ferah performans izleme. |
| **🧮 Hesap Makinesi** | Klavyeyle giriş yapılabilen ifade hesaplayıcı (`+`, `-`, `*`, `/`); 128 bit sabit nokta aritmetiğiyle geniş sayı aralığı ve altı basamağa kadar ondalık hassasiyet. `Backspace` ve `Esc` ile düzenleme/temizleme. |
| **🎨 Çizim Tahtası** | Piksel tabanlı serbest çizim tuvali, renk paleti ve dairesel silgi aracı. Fare basılı tutuldukça **sürekli/akıcı çizim**, fırça kenarının otomatik kırpılması, doğrudan `/root/Documents/cizim.bmp` formatında kaydetme/açma ve 4096 noktalık tuval geçmişi. |
| **⌨️ Komut İstemi (MinShell)** | UNIX/DOS benzeri etkileşimli kabuk: dosya/klasör yönetimi, bellek durumu, ağ teşhisi, `curl` ile HTTP indirme, `format` ile disk biçimlendirme (FAT32/ext4/exFAT/ext2), `pkg`, `exec`, Tab tamamlama ve komut geçmişi. |
| **💽 Tek Tıkla Kur (Kurulum Sihirbazı)** | Adımlı kurulum penceresi: **1)** hedef disk seçimi (ATA sabit disk / QCOW2), **2)** `Biçimlendir (FAT32)` butonu ile diski biçimlendirme ve **3)** `Tek Tıkla Kur` ile kalıcı kurulum. Kurulum; `MINOS.SYS` işaret dosyasını, `BENIOKU.TXT`, mevcut `settings.cfg` ayarlarını ve VFS ağacının tamamını (`VFS.DAT` anlık görüntüsü) diske yazar. Disk değiştirildiğinde biçimlendirme/kurulum durumu sıfırlanır; TR/EN dilleri ve ilerleme çubuğu desteklenir. |
| **🧊 3D Gösterici** | Küp, silindir, küre ve piramit mesh'lerini dolgulu gölgelendirme veya tel kafes modunda gösterir; sürükleyerek döndürme, yakınlaştırma, otomatik döndürme ve `.3d` dosyalarından şekil/metin yükleme. Arka-yüz eleme ve uzaktan-yakına ressam sıralaması ile doğru katı cisim görünümü. |

> **Not:** `src/apps/` altındaki **Kontrol Paneli** (`controlpanel.rs`) kod tabanında mevcuttur; ayarlar ve kişiselleştirme işlevleri **Sistem Ayarları** (`settings.rs`) üzerinden yürütülmektedir.


---


## 🗺️ Gelecek Planları ve Yol Haritası (Roadmap)

> Detaylı mimari ve faz planı için: [`planlama.md`](../planlama.md)

Yakın ve orta vadede minOs için planlanan başlıca geliştirmeler:

- [x] **Gelişmiş TCP/IP Yığını & Socket API:**
  - Soket başına durum makineli TCP (`src/drivers/socket.rs`): 3'lü el sıkışma, sıra/ack takibi, üssel geri çekilmeli yeniden iletim, 8 eşzamanlı soket; `socket()`, `connect()`, `send()`, `recv()`, `close()`, UDP `sendto()`/`recvfrom()`, önbellekli `resolve_host()`.
  - Komut satırından `curl http://...` ile dosya indirme (`src/drivers/http.rs`: chunked çözümü, 3xx yönlendirme takibi).
- [x] **Kalıcı FAT32 / Ext2 Dosya Sistemi Desteği:**
  - Ext2 read-write ve MBR/GPT bölümleme (`src/fs/ext2.rs`, `src/fs/part.rs`) tamamlandı.
- [x] **Tam Ring 3 Kullanıcı Alanı & Syscall API (v1.4.0):**
  - `int 0x80` mimarisiyle çekirdekten izole kullanıcı alanı süreçleri, GDT TSS kullanıcı segmentleri.
  - Statik ELF yükleyici (`src/mm/loader.rs`), sanal bellek sayfa koruması ve `#GP` / `#PF` hata yakalama.
- [x] **SMP (Symmetric Multiprocessing) Kod Bileşenleri (v1.4.0; mevcut boot yolunda kapalı):**
  - Limine SMP protokolü ile AP çekirdeklerini uyandırma, bağımsız yığınlar ve Local APIC Timer/vektör izolasyonu.
  - TTAS Spinlock (`src/arch/x86_64/sync.rs`), çekirdekler arası paralel atomik iş kuyruğu ve sağlama testleri.
- [x] **Paket Yöneticisi (`minPak`) (v1.4.0 / v1.5.0):**
  - Grafiksel (`pkgmgr`) ve komut satırı (`minPak`) arayüzü; depo katalog sorgulama, paket arama, yerel/ağ üzerinden paket kurulumu ve kaldırma.
- [x] **Gelişmiş Grafik Sürücüleri & Sanallaştırma Hızlandırması (v1.4.0):**
  - VMware SVGA II ve konuk grafik hızlandırma algılaması (`src/drivers/gpu.rs`), donanım destekli çift tamponlama ve optimize çizim motoru.


### 🆕 Modern Arayüz ve Kullanılabilirlik Yenilikleri

- **🛡️ Erken İstisna Yakalama & Triple Fault Koruması:** Çekirdek başlangıcında (`init_interrupts`) ilk iş olarak BSP için `GDT+TSS (IST1)` ve erken IDT istisna kapıları (`#DE`, `#UD`, `#DF`, `#SS`, `#GP`, `#PF`, `0x80`) yüklenir. Sürücü başlatma veya bellek aşımı anında donanımın sessizce üçlü hataya (triple fault) düşüp reset atması engellenir.
- **📟 Etkileşimli Metin Kurtarma Konsolu (`recovery#`):** Açılışta veya kritik sürücülerde hata meydana geldiğinde karşılama ekranına geçilmeden önce ekranda hata detayları (Vektör, RIP, CS, CR2, Err) renkli metinle sunulur ve kullanıcıya interaktif komut istemi sağlanır (`restart` / `reboot` / `r`, `halt` / `h`, `help`).
- **🖥️ 2560×1600'e Kadar Yüksek Çözünürlük ve 16:10 Desteği:** Çekirdek arka tamponu (backbuffer) `2560x1600` boyutuna genişletildi. 1920×1080 (FHD), 1920×1200 (16:10 WUXGA), 2560×1440 (2K) ve 2560×1600 ekranlar tam desteklenir; 1920 çözünürlükteki sınır doğrulama algoritması düzeltildi.
- **🗺️ Tam VRAM MMIO Haritalaması:** Açılışta 16 MB VRAM aralığının tamamı sayfa tablolarına 2MB huge page olarak haritalanır; VRAM sınır aşımından kaynaklı sayfa hataları (`#PF`) önlenir.
- **🔊 Intel HDA & Modern Laptop Ses Uyumluluğu:** Intel Smart Sound (DSP/cAVS) yonga setleri için zaman aşımı ve PCI multi-function denetimleri eklendi; donanım yanıt vermediğinde sistem çökmeden güvenle PC Speaker PWM moduna geri düşer.
- **⚙️ Ayarlar Penceresi Tıklama İzolasyonu:** Yazı yakınlaştırma (%100, %125, %150) butonlarına tıklandığında çözünürlük butonlarının yanlışlıkla tetiklenmesi önlendi; gerçek donanımda BGA yazımları kısıtlanarak güvenli ekran modu yönetimi sağlandı.
- **🌐 Soket API & `curl`:** Çekirdekte BSD tarzı soketler (`socket`/`connect`/`send`/`recv`/`close`, UDP, DNS önbelleği) ve komut istemcisinden `curl http://...` ile HTTP indirme (chunked + yönlendirme destekli).
- **👋 Hoş Geldin Ekranı:** Masaüstü açılmadan önce arayüz dilini (Türkçe/English) ve beş masaüstü temasından birini seçin. Tercihler anında uygulanır ve kalıcı olarak diske kaydedilir.
- **🚀 Limine Açılış Menüsü (Live & Kurulu Sistem Seçenekleri):**
  - **`minOs Live Environment`**: Kurulum yapmadan doğrudan RAM üzerinden çalışan Live CD ortamı.
  - **`minOs Installed System`**: Diske kurulmuş kalıcı minOs sistemine doğrudan giriş.
  - **`Display Resolutions`**: 800×600 (SVGA), 1024×768 (XGA), 1280×720 (HD) ve 1920×1080 (FHD) seçenekleriyle açılış çözünürlüğü menüsü.
- **💽 Tek Tıkla Kur & Kalıcı Depolama (Persistent Storage):** Masaüstündeki veya Başlat menüsündeki "minOs'u Kur" sihirbazı ile seçilen hedef sabit disk (ATA / QCOW2) tek tıkla FAT32 olarak biçimlendirilir, `MINOS.SYS` kurulum işaretçisi, sistem yapılandırması ve VFS ağacının tamamı (`VFS.DAT`) diske yazılır. Kullanıcı dosyaları, klasörleri, masaüstü öğeleri, çöp kutusu içeriği ve ayarlar (`settings.cfg`) sonraki açılışlarda otomatik olarak geri yüklenir.
- **🖱️ Masaüstü ve Giriş Alanı Sağ Tık Simgeleri & Kısayol İpuçları:** Masaüstü sağ tık menüsünde ve metin kutusu/textarea sağ tık menülerinde görsel simgeler ile kısayol bilgileri (Ctrl+C, Ctrl+X, Ctrl+V, Ctrl+O, Ctrl+S) gösterilir.
- **📐 Sağ Kenara Dayalı Scrollbar ve Dıştan Resize:** Kaydırma çubukları pencere sağına tam yaslanır; pencere sağ resize tutamaçları pencere dışından da rahatça yakalanabilir.
- **📜 Not Defteri Satır Numaraları:** Not Defteri satır numaralandırması netleştirildi; dikey kaydırma ve imleç takibiyle tam hizalı çalışır.
- **🇹🇷 Tam Türkçe Karakter Desteği:** Sistem Ayarları, Görev Yöneticisi, Not Defteri, Dosya Yöneticisi, Başlat menüsü ve durum çubukları dahil arayüzdeki tüm metinler Türkçe karakterlerle (`ç`, `Ç`, `ğ`, `Ğ`, `ı`, `I`, `İ`, `ö`, `Ö`, `ş`, `Ş`, `ü`, `Ü`) işlenir.
- **🔤 TTF Tabanlı Fontlar:** Genel arayüz Inria Sans, komut istemi Geist Mono kullanır. Derleme betiği fontların sunduğu karakterlerden bitmap atlas oluşturur; yazı yakınlaştırma ayarı arayüzde uygulanır.
- **⌨️ Klavyeyle Menü Gezintisi (Ok Tuşları & Enter):** Başlat menüsü ve bağlam menülerinde **Yukarı/Aşağı/Sol/Sağ ok tuşları** ile gezinebilir, **Enter** tuşuna basarak seçili uygulamayı veya komutu çalıştırabilirsiniz.
- **🖱️ Fare Tekerleği (Scroll Wheel) Desteği:** Başlat menüsü açıkken fare tekerleğiyle liste seçenekleri arasında geçiş yapılabilir. Pencere içerikleri ve açılış logu dikey olarak kaydırılabilir.
- **📐 Daha Geniş Kenar Tutamaçları (12px Resize Grip):** Pencereleri kenarlarından tutup boyutlandırma alanı 8 pikselden **12 piksele** çıkarıldı. Artık pencerelerin kenar ve köşelerinden tutulması ve boyutlandırılması çok daha kolaydır.
- **🖼️ Masaüstü Simgesi Tıklama Koruması:** Açık ve görünür bir pencere masaüstü simgelerinin üzerine geldiğinde, pencereye tıklanıldığında altındaki masaüstü simgelerinin istemeden açılması engellendi.
- **🎨 Dinamik Tema ve Kontrast:** Kadife Gece, Gün Işığı, Kutup Mavisi, Yeşil Terminal ve Gün Batımı temaları arasında canlı geçiş. Düğme ve seçili öğe metinleri zemin rengine göre siyah veya beyaz seçilerek kontrast korunur.
- **🪟 Akıllı Pencere Yönetimi:** Çift tıkla maksimize etme, ekranın en üstüne sürükleyerek otomatik maksimize (snap) etme, pencere ortalama, simge durumuna küçültme ve yumuşak z-sırası odak yönetimi.
- **🗔 Çoklu Pencere:** Dosya yöneticisi, not defteri ve diğer desteklenen uygulamalar birden fazla pencerede açılabilir. Sistem Ayarları ve Görev Yöneticisi tek örnek olarak çalışır.
- **📌 Menülerin Dış Tıklamayla Kapanması:** Başlat, sağ tık, ses, takvim ve Görev Yöneticisi açılır menüsünün dışına tıklayınca menü kapanır; tıklama hedefteki pencere veya görev çubuğu öğesine iletilir.
- **📋 Not Defteri Açılır Menü Komutları:**
  - **Dosya:** Yeni Belge, Kaydet, Temizle
  - **Düzen:** Kes, Kopyala, Yapıştır, Tümünü Seç
  - **Yardım:** Not Defteri Hakkında
- **📝 Gelişmiş Metin Düzenleme:**
  - **Çift tıkla kelime seçimi:** Herhangi bir kelimeye çift tıklayarak o kelimeyi seçin.
  - **Sürükleyerek seçim:** Sol tuşu basılı tutup fareyi sürükleyerek metin seçin; tıklanan karaktere doğru imleç konumlanır (satır kaydırma ve dikey kaydırma hesaba katılır).
  - **Home / End:** İmleci satır başına/sonuna taşır.
  - **Shift + Ok Tuşları:** Yukarı/Aşağı/Sol/Sağ ile seçimi genişletir.
  - **Geri Al / İleri Al:** `Ctrl+Z` (Geri Al) ve `Ctrl+Y` (İleri Al) — 32 seviyelik düzenleme geçmişi.
- **🎨 Piksel Tabanlı Çizim:**
  - **Basılı Tutup Çizme:** Sol tuşu basılı tuttukça fare hareketi sürekli çizer; hızlı hareketlerde iki nokta arası enterpolasyonla doldurulup kesik çizgi oluşmaz.
  - **Otomatik Vuruş Bittiği:** Tuval dışına çıkıldığında veya tuş bırakıldığında kalem yolu otomatik sonlanır.
  - **Renk Paleti ve Silgi:** Mavi/Kırmızı/Yeşil/Siyah renk örnekleri, dairesel **Silgi** ve **Temizle** düğmeleri.

---

