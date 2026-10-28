# Installation and Running

All steps to build, create an ISO, and run minOs on QEMU or real hardware.

---

## Prerequisites

| Tool | Description |
| :--- | :--- |
| **Rust Nightly** | `rustup default nightly` and `rustup component add rust-src` |
| **QEMU x86_64** | [QEMU Official Download](https://www.qemu.org/download/#windows) |
| **ISO Creation** | Windows: `oscdimg.exe`; Linux/macOS: `xorriso` or `mkisofs` |
| **PowerShell 5.1+** | Build and ISO packaging automation |

---

## Build and Run in One Command (Recommended)

```powershell
git clone https://github.com/tbagriyanik/minos.git
cd minos
powershell -ExecutionPolicy Bypass -File .\run.ps1
```

### `run.ps1` Options

| Key | Description |
| :--- | :--- |
| `-BuildOnly` | Build only; QEMU is not launched |
| `-Debug` | Build with debug profile (default is release) |
| `-NoIso` | Do not mount ISO; boot directly from hard disk |
| `-Live` | Force live environment (`-boot d`) |
| `-ReprepareDisk` | Reformat the hard disk |
| `-DiskImage dosya.qcow2` | Virtual disk image path (default `.\disk.qcow2`) |
| `-UsbDisks` | Mount USB disks read-only |
| `-UsbDiskReadWrite` | Mount USB disks with write permission |
| `-UsbHid` | Add `usb-kbd` and `usb-mouse` to QEMU |
| `-NoPs2` | Disable PS/2 controller; `-UsbHid` auto-enabled |
| `-Display <sdl\|gtk\|default>` | QEMU graphics backend |
| `-CpuCount N` | Virtual CPU count (default 2, max 8) |
| `-Memory 1G` | Guest RAM amount (default 512M) |

---

## Boot from Hard Disk (QCOW2 / RAW)

```powershell
powershell -ExecutionPolicy Bypass -File .\run.ps1 -NoIso -DiskImage .\disk.qcow2
```

If the disk is empty or unformatted, QEMU will not boot and a safe error message is shown. First boot from ISO and run the "Install" wizard, or prepare the disk with `build_disk.ps1`.

### Disk Preparation (Method B - PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -File .\build_disk.ps1 -DiskImage .\disk.qcow2 -Force
```

---

## Manual Build Steps

### 1. Build the Kernel

```powershell
cargo build --release -Zjson-target-spec --target x86_64-minos.json `
  -Zbuild-std=core,alloc -Zbuild-std-features=compiler-builtins-mem
```

### 2. Run Unit Tests

```powershell
powershell -ExecutionPolicy Bypass -File .\test_host.ps1
```

### 3. Create the ISO Image

```powershell
powershell -ExecutionPolicy Bypass -File .\build_iso.ps1
```

`minos.iso` is created in the project root.

### 4. Run in QEMU

**Linux (PulseAudio + Intel HDA + e1000):**
```bash
qemu-system-x86_64 -cdrom minos.iso -m 512M -smp 2 -vga std -global VGA.vgamem_mb=64 \
  -audiodev pa,id=snd0 -device intel-hda -device hda-duplex,audiodev=snd0 \
  -netdev user,id=n0 -device e1000,netdev=n0
```

**Windows (SDL + Intel HDA Output + e1000):**
```powershell
qemu-system-x86_64.exe -cdrom .\minos.iso -m 512M -smp 2 -vga std -global VGA.vgamem_mb=64 `
  -audiodev driver=sdl,id=snd0 -device intel-hda -device hda-output,audiodev=snd0 `
  -netdev user,id=n0 -device e1000,netdev=n0
```

---

## VirtualBox Setup

| Setting | Recommended Value |
| :--- | :--- |
| OS Type | Other / Other (64-bit) |
| Memory | 128 MB – 512 MB |
| CPU | 1 vCPU |
| Video Memory | 16 MB – 32 MB |
| Graphics Controller | VBoxVGA or VMSVGA (3D disabled) |
| Network Adapter | Intel PRO/1000 MT Desktop (82540EM) |
| Audio Controller | Intel HD Audio |

> **Troubleshooting:**
> - `PANIC: limine: This CPU does not support 64-bit mode`: Ensure VM type is `Other (64-bit)`.
> - Black screen: Set graphics controller to `VBoxVGA`, disable 3D acceleration, VRAM at least 16 MB.
> - UEFI: `VBoxManage modifyvm "minOs" --firmware efi`.

---

## Minimum Hardware Requirements

| Component | Minimum | Recommended |
| :--- | :--- | :--- |
| CPU | x86_64, SSE2 | x86_64, SSE4.1+ |
| RAM | 4 MB | 32 MB – 64 MB |
| Storage | CD/ISO only | 64 MB+ qcow2 disk |
| Display | 1024×768 32-bit | 1024×768 or higher |
| Audio | — (optional) | HDA compatible |
| Network | — (optional) | Realtek PCIe / Intel e1000 / VMware VMXNET3 |
