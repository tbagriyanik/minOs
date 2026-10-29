# 🤝 minOs Katkı Rehberi (Contributing Guide)

minOs, açık kaynaklı ve eğitim/araştırma amaçlı geliştirilen bir işletim sistemi projesidir. Katkılarınızı memnuniyetle karşılarız. Bu rehber, kod katkısı, derleme, test ve Pull Request (PR) sürecini açıklar.

> 📄 **English:** [CONTRIBUTING.en.md](CONTRIBUTING.en.md) (English version below)

---

## 📋 İçerik

1. [Başlarken](#-başlarken)
2. [Derleme Süreci](#-derleme-süreci)
3. [Test Süreci](#-test-süreci)
4. [Kod Stili ve Kalite](#-kod-stili-ve-kalite)
5. [Pull Request (PR) Süreci](#-pull-request-pr-süreci)
6. [Commit İletişim Kuralları](#-commit-iletişim-kuralları)
7. [Issue ve Özellik İstekleri](#-issue-ve-özellik-istekleri)
8. [CI/CD ve GitHub Actions](#-cicd-ve-github-actions)
9. [Lisans](#-lisans)

---

## 🚀 Başlarken

### 1. Depoyu klonlayın

```powershell
git clone https://github.com/tbagriyanik/minos.git
cd minos
```

### 2. Ön koşulları yükleyin

| Araç | Açıklama |
| :--- | :--- |
| **Rust Nightly** | `rustup default nightly` + `rustup component add rust-src` |
| **QEMU x86_64** | Önyükleme ve entegrasyon testleri için |
| **PowerShell 5.1+** | Derleme ve ISO paketleme betikleri için (Windows) |
| **xorriso / oscdimg / mkisofs** | Boot edilebilir ISO oluşturma için |
| **Limine** | `limine bios-install` komutu için (ISO/disk önyükleyici imzası) |

### 3. Derleme

minOs çekirdeği özel bir `x86_64-minos` hedefi ve `no_std` ortamında derlenir:

```powershell
cargo build --release -Zjson-target-spec --target x86_64-minos.json `
  -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
```

Derlenen ikili dosya: `target\x86_64-minos\release\minos-kernel`

### 4. ISO oluşturma

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```

Çıktı: `minos.iso` (kök dizin) ve `target\release\minos.iso`

### 5. QEMU ile çalıştırma (tek komut)

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

`run.ps1` seçenekleri için `docs/building.md` ve wiki sayfalarına bakın.

---

## 🧪 Test Süreci

### Host birim testleri (195 test)

Kriptografi, X.509, TLS, JavaScript motoru, HTML ayrıştırıcı ve GUI metin tamponu testleri:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

Beklenen sonuç: **195 geçti / 0 hata**

### QEMU entegrasyon testi

Önyükleme, sürücü başlatma, masaüstü erişimi ve gerçek donanım davranışlarını doğrular:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_qemu.ps1
```

### Disk hazırlama (kalıcı önyükleme)

```powershell
powershell -ExecutionPolicy Bypass -File .\build_disk.ps1 -DiskImage .\disk.qcow2 -Force
```

---

## 🎨 Kod Stili ve Kalite

- **Biçimlendirme:** `cargo fmt --all` ile Rust biçimlendirmesi uygulanmalıdır.
- **Linter:** `cargo clippy` uyarıları giderilmelidir (CI'de non-blocking).
- **Dil:** Rust 2021 edition, Nightly toolchain.
- **Çekirdek:** `no_std`, `no_main`, `panic = "abort"`.
- **Yorumlar ve dökümantasyon:** Türkçe (ana döküm), İngilizçe (wiki/English README).
- **Güvenlik:** `unsafe` blokları minimumda tutulmalı ve nedeni yorumla açıklanmalı.

CI'de çalışan kalite kapısı:
- `cargo fmt --all -- --check`
- `cargo clippy`
- Host testleri (`test_host.ps1`)
- Bare-metal kernel derleme (`cargo build --release`)
- QEMU önyükleme ve log doğrulama

---

## 🔀 Pull Request (PR) Süreci

### 1. Dal (branch) oluşturun

`main` dalından yeni bir özellik/hata düzeltme dalı oluşturun:

```powershell
git checkout -b feature/ozellik-adi
# veya
git checkout -b fix/hata-aciklamasi
```

### 2. Değişiklikleri yapın ve test edin

- Kodunuzu `cargo fmt` ile biçimlendirin.
- `cargo clippy` ile uyarıları giderin.
- Host testlerini çalıştırın (`test_host.ps1`).
- QEMU entegrasyon testini çalıştırın (`test_qemu.ps1`).
- Derlemeyi doğrulayın (`cargo build --release`).

### 3. Commit yapın

Commit iletileri [Commit İletişim Kuralları](#-commit-iletişim-kuralları) bölümüne uygun olmalıdır.

```powershell
git add .
git commit -m "feat: tarayıcı demo sayfaları eklendi"
```

### 4. PR açın

GitHub üzerinden `main` dalına bir Pull Request açın. PR şablonunda aşağıdaki bilgileri doldurun:

- **Değişiklik özeti:** Ne yapıldı, neden yapıldı.
- **Testler:** Hangileri çalıştırdı, sonuçları.
- **Ekran görüntüsü:** GUI değişikliği varsa ekleyin.
- **İlişki:** İlgili issue numarası varsa belirtin.

### 5. İnceleme ve birleştirme

- Proje bakıcısı PR'yi inceleyecek ve istenirse değişiklik istiyor olabilir.
- CI kalite kapısı geçişli olmalı (format, clippy, host tests, kernel build, QEMU boot).
- Onaylandıktan sonra `main` dalına birleştirilir (squash merge tercih edilir).

---

## 📝 Commit İletişim Kuralları

Conventional Commits formatı kullanılır:

| Tür | Açıklama | Örnek |
| :--- | :--- | :--- |
| `feat` | Yeni özellik | `feat: USB masaüstü toast bildirimi eklendi` |
| `fix` | Hata düzeltmesi | `fix: xHCI MaxPorts bit aralığı hatası düzeltildi` |
| `docs` | Dokümantasyon | `docs: derleme rehberi güncellendi` |
| `style` | Biçimlendirme | `style: rustfmt uygulandı` |
| `refactor` | Yeniden yapılandırma | `refactor: netstack katmanı sadeleştirildi` |
| `test` | Test ekleme/güncelleme | `test: exFAT parçalanma stres testi eklendi` |
| `build` | Derleme sistemi | `build: build_iso.ps1 UEFI desteği eklendi` |
| `ci` | CI/CD ayarları | `ci: QEMU boot testi job'ı eklendi` |
| `chore` | Diğer | `chore: dependency güncellemesi` |

Örnek commit iletisi:

```
feat: USB HID klavye ve fare hotplug desteği eklendi

- xHCI port tak/çıkarma olayları dinleniyor
- Masaüstünde canlı toast bildirimi gösteriliyor
- Donanım listesi anında güncelleniyor

Closes #123
```

---

## 🐛 Issue ve Özellik İstekleri

### Hata raporu

Yeni bir issue açarken aşağıdaki bilgileri ekleyin:

- minOs sürümü (örn. v2.1.0)
- Donanım / emülatör (QEMU, VirtualBox, fiziksel laptop)
- Adımları tekrar edebilir şekilde açıklayın
- Beklenen ve gerçek davranış
- Önyükleme konsolu / seri log çıktısı (varsa)

### Özellik isteği

`.github\ISSUE_TEMPLATE\feature_request.md` şablonunu kullanın:

- Sorunun tanımı
- İstenen çözüm
- Düşünülen alternatifler
- Ek bağlam / ekran görüntüsü

---

## 🔄 CI/CD ve GitHub Actions

Proje iki GitHub Actions workflow kullanır:

| Dosya | Amaç |
| :--- | :--- |
| `.github\workflows\ci.yml` | Kod kalitesi, host testleri, kernel derleme ve QEMU önyükleme testi |
| `.github\workflows\release.yml` | Sürüm derleme, ISO paketleme ve GitHub Release yayınlama |

CI her `push` ve `pull_request` olayında `main` dalında çalışır. PR'ler kalite kapısını geçmeden birleştirilemez.

---

## 📄 Lisans

Bu proje [MIT Lisansı](licence.md) kapsamında dağıtılmaktadır. Katkıda bulunarak kodunuzu bu lisans şartlarıyla paylaşmış olursunuz.

---

## 🙏 Teşekkürler

Katkıda bulunan herkese teşekkür ederiz. minOs'u daha iyi hale getirmenize yardımcı olduğunuz için teşekkürler! 🐾

---

## 🇬🇧 English Version

# 🤝 minOs Contributing Guide

minOs is an open-source operating system project developed for education and research purposes. We welcome your contributions. This guide explains the code contribution, build, test, and Pull Request (PR) process.

---

## 📋 Contents

1. [Getting Started](#-getting-started)
2. [Build Process](#-build-process)
3. [Test Process](#-test-process)
4. [Code Style and Quality](#-code-style-and-quality)
5. [Pull Request (PR) Process](#-pull-request-pr-process)
6. [Commit Message Conventions](#-commit-message-conventions)
7. [Issues and Feature Requests](#-issues-and-feature-requests)
8. [CI/CD and GitHub Actions](#-cicd-and-github-actions)
9. [License](#-license)

---

## 🚀 Getting Started

### 1. Clone the repository

```powershell
git clone https://github.com/tbagriyanik/minos.git
cd minos
```

### 2. Install prerequisites

| Tool | Description |
| :--- | :--- |
| **Rust Nightly** | `rustup default nightly` + `rustup component add rust-src` |
| **QEMU x86_64** | For boot and integration testing |
| **PowerShell 5.1+** | For build and ISO packaging scripts (Windows) |
| **xorriso / oscdimg / mkisofs** | For bootable ISO creation |
| **Limine** | For `limine bios-install` (ISO/disk boot sector signing) |

### 3. Build

minOs kernel is built with a custom `x86_64-minos` target in a `no_std` environment:

```powershell
cargo build --release -Zjson-target-spec --target x86_64-minos.json `
  -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
```

Output binary: `target\x86_64-minos\release\minos-kernel`

### 4. Create ISO

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```

Output: `minos.iso` (root) and `target\release\minos.iso`

### 5. Run in QEMU (one command)

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

For `run.ps1` options see `docs/building.md` and wiki pages.

---

## 🧪 Test Process

### Host unit tests (195 tests)

Cryptography, X.509, TLS, JavaScript engine, HTML parser, and GUI text buffer tests:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

Expected result: **195 passed / 0 failed**

### QEMU integration test

Verifies boot, driver initialization, desktop access, and real hardware behavior:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_qemu.ps1
```

### Disk preparation (persistent boot)

```powershell
powershell -ExecutionPolicy Bypass -File .\build_disk.ps1 -DiskImage .\disk.qcow2 -Force
```

---

## 🎨 Code Style and Quality

- **Formatting:** Apply `cargo fmt --all`.
- **Lint:** Fix `cargo clippy` warnings (non-blocking in CI).
- **Language:** Rust 2021 edition, Nightly toolchain.
- **Kernel:** `no_std`, `no_main`, `panic = "abort"`.
- **Comments and documentation:** Turkish (main docs), English (wiki/English README).
- **Security:** Keep `unsafe` blocks minimal and document the reason.

CI quality gate runs:
- `cargo fmt --all -- --check`
- `cargo clippy`
- Host tests (`test_host.ps1`)
- Bare-metal kernel build (`cargo build --release`)
- QEMU boot and log verification

---

## 🔀 Pull Request (PR) Process

### 1. Create a branch

Create a new feature/bugfix branch from `main`:

```powershell
git checkout -b feature/feature-name
# or
git checkout -b fix/bug-description
```

### 2. Make changes and test

- Format your code with `cargo fmt`.
- Fix `cargo clippy` warnings.
- Run host tests (`test_host.ps1`).
- Run QEMU integration test (`test_qemu.ps1`).
- Verify build (`cargo build --release`).

### 3. Commit

Commit messages must follow [Commit Message Conventions](#-commit-message-conventions).

```powershell
git add .
git commit -m "feat: browser demo pages added"
```

### 4. Open a PR

Open a Pull Request against `main` on GitHub. Fill in the PR template:

- **Change summary:** What was done and why.
- **Tests:** Which tests were run and results.
- **Screenshot:** Attach if GUI change.
- **Reference:** Related issue number if any.

### 5. Review and merge

- Maintainer reviews the PR and may request changes.
- CI quality gate must pass (format, clippy, host tests, kernel build, QEMU boot).
- Once approved, merged into `main` (squash merge preferred).

---

## 📝 Commit Message Conventions

Conventional Commits format is used:

| Type | Description | Example |
| :--- | :--- | :--- |
| `feat` | New feature | `feat: USB desktop toast notification added` |
| `fix` | Bug fix | `fix: xHCI MaxPorts bit range bug fixed` |
| `docs` | Documentation | `docs: build guide updated` |
| `style` | Formatting | `style: rustfmt applied` |
| `refactor` | Refactoring | `refactor: netstack layer simplified` |
| `test` | Test add/update | `test: exFAT fragmentation stress test added` |
| `build` | Build system | `build: build_iso.ps1 UEFI support added` |
| `ci` | CI/CD settings | `ci: QEMU boot test job added` |
| `chore` | Other | `chore: dependency update` |

Example commit message:

```
feat: USB HID keyboard and mouse hotplug support added

- xHCI port connect/disconnect events are monitored
- Live toast notification shown on desktop
- Hardware list updated instantly

Closes #123
```

---

## 🐛 Issues and Feature Requests

### Bug report

When opening a new issue, include:

- minOs version (e.g. v2.1.0)
- Hardware / emulator (QEMU, VirtualBox, physical laptop)
- Reproducible steps
- Expected and actual behavior
- Boot console / serial log output (if any)

### Feature request

Use `.github\ISSUE_TEMPLATE\feature_request.md` template:

- Problem definition
- Desired solution
- Considered alternatives
- Additional context / screenshots

---

## 🔄 CI/CD and GitHub Actions

The project uses two GitHub Actions workflows:

| File | Purpose |
| :--- | :--- |
| `.github\workflows\ci.yml` | Code quality, host tests, kernel build, QEMU boot test |
| `.github\workflows\release.yml` | Release build, ISO packaging, GitHub Release publishing |

CI runs on every `push` and `pull_request` to `main`. PRs cannot be merged without passing the quality gate.

---

## 📄 License

This project is distributed under the [MIT License](licence.md). By contributing, you agree to license your code under these terms.

---

## 🙏 Acknowledgments

Thank you to everyone who contributes. Thanks for helping make minOs better! 🐾
