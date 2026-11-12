# 🚀 Sürüm ve Test Süreci (Release & Testing)

Bu belge, minOs ISO'sunun nasıl üretildiğini, otomatik testlerin nasıl
çalıştığını ve test raporlarının sürüm sürecine nasıl bağlandığını açıklar.

## 📦 Çıktılar (Artifacts)

| Dosya | Üretim | Amaç |
| :--- | :--- | :--- |
| `minos.iso` | `build_iso.ps1` / CI `release.yml` | Önyüklenebilir sürüm imajı |
| `test-report.md` | `test_qemu.ps1` | QEMU entegrasyon test raporu (Markdown) |
| `target/qemu_audio.wav` | `test_qemu.ps1` (HDA) | Gerçek ses çıkış doğrulaması |
| `serial.log` | CI `qemu-boot-test` | Önyükleme seri günlüğü |

---

## 🔨 ISO Üretimi

### Yerel (Windows / Linux)

```powershell
# 1) Çekirdeği derle ve ISO'yu üret
./run.ps1 -BuildOnly
./build_iso.ps1
```

`build_iso.ps1` çıktıyı `minos.iso` olarak üretir ve `target/iso_root/`
dizinini hazırlar. Ayrıntılar: [`../ISO_GUIDE.md`](../ISO_GUIDE.md).

### CI (`.github/workflows/release.yml`)

- Her `main` itmesi ve PR'da `quality` işi çekirdeği `x86_64-minos` hedefinde denetler.
- `build-release` işi ISO'yu üretir ve `minos-iso` artifact'i olarak yükler.
- `v*` etiketlerinde GitHub Release oluşturulur ve ISO eklenir.
- `main` / `workflow_dispatch` için `latest` rolling release güncellenir.

---

## 🧪 Testler

### 1) Host Birim Testleri — `test_host.ps1`

Saf mantık modüllerini (`src/drivers/crypto` vb.) `tests/host_tests.rs`
üzerinden **host** hedefinde derleyip çalıştırır. Çekirdek `no_std` olduğu için
`cargo test` doğrudan kullanılamaz; bu betik `--target` ve `build-std` ayarlarını
devre dışı bırakır.

```powershell
./test_host.ps1
```

### 2) QEMU Entegrasyon Testi — `test_qemu.ps1`

`minos.iso`'yu QEMU'da başlatır ve seri günlükteki **pozitif** sürücü/başlatma
kalıplarını doğrular. Profiller: `minimal` (128 MB), `default` (512 MB),
`high` (8 GB + USB HID). HDA ses çıkışını `driver=wav` ile gerçek WAV dosyasına
yazar.

```powershell
# Temel (default profil)
./test_qemu.ps1

# Yüksek donanım profili (USB HID + QMP ile klavye/fare enjeksiyonu)
./test_qemu.ps1 -HighEnd

# Minimal profil (ağ/ses yok)
./test_qemu.ps1 -Minimal

# Yeniden derleyerek
./test_qemu.ps1 -Rebuild

# FPS 3D acilis + gercek FPS/min-FPS/frame-time stres testi (10/30/60 dk)
./test_qemu.ps1 -FpsStressTest -FpsStressMinutes 10
```

Test sonunda:
- Ekranda özet rapor basılır.
- **`test-report.md`** dosyası depo köküne yazılır.
- Başarılıysa çıkış kodu `0`, değilse `1`.

Test kapsamı: PMM, VMM, allocator, CPU açılışı, ağ adaptörü link/ring,
**ağ yığını özyin testi (ARP/ICMP/TCP/HTTP + soket/HTTP mantık)**, giriş
sürücüleri, Intel HDA + gerçek WAV PCM çıkışı ve masaüstü olay döngüsü.

### 3) CI (`.github/workflows/ci.yml`)

| İş | Ne yapar |
| :--- | :--- |
| `code-quality` | `cargo fmt --check`, `test_host.ps1`, clippy |
| `kernel-build` | `cargo check` (0 uyarı kapısı) + release build |
| `qemu-boot-test` | ISO'yu 3 profilde (default/minimal/high) QEMU'da başlatır, `[BOOT] CRITICAL_CHECKS_COMPLETE` işaretini doğrular |

---

## 🔗 Test Raporlarını Sürüme Bağlama

`release.yml` içinde ISO üretildikten sonra `test_qemu.ps1` çalıştırılır ve
üretilen **`test-report.md`** sürümle birlikte yayınlanır:

1. `quality` → çekirdek denetimi (0 uyarı).
2. `build-release` → ISO üretimi.
3. `qemu-test` → `test_qemu.ps1` ile entegrasyon testi; `test-report.md` üretilir.
4. `Upload Test Report` → `test-report` artifact'i olarak yüklenir.
5. `Create Release` → ISO + test raporu sürüm varlıklarına eklenir.

> Not: QEMU testi `continue-on-error` ile yürütülür; altyapı kaynaklı bir
> aksaklık sürümü engellemez, ancak rapor eksikse bu bir uyarı olarak görülür.

---

## 🧷 Gerçek Donanım Doğrulama

QEMU emülasyonu mantığı doğrular; gerçek donanım için aşağıdaki adımlar
izlenir (ayrıntılı tablo: [`HARDWARE_COMPATIBILITY.md`](./HARDWARE_COMPATIBILITY.md)):

1. `minos.iso`'yu Rufus/Ventoy ile USB'ye yazdırın.
2. Sistemi USB'den başlatın.
3. **Ağ:** Sistem Ayarları → Ağ sekmesinde ARP / ICMP (Ping) / TCP testlerini çalıştırın.
4. **Ses:** Masaüstü açılış melodisini ve sistem seslerini dinleyin.
5. **Giriş:** USB klavye ve fareyi (ve varsa PS/2) test edin.
6. Sonuçları uyumluluk tablosuna işleyin.

---

## 📋 Sürüm Kontrol Listesi

- [ ] `test_host.ps1` geçiyor.
- [ ] `test_qemu.ps1 -HighEnd` ve `-Minimal` geçiyor; `test-report.md` üretildi.
- [ ] CI yeşil (fmt, check, qemu-boot-test).
- [ ] ISO boyutu ve önyükleme (BIOS + UEFI) doğrulandı.
- [ ] Gerçek donanım tablosu güncellendi.
- [ ] Sürüm notları (`docs/releases.md`) güncellendi.
- [ ] `v*` etiketi atıldı; ISO + test raporu release'e eklendi.

---

## 🔗 İlgili Dosyalar

- [`../build_iso.ps1`](../build_iso.ps1), [`../run.ps1`](../run.ps1)
- [`../test_host.ps1`](../test_host.ps1), [`../test_qemu.ps1`](../test_qemu.ps1)
- [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)
- [`.github/workflows/release.yml`](../.github/workflows/release.yml)
- [`HARDWARE_COMPATIBILITY.md`](./HARDWARE_COMPATIBILITY.md)