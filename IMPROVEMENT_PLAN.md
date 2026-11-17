# MinOS Code Improvement Plan

## Overview
This document outlines a comprehensive plan for improving the MinOS codebase across all layers: kernel, drivers, GUI, and applications.

## Priority Levels
- **P0 (Critical)**: Security, stability, data integrity
- **P1 (High)**: Performance, maintainability, major UX issues
- **P2 (Medium)**: Code quality, documentation, testing
- **P3 (Low)**: Nice-to-have features, polish

---

## 1. Code Organization & Modularity (P1)

### 1.1 Main.rs Refactoring
**Problem**: `main.rs` is 1700+ lines with boot logic, window setup, and main loop all mixed together.
**Solution**: 
- Extract boot phases into `src/boot/` modules
- Move window/app initialization to `src/gui/setup.rs`
- Move main loop to `src/gui/event_loop.rs`
- Create `src/kernel/` for core kernel utilities

### 1.2 Application Architecture
**Problem**: Apps directly imported and instantiated in main.rs; no plugin system.
**Solution**:
- Create `src/apps/registry.rs` for dynamic app registration
- Define `Application` trait for consistent app interface
- Support hot-loading of apps from `/bin`

### 1.3 Driver Framework
**Problem**: Drivers directly called from main; no unified interface.
**Solution**:
- Define `Driver` trait with init/probe/shutdown methods
- Create driver registry for auto-discovery
- Implement proper hotplug support

---

## 2. Memory Management (P0/P1)

### 2.1 Heap Allocator Improvements
**Problem**: Fixed 16MB heap with linked_list_allocator; no OOM recovery.
**Status**: **Done** — heap grows on demand inside a 64 MiB virtual window
(`HEAP_WINDOW_BASE`), pages map lazily and large allocations (≥64 KiB)
populate on first touch. Tiered allocation landed (`tiered_alloc` /
`test_tiered_alloc`), pressure detection triggers compression
(`check_pressure` → `mm::memory::compress::pressure_report`,
`MemoryProfile::under_pressure`), and per-allocation tracking exists
(`track_insert`/`track_remove`, `[MEM-PROFILE]` markers).
**Solution**:
- Implement tiered allocator (small/large object separation)
- Add memory pressure notifications
- Implement allocation tracking for debugging
- ~~Add configurable heap size via boot parameter~~ (dynamic growth landed)

### 2.2 Page Frame Allocator
**Problem**: Basic bitmap allocator; no zone support (DMA, Normal, HighMem).
**Status**: **Done (core)** — 2 MiB huge-page alloc/free
(`allocate_big_page`/`free_big_page`, `bigpage.rs`), contiguous DMA
allocation, SMP-safe bitmap under an internal `PMM_LOCK` (mutating methods
serialize; `*_locked` variants avoid self-deadlock), three memory zones
(`MemoryZone::{Dma,Normal,HighMem}`, `zone_for_address`, `allocate_frame_in_zone`)
and per-CPU/per-zone page caches (`PerCpuPageCache`, batch 16, double-free
guard, cache refill/flush) landed. `[PMM-ZONE]` boot self-test and
`[MEM-ZONE]` profile output verify accounting.
**Solution**:
- ~~Add memory zones~~
- ~~Implement per-cpu page caches~~

### 2.3 Virtual Memory
**Problem**: Manual page table management; no ASLR for kernel.
**Status**: Partially done — user-space ASLR and copy-on-write landed
(`[USER-MEM]`/`[COW-TEST]` boot checks, `mm::user_mem`, `mm::memory::cow`).
KASLR is active through Limine (`kaslr: yes`, ET_DYN link at 0) and now
verified at boot: `[KASLR] fiziksel=… sanal=… kayma=… hizalama=ok|HATA
durum=etkin|kapali` (`boot::limine::kaslr_slide`). PKU detection,
`CR4.PKE` enablement and `PKRU` read/write helpers landed
(`arch::x86_64::pku`, `[PKU]` marker). Remaining: per-page protection-key
assignment / per-process PKRU policy, full KASLR entropy audit.

---

## 3. GUI Performance & UX (P1)

### 3.1 Rendering Pipeline
**Problem**: Full-frame redraw on every tick; limited dirty-rect support.
**Status**: Partially done — `GlobalDirtyTracker` + `take_dirty_region()`
feed a unified dirty rect into `render_full_frame`, which clips drawing via
`fb.set_frame_clip()`; overflow or untracked regions fall back to full
redraw. Remaining: per-window render lists, frame pacing/vsync, broader GPU
acceleration.

