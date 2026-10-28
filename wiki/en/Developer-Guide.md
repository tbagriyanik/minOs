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
│   ├── apps/                # 15 desktop applications
│   ├── mm/                  # Physical memory manager (PMM), process table, ELF loader
│   └── main.rs              # Kernel entry point (_start)
├── tests/                   # Host unit tests (195 tests)
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

Cryptography (AES, GCM, RSA, ECC, ECDSA, SHA-256, SHA-384, SHA-512), X.509 certificate, chain depth and validity period/hostname verification, TLS 1.2 / TLS 1.3 handshake, JavaScript engine, HTML parser and GUI text buffer (`TextBuf`) — 195 unit tests executed in the host environment:

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

Test results: **195 passed / 0 failed**

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

## Code Statistics (v2.1.0)

| Metric | Value |
| :--- | :--- |
| **Total Rust Source Files** | 147 files |
| **Total Lines of Code** | ~100,971 lines |
| **Language** | Rust (Nightly, `no_std`) |
| **Build Target** | `x86_64-minos` (custom bare-metal) |
| **Unit Tests** | 195 tests (0 failed, 100% passing) |
| **Built-in Applications** | 15 desktop applications + Window Manager |
| **Kernel / ISO Size** | ~2.0 MB / ~6.0 MB |
