# Supported Hardware List

Current status table of native drivers and hardware layers inside the minOs kernel.

> **Status Indicators:**
> - 🟢 **Full Support:** Stable for daily use, all core and advanced functions fully operational.
> - 🟡 **Partial Support:** Core functionality and the driver are operational; some advanced profiles/features are under development.
> - 🔴 **Skeleton / WIP:** Device detection or infrastructure draft exists; full protocol integration is planned.

---

## Graphics & Display

| Hardware / Device | Driver Module | Status | Scope & Capabilities |
| :--- | :--- | :---: | :--- |
| Bochs BGA / QEMU Standard VGA | `gpu.rs` | 🟢 Full | 1080p+, dynamic resolution change, LFB |
| VirtIO-GPU (QEMU/KVM) | `virtio_gpu.rs` | 🟢 Full | Split VirtQueue, 2D command set, scanout, default desktop hardware 2D render & flush path, mode-set |
| NVIDIA GeForce GPU | `nvidia.rs` | 🟡 Partial | BAR0/BAR1 VRAM LFB, 2D drawing, mode-set (3D acceleration planned) |
| AMD Radeon GPU | `amdgpu.rs` | 🟡 Partial | BAR0 64-bit VRAM LFB, 2D drawing, mode-set (3D acceleration planned) |
| VMware SVGA-II | `gpu.rs` | 🟡 Partial | PCI detection, basic resolution reading |

---

## Network & Communication

| Hardware / Device | Driver Module | Status | Scope & Capabilities |
| :--- | :--- | :---: | :--- |
| Intel e1000 / e1000e (Gigabit) | `e1000.rs` | 🟢 Full | DMA ring, DHCP, DNS, TCP/UDP, IPv4/IPv6, ICMPv6, TLS 1.3 |
| Intel I219-V Ethernet | `i219.rs` | 🟢 Full | Gigabit Ethernet link and packet transfer |
| Realtek PCIe (RTL8168/8169/8125) | `net.rs` | 🟢 Full | C+ mode, 64-bit RX/TX rings, PHYSTATUS link tracking |
| Broadcom NetXtreme (BCM57xx) | `net.rs` | 🟢 Full | MAC hash, RX/TX, PHY link check |
| VirtIO-net (QEMU/KVM/Cloud) | `virtio_net.rs` | 🟢 Full | High-speed virtualized network adapter |
| Atheros Ethernet | `atheros.rs` | 🟡 Partial | Basic link and packet transfer (offload filters TODO) |
| Marvell Ethernet | `marvell.rs` | 🟡 Partial | Basic RX/TX packet processing |
| Bluetooth HCI | `bluetooth.rs` | 🟡 Partial | Command (0x01), ACL (0x02), Event (0x04), Inquiry, connection management (A2DP/GATT TODO) |
| Wi-Fi (802.11 Wireless) | `wifi.rs` | 🔴 Skeleton | PCI/USB interface detection (WPA handshake WIP) |
| VMware VMXNET3 | `e1000.rs` | 🟢 Full | VMware PCI vendor `0x15AD`, MAC address, link polling |

---

## Input Devices

| Hardware / Device | Driver Module | Status | Scope & Capabilities |
| :--- | :--- | :---: | :--- |
| PS/2 Keyboard & Mouse | `ps2.rs`, `keyboard.rs`, `mouse.rs` | 🟢 Full | Interrupt-driven input, Turkish Q / English key mapping |
| USB HID Keyboard & Mouse | `usb_hid.rs`, `usb_transfer.rs` | 🟢 Full | xHCI/EHCI/UHCI/OHCI full transfer layer, 16-bit axes, wheel, hotplug |
| USB Touchscreen | `usb_touchscreen.rs` | 🟡 Partial | Single-finger touch/click (multi-touch analysis TODO) |

---

## Storage

| Hardware / Device | Driver Module | Status | Scope & Capabilities |
| :--- | :--- | :---: | :--- |
| ATA / IDE / ATAPI | `ata.rs` | 🟢 Full | LBA28/48, hard disk and CD/DVD-ROM |
| AHCI / SATA | `ahci.rs` | 🟢 Full | SATA SSD and HDD block I/O |
| NVMe SSD (PCIe M.2) | `nvme.rs` | 🟢 Full | NVMe command queues and block read/write |
| VirtIO-blk (QEMU/KVM) | `virtio_blk.rs` | 🟢 Full | Virtual block device integration |
| USB Mass Storage (Flash Drive) | `usb_mass_storage.rs` | 🟢 Full | Storage over the SCSI BOT transfer protocol |
| SD / MMC Card | `sdmmc.rs` | 🟡 Partial | Sector reading active (CSD parsing and reliable writes WIP) |

---

## Power & Sensors

| Hardware / Device | Driver Module | Status | Scope & Capabilities |
| :--- | :--- | :---: | :--- |
| ACPI AML Interpreter & Battery | `acpi_battery.rs` | 🟢 Full | DSDT/SSDT AML parsing, `_BST`/`_BIX`/`_BIF` live percentage, voltage, current, cycles |
| ACPI Power Management | `acpi_power.rs` | 🟢 Full | `_S0`..`_S5` sleep states detection, PowerResource, dynamic AC/Battery status tracking |
| Fan & Temperature Sensors | `fan_temp.rs` | 🟡 Partial | ACPI ThermalZone parsing, Celsius conversion, fan RPM monitoring |
| Bus & Timers | `pci.rs`, `pcie.rs`, `timer.rs` | 🟢 Full | PCI/PCIe configuration, TSC/HPET/APIC timers |

---

## Audio & Multimedia

| Hardware / Device | Driver Module | Status | Scope & Capabilities |
| :--- | :--- | :---: | :--- |
| Intel High Definition Audio (HDA) | `hda.rs`, `audio.rs` | 🟢 Full | DMA stream rings, mixer, WAV/MP3 playback |
| PC Speaker (PWM) | `audio.rs` | 🟢 Full | Hardware timer beeps and PWM tones |
| USB Audio | `usb_audio.rs` | 🟡 Partial | Speaker/headphone output (microphone recording TODO) |

---

## File Systems

| File System | Support Level | Description |
| :--- | :--- | :--- |
| **FAT32** | Full Read / Write / Format / Persistence | Full file reading, writing, creation and deletion on MBR/GPT disks |
| **Ext2** | Read / Write / Format | Mounting ATA disk partitions, directory listing and file read/write |
| **Ext4** | Read / Write / Format | 1024-byte blocks, 256-byte inodes, multiple block groups (extents, journal, htree not supported) |
| **exFAT** | Read / Write / Format | VBR, FAT, allocation bitmap, up-case table, Unicode names |
| **ISO9660** | Read-only | Read support for live CD/DVD and installation media contents |
