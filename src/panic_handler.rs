//! Cekirdek panik isleyicisi: seri porta ve dogrudan framebuffer'a yazar.

use crate::boot::console::serial_print;
use crate::boot::text_console;
use core::fmt::Write as FmtWrite;
use core::panic::PanicInfo;

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
    unsafe {
        text_console::show_boot_log();
    }
    crate::boot::text_console::write("\n\n[FATAL] KERNEL PANIC\n");
    struct PanicText;
    impl FmtWrite for PanicText {
        fn write_str(&mut self, text: &str) -> core::fmt::Result {
            crate::boot::text_console::write(text);
            Ok(())
        }
    }
    let _ = writeln!(&mut PanicText, "{}", info.message());
    if let Some(location) = info.location() {
        let _ = writeln!(&mut PanicText, "{}:{}", location.file(), location.line());
    }
    crate::boot::text_console::write(
        "Grafik arayüz başlatılamadı. Sistem güvenli şekilde durduruldu.\n",
    );

    crate::boot::text_console::interactive_error_prompt("Kernel Panic");
}
