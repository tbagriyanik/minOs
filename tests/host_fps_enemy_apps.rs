//! Host testleri için `apps` modül ağacı.
//!
//! Çekirdekte `crate::apps::gfx3d` ve `crate::apps::fps_enemy_model`
//! yollarıyla erişilen modüller burada aynı isimle sunulur.

#[path = "../src/apps/gfx3d.rs"]
pub mod gfx3d;

#[path = "../src/apps/fps_enemy_model.rs"]
pub mod fps_enemy_model;
