//! Çekirdek yardımcı modülleri: temel tipler, tanılama ve biçim ayrıştırıcıları.
//! `main.rs` içinden ayrıştırılmış modüler çekirdek katmanı.

pub mod diag;
pub mod formats;
pub mod heap;
pub mod primitives;

// Geriye dönük uyumluluk için yeniden dışa aktarımlar.
pub use diag as kernel_diag;
pub use formats as kernel_formats;
pub use primitives as kernel_primitives;
