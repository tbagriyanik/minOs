# 🐱 minOs - Sarman Edition

[English](README.en.md) | [Türkçe](README.md)

[![Rust](https://img.shields.io/badge/Language-Rust%20nightly-orange.svg)](https://www.rust-lang.org/)
[![Architecture](https://img.shields.io/badge/Architecture-x86__64%20Long%20Mode-blue.svg)]()
[![Bootloader](https://img.shields.io/badge/Bootloader-Limine%20v8-green.svg)](https://limine-bootloader.org/)
[![Build](https://img.shields.io/badge/Build-Passing-brightgreen.svg)]()
[![Tests](https://img.shields.io/badge/Tests-234%20registered-brightgreen.svg)]()
[![Trusted Root CAs](https://img.shields.io/badge/Trusted%20Root%20CAs-51-blue.svg)]()
[![Display](https://img.shields.io/badge/Resolution-1080p%20%7C%20720p%2032--bit%20RGB-purple.svg)]()
[![Min RAM](https://img.shields.io/badge/Min%20RAM-128%20MB-red.svg)]()
[![Min CPU](https://img.shields.io/badge/Min%20CPU-x86__64%20SSE2-red.svg)]()
[![Lines of Code](https://img.shields.io/badge/Lines%20of%20Code-119%2C427%20(Rust)-lightgrey.svg)](https://github.com/tbagriyanik/minos)
[![GitHub Release](https://img.shields.io/badge/Release-v2.3.0-brightgreen.svg)](https://github.com/tbagriyanik/minos/releases)
[![Wiki](https://img.shields.io/badge/Docs-GitHub%20Wiki-blueviolet.svg)](https://github.com/tbagriyanik/minOs/wiki)

**minOs (Sarman Edition) v2.3.0** is a standalone 64-bit (`x86_64`) operating system written in Rust that uses `no_std` and `no_main` in the kernel. During the build, bitmap atlases are generated from TTF fonts using `fontdue`; the kernel uses these atlases in its graphical interface.

> 📚 **Official Guide and Documentation:** Visit the **[minOs GitHub Wiki](https://github.com/tbagriyanik/minOs/wiki)** for all hardware drivers, application guides, network/TLS configuration and developer documents.

---

## What's New (v2.3.0)

- **🔊 Direct WAV Playback:** Double-clicking a `.wav` file in File Manager plays that selected file in Music Player.
- **🖼️ Tile Image on Desktop:** Image Viewer can apply PNG/JPEG/GIF/BMP images as tiled desktop wallpapers.
- **🌄 New Desktop Images:** Settings now includes `desktop1.png`, `desktop2.png`, and `desktop3.png` wallpaper choices.
- **📁 ISO Sample Media:** Sample PNG/JPEG/BMP and WAV files are packaged under `/root/Media` in the ISO and are available at `/media/cdrom/root/Media` when booted from CD.

- **🔐 TLS 1.3 HTTPS Handshake Completed:** The browser's `https://` connections can now complete the TLS 1.3 handshake end-to-end and process the encrypted HTTP response. Three critical fixes were made in line with RFC 8446:
  - **AAD length (§5.2):** The Additional Authenticated Data of encrypted records is now computed as exactly 5 bytes (content type + version + length); the previous 11-byte length caused a GCM tag mismatch.
  - **Finished key derivation (§4.4.4):** `finished_key` is derived directly from `server/client_hs_traffic_secret`; the extra HKDF steps in between caused a `verify_data` mismatch and stalled the handshake at the `CertificateReceived` stage.
  - **Graceful `close_notify` (§6.1):** The server's `close_notify` (Alert 0) is no longer treated as a fatal error; by setting the `peer_closed` state, application data delivered in the same record is preserved. Response bodies are therefore displayed in the browser after `state=Established`.
- **🌐 Browser Improvements (Caching & Visualization):** VFS-based cache system added for HTTP/HTTPS requests (`/browser/cache/`); CSS, images and JS files are reused. Browser CSS support expanded: `transform` (translate/scale/rotate), `transition`/`animation`, basic `flexbox`/`grid`, shadows (`box-shadow`, `text-shadow`), border-radius, opacity and better color support. Image loading/processing improved (`imgdecode`), SVG representation support added and page rendering stabilized. Browser settings tab expanded. Pages downloaded from the browser are saved under `/root/Downloads/` with `.html` extension. Double-clicking `.html`, `.htm`, `.xhtml` and `.url` files in File Manager reads from VFS and opens in browser via `load_html`.
- **🌍 Progressive JPEG & @font-face Support:** The image decoder (`imgdecode`) can now decode **progressive JPEG** (SOF2) files using multi-scan coefficient accumulation; previously these files were interpreted as baseline and rendered corrupted/incomplete. The CSS engine parses `@font-face` rules and records their font-family sources, and `font-family` is carried on `ElementStyle`. `@keyframes` animations now interpolate `color` and `background-color` (not just `opacity`) and continue across re-renders instead of restarting.
- **🎨 HTML Color and Image Support:** The web browser supports HTML/CSS colors and images in the following formats:
  - **Colors:** Named colors (140+ CSS named colors, e.g. `red`, `blue`, `aliceblue`), `#RGB`/`#RGBA`, `#RRGGBB`/`#RRGGBBAA`, `rgb()`/`rgba()`, `hsl()`/`hsla()` and `transparent`.
  - **Image formats:** PNG, JPEG (baseline and progressive), GIF (first frame), WebP (lossless/lossy), BMP (24/32-bit) and SVG (basic shapes).
  - **CSS properties:** `color`, `background-color`, `border-color`, opacity blending, `background-image: url(...)`, `background-size` (auto/cover/contain/stretch), `background-repeat`.
  - **Sources:** `data:image/...` (base64), local VFS files (`/root/Media/...`), remote HTTP/HTTPS URLs (asynchronous cached loading).
- **⏱️ IPv6-First HTTP Requests (Dual-Stack): `http_get_impl` resolves the **AAAA** record first and connects over IPv6 with `connect_v6` (the HTTPS path runs the TLS 1.3 handshake over the IPv6 socket); if no AAAA record exists or the IPv6 path fails, it falls back to the IPv4 `fetch_once` path. Connection-level IPv6 → IPv4 fallback is also implemented in `WebSocketConnection::connect`, `http2::connect` and the browser TLS path: if the IPv6 TCP/TLS connection fails after establishment, the client transparently retries over IPv4. 3xx redirect following and `Transfer-Encoding: chunked` decoding are also applied to IPv6 responses.
- **🧩 HTTP/2 Client Core:** `http.rs` now contains the RFC 7540 frame layer (the `h2` module: DATA/HEADERS/SETTINGS/PING/GOAWAY/WINDOW_UPDATE), an HPACK header block encoder for `:method`/`:scheme`/`:authority`/`:path`, `Http2Connection` (multiplexed streams over a single socket, `SETTINGS` ACK, per-stream `WINDOW_UPDATE`) and the `http2_get()` convenience function. HTTP/2 ALPN (`h2`, `http/1.1`) is now declared in all TLS ClientHello call sites (`http.rs`, `http2.rs`, browser); if the server does not select `h2`, the connection falls back to HTTP/1.1.
- **🐾 Browser Demo Pages:** Three local, network-free demo pages were added to the browser: `minos://demo` (basic HTML/CSS), `minos://demo-img` (image layout) and `minos://demo-video` (video layout). These pages can be used to exercise the HTML/CSS engine and layout capabilities offline.
- **🚫 Browser Freeze Fix (Non-blocking Resource Loading):** System freeze when opening web pages has been eliminated. Synchronous (blocking) `http_get()` calls for external resources (`<link rel="stylesheet">`, `<script src="...">`, `<img src="...">`) were removed from HTML rendering. Instead, resource URLs are collected into a `pending_resources` list and fetched one per frame asynchronously by the `poll_network()` state machine. The page re-renders after each resource is fetched, so network operations never block the main event loop and the browser remains responsive while pages load.
- **🛠️ Browser Developer Tools:** A DevTools panel toggled with `Ctrl+Shift+D` is now available in the browser. The `Elements` tab reports links, images, canvases, media, iframes, WASM modules and Workers; `Console` shows JavaScript alerts and error logs; `Network` shows request state and downloads; `Storage` shows cookies and form-autofill entries.
- **Expanded Hardware Drivers (20+ New Drivers):**
  - ✅ **Networking:** VirtIO-net, Intel I219-V, Atheros, Marvell Ethernet, Realtek RTL8139 and NE2000 (ISA PnP + PCI RTL8029AS).
  - ✅ **Bluetooth HCI Protocol:** Command (0x01), ACL data (0x02) and Event (0x04) packet formatting/parsing, inquiry and connection management completed.
  - ✅ **Storage:** AHCI/SATA, NVMe SSD, VirtIO-blk, SD/MMC.
  - ✅ **USB:** Mass Storage, Printer, Audio, Touchscreen.
  - ✅ **Graphics:** VirtIO-gpu (split VirtQueue, 2D command engine, default desktop hardware 2D render & flush path, mode-set), AMD Radeon and NVIDIA GeForce (BAR0/BAR1 VRAM framebuffer, 2D primitives, mode-set).
  - ✅ **Sensors & Power:** ACPI AML interpreter (Method, Device, Scope, Math/Logic), `_BST` / `_BIX` / `_BIF` battery status reading, `_S0`..`_S5` sleep states and PowerResource management, ThermalZone (`_TZ_`, `_TMP`, `_CRT`) and fan sensors.
  - ✅ **Bus & Timing:** PCIe, PCI and Timer (PIT/HPET/APIC/TSC) infrastructure.
- **Live USB Hot-plug:** USB devices are detected instantly while running and toast notifications are shown. When a USB keyboard or mouse is plugged in or removed while minOs is running, the system detects it immediately. A live toast notification is shown on the desktop (`USB keyboard connected`, `USB mouse removed`), open screens/windows are redrawn and the hardware list in System Settings > Devices is updated instantly.
- **OHCI & UHCI & EHCI Transfer Layer:** A complete transfer layer and port handoff infrastructure was integrated for OHCI (`OhciEd`, `OhciTd`, `OhciHcca` and DMA queue management), UHCI (`UhciQh`, `UhciTd`) and EHCI controllers.
- **minJs Advanced JavaScript Runtime:** The built-in `no_std` JavaScript engine for the web browser was expanded: binary arithmetic operators with operator precedence (`+`, `-`, `*`, `/`, `%`, `(...)`, unary `+`/`-`/`!`), loops (`for (init; cond; step)`, `for (item of array)`, `for (key in object)`, `while`, `break`, `continue`), variable mutations (`++`, `--`, `+=`, `-=`, `*=`, `/=`, `%=`), array methods (`.push()`, `.pop()`, `.shift()`, `.unshift()`, `.reverse()`, `.join()`, `.indexOf()`, `.includes()`, `.slice()`, `.concat()`), object properties and bracket indexing (`obj.prop`, `obj["key"]`, `delete obj.prop`), `Object.keys()` / `Object.values()` / `Object.entries()`, JSON parsing and serialization (`JSON.stringify()`, `JSON.parse()`), math and string helpers (`Math.{abs, min, max, pow, round, floor, ceil}`, `str.{indexOf, includes, slice, split, trim, toLowerCase, toUpperCase, charAt}`) and global conversions (`parseInt`, `parseFloat`, `String`, `Number`, `Boolean`). It also supports prototype chains (`Object.create` / `setPrototypeOf` / `__proto__`), getters/setters, `Proxy` traps, and a registry-based `import`/`export` module system.
- **minJs Closures and Timers:** **Closure** support was added to the minJs engine: function expressions (`var f = function (a) { ... }`), arrow functions (`var f = (a) => ...`, `a => expr`) and anonymous callbacks (`arr.forEach(function (x) { ... })`) capture the scope at creation time; each closure carries its own private state (as in a counter example). Function values can be assigned to variables and object properties and invoked as `obj.method()`. In addition, **`setTimeout(cb, ms)`** and **`setInterval(cb, ms)`** are registered as real timers — the callback may be a function name, a variable holding a closure, an arrow function, a function expression, or quoted raw JS code — and can be cancelled with `clearTimeout(id)` / `clearInterval(id)`; due timers are executed via `check_timers()`. Immediately-invoked function expressions (IIFE: `(function () { ... })()`) are also supported. Array callback methods (`forEach`, `map`, `filter`, `find`, `findIndex`, `some`, `every`, `reduce`) were added and accept closures/arrow functions.
- **IPv6 Protocol & Socket Infrastructure:** `Ipv6Address`, `IpAddress` (V4/V6), an RFC 4291 address parser (`::` compression and `[...]` format), RFC 2464 Multicast MAC mapping, RFC 8200 pseudo-header checksum, ICMPv6 Echo Request/Reply (`ping6`), Neighbor Discovery (Solicitation/Advertisement) and `ETHERTYPE_IPV6` (0x86DD) support were added to the `netstack.rs` layer; terminal `ipconfig` local IPv6 (Link-Local EUI-64) display and dual-stack `ping`/`ping6` commands; `AF_INET6` (10), `resolve_ip()` / `resolve_ipv6()` and IPv6 address validation in the web browser were added to the `socket.rs` layer.
- **exFAT and ext4 Fragmentation & Deep Directory Integrity:** The ability to create files in deep directory trees, punch holes by deleting files and consistently allocate new files that fill fragmented block/cluster chains on ext4 and exFAT was verified with host stress tests (`host_fs_tests.rs`).
- **Expanded Trust Store (Root CA):** 51 trusted root CA certificates embedded into the kernel (`src/drivers/crypto/roots/`). Includes Let's Encrypt ISRG Root X1 & X2, Google Trust Services (GTS Root R1 & R2), DigiCert Global Root (CA & G2), SSL.com TLS ECC Root CA 2022, GoDaddy, Starfield, IdenTrust, QuoVadis, Certum, Buypass, Actalis, T-TeleSec, Microsoft RSA/ECC Root CA 2017, D-TRUST, Telia, SwissSign, OISTE WISeKey, FNMT-RCM and SECOM roots.
- **ISRG Root X1 Root CA & TLS Chain Verification:** The Let's Encrypt ISRG Root X1 root certificate (`isrgrootx1.der`) is embedded in the kernel. The certificate chain received from the server is scanned with RFC 5280 rules and the RSA PKCS#1 v1.5 and ECDSA P-256 signatures of leaf and intermediate certificates are fully verified up to the trusted ISRG Root X1 public key.
- **Devices Behind a USB Hub (xHCI):** A multi-tier USB hub architecture is natively supported. Hub descriptors (`GET_DESCRIPTOR 0x29`), port power control (`PORT_POWER`), port reset (`PORT_RESET`), the xHCI 20-bit hierarchical `Route String` and the Transaction Translator (TT / Split Transaction) mechanism were integrated. Keyboards and mice behind a hub are automatically enumerated and driven.
- **EHCI (USB 2.0), UHCI (USB 1.1) and OHCI Controller Support:** EHCI EECP OS handoff, CONFIGFLAG port routing, UHCI Global/Host Reset and OHCI operational mode transitions were implemented so that USB ports and BIOS legacy emulation work reliably on older motherboards and laptops that have no xHCI controller.
- **Smart Error and Warning Coloring in the Boot Console:** Undetected hardware and error or warning lines (`[BOOT]`, `[INPUT]`, `[DRIVER]`) in the UEFI/BIOS text console are automatically highlighted in red (`#FF6B6B`) on the boot screen.
- **Notepad – Find & Replace and Overflow Menu:** `Ctrl+F` opens the **Find** bar and `Ctrl+H` opens the **Replace** bar. Text input is routed to the active field and `Esc` closes the bar. Advanced search options and an overflow menu for small screens were integrated.
- **Realtek PCIe Network Card Support (RTL8168/8169/8125/8101):** Native driver support was integrated for the most widely used Realtek Gigabit and Fast Ethernet controllers on physical motherboards and PCIe expansion slots. Multi PCI bus scanning (Bus 0..8), C+ mode initialization, 64-bit RX/TX descriptor rings and live hardware cable/link status (PHYSTATUS) tracking were added.
- **Realtek RTL8139 Network Card Support (PCI 0x10EC:0x8139):** The classic PCI Fast Ethernet controller is now driven natively: C+ mode initialisation, I/O port register access (BAR0 is an I/O BAR), four TX channels and an 8 KiB RX ring (16-byte WRAP padding, CAPR/CBR empty-ring detection) plus MII (BMSR) link polling with an MSR fallback (`e1000.rs`, `NicType::RealtekRtl8139`). The MAC address is read from IDR0, with a 93C46 EEPROM fallback when it is blank or invalid. Testable in QEMU with `-device rtl8139,netdev=n0`.
- **NE2000 Driver (ISA PnP + PCI RTL8029AS):** Novell NE2000 / RTL8019AS compatible ISA cards are located through ISA PnP isolation (0x279/0xA79/0x203, 72-bit initiation key, MAC and resource settings read from the 72-byte response structure); when PnP does not answer, the common jumper bases 0x300, 0x280, 0x320 … 0x2E0 are probed. The driver performs DP8390 page handling (CR/PS0-PS1), RTL8019AS reset, an 8 KiB RX ring (PSTART/PSTOP = 0x46/0x80), a TX buffer at 0x40, remote DMA through the 0x10 data port with automatic 8/16-bit selection (ISR/RSR), 60-byte padding, overrun (OVW) recovery and per-frame BNRY updates (`ne2000.rs`). PCI RTL8029AS cards (device 0x8029) reuse the same core through `init_pci`. The driver is started automatically at boot when no PCI NIC is found; bus selection goes through `net::BusKind` (`Pci` / `IsaPnp`), so the DHCP, ARP, DNS, TCP/UDP and TLS stack works unchanged on every card. Settings, Control Panel, the system status window and the boot log report the active adapter in TR/EN.
- **Broadcom NetXtreme Network Card Support (BCM57xx, PCI 0x14E4):** A native driver was added for Broadcom, the fourth major NIC vendor. The BCM5701–5782 family is recognized; Register Access Enable, MAC hash table, RX/TX descriptor rings and link status (PHYSTATUS) support are provided.
- **Smooth Mouse & Ballistic Acceleration on High-Resolution Displays (1080p+):** Clipping of the 12/16-bit movement data in USB mouse reports to a narrow range was prevented, providing full axis sensitivity. Resolution-scaled dynamic base speed (~2.8x) for 1080p and 2K/4K screens and a 4-stage ballistic acceleration that does not disturb precise pixel clicks were applied.
- **Renewed Medium-Sized & High-Contrast Mouse Cursor:** A modern 15-pixel sharp arrow cursor with an ideal size for 1080p and higher resolutions was designed. The full 8-direction box outline provides high contrast and clear visibility on all light and dark backgrounds; the VRAM backup buffer was increased to 32x32 pixels to prevent cursor trails.
- **X.509 Certificate Verification (Time & SNI/CN):** During the TLS handshake, the validity period of the server certificate (`not_before` / `not_after`) is verified against the CMOS RTC clock and the target host name is verified against CN (`CommonName`) and SAN (`Subject Alternative Name`) DNS records using RFC 6125 rules (including single-level wildcard `*.domain.com`).
- **Certificate Chain Depth Limit:** `verify_certificate_chain` in `src/drivers/crypto/x509.rs` verifies chains with at most `MAX_CHAIN_DEPTH` (6) certificates to bound the DER parsing cost and memory of extremely long enterprise/private CA chains in a kernel without an intermediate certificate cache; longer chains are rejected with `ChainTooDeep` before parsing.
- **Certificate Revocation Checking (CRL & OCSP):** `src/drivers/crypto/revocation_core.rs` parses DER-encoded CRL (`CertificateList`) and OCSP (`BasicOCSPResponse`) responses; revocation status, serial number, revocation time/reason and response freshness are checked. `revocation.rs` applies a fail-closed policy: the connection is rejected when the revocation status is unknown. OCSP response signatures are verified via `OcspResponseInfo::verify_signature` against the responder/CA certificate (RFC 6960 §4.2.1); responses with invalid signatures are treated as `Unknown` and rejected. Automatic CRL/OCSP downloading (`revocation_fetch.rs`) performs signature verification before applying results.
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
- **🐾 Application Close Fix (Watchdog):** The watchdog timer that automatically closed applications after 5 seconds of inactivity has been fixed. Newly opened windows are added to the watch list only after their first render; open windows are marked as active every frame. As a result, applications no longer close even if the user does not interact with them.
- **🎮 FPS Performance Test Game:** A new `FPS Game` application (app ID 15) was added. It features a raycasting-based 3D engine, mouse look, WASD movement, distance-shaded walls, enemy targets, radar, and PC Speaker/HDA sound effects. Launch from Start Menu → Media → FPS Game or Run Dialog → `fps` / `oyun`.
- **🛡️ FPS Game Freeze Fix:** The game no longer freezes when health reaches zero; the player respawns and continues. Freezes caused by negative screen coordinates and direction vector floating-point drift were fixed.
- **⚙️ Control Panel Cleanup:** Sound test button removed; network test button now appears only on the Network tab; overlapping button layout fixed.
- **🎨 Theme Change Only in Settings:** Theme change button removed from taskbar, desktop right-click menu, and control panel; it remains available only in Settings > Display.

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
| | Realtek PCIe (RTL8168/8169/8125) | `e1000.rs` | 🟢 Full | C+ mode, 64-bit RX/TX rings, PHYSTATUS link tracking |
| | Realtek RTL8139 (PCI 10/100) | `e1000.rs` | 🟢 Full | I/O port access, C+ mode, 4 TX channels, 8 KiB RX ring, MII link, 93C46 MAC fallback |
| | NE2000 / RTL8019AS (ISA, PnP) | `ne2000.rs` | 🟢 Full | ISA PnP + jumper probing, remote DMA, 8K RX ring, TR/EN reporting |
| | NE2000 compatible PCI (RTL8029AS) | `ne2000.rs` | 🟢 Full | PCI I/O BAR, remote DMA, automatic 8/16-bit selection |
| | Broadcom NetXtreme (BCM57xx) | `e1000.rs` | 🟢 Full | MAC hash, RX/TX, PHY link check |
| | VirtIO-net (QEMU/KVM/Cloud) | `virtio_net.rs` | 🟢 Full | High-speed virtualized network adapter |
| | Atheros Ethernet | `atheros.rs` | 🟡 Partial | PCI detection + 2D mode-set skeleton, basic RX/TX; checksum offload and filters TODO |
| | Marvell Ethernet | `marvell.rs` | 🟡 Partial | PCI detection + 2D mode-set skeleton, basic RX/TX; checksum offload and filters TODO |
| | Bluetooth HCI | `bluetooth.rs` | 🟡 Partial | Command (0x01), ACL (0x02), Event (0x04), Inquiry, connection management (A2DP/GATT TODO) |
| | Wi-Fi (802.11 Wireless) | `wifi.rs` | 🟡 Partial | 802.11 frame structures, WPA2-PSK PBKDF2-HMAC-SHA1 PMK/PTK derivation, WPA 4-way handshake, PCI/USB interface detection; real PHY/RX-TX not yet |
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
| **Power & Sensors**
- ✅ **Sensors & Power:** ACPI AML interpreter (Method, Device, Scope, Math/Logic), `_BST` / `_BIX` / `_BIF` battery status reading, `_S0`..`_S5` sleep states and PowerResource management, ThermalZone (`_TZ_`, `_TMP`, `_CRT`) and fan sensors.
- ✅ **Sensor & Fan Management:** Hardware-specific EC (Embedded Controller) fan profiles for `fan_temp.rs` completed (Silent/Normal/Performance/Full/Auto, temperature-based auto-switching, 0x66/0x62 EC protocol).
- 📋 **Settings App Updates:**
  - **Hardware Monitoring in Tests Tab:** A new "Hardware Monitoring" block has been added to the Settings > Tests tab, showing CPU/Chipset/GPU temperatures and fan speeds (RPM).
  - **Wi-Fi Scan in Network Tab:** A "Scan Networks" button has been added to the Settings > Network tab; scanning detects nearby Wi-Fi networks (SSID, signal strength, channel, security).
  - **Scroll Support:** The Settings app now supports scrolling through content across its 5 tabs (Basic, Display, Network, Devices, Tests) using `ScrollState`; long content can be scrolled.
  - **Hardware Monitoring in Basic Tab:** Hardware monitoring information is shown in the Settings > Basic tab; fan speeds and temperatures are refreshed at the bottom of the interface.
- ✅ **Sensors & Power:** ACPI AML interpreter (Method, Device, Scope, Math/Logic), `_BST` / `_BIX` / `_BIF` battery status reading, `_S0`..`_S5` sleep states and PowerResource management, ThermalZone (`_TZ_`, `_TMP`, `_CRT`) and fan sensors.
- ✅ **Sensor & Fan Management:** Hardware-specific EC (Embedded Controller) fan profiles for `fan_temp.rs` completed (Silent/Normal/Performance/Full/Auto, temperature-based auto-switching, 0x66/0x62 EC protocol).

- 📋 **Settings App Updates:**
  - **Hardware Monitoring in Tests Tab:** A new "Hardware Monitoring" block has been added to the Settings > Tests tab, showing CPU/Chipset/GPU temperatures and fan speeds (RPM).
  - **Wi-Fi Scan in Network Tab:** A "Scan Networks" button has been added to the Settings > Network tab; scanning detects nearby Wi-Fi networks (SSID, signal strength, channel, security).
  - **Scroll Support:** The Settings app now supports scrolling through content across its 5 tabs (Basic, Display, Network, Devices, Tests) using `ScrollState`; long content can be scrolled.
  - **Hardware Monitoring in Basic Tab:** Hardware monitoring information is shown in the Settings > Basic tab; fan speeds and temperatures are refreshed at the bottom of the interface.
