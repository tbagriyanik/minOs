use crate::drivers::mouse::MouseState;
use crate::drivers::keyboard::KeyboardState;

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack, preserves_flags));
    val
}

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

fn wait_write() {
    for _ in 0..10_000 {
        if unsafe { inb(0x64) } & 0x02 == 0 {
            return;
        }
    }
}

fn wait_read() -> bool {
    for _ in 0..10_000 {
        if unsafe { inb(0x64) } & 0x01 != 0 {
            return true;
        }
    }
    false
}

static mut MOUSE_CYCLE: u8 = 0;
static mut MOUSE_BYTE: [u8; 4] = [0; 4];
static mut HAS_WHEEL: bool = false;

fn mouse_write(cmd: u8) {
    wait_write();
    unsafe { outb(0x64, 0xD4); }
    wait_write();
    unsafe { outb(0x60, cmd); }
    if wait_read() {
        unsafe { let _ = inb(0x60); }
    }
}

pub fn init() {
    unsafe {
        // 1. Çıkış arabelleğinde kalan artık baytları temizle
        while inb(0x64) & 0x01 != 0 {
            let _ = inb(0x60);
        }

        // 2. İkinci PS/2 portunu (Fare / Auxiliary) etkinleştir
        wait_write();
        outb(0x64, 0xA8);

        // 3. Birinci PS/2 portunu (Klavye) etkinleştir
        wait_write();
        outb(0x64, 0xAE);

        // 4. Denetleyici komut baytını oku ve yapılandır
        wait_write();
        outb(0x64, 0x20);
        if wait_read() {
            let mut status = inb(0x60);
            status |= 0x03;   // IRQ1 ve IRQ12 bitlerini etkinleştir
            status &= !0x30;  // Saat hatlarını aç
            wait_write();
            outb(0x64, 0x60);
            wait_write();
            outb(0x60, status);
        }

        // Klavye LED'leri: Scroll Lock=0, Num Lock=1, Caps Lock=0.
        // ACK (0xFA) ve RESEND (0xFE) yanıtlarını kısa timeout ile tüket.
        wait_write();
        outb(0x60, 0xED);
        if wait_read() && inb(0x60) == 0xFA {
            wait_write();
            outb(0x60, 0x02);
            if wait_read() { let _ = inb(0x60); }
        }

        // 5. Fareyi varsayılan ayarlara al
        mouse_write(0xF6);

        // IntelliMouse (Scroll Wheel) etkinleştirme sihirli dizisi (Sample Rate: 200 -> 100 -> 80)
        mouse_write(0xF3); mouse_write(200);
        mouse_write(0xF3); mouse_write(100);
        mouse_write(0xF3); mouse_write(80);

        // Fare kimliğini (Device ID) kontrol et (0x03 ise scroll wheel destekli)
        wait_write(); outb(0x64, 0xD4);
        wait_write(); outb(0x60, 0xF2);
        if wait_read() {
            let _ = inb(0x60); // ACK
            if wait_read() {
                let dev_id = inb(0x60);
                if dev_id == 3 {
                    HAS_WHEEL = true;
                }
            }
        }

        // 6. Fare veri akışını başlat (Data Reporting)
        mouse_write(0xF4);

        // 7. Çıkış arabelleğini tekrar temizle
        while inb(0x64) & 0x01 != 0 {
            let _ = inb(0x60);
        }
    }
}

pub fn poll_input(mouse: &mut MouseState, keyboard: &mut KeyboardState, screen_w: i32, screen_h: i32) -> bool {
    let mut updated = false;

    unsafe {
        while inb(0x64) & 0x01 != 0 {
            let status = inb(0x64);
            let data = inb(0x60);

            if status & 0x20 != 0 {
                // PS/2 Fare Baytı
                match MOUSE_CYCLE {
                    0 => {
                        if data & 0x08 != 0 {
                            MOUSE_BYTE[0] = data;
                            MOUSE_CYCLE = 1;
                        }
                    }
                    1 => {
                        MOUSE_BYTE[1] = data;
                        MOUSE_CYCLE = 2;
                    }
                    2 => {
                        MOUSE_BYTE[2] = data;
                        if HAS_WHEEL {
                            MOUSE_CYCLE = 3;
                        } else {
                            MOUSE_CYCLE = 0;
                            let flags = MOUSE_BYTE[0];
                            let raw_x = MOUSE_BYTE[1] as i16;
                            let raw_y = MOUSE_BYTE[2] as i16;
                            let dx = if flags & 0x10 != 0 { (raw_x | !0xFF) as i8 } else { raw_x as i8 };
                            let dy = if flags & 0x20 != 0 { (raw_y | !0xFF) as i8 } else { raw_y as i8 };
                            if flags & 0xC0 == 0 {
                                mouse.update_packet(flags, dx, dy, screen_w, screen_h);
                                updated = true;
                            }
                        }
                    }
                    3 => {
                        MOUSE_BYTE[3] = data;
                        MOUSE_CYCLE = 0;

                        let flags = MOUSE_BYTE[0];
                        let raw_x = MOUSE_BYTE[1] as i16;
                        let raw_y = MOUSE_BYTE[2] as i16;
                        let scroll = MOUSE_BYTE[3] as i8;

                        let dx = if flags & 0x10 != 0 { (raw_x | !0xFF) as i8 } else { raw_x as i8 };
                        let dy = if flags & 0x20 != 0 { (raw_y | !0xFF) as i8 } else { raw_y as i8 };

                        if flags & 0xC0 == 0 {
                            mouse.update_packet(flags, dx, dy, screen_w, screen_h);
                            mouse.scroll_delta = scroll;
                            updated = true;
                        }
                    }
                    _ => {
                        MOUSE_CYCLE = 0;
                    }
                }
            } else {
                // PS/2 Klavye Tarama Kodu (Scancode)
                keyboard.handle_scancode(data);
                updated = true;
            }
        }
    }

    updated
}
