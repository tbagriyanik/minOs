//! minOs web tarayıcısı — çekirdek (no_std) kütüphanesi.
//!
//! Bu kutuphane `eframe`/`egui`'ye bagimli degildir. HTML ayristirma,
//! gezinme ve sayfa durumu yonetimi `alloc` uzerinden calisir. Cizim
//! islemleri `Painter` trait'i uzerinden yapilir; çekirdek bu trait'i
//! `Framebuffer` ile gerceklestirir.

#![no_std]

extern crate alloc;

pub mod app;
pub mod color;
pub mod css;
pub mod html;
pub mod imgdecode;
pub mod types;

pub use app::{HostBrowser, Painter};
pub use types::{
    BrowserLine, Bookmark, HtmlTagType, TextAlign, TextSpan,
};

