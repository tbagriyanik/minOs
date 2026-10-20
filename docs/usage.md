# Kullanım Rehberi

Komut istemi, masaüstü kısayolları ve kaynak ağacı referansı.

## 💻 Komut İstemi (MinShell) Desteklenen Komutlar

MinShell, klavye geçmişi (Yukarı/Aşağı oklar), akıllı sekme tamamlama (`Tab`) ve dinamik çıktı kaydırma desteğiyle aşağıdaki komutları destekler:

### 1. Dosya ve Dizin Yönetimi
| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `pwd` | `pwd` | Geçerli çalışma dizinini (CWD) ekrana yazar. |
| `cd` | `cd <dizin>` | Çalışma dizinini değiştirir (`cd ..`, `cd /`, `cd ~` veya göreceli/mutlak yollar). |
| `ls` | `ls [dizin]` | Geçerli dizindeki veya hedefteki dosya ve klasörleri boyutlarıyla listeler. |
| `cat` | `cat <dosya>` | Metin dosyasının içeriğini terminale yazdırır. |
| `touch` | `touch <dosya>` | Sanal dosya sisteminde yeni ve boş bir dosya oluşturur. |
| `write` | `write <dosya> <metin>` | Belirtilen dosyaya metin verisi yazar/üzerine kaydeder. |
| `mkdir` | `mkdir <klasor>` | Yeni bir sanal klasör oluşturur. |
| `rm` | `rm <hedef>` | Belirtilen dosya veya dizini siler. |
| `disks` / `df` | `disks` veya `df` | Bağlı IDE/ATA kanallarını, USB (FAT32), Sabit Disk ve CD-ROM sürücülerini listeler. |

### 2. Sistem Bilgisi ve Bellek
| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `mem` | `mem` | Fiziksel Bellek Yöneticisi (PMM) üzerinden toplam ve boş bellek miktarını (KB) gösterir. |
| `ps` | `ps` | Çekirdek ve aktif sistem süreçlerini (PID, isim) listeler. |
| `date` | `date` | Gerçek Zamanlı Saat (RTC / CMOS) üzerinden güncel tarih ve saati gösterir. |
| `uname` | `uname` | minOs çekirdek sürümünü ve mimari bilgisini (`x86_64 Sarman Edition v2.0`) yazar. |
| `whoami` | `whoami` | Aktif kullanıcı yetkisini gösterir (`sarman (root)`). |
| `theme` | `theme` | Masaüstü temasını komut satırından anında sıradaki temaya geçirir. |
| `uptime` | `uptime` | Sistemin açılışından itibaren geçen çalışma süresini (saat, dakika, saniye, milisaniye) gösterir. |
| `fetch` / `neofetch` | `fetch` veya `neofetch` | Renkli ASCII Sarman maskotu ile çekirdek, mimari, boş/toplam RAM ve VFS düğüm özetini sunar. |
| `clear` | `clear` | Terminal ekranını temizler. |
| `help` | `help` | Kullanılabilir temel komutların özet listesini görüntüler. |

### 3. Ağ Teşhisi ve Araçları
| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `ipconfig` | `ipconfig` | `eth0` arabiriminin yerel IP, ağ maskesi, ağ geçidi, DNS, MAC adresi ve DHCP durumunu raporlar. |
| `ping` | `ping <IPv4>` | Belirtilen IPv4 hedefine ICMP Echo Request paketi gönderir. |
| `dns` | `dns <alan_adi>` | Belirtilen sunucu adı için yapılandırılmış DNS sunucusuna UDP DNS sorgusu iletir. |
| `curl` | `curl http://<host>[:port][/yol]` | Soket API üzerinden HTTP GET indirir: durum kodu + gövde; chunked kod çözme ve 3xx yönlendirme takibi desteklenir. Terminal komutu `https` desteklemez; şifreli sayfalar için **Web Tarayıcısı** uygulamasını (`https://` şeması) kullanın. |

### 4. Disk ve Çekirdek Komutları
| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `format` / `mkfs` | `format <fat32\|ext4\|exfat\|ext2> <0..3>` | Bağlı ATA sabit diski belirtilen dosya sisteminde biçimlendirir (FAT32, ext4, exFAT veya ext2). |
| `minPak` / `pkg` | `minPak <list\|search\|install\|remove>` | `minPak` komut satırı paketi: kurulu paketleri listeler, arar, yerel/ağ üzerinden paket kurar ve kaldırır. |
| `exec` | `exec <dosya_yolu>` | ELF/ikili dosyayı ELF yükleyici ile belleğe yükler ve Ring 3 kullanıcı kipinde çalıştırır. |