### 3.2 Window Management
**Problem**: O(n²) z-order sorting; no workspace support.
**Status**: Partially done — 4 virtual workspaces landed
(`active_workspace`, `switch_workspace`, `move_active_window_workspace`,
workspace-aware focus/alt-tab), Aero Snap preview exists. `draw_order` now uses
an insertion sort with a local z-value array (near-O(n) for the common already
ordered case), avoiding repeated `windows[...]` lookups. Remaining: true
incremental z-index list/cache, more snap zones.

### 3.3 Input Handling
**Problem**: Direct mouse/keyboard polling in main loop.
**Status**: **Done (core)** — PS/2 IRQ1/IRQ12 routed through IOAPIC into
`INPUT_QUEUE` (`InputEvent` ring buffer); `drain_input_events` feeds
`MouseState`/`KeyboardState` in the event loop, with polling fallback when
IOAPIC is unavailable. USB HID path also feeds the same queue.
Remaining: screen reader, richer accessibility options.

### 3.4 Theme System
**Problem**: Global theme token; no per-app theming.
**Status**: **Done (core)** — `gui::theme` provides 9 runtime-switchable
themes (`ThemeId`: VelvetDark, Daylight, Nord, TerminalGreen, Sunset,
Dracula, Monokai, SolarizedDark, HighContrast) via a token palette;
settings UI swatches, welcome-screen picker, and config persistence all
landed. Remaining: per-app/widget theme inheritance, auto (day/night)
mode.

---

## 4. Error Handling & Reliability (P0)

### 4.1 Result-Based APIs
**Problem**: Panics on errors; no recovery paths.
**Solution**:
- Convert fallible operations to `Result<T, Error>`
- Define error hierarchy (KernelError, DriverError, FsError, etc.)
- Implement error recovery strategies

### 4.2 Watchdog & Recovery
**Problem**: Single watchdog for all apps; no per-process supervision.
**Solution**:
- Per-task watchdog with configurable timeouts
- Automatic restart for critical services
- Crash dump generation

### 4.3 Logging & Diagnostics
**Problem**: Serial output only; no structured logging.
**Solution**:
- Structured logging with levels
- Log persistence to disk
- Remote logging support

---

## 5. Security Hardening (P0)

### 5.1 Input Validation
**Problem**: Direct use of user input in render paths; no bounds checking.
**Solution**:
- Validate all user-controlled inputs
- Add bounds checking to all buffer operations
- Implement capability-based access control

### 5.2 Memory Safety
**Problem**: Unsafe blocks without justification.
**Solution**:
- Audit all `unsafe` blocks
- Add safety comments per Rust guidelines
- Use safe abstractions where possible

### 5.3 Network Security
**Problem**: HTTP/TLS without certificate validation by default.
**Solution**:
- Enforce certificate verification — **Done**: `drivers::http::fetch_once`, the proxy/CONNECT path (`fetch_https`), the IPv6 dual-stack path, and `drivers::http2::Http2Connection::connect` now abort with the new `E_CERT` error when the server certificate is not verified against the embedded root CA store (`drivers/crypto/roots`, e.g. ISRG Root X1, DigiCert, GlobalSign, etc.). Host tests cover `verify_certificate_chain` trust-store behavior and TLS-level rejection of an unverified server certificate (`tests/host_driver_tests.rs`).
- Add HSTS support — **Done**: `http_hsts_note` / `http_hsts_should_use_https` cache `Strict-Transport-Security: max-age` per host; `http://` URLs to HSTS hosts are upgraded to `https://` automatically (RFC 6797).
- Implement secure defaults — HTTPS is the default transport for `https://` URLs; certificate verification failure closes the connection before any application data is sent.

---

## 6. Testing Infrastructure (P2)

### 6.1 Unit Tests
- **Done (host suite)**: `tests/host_tests.rs` + `test_host.ps1` run ~300
  host-side unit tests covering allocator, VFS/fs (ext4/exfat), crypto,
  TLS, JS engine, widgets/themes readability, drivers' pure logic (322 tests).
- Remaining: in-kernel test coverage beyond boot self-tests, fuzzing.

### 6.2 Integration Tests
- Boot-to-desktop test
- File system round-trip
- Network stack tests

### 6.3 Fuzzing
- File format parsers (ELF, PNG, etc.)
- Network protocol handlers
- USB descriptors

---

## 7. Documentation (P2)

### 7.1 Code Documentation
- Rustdoc for all public APIs
- Module-level documentation
- Architecture diagrams

