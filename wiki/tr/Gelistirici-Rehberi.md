# Geliştirici Rehberi

Kaynak kod dizin haritası, birim testleri ve derleme bilgileri.

---

## Kaynak Dizin Haritası

```
minos/
├── src/
│   ├── arch/x86_64/         # SSE, IDT kesmeleri ve PIC maskeleme
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
│   ├── apps/                # 15 Masaüstü uygulaması
│   ├── mm/                  # Fiziksel bellek yöneticisi (PMM), süreç tablosu, ELF yükleyici
│   └── main.rs              # Çekirdek giriş noktası (_start)
├── tests/                   # Host birim testleri (195 test)
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

---

## Birim Testleri (`test_host.ps1`)

Kriptografi (AES, GCM, RSA, ECC, ECDSA, SHA-256, SHA-384, SHA-512), X.509 sertifika, zincir derinliği ve geçerlilik süresi/hostname doğrulama, TLS 1.2 / TLS 1.3 el sıkışması, JavaScript motoru, HTML ayrıştırıcı ve GUI metin tamponu (`TextBuf`) testlerini içeren 195 birim testi host ortamında yürütülür:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

Test sonuçları: **195 geçti / 0 hata**

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

## Kod İstatistikleri (v2.1.0)

| Ölçüt | Değer |
| :--- | :--- |
| **Toplam Rust Kaynak Dosyası** | 147 dosya |
| **Toplam Kod Satırı** | ~100.971 satır |
| **Dil** | Rust (Nightly, `no_std`) |
| **Derleme Hedefi** | `x86_64-minos` (özel bare-metal) |
| **Birim Testleri** | 195 test (0 hata, %100 başarılı) |
| **Yerleşik Uygulama** | 15 masaüstü uygulaması + Pencere Yöneticisi |
| **Çekirdek / ISO Boyutu** | ~2.0 MB / ~6.0 MB |
