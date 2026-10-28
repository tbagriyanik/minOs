# About & Architecture

minOs (Sarman Edition) v2.1.0 is a standalone 64-bit (`x86_64`) operating system written in Rust that uses `no_std` and `no_main` in the kernel. It boots via Limine v8 on both UEFI and BIOS (Legacy) environments.

---

## Design Philosophy

- **Minimal and Standalone:** Not built on top of any existing operating system; direct hardware access.
- **Rust Safety:** Memory safety, type safety, and data race prevention in a `no_std` environment.
- **Modular Architecture:** Drivers, file systems, GUI, and applications are organized as separate modules.
- **Education and Research:** Open source, developed for learning and experimentation purposes.

---

## Core Components

| Component | File | Description |
| :--- | :--- | :--- |
| **Kernel Entry** | `src/main.rs` | `_start` entry point, Limine requests, hardware setup |
| **Architecture** | `src/arch/x86_64/` | SSE, IDT interrupts, PIC masking, GDT/TSS |
| **Boot** | `src/boot/` | Limine console, text rendering, progress bar |
| **Drivers** | `src/drivers/` | GPU, network, storage, input, audio, USB, ACPI, crypto |
| **GUI** | `src/gui/` | Window manager, font engine, theme engine |
| **Applications** | `src/apps/` | 15 desktop applications |
| **Desktop** | `src/desktop/` | Session management, main event loop |
| **Memory** | `src/mm/` | Physical memory manager (PMM), ELF loader |
| **File System** | `src/fs/` | VFS, FAT32, Ext2, Ext4, exFAT, ISO9660 |

---

## `no_std` Model

The kernel is compiled without the Rust standard library. Memory allocation uses `linked_list_allocator`. Bitmap atlases from TTF fonts are generated at build time using `fontdue` and used in the graphical interface.

---

## Memory Management (PMM)

- **Static BSS Architecture:** `MemoryManager` moved to static `.bss` segment (CodeQL Alert #25 fix).
- **Stack Escape Prevention:** Raw pointer resolution made safe.
- **Page Tables:** VRAM range mapped as 2 MB huge pages.

---

## Interrupt and Exception Handling (IDT)

From the very first moment of boot, BSP GDT+TSS (`IST1`) and IDT exception gates are loaded:

- `#DE` (Divide Error)
- `#UD` (Invalid Opcode)
- `#DF` (Double Fault)
- `#SS` (Stack Segment Fault)
- `#GP` (General Protection Fault)
- `#PF` (Page Fault)

Sudden motherboard reboots (triple fault) are prevented on hardware faults during driver initialization.

---

## SMP (Symmetric Multiprocessing)

- Limine SMP protocol is available.
- Default profile starts in single-core mode (physical laptop stability).
- AP boot code exists in `src/arch/x86_64/smp.rs` but is not used in the default boot path.
- TTAS spinlock synchronization and per-core CPU monitoring bars are available.
