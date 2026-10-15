with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Fix the remaining missing cfg attributes
fixes = [
    # Line 256 - restart system
    ('serial_print("[minOs] Restart requested from Start Menu.\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Restart requested from Start Menu.\n");'),
    # Line 714-726 - Limine framebuffer errors
    ('serial_print("[minOs] ERROR: Limine framebuffer list is NULL!\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ERROR: Limine framebuffer list is NULL!\n");'),
    ('serial_print("[minOs] ERROR: Limine framebuffer entry is NULL!\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ERROR: Limine framebuffer entry is NULL!\n");'),
    ('serial_print("[minOs] ERROR: Limine framebuffer buffer address is NULL or non-canonical!\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ERROR: Limine framebuffer buffer address is NULL or non-canonical!\n");'),
    ('serial_print("[minOs] ERROR: Limine Framebuffer response is NULL!\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ERROR: Limine Framebuffer response is NULL!\n");'),
    # Line 797 - File manager state initialized (this was skipped earlier)
    ('serial_print("[minOs] File manager state initialized.\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] File manager state initialized.\n");'),
    # Line 799 - File manager populated
    ('serial_print("[minOs] File manager populated.\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] File manager populated.\n");'),
    # Line 899 - Window list initialized
    ('serial_print("[minOs] Window list initialized.\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Window list initialized.\n");'),
    # Line 906 - Entering welcome screen
    ('serial_print("[minOs] Entering welcome screen.\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Entering welcome screen.\n");'),
    # Line 974 - RTC clock online
    ('serial_print("[minOs] RTC clock online.\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] RTC clock online.\n");'),
    # Shutdown messages
    ('serial_print("[minOs] System Shutdown confirmed via Start Menu.\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] System Shutdown confirmed via Start Menu.\n");'),
    ('serial_print("[minOs] System Shutdown confirmed via Keyboard.\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] System Shutdown confirmed via Keyboard.\n");'),
    ('serial_print("[minOs] System Shutdown requested!\n");', '#[cfg(minos_boot_log)]\n    serial_print("[minOs] System Shutdown requested!\n");'),
]

for old, new in fixes:
    content = content.replace(old, new)

with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print('Done fixing remaining cfg attributes')