# User Guide and Shortcuts

Desktop usage, MinShell commands and shortcut reference.

---

## MinShell Commands

### File and Directory Management

| Command | Usage | Description |
| :--- | :--- | :--- |
| `pwd` | `pwd` | Prints current working directory |
| `cd` | `cd <dir>` | Changes working directory |
| `ls` | `ls [dir]` | Lists files and folders |
| `cat` | `cat <file>` | Prints text file content |
| `touch` | `touch <file>` | Creates new empty file |
| `write` | `write <file> <text>` | Writes text to file |
| `mkdir` | `mkdir <folder>` | Creates new folder |
| `rm` | `rm <target>` | Deletes file or directory |
| `disks` / `df` | `disks` or `df` | Lists connected drives |

### System Information and Memory

| Command | Usage | Description |
| :--- | :--- | :--- |
| `mem` | `mem` | Shows total and free memory |
| `ps` | `ps` | Lists active system processes |
| `date` | `date` | Shows date and time from RTC |
| `uname` | `uname` | Prints kernel version and architecture |
| `whoami` | `whoami` | Shows active user privileges |
| `theme` | `theme` | Switches desktop theme to next |
| `uptime` | `uptime` | Shows system uptime |
| `fetch` / `neofetch` | `fetch` or `neofetch` | Shows system summary with ASCII mascot |
| `clear` | `clear` | Clears terminal screen |
| `help` | `help` | Shows summary of available commands |

### Network Diagnostics and Tools

| Command | Usage | Description |
| :--- | :--- | :--- |
| `ipconfig` | `ipconfig` | Reports `eth0` interface information |
| `ping` | `ping <IPv4>` | Sends ICMP Echo Request |
| `ping6` | `ping6 <IPv6>` | Sends ICMPv6 Echo Request |
| `dns` | `dns <domain>` | Sends DNS query |
| `curl` | `curl http://<host>[:port][/path]` | HTTP GET download |

### Disk and Kernel Commands

| Command | Usage | Description |
| :--- | :--- | :--- |
| `format` / `mkfs` | `format <fat32\|ext4\|exfat\|ext2> <0..3>` | Formats hard disk with specified file system |
| `minPak` / `pkg` | `minPak <list\|search\|install\|remove>` | Package management |
| `exec` | `exec <file_path> [args]` | Runs ELF/binary file in Ring 3 |

### Application Launching

| Command | Alternatives | Description |
| :--- | :--- | :--- |
| `open <app>` | `run <app>` | Opens application in new or existing window |
| Direct Name | `files`, `notepad`, `calc`, `paint`, `browser`, `image`, `music`, `video`, `taskmgr`, `settings`, `cmd`, `pkgmgr` | Application name used directly as command |

---

## Desktop Usage and Shortcuts

- **Start Menu:** Navigate with Up/Down/Left/Right arrow keys, Enter to launch, mouse wheel to scroll.
- **Boot Resolution:** Select from Limine menu under `/Display Resolutions` or save from System Settings.
- **Window Resizing:** Drag from window edge or corner (12px grab area).
- **Window Dragging:** Click and drag the top title bar.
- **Window Controls:**
  - Red `[x]`: Closes
  - Green `[□]`: Maximizes (click again to restore; double-click also works)
  - Yellow `[-]`: Minimizes to taskbar
- **Window Snap:** Drag title bar to top of screen (auto-maximize).
- **Window Switching:** `Alt+Tab` (forward), `Shift+Alt+Tab` (backward).
- **Super Shortcuts:** `Super+D` desktop, `Super+M` minimize all windows, `Super+E` File Manager.
- **System Tray:** Keyboard layout (`TR`/`EN`), theme button, volume button, live clock.
- **Theme Change:** Taskbar color swatch, System Settings > Change, theme palette strip, or desktop right-click > Change Theme.
- **Notepad Shortcuts:** `Ctrl+C` (Copy), `Ctrl+X` (Cut), `Ctrl+V` (Paste), `Ctrl+A` (Select All), `Ctrl+Z` (Undo), `Ctrl+Y` (Redo), `Delete`, `Home`/`End`, `Shift+Arrow keys`.
- **Paint:** Select color, Eraser, Open or Save; draw with left mouse button held. `C` clears, `E` toggles eraser. Saved as 24-bit BMP to `/root/Documents/cizim.bmp`.
- **Image Viewer:** `+`/`=` zoom in, `-` zoom out, `0` default scale.
- **Media Keys:** Volume up, volume down, mute, play/pause, previous/next track.
- **Refresh (`F5`):** Refreshes focused application content.
- **Video Player:** `Play/Pause` toggles animation and timeline; `Stop` resets to beginning.

---

## Boot Error and Recovery Console (`recovery#`)

On kernel exceptions (`#DE`, `#UD`, `#DF`, `#SS`, `#GP`, `#PF`) and critical driver faults, the interactive text console is shown.

| Command | Alternatives | Description |
| :--- | :--- | :--- |
| `[Enter]` | `restart`, `reboot`, `r` | Restarts the system safely |
| `halt` | `shutdown`, `h` | Halts the processor (`cli` and `hlt`) |
| `help` | `?` | Lists available commands |

> Commands are executed only when **Enter** is pressed.
