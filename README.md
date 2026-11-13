# 🐱 minOs — Sarman Edition

[Türkçe](README.md) | [English](README.en.md)

![minOs Sarman Edition](public/minOs%20Sarman%20Edition.png)

[![Rust](https://img.shields.io/badge/Language-Rust%20nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64-blue.svg)]()
[![Lines of Code](https://img.shields.io/badge/Lines%20of%20Code-140%2C315%20(Rust)-lightgrey.svg)](https://github.com/tbagriyanik/minos)
[![Version](https://img.shields.io/badge/Version-v2.4.0-brightgreen.svg)](https://github.com/tbagriyanik/minos/releases)

minOs, Rust ile geliştirilen bağımsız bir 64-bit (`x86_64`) işletim sistemidir. Çekirdek `no_std` ve `no_main` kullanır; grafik masaüstü ve yerleşik uygulamalar sunar.

## Güncel bilgiler · v2.4.0

- Donmuş uygulamaların izlenmesi ana döngüden bağımsız çalışır; FPS 3D uygulaması gerçek kare hızı ölçümleri üretir.
- Render bütçesi aşılırsa tamamlanmamış kare ekranda bırakılmaz.
- `TimeBudget` ve `OperationGuard` için deterministik host testleri eklendi.
- Dosya Yöneticisi'nden WAV dosyalarını doğrudan oynatma ve görüntüyü döşeli masaüstü arka planı yapma eklendi.
- Ayarlar'a üç yeni masaüstü görseli eklendi.

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

## Özellikler

- Grafik masaüstü ve yerleşik uygulama paketi — [özellikler ve yol haritası](docs/features.md)
- Desteklenen donanım ve bilinen sınırlamalar — [Hardware & Driver Support Matrix](docs/HARDWARE_DRIVER_SUPPORT_MATRIX.md)
- MinShell komutları ve masaüstü kısayolları — [kullanım rehberi](docs/usage.md)
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
