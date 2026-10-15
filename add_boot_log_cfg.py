import re

with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

replacements = {
    'serial_print("[minOs] Theme engine initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Theme engine initialized.");',
    'serial_print("[minOs] Splash rendered; initializing GUI state.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Splash rendered; initializing GUI state.");',
    'serial_print("[minOs] Compositor initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Compositor initialized.");',
    'serial_print("[minOs] File manager state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] File manager state initialized.");',
    'serial_print("[minOs] File manager populated.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] File manager populated.");',
    'serial_print("[minOs] Settings state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Settings state initialized.");',
    'serial_print("[minOs] Notepad state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Notepad state initialized.");',
    'serial_print("[minOs] Browser state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Browser state initialized.");',
    'serial_print("[minOs] ImageViewer state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ImageViewer state initialized.");',
    'serial_print("[minOs] MusicPlayer state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] MusicPlayer state initialized.");',
    'serial_print("[minOs] VideoPlayer state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] VideoPlayer state initialized.");',
    'serial_print("[minOs] TaskManager state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] TaskManager state initialized.");',
    'serial_print("[minOs] Calculator state initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Calculator state initialized.");',
    'serial_print("[minOs] Paint slots initialized.");': '#[cfg(minos_boot_log)]\n    serial_print("[minOs] Paint slots initialized.");',
}

for old, new in replacements.items():
    content = content.replace(old, new)

with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print('Done adding cfg attributes')