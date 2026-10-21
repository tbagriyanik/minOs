//! Seri konsol ciktisi ve yeniden baslatma yardimcilari.

use crate::{fs, serial};

#[inline(never)]
pub fn serial_print(s: &str) {
    serial::print(s);
    crate::boot::text_console::write(s);
}

pub fn save_boot_log(vfs: &mut fs::vfs::VirtualFileSystem) {
    let log = serial::log_snapshot();
    let _ = vfs.write_file("/root/Documents/boot.log", &log);
}

#[inline(never)]
pub fn serial_print_dec(v: u64) {
    if v == 0 { serial_print("0"); return; }
    let mut value = v;
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while value > 0 {
        i -= 1;
        buf[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    if let Ok(text) = core::str::from_utf8(&buf[i..]) { serial_print(text); }
}

#[inline(never)]
pub fn serial_print_hex(v: u64) {
    serial_print("0x");
    if v == 0 { serial_print("0"); return; }
    let mut value = v;
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    let hex = b"0123456789ABCDEF";
    while value > 0 {
        i -= 1;
        buf[i] = hex[(value & 0xF) as usize];
        value >>= 4;
    }
    if let Ok(text) = core::str::from_utf8(&buf[i..]) { serial_print(text); }
}

pub fn restart_system() -> ! {
    serial_print("[minOs] Restarting system...\n");
    unsafe {
        // 1. QEMU/PC reset control register
        core::arch::asm!("out dx, al", in("dx") 0xCF9u16, in("al") 0x06u8);
        // 2. Legacy 8042 keyboard controller reset
        core::arch::asm!("out dx, al", in("dx") 0x64u16, in("al") 0xFEu8);
        // 3. Triple fault fallback if port reset fails
        let null_idt: [u16; 5] = [0, 0, 0, 0, 0];
        core::arch::asm!("lidt [{}]", in(reg) &null_idt);
        core::arch::asm!("int3");
        core::arch::asm!("cli");
    }
    loop { core::hint::spin_loop(); }
}
