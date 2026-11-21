# Kullanıcı Rehberi ve Kısayollar

Masaüstü kullanımı, MinShell komutları ve kısayol referansı.

---

## MinShell Komutları

### Dosya ve Dizin Yönetimi

| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `pwd` | `pwd` | Geçerli çalışma dizinini yazar |
| `cd` | `cd <dizin>` | Çalışma dizinini değiştirir |
| `ls` | `ls [dizin]` | Dosya ve klasörleri listeler |
| `cat` | `cat <dosya>` | Metin dosyasının içeriğini yazar |
| `touch` | `touch <dosya>` | Yeni boş dosya oluşturur |
| `write` | `write <dosya> <metin>` | Dosyaya metin yazar |
| `mkdir` | `mkdir <klasor>` | Yeni klasör oluşturur |
| `rm` | `rm <hedef>` | Dosya veya dizini siler |
| `disks` / `df` | `disks` veya `df` | Bağlı sürücüleri listeler |

### Sistem Bilgisi ve Bellek

| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `mem` | `mem` | Toplam ve boş bellek miktarını gösterir |
| `meminfo` | `meminfo` | Genişletilmiş bellek raporu (PMM, heap, COW paylaşımı, sıkıştırılmış depo) |
| `ps` | `ps` | Aktif sistem süreçlerini listeler |
| `date` | `date` | RTC üzerinden tarih ve saati gösterir |
| `uname` | `uname` | Çekirdek sürümünü ve mimariyi yazar |
| `whoami` | `whoami` | Aktif kullanıcı yetkisini gösterir |
| `theme` | `theme` | Masaüstü temasını sıradaki temaya geçirir |
| `uptime` | `uptime` | Çalışma süresini gösterir |
| `fetch` / `neofetch` | `fetch` veya `neofetch` | Renkli ASCII maskotu ile sistem özetini sunar |
| `clear` | `clear` | Terminal ekranını temizler |
| `help` | `help` | Kullanılabilir komutların özetini görüntüler |

### Ağ Teşhisi ve Araçları

| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `ipconfig` | `ipconfig` | `eth0` arabirimi bilgilerini raporlar |
| `ping` | `ping <IPv4>` | ICMP Echo Request gönderir |
| `ping6` | `ping6 <IPv6>` | ICMPv6 Echo Request gönderir |
| `dns` | `dns <alan_adı>` | DNS sorgusu iletir |
| `curl` | `curl http://<host>[:port][/yol]` | HTTP GET indirir |

### Disk ve Çekirdek Komutları

| Komut | Kullanım | Açıklama |
| :--- | :--- | :--- |
| `format` / `mkfs` | `format <fat32\|ext4\|exfat\|ext2> <0..3>` | Sabit diski biçimlendirir |
| `minPak` / `pkg` | `minPak <list\|search\|install\|remove>` | Paket yönetimi |
| `exec` | `exec <dosya_yolu> [argümanlar]` | ELF/ikili dosyayı Ring 3'te çalıştırır |

### Uygulama Başlatma

| Komut | Alternatifler | Açıklama |
| :--- | :--- | :--- |
| `open <uygulama>` | `run <uygulama>` | Uygulamayı yeni veya mevcut pencerede açar |
| Doğrudan İsim | `files`, `notepad`, `calc`, `paint`, `browser`, `image`, `music`, `video`, `taskmgr`, `settings`, `cmd`, `pkgmgr` | Uygulama adı doğrudan komut olarak yazılır |

---

## Masaüstü Kullanımı ve Kısayollar

- **Başlat Menüsü:** Yukarı / Aşağı / Sol / Sağ ok tuşlarıyla gezinin, Enter ile çalıştırın, fare tekerleğiyle kaydırın.
- **Açılış Çözünürlüğü:** Limine menüsünden `/Display Resolutions` altından seçin veya Sistem Ayarları'dan kaydedin.
- **Pencere Boyutlandırma:** Pencere kenarından veya köşesinden basılı tutarak boyutlandırın (12px tutma alanı).
- **Pencere Taşıma:** Üst başlık çubuğuna sol tıklayıp sürükleyin.
- **Pencere Kontrolleri:**
  - Kırmızı `[x]`: Kapatır
  - Yeşil `[□]`: Maksimize eder (tekrar tıklayınca geri yükler; çift tıkla da aynı)
  - Sarı `[-]`: Simge durumuna küçültür
