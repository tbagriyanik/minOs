# 15 Built-in Applications

minOs includes **15 built-in applications** and a window manager, providing a full desktop experience.

| Application | Description |
| :--- | :--- |
| **🔘 Start Menu** | Category menus (`System`, `Productivity`, `Media`, `Power`) with iconified graphical listing. Keyboard arrow keys, Enter, mouse wheel navigation and search. |
| **📁 File Manager** | Virtual file system (VFS/TarFS) explorer; file type icons (`[LOG]`, `[TXT]`, `[IMG]`, `[SND]`, `[VID]`), sizes and location bar. USB drive **FAT32** and CD **ISO9660** contents listed via `-UsbDisks`. Desktop (`/desktop`) is independent from the file manager. |
| **⚙️ System Settings** | Five tabs (Basic / Display / Network / Devices / Tests): v2.1.0 version info and kernel status, five themes, volume, night light, language (TR/EN), keyboard layout, text zoom, target resolution and region selection; selectable wallpaper/background with instant preview; window layout saving and **persistent settings via `settings.cfg`**. Default screen mode selected from Limine boot menu. |
| **🎛️ Control Panel** | Live system info: OS/kernel version, CPU vendor and logical core count, RAM usage and fill bar, resolution, theme, keyboard layout, uptime and date. Network tab with e1000 driver, link status, DHCP/IP/mask/gateway/DNS and audio driver; one-click theme change. |
| **📦 Package Manager (`minPak`)** | Graphical and command-line package management interface. Repository catalog listing, package search, local/network package installation and installed package management. |
| **📝 Notepad** | Top menu bar (`File`, `Edit`, `Help`), text clear/save, **Cut / Copy / Paste / Select All**, **Undo / Redo** (Ctrl+Z / Ctrl+Y). Turkish character support, `Backspace`, `Enter`, **Home / End**, **Shift+arrow keys selection**, line numbers and cursor. |
| **🌐 Web Browser** | Favorites (Bookmarks) bar, lightweight HTTP test site, `minos://bookmarks`, `minos://help`, `minos://cats`, `minos://net`, `minos://history` and `minos://demo` / `minos://demo-img` / `minos://demo-video` local demo pages, tabbed navigation and e1000 ARP/ping/DNS test panel. `https://` addresses (port 443) establish secure connections via **TLS 1.3** handshake; server response decrypted with AES-128-GCM and processed as HTTP. Downloaded pages saved under `/root/Downloads/` with `.html` extension; double-clicking `.html`/`.htm`/`.xhtml` files in File Manager opens them in the browser. |

---

## 📜 minJs JavaScript Engine

minOs includes a minimal client-side JavaScript engine (`src/apps/js.rs`, `no_std`) used by the browser `<script>` blocks. It is **not** a full ECMAScript implementation — only a subset commonly needed for simple page scripts.

### Supported Features

- Variables: `var`/`let`/`const` assignment and reading
- Arithmetic: `+`, `-`, `*`, `/`, `%`, parentheses, unary `+`/`-`/`!`
- Comparisons: `!`, `&&`, `||`, `==`/`!=`/`===`/`!==`, `<=`, `>=`, `<`, `>`
- Functions: `function name(a, b) { ... }`, `return`, calls
- Loops: `for (init; cond; step)`, `for (item of array)`, `for (key in object)`, `while`, `break`, `continue`
- Arrays: literals, indexing, `.push()`, `.pop()`, `.shift()`, `.unshift()`, `.reverse()`, `.join()`, `.indexOf()`, `.includes()`, `.slice()`, `.concat()`
- Objects: literals, property access, `delete`, `Object.keys()`, `Object.values()`
- JSON: `JSON.stringify()`, `JSON.parse()`
- Globals: `parseInt()`, `parseFloat()`, `String()`, `Number()`, `Boolean()`, `Math.*`, `String.prototype.*`
- DOM: `getElementById`, `getElementsByClassName`, `innerHTML`, `textContent`, `createElement`, `appendChild`, `setAttribute`, `getAttribute`, `addEventListener`, `dispatchEvent`
- Promises: `new Promise(executor)`, `.then()`, `.catch()`, `await`, `Promise.resolve()`, `Promise.reject()`, `Promise.all()`, `Promise.race()`, `Promise.allSettled()`
- Strict mode: `"use strict"` directive detected; undeclared variable assignment is rejected

