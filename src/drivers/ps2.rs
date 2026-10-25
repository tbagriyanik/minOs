use crate::drivers::keyboard::KeyboardState;
use crate::drivers::mouse::MouseState;

#[derive(Copy, Clone, Default)]
pub struct InputUpdate {
    pub mouse: bool,
    pub keyboard: bool,
}

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

#[inline(always)]
fn io_wait() {
    unsafe {
        outb(0x80, 0);
    }
}

fn wait_write() -> bool {
    for _ in 0..10_000 {
        if unsafe { inb(0x64) } & 0x02 == 0 {
            return true;
        }
        io_wait();
    }
    false
}

fn wait_read() -> bool {
    for _ in 0..10_000 {
        if unsafe { inb(0x64) } & 0x01 != 0 {
            return true;
        }
        io_wait();
    }
    false
}

static mut MOUSE_CYCLE: u8 = 0;
static mut MOUSE_BYTE: [u8; 4] = [0; 4];
static mut HAS_WHEEL: bool = false;
// 8042 denetleyicisinin varlığı. QEMU `-machine ...,i8042=off` gibi
// yapılandırmalarda 0x64 portu boşta 0xFF okur; bu durumda yoklama
// döngüsü sonsuza kadar dönmemelidir.
static mut PRESENT: Option<bool> = None;
// Tanılama: yoklamada görülen son ham (status, data) çifti ve sayaç.
static mut TRACE_STATUS: u8 = 0;
static mut TRACE_DATA: u8 = 0;
static mut TRACE_COUNT: u64 = 0;

pub fn trace_last() -> (u8, u8, u64) {
    unsafe { (TRACE_STATUS, TRACE_DATA, TRACE_COUNT) }
}

/// 8042 denetleyicisi mevcut mu? Sonuç ilk çağrıda ölçülüp önbelleğe alınır.
///
/// Denetleyici yoksa (ör. QEMU `i8042=off`), 0x64 durum portu 0xFF döndürür.
/// Bu değer "çıkış arabelleği dolu" bitini (0x01) sürekli 1 yaptığı için
/// `poll_input` sonsuz döngüye girer; bu yüzden önce varlık denetlenir.
pub fn present() -> bool {
    unsafe {
        if let Some(present) = PRESENT {
            return present;
        }
        let present = inb(0x64) != 0xFF;
        PRESENT = Some(present);
        present
    }
}

pub fn mouse_wheel_enabled() -> bool {
    unsafe { HAS_WHEEL }
}

fn mouse_write(cmd: u8) {
    if !wait_write() {
        return;
    }
    unsafe {
        outb(0x64, 0xD4);
    }
    if !wait_write() {
        return;
    }
    unsafe {
        outb(0x60, cmd);
    }
    if wait_read() {
        unsafe {
            let _ = inb(0x60);
        }
    }
}

pub fn init() {
    if !present() {
        crate::serial::print("[PS/2] 8042 denetleyicisi bulunamadi; PS/2 girdisi devre disi.\n");
        return;
    }

    unsafe {
        // 1. Çıkış arabelleğinde kalan artık baytları temizle (sınırlı döngü)
        let mut flushes = 0;
        while (inb(0x64) & 0x01 != 0) && flushes < 256 {
            let _ = inb(0x60);
            flushes += 1;
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
            // IRQ1/IRQ12 ÜRETME: bu sürücü saf yoklamalıdır (polling) ve IDT'de
            // klavye/fare kesme kapısı yoktur. Bitleri açmak, IOAPIC'li
            // makinelerde (VirtualBox SMP) vektörsüz kesme → #GP → donma
            // demektir; PIC maskesi IOAPIC yolunu kapatmaz.
            status &= !0x03; // IRQ1 ve IRQ12 bitlerini kapat
            status &= !0x30; // Saat hatlarını aç
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
            if wait_read() {
                let _ = inb(0x60);
            }
        }

        // 5. Fareyi varsayılan ayarlara al
        mouse_write(0xF6);

        // Laptop Touchpad / Standart Fare Çözünürlük ve Örnekleme Ayarı
        // 0xE8 0x03 -> Çözünürlük: 8 count/mm (Touchpad'lerde akıcı hassasiyet için)
        mouse_write(0xE8);
        mouse_write(0x03);

        // IntelliMouse (Scroll Wheel) etkinleştirme sihirli dizisi (Sample Rate: 200 -> 100 -> 80)
        mouse_write(0xF3);
        mouse_write(200);
        mouse_write(0xF3);
        mouse_write(100);
        mouse_write(0xF3);
        mouse_write(80);

        // Fare kimliğini (Device ID) kontrol et (0x03 / 0x04 ise scroll wheel destekli)
        flushes = 0;
        while (inb(0x64) & 0x01 != 0) && flushes < 256 {
            let _ = inb(0x60);
            flushes += 1;
        }
        wait_write();
        outb(0x64, 0xD4);
        wait_write();
        outb(0x60, 0xF2);
        if wait_read() {
            let _ = inb(0x60); // ACK
            if wait_read() {
                let dev_id = inb(0x60);
                if dev_id == 3 || dev_id == 4 {
                    HAS_WHEEL = true;
                }
            }
        }

        // Örnekleme hızı: 100 örnek/saniye (Laptop touchpad ve fare için kararlı akış)
        mouse_write(0xF3);
        mouse_write(100);

        // Akış Modunu Etkinleştir (Set Stream Mode - 0xEA)
        mouse_write(0xEA);

        // 6. Fare veri akışını başlat (Data Reporting - 0xF4)
        mouse_write(0xF4);

        // 7. Çıkış arabelleğini tekrar temizle
        flushes = 0;
        while (inb(0x64) & 0x01 != 0) && flushes < 256 {
            let _ = inb(0x60);
            flushes += 1;
        }
    }
}