- **Pencere Yerleştirme (Snap):** Başlık çubuğunu ekran üstüne sürükleyip bırakın (otomatik maksimize).
- **Pencere Değiştirme:** `Alt+Tab` (ileri), `Shift+Alt+Tab` (geri).
- **Super Kısayolları:** `Super+D` masaüstü, `Super+M` tüm pencereleri küçültür, `Super+E` Dosya Yöneticisi, `Super+R` Çalıştır iletişim kutusu.
- **Workspace Kısayolları:** `Super+←` / `Super+→` sanal masaüstleri arasında geçiş yapar; `Super+Shift+←` / `Super+Shift+→` odaklı pencereyi önceki/sonraki workspace'e taşır.
- **Diğer Sistem Kısayolları:** `Alt+F4` pencereyi kapatır, `F2` yeniden adlandırır, `F5` yeniler, `F12`/`PrintScreen` ekran görüntüsü alır, `Ctrl+Alt+Delete` Görev Yöneticisi'ni açar, `Ctrl+Alt+NumLock` fare tuşları (MouseKeys) modunu açıp kapatır.
- **Sistem Tepsisi:** Klavye rozeti (`TR`/`EN`), tema düğmesi, SES düğmesi, canlı saat.
- **Tema Değiştirme:** Görev çubuğundaki renk örneği, Sistem Ayarları > Değiştir veya tema palet şeridinden yapılır.
- **Masaüstü Sağ Tık Menüsü:** Boş alanda **Yeni** alt menüsüyle dosya/klasör oluşturabilir, masaüstünü **Yenile**'yebilir veya **Görev Yöneticisi**'ni açabilirsiniz.
- **Not Defteri Kısayolları:** `Ctrl+C` (Kopyala), `Ctrl+X` (Kes), `Ctrl+V` (Yapıştır), `Ctrl+A` (Tümünü Seç), `Ctrl+Z` (Geri Al), `Ctrl+Y` (İleri Al), `Delete`, `Home`/`End`, `Shift+Ok tuşları`.
- **Çizim Tahtası:** Üst şeritten **Dosya** menüsü, **Aç**/**Kaydet**/**Geri Al**/**Yinele**/**Temizle** ve fırça boyutu (`-`/`+`), alt şeritten hızlı renkler, **Renkler** seçici, **Fırça**, **Silgi**, şekil araçları (Çizgi/Dikdörtgen/Daire) ve zoom (`-`/`+`) kullanılır. Sol tuşu basılı tutarak çizin; hızlı hareketlerde çizgi otomatik enterpolasyonla kesintisiz kalır. **Sağ tuşu basılı tutup sürüklemek** de silgi darbesi yapar. `Ctrl+N` yeni tuval, `Ctrl+O` BMP açar, `Ctrl+S` kaydeder, `C` temizler, `E` silgiyi açar/kapar. 24-bit BMP olarak `/root/Documents/cizim.bmp` konumuna kaydedilir.
- **Resim Görüntüleyici:** `+`/`=` yakınlaştırır, `-` uzaklaştırır, `0` varsayılan ölçeğe döner.
- **Resim Görüntüleyici Arka Planı:** Açık resimde **Masaüstüne döşe** düğmesine basarak görüntüyü masaüstü arka planı yapın.
- **Masaüstü Görselleri:** Sistem Ayarları'ndaki **Masaüstü 1**, **Masaüstü 2** ve **Masaüstü 3** seçenekleri yeni PNG görsellerini kullanır.
- **WAV Oynatma:** Dosya Yöneticisi'nde `.wav` dosyasına çift tıklamak, seçilen dosyayı doğrudan oynatır.
- **Örnek Medyalar:** ISO içindeki örnek dosyalar çalışan sistemde `/media/cdrom/root/Media` altında bulunur. PNG/JPEG/BMP dosyaları Resim Gösterici'de, WAV dosyaları Müzik Çalar'da açılabilir.
- **Medya Tuşları:** Ses açma, kısma, susturma, oynat/duraklat, önceki/sonraki parça.
- **Yenileme (`F5`):** Odaklı uygulamanın içeriğini tazeler.
- **Video Oynatıcı:** `Oynat/Durdur` animasyonu ve zaman çizelgesini duraklatır/devam ettirir; `Durdur` başa sarar.
- **Yumuşak Kaydırma:** Fare tekerleği tüm uygulamalarda (tarayıcı kendi sistemini kullanır) kısa bir ease-out animasyonuyla kaydırır; kaydırma çubuğu tıklaması/sürüklemesi animasyonu anında iptal eder.
- **Açılış Animasyonları:** Pencereler, Başlat Menüsü, sağ tık menüleri, Alt+Tab geçiş penceresi ve takvim/ses açılır pencereleri kısa kayma animasyonuyla belirir; toast bildirimler sağdan kayarak girer/çıkar.
- **İpuçları (Tooltip):** Görev çubuğu ve sistem tepsisindeki öğelerin, ayrıca pencere başlığındaki düğmelerin (kapat/büyüt/küçült) ve görev çubuğu uygulama sekmelerinin üzerine ~0,7 sn bekleyince açıklama balonu gösterilir. Kaydırma çubuğu tutamağının üzerine gelince de vurgulanır.
- **Arayüz Animasyonları Anahtarı:** Sistem Ayarları > Ekran sekmesindeki **Arayüz animasyonları** kapatıldığında tüm hareket animasyonları (açılışlar, toast kayması ve yumuşak kaydırma dahil) anında tamamlanır.
- **Saydam Pencereler:** Sistem Ayarları > Ekran sekmesindeki **Saydam pencereler** açıkken pencere gövdesi yarı saydam olur; altındaki masaüstü ve pencereler görünür. (Uygulama içerik zeminleri opak kalır.)

---

## Açılış Hata ve Kurtarma Konsolu (`recovery#`)

Çekirdek istisnaları (`#DE`, `#UD`, `#DF`, `#SS`, `#GP`, `#PF`) ve kritik sürücü arızalarında etkileşimli metin konsoluna yönlendirilir.

| Komut | Alternatifler | Açıklama |
| :--- | :--- | :--- |
| `[Enter]` | `restart`, `reboot`, `r` | Sistemi güvenli şekilde yeniden başlatır |
| `halt` | `shutdown`, `h` | İşlemciyi durdurur (`cli` ve `hlt`) |
| `help` | `?` | Kullanılabilir komutları listeler |

> Komutlar yalnızca **Enter** tuşuna basıldığında yürütülür.
