# 🐱 minOs — Sarman Edition

[Türkçe](README.md) | [English](README.en.md)

![minOs Sarman Edition](public/minOs%20Sarman%20Edition.png)

[![Rust](https://img.shields.io/badge/Language-Rust%20nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64-blue.svg)]()
[![Lines of Code](https://img.shields.io/badge/Lines%20of%20Code-206%2C213%20(Rust)-lightgrey.svg)](https://github.com/tbagriyanik/minos)
[![Host Tests](https://img.shields.io/badge/Host%20Tests-547%20passing-brightgreen.svg)](https://github.com/tbagriyanik/minos)
[![Version](https://img.shields.io/badge/Version-v2.6.0-brightgreen.svg)](https://github.com/tbagriyanik/minos/releases)

minOs, Rust ile geliştirilen bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Çekirdek `no_std` ve `no_main` kullanır; grafik masaüstü ve yerleşik uygulamalar sunar.

## Güncel bilgiler · v2.6.0

- Çizim Tahtası (Paint) arayüzü baştan tasarlandı: iki ölçeklenebilir araç şeridi, görünen ve hover/basma animasyonlu düğmeler (Aç/Kaydet/Geri Al/Yinele/Temizle/Fırça/Silgi/şekiller), renk seçici açılır penceresi, fırça ucu önizlemesi, sağ tuşla sürükleyerek silme ve Ctrl+N/Ctrl+O/Ctrl+S kısayolları; hızlı sürüklemelerde çizgi ve silme artık enterpolasyonla kesintisiz.
- Uygulama örnekleri tembel yüklenir: kabuk uygulamaları (Dosya Yöneticisi, Ayarlar, Tarayıcı, Müzik, Görev Yöneticisi) her zaman bellekte kalır; diğer uygulamalar ilk açılışta `AppInstances::load` ile yüklenir ve son pencere kapanınca `unload` ile bellekten düşürülür — boşta duran uygulamalar RAM kaplamaz.
- Sürücü görünürlüğü ve hata ayıklama: ortak durum modeliyle her donanım sürücüsü `Hazır` / `Kullanılamıyor` / `Hata` olarak izlenir; hata kodu ve açıklama seri günlüğe yazılır. Kontrol Paneli'ne **Sürücüler** sekmesi eklendi (durum tablosu + ağ/grafik/ses/depolama ayrıntıları + **Yeniden Tara**); sürücüsü hatalı uygulamalar güvenli biçimde engellenir.
- Dosya Yöneticisi ve Kurulum Sihirbazı'na onay diyalogları eklendi: yeni dosya/klasör oluşturma, yeniden adlandırma ve silme (Geri Dönüşüm Kutusu'na taşıma veya kalıcı silme) artık adlandırmalı onay penceresiyle onaylatılıyor; Enter onaylar, Esc vazgeçer. Kurulum sihirbazında `Biçimlendir` ve `Tek Tıkla Kur` adımları da aynı şekilde onay istiyor.
- Donmuş uygulamaların izlenmesi ana döngüden bağımsız çalışır; FPS 3D uygulaması gerçek kare hızı ölçümleri üretir.
- FPS Oyunu başlangıç ve sonuç menülerini içerir; temel form kontrollerinde Tab ile gezinme, Enter ile onaylama ve Esc ile vazgeçme desteği bulunur (Hoş Geldin ekranı dahil).
- Render bütçesi aşılırsa tamamlanmamış kare ekranda bırakılmaz.
- TLS/X.509 doğrulaması yaprak ve ara sertifika tarihlerini, CA BasicConstraints/pathLen kısıtlarını ve varsa KeyUsage/EKU uzantılarını denetler; testlerde doğrulama kestirmesi kaldırıldı. Host paketi 547 test içerir.
- Çekirdek loglama artık seviyeli ve filtrelenebilir: `trace/debug/info/warn/error` seviyeleri ile `boot`, `driver` ve `net` hedeflerine ayrılan kayıtlar `[LEVEL][hedef]` önekiyle seri konsola yazılır. Varsayılanlar Limine cmdline `log=` belirtecinden (`log=debug`, `log=info,net:trace` gibi), çalışma anı değişiklikleri seri konsoldan `log ...` komutlarıyla yapılır; CI'a çekirdek hedefi için `cargo clippy -D warnings` kapısı eklendi.
- Web tarayıcısı modern bir tarayıcıya dönüştü: HTTP/1.1 + TLS 1.3'in yanına HTTP/2 ve HTTP/3 (QUIC) eklendi; HTML5'te satır-içi SVG rasterizasyonu, `<details>`/`<dialog>`/`<template>`/`<picture>` desteği; CSS3'te bağlam-duyarlı seçiciler (`:nth-child`, `[attr=]`, `:is()`/`:not()`, kardeş/ata birleşimleyicileri); Canvas 2D'de gradyan, transform, Bézier/arc path'leri, `save`/`restore` ve `getImageData`/`putImageData`; minJs'te `XMLHttpRequest`, `history.pushState` ve `requestAnimationFrame`.
- Tarayıcı CSS3 `display` (flex/grid/block/inline/inline-block) ve `overflow` kuralları artık doğru uygulanıyor; çoklu CPU scheduler'ında bir işlemcinin kendi hazır görevini atlayıp boşta kalma hatası düzeltildi.
- `TimeBudget` ve `OperationGuard` için deterministik host testleri eklendi.
- Dosya Yöneticisi'nden WAV dosyalarını doğrudan oynatma ve görüntüyü döşeli masaüstü arka planı yapma eklendi.
- Ayarlar'a üç yeni masaüstü görseli eklendi.
- `munmap` artık gerçek sayfa kaldırma yapar: gerçekleşmiş çerçeveler kaynak defterinden düşülüp PMM'e iade edilir, boşalan sayfa tabloları (PT/PD/PDPT) geri kazanılır; bölge tablosu doluyken ortadan bölme hiçbir eşleme kaldırılmadan reddedilir. Süreç çıkışı ve fork hata yollarındaki paylaşımlı çerçeve referans sızıntısı da giderildi.
- QEMU varsayılan çalıştırmasında klavye ve fare artık çalışıyor: `run.ps1` artık varsayılan olarak `qemu-xhci + usb-kbd + usb-tablet` (USB HID) bağlar. `usb-tablet` mutlak koordinatlıdır ve QEMU fare konumunu doğrudan host imleciyle senkronize eder, böylece QEMU'da fare takılmadan pürüzsüz hareket eder. QEMU 11'in varsayılan `pc` makinesinde i8042 denetleyicisi olduğuna karşın PS/2 klavye/fare aygıtı bağlı gelmediği için önceki sürümlerde masaüstü giriş olayları gelmemişti. 
- Masaüstü arayüzü uçtan uca cilalandı ve hızlandırıldı: tüm uygulamalarda tekerlekle yumuşak kaydırma; pencere, başlat menüsü, bağlam menüsü, Alt+Tab geçişi ve takvim/ses açılır pencerelerinde kısa açılış animasyonları; görev çubuğu ve tepside ipucu (tooltip) balonları; toast bildirimlerde kayma animasyonu; metin çiziminde glyph önbelleği. Sistem Ayarları > Ekran sekmesine **Arayüz animasyonları** (tüm hareketi kapatır) ve **Saydam pencereler** anahtarları eklendi.
- Erişilebilirlik iyileştirmeleri: başlık çubuğunda kaydedilmemiş değişiklik işareti (●) ve uzun başlık kısaltma (…); düğmelerde seçili (yeşil çerçeve), odaklanmış (sarı odak halkası) ve devre dışı (koyulaştırılmış) durumlar ayrıştırıldı; tüm pencereler doğru uygulama kimliği (app_id) ile başlık ve simge alır.
- Host test paketi 547 teste ulaştı; tüm testler geçti (547 passed, 0 failed).

Değişikliklerin tam listesi: [Sürüm günlüğü](docs/CHANGELOG.md).

## Hemen deneyin

Gereksinimler ve alternatif çalıştırma seçenekleri için [Derleme ve donanım rehberine](docs/building.md) bakın.

```powershell
git clone https://github.com/tbagriyanik/minos.git
cd minos
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

Host testlerini çalıştırmak için:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

Son doğrulama: **547 host testi geçti** (`547 passed; 0 failed`). Projede toplam **206.213 Rust satırı** bulunur; bunun **195.541 satırı `src/`**, **4.893 satırı `tests/`**, kalan **5.779 satır** ise fuzz hedefleri, `build.rs`, `tools/` ve kök seviyedeki test yardımcı dosyalarıdır (`target/` hariç). Satır sayısı mevcut çalışma ağacındaki `.rs` dosyalarının fiziksel satırlarından hesaplanır.

## Özellikler

- Grafik masaüstü ve yerleşik uygulama paketi — [özellikler ve yol haritası](docs/features.md)
- Desteklenen donanım ve bilinen sınırlamalar — [Hardware & Driver Support Matrix](docs/HARDWARE_DRIVER_SUPPORT_MATRIX.md)
- MinShell komutları, masaüstü ve workspace (sanal masaüstü) kısayolları — [kullanım rehberi](docs/usage.md)
- Gereksinimler, derleme ve QEMU/VirtualBox — [derleme rehberi](docs/building.md)
- Eski sürüm notları — [arşiv](docs/releases.md)
- Ayrıntılı teknik belgeler — [GitHub Wiki](https://github.com/tbagriyanik/minOs/wiki)

## Ekran görüntüleri

| Welcome | Tema 1 | Tema 2 |
|---|---|---|
| ![Welcome](docs/screenshots/welcome.png) | ![Tema 1](docs/screenshots/tema1.png) | ![Tema 2](docs/screenshots/tema2.png) |

[Tanıtım videosunu izleyin](https://www.youtube.com/watch?v=fB1MQPZTMEg).

## Katkı

Katkı yönergeleri için [CONTRIBUTING.md](CONTRIBUTING.md) dosyasına bakın.