pub fn poll_input(
    mouse: &mut MouseState,
    keyboard: &mut KeyboardState,
    screen_w: i32,
    screen_h: i32,
) -> InputUpdate {
    let mut updated = InputUpdate::default();

    if !present() {
        return updated;
    }

    unsafe {
        loop {
            if inb(0x64) & 0x01 == 0 {
                // Çok baytlı fare/touchpad paketinin ortasındaysak (~30-50µs aralıkla gelen)
                // kalan baytları makul bir süre bekle. Real hardware touchpad'ler için
                // 3_000 spin döngüsü bekleyelim, eğer hala gelmediyse MOUSE_CYCLE'ı
                // HEMEN SIFIRLAMA; bir sonraki frame/poll çağrısında devam edebilsin.
                // Sadece bayt 0 bit 3 bozulursa veya 0xC0 overflow varsa framing yeniden senkronize edilir.
                if MOUSE_CYCLE != 0 {
                    let mut arrived = false;
                    for _ in 0..3_000 {
                        core::hint::spin_loop();
                        if inb(0x64) & 0x01 != 0 {
                            arrived = true;
                            break;
                        }
                    }
                    if !arrived {
                        // Kalan bayt henüz gelmedi, sonraki poll_input çağrısında gelmesi için
                        // döngüyü koru ve çık.
                        break;
                    }
                } else {
                    break;
                }
            }

            let status = inb(0x64);
            let data = inb(0x60);
            TRACE_STATUS = status;
            TRACE_DATA = data;
            TRACE_COUNT = TRACE_COUNT.wrapping_add(1);

            if status & 0x20 != 0 {
                // PS/2 Fare / Touchpad / Emüle USB Fare Baytı
                match MOUSE_CYCLE {
                    0 => {
                        // PS/2 standart fare ve touchpad paketlerinde 1. baytın 3. biti daima 1'dir.
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
                            let dx = if flags & 0x10 != 0 {
                                (raw_x | !0xFF) as i8
                            } else {
                                raw_x as i8
                            };
                            let dy = if flags & 0x20 != 0 {
                                (raw_y | !0xFF) as i8
                            } else {
                                raw_y as i8
                            };
                            // Overflow bitleri (0x40, 0x80) kontrolü
                            if flags & 0xC0 == 0 {
                                mouse.update_packet(flags, dx, dy, screen_w, screen_h);
                                updated.mouse = true;
                            }
                        }
                    }
                    3 => {
                        MOUSE_BYTE[3] = data;
                        MOUSE_CYCLE = 0;

                        let flags = MOUSE_BYTE[0];
                        let raw_x = MOUSE_BYTE[1] as i16;
                        let raw_y = MOUSE_BYTE[2] as i16;
                        // Tekerlek verisi standart IntelliMouse paketinde alt 4 bittir
                        // (işaretli). Bazı denetleyiciler tam bayt gönderir; 4 bitlik
                        // işaret genişletme her ikisini de doğru yorumlar.
                        let mut wheel = (MOUSE_BYTE[3] & 0x0F) as i8;
                        if wheel & 0x08 != 0 {
                            wheel -= 0x10;
                        }

                        let dx = if flags & 0x10 != 0 {
                            (raw_x | !0xFF) as i8
                        } else {
                            raw_x as i8
                        };
                        let dy = if flags & 0x20 != 0 {
                            (raw_y | !0xFF) as i8
                        } else {
                            raw_y as i8
                        };

                        if flags & 0xC0 == 0 {
                            mouse.update_packet(flags, dx, dy, screen_w, screen_h);
                            // Aynı karede birden çok paket gelirse biriktir.
                            mouse.scroll_delta = mouse.scroll_delta.saturating_sub(wheel);
                            updated.mouse = true;
                        }
                    }
                    _ => {
                        MOUSE_CYCLE = 0;
                    }
                }
            } else {
                // PS/2 Klavye Tarama Kodu (Scancode)
                keyboard.handle_scancode(data);
                updated.keyboard = true;
            }
        }
    }

    updated
}

/// PS/2 denetleyicisinde okunmayı bekleyen veri olup olmadığını bildirir (port 0x64 status bit 0).
#[inline(always)]
pub fn has_input() -> bool {
    present() && unsafe { inb(0x64) & 0x01 != 0 }
}

/// Tanılama için 0x64 durum yazmacının ham değeri.
#[inline(always)]
pub fn status_byte() -> u8 {
    unsafe { inb(0x64) }
}