### 7.2 Developer Guides
- Building and testing
- Adding new drivers
- Creating applications
- Porting guide

---

## 8. Build System & CI (P2)

### 8.1 Cargo Workspace
- Separate kernel, drivers, gui, apps crates
- Shared types crate
- Benchmarks

### 8.2 CI Pipeline
- rustfmt/clippy checks — `cargo fmt` + `cargo clippy` run clean locally
- Unit tests on host target — `test_host.ps1` (322 tests passing)
- QEMU integration tests — `test_qemu.ps1`, `boot_check.ps1`
- Binary size tracking

---

## 9. Process & Kernel Roadmap (P1)

Scheduler/process subsystem (`src/task`, `src/mm/process.rs`, `src/arch/x86_64/ring3.rs`).

| # | Item | Priority | Status |
| :- | :--- | :--- | :--- |
| 9.1 | Multi-core scheduler stress tests | P1 | Partial — `task::test_scheduler_multicore_race` exists; `test_stress_24h.ps1` provides configurable-duration QEMU soak runs (panic/exception/QEMU-exit auto-detection, `-Fps3d` render load); needs repeated/soak runs and per-CPU accounting checks |
| 9.2 | Scheduler tests on real hardware | P1 | Todo — validate APIC/SMP paths outside QEMU |
| 9.3 | CPU affinity support | P2 | Partial — `Task::cpu` + `least_loaded_cpu` assign queues; `SYS_SCHED_SETAFFINITY`/`GETAFFINITY` (24/25) store a per-process mask, but the scheduler does not yet consult it (Ring 3 slices still run on the BSP) |
| 9.4 | Deeper security audit of process isolation | P0 | **Done** — `uaccess` range/USER-bit validation, `translate_user` 4-level USER checks, `clone_kernel_mappings` USER-bit stripping, syscall pointer coverage audited; ELF loader hardened (`e_entry` validation, phdr overflow/`e_phentsize` checks, NULL-page rejection), `SYS_SIGACTION` handler restricted to user-executable space |
| 9.4a | KASLR | P0 | **Done** — kernel builds PIE/ET_DYN (linker base 0, `PT_DYNAMIC`), Limine `kaslr: yes` + `randomise_hhdm_base: yes`; slide verified at boot via `[KASLR]` line; naked-asm absolute operands converted to RIP-relative; `gdt.rs` missing `rax` clobber fixed |
| 9.5 | Signal mechanism for user applications | P1 | Partial — `pending_signals` bitmask, `signals`/`SignalAction` tables, `SYS_SIGACTION` (26), `SYS_SIGPROCMASK` (27), mask-aware default actions and `rust_int80` delivery point landed; Ring 3 user-handler invocation (trampoline/return frame) still pending |
| 9.6 | `kill`, `pause`, `resume` system calls | P1 | **Done** — `SYS_KILL` (19): SIGKILL/SIGTERM terminate, SIGSTOP suspends (`ProcessState::Suspended`), SIGCONT resumes, `sig=0` probes, negative `pid` targets a process group; other signals pended |
| 9.7 | Process group and session support | P2 | **Done (core)** — `pgid`/`sid` inheritance via `spawn`/`fork`, `SYS_SETPGID`/`GETPGID`/`SETSID`/`GETSID` (20–23), `kill_group`/`kill_session`, session-leader checks and orphan reparenting to PID 0 |
| 9.8 | Sandboxing of user applications | P1 | **Done (core)** — `SYS_SECCOMP` (28): per-process irreversible syscall allow-mask (`Process::syscall_mask`, AND-only narrowing, `SYS_EXIT` exempt); enforced centrally in `syscall_dispatch_abi`; `[SECCOMP-TEST]` boot test; ABI docs in `docs/SYSCALL_ABI.md` 2.14d. Remaining: resource-limit tightening, optional restricted address space |

Notes:
- `Suspended` is a single process state; a process blocked in `SYS_WAIT` cannot
  currently be stopped (blocked+stopped is not representable yet).
- Signal delivery to Ring 3 user handlers is the remaining part of 9.5:
  registration (`SYS_SIGACTION`), masking (`SYS_SIGPROCMASK`) and the
  post-syscall default-action delivery point are implemented and covered by
  `[PROC-GROUP-TEST]`. `waitpid` status encoding (signal vs exit code) is
  already handled via negative exit codes.

---

## Implementation Order (First 3 Months)

