# minOs Wiki – Welcome

minOs (Sarman Edition) v2.1.0 is a standalone 64-bit (`x86_64`) operating system written in Rust that uses `no_std` and `no_main` in the kernel. It boots via Limine v8 on both UEFI and BIOS environments.

> 📄 **Project:** [GitHub Repository](https://github.com/tbagriyanik/minos)  
> 📄 **Language:** [English](/)| [Türkçe](../tr/)

---

## Quick Access

| Page | Content |
| :--- | :--- |
| [About & Architecture](About-Architecture) | Design philosophy, `no_std` model, PMM and IDT structure |
| [Installation and Running](Installation-and-Running) | `run.ps1` options, Live USB creation, disk installation |
| [Supported Hardware List](Supported-Hardware) | Driver matrix, network, graphics, storage, input devices |
| [15 Built-in Applications](Built-in-Applications) | Web Browser, Notepad, File Manager, etc. |
| [Network and Internet](Network-and-Internet) | TCP/IP, DNS (A/AAAA), HTTP/1.1, TLS 1.3 (HTTPS), HTTP/2 infrastructure |
| [File Systems](File-Systems) | VFS, FAT32, Ext2, Ext4, exFAT, ISO9660 |
| [User Guide](User-Guide) | Desktop shortcuts, MinShell commands |
| [Developer Guide](Developer-Guide) | Source tree map, unit tests, building |
| [Release History](Release-History) | Release notes and future plans |

---

## Status Summary (v2.1.0)

| Metric | Value |
| :--- | :--- |
| **Rust Compiler** | 1.101.0-nightly |
| **Total Lines of Code** | ~100,971 lines (Rust) |
| **Unit Tests** | 195 passed / 0 failed |
| **Trusted Root CAs** | 51 certificates |
| **Built-in Applications** | 15 desktop applications |
| **Kernel / ISO Size** | ~2.0 MB / ~6.0 MB |

---

## Feature Highlights

- **TLS 1.3 HTTPS:** Browser completes `https://` connections per RFC 8446, preferring IPv6 (AAAA) with an IPv4 fallback.
- **Browser Engine:** Progressive (SOF2) JPEG decoding, `@font-face` rule parsing and `@keyframes` color/opacity animation.
- **Dynamic Themes:** 5 themes (Velvet Dark, Daylight, Nord, Terminal Green, Sunset) with live switching.
- **xHCI USB HID:** Native USB keyboard/mouse support with PS/2 fallback.
- **Networking:** Intel e1000, I219-V, Realtek PCIe, Broadcom NetXtreme, VirtIO-net, VMware VMXNET3.
- **Storage:** AHCI/SATA, NVMe, VirtIO-blk, USB Mass Storage, SD/MMC.
- **File Systems:** FAT32, Ext2, Ext4, exFAT (read/write/format).
- **Crypto:** SHA-256/384/512, HMAC, AES-GCM, RSA, ECDSA, X.509, CRL/OCSP.
- **Recovery Console:** `recovery#` interactive text console.
