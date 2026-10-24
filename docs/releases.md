# Önceki Sürüm Notları

README'den ayrılan v1.6.0 ve v1.5.0 değişiklik listeleri.

## Yenilikler (v1.6.0)

- **🖥️ UEFI Önyükleme Tanılama Konsolu:** Limine framebuffer doğrulandıktan sonra bellek, sürücü ve sistem başlatma adımları metin olarak ekrana yazılır. Hata durumunda son loglar korunur; kritik hatada GUI başlatılmaz.
- **🛑 Panik Sonrası Otomatik Reset Yok:** Kernel panic mesajı ekranda bırakılır ve sistem bekler; hata okunmadan yeniden başlatılmaz.
- **🧵 Laptop için Tek CPU Açılışı:** Gerçek donanımda SMP/APIC başlatmasının reset oluşturduğu test yolunda AP başlatımı ve APIC timer devre dışı; açılış BSP üzerinde TSC tabanlı zamanla sürer. Çok çekirdek desteği bu açılış yapılandırmasında etkin değildir.
- **🖱️ PS/2 Fare ve Touchpad Akışı:** PS/2 uyumlu fare/touchpad için çözünürlük ve örnekleme hızı ayarlandı; eksik paket baytında ayrıştırma durumu sonraki yoklamaya kadar korunur. Firmware'in 8042/PS/2 uyumluluğu sunması gerekir.
- **🔌 Yerel USB Klavye ve Fare (xHCI):** `src/drivers/usb_hid.rs`, PCI üzerinde bulunan xHCI (USB 3.x) denetleyicilerini başlatır, bağlı aygıtları numaralandırır ve HID Boot Protocol kullanan klavye/fare raporlarını yoklama ile okur. USB 1.1/2.0 klavye ve fareler xHCI'nin USB 2.0 portları üzerinden çalışır. Raporlar mevcut PS/2 tarama kodu/durum katmanına çevrildiği için masaüstü, uygulamalar ve kısayollar değişmeden çalışır. UHCI/OHCI/EHCI denetleyicileri için yerel sürücü henüz yoktur; bu donanımda firmware'in USB Legacy/8042 emülasyonu yedek yol olarak kullanılır.

- **🌐 Tam Ağ Yığını & DNS Çözümleme İyileştirmeleri:**
  - RFC 1035 uyumlu DNS soru ve yanıt ayrıştırma mekanizması; etiket (label) ve sıkıştırılmış işaretçi (`0xC0`) desteği ile harici alan adları (`example.com`, `google.com` vb.) anında çözümlenir.
  - Harici internet yönlendirmesinde dinamik ağ geçidi (Gateway) MAC geri düşüşü (ARP fallback) eklendi; yerel alt ağ dışındaki IP paketlerinin düşmesi önlendi.
  - Paralel DNS sorgulama (Yerel DNS + Cloudflare 1.1.1.1 fallback) desteği.
  - Terminalde `dns <alan_adı>` komutu ile canlı IP çözümleme ve renkli adres çıktısı.
  - HTTP bağlantı zaman aşımı süresi 8 saniyeye çıkarılarak yavaş internet hatlarında stabilite artırıldı.
- **⚙️ Sistem Ayarları Ağ Tanılama Sekmesi:** Ağ sekmesi altına canlı ARP, ICMP (Ping) ve TCP bağlantı sınama testleri eklendi.
- **🖥️ Terminal Tema Kontrast İyileştirmesi:** Açık temalarda (Daylight vb.) terminal yazı renkleri koyulaştırılarak okunabilirlik artırıldı.


## Yenilikler (v1.5.0)

- **📦 Yeni Paket Yöneticisi Komutu (`minPak`):** Terminalde `minPak` komutu ile paket yönetimi; `minPak list` ile `/bin` altındaki kurulu paketleri listeleme, `minPak search <paket>` ile ad/açıklamada arama, `minPak install <paket|url>` ile HTTP üzerinden paket indirme ve kurma (ağ yoksa yerleşik paketi kurar), `minPak remove <paket>` ile kurulu paketi silme. Alt komutsuz çağrıda yardım menüsü gösterilir (eski `pkg`/`minpkg` adları da çalışmaya devam eder).
- **🌐 Gelişmiş Web Tarayıcısı & Ağ Yığını (HTTP/1.1):**
  - **HTTP/1.1 Protokol Desteği:** `Connection: close`, standart `Host` ve `User-Agent: minOs/1.5` başlıkları.
  - **Genişletilmiş Ağ Tamponları:** 64 KB TCP alma tamponu (`TCP_DATA`) ve 64 KB dinamik yanıt tamponu (`response_data`). 256 bayt URL yolu ve 128 bayt ana makine adı desteği.
  - **Dikey Kaydırma (Scrolling):** Sayfa satır sayısına göre dinamik pencere içerik yüksekliği (`content_height`), pencere kaydırma çubuğu ve fare tekerleği (wheel scroll) entegrasyonu.
  - **HTTP 3xx Yönlendirme (Redirect) Takibi:** 301, 302, 303, 307 ve 308 durum kodları ve `Location:` başlığı çözümleyicisi (azami 3 atlama).
  - **POST Metodu ve Form Gönderimi:** `navigate_post()` ile `application/x-www-form-urlencoded` ve dinamik `Content-Length` başlığı ile veri gönderebilme.
  - **Çerez (Cookie) Yönetimi:** `Set-Cookie:` başlıklarının otomatik kaydedilmesi ve domain/path eşleşmesine göre isteklerde `Cookie:` başlığının gönderilmesi.
  - **Parçalı Gönderim Çözümleme (Chunked Transfer Decoding):** `Transfer-Encoding: chunked` yanıtlarının yerinde çözülerek temiz HTML/metin içeriğe dönüştürülmesi.
  - **15 Saniyelik Ağ Zaman Aşımı:** Yavaş bağlantılar ve büyük web sayfaları için kararlı istek tamamlama.