### Month 1: Foundation
1. Refactor main.rs into modules
2. Implement Result-based error handling
3. Add comprehensive Rustdoc
4. Set up workspace structure

### Month 2: Core Systems
1. Memory allocator improvements
2. Driver framework
3. Event-driven input
4. Full dirty-rect rendering

### Month 3: UX & Reliability
1. Workspace/workspace support
2. Theme system overhaul
3. Watchdog per-process
4. Application plugin system

---

## Boot & SMP Notes (2026-11)

- **Boot time profiling**: `src/boot/timing.rs` (`PhaseTimer`, `begin()`,
  `mark()`, `report_total()`) instruments every boot phase; serial output
  shows `[BOOT-TIME] phase=Nms toplam=Mms` and a final
  `SURUM=<ms> asama=<n> en-yavas=<ms>ms butce=3000ms durum=OK|ASIM` line.
  Phases at or above 500 ms are flagged as slow.
- **Idle power**: after APIC timer setup the BSP probes interrupt delivery
  (`apic::enable_bsp_idle()`, 25 ms bounded tick wait) and then uses
  `sti; hlt` while idle (`[IDLE]` marker); platforms where the timer never
  arrives keep the polling fallback. The GUI loop no longer prints a serial
  trace line for every `int 0x80`.
- **Parallel driver init**: independent driver groups (HDA audio, NIC+netstack,
  USB host+Wi-Fi, xHCI HID) run as lanes on APs via
  `smp::parallel_run()`/`ParallelTask` (`unsafe fn(*mut u8)` work queue
  polled by AP idle loops and the BSP). DMA frames are pre-allocated on the
  BSP; the HID lane is the only PMM consumer during the run.
- **New SMP safety locks** (required by parallel init):
  - `drivers::port_io::PCI_CONFIG_LOCK` — serializes the shared 0xCF8/0xCFC
    config mechanism across all driver-local `pci_read*`/`pci_write*` helpers.
  - `mm::pmm::PMM_LOCK` — internal spinlock in `PhysicalMemoryManager`;
    mutating methods lock, internal callers use `*_locked` variants.
  - `serial::SERIAL_LOCK` — protects `LOG_BYTES` and UART output.
- **SMP audit fixes (post-P0 review)**: `init_interrupts(cpu)` loads the
  caller's own GDT/TSS slot (APs previously rebound to `TSSS[0]`/BSP stack);
  `current_cpu_index()` scans all `MAX_CPUS` slots instead of `cpu_count()`
  (wrong-BSP attribution after `wait_for_aps` timeout); APIC ticks moved to
  `TICKS_PER_CPU[MAX_CPUS]` so `enable_bsp_idle()` verifies BSP ticks only.
- **24h soak harness**: `test_stress_24h.ps1` — configurable-duration QEMU
  run with TCP-chardev serial capture, panic/exception/triple-fault/QEMU-exit
  auto-detection and `target/stress_report.txt` reporting.

## 10. UI Polish & Accessibility (P3)

Low-priority polish items tracked here; they do not block releases.

| # | Item | Priority | Status |
| :- | :--- | :- | :- |
| 10.1 | UI tema ve erişilebilirlik testlerini artır | P3 | Todo — kontrast (WCAG AA/AAA), palet geçerliliği ve tema değiştirme testleri eklenecek (`tests/host_tests.rs` üzerinden `gui::theme`) |
| 10.2 | Türkçe karakter ve font fallback testlerini otomatikleştir | P3 | Todo — `ğ, ü, ş, İ, ı, ö, ç` karakterleri ve yedek font tier'ları için `font_render.rs` testleri eklenecek |
| 10.3 | Uygulama açılış süreleri ve bellek kullanımını ölç | P3 | Kısmi — `src/boot/timing.rs` `Stopwatch` eklendi (uygulama başlangıç süresi ölçümü); bellek ölçümü mevcut `MEM-PROFILE`/heap takibiyle |
| 10.4 | Açılış, kapanış ve hata ekranlarını görsel olarak iyileştir | P3 | Todo — `src/gui/screens.rs` ekranları geliştirilecek |
| 10.5 | Sürüm notlarında başarısız veya deneysel özellikleri açıkça işaretle | P3 | Todo — `docs/releases.md` deneysel/başarısız özellik etiketleri eklenecek |

## Success Metrics
- Boot time < 3s kernel-side (currently ~3.5s to desktop-ready; welcome
  screen is interactive and excluded from kernel timing)
- Idle CPU < 1% (currently ~3%)
- Zero panics in 24h stress test
- All public APIs documented
- Clippy clean build