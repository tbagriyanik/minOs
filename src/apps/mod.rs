pub mod browser;
pub mod calc_core;
pub mod calculator;
pub mod controlpanel;
pub mod filemanager;
pub mod fps_game;
pub mod fps_maze;
pub mod game;
pub mod game_core;
pub mod guard;
pub mod imageviewer;
pub mod imgdecode;
pub mod installer;
pub mod js;
pub mod musicplayer;
pub mod notepad;
pub mod paint;
pub mod pkg_deps;
pub mod pkgmgr;
pub mod registry;
pub mod settings;
pub mod taskmgr;
pub mod terminal;
pub mod videoplayer;
pub mod viewer3d;

use crate::drivers::video::Framebuffer;

/// Ortak uygulama arayüzü.
/// Yerleşik uygulamalar farklı imzalara sahip olsa da bu trait,
/// kayıt defteri ve eklenti/hot-load senaryoları için birleşik bir sözleşme sağlar.
pub trait Application {
    /// Uygulamanın görünen adı.
    fn name(&self) -> &str;

    /// Uygulama başlatıldığında/odaklandığında çağrılabilir.
    fn on_start(&mut self) {}

    /// Periyodik güncelleme/tick.
    fn tick(&mut self) {}

    /// İçerik alanını çiz.
    /// Varsayılan uygulama: boş (uygulama özel render yolu kullanılabilir).
    fn render(&mut self, _fb: &mut Framebuffer, _x: usize, _y: usize, _w: usize, _h: usize) {}

    /// Uygulama kapatılırken çağrılabilir.
    fn on_shutdown(&mut self) {}
}
