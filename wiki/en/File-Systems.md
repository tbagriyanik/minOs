# File Systems

The minOs VFS layer supports several file systems, but each file system's capability level differs.

---

## File System Comparison

| File System | Support Level | Description |
| :--- | :--- | :--- |
| **FAT32** | **Full Read / Write / Format / Persistence** | Full file reading, writing, creation and deletion on MBR/GPT disks; `settings.cfg` and `VFS.DAT` system persistence. |
| **Ext2** | **Read / Write / Format** | Mounting ATA disk partitions via `src/fs/ext2.rs`, directory listing and file read/write. |
| **Ext4** | **Read / Write / Format** | `src/fs/ext4.rs` (format) + `src/fs/ext4_rw.rs` (read/write) with 1024-byte blocks, 256-byte inodes and multiple block groups. Block mapping is ext2-compatible (direct + single/double indirect); **extents tree, journal (JBD2), htree (dir_index) and inline data are not supported** — volumes with these features enabled are not mounted. The Linux `ext4` driver mounts the volume without problems. |
| **exFAT** | **Read / Write / Format** | `src/fs/exfat.rs` (format: VBR, FAT, allocation bitmap, up-case table) + `src/fs/exfat_rw.rs` (read/write: root/sub directories, file creation/deletion, FAT-chained multi-cluster files, Unicode names). Full validation of long-name entries and timestamp/ACL writing are not yet available. |
| **ISO9660** | **Read-only** | Read support for live CD/DVD and installation media contents. |

---

## VFS (Virtual File System)

- `src/fs/vfs.rs` — Virtual file system interface
- `src/fs/diskfs.rs` — Disk readers (FAT32 + ISO9660)
- `src/fs/part.rs` — MBR/GPT partitioning
- `src/fs/block.rs` — Block device abstraction

---

## Persistence

- `settings.cfg` — System settings persistent storage
- `VFS.DAT` — VFS tree snapshot
- `MINOS.SYS` — Installation marker file
- `BENIOKU.TXT` — Readme file

---

## Disk Formatting (MinShell)

| Command | Usage | Description |
| :--- | :--- | :--- |
| `format` | `format <fat32\|ext4\|exfat\|ext2> <0..3>` | Formats the connected ATA hard disk with the specified file system |