### 5. Uygulama Başlatma
| Komut | Alternatifler | Açıklama |
| :--- | :--- | :--- |
| `open <uygulama>` | `run <uygulama>` | İstenen masaüstü uygulamasını yeni veya mevcut pencerede açar. |
| Doğrudan İsim | `files`, `notepad`, `calc`, `paint`, `browser`, `image`, `music`, `video`, `taskmgr`, `settings`, `cmd`, `pkgmgr` | Uygulama adı doğrudan komut olarak yazıldığında ilgili grafik arayüz penceresi odaklanır/açılır. |

---


## 🎮 Masaüstü Kullanımı ve Kısayollar

- **Başlat Menüsünde Gezinme:** Klavye **Yukarı / Aşağı / Sol / Sağ ok tuşları** ile menüler arasında dolaşın, **Enter** ile seçili uygulamayı çalıştırın veya **fare tekerleğini** çevirerek kaydırın.
- **Açılış Çözünürlüğü Seçimi:** Limine menüsünde minOs çözünürlük girdilerinden birini seçin. Hoş geldin ekranı ve Sistem Ayarları çözünürlük tercihini kaydeder; çalışan framebuffer modunu değiştirmez.
- **Açılış Günlüğü Kaydırma:** Hoş geldin ekranındaki log paneli üzerinde fare tekerleğini kullanarak daha eski veya yeni satırlara gidin.
- **Pencere Boyutlandırma:** Pencerelerin herhangi bir kenarından veya köşesinden basılı tutarak boyutlandırın (tutma alanı 12px'e çıkarılmıştır).
- **Pencere Taşıma (Drag & Drop):** Herhangi bir pencerenin üst başlık çubuğuna sol tıklayıp basılı tutarak ekran üzerinde sürükleyin.
- **Pencere Kontrolleri:**
  - Kırmızı `[x]` butonu: Pencereyi kapatır.
  - Yeşil `[□]` butonu: Pencereyi maksimize eder (tekrar tıklayınca geri yükler). Başlık çubuğuna **çift tıklamak** da aynı işi yapar.
  - Sarı `[-]` butonu: Pencereyi simge durumuna küçülterek görev çubuğuna indirir.
- **Pencere Yerleştirme (Snap):** Başlık çubuğunu ekranın en üstüne sürükleyip bırakırsanız pencere otomatik olarak maksimize edilir.
- **Pencere Değiştirme:** `Alt+Tab` pencereler arasında soldan sağa ilerler; `Shift+Alt+Tab` aynı sırada geriye gider. Alt tuşunu bırakınca seçili pencere öne gelir.
- **Super Kısayolları:** `Super+D` masaüstünü gösterir/geri getirir, `Super+M` tüm pencereleri küçültür, `Super+E` Dosya Yöneticisi'ni açar. Küçültülmüş pencereler de `Alt+Tab` listesinde görünür ve seçilince geri açılır.
- **Sistem Tepsisi (Sağ Alt):** Sırasıyla **klavye rozeti (`TR`/`EN`)**, **tema düğmesi**, **SES** düğmesi ve **canlı saat** bulunur. Saate tıklayınca canlı takvim, **SES**'e tıklayınca ses paneli açılır.
- **Tema Değiştirme:** Görev çubuğundaki renk örneğine, Sistem Ayarları > **Değiştir** düğmesine, **tema palet şeridindeki renk örneklerine** veya masaüstü sağ tık > **Tema Değiştir**'e tıklayın.
- **Bağlama Duyarlı Sağ Tık:** Masaüstünde, Not Defteri'nde, Dosya Yöneticisi'nde veya pencere başlığında özel sağ tık menüleri açılır.
- **Not Defteri Düzenleme Kısayolları:** `Ctrl+C` (Kopyala), `Ctrl+X` (Kes), `Ctrl+V` (Yapıştır), `Ctrl+A` (Tümünü Seç), `Ctrl+Z` (Geri Al), `Ctrl+Y` (İleri Al), `Delete` (İleri sil), `Home`/`End` (satır başı/sonu) ve `Shift+Ok tuşları` (seçim genişletme).
- **Not Defteri Menüsü:** **Dosya** menüsünden yeni belge, açma, kaydetme ve temizleme; **Düzen** menüsünden kesme, kopyalama, yapıştırma ve tümünü seçme işlemleri yapılır.
- **Çizim Tahtası Kullanımı:** Araç çubuğundan renk, **Silgi**, **Aç** veya **Kaydet** seçin; tuval üzerinde sol tuşu basılı tutarak çizin. `C` tuşu tuvali temizler, `E` tuşu silgiyi açıp kapatır. Çizimler 24-bit BMP olarak `/root/Documents/cizim.bmp` konumuna kaydedilir.
- **Resim Görüntüleyici:** `+`/`=` yakınlaştırır, `-` uzaklaştırır, `0` varsayılan ölçeğe döner. Fare tekerleği de yakınlaştırma/uzaklaştırma için kullanılabilir; geçerli oran arayüzde gösterilir.
- **Medya Tuşları:** Klavyedeki ses açma, ses kısma, susturma, oynat/duraklat, önceki parça ve sonraki parça tuşları çalışır. Ses düzeyi `%5` adımlarla değişir.

---


## 📂 Proje Dizin Yapısı

```
minos/
├── src/
│   ├── arch/x86_64/         # SSE, IDT kesmeleri ve PIC maskeleme
│   ├── drivers/             # video (framebuffer), PS/2 fare & klavye, RTC (CMOS) saat,
│   │                        #   e1000 + TCP/IP yığını (netstack) + soket API (socket.rs) + HTTP (http.rs),
│   │                        #   TLS 1.2 istemcisi (tls.rs) + kripto ilkelleri (crypto/: SHA-256,
│   │                        #   HMAC, AES, GCM, RSA, X.509),
│   │                        #   HDA ses, ATA/ATAPI (PIO), USB durum katmanı ve xHCI HID
│   │                        #   klavye/fare sürücüsü (usb_hid.rs)
│   ├── gui/                 # MinGUI pencere yöneticisi, font motoru, tema motoru (theme.rs),
│   │                        #   kaydırma çubuğu ve pano (clipboard)
│   ├── apps/                # 14 Masaüstü uygulaması (Not Defteri, Dosya Yöneticisi, Paket Yöneticisi, Görev Yöneticisi, 3D Gösterici...)
│   ├── desktop/             # Masaüstü oturumu (session.rs: DesktopState kurulumu + ana olay döngüsü) ve yardımcılar
│   ├── mm/                  # Fiziksel bellek yöneticisi (PMM), süreç tablosu ve ELF yükleyici
│   ├── fs/                  # Sanal dosya sistemi (vfs.rs) + disk okuyucuları (diskfs.rs: FAT32 + ISO9660),
│   │                        #   Ext2 read-write (ext2.rs), ext4/exFAT biçimlendirme (ext4.rs, exfat.rs)
│   │                        #   + MBR/GPT bölümleme (part.rs)
│   └── main.rs              # Çekirdek giriş noktası (_start): Limine istekleri, donanım kurulumu ve
│                            #   DesktopState oluşturup masaüstü oturumunu (desktop/session.rs) başlatır
├── boot/                    # Limine bootloader yapılandırma dosyaları (limine.cfg)
├── docs/screenshots/        # Masaüstü ve başlat menüsü ekran görüntüleri
├── build_iso.ps1            # El Torito yamalı hibrit ISO oluşturucu
├── run.ps1                  # Tek tıkla derleme ve QEMU başlatma betiği (-Debug, -BuildOnly, -UsbDisks, -UsbDiskReadWrite, -CpuCount, -Memory, -DiskImage)
├── build.rs                 # fontdue ile TTF → bitmap font atlası üretimi
└── x86_64-minos.json        # Hedef x86_64 Long Mode tanımlama dosyası
```

---


## 📊 Kod İstatistikleri (Code Statistics)

> Son güncelleme: 20 Ekim 2026 (v1.7.0)
| Ölçüt | Değer |
| :--- | :--- |
| **Toplam Rust Kaynak Dosyası** | 92 dosya (`src/**/*.rs`) |
| **Toplam Kod Satırı (ham)** | 41.703 satır (`src/**/*.rs`) |
| **Etkin Kod Satırı** (boş ve satır başı yorumları hariç, yaklaşık) | 35.234 satır |
| **Dil** | Rust (Nightly, `no_std`) |
| **Derleme Hedefi** | `x86_64-minos` (özel bare-metal) |
| **Yerleşik Uygulama Sayısı** | 14 masaüstü uygulaması + Pencere Yöneticisi |
| **Kernel / ISO Boyutu** | ~1.85 MB Çekirdek / ~5.85 MB Hibrit Bootable ISO |

---


