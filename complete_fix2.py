#!/usr/bin/env python3
import re

with open('src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# 2. Limine framebuffer errors - check exact format
# From the file: serial_print("[minOs] ERROR: Limine framebuffer list is NULL!\n");
patterns_fixed = [
    # Limine framebuffer list is NULL
    ('serial_print("[minOs] ERROR: Limine framebuffer list is NULL!\\n");', 
     '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ERROR: Limine framebuffer list is NULL!\\n");'),
    # Limine framebuffer entry is NULL
    ('serial_print("[minOs] ERROR: Limine framebuffer entry is NULL!\\n");', 
     '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ERROR: Limine framebuffer entry is NULL!\\n");'),
    # Limine framebuffer buffer address is NULL or non-canonical
    ('serial_print("[minOs] ERROR: Limine framebuffer buffer address is NULL or non-canonical!\\n");', 
     '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ERROR: Limine framebuffer buffer address is NULL or non-canonical!\\n");'),
    # Limine Framebuffer response is NULL
    ('serial_print("[minOs] ERROR: Limine Framebuffer response is NULL!\\n");', 
     '#[cfg(minos_boot_log)]\n    serial_print("[minOs] ERROR: Limine Framebuffer response is NULL!\\n");'),
]

for old, new in patterns_fixed:
    if old in content:
        content = content.replace(old, new)
        print(f"Fixed: {old[:30]}...")
    else:
        print(f"NOT FOUND: {old[:30]}...")

# Shutdown messages - let me check their format too
# Check lines around 1513, 2398, 2554
import subprocess
result = subprocess.run(['python', '-c', '''
with open(\"F:\\\\minos\\\\src\\\\main.rs\", \"r\") as f:
    lines = f.readlines()
for i in [1512, 2397, 2553]:
    if 0 <= i < len(lines):
        print(f\"Line {i+1}: {repr(lines[i])}\")
'''], capture_output=True, text=True)
print(result.stdout)

with open('src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print('\\nDone')