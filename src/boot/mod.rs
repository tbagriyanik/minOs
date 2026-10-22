//! Onyukleme (boot) katmani: Limine protokolu, seri konsol, zaman tabani, CPU bilgisi,
//! ilerleme gostergesi ve giris aygiti global'leri.
//!
//! Bu moduller `src/main.rs` icinden ayristirildi (Asama 1 refactor).

pub mod console;
pub mod text_console;
pub mod cpu;
pub mod devices;
pub mod limine;
pub mod progress;
pub mod subsystems;
pub mod time;
