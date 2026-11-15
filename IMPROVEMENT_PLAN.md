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
**Status**: Partially done — 2 MiB huge-page alloc/free landed
(`allocate_big_page`/`free_big_page`, `bigpage.rs`), contiguous DMA
allocation exists, and the whole PMM is now SMP-safe via an internal
`PMM_LOCK` spinlock (all mutating methods serialize; `*_locked` private
variants avoid self-deadlock). Remaining: memory zones, per-cpu page
caches.
**Solution**:
- Add memory zones
- Implement per-cpu page caches

### 2.3 Virtual Memory
**Problem**: Manual page table management; no ASLR for kernel.
**Status**: Partially done — user-space ASLR and copy-on-write landed
(`[USER-MEM]`/`[COW-TEST]` boot checks, `mm::user_mem`, `mm::memory::cow`).
Remaining: KASLR, PKRU.

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
workspace-aware focus/alt-tab), Aero Snap preview exists. Remaining:
O(n²) z-order scan (still a linear `draw_order` pass), more snap zones.

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
- Enforce certificate verification
- Add HSTS support
- Implement secure defaults

---

## 6. Testing Infrastructure (P2)

### 6.1 Unit Tests
- **Done (host suite)**: `tests/host_tests.rs` + `test_host.ps1` run ~300
  host-side unit tests covering allocator, VFS/fs (ext4/exfat), crypto,
  TLS, JS engine, widgets/themes readability, drivers' pure logic.
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
- Unit tests on host target — `test_host.ps1` (295+ tests passing)
- QEMU integration tests — `test_qemu.ps1`, `boot_check.ps1`
- Binary size tracking

---

## 9. Process & Kernel Roadmap (P1)

Scheduler/process subsystem (`src/task`, `src/mm/process.rs`, `src/arch/x86_64/ring3.rs`).

| # | Item | Priority | Status |
| :- | :--- | :--- | :--- |
| 9.1 | Multi-core scheduler stress tests | P1 | Partial — `task::test_scheduler_multicore_race` exists; needs repeated/soak runs and per-CPU accounting checks |
| 9.2 | Scheduler tests on real hardware | P1 | Todo — validate APIC/SMP paths outside QEMU |
| 9.3 | CPU affinity support | P2 | Partial — `Task::cpu` + `least_loaded_cpu` assign queues; no user-controllable affinity API |
| 9.4 | Deeper security audit of process isolation | P0 | Todo — audit CR3 switching, kernel-pointer leaks, syscall pointer validation coverage |
| 9.5 | Signal mechanism for user applications | P1 | Partial — `pending_signals` bitmask + `signals` module landed; user handler delivery pending |
| 9.6 | `kill`, `pause`, `resume` system calls | P1 | **Done** — `SYS_KILL` (19): SIGKILL/SIGTERM terminate, SIGSTOP suspends (`ProcessState::Suspended`), SIGCONT resumes, `sig=0` probes; other signals pended |
| 9.7 | Process group and session support | P2 | Todo — pgid/sid fields, group kill, orphan reparenting rules |
| 9.8 | Sandboxing of user applications | P1 | Todo — seccomp-style syscall filtering, resource-limit tightening, optional restricted address space |

Notes:
- `Suspended` is a single process state; a process blocked in `SYS_WAIT` cannot
  currently be stopped (blocked+stopped is not representable yet).
- Signal delivery to Ring 3 handlers, `sigaction`-style registration, and
  `waitpid` status encoding (signal vs exit code) are part of 9.5.

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
  shows `[BOOT-TIME] phase=Nms toplam=Mms` and a final `SURUM=` line.
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

## Success Metrics
- Boot time < 3s kernel-side (currently ~3.5s to desktop-ready; welcome
  screen is interactive and excluded from kernel timing)
- Idle CPU < 1% (currently ~3%)
- Zero panics in 24h stress test
- All public APIs documented
- Clippy clean build