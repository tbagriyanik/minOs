# 🐱 minOs - Sarman Edition

[English](README.en.md) | [Türkçe](README.md)

[![Rust](https://img.shields.io/badge/Language-Rust%201.101.0--nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Build](https://img.shields.io/badge/Build-Passing-brightgreen.svg)]()
[![Tests](https://img.shields.io/badge/Tests-190%20passed-brightgreen.svg)]()
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](licence.md)
[![Display](https://img.shields.io/badge/Resolution-1080p%20%7C%20720p%2032--bit%20RGB-purple.svg)]()
[![Min RAM](https://img.shields.io/badge/Min%20RAM-4%20MB-red.svg)]()
[![Min CPU](https://img.shields.io/badge/Min%20CPU-x86__64%20SSE2-red.svg)]()
[![Lines of Code](https://img.shields.io/badge/Lines%20of%20Code-89%2C992%20(Rust)-lightgrey.svg)]()
[![GitHub Release](https://img.shields.io/badge/Release-v2.1.0-brightgreen.svg)](https://github.com/tbagriyanik/minos/releases)
[![Wiki](https://img.shields.io/badge/Docs-GitHub%20Wiki-blueviolet.svg)](https://github.com/tbagriyanik/minOs/wiki)

**minOs (Sarman Edition) v2.1.0** is a standalone 64-bit (`x86_64`) operating system written in Rust that uses `no_std` and `no_main` in the kernel. During the build, bitmap atlases are generated from TTF fonts using `fontdue`; the kernel uses these atlases in its graphical interface.

> 📚 **Official Guide and Documentation:** Visit the **[minOs GitHub Wiki](https://github.com/tbagriyanik/minOs/wiki)** for all hardware drivers, application guides, network/TLS configuration and developer documents.

---

## What's New (v2.1.0)

- **🔐 TLS 1.3 HTTPS Handshake Completed:** The browser's `https://` connections can now complete the TLS 1.3 handshake end-to-end and process the encrypted HTTP response. Three critical fixes were made in line with RFC 8446:
  - **AAD length (§5.2):** The Additional Authenticated Data of encrypted records is now computed as exactly 5 bytes (content type + version + length); the previous 11-byte length caused a GCM tag mismatch.
  - **Finished key derivation (§4.4.4):** `finished_key` is derived directly from `server/client_hs_traffic_secret`; the extra HKDF steps in between caused a `verify_data` mismatch and stalled the handshake at the `CertificateReceived` stage.
  - **Graceful `close_notify` (§6.1):** The server's `close_notify` (Alert 0) is no longer treated as a fatal error; by setting the `peer_closed` state, application data delivered in the same record is preserved. Response bodies are therefore displayed in the browser after `state=Established`.
- **🌐 Browser Page Download and HTML Integration:** Pages downloaded from the browser are saved under `/root/Downloads/` with the `.html` extension. Double-clicking `.html`, `.htm`, `.xhtml` and `.url` files in the File Manager reads the file from the VFS and opens it in the browser via `load_html`.
- **🐾 Browser Demo Pages:** Three local, network-free demo pages were added to the browser: `minos://demo` (basic HTML/CSS), `minos://demo-img` (image layout) and `minos://demo-video` (video layout). These pages can be used to exercise the HTML/CSS engine and layout capabilities offline.
- **Expanded Hardware Drivers (20+ New Drivers):**
  - ✅ **Networking:** VirtIO-net, Intel I219-V, Atheros, Marvell Ethernet.
  - ✅ **Bluetooth HCI Protocol:** Command (0x01), ACL data (0x02) and Event (0x04) packet formatting/parsing, inquiry and connection management completed.
  - ✅ **Storage:** AHCI/SATA, NVMe SSD, VirtIO-blk, SD/MMC.
  - ✅ **USB:** Mass Storage, Printer, Audio, Touchscreen.
  - ✅ **Graphics:** VirtIO-gpu (split VirtQueue, 2D command engine, default desktop hardware 2D render & flush path, mode-set), AMD Radeon and NVIDIA GeForce (BAR0/BAR1 VRAM framebuffer, 2D primitives, mode-set).
  - ✅ **Sensors & Power:** ACPI AML interpreter (Method, Device, Scope, Math/Logic), `_BST` / `_BIX` / `_BIF` battery status reading, `_S0`..`_S5` sleep states and PowerResource management, ThermalZone (`_TZ_`, `_TMP`, `_CRT`) and fan sensors.
  - ✅ **Bus & Timing:** PCIe, PCI and Timer (PIT/HPET/APIC/TSC) infrastructure.
- **Live USB Hot-plug:** USB devices are detected instantly while running and toast notifications are shown. When a USB keyboard or mouse is plugged in or removed while minOs is running, the system detects it immediately. A live toast notification is shown on the desktop (`USB keyboard connected`, `USB mouse removed`), open screens/windows are redrawn and the hardware list in System Settings > Devices is updated instantly.
- **OHCI & UHCI & EHCI Transfer Layer:** A complete transfer layer and port handoff infrastructure was integrated for OHCI (`OhciEd`, `OhciTd`, `OhciHcca` and DMA queue management), UHCI (`UhciQh`, `UhciTd`) and EHCI controllers.
- **minJs Advanced JavaScript Runtime:** The built-in `no_std` JavaScript engine for the web browser was expanded: binary arithmetic operators with operator precedence (`+`, `-`, `*`, `/`, `%`, `(...)`, unary `+`/`-`/`!`), loops (`for (init; cond; step)`, `for (item of array)`, `for (key in object)`, `while`, `break`, `continue`), variable mutations (`++`, `--`, `+=`, `-=`, `*=`, `/=`, `%=`), array methods (`.push()`, `.pop()`, `.shift()`, `.unshift()`, `.reverse()`, `.join()`, `.indexOf()`, `.includes()`, `.slice()`, `.concat()`), object properties and bracket indexing (`obj.prop`, `obj["key"]`, `delete obj.prop`), `Object.keys()` / `Object.values()`, JSON parsing and serialization (`JSON.stringify()`, `JSON.parse()`), math and string helpers (`Math.{abs, min, max, pow, round, floor, ceil}`, `str.{indexOf, includes, slice, split, trim, toLowerCase, toUpperCase, charAt}`) and global conversions (`parseInt`, `parseFloat`, `String`, `Number`, `Boolean`).
- **IPv6 Protocol & Socket Infrastructure:** `Ipv6Address`, `IpAddress` (V4/V6), an RFC 4291 address parser (`::` compression and `[...]` format), RFC 2464 Multicast MAC mapping, RFC 8200 pseudo-header checksum, ICMPv6 Echo Request/Reply (`ping6`), Neighbor Discovery (Solicitation/Advertisement) and `ETHERTYPE_IPV6` (0x86DD) support were added to the `netstack.rs` layer; terminal `ipconfig` local IPv6 (Link-Local EUI-64) display and dual-stack `ping`/`ping6` commands; `AF_INET6` (10), `resolve_ip()` / `resolve_ipv6()` and IPv6 address validation in the web browser were added to the `socket.rs` layer.
- **exFAT and ext4 Fragmentation & Deep Directory Integrity:** The ability to create files in deep directory trees, punch holes by deleting files and consistently allocate new files that fill fragmented block/cluster chains on ext4 and exFAT was verified with host stress tests (`host_fs_tests.rs`).
- **Expanded Trust Store (Root CA):** The Let's Encrypt ISRG Root X1 & X2, Google Trust Services (GTS Root R1 & R2), DigiCert Global Root (CA & G2) and SSL.com TLS ECC Root CA 2022 root certificates were embedded into the kernel (`src/drivers/crypto/roots/`).
- **ISRG Root X1 Root CA & TLS Chain Verification:** The Let's Encrypt ISRG Root X1 root certificate (`isrgrootx1.der`) is embedded in the kernel. The certificate chain received from the server is scanned with RFC 5280 rules and the RSA PKCS#1 v1.5 and ECDSA P-256 signatures of leaf and intermediate certificates are fully verified up to the trusted ISRG Root X1 public key.
- **Devices Behind a USB Hub (xHCI):** A multi-tier USB hub architecture is natively supported. Hub descriptors (`GET_DESCRIPTOR 0x29`), port power control (`PORT_POWER`), port reset (`PORT_RESET`), the xHCI 20-bit hierarchical `Route String` and the Transaction Translator (TT / Split Transaction) mechanism were integrated. Keyboards and mice behind a hub are automatically enumerated and driven.
- **EHCI (USB 2.0), UHCI (USB 1.1) and OHCI Controller Support:** EHCI EECP OS handoff, CONFIGFLAG port routing, UHCI Global/Host Reset and OHCI operational mode transitions were implemented so that USB ports and BIOS legacy emulation work reliably on older motherboards and laptops that have no xHCI controller.
- **Smart Error and Warning Coloring in the Boot Console:** Undetected hardware and error or warning lines (`[BOOT]`, `[INPUT]`, `[DRIVER]`) in the UEFI/BIOS text console are automatically highlighted in red (`#FF6B6B`) on the boot screen.
- **Notepad – Find & Replace and Overflow Menu:** `Ctrl+F` opens the **Find** bar and `Ctrl+H` opens the **Replace** bar. Text input is routed to the active field and `Esc` closes the bar. Advanced search options and an overflow menu for small screens were integrated.
- **Realtek PCIe Network Card Support (RTL8168/8169/8125/8101):** Native driver support was integrated for the most widely used Realtek Gigabit and Fast Ethernet controllers on physical motherboards and PCIe expansion slots. Multi PCI bus scanning (Bus 0..8), C+ mode initialization, 64-bit RX/TX descriptor rings and live hardware cable/link status (PHYSTATUS) tracking were added.
- **Broadcom NetXtreme Network Card Support (BCM57xx, PCI 0x14E4):** A native driver was added for Broadcom, the fourth major NIC vendor. The BCM5701–5782 family is recognized; Register Access Enable, MAC hash table, RX/TX descriptor rings and link status (PHYSTATUS) support are provided.
- **Smooth Mouse & Ballistic Acceleration on High-Resolution Displays (1080p+):** Clipping of the 12/16-bit movement data in USB mouse reports to a narrow range was prevented, providing full axis sensitivity. Resolution-scaled dynamic base speed (~2.8x) for 1080p and 2K/4K screens and a 4-stage ballistic acceleration that does not disturb precise pixel clicks were applied.
- **Renewed Medium-Sized & High-Contrast Mouse Cursor:** A modern 15-pixel sharp arrow cursor with an ideal size for 1080p and higher resolutions was designed. The full 8-direction box outline provides high contrast and clear visibility on all light and dark backgrounds; the VRAM backup buffer was increased to 32x32 pixels to prevent cursor trails.
- **X.509 Certificate Verification (Time & SNI/CN):** During the TLS handshake, the validity period of the server certificate (`not_before` / `not_after`) is verified against the CMOS RTC clock and the target host name is verified against CN (`CommonName`) and SAN (`Subject Alternative Name`) DNS records using RFC 6125 rules (including single-level wildcard `*.domain.com`).
- **Certificate Chain Depth Limit:** `verify_certificate_chain` in `src/drivers/crypto/x509.rs` verifies chains with at most `MAX_CHAIN_DEPTH` (6) certificates to bound the DER parsing cost and memory of extremely long enterprise/private CA chains in a kernel without an intermediate certificate cache; longer chains are rejected with `ChainTooDeep` before parsing.
- **Certificate Revocation Checking (CRL & OCSP parsing):** `src/drivers/crypto/revocation_core.rs` parses DER-encoded CRL (`CertificateList`) and OCSP (`BasicOCSPResponse`) responses; revocation status, serial number, revocation time/reason and response freshness are checked. `revocation.rs` applies a fail-closed policy: the connection is rejected when the revocation status is unknown. Limits: automatic downloading of CRL/OCSP responses (HTTP/OCSP request generation) is not implemented yet; manual marking with the `Manual` source is available.
- **VMware VMXNET3 & Expanded Network Support:** In addition to the Intel e1000 family, VMware PCI vendor `0x15AD` and device IDs (`0x07B0` VMXNET3, `0x0720` VMXNET) are recognized; MAC address, link polling and hardware name reporting were integrated.
- **minPak / minpkg SHA-256 Integrity Verification:** Package manager gained the ability to verify the SHA-256 digest of downloaded and installed binary packages (`minPak install <package> [hash]`, `minPak sha256 <file>`, `minPak verify <file> <hash>`).
- **minpkg Dependency Resolution:** `src/apps/pkg_deps.rs` handles version constraints (`>=`, `>`, `=`, `<=`, `<`), transitive dependency ordering, conflict and cycle detection. `minPak install` installs local catalog packages in dependency order; `minPak depends <package>` shows the resolution plan.
- **USB Keyboard and Mouse Improvements (xHCI):** Core bugs in xHCI initialization were fixed: the `HCSPARAMS1.MaxPorts` field was read from the wrong bit range, MMIO register accesses added the base address twice, port registers had to be read at 16-byte instead of 4-byte strides and the A0 (Slot Context) flag was missing in the `Address Device` context. With these fixes USB keyboards/mice are successfully enumerated on QEMU xHCI. The off-by-one in the HID usage→PS/2 mapping was also fixed (arrow keys, Home/End, Insert/Delete and the numeric keypad now work correctly); report descriptors are parsed correctly, adding absolute/relative distinction, 12/16-bit axes, Report ID and wheel support for mice that do not offer the boot protocol. All HID interfaces of multi-interface devices such as wireless keyboard+mouse receivers run on the same xHCI slot; Caps Lock/Num Lock LEDs are fed back to the device and DMA memory leaks during plug/unplug were prevented.
- **USB HID & PS/2 Fallback Diagnostics in the Boot Console:** When no native xHCI USB HID controller or device is found at boot, a clear diagnostic log is printed to the boot console and serial port (`[INPUT] USB HID not found, using PS/2 fallback`).
- **Automatic Resolution and Persistence at Boot:** The screen resolution selected in the Limine boot menu (`kernel_cmdline: res=WxH`) and the preferences saved to disk from the Settings app (`settings.cfg`) are applied to the hardware (BGA) automatically before the desktop opens; the system boots directly with the selected resolution.
- **Dynamic Resolution and Desktop Adaptation:** When the resolution is changed at runtime, the desktop bounds, cursor movement area, window positions (`keep_windows_on_screen`) and wallpaper adapt instantly to the new resolution; the screen is fully redrawn (`force_render`).
- **QEMU SDL / GTK Support & 64 MB VRAM:** A `-Display sdl` parameter was added to the `run.ps1` script (GTK pixbuf warnings are suppressed, lower mouse latency). QEMU video memory was raised to 64 MB (`-global VGA.vgamem_mb=64`).
- **Static BSS Memory Architecture (CodeQL Alert #25 Fix):** The kernel memory manager (`MemoryManager`) was moved to static BSS storage, making stack memory escapes and raw pointer resolution safe.
- **Expanded Crypto & TLS 1.3:** SHA-384, SHA-512 and ECDSA (secp256r1) signature verification/generation primitives were added (190 host tests passing); the full TLS 1.3 handshake loop (RSA & ECDSA CertificateVerify, HKDF key schedule and encrypted record processing) was integrated.
- **Early Exception Catching & Hardware Triple Fault Protection:** From the very first moment of boot, the BSP GDT+TSS (`IST1`) and IDT exception gates (`#DE`, `#UD`, `#DF`, `#SS`, `#GP`, `#PF`) are loaded. Sudden reboots of the motherboard are prevented on hardware faults that may occur during driver initialization.
- **Interactive Text Recovery Console (`recovery#`):** When an error occurs at boot or in critical drivers, error details (vector, RIP, CR2, error code) are shown on screen before switching to the graphical welcome screen and an interactive prompt is offered to the user (`restart` / `reboot` / `r`, `halt` / `h`, `help`).
- **Expanded High Resolution and 16:10 Support:** The kernel backbuffer was enlarged to `2560x1600`. 1920x1080 (FHD), 1920x1200 (16:10 WUXGA), 2560x1440 (2K) and 2560x1600 screens are fully supported; the 1920 bound-check algorithm was fixed.
- **Dynamic VRAM MMIO Mapping:** At boot the entire 16 MB VRAM range is mapped into the page tables as 2 MB huge pages; page faults caused by VRAM bounds overruns during resolution changes or window rendering are prevented.
- **Intel HDA & Modern Laptop Compatibility:** Timeouts and PCI multi-function checks were added for Intel Smart Sound (DSP/cAVS) chipsets; when the hardware does not respond, the system safely falls back to PC Speaker PWM mode instead of crashing.
- **Settings Window Click Isolation:** Clicking the text zoom (%100, %125, %150) buttons no longer accidentally triggers the resolution buttons; safe display mode management is provided by restricting BGA writes on real hardware.
- **Default Hardware Driver Profile:** The full driver profile is used in normal Live and Installed boots; the Safe Mode option in the Limine menu is preserved for boots without drivers.
- **More Readable Boot Log:** The text console flushes every completed line to the screen and waits 45 ms before moving to the next line once the time base is ready. Boot diagnostic messages are therefore easier to follow; early messages written before the time base is ready are not delayed.
- **Browser Favorites (Bookmarks Bar):** An interactive bookmarks bar was added to the web browser; a pure HTTP test site and quick-access buttons for `minos://cats`, `minos://net`, `minos://history`, `minos://demo` and `minos://demo-img` / `minos://demo-video` plus the `minos://bookmarks` page were integrated.

### 🛡️ Boot Recovery Console (`recovery#`)

If a hardware exception (`#PF`, `#GP`, `#DF`) or a fault is detected at boot or in critical drivers, error details are shown on screen before switching to the graphical welcome screen and an interactive prompt opens:

- **`[Enter]`** or **`restart`** / **`reboot`** / **`r`**: Restarts the system immediately and safely.
- **`halt`** / **`shutdown`** / **`h`**: Halts the system (`hlt`).
- **`help`**: Lists the available commands.
- Detailed guide: [Boot Error and Recovery Console Guide](docs/usage.md#️-açılış-hata-ve-kurtarma-konsolu-boot-recovery-console)

---

## 🧭 Hardware & Driver Support Matrix

Current status table of native drivers and hardware layers inside the minOs kernel:

> **Status Indicators:**
> - 🟢 **Full Support:** Stable for daily use, all core and advanced functions fully operational.
> - 🟡 **Partial Support:** Core functionality and the driver are operational; some advanced profiles/features are under development.
> - 🔴 **Skeleton / WIP:** Device detection or infrastructure draft exists; full protocol integration is planned.

| Category | Hardware / Device | Driver Module | Status | Scope & Capabilities |
| :--- | :--- | :--- | :---: | :--- |
| **Graphics & Display** | Bochs BGA / QEMU Standard VGA | `gpu.rs` | 🟢 Full | 1080p+, dynamic resolution change, LFB |
| | VirtIO-GPU (QEMU/KVM) | `virtio_gpu.rs` | 🟢 Full | Split VirtQueue, 2D command set, scanout, default desktop hardware 2D render & flush path, mode-set |
| | NVIDIA GeForce GPU | `nvidia.rs` | 🟡 Partial | BAR0/BAR1 VRAM LFB, 2D drawing, mode-set (3D acceleration planned) |
| | AMD Radeon GPU | `amdgpu.rs` | 🟡 Partial | BAR0 64-bit VRAM LFB, 2D drawing, mode-set (3D acceleration planned) |
| | VMware SVGA-II | `gpu.rs` | 🟡 Partial | PCI detection, basic resolution reading |
| **Network & Communication** | Intel e1000 / e1000e (Gigabit) | `e1000.rs` | 🟢 Full | DMA ring, DHCP, DNS, TCP/UDP, IPv4/IPv6, ICMPv6, TLS 1.3 |
| | Intel I219-V Ethernet | `i219.rs` | 🟢 Full | Gigabit Ethernet link and packet transfer |
| | Realtek PCIe (RTL8168/8169/8125) | `net.rs` | 🟢 Full | C+ mode, 64-bit RX/TX rings, PHYSTATUS link tracking |
| | Broadcom NetXtreme (BCM57xx) | `net.rs` | 🟢 Full | MAC hash, RX/TX, PHY link check |
| | VirtIO-net (QEMU/KVM/Cloud) | `virtio_net.rs` | 🟢 Full | High-speed virtualized network adapter |
| | Atheros Ethernet | `atheros.rs` | 🟡 Partial | Basic link and packet transfer (offload filters TODO) |
| | Marvell Ethernet | `marvell.rs` | 🟡 Partial | Basic RX/TX packet processing |
| | Bluetooth HCI | `bluetooth.rs` | 🟡 Partial | Command (0x01), ACL (0x02), Event (0x04), Inquiry, connection management (A2DP/GATT TODO) |
| | Wi-Fi (802.11 Wireless) | `wifi.rs` | 🔴 Skeleton | PCI/USB interface detection (WPA handshake WIP) |
| **Input Devices** | PS/2 Keyboard & Mouse | `ps2.rs`, `keyboard.rs`, `mouse.rs` | 🟢 Full | Interrupt-driven input, Turkish Q / English key mapping |
| | USB HID Keyboard & Mouse | `usb_hid.rs`, `usb_transfer.rs` | 🟢 Full | xHCI/EHCI/UHCI/OHCI full transfer layer (QH, TD, ED, HCCA), 16-bit axes, wheel, hotplug |
| | USB Touchscreen | `usb_touchscreen.rs` | 🟡 Partial | Single-finger touch/click (multi-touch analysis TODO) |
| **Storage** | ATA / IDE / ATAPI | `ata.rs` | 🟢 Full | LBA28/48, hard disk and CD/DVD-ROM |
| | AHCI / SATA | `ahci.rs` | 🟢 Full | SATA SSD and HDD block I/O |
| | NVMe SSD (PCIe M.2) | `nvme.rs` | 🟢 Full | NVMe command queues and block read/write |
| | VirtIO-blk (QEMU/KVM) | `virtio_blk.rs` | 🟢 Full | Virtual block device integration |
| | USB Mass Storage (Flash Drive) | `usb_mass_storage.rs` | 🟢 Full | Storage over the SCSI BOT transfer protocol |
| | SD / MMC Card | `sdmmc.rs` | 🟡 Partial | Sector reading active (CSD parsing and reliable writes WIP) |
| **File Systems** | exFAT & ext4 | `exfat_rw.rs`, `ext4_rw.rs` | 🟢 Full | Deep directory hierarchy, fragmentation resistance, full read/write |
| | ext2 & FAT32 | `ext2.rs`, `diskfs.rs` | 🟢 Full | File reading and system directories |
| **Power & Sensors** | ACPI AML Interpreter & Battery | `acpi_battery.rs` | 🟢 Full | DSDT/SSDT AML parsing (Method, Device, Scope, Math/Logic), `_BST`/`_BIX`/`_BIF` live percentage, voltage, current, cycles |
| | ACPI Power Management | `acpi_power.rs` | 🟢 Full | `_S0`..`_S5` sleep states detection, PowerResource (`_ON_`/`_OFF`/`_STA`), dynamic AC/Battery status tracking |
| | Fan & Temperature Sensors | `fan_temp.rs` | 🟡 Partial | ACPI ThermalZone (`_TZ_`, `_TMP`, `_CRT`, `_HOT`, `_PSV`) parsing, Celsius conversion and fan RPM monitoring |
| | Bus & Timers | `pci.rs`, `pcie.rs`, `timer.rs` | 🟢 Full | PCI/PCIe configuration, TSC/HPET/APIC timers |
| **Audio & Multimedia** | Intel High Definition Audio (HDA) | `hda.rs`, `audio.rs` | 🟢 Full | DMA stream rings, mixer, WAV/MP3 playback |
| | PC Speaker (PWM) | `audio.rs` | 🟢 Full | Hardware timer beeps and PWM tones |
| | USB Audio | `usb_audio.rs` | 🟡 Partial | Speaker/headphone output (microphone recording TODO) |

---

### ⚠️ Roadmap & Advanced Development

- **Bluetooth Upper-Layer Profiles:** The basic HCI layer, inquiry and connection management are ready; A2DP audio streaming and BLE GATT profiles are planned for future releases.
- **NVIDIA / AMD 3D Acceleration:** Linear frame buffer (VRAM LFB), 2D drawing and dynamic resolution change are ready; 3D acceleration (Vulkan/Gallium) is a separate stage.
- **Sensor & Fan Management:** Hardware-specific EC (Embedded Controller) fan profiles for `fan_temp.rs`.
- **Atheros / Marvell Ethernet:** Basic RX/TX transfer is active; hardware checksum offload and advanced packet filters are planned.
- **USB Audio Recording:** Speaker output (playback) is active; PCM recording via microphone is TODO.
- **SD/MMC Card Writing:** Sector reading is active; CSD register parsing and block write reliability are WIP.
- **CRL/OCSP Revocation Checking:** Parsing algorithms are integrated; automatic HTTP/OCSP download requests are planned.
- **IPv6 DNS (AAAA) Resolution:** The IPv6 data path and `resolve_ipv6()` are ready, but they only parse **address literals** (`[2001:db8::1]`). `dns_query()` emits only **QTYPE=A (1)** queries and `dns_parse_a()` accepts only A records in the response; **no AAAA (28) query is sent and AAAA answers are not parsed**. Host names therefore resolve to IPv4 only, and IPv6-only (AAAA-only) targets cannot be reached by name; the `dns`/`nslookup` output reports A records only.
- **minJs JavaScript Limitations:** minJs supports functions, arrays, objects, loops (`for`/`while`), JSON, and core DOM APIs in a `no_std` environment; advanced ECMAScript features such as lexical closures, prototype inheritance chains, class inheritance, and async/await/Promises are not yet implemented.

---

## 💽 Ready-to-Run ISO Image

A ready ISO image for testing minOs directly on QEMU, VirtualBox (64bit) or real hardware without building:

🔗 **Download Directly from the Repo:**
[https://github.com/tbagriyanik/minos/blob/main/minos.iso](https://github.com/tbagriyanik/minos/blob/main/minos.iso)

---

## 🎥 Promo Video

[![Watch the minOs promo video](https://img.youtube.com/vi/fB1MQPZTMEg/hqdefault.jpg)](https://www.youtube.com/watch?v=fB1MQPZTMEg)

▶️ [Watch the video on YouTube](https://youtu.be/fB1MQPZTMEg)

---

## 📸 Screenshots

### Welcome Screen
![Welcome](docs/screenshots/welcome.png)

### Theme 1
![Theme 1](docs/screenshots/tema1.png)

### Theme 2
![Theme 2](docs/screenshots/tema2.png)

---

## 📚 Documentation and Wiki

Detailed guides and technical reference resources:

- 📖 **[minOs GitHub Wiki (Home)](https://github.com/tbagriyanik/minOs/wiki)**
  - ℹ️ **[About & Architecture](https://github.com/tbagriyanik/minOs/wiki/Hakkinda)**: Design philosophy, `no_std` model, PMM and IDT structure.
  - 🚀 **[Installation and Running](https://github.com/tbagriyanik/minOs/wiki/Kurulum-ve-Calistirma)**: `run.ps1` options, creating a Live USB and installing to disk.
  - 💻 **[Supported Hardware List](https://github.com/tbagriyanik/minOs/wiki/Desteklenen-Donanimlar)**: xHCI USB HID, Touchpad (tap-to-click), UVC camera, e1000 and Wi-Fi.
  - 📦 **[15 Built-in Applications](https://github.com/tbagriyanik/minOs/wiki/Uygulamalar)**: Web Browser, Notepad, File Manager, Control Panel, Package Manager etc.
  - 🌐 **[Network and Internet](https://github.com/tbagriyanik/minOs/wiki/Ag-ve-Internet)**: TCP/IP stack, DNS, HTTP/1.1 and TLS 1.3 (HTTPS).
  - 💾 **[File Systems](https://github.com/tbagriyanik/minOs/wiki/Dosya-Sistemleri)**: VFS, FAT32 (r/w/format), Ext2, ISO9660 and persistence (`VFS.DAT`).
  - ⌨️ **[User Guide and Shortcuts](https://github.com/tbagriyanik/minOs/wiki/Kullanici-Rehberi)**: Desktop shortcuts and MinShell command reference.
  - 🛠️ **[Developer Guide](https://github.com/tbagriyanik/minOs/wiki/Gelistirici-Rehberi)**: Source tree map, unit tests (`test_host.ps1`) and building.
  - 🗺️ **[Release History and Roadmap](https://github.com/tbagriyanik/minOs/wiki/Surum-Gecmisi)**: Release notes and future plans.

Local Markdown documents (Turkish):
- [Features and Roadmap](docs/features.md)
- [Commands, Shortcuts and Project Structure](docs/usage.md)
- [Hardware, Requirements and Building](docs/building.md)
- [Previous Release Notes](docs/releases.md)

---

## 💾 File System Support and Format Limits

The minOs VFS layer supports several file systems, but each file system's capability level differs:

| File System | Support Level | Description |
| :--- | :--- | :--- |
| **FAT32** | **Full Read / Write / Format / Persistence** | Full file reading, writing, creation and deletion on MBR/GPT disks; `settings.cfg` and `VFS.DAT` system persistence. |
| **Ext2** | **Read / Write / Format** | Mounting ATA disk partitions via `src/fs/ext2.rs`, directory listing and file read/write. |
| **Ext4** | **Read / Write / Format** | `src/fs/ext4.rs` (format) + `src/fs/ext4_rw.rs` (read/write) with 1024-byte blocks, 256-byte inodes and multiple block groups. Block mapping is ext2-compatible (direct + single/double indirect); **extents tree, journal (JBD2), htree (dir_index) and inline data are not supported** — volumes with these features enabled are not mounted. The Linux `ext4` driver mounts the volume without problems. |
| **exFAT** | **Read / Write / Format** | `src/fs/exfat.rs` (format: VBR, FAT, allocation bitmap, up-case table) + `src/fs/exfat_rw.rs` (read/write: root/sub directories, file creation/deletion, FAT-chained multi-cluster files, Unicode names). Full validation of long-name entries and timestamp/ACL writing are not yet available. |
| **ISO9660** | **Read-only** | Read support for live CD/DVD and installation media contents. |

---

### ⚡ Kernel Architecture and Hardware Status (UEFI, USB-HID, SMP)

- **UEFI & BIOS Dual-Boot:** Thanks to the Limine v8 bootloader, the system boots smoothly on both modern x86_64 UEFI systems and traditional BIOS (Legacy) environments.
- **Native xHCI USB HID & PS/2 Fallback:** A native USB keyboard and mouse driver (`src/drivers/usb_hid.rs`) is active over xHCI (USB 3.x) controllers. Initialization/handoff and a transfer layer exist for EHCI/UHCI/OHCI controllers (see `src/drivers/usb.rs`), but HID report parsing is not yet available on these controllers; when no USB device is found or BIOS CSM/Legacy USB is disabled, the system automatically switches to PS/2 fallback mode for stability and logs an informational message to the boot console. To completely disable PS/2 in QEMU and test input only through a USB keyboard/mouse use `.\run.ps1 -NoPs2`; for end-to-end verification including automated keyboard/mouse injection over QMP use `.\test_qemu.ps1 -NoPs2`.
- **SMP (Symmetric Multiprocessing) Infrastructure:** The kernel includes the Limine SMP protocol, Local APIC timer/IPI, TTAS spinlock synchronization and per-core CPU monitoring bars in Task Manager; the default profile starts with single-core safety for physical laptop boot stability.

---

### 💻 Direct QEMU Run Examples (Including Audio and Network)

If you want to run directly from the QEMU command line without the `run.ps1` automation, the audio (HDA) and network (e1000) flags must be supplied completely:

#### Linux (PulseAudio + Intel HDA Duplex + e1000):
```bash
qemu-system-x86_64 -m 512M -smp 2 \
  -cdrom minos.iso -boot d \
  -vga virtio \
  -rtc base=utc,clock=host,driftfix=slew \
  -audiodev pa,id=snd0 \
  -device intel-hda -device hda-duplex,audiodev=snd0 \
  -netdev user,id=n0 -device e1000,netdev=n0
```

#### Windows (SDL or DirectSound + Intel HDA Output + e1000):
```powershell
# The SDL audio backend prevents the DirectSoundCapture (microphone open) error:
qemu-system-x86_64.exe -m 512M -smp 2 `
  -cdrom .\minos.iso -boot d `
  -vga virtio `
  -rtc base=utc,clock=host,driftfix=slew `
  -audiodev driver=sdl,id=snd0 `
  -device intel-hda -device hda-output,audiodev=snd0 `
  -netdev user,id=n0 -device e1000,netdev=n0
```
> 💡 *Note: If you use DirectSound, using `hda-output` instead of `hda-duplex` or `-audiodev driver=sdl` prevents QEMU startup crashes caused by a missing microphone capture device.*
> 💡 *Note: VirtIO-GPU (`-vga virtio`) provides direct 2D hardware acceleration and scanout flush queues in QEMU/KVM environments. For legacy/standard VGA, `-vga std -global VGA.vgamem_mb=64` can be used.*
> 💡 *Note: In QEMU, VirtualBox, and VMware, hardware RTC operates in UTC and minOs applies the timezone offset (`settings.cfg: region`, default UTC+3) to ensure exact synchronization with host real time.*

---

### 🚀 Quick Start

```powershell
git clone https://github.com/tbagriyanik/minOs.git
cd minOs
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

---

## 📄 License

This project is open source and developed for educational/research purposes.
Feel free to contribute and suggest improvements! 🐾
