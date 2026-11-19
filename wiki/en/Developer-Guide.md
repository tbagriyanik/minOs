# Developer Guide

Source tree map, unit tests and build information.

---

## Source Tree Map

```
minos/
├── src/
│   ├── arch/x86_64/         # SSE, IDT interrupts and PIC masking
│   ├── boot/                # Limine console, text rendering, progress bar
│   ├── desktop/             # Desktop session (session.rs) and helpers
│   ├── drivers/             # video, PS/2, RTC, e1000, TCP/IP, socket API, HTTP,
│   │                        # TLS 1.2/1.3, crypto (SHA-256, HMAC, AES, GCM, RSA,
│   │                        # ECC/ECDSA, X.509, root CA store roots/), HDA audio,
│   │                        # ATA/ATAPI, USB status layer, xHCI HID
│   ├── fs/                  # VFS, disk readers (FAT32 + ISO9660), Ext2 r/w,
│   │                        # ext4/exFAT formatting, MBR/GPT partitioning
│   ├── gui/                 # MinGUI window manager, font engine, theme engine,
│   │                        # scrollbar and clipboard
│   ├── apps/                # 17 registered apps (app ID 0–16)
│   ├── mm/                  # Physical memory manager (PMM), process table, ELF loader,
│   │                        # memory/ submodule (huge pages, compression, profiling)
│   └── main.rs              # Kernel entry point (_start)
├── tests/                   # Host unit tests (446 tests)
├── boot/                    # Limine bootloader configuration (limine.conf)
├── docs/                    # Local Markdown documents
├── wiki/                    # Wiki pages (this directory)
├── build_iso.ps1            # El Torito patched hybrid ISO creator
├── run.ps1                  # One-click build and QEMU launcher script
├── build.rs                 # fontdue TTF → bitmap font atlas generation
├── x86_64-minos.json        # Target x86_64 Long Mode definition file
├── linker.ld                # Linker script
└── rust-toolchain.toml      # Rust toolchain configuration (nightly)
```

---

## Unit Tests (`test_host.ps1`)

Cryptography, X.509, TLS 1.2 / TLS 1.3, JavaScript, HTML, Canvas 2D, SVG rasterizer, filesystem, memory, driver and GUI coverage — 443 unit tests run in the host environment. TLS tests now use the production certificate-validation path and cover valid RSA/ECDSA chains plus invalid and expired intermediate CA cases.

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

Test results: **446 passed / 0 failed**

---

## Build

```powershell
cargo build --release -Zjson-target-spec --target x86_64-minos.json `
  -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
```

ISO creation:

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```

---

## Code Statistics (v2.5.0)

| Metric | Value |
| :--- | :--- |
| **Total Rust Source Files** | 223 files (`src/` + `tests/` + `build.rs`) |
| **Total Raw Rust Lines** | 177,084 lines |
| **Language** | Rust (Nightly, `no_std`) |
| **Build Target** | `x86_64-minos` (custom bare-metal) |
| **Unit Tests** | 446 tests (0 failed, 100% passing) |
| **Built-in Applications** | 17 registered apps (app ID 0–16) + Window Manager |
| **Kernel / ISO Size** | ~2.0 MB / ~6.0 MB |