### Known Limitations (vs ECMA-262)

| Feature | Status | Notes |
| :--- | :---: | :--- |
| `class` syntax | ✅ Supported | `class Name [extends Parent] { constructor() { } method() { } }`, `new Name(args)`, instance method calls |
| Strict mode | ✅ Supported | `"use strict"` detected; undeclared variable assignment rejected |
| `Promise.all/race/allSettled` | ✅ Supported | Static methods on Promise |
| `with` statement | ❌ Not supported | Parsing not implemented |
| `eval()` scope isolation | ❌ Not supported | `eval` shares current scope |
| `let`/`const` block scoping | ⚠️ Partial | Treated like `var` (function/global scope) |
| Template literals | ⚠️ Partial | Backtick strings supported, `${}` interpolation not |
| Optional chaining (`?.`) | ❌ Not supported | |
| Nullish coalescing (`??`) | ❌ Not supported | |
| Arrow functions | ❌ Not supported | |
| Generators / async-await | ⚠️ Partial | `await` supported on Promise values; `async function` not |

> **Note:** minJs is intentionally small — its goal is running simple page-enhancing scripts, not full web application compatibility.

| **🖼️ Image Viewer** | Opens 24-bit `.bmp` drawings saved from Paint and images in the virtual file system; zoom in/out and detailed image metadata panel; shows a cute cat mascot when no image is loaded. |
| **🎵 Music Player** | Audio via HDA/PC speaker when user starts playback, animated spectrum equalizer and playlist. No automatic sound at system boot. |
| **🎬 Video Player** | Live synthwave horizon scene, neon sun scanlines, downward-flowing 3D perspective grid road and cyber vehicle simulation. |
| **📊 Task Manager** | Kernel memory usage, CPU fluctuation graph, multi-core (SMP) per-core CPU bars, process table, process termination (`Kill`), alphabetical "Launch Program" start menu and RAM fill bar. Status bar pinned below window for comfortable performance monitoring. |
| **🧮 Calculator** | Keyboard-input expression calculator (`+`, `-`, `*`, `/`); wide number range with 128-bit fixed-point arithmetic and up to six decimal places precision. `Backspace` and `Esc` for editing/clearing. |
| **🎨 Paint** | Pixel-based free drawing canvas, color palette and circular eraser tool. **Continuous/smooth drawing** while mouse button held, automatic brush edge clipping, save/open directly as `/root/Documents/cizim.bmp` and 4096-point canvas history. |
| **⌨️ Command Prompt (MinShell)** | UNIX/DOS-like interactive shell: file/folder management, memory status, network diagnostics, `curl` HTTP download, `format` disk formatting (FAT32/ext4/exFAT/ext2), `pkg`, `exec`, Tab completion and command history. |
| **💽 One-Click Install (Installation Wizard)** | Step-by-step installation window: **1)** target disk selection (ATA hard disk / QCOW2), **2)** `Format (FAT32)` button to format disk, **3)** `One-Click Install` for permanent installation. Installation writes `MINOS.SYS` marker file, `README.TXT`, current `settings.cfg` settings and the entire VFS tree (`VFS.DAT` snapshot) to disk. |
| **🧊 3D Viewer** | Cube, cylinder, sphere and pyramid meshes displayed in solid shading or wireframe mode; drag to rotate, zoom, auto-rotate and load shape/text from `.3d` files. Back-face culling and far-to-near painter's algorithm for correct solid object appearance. |

> **Note:** The **Control Panel** (`controlpanel.rs`) exists in `src/apps/`; settings and customization functions are executed via **System Settings** (`settings.rs`).
