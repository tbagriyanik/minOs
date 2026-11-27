//! Önizleme aracı için çekirdek oyun modüllerinin bağlanması.
//!
//! Yol, `preview_apps_before.rs` varyantı ile HEAD sürümüne yönlendirilebilir.

#[path = "../src/apps/gfx3d.rs"]
pub mod gfx3d;

#[path = "../src/apps/fps_enemy_model.rs"]
pub mod fps_enemy_model;

#[path = "../src/apps/fps_game.rs"]
pub mod fps_game;
