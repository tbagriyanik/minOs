//! Helper macros for serial output

#[macro_export]
macro_rules! video_info {
    ($width:expr, $height:expr, $bpp:expr) => {{
        // Print video mode information in a single compact macro.
        // Expands to the original series of serial_print / serial_print_dec calls.
        serial_print("[VIDEO] ");
        serial_print_dec($width as u64);
        serial_print("x");
        serial_print_dec($height as u64);
        serial_print(" @ ");
        serial_print_dec($bpp as u64);
        serial_print(" bpp.\n");
    }};
}

