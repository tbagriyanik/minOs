# Release History and Roadmap

minOs release notes and future plans.

---

## v2.2.0 (Current)

### What's New

- **🔐 TLS 1.3 HTTPS Handshake Completed:** Browser's `https://` connections now complete the TLS 1.3 handshake end-to-end. Three RFC 8446 fixes: AAD length is exactly 5 bytes (§5.2), `finished_key` derived directly from `server/client_hs_traffic_secret` (§4.4.4), graceful `close_notify` (§6.1).
- **🌐 Browser Improvements (Caching & Visualization):** VFS-based cache (`/browser/cache/`); CSS `transform`, `transition`/`animation`, `flexbox`/`grid`, shadows, border-radius, opacity; `imgdecode` improvements and SVG support.
- **🌍 Progressive JPEG & @font-face Support:** Multi-scan progressive JPEG (SOF2) decoding; CSS `@font-face` and `@keyframes` (`color`/`background-color` interpolation).
- **⏱️ IPv6-First HTTP (Dual-Stack):** AAAA resolved first, IPv6 connection attempted; falls back to IPv4 on failure. 3xx redirects and `chunked` decoding applied to IPv6 responses.
- **🧩 HTTP/2 Client Core:** RFC 7540 frame layer (`h2`), HPACK encoder, `Http2Connection`, `http2_get()`. Infrastructure ready; browser still uses HTTP/1.1.
- **🐾 Browser Demo Pages:** `minos://demo`, `minos://demo-img`, `minos://demo-video`.
- **🚫 Browser Freeze Fix:** External resources collected into `pending_resources` and fetched one per frame asynchronously by `poll_network()`; main event loop never blocked.
- **🛠️ Browser Developer Tools:** `Ctrl+Shift+D` DevTools panel (Elements, Console, Network, Storage).
- **Expanded Hardware Drivers (20+):** Networking (VirtIO-net, Intel I219-V, Atheros, Marvell, Realtek RTL8139, NE2000), Bluetooth HCI, Storage (AHCI/SATA, NVMe, VirtIO-blk, SD/MMC), USB (Mass Storage, Printer, Audio, Touchscreen), Graphics (VirtIO-gpu, AMD Radeon, NVIDIA GeForce), Sensors & Power (ACPI AML, battery, sleep, ThermalZone, fan), Bus & Timing (PCIe, PCI, PIT/HPET/APIC/TSC).
- **Live USB Hot-plug:** USB devices detected instantly with toast notifications.
- **OHCI & UHCI & EHCI Transfer Layer:** Complete transfer layer and port handoff infrastructure.
- **minJs Advanced JavaScript Runtime:** Binary arithmetic, loops, variable mutations, array methods, object properties, JSON, math/string helpers, global conversions.
- **minJs Closures & Timers:** Closure support (function expressions, arrow functions, scope capture, private state per closure), `setTimeout`/`setInterval` timers with `clearTimeout`/`clearInterval`, `Object.keys()`/`Object.values()`/`Object.entries()`, array callback methods (`forEach`, `map`, `filter`, `find`, `findIndex`, `some`, `every`, `reduce`), and IIFE support.
- **IPv6 Protocol & Socket Infrastructure:** `Ipv6Address`, `IpAddress`, RFC 4291, RFC 2464, RFC 8200, ICMPv6 (`ping6`), Neighbor Discovery, `ETHERTYPE_IPV6`; terminal `ipconfig` IPv6 and `ping`/`ping6` support.
- **exFAT and ext4 Fragmentation & Deep Directory Integrity:** Verified with host stress tests.
- **Expanded Trust Store (Root CA):** 51 trusted root CA certificates embedded into the kernel.
- **ISRG Root X1 Root CA & TLS Chain Verification:** ISRG Root X1 embedded; chain verified per RFC 5280.
- **Devices Behind a USB Hub (xHCI):** Multi-tier USB hub architecture natively supported.
- **EHCI, UHCI and OHCI Controller Support:** USB ports and BIOS legacy emulation on older hardware.
- **Smart Error and Warning Coloring in Boot Console:** `[BOOT]`, `[INPUT]`, `[DRIVER]` lines highlighted in red (`#FF6B6B`).
- **Notepad – Find & Replace and Overflow Menu:** `Ctrl+F` Find, `Ctrl+H` Replace, `Esc` to close.
- **Realtek PCIe Network Card Support (RTL8168/8169/8125/8101):** Native driver, multi PCI bus scan, 64-bit RX/TX rings, PHYSTATUS link tracking.
- **Broadcom NetXtreme Network Card Support (BCM57xx, PCI 0x14E4):** Native driver added.
- **Smooth Mouse & Ballistic Acceleration on High-Resolution Displays (1080p+):** 12/16-bit movement clipping prevented; resolution-scaled dynamic base speed and 4-stage ballistic acceleration.
- **Renewed Medium-Sized & High-Contrast Mouse Cursor:** 15-pixel sharp arrow, 8-direction box outline, VRAM backup buffer 32x32 pixels.
- **X.509 Certificate Verification (Time & SNI/CN):** Validity period against CMOS RTC, host name against CN/SAN DNS records per RFC 6125.
- **Certificate Chain Depth Limit:** `MAX_CHAIN_DEPTH` (6); longer chains rejected with `ChainTooDeep`.
- **Certificate Revocation Checking (CRL & OCSP parsing):** DER CRL/OCSP parsing, fail-closed policy.
- **VMware VMXNET3 & Expanded Network Support:** VMware PCI vendor `0x15AD` (`0x07B0` VMXNET3, `0x0720` VMXNET) recognized.
- **minPak / minpkg SHA-256 Integrity Verification:** `minPak install <package> [hash]`, `minPak sha256 <file>`, `minPak verify <file> <hash>`.
- **minpkg Dependency Resolution:** Version constraints, transitive ordering, conflict and cycle detection.
- **USB Keyboard and Mouse Improvements (xHCI):** Initialization bugs fixed; HID usage→PS/2 mapping, report descriptor parsing, multi-interface device support.
- **USB HID & PS/2 Fallback Diagnostics in Boot Console:** `[INPUT] USB HID not found, using PS/2 fallback`.
- **Automatic Resolution and Persistence at Boot:** Limine `res=WxH` and `settings.cfg` preferences applied automatically.
- **Dynamic Resolution and Desktop Adaptation:** Runtime resolution changes instantly adapted (`force_render`).
- **QEMU SDL / GTK Support & 64 MB VRAM:** `-Display sdl` parameter, video memory raised to 64 MB.
- **Static BSS Memory Architecture (CodeQL Alert #25 Fix):** `MemoryManager` moved to static BSS storage.
- **Expanded Crypto & TLS 1.3:** SHA-384, SHA-512, ECDSA (secp256r1) signature verification/generation (190 host tests).
- **Early Exception Catching & Hardware Triple Fault Protection:** BSP GDT+TSS (`IST1`) and IDT exception gates (`#DE`, `#UD`, `#DF`, `#SS`, `#GP`, `#PF`).
- **Interactive Text Recovery Console (`recovery#`):** Error details (vector, RIP, CR2, error code) and `restart`/`halt`/`help` commands.
- **Expanded High Resolution and 16:10 Support:** Kernel backbuffer enlarged to `2560x1600`; 1920x1080, 1920x1200, 2560x1440, 2560x1600 supported.
- **Dynamic VRAM MMIO Mapping:** 16 MB VRAM range mapped as 2 MB huge pages.
- **Intel HDA & Modern Laptop Compatibility:** Timeouts and PCI multi-function checks; PC Speaker PWM fallback.
- **Settings Window Click Isolation:** Text zoom buttons no longer trigger resolution buttons.
- **Default Hardware Driver Profile:** Full driver profile in normal Live and Installed boots; Safe Mode option preserved.
- **More Readable Boot Log:** Text console flushes every completed line, 45 ms delay once time base is ready.
- **Browser Favorites (Bookmarks Bar):** Interactive bookmarks bar and `minos://bookmarks` page.

---

## v2.1.0 (Previous)

### What's New

- **🛠️ Browser DevTools Panel:** `Ctrl+Shift+D` opens developer tools with Elements, Console, Network and Storage tabs for page-element counts, JavaScript error logs, request/download state, cookies and form-autofill entries.
- **🔐 TLS 1.3 HTTPS Handshake Completed:** Browser's `https://` connections can now complete the TLS 1.3 handshake end-to-end and process the encrypted HTTP response. Three critical fixes per RFC 8446:
  - **AAD length (§5.2):** Additional Authenticated Data computed as exactly 5 bytes.
  - **Finished key derivation (§4.4.4):** `finished_key` derived directly from `server/client_hs_traffic_secret`.
  - **Graceful `close_notify` (§6.1):** Server's `close_notify` no longer treated as fatal error.
- **🌐 Browser Page Download and HTML Integration:** Downloaded pages saved under `/root/Downloads/` with `.html` extension. Double-clicking `.html`, `.htm`, `.xhtml` and `.url` files in File Manager opens them in the browser via `load_html`.
- **🌍 Progressive JPEG, `@font-face` & `@keyframes` Color Animation:** The image decoder handles multi-scan **progressive JPEG** (SOF2) by accumulating coefficients; the CSS engine parses `@font-face` rules and carries `font-family` on `ElementStyle`; `@keyframes` now animates `color`/`background-color` in addition to `opacity`. Limit: remote TTF/WOFF data is not downloaded or rasterized.
- **⏱️ IPv6-First HTTP & HTTP/2 Client Infrastructure:** `http_get_impl` resolves AAAA first and connects over IPv6 (TLS handshake over IPv6 for HTTPS), falling back to IPv4 on failure. `http.rs` gained the RFC 7540 frame layer, an HPACK request encoder and `Http2Connection` (`http2_get`). Limit: HTTPS frame encryption, ALPN `h2` negotiation and HPACK decoding are missing, so the browser still uses HTTP/1.1.
- **🐾 Browser Demo Pages:** Three local demo pages added: `minos://demo`, `minos://demo-img`, `minos://demo-video`.
- **Expanded Hardware Drivers (20+ New Drivers):** Networking, Bluetooth HCI, Storage, USB, Graphics, Sensors & Power, Bus & Timing.
- **Live USB Hot-plug:** USB devices detected instantly while running with toast notifications.
- **OHCI & UHCI & EHCI Transfer Layer:** Complete transfer layer and port handoff infrastructure integrated.
- **minJs Advanced JavaScript Runtime:** Binary arithmetic, loops, variable mutations, array methods, object properties, JSON, math/string helpers, global conversions.
- **minJs Closures & Timers:** Closure support (function expressions, arrow functions, scope capture, private state per closure), `setTimeout`/`setInterval` timers with `clearTimeout`/`clearInterval`, `Object.keys()`/`Object.values()`/`Object.entries()`, array callback methods (`forEach`, `map`, `filter`, `find`, `findIndex`, `some`, `every`, `reduce`), and IIFE support.
- **IPv6 Protocol & Socket Infrastructure:** `Ipv6Address`, `IpAddress`, RFC 4291, RFC 2464, RFC 8200, ICMPv6, Neighbor Discovery, `ETHERTYPE_IPV6`.
- **exFAT and ext4 Fragmentation & Deep Directory Integrity:** Verified with host stress tests.
- **Expanded Trust Store (Root CA):** 51 trusted root CA certificates embedded into the kernel.
- **ISRG Root X1 Root CA & TLS Chain Verification:** Let's Encrypt ISRG Root X1 root certificate embedded.
- **Devices Behind a USB Hub (xHCI):** Multi-tier USB hub architecture natively supported.
- **EHCI, UHCI and OHCI Controller Support:** USB ports and BIOS legacy emulation work reliably on older hardware.
- **Smart Error and Warning Coloring in Boot Console:** `[BOOT]`, `[INPUT]`, `[DRIVER]` lines highlighted in red.
- **Notepad – Find & Replace and Overflow Menu:** `Ctrl+F` Find, `Ctrl+H` Replace bar.
- **Realtek PCIe Network Card Support (RTL8168/8169/8125/8101):** Native driver support integrated.
- **Broadcom NetXtreme Network Card Support (BCM57xx, PCI 0x14E4):** Native driver added.
- **Smooth Mouse & Ballistic Acceleration on High-Resolution Displays (1080p+):** 12/16-bit movement data clipping prevented.
- **Renewed Medium-Sized & High-Contrast Mouse Cursor:** 15-pixel sharp arrow cursor, 8-direction box outline.
- **X.509 Certificate Verification (Time & SNI/CN):** Validity period and host name verified.
- **Certificate Chain Depth Limit:** `MAX_CHAIN_DEPTH` (6) certificate limit.
- **Certificate Revocation Checking (CRL & OCSP parsing):** Fail-closed policy applied.
- **VMware VMXNET3 & Expanded Network Support:** VMware PCI vendor `0x15AD` recognized.
- **minPak / minpkg SHA-256 Integrity Verification:** Package manager SHA-256 digest check added.
- **minpkg Dependency Resolution:** Version constraints, transitive ordering, conflict and cycle detection.
- **USB Keyboard and Mouse Improvements (xHCI):** Core bugs in xHCI initialization fixed.
- **USB HID & PS/2 Fallback Diagnostics in Boot Console:** Clear diagnostic log printed.
- **Automatic Resolution and Persistence at Boot:** Limine menu resolution and `settings.cfg` preferences applied automatically.
- **Dynamic Resolution and Desktop Adaptation:** Runtime resolution changes instantly adapted.
- **QEMU SDL / GTK Support & 64 MB VRAM:** `-Display sdl` parameter added, video memory raised to 64 MB.
- **Static BSS Memory Architecture (CodeQL Alert #25 Fix):** `MemoryManager` moved to static BSS storage.
- **Expanded Crypto & TLS 1.3:** SHA-384, SHA-512 and ECDSA (secp256r1) signature verification/generation added.
- **Early Exception Catching & Hardware Triple Fault Protection:** BSP GDT+TSS (`IST1`) and IDT exception gates loaded.
- **Interactive Text Recovery Console (`recovery#`):** Error details shown and interactive prompt offered.
- **Expanded High Resolution and 16:10 Support:** Kernel backbuffer enlarged to `2560x1600`.
- **Dynamic VRAM MMIO Mapping:** 16 MB VRAM range mapped as 2 MB huge pages.
- **Intel HDA & Modern Laptop Compatibility:** Timeouts and PCI multi-function checks added.
- **Settings Window Click Isolation:** Text zoom buttons no longer trigger resolution buttons.
- **Default Hardware Driver Profile:** Full driver profile used in normal Live and Installed boots.
- **More Readable Boot Log:** Text console flushes every completed line with 45 ms delay.
- **Browser Favorites (Bookmarks Bar):** Interactive bookmarks bar integrated.

---

## Roadmap

- **Bluetooth Upper-Layer Profiles:** A2DP audio streaming and BLE GATT profiles planned.
- **NVIDIA / AMD 3D Acceleration:** Vulkan/Gallium is a separate stage.
- **Sensor & Fan Management:** Hardware-specific EC fan profiles.
- **Atheros / Marvell Ethernet:** Hardware checksum offload and advanced packet filters planned.
- **USB Audio Recording:** PCM recording via microphone is TODO.
- **SD/MMC Card Writing:** CSD register parsing and block write reliability WIP.
- **CRL/OCSP Revocation Checking:** Automatic HTTP/OCSP download requests planned.
- **IPv6 DNS (AAAA) Resolution:** No AAAA query sent and AAAA answers not parsed.
- **minJs JavaScript Limitations:** Deep lexical scoping rules, full prototype chain mechanism, getters/setters, `Proxy`, module system (`import`/`export`), `yield`/generators and complete Promise/microtask scheduling not yet implemented.
