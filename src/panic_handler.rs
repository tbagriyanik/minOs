//! Cekirdek panik isleyicisi: seri porta ve dogrudan framebuffer'a yazar.

use core::fmt::Write as FmtWrite;
use core::panic::PanicInfo;
use crate::boot::console::serial_print;
use crate::{drivers, gui, PANIC_FB_ADDR, PANIC_FB_W, PANIC_FB_H, PANIC_FB_PITCH, PANIC_FB_BPP};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    struct PanicSerial;
    impl FmtWrite for PanicSerial {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            serial_print(text);
            Ok(())
        }
    }

    serial_print("\n==================== [ KERNEL PANIC ] ====================\n");
    serial_print("[PANIC] Mesaj: ");
    let mut output = PanicSerial;
    let _ = writeln!(&mut output, "{}", info.message());
    if let Some(location) = info.location() {
        serial_print("[PANIC] Konum: ");
        let _ = writeln!(
            &mut output,
            "{}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        );
    }
    serial_print("==========================================================\n");

    // Ekrana doğrudan siyah zemin üzerine turuncu hata ekranı çiz
    unsafe {
        if !PANIC_FB_ADDR.is_null() && PANIC_FB_W > 0 && PANIC_FB_H > 0 {
            let mut pfb = drivers::video::Framebuffer::new(
                PANIC_FB_ADDR,
                PANIC_FB_W,
                PANIC_FB_H,
                PANIC_FB_PITCH,
                PANIC_FB_BPP,
            );

            // Siyah arka plan (0x000000)
            pfb.draw_rect(0, 0, PANIC_FB_W, PANIC_FB_H, 0x000000);

            // Turuncu renk tonları:
            // Parlak Turuncu: 0xFFA500, Koyu Turuncu: 0xFF8C00, Açık Turuncu: 0xFFB347
            const ORANGE_TITLE: u32 = 0xFFA500;
            const ORANGE_TEXT: u32 = 0xFF8C00;
            const ORANGE_MUTED: u32 = 0xCC7000;

            let center_x = 40;
            let mut cur_y = 50;

            // 1. Başlık
            gui::MinGuiCompositor::draw_string_scaled(
                &mut pfb,
                center_x,
                cur_y,
                "!!! minOs KERNEL PANIC !!!",
                ORANGE_TITLE,
                2,
            );
            cur_y += 50;

            // 2. Çizgi
            pfb.draw_rect(center_x, cur_y, PANIC_FB_W.saturating_sub(80), 2, ORANGE_MUTED);
            cur_y += 30;

            // 3. Hata Açıklaması
            gui::MinGuiCompositor::draw_string(
                &mut pfb,
                center_x,
                cur_y,
                "Çekirdek çalışırken kritik bir hata ile karşılaştı ve durduruldu.",
                ORANGE_TEXT,
            );
            cur_y += 24;

            // 4. Hata Mesajı
            let mut msg_buf = gui::TextBuf::new();
            msg_buf.push_str("Hata: ");
            struct StrBufWriter<'a>(&'a mut gui::TextBuf);
            impl<'a> FmtWrite for StrBufWriter<'a> {
                fn write_str(&mut self, s: &str) -> core::fmt::Result {
                    self.0.push_str(s);
                    Ok(())
                }
            }
            let _ = write!(&mut StrBufWriter(&mut msg_buf), "{}", info.message());
            gui::MinGuiCompositor::draw_string(&mut pfb, center_x, cur_y, msg_buf.as_str(), 0xFFFFFF);
            cur_y += 26;

            // 5. Konum
            if let Some(loc) = info.location() {
                let mut loc_buf = gui::TextBuf::new();
                loc_buf.push_str("Konum: ");
                loc_buf.push_str(loc.file());
                loc_buf.push_byte(b':');
                loc_buf.push_usize(loc.line() as usize);
                loc_buf.push_byte(b':');
                loc_buf.push_usize(loc.column() as usize);
                gui::MinGuiCompositor::draw_string(&mut pfb, center_x, cur_y, loc_buf.as_str(), ORANGE_MUTED);
                cur_y += 36;
            } else {
                cur_y += 10;
            }

            // 6. Bilgilendirme ve Yeniden Başlatma Talimatı
            pfb.draw_rect(center_x, cur_y, PANIC_FB_W.saturating_sub(80), 1, ORANGE_MUTED);
            cur_y += 24;

            gui::MinGuiCompositor::draw_string(
                &mut pfb,
                center_x,
                cur_y,
                "Yeniden başlatmak (Restart) için herhangi bir tuşa [ENTER / ESC / SPACE] basınız.",
                ORANGE_TITLE,
            );
            cur_y += 24;
            gui::MinGuiCompositor::draw_string(
                &mut pfb,
                center_x,
                cur_y,
                "Press any key [ENTER / ESC / SPACE] to reboot the system.",
                ORANGE_MUTED,
            );

            // Ekrana blit et
            pfb.flush();
        }
    }

    // Tuş bekleme ve Yeniden Başlatma (Restart) Döngüsü
    loop {
        // PS/2 klavye kontrolcüsünde (port 0x64 status bit 0) veri var mı?
        let has_key = unsafe {
            let status: u8;
            core::arch::asm!("in al, 0x64", out("al") status, options(nomem, nostack, preserves_flags));
            status & 0x01 != 0
        };

        if has_key {
            let scancode: u8;
            unsafe {
                core::arch::asm!("in al, 0x60", out("al") scancode, options(nomem, nostack, preserves_flags));
            }
            // Tuş basıldı (Make code < 0x80)
            if scancode < 0x80 {
                serial_print("[PANIC] Tusa basildi. Sistem yeniden baslatiliyor (Reboot)...\n");

                // 1. Yöntem: Klavye kontrolcüsü üzerinden reset (0x64 portuna 0xFE gönder)
                unsafe {
                    core::arch::asm!("out 0x64, al", in("al") 0xFEu8, options(nomem, nostack, preserves_flags));
                }

                // 2. Yöntem: ACPI / Bochs / QEMU reset portları (0xCF9 veya 0x604)
                unsafe {
                    core::arch::asm!("out dx, al", in("dx") 0xCF9u16, in("al") 0x06u8, options(nomem, nostack, preserves_flags));
                }

                // 3. Yöntem: Triple Fault (Boş IDT yükleyip yazılımsal kesme tetikle)
                #[repr(C, packed)]
                struct Idtr {
                    limit: u16,
                    base: u64,
                }
                let null_idtr = Idtr { limit: 0, base: 0 };
                unsafe {
                    core::arch::asm!("lidt [{}]", in(reg) &null_idtr, options(nomem, nostack, preserves_flags));
                    core::arch::asm!("int3", options(nomem, nostack, preserves_flags));
                }
            }
        }

        core::hint::spin_loop();
    }
}
