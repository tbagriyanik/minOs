#!/usr/bin/env python3
with open('F:\\minos\\src\\main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Fix shutdown messages using raw strings to handle \n correctly
# The actual file content has: serial_print("[minOs] System Shutdown confirmed via Start Menu!\n");
# In Python, use raw string r"..." to preserve the \n as literal

old1 = r'serial_print("[minOs] System Shutdown confirmed via Start Menu!\n");'
new1 = '#[cfg(minos_boot_log)]\n    ' + old1
if old1 in content:
    content = content.replace(old1, new1)
    print('Fixed shutdown 1')
else:
    print('NOT FOUND: shutdown 1 - tried pattern:', repr(old1))

old2 = r'serial_print("[minOs] System Shutdown confirmed via Keyboard!\n");'
new2 = '#[cfg(minos_boot_log)]\n    ' + old2
if old2 in content:
    content = content.replace(old2, new2)
    print('Fixed shutdown 2')
else:
    print('NOT FOUND: shutdown 2 - tried pattern:', repr(old2))

old3 = r'serial_print("[minOs] System Shutdown requested!\n");'
new3 = '#[cfg(minos_boot_log)]\n    ' + old3
if old3 in content:
    content = content.replace(old3, new3)
    print('Fixed shutdown 3')
else:
    print('NOT FOUND: shutdown 3 - tried pattern:', repr(old3))

with open('F:\\minos\\src\\main.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print('\\nDone')