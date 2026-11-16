use crate::drivers::input_queue::{InputEvent, INPUT_QUEUE};
use crate::drivers::keyboard::KeyboardState;
use crate::drivers::mouse::MouseState;

#[derive(Copy, Clone, Default)]
pub struct InputUpdate {
    pub mouse: bool,
    pub keyboard: bool,
    pub touch: bool,
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

        // Örnekleme hızı: 200 örnek/saniye (daha akıcı fare hareketi)
        mouse_write(0xF3);
        mouse_write(200);

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

    init_irq_mode();
}

pub fn poll_input(
    mouse: &mut MouseState,
    keyboard: &mut KeyboardState,
    screen_w: i32,
    screen_h: i32,
) -> InputUpdate {
    let mut updated = InputUpdate::default();

    if !present() || irq_mode() {
        // Kesme modu etkinken baytlar IRQ işleyicisi tarafından tüketilir;
        // portu bir daha okumak aynı baytı iki kez işlemeye yol açar.
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

// ---------------------------------------------------------------------------
// Kesme (IRQ) modu: IOAPIC üzerinden IRQ1 (klavye) ve IRQ12 (fare)
// ---------------------------------------------------------------------------
//
// 8259 PIC `disable_pic()` ile tümüyle maskelidir; ISA IRQ hatları IOAPIC
// üzerinden Local APIC'e yönlendirilir. Baytlar IRQ işleyicisinde
// `INPUT_QUEUE`'ya normalize edilmiş olay olarak bırakılır; ana döngü
// `drain_input_events` ile kuyruğu boşaltıp `MouseState`/`KeyboardState`'i
// günceller. Kuyruk yaklaşımı sayesinde işleyici yalnızca port okur ve
// olay iter; durum makinesi ve hız ölçekleme çekirdek bağlamında kalır.

/// Klavye IRQ1'in yönlendirildiği IDT vektörü.
pub const IRQ_KBD_VECTOR: u8 = 0x30;
/// Fare IRQ12'nin yönlendirildiği IDT vektörü.
pub const IRQ_MOUSE_VECTOR: u8 = 0x31;

static mut IRQ_MODE: bool = false;

/// PS/2 girdisinin kesme modunda çalışıp çalışmadığını bildirir.
pub fn irq_mode() -> bool {
    unsafe { IRQ_MODE }
}

/// 8042 komut yazmacından IRQ1/IRQ12 üretim bitlerini (0-1) açar.
/// `init()` bu bitleri bilerek kapatmıştı; yalnızca IOAPIC yönlendirmesi
/// başarıyla kurulduğunda tekrar açılır.
fn enable_8042_irq_output() {
    unsafe {
        if !wait_write() {
            return;
        }
        outb(0x64, 0x20);
        if !wait_read() {
            return;
        }
        let mut status = inb(0x60);
        status |= 0x03; // IRQ1 (klavye) + IRQ12 (yardımcı/fare) hatlarını aç
        if !wait_write() {
            return;
        }
        outb(0x64, 0x60);
        if !wait_write() {
            return;
        }
        outb(0x60, status);
    }
}

/// IOAPIC keşfi + yönlendirme + IDT kapıları. `init()` sonunda çağrılır;
/// IOAPIC bulunamazsa yoklamalı mod korunur.
fn init_irq_mode() {
    if !crate::arch::x86_64::ioapic::discover() {
        return;
    }

    // IRQ1 → GSI (ISO varsa) → vektör 0x30; IRQ12 → vektör 0x31.
    // Hedef: BSP'nin LAPIC kimliği (IOAPIC yönlendirmesi tek CPU'ya gider).
    let dest = crate::arch::x86_64::smp::lapic_id_of(0);
    let mut routed = 0;

    for (irq, vector) in [(1u8, IRQ_KBD_VECTOR), (12u8, IRQ_MOUSE_VECTOR)] {
        if let Some((gsi, flags)) = crate::arch::x86_64::ioapic::gsi_for_irq(irq) {
            // ISO bit1 = aktif düşük polarite. PS/2 aygıtları her zaman
            // kenar tetikli olduğundan seviye modu kullanılmaz.
            let active_low = flags & 0x02 != 0;
            if crate::arch::x86_64::ioapic::route_gsi(gsi, vector, active_low, false, dest) {
                routed += 1;
            }
        }
    }

    if routed == 0 {
        crate::serial_print("[PS/2] IOAPIC yönlendirmesi kurulamadı; yoklama modu sürüyor.\n");
        return;
    }

    unsafe {
        let idt = &mut *core::ptr::addr_of_mut!(crate::arch::x86_64::IDT);
        idt.entries[IRQ_KBD_VECTOR as usize].set_handler(ps2_kbd_irq_entry as *const () as usize);
        idt.entries[IRQ_MOUSE_VECTOR as usize]
            .set_handler(ps2_mouse_irq_entry as *const () as usize);
    }

    enable_8042_irq_output();
    unsafe { IRQ_MODE = true };
    crate::serial_print("[PS/2] Kesme modu etkin (IRQ1→0x30, IRQ12→0x31, IOAPIC).\n");
}

/// IRQ işleyicisinin ortak gövdesi: çıkış arabelleğindeki baytları sınırlı
/// sayıda okuyup klavye/fare ayrımını 0x64 bit 5 üzerinden yapar.
/// Her tamamlanan fare paketi ve her klavye baytı `INPUT_QUEUE`'ya itilir.
///
/// # Safety
/// Yalnızca donanım kesmesi bağlamında çağrılmalıdır.
unsafe fn handle_irq_bytes() {
    // Tek kesmede birden çok bayt birikebilir; 64 ile sınırla.
    for _ in 0..64 {
        let status = inb(0x64);
        if status & 0x01 == 0 {
            break;
        }
        let data = inb(0x60);
        if status & 0x20 != 0 {
            handle_mouse_byte_irq(data);
        } else {
            // Ham tarama kodu: 0x80 kırılım biti ve 0xE0 öneki dahil.
            // `KeyboardState::handle_scancode` bunların hepsini çözer.
            let _ = INPUT_QUEUE.push(InputEvent::KeyPress(data));
        }
    }
}

/// Fare paket baytını IRQ bağlamında işler; tamamlanan paketleri
/// `INPUT_QUEUE`'ya `MouseMove`/`MouseWheel` olarak bırakır.
///
/// # Safety
/// Yalnızca kesme bağlamında çağrılmalıdır.
unsafe fn handle_mouse_byte_irq(data: u8) {
    match MOUSE_CYCLE {
        0 => {
            // PS/2 paket ilk baytının 3. biti daima 1'dir.
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
                emit_mouse_packet_irq(false);
            }
        }
        3 => {
            MOUSE_BYTE[3] = data;
            MOUSE_CYCLE = 0;
            emit_mouse_packet_irq(true);
        }
        _ => {
            MOUSE_CYCLE = 0;
        }
    }
}

/// Tamamlanan fare paketini kuyruk olayına çevirir.
/// `has_wheel`: paketin 4. baytı (tekerlek) içerip içermediği.
///
/// # Safety
/// Yalnızca kesme bağlamında çağrılmalıdır.
unsafe fn emit_mouse_packet_irq(has_wheel: bool) {
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

    // Taşma bitleri (0x40, 0x80) bozuksa paketi at.
    if flags & 0xC0 != 0 {
        return;
    }

    // Tekerlek verisi standart IntelliMouse paketinde alt 4 bittir
    // (işaretli). Yoklama yoluyla aynı yorum.
    if has_wheel {
        let mut wheel = (MOUSE_BYTE[3] & 0x0F) as i8;
        if wheel & 0x08 != 0 {
            wheel -= 0x10;
        }
        if wheel != 0 {
            let _ = INPUT_QUEUE.push(InputEvent::MouseWheel(wheel));
        }
    }

    // Fare tuşları: bit0=sol, bit1=sağ, bit2=orta (InputEvent düzeniyle aynı).
    let _ = INPUT_QUEUE.push(InputEvent::MouseMove {
        dx,
        dy,
        buttons: flags & 0x07,
    });
}

/// Klavye IRQ1 Rust işleyicisi: baytları kuyruğa bırakır, EOI gönderir.
///
/// # Safety
/// `ps2_kbd_irq_entry` tarafından kesme bağlamında çağrılır.
#[no_mangle]
pub unsafe extern "C" fn ps2_kbd_irq_rust() {
    unsafe {
        handle_irq_bytes();
        crate::arch::x86_64::ioapic::eoi(IRQ_KBD_VECTOR);
    }
}

/// Fare IRQ12 Rust işleyicisi: baytları kuyruğa bırakır, EOI gönderir.
///
/// # Safety
/// `ps2_mouse_irq_entry` tarafından kesme bağlamında çağrılır.
#[no_mangle]
pub unsafe extern "C" fn ps2_mouse_irq_rust() {
    unsafe {
        handle_irq_bytes();
        crate::arch::x86_64::ioapic::eoi(IRQ_MOUSE_VECTOR);
    }
}

macro_rules! ps2_irq_entry {
    ($name:ident, $handler:ident) => {
        /// Naked kesme kapısı: tüm yazmaçları + FPU durumunu korur,
        /// Rust işleyicisini çağırır ve `iretq` ile döner.
        ///
        /// # Safety
        /// Donanım kesmesi tarafından IDT üzerinden çağrılır; Rust
        /// kodundan doğrudan çağrılmamalıdır.
        #[no_mangle]
        #[unsafe(naked)]
        pub unsafe extern "C" fn $name() {
            core::arch::naked_asm!(
                "push r15",
                "push r14",
                "push r13",
                "push r12",
                "push r11",
                "push r10",
                "push r9",
                "push r8",
                "push rbp",
                "push rdi",
                "push rsi",
                "push rdx",
                "push rcx",
                "push rbx",
                "push rax",
                "mov r12, rsp",
                "and rsp, -16",
                "sub rsp, 512",
                "fxsave64 [rsp]",
                "mov rdi, r12",
                "call {handler}",
                "fxrstor64 [rsp]",
                "mov rsp, r12",
                "pop rax",
                "pop rbx",
                "pop rcx",
                "pop rdx",
                "pop rsi",
                "pop rdi",
                "pop rbp",
                "pop r8",
                "pop r9",
                "pop r10",
                "pop r11",
                "pop r12",
                "pop r13",
                "pop r14",
                "pop r15",
                "iretq",
                handler = sym $handler,
                options(),
            );
        }
    };
}

ps2_irq_entry!(ps2_kbd_irq_entry, ps2_kbd_irq_rust);
ps2_irq_entry!(ps2_mouse_irq_entry, ps2_mouse_irq_rust);

/// `INPUT_QUEUE`'daki birikmiş PS/2 olaylarını `MouseState`/`KeyboardState`'e
/// uygular. Olay yoksa `InputUpdate::default()` döner.
///
/// Klavye olayları `KeyboardState::handle_scancode`'a ham bayt olarak
/// verilir (kırılım biti ve 0xE0 öneki dahil); fare olayları
/// `update_packet_i32` üzerinden hız ölçeklemesi ve sınırlamadan geçer.
pub fn drain_input_events(
    mouse: &mut MouseState,
    keyboard: &mut KeyboardState,
    screen_w: i32,
    screen_h: i32,
) -> InputUpdate {
    let mut updated = InputUpdate::default();
    while let Some(ev) = INPUT_QUEUE.pop() {
        match ev {
            InputEvent::KeyPress(scancode) => {
                keyboard.handle_scancode(scancode);
                updated.keyboard = true;
            }
            InputEvent::KeyRelease(scancode) => {
                keyboard.handle_scancode(0x80 | scancode);
                updated.keyboard = true;
            }
            InputEvent::MouseMove { dx, dy, buttons } => {
                mouse.update_packet_i32(buttons, dx as i32, dy as i32, screen_w, screen_h);
                updated.mouse = true;
            }
            InputEvent::MouseWheel(delta) => {
                mouse.scroll_delta = mouse.scroll_delta.saturating_sub(delta);
                updated.mouse = true;
            }
            // Dokunmatik/USB olayları kendi yollarından işlenir.
            _ => {}
        }
    }
    updated
}

/// Yalnızca klavye olaylarını boşaltır (metin konsolu kurtarma istemi gibi
/// fare durumuna ihtiyaç duymayan tüketiciler için).
pub fn drain_keyboard_events(keyboard: &mut KeyboardState) {
    while let Some(ev) = INPUT_QUEUE.pop() {
        match ev {
            InputEvent::KeyPress(scancode) => {
                keyboard.handle_scancode(scancode);
            }
            InputEvent::KeyRelease(scancode) => {
                keyboard.handle_scancode(0x80 | scancode);
            }
            _ => {}
        }
    }
}
