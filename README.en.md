# 🐱 minOs — Sarman Edition

[English](README.en.md) | [Türkçe](README.md)

![minOs Sarman Edition](public/minOs%20Sarman%20Edition.png)

[![Rust](https://img.shields.io/badge/Language-Rust%20nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64-blue.svg)]()
[![Lines of Code](https://img.shields.io/badge/Lines%20of%20Code-162%2C097%20(Rust)-lightgrey.svg)](https://github.com/tbagriyanik/minos)
[![Version](https://img.shields.io/badge/Version-v2.5.0-brightgreen.svg)](https://github.com/tbagriyanik/minos/releases)

minOs is a standalone 64-bit (`x86_64`) operating system written in Rust. Its kernel uses `no_std` and `no_main`, and provides a graphical desktop with built-in applications.

## Current · v2.5.0

- Frozen-app monitoring now runs independently of the main loop, and the FPS 3D app reports real frame-rate measurements.
- The FPS game has start and result menus; basic form controls support Tab navigation, Enter confirmation and Esc cancellation, including the Welcome screen.
- Incomplete frames are no longer displayed when the render budget expires.
- TLS/X.509 verification now checks leaf and intermediate validity, CA BasicConstraints/pathLen, and any KeyUsage/EKU extensions; the test-only validation bypass was removed. The host suite has 322 tests.
- Deterministic host tests were added for `TimeBudget` and `OperationGuard`.
- File Manager can open WAV files directly in Music Player, and Image Viewer can set an image as a tiled desktop wallpaper.
- Settings includes three additional desktop images.
- `munmap` now performs real page teardown: realized frames are dropped from the resource ledger and returned to the PMM, and emptied page-table frames (PT/PD/PDPT) are reclaimed; middle splits are rejected without removing any mapping when the region table is full. A shared-frame reference leak on process exit and fork-failure paths was also fixed.
- Keyboard and mouse now work in default QEMU runs: `run.ps1` now attaches `qemu-xhci + usb-kbd + usb-mouse` (USB HID) by default. QEMU 11's default `pc` machine had the i8042 controller but no PS/2 keyboard/mouse device attached, so previous releases never received desktop input events; this is fixed.

See the [full changelog](docs/CHANGELOG.en.md) for details.

## Quick start

See the [build and hardware guide](docs/building.md) for prerequisites and other run options.

```powershell
git clone https://github.com/tbagriyanik/minos.git
cd minos
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

Run host tests with:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

## Documentation

- [Features and roadmap](docs/features.md) *(Turkish)*
- [Hardware & Driver Support Matrix](docs/HARDWARE_DRIVER_SUPPORT_MATRIX.md) *(Turkish)*
- [Shell commands, desktop and workspace (virtual desktop) shortcuts](docs/usage.md) *(Turkish)*
- [Requirements, building, QEMU and VirtualBox](docs/building.md) *(Turkish)*
- [Previous release notes](docs/releases.md) *(Turkish)*
- [Detailed technical documentation](https://github.com/tbagriyanik/minOs/wiki)

## Screenshots

| Welcome | Theme 1 | Theme 2 |
|---|---|---|
| ![Welcome](docs/screenshots/welcome.png) | ![Theme 1](docs/screenshots/tema1.png) | ![Theme 2](docs/screenshots/tema2.png) |

Watch the [project introduction video](https://www.youtube.com/watch?v=fB1MQPZTMEg).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines.
