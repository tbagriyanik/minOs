#!/usr/bin/env python3
import re

with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Check what the actual content looks like for specific lines
# The \n is inside the Rust string literal

# 1. "Restart requested from Start Menu" - line 256
# The actual content is: serial_print("[minOs] Restart requested from Start Menu.\n");
old1 = 'serial_print("[minOs] Restart requested from Start Menu.\\n");'
new1 = '#[cfg(minos_boot_log)]\n    ' + old1
if old1 in content:
    content = content.replace(old1, new1)
    print(f"Fixed 1: Restart requested")
else:
    print(f"NOT FOUND: Restart requested pattern")

# 2. Limine framebuffer errors
old2a = 'serial_print("[minOs] ERROR: Limine framebuffer list is NULL.\\n");'
new2a = '#[cfg(minos_boot_log)]\n    ' + old2a
if old2a in content:
    content = content.replace(old2a, new2a)
    print(f"Fixed 2a: Limine framebuffer list is NULL")
else:
    print(f"NOT FOUND: Limine framebuffer list is NULL pattern")

old2b = 'serial_print("[minOs] ERROR: Limine framebuffer entry is NULL.\\n");'
new2b = '#[cfg(minos_boot_log)]\n    ' + old2b
if old2b in content:
    content = content.replace(old2b, new2b)
    print(f"Fixed 2b: Limine framebuffer entry is NULL")
else:
    print(f"NOT FOUND: Limine framebuffer entry is NULL pattern")

old2c = 'serial_print("[minOs] ERROR: Limine framebuffer buffer address is NULL or non-canonical.\\n");'
new2c = '#[cfg(minos_boot_log)]\n    ' + old2c
if old2c in content:
    content = content.replace(old2c, new2c)
    print(f"Fixed 2c: Limine framebuffer buffer address")
else:
    print(f"NOT FOUND: Limine framebuffer buffer address pattern")

old2d = 'serial_print("[minOs] ERROR: Limine Framebuffer response is NULL.\\n");'
new2d = '#[cfg(minos_boot_log)]\n    ' + old2d
if old2d in content:
    content = content.replace(old2d, new2d)
    print(f"Fixed 2d: Limine Framebuffer response is NULL")
else:
    print(f"NOT FOUND: Limine Framebuffer response is NULL pattern")

# 3. "File manager state initialized"
old3 = 'serial_print("[minOs] File manager state initialized.\\n");'
new3 = '#[cfg(minos_boot_log)]\n    ' + old3
if old3 in content:
    content = content.replace(old3, new3)
    print(f"Fixed 3: File manager state initialized")
else:
    print(f"NOT FOUND: File manager state initialized pattern")

# 4. "File manager populated"
old4 = 'serial_print("[minOs] File manager populated.\\n");'
new4 = '#[cfg(minos_boot_log)]\n    ' + old4
if old4 in content:
    content = content.replace(old4, new4)
    print(f"Fixed 4: File manager populated")
else:
    print(f"NOT FOUND: File manager populated pattern")

# 5. "Window list initialized"
old5 = 'serial_print("[minOs] Window list initialized.\\n");'
new5 = '#[cfg(minos_boot_log)]\n    ' + old5
if old5 in content:
    content = content.replace(old5, new5)
    print(f"Fixed 5: Window list initialized")
else:
    print(f"NOT FOUND: Window list initialized pattern")

# 6. "Entering welcome screen"
old6 = 'serial_print("[minOs] Entering welcome screen.\\n");'
new6 = '#[cfg(minos_boot_log)]\n    ' + old6
if old6 in content:
    content = content.replace(old6, new6)
    print(f"Fixed 6: Entering welcome screen")
else:
    print(f"NOT FOUND: Entering welcome screen pattern")

# 7. "RTC clock online"
old7 = 'serial_print("[minOs] RTC clock online.\\n");'
new7 = '#[cfg(minos_boot_log)]\n    ' + old7
if old7 in content:
    content = content.replace(old7, new7)
    print(f"Fixed 7: RTC clock online")
else:
    print(f"NOT FOUND: RTC clock online pattern")

# 8. Shutdown messages
old8a = 'serial_print("[minOs] System Shutdown confirmed via Start Menu.\\n");'
new8a = '#[cfg(minos_boot_log)]\n    ' + old8a
if old8a in content:
    content = content.replace(old8a, new8a)
    print(f"Fixed 8a: Shutdown confirmed via Start Menu")
else:
    print(f"NOT FOUND: Shutdown confirmed via Start Menu pattern")

old8b = 'serial_print("[minOs] System Shutdown confirmed via Keyboard.\\n");'
new8b = '#[cfg(minos_boot_log)]\n    ' + old8b
if old8b in content:
    content = content.replace(old8b, new8b)
    print(f"Fixed 8b: Shutdown confirmed via Keyboard")
else:
    print(f"NOT FOUND: Shutdown confirmed via Keyboard pattern")

old8c = 'serial_print("[minOs] System Shutdown requested.\\n");'
new8c = '#[cfg(minos_boot_log)]\n    ' + old8c
if old8c in content:
    content = content.replace(old8c, new8c)
    print(f"Fixed 8c: System Shutdown requested")
else:
    print(f"NOT FOUND: System Shutdown requested pattern")

with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print('\\nDone applying all cfg fixes')