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
- **Super Kısayolları:** `Super+D` masaüstü, `Super+M` tüm pencereleri küçültür, `Super+E` Dosya Yöneticisi.
- **Sistem Tepsisi:** Klavye rozeti (`TR`/`EN`), tema düğmesi, SES düğmesi, canlı saat.
- **Tema Değiştirme:** Görev çubuğundaki renk örneği, Sistem Ayarları > Değiştir, tema palet şeridi veya masaüstü sağ tık > Tema Değiştir.
- **Not Defteri Kısayolları:** `Ctrl+C` (Kopyala), `Ctrl+X` (Kes), `Ctrl+V` (Yapıştır), `Ctrl+A` (Tümünü Seç), `Ctrl+Z` (Geri Al), `Ctrl+Y` (İleri Al), `Delete`, `Home`/`End`, `Shift+Ok tuşları`.
- **Çizim Tahtası:** Renk, Silgi, Aç veya Kaydet seçin; sol tuşu basılı tutarak çizin. `C` temizler, `E` silgiyi açar/kapar. 24-bit BMP olarak `/root/Documents/cizim.bmp` konumuna kaydedilir.
- **Resim Görüntüleyici:** `+`/`=` yakınlaştırır, `-` uzaklaştırır, `0` varsayılan ölçeğe döner.
- **Medya Tuşları:** Ses açma, kısma, susturma, oynat/duraklat, önceki/sonraki parça.
- **Yenileme (`F5`):** Odaklı uygulamanın içeriğini tazeler.
- **Video Oynatıcı:** `Oynat/Durdur` animasyonu ve zaman çizelgesini duraklatır/devam ettirir; `Durdur` başa sarar.

---

## Açılış Hata ve Kurtarma Konsolu (`recovery#`)

Çekirdek istisnaları (`#DE`, `#UD`, `#DF`, `#SS`, `#GP`, `#PF`) ve kritik sürücü arızalarında etkileşimli metin konsoluna yönlendirilir.

| Komut | Alternatifler | Açıklama |
| :--- | :--- | :--- |
| `[Enter]` | `restart`, `reboot`, `r` | Sistemi güvenli şekilde yeniden başlatır |
| `halt` | `shutdown`, `h` | İşlemciyi durdurur (`cli` ve `hlt`) |
| `help` | `?` | Kullanılabilir komutları listeler |

> Komutlar yalnızca **Enter** tuşuna basıldığında yürütülür.