- **📁 Çoklu Dosya Seçimi ve Toplu İşlemler:** Masaüstünde ve Dosya Yöneticisi'nde `Ctrl` ve `Shift` tuşlarıyla birden fazla dosya/klasör seçebilme; seçilen tüm öğeleri toplu olarak kopyalama (`Ctrl+C`), kesme (`Ctrl+X`), panoya alma ve kalıcı/Geri Dönüşüm Kutusu'na silme (`Delete`) desteği.
- **🔄 Masaüstü ↔ Dosya Yöneticisi İki Yönlü Sürükle-Bırak (Drag & Drop):** Masaüstünden seçilen tek veya çoklu dosyaları Dosya Yöneticisi pencerelerine (açık olan dizine, kenar çubuğundaki hızlı konumlara veya klasörlere) ya da Dosya Yöneticisi'nden masaüstü boşluğuna sürükleyip bırakarak taşıma ve kopyalama (`Ctrl` basılıyken kopyalama, varsayılanda taşıma). İşlem sonrası her iki arayüzde anında dinamik VFS senkronizasyonu.
- **🏁 Karbon Doku (Carbon Fiber) Masaüstü Duvar Kağıdı:** Gerçekçi 2x2 Twill Weave (çapraz örgülü) karbon fiber dokusu; yatay ve dikey antrasit lifler, aydınlık/gölge derinlik geçişleri ve pürüzsüz arka plan önbellek (`WALLPAPER_CACHE`) entegrasyonu.
- **⌨️ Nümerik Enter ve Gelişmiş Klavye Kısayolları:** Numpad (sayı tuş takımı) üzerindeki `Enter` tuşunun Terminal (cmd) ve tüm diyaloglarda standart `Enter` ile birebir aynı komut yürütme işlevini görmesi.
- **🪟 Super Tuş Kombinasyonu İyileştirmeleri:** `Super + D` gibi sistem kısayollarında Başlat menüsünün yanlışlıkla açılmasının engellenmesi ve masaüstünü göster işlevinin kararlı çalışması.
- **✨ Sağ Tık Bağlam Menüsünde "Yeni ▶" Alt Menüsü:** Masaüstü ve Dosya Yöneticisi sağ tık menüsüne modern işletim sistemlerindeki gibi yana açılır hiyerarşik "Yeni ▶" (New ▶) alt menüsü eklendi. Metin Belgesi (`.txt`), Klasör, Çizim / Resim (`.bmp`) ve Boş Belge (`.dat`) doğrudan sağ tık menüsünden anında oluşturulabilir.
- **📸 Evrensel Ekran Görüntüsü Alma (PrtScn / F12):** Herhangi bir ekranda `PrtScn` veya `F12` tuşuna basıldığında tüm ekran 24-bit uncompressed `.bmp` formatında `/desktop/ekran_N.bmp` dosyasına anında kaydedilir ve sesli bildirim verilir.
- **⚡ Hızlı Çalıştır / Run Program Diyaloğu (Super+R):** `Super + R` tuş kombinasyonu ile açılan şık ve gölgeli modal pencereden `cmd`, `not`, `ayar`, `task`, `calc`, `web`, `files` gibi komutlarla hızlı uygulama başlatma.
- **📋 Uygulamalar Arası Pano (Clipboard) ve Terminal Entegrasyonu:** Terminal üzerinde `Ctrl+C` ile satırı panoya kopyalama ve `Ctrl+V` ile panodaki metni terminale yapıştırma desteği (Notepad, Calc, Browser ile ortak paylaşılan pano).
- **⏱️ Yeni Terminal Komutları (`uptime` & `fetch` / `neofetch`):** Sistemin çalışma süresini milisaniye hassasiyetinde gösteren `uptime` ve renkli kedi maskotuyla mikroçekirdek, RAM ve VFS durumunu özetleyen `fetch` / `neofetch` komutları eklendi.
- **🔊 Başlangıç Melodisi ve Sistem Ses Efektleri:** Masaüstüne ilk girişte çalan zarif 3 tonlu karşılama melodisi (`play_chime()`) ve ekran görüntüsü gibi kullanıcı eylemlerine yanıt veren kısa sesli bildirimler (`play_beep()`).
- **🔒 HTTPS/TLS Bağlantı Katmanı (TLS 1.2):** `src/drivers/tls.rs` ile tam TLS 1.2 istemci el sıkışması: ClientHello (SNI), ServerHello, Certificate (X.509 RSA anahtar çıkarma), ServerHelloDone, ClientKeyExchange (RSA PKCS#1 v1.5), ChangeCipherSpec, Finished ve şifreli Application Data işleme. AES-128-GCM şifreleme, SHA-256 PRF ve anahtar türetme dahil; `src/drivers/crypto/` içinde SHA-256, HMAC-SHA256, AES-128/256, GCM ve RSA/PKCS#1Primitive’leri no_std olarak el yazısıyla uygulanmıştır. Web Tarayıcıda `https://` adresleri (443 portu) şifresi çözümlenip HTTP yanıtı olarak görüntülenebilir.
- **💾 ext4 ve exFAT Disk Biçimlendirme Desteği:** Yeni `src/fs/ext4.rs` (ext2 uyumlu superblock + EXT4 magic 0xEF53) ve `src/fs/exfat.rs` (standart Microsoft exFAT VBR + 11 sektör VBR dizisi, boot checksum) modülleri. Terminalde `format ext4|exfat` komutu ve Kurulum Sihirbazı'nda dosya sistemi seçim radyo düğmeleri (FAT32 / ext4 / exFAT).
- **🌙 Gece Işığı (Night Light):** Sistem Ayarları'ndan açılıp kapatılabilen, tüm ekranı kademeli ısıtan (55-100 seviye) ekran filtresi; framebuffer düzeyinde piksel bazlı uygulama.
- **📑 Özellikler (Properties) Modal Penceresi:** Dosya Yöneticisi'nde seçili öğe için ad, tür, boyut, konum ve tam yol bilgisi gösteren modal diyalog; **X** veya **Tamam** düğmesi, **ESC**/**Enter** ve diyalog dışına tıklama ile kapatma; fare imleci modalin üstünde çizilir.
- **📦 ISO Boyutu Optimizasyonu (21 MB ➔ 1.89 MB):** Ham bitmap'lerin temizlenmesi ve linker script (`linker.ld`) `.got` kesitinin yeniden düzenlenmesiyle gereksiz 11 MB BSS boşluk padding'i kaldırılarak ISO dosya boyutu 1.89 MB'a düşürüldü.
- **Çok Çekirdekli (SMP) Görev Yöneticisi:** Görev Yöneticisi Performans sekmesinde dinamik çoklu çekirdek izleme desteği. Genel CPU kullanımının yanı sıra her bir CPU çekirdeği (C0, C1, ...) için ayrı grafik barları, yüzde göstergeleri ve RAM kullanım grafiği eklendi. Metinlerin ve grafiklerin çakışması önlendi.
- **Yarı Saydam (Alpha Blended) Masaüstü Öğeleri:** Masaüstü simge kartları ve sol taraftaki hızlı erişim ("Bilgisayarım", "Notlar" vb.) yazı etiketlerinin arka planları %50 saydam (`alpha: 128`) yapılarak duvar kağıdının pürüzsüzce arkadan görünmesi sağlandı.
- **Görev Yöneticisi Düğme ve Menü Düzeni:** "Program Aç" ve "Sonlandır" düğmeleri yalnızca ilk sekmede ("Uygulamalar") gösterilecek şekilde düzenlendi. "Program Aç" menüsündeki uygulamalar Türkçe alfabetik sıraya dizildi ve menüden Görev Yöneticisi'nin kendisi kaldırıldı.
- **Pencere Altına Sabitlenen Durum Çubuğu:** Görev Yöneticisi durum çubuğu pencerenin en alt kenarına tam yapıştırıldı ve süreç listesi pencere boyutuna göre sınırlandırıldı.
- **Yenilenen Pencere Restore (Önceki Boyut) Simgesi:** Pencereler tam ekran yapıldığında gösterilen "Önceki Boyut" (Restore) düğme simgesi, modern çift üst üste binen pencere ikonuyla yenilendi.
- **Seçilebilir masaüstü arka planları:** Sistem Ayarları > Ekran sekmesinden tema rengi veya tek renk seçenekleri seçilebilir.
- **Anında önizleme:** Duvar kağıdı seçimi yapıldığında arka plan hemen uygulanır; seçim çevreli kart ve yeşil kenarlıkla gösterilir.
- **Kalıcı ayarlar:** Arka plan seçimi `settings.cfg` (`wallpaper`, `wallcolor`) üzerinden VFS ve diske kaydedilir, yeniden açılışta geri yüklenir.
- **Ayarlar ve Görev Yöneticisi standart düzeni:** Tüm sekme ve pencerelerde standartlaştırılmış dikey margin ve padding aralıkları ile okunaklı, çakışmayan ferah UI düzeni sağlandı.

---

