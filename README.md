# 🐱 minOs - Sarman Edition

[![Rust](https://img.shields.io/badge/Language-Rust%20Nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Display](https://img.shields.io/badge/Resolution-1280x720%2032--bit%20RGB-purple.svg)]()
[![Min RAM](https://img.shields.io/badge/Min%20RAM-4%20MB-red.svg)]()
[![Min CPU](https://img.shields.io/badge/Min%20CPU-x86__64%20SSE2-red.svg)]()
[![Lines of Code](https://img.shields.io/badge/Kod%20Satırı-41.703%20(Rust)-lightgrey.svg)]()
[![GitHub Release](https://img.shields.io/badge/Release-v1.7.0-brightgreen.svg)](https://github.com/tbagriyanik/minos/releases)

**minOs (Sarman Edition) v1.7.0**, çekirdek tarafında `no_std` ve `no_main` kullanan, Rust ile geliştirilmiş bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Derleme sırasında `fontdue` kullanılarak TTF yazı tiplerinden bitmap atlasları üretilir; çekirdek bu atlasları grafik arayüzünde kullanır.

---

## Yenilikler (v1.7.0)

- **Varsayılan Donanım Sürücü Profili:** Normal Live ve Installed açılışlarında tam sürücü profili kullanılır; sürücüsüz açılış için Limine menüsündeki Safe Mode seçeneği korunur.
- **Daha Okunabilir Boot Günlüğü:** Metin konsolu her tamamlanan satırı ekrana aktarır ve zaman tabanı hazır olduktan sonra sonraki satıra geçmeden önce 45 ms bekler. Böylece açılış tanılama mesajları daha rahat izlenir; zaman tabanı hazır olmadan yazılan erken mesajlar geciktirilmez.
- **Tarayıcı Sık Kullanılanlar (Yer İmleri Çubuğu):** Web tarayıcısına interaktif yer imleri çubuğu eklendi; http://httpforever.com/ saf HTTP test sitesi, minos://cats, minos://net, minos://history ve minos://demo hızlı erişim butonları ile minos://bookmarks sayfası entegre edildi.

## 📥 Hazır ISO İndirme (Download)

minOs'u derlemeden doğrudan QEMU, VirtualBox (64bit) veya gerçek donanımda test etmek için hazır ISO imajı:

🔗 **Repo Üzerinden Doğrudan İndir:**
[https://github.com/tbagriyanik/minos/blob/main/minos.iso](https://github.com/tbagriyanik/minos/blob/main/minos.iso)

---
## 🎥 Tanıtım Videosu

[![minOs tanıtım videosunu izleyin](https://img.youtube.com/vi/fB1MQPZTMEg/hqdefault.jpg)](https://www.youtube.com/watch?v=fB1MQPZTMEg)

▶️ [Videoyu YouTube'da izleyin](https://youtu.be/fB1MQPZTMEg)
---

## 📸 Ekran Görüntüleri (Screenshots)

### Welcome Ekranı
![Welcome](docs/screenshots/welcome.png)

### Tema 1
![Tema 1](docs/screenshots/tema1.png)
  - **Sık Kullanılanlar / Yer İmleri Çubuğu (Bookmarks Bar):** Adres çubuğunun hemen altında yer alan hızlı erişim çubuğu. http://httpforever.com/ saf HTTP test sitesi, minos://cats (KediWiki), minos://net (Ağ Durumu), minos://history (Geçmiş) ve minos://demo hızlı erişim düğmeleri olarak eklendi. Ayrıca minos://bookmarks sayfası üzerinden tüm yer imleri açıklamalarıyla birlikte listelenir ve tek tıkla ziyaret edilebilir.

### Tema 2
![Tema 2](docs/screenshots/tema2.png)

---

## Dokümantasyon

- [Özellikler ve yol haritası](docs/features.md)
- [Komutlar, kısayollar ve proje yapısı](docs/usage.md)
- [Donanım, gereksinimler ve derleme](docs/building.md)
- [Önceki sürüm notları](docs/releases.md)

### Hızlı Başlangıç

```powershell
git clone https://github.com/tbagriyanik/minos.git
cd minos
powershell -ExecutionPolicy Bypass -File .\run.ps1
```
## 📄 Lisans

Bu proje açık kaynaklıdır ve eğitim/araştırma amaçlı geliştirilmektedir.
Katkıda bulunmaktan ve geliştirmeler önermekten çekinmeyin! 🐾
