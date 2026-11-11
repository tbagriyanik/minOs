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
**Solution**:
- Implement tiered allocator (small/large object separation)
- Add memory pressure notifications
- Implement allocation tracking for debugging
- Add configurable heap size via boot parameter

### 2.2 Page Frame Allocator
**Problem**: Basic bitmap allocator; no zone support (DMA, Normal, HighMem).
**Solution**:
- Add memory zones
- Implement per-cpu page caches
- Add huge page support (2MB/1GB)

### 2.3 Virtual Memory
**Problem**: Manual page table management; no ASLR for kernel.
**Solution**:
- Add KASLR support
- Implement copy-on-write for process memory
- Add memory protection keys (PKRU)

---

## 3. GUI Performance & UX (P1)

### 3.1 Rendering Pipeline
**Problem**: Full-frame redraw on every tick; limited dirty-rect support.
**Solution**:
- Full dirty-rect tracking per window
- Implement render command buffering
- Add frame pacing (vsync support)
- GPU acceleration path for virtio-gpu/AMD/NVIDIA

### 3.2 Window Management
**Problem**: O(n²) z-order sorting; no workspace support.
**Solution**:
- Use balanced tree for Z-order
- Add virtual desktop/workspace support
- Implement window snapping zones

### 3.3 Input Handling
**Problem**: Direct mouse/keyboard polling in main loop.
**Solution**:
- Event-driven input subsystem
- Input focus management
- Accessibility support (screen reader, high contrast)

### 3.4 Theme System
**Problem**: Global theme token; no per-app theming.
**Solution**:
- Theme inheritance (global → app → widget)
- Runtime theme loading from config
- Dark/light/auto modes

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
- Memory allocator tests
- VFS operation tests
- GUI widget tests
- Parser tests (HTTP, HTML, CSS)

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
- rustfmt/clippy checks
- Unit tests on host target
- QEMU integration tests
- Binary size tracking

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

## Success Metrics
- Boot time < 3s (currently ~5s)
- Idle CPU < 1% (currently ~3%)
- Zero panics in 24h stress test
- All public APIs documented
- Clippy clean build