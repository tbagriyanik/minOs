# Geliştirici Rehberi

Kaynak kod dizin haritası, birim testleri ve derleme bilgileri.

---

## Kaynak Dizin Haritası

```
minos/
├── src/
│   ├── arch/x86_64/         # SSE, IDT, APIC, SMP ve 1000Hz scheduler tick
│   ├── boot/                # Limine konsolu, metin çizimi, ilerleme çubuğu
│   ├── desktop/             # Masaüstü oturumu (session.rs) ve yardımcılar
│   ├── drivers/             # video, PS/2, RTC, e1000, TCP/IP, soket API, HTTP,
│   │                        # TLS 1.2/1.3, kripto (SHA-256, HMAC, AES, GCM, RSA,
│   │                        # ECC/ECDSA, X.509, kök CA deposu roots/), HDA ses,
│   │                        # ATA/ATAPI, USB durum katmanı, xHCI HID
│   ├── fs/                  # VFS, disk okuyucuları (FAT32 + ISO9660), Ext2 r/w,
│   │                        # ext4/exFAT biçimlendirme, MBR/GPT bölümleme
│   ├── gui/                 # MinGUI pencere yöneticisi, font motoru, tema motoru,
│   │                        # kaydırma çubuğu ve pano
│   ├── apps/                # 17 kayıtlı uygulama (app ID 0–16)
│   ├── mm/                  # PMM, 4/5-level paging, süreç tablosu, ELF yükleyici,
│   │                        # memory/ alt modülü (büyük sayfa, sıkıştırma, profilleme)
│   └── main.rs              # Çekirdek giriş noktası (_start)
├── tests/                   # Host birim testleri (446 test)
├── boot/                    # Limine bootloader yapılandırması (limine.conf)
├── docs/                    # Yerel Markdown dokümanları
├── wiki/                    # Wiki sayfaları (bu dizin)
├── build_iso.ps1            # El Torito yamalı hibrit ISO oluşturucu
├── run.ps1                  # Tek tıkla derleme ve QEMU başlatma betiği
├── build.rs                 # fontdue ile TTF → bitmap font atlası üretimi
├── x86_64-minos.json        # Hedef x86_64 Long Mode tanımlama dosyası
├── linker.ld                # Bağlayıcı komut dosyası
└── rust-toolchain.toml      # Rust toolchain yapılandırması (nightly)
```

### Kernel Cargo katmanı

Çekirdek yalnızca gerçekten çağırdığı 8 crate'e bağımlıdır: `linked_list_allocator`
(heap), `p384` (P-384 imza), `bitflags`, `heapless`, `zerocopy`, `crc32fast`,
`raw-cpuid` ve `log`. Ağ yığını (`netstack.rs`), kök CA deposu (`roots/`),
rastgelelik (`random.rs`, xorshift64*) ve IPC kodlaması (`kernel_formats.rs`) kendi
kodlarıyla, sıfır harici bağımlılıkla çalışır; `webpki-roots`, `smoltcp`, `rand_hc`,
`elf`, `embedded-io`, `postcard`, `serde` ve `acpi` kaldırılmıştır. Bağımlılıklar
`default-features = false` ile `no_std` hedefine göre sınırlandırılmıştır.

---

## Birim Testleri (`test_host.ps1`)

Kriptografi, X.509, TLS 1.2 / TLS 1.3, JavaScript, HTML, Canvas 2D, SVG rasterizer, dosya sistemi, bellek, sürücüler ve GUI testlerini içeren 443 birim testi host ortamında yürütülür. TLS testleri artık üretimdeki sertifika doğrulama yolunu kullanır; geçerli RSA/ECDSA zincirleri ile hatalı ve süresi dolmuş ara CA senaryolarını kapsar.

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

Test sonuçları: **446 geçti / 0 hata**

---

## Derleme

```powershell
cargo build --release -Zjson-target-spec --target x86_64-minos.json `
  -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
```

ISO oluşturma:

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```

---

## Kod İstatistikleri (v2.5.0)

| Ölçüt | Değer |
| :--- | :--- |
| **Toplam Rust Kaynak Dosyası** | 223 dosya (`src/` + `tests/` + `build.rs`) |
| **Toplam Kod Satırı (ham)** | 177.770 satır |
| **Dil** | Rust (Nightly, `no_std`) |
| **Derleme Hedefi** | `x86_64-minos` (özel bare-metal) |
| **Birim Testleri** | 446 test (0 hata, %100 başarılı) |
| **Yerleşik Uygulama** | 17 kayıtlı uygulama (app ID 0–16) + Pencere Yöneticisi |
| **Çekirdek / ISO Boyutu** | ~2.0 MB / ~6.0 MB |
