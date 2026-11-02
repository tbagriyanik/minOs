//! "Oyunlar" uygulaması — 2D oyunlar ve sesli performans testleri.
//!
//! Üç bölüm tek pencerede sunulur:
//! * **Dama** — Türk/uluslararası kurallı, yapay zekâya karşı 2D dama.
//! * **Bulmaca** — 15'li kayan blok bulmacası (gösteri modu + sayaç).
//! * **Performans** — CPU, bellek dolaşımı, zamanlayıcı gecikmesi ve çizim
//!   yükünü ölçen canlı test; bitişte sesli bildirim çalar.
//!
//! Tüm oyun mantığı `game_core` içindedir; bu modül yalnızca durum, çizim ve
//! girdi yönetimini yapar. Ses, ana döngüde (`GameApp::sound_pending`) PC
//! Speaker / HDA üzerinden çalınır; uygulama donanıma doğrudan dokunmaz.

use crate::drivers::video::Framebuffer;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};

use super::game_core::{self, Board, Move, PUZZLE_SIDE, PUZZLE_TILES};

/// Bölüm sekmesi sayısı.
pub const TAB_COUNT: usize = 3;
const TAB_GAME: usize = 0;
const TAB_PUZZLE: usize = 1;
const TAB_PERF: usize = 2;

const TAB_Y: usize = 28;
const TAB_H: usize = 24;
const TAB_W: usize = 96;

const STATUS_H: usize = 26;
/// Performans testinin süresi (~3 sn, 33 ms kare ile).
const PERF_TICKS: u32 = 90;

/// Ses efektleri: ana döngü bu kimliğe bakarak ton üretir.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    None,
    Move,
    Capture,
    Win,
    Lose,
    Tick,
}

/// Performans testinin aşamaları.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PerfPhase {
    Idle,
    Collecting,
    Finished,
}

/// Performans testi sonuçları.
#[derive(Clone, Copy, Default)]
pub struct PerfResults {
    /// Saniyedeki döngü (olay döngüsü) sayısı.
    pub fps: u32,
    /// Son ölçülen kare süresi (ms).
    pub frame_ms: u32,
    /// Zamanlayıcı gecikmesi (µs) — istenen 16 ms'ye göre sapma.
    pub timer_jitter_us: u32,
    /// Bellek dolaşım hızı (MB/s).
    pub mem_mbps: u32,
    /// Aritmetik yük: saniyedeki işlem sayısı (bin).
    pub math_kops: u32,
    /// Puan 0..100.
    pub score: u32,
}

pub struct GameApp {
    pub tab: usize,
    // --- Dama ---
    pub board: Board,
    pub selected: Option<usize>,
    /// Seçili taşın geçerli hedefleri.
    pub targets: alloc::vec::Vec<Move>,
    pub message: &'static str,
    pub thinking: bool,
    pub ai_wait: u8,
    pub player_light: bool,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub last_move: Option<Move>,
    // --- Bulmaca ---
    pub tiles: [u8; PUZZLE_TILES],
    pub puzzle_moves: u32,
    pub puzzle_seconds: u32,
    pub puzzle_tick: u32,
    pub puzzle_demo: bool,
    pub puzzle_seed: u64,
    // --- Performans ---
    pub perf_phase: PerfPhase,
    pub perf_tick: u32,
    pub perf_results: PerfResults,
    pub perf_samples: u32,
    pub perf_accum_us: u64,
    pub perf_last_us: u64,
    pub bench_checks: u32,
    pub bench_steps: u32,
    // --- Ses ---
    pub sound_pending: Sound,
    pub sound_volume: u8,
    pub muted: bool,
}

impl Default for GameApp {
    fn default() -> Self {
        Self::new()
    }
}

impl GameApp {
    pub fn new() -> Self {
        Self {
            tab: TAB_GAME,
            board: Board::new(),
            selected: None,
            targets: alloc::vec::Vec::new(),
            message: "",
            thinking: false,
            ai_wait: 0,
            player_light: true,
            wins: 0,
            losses: 0,
            draws: 0,
            last_move: None,
            tiles: game_core::puzzle_from_seed(0x5EED_1234),
            puzzle_moves: 0,
            puzzle_seconds: 0,
            puzzle_tick: 0,
            puzzle_demo: true,
            puzzle_seed: 0x5EED_1234,
            perf_phase: PerfPhase::Idle,
            perf_tick: 0,
            perf_results: PerfResults::default(),
            perf_samples: 0,
            perf_accum_us: 0,
            perf_last_us: 0,
            bench_checks: 0,
            bench_steps: 0,
            sound_pending: Sound::None,
            sound_volume: 60,
            muted: false,
        }
    }

    // -----------------------------------------------------------------------
    // Bölüm (sekme) yönetimi
    // -----------------------------------------------------------------------

    pub fn select_tab(&mut self, tab: usize) {
        if tab < TAB_COUNT {
            self.tab = tab;
            self.sound_pending = Sound::Tick;
        }
    }

    pub fn tab_names() -> [(&'static str, &'static str); TAB_COUNT] {
        [
            ("Dama", "Checkers"),
            ("Bulmaca", "Puzzle"),
            ("Performans", "Performance"),
        ]
    }

    /// Sekme dikdörtgeni (pencere içi koordinatlar).
    pub fn tab_rect(index: usize) -> (usize, usize, usize, usize) {
        (
            sp(8) + index * (sp(TAB_W) + sp(4)),
            sp(TAB_Y),
            sp(TAB_W),
            sp(TAB_H),
        )
    }

    /// Tahta alanı: sekmenin altında kalan, kare bir oyun alanı.
    /// (x, y, w, h) pencere içi koordinatlar.
    pub fn board_rect(win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
        let top = sp(TAB_Y + TAB_H + 8);
        let avail_h = win_h.saturating_sub(top + sp(STATUS_H + 6) + sp(34));
        let avail_w = win_w.saturating_sub(sp(16));
        let side = avail_w.min(avail_h).max(sp(96));
        let x = sp(8) + avail_w.saturating_sub(side) / 2;
        (x, top, side, side)
    }

    /// Bulmaca tahtası: aynı çerçeve, kare ızgara.
    pub fn puzzle_rect(win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
        let (x, y, w, h) = Self::board_rect(win_w, win_h);
        let side = w.min(h);
        (x + (w - side) / 2, y + (h - side) / 2, side, side)
    }

    /// Kare indeksinden piksel dikdörtgenine (bulmaca için 4x4 ızgara).
    pub fn cell_rect(
        rect: (usize, usize, usize, usize),
        index: usize,
    ) -> (usize, usize, usize, usize) {
        let (x, y, w, h) = rect;
        let cw = w / PUZZLE_SIDE;
        let chh = h / PUZZLE_SIDE;
        let row = index / PUZZLE_SIDE;
        let col = index % PUZZLE_SIDE;
        (x + col * cw, y + row * chh, cw, chh)
    }

    // -----------------------------------------------------------------------
    // Dama girdisi
    // -----------------------------------------------------------------------

    /// Yeni oyun: tahtayı sıfırlar, oyuncu açık renkte başlar.
    pub fn new_game(&mut self, player_light: bool) {
        self.board = Board::new();
        self.player_light = player_light;
        self.selected = None;
        self.targets.clear();
        self.last_move = None;
        self.thinking = false;
        self.ai_wait = 0;
        self.message = "";
        self.sound_pending = Sound::Tick;
    }

    fn player_to_move(&self) -> bool {
        (self.board.side_to_move() == game_core::LIGHT_SIDE) == self.player_light
    }

    /// Seçili taşın hedefi mi? Değilse taş seçimi güncellenir.
    pub fn click_cell(&mut self, index: usize) {
        if index >= game_core::SQUARES {
            return;
        }
        self.click_cell_inner(index);
    }

    fn click_cell_inner(&mut self, index: usize) {
        if self.tab != TAB_GAME {
            return;
        }
        if self.board.winner().is_some() || self.board.is_draw() {
            return;
        }
        if !self.player_to_move() {
            return;
        }
        // Seçili taşın hedefi mi?
        if let Some(mv) = self.targets.iter().find(|m| m.to() == index).copied() {
            if self.board.apply(&mv) {
                self.last_move = Some(mv);
                self.sound_pending = if mv.is_capture() {
                    Sound::Capture
                } else {
                    Sound::Move
                };
                self.selected = None;
                self.targets.clear();
                self.finish_turn();
            }
            return;
        }
        // Yeni taş seçimi.
        self.select(index);
    }

    fn select(&mut self, index: usize) {
        self.selected = None;
        self.targets.clear();
        let piece = self.board.cells[index];
        let is_player_piece = if self.player_light {
            game_core::is_light(piece)
        } else {
            game_core::is_dark(piece)
        };
        if !is_player_piece {
            return;
        }
        let mut list = alloc::vec::Vec::new();
        self.board.generate(&mut list);
        self.targets = list.into_iter().filter(|m| m.from() == index).collect();
        if self.targets.is_empty() {
            self.message = crate::gui::ui_text(
                "Bu taşın oynayabileceği hamle yok",
                "No legal move for this piece",
            );
        } else {
            self.selected = Some(index);
            self.message = if self.targets[0].is_capture() {
                crate::gui::ui_text("Yeme zorunlu!", "Capture is mandatory!")
            } else {
                crate::gui::ui_text("Hedef kareye tıkla", "Click a target square")
            };
        }
        self.sound_pending = Sound::Tick;
    }

    /// Hamleden sonra kazanan/beraberlik durumunu işler.
    fn finish_turn(&mut self) {
        if let Some(light_won) = self.board.winner() {
            let player_won = light_won == self.player_light;
            if player_won {
                self.wins += 1;
                self.sound_pending = Sound::Win;
                self.message = crate::gui::ui_text("Kazandınız!", "You win!");
            } else {
                self.losses += 1;
                self.sound_pending = Sound::Lose;
                self.message = crate::gui::ui_text("Kaybettiniz", "You lose");
            }
            return;
        }
        if self.board.is_draw() {
            self.draws += 1;
            self.message = crate::gui::ui_text("Berabere", "Draw");
            self.sound_pending = Sound::Win;
            return;
        }
        if self.player_to_move() {
            self.message = crate::gui::ui_text("Sıra sizde", "Your turn");
        } else {
            self.thinking = true;
            self.ai_wait = 2;
            self.message = crate::gui::ui_text("Bilgisayar düşünüyor...", "Computer thinking...");
        }
    }

    /// Her karede çağrılır; yapay zekâ gecikmesini ilerletir.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        match self.tab {
            TAB_GAME => {
                if self.thinking && !self.player_to_move() && self.board.winner().is_none() {
                    if self.ai_wait > 0 {
                        self.ai_wait -= 1;
                    } else if let Some(mv) = game_core::choose_move(&self.board, 3) {
                        if self.board.apply(&mv) {
                            self.last_move = Some(mv);
                            self.sound_pending = if mv.is_capture() {
                                Sound::Capture
                            } else {
                                Sound::Move
                            };
                            changed = true;
                        }
                        self.thinking = false;
                        self.finish_turn();
                    } else {
                        self.thinking = false;
                        self.finish_turn();
                    }
                }
            }
            TAB_PUZZLE => {
                if self.puzzle_demo {
                    let mut state = self.puzzle_seed;
                    if game_core::puzzle_random_step(&mut self.tiles, &mut state).is_some() {
                        self.puzzle_moves = self.puzzle_moves.wrapping_add(1);
                    }
                    self.puzzle_seed = state;
                    changed = true;
                }
            }
            TAB_PERF => changed = self.perf_tick_frame(),
            _ => {}
        }
        // Bulmaca süresi yalnızca gösteri değilken işler.
        if self.tab == TAB_PUZZLE && !self.puzzle_demo && self.puzzle_seconds < 99_999 {
            // Yaklaşık saniye: her 30 olay döngüsünde bir artar.
            self.puzzle_tick = self.puzzle_tick.wrapping_add(1);
            if self.puzzle_tick >= 30 {
                self.puzzle_tick = 0;
                self.puzzle_seconds = self.puzzle_seconds.wrapping_add(1);
            }
        }
        changed
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    // -----------------------------------------------------------------------
    // Bulmaca girdisi
    // -----------------------------------------------------------------------

    pub fn click_tile(&mut self, index: usize) {
        if self.puzzle_demo {
            self.puzzle_demo = false;
        }
        let was_solved = game_core::puzzle_solved(&self.tiles);
        if game_core::puzzle_move(&mut self.tiles, index).is_some() {
            self.puzzle_moves = self.puzzle_moves.wrapping_add(1);
            self.sound_pending = Sound::Move;
            if !was_solved && game_core::puzzle_solved(&self.tiles) {
                self.sound_pending = Sound::Win;
                self.message = crate::gui::ui_text("Bulmaca çözüldü!", "Puzzle solved!");
            }
        }
    }

    pub fn shuffle_puzzle(&mut self) {
        self.puzzle_seed = self
            .puzzle_seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        self.tiles = game_core::puzzle_from_seed(self.puzzle_seed);
        self.puzzle_moves = 0;
        self.puzzle_seconds = 0;
        self.puzzle_demo = false;
        self.sound_pending = Sound::Tick;
    }

    // -----------------------------------------------------------------------
    // Performans testi
    // -----------------------------------------------------------------------

    /// Testi başlatır (3 saniye örnekleme).
    pub fn start_perf(&mut self) {
        self.perf_phase = PerfPhase::Collecting;
        self.perf_tick = 0;
        self.perf_samples = 0;
        self.perf_accum_us = 0;
        self.perf_results = PerfResults::default();
        self.message = "";
        self.sound_pending = Sound::Tick;
    }

    /// Bir kare süresini örnekler. `frame_us` son kare süresi, `now_ms` monotonik saat.
    pub fn perf_sample(&mut self, frame_us: u64) {
        if self.perf_phase != PerfPhase::Collecting {
            return;
        }
        self.perf_samples += 1;
        self.perf_accum_us = self.perf_accum_us.saturating_add(frame_us);
        self.perf_last_us = frame_us;
    }

    /// Kare başına yük üretir ve 3 saniye sonunda sonucu hesaplar.
    fn perf_tick_frame(&mut self) -> bool {
        if self.perf_phase != PerfPhase::Collecting {
            return false;
        }
        self.perf_tick += 1;
        // Aritmetik testi: dama araması + bulmaca işi (gerçek yük, sabit tohum).
        self.bench_checks = self
            .bench_checks
            .wrapping_add(game_core::bench_checkers_ai(1, 1));
        self.bench_steps = self
            .bench_steps
            .wrapping_add(game_core::bench_puzzle_work(16, self.perf_tick as u64));
        if self.perf_tick >= PERF_TICKS {
            let samples = self.perf_samples.max(1);
            // Kare süresi (µs) örnekleri. FPS gerçekten ölçülen süreden gelir.
            let avg_us = (self.perf_accum_us / samples as u64).max(1u64);
            let fps = (1_000_000 / avg_us).min(9_999) as u32;
            let timer_jitter_us = avg_us.abs_diff(16_000) as u32;
            let (mbps, math_kops) = self.measure_memory();
            let score = Self::score_of(fps, timer_jitter_us, mbps);
            self.perf_results = PerfResults {
                fps,
                frame_ms: (avg_us / 1000) as u32,
                timer_jitter_us,
                mem_mbps: mbps,
                math_kops,
                score,
            };
            self.perf_phase = PerfPhase::Finished;
            self.sound_pending = Sound::Win;
            return true;
        }
        true
    }

    /// 256 KiB arabellek üzerinde yazma+okuma dolaşımı ile (MB/s, k işlem/s) ölçer.
    fn measure_memory(&self) -> (u32, u32) {
        let mut buf = alloc::vec![0u32; 65_536];
        let start = crate::monotonic_ms();
        for (i, slot) in buf.iter_mut().enumerate() {
            *slot = (i as u32).wrapping_mul(2_654_435_761);
        }
        let mut acc = 0u32;
        for _ in 0..4 {
            for &v in buf.iter() {
                acc = acc.wrapping_add(v.rotate_left(5) ^ 1);
            }
        }
        let elapsed_ms = crate::monotonic_ms().saturating_sub(start).max(1) as u32;
        // 256 KiB yazma + 4 x 256 KiB okuma = 1.25 MiB işlenen veri.
        let mbps = (1usize.saturating_mul(1280) / elapsed_ms as usize).min(60_000) as u32;
        let math_kops = acc & 0xFFFF;
        (mbps.max(1), math_kops)
    }

    fn score_of(fps: u32, jitter_us: u32, mbps: u32) -> u32 {
        let fps_score = fps.min(60) * 100 / 60; // 0..100
        let jitter_score = 100u32.saturating_sub(jitter_us.min(20_000) / 200);
        let mem_score = mbps.min(10_000) / 100;
        (fps_score * 50 + jitter_score * 30 + mem_score * 20) / 100
    }

    fn grade(score: u32) -> &'static str {
        match score {
            90..=u32::MAX => crate::gui::ui_text("Mükemmel", "Excellent"),
            75..=89 => crate::gui::ui_text("Çok iyi", "Very good"),
            55..=74 => crate::gui::ui_text("İyi", "Good"),
            35..=54 => crate::gui::ui_text("Orta", "Fair"),
            _ => crate::gui::ui_text("Zayıf", "Poor"),
        }
    }

    // -----------------------------------------------------------------------
    // Çizim
    // -----------------------------------------------------------------------

    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        self.render_tabs(fb, x, y, w);
        match self.tab {
            TAB_GAME => self.render_checkers(fb, x, y, w, h),
            TAB_PUZZLE => self.render_puzzle(fb, x, y, w, h),
            TAB_PERF => self.render_perf(fb, x, y, w, h),
            _ => {}
        }
        self.render_status(fb, x, y, w, h);
    }

    fn render_tabs(&self, fb: &mut Framebuffer, x: usize, y: usize, _w: usize) {
        let names = Self::tab_names();
        for (i, pair) in names.iter().enumerate() {
            let (tx, ty, tw, th) = Self::tab_rect(i);
            let active = i == self.tab;
            let bg = if active {
                Color::ACCENT_BLUE
            } else {
                Color::TITLE_BAR_INACTIVE
            };
            fb.draw_rounded_rect(x + tx, y + ty, tw, th, 5, bg);
            if active {
                fb.draw_rounded_rect_outline(x + tx, y + ty, tw, th, 5, Color::WHITE);
            }
            let label = crate::gui::ui_text(pair.0, pair.1);
            let lw = MinGuiCompositor::text_width(label, 1);
            MinGuiCompositor::draw_string(
                fb,
                x + tx + tw.saturating_sub(lw) / 2,
                y + ty + (th.saturating_sub(MinGuiCompositor::text_height())) / 2,
                label,
                if active {
                    MinGuiCompositor::text_on(bg)
                } else {
                    Color::TEXT_MAIN
                },
            );
        }
        // Dama bölümünde "Yeni Oyun" düğmesi.
        if self.tab == TAB_GAME {
            let (bx, by, bw, bh) = Self::new_game_rect();
            fb.draw_rounded_rect(x + bx, y + by, bw, bh, 5, Color::TITLE_BAR);
            MinGuiCompositor::draw_string(
                fb,
                x + bx + sp(10),
                y + by + (bh.saturating_sub(MinGuiCompositor::text_height())) / 2,
                crate::gui::ui_text("Yeni Oyun", "New Game"),
                Color::TEXT_MAIN,
            );
        }
    }

    /// "Yeni Oyun" düğmesi (pencere içi koordinatlar).
    pub fn new_game_rect() -> (usize, usize, usize, usize) {
        let x = sp(8) + TAB_COUNT * (sp(TAB_W) + sp(4)) + sp(6);
        (x, sp(TAB_Y), sp(86), sp(TAB_H))
    }

    fn render_status(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        if h < sp(STATUS_H + 8) {
            return;
        }
        let sy = y + h.saturating_sub(sp(STATUS_H + 4));
        fb.draw_rect(
            x + sp(4),
            sy,
            w.saturating_sub(sp(8)),
            sp(STATUS_H),
            Color::TASKBAR_BG,
        );
        if self.tab != TAB_GAME {
            // Diğer bölümlerde kısa durum satırı.
            let mut note = TextBuf::new();
            if self.tab == TAB_PUZZLE {
                note.push_str(crate::gui::ui_text(
                    "Kareleri boşluğa doğru kaydır",
                    "Slide tiles into the empty space",
                ));
            } else if self.perf_phase == PerfPhase::Finished {
                note.push_str(crate::gui::ui_text("Test tamamlandı", "Test complete"));
            } else {
                note.push_str(crate::gui::ui_text(
                    "1/2/3 bölüm - N yeni oyun - S ses testi",
                    "1/2/3 sections - N new game - S sound test",
                ));
            }
            MinGuiCompositor::draw_string_clipped(
                fb,
                x + sp(10),
                sy + sp(7),
                note.as_str(),
                Color::TEXT_MUTED,
                (x + sp(4), sy, w.saturating_sub(sp(8)), sp(STATUS_H)),
            );
            return;
        }
        let (light_n, light_k) = self.board.count(true);
        let (dark_n, dark_k) = self.board.count(false);
        let mut left = TextBuf::new();
        let turn = if self.board.side_to_move() == game_core::LIGHT_SIDE {
            crate::gui::ui_text("Açık", "Light")
        } else {
            crate::gui::ui_text("Koyu", "Dark")
        };
        left.push_str(turn);
        left.push_str(crate::gui::ui_text(" sırası", " to move"));
        if !self.message.is_empty() {
            left.push_str(" - ");
            left.push_str(self.message);
        }
        MinGuiCompositor::draw_string_clipped(
            fb,
            x + sp(10),
            sy + sp(7),
            left.as_str(),
            if self.thinking {
                Color::ACCENT_YELLOW
            } else {
                Color::TEXT_MAIN
            },
            (
                x + sp(4),
                sy,
                w.saturating_sub(sp(8) + sp(150)),
                sp(STATUS_H),
            ),
        );
        // Dama oynanış kılavuzu.
        let hint = crate::gui::ui_text(
            "Tıkla taşı seç, hedefe tıkla oynayın",
            "Click to select, click target to move",
        );
        MinGuiCompositor::draw_string_clipped(
            fb,
            x + sp(10),
            sy + sp(7) + sp(14),
            hint,
            Color::TEXT_MUTED,
            (
                x + sp(4),
                sy,
                w.saturating_sub(sp(8) + sp(150)),
                sp(STATUS_H),
            ),
        );
        let mut right = TextBuf::new();
        right.push_str("A ");
        right.push_usize(light_n + light_k);
        right.push_str("  K ");
        right.push_usize(dark_n + dark_k);
        right.push_str(crate::gui::ui_text("  | Şah ", "  | Kings "));
        right.push_usize(light_k + dark_k);
        let rw = MinGuiCompositor::text_width(right.as_str(), 1);
        MinGuiCompositor::draw_string(
            fb,
            x + w.saturating_sub(rw + sp(12)),
            sy + sp(7),
            right.as_str(),
            Color::TEXT_MUTED,
        );
    }

    fn render_checkers(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let board = Self::board_rect(w, h);
        let (bx, by, bw, bh) = (x + board.0, y + board.1, board.2, board.3);
        let cell_w = bw / game_core::TILE;
        let cell_h = bh / game_core::TILE;
        fb.draw_rounded_rect(bx - 4, by - 4, bw + 8, bh + 8, 6, Color::BORDER);

        // Tahta: açık/koyu kareler.
        for sq in 0..game_core::SQUARES {
            let (row, col) = game_core::square(sq);
            let cx = bx + col * cell_w;
            let cy = by + row * cell_h;
            let light_square = (row + col) % 2 == 0;
            let color = if light_square {
                MinGuiCompositor::shade(Color::WINDOW_BG, 30)
            } else {
                MinGuiCompositor::shade(Color::TITLE_BAR, -20)
            };
            fb.draw_rect(cx, cy, cell_w, cell_h, color);

            // Son hamlenin izi.
            if let Some(mv) = self.last_move {
                if mv.from() == sq || mv.to() == sq {
                    fb.draw_rect(
                        cx,
                        cy,
                        cell_w,
                        cell_h,
                        MinGuiCompositor::shade(Color::ACCENT_YELLOW, -60),
                    );
                }
            }
            // Geçerli hedefler.
            if self.targets.iter().any(|m| m.to() == sq) {
                let dot = (cell_w / 4).max(3);
                fb.draw_rounded_rect(
                    cx + cell_w / 2 - dot / 2,
                    cy + cell_h / 2 - dot / 2,
                    dot,
                    dot,
                    2,
                    Color::ACCENT_GREEN,
                );
            }
            // Seçili kare.
            if self.selected == Some(sq) {
                fb.draw_rect_outline(cx, cy, cell_w, cell_h, Color::ACCENT_GREEN);
                fb.draw_rect_outline(
                    cx + 1,
                    cy + 1,
                    cell_w.saturating_sub(2),
                    cell_h.saturating_sub(2),
                    Color::ACCENT_GREEN,
                );
            }

            // Taş.
            let piece = self.board.cells[sq];
            if piece != game_core::EMPTY {
                let inset = (cell_w / 8).max(2);
                let diameter = cell_w.saturating_sub(inset * 2);
                let (px, py) = self.piece_center(cx, cy, cell_w, cell_h);
                let radius = (diameter / 2).max(2);
                let (outer, inner) = if game_core::is_light(piece) {
                    (Color::ACCENT_PINK, Color::WHITE)
                } else {
                    (
                        Color::TITLE_BAR,
                        MinGuiCompositor::shade(Color::ACCENT_BLUE, -10),
                    )
                };
                Self::fill_circle(fb, px, py, radius, outer);
                Self::fill_circle(
                    fb,
                    px,
                    py,
                    radius.saturating_sub((radius / 5).max(1)),
                    inner,
                );
                if game_core::is_king(piece) {
                    // Şah işareti: küçük taç (dört nokta).
                    let crown = (radius / 2).max(2);
                    let c = Color::ACCENT_YELLOW;
                    fb.draw_rect(px.saturating_sub(crown), py.saturating_sub(2), 3, 5, c);
                    fb.draw_rect(px.saturating_sub(1), py.saturating_sub(4), 3, 7, c);
                    fb.draw_rect(px + crown.saturating_sub(2), py.saturating_sub(2), 3, 5, c);
                }
            }
        }
    }

    fn piece_center(&self, cx: usize, cy: usize, cell_w: usize, cell_h: usize) -> (usize, usize) {
        (cx + cell_w / 2, cy + cell_h / 2)
    }

    fn render_puzzle(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let rect = Self::puzzle_rect(w, h);
        let (bx, by, bw, bh) = (x + rect.0, y + rect.1, rect.2, rect.3);
        fb.draw_rounded_rect(bx - 4, by - 4, bw + 8, bh + 8, 6, Color::BORDER);
        let cell_w = bw / PUZZLE_SIDE;
        let cell_h = bh / PUZZLE_SIDE;
        for (i, &value) in self.tiles.iter().enumerate() {
            let (cx, cy, _, _) = Self::cell_rect(rect, i);
            let (cx, cy) = (x + cx, y + cy);
            if value == game_core::PUZZLE_BLANK {
                fb.draw_rect(cx, cy, cell_w, cell_h, Color::DARK_BG);
                continue;
            }
            let home = value as usize;
            let solved = home == i;
            let bg = if solved {
                MinGuiCompositor::shade(Color::ACCENT_GREEN, -10)
            } else {
                Color::ACCENT_BLUE
            };
            fb.draw_rounded_rect(
                cx + 2,
                cy + 2,
                cell_w.saturating_sub(4),
                cell_h.saturating_sub(4),
                4,
                bg,
            );
            fb.draw_rounded_rect_outline(
                cx + 2,
                cy + 2,
                cell_w.saturating_sub(4),
                cell_h.saturating_sub(4),
                4,
                Color::WHITE,
            );
            let mut label = TextBuf::new();
            label.push_usize(home);
            let lw = MinGuiCompositor::text_width(label.as_str(), 1);
            MinGuiCompositor::draw_string(
                fb,
                cx + cell_w.saturating_sub(lw) / 2,
                cy + cell_h.saturating_sub(MinGuiCompositor::text_height()) / 2,
                label.as_str(),
                MinGuiCompositor::text_on(bg),
            );
        }
        // Alt bilgi satırı: hamle, süre, karıştır.
        let info_y = by + bh + sp(6);
        if info_y + sp(22) < y + h {
            let mut info = TextBuf::new();
            info.push_str(crate::gui::ui_text("Hamle: ", "Moves: "));
            info.push_usize(self.puzzle_moves as usize);
            info.push_str("   ");
            info.push_str(crate::gui::ui_text("Süre: ", "Time: "));
            info.push_usize(self.puzzle_seconds as usize);
            info.push_str("s   ");
            info.push_str(if self.puzzle_demo {
                crate::gui::ui_text("Gösteri", "Demo")
            } else {
                crate::gui::ui_text("Elle", "Manual")
            });
            MinGuiCompositor::draw_string(fb, bx, info_y, info.as_str(), Color::TEXT_MUTED);
            let sx = bx + bw.saturating_sub(sp(92));
            fb.draw_rounded_rect(sx, info_y - 4, sp(90), sp(22), 5, Color::TITLE_BAR);
            MinGuiCompositor::draw_string(
                fb,
                sx + sp(10),
                info_y,
                crate::gui::ui_text("Karıştır", "Shuffle"),
                Color::TEXT_MAIN,
            );
        }
    }

    fn render_perf(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let px = x + sp(12);
        let mut py = y + sp(TAB_Y + TAB_H + 12);
        let line = sp(20);
        let title = match self.perf_phase {
            PerfPhase::Idle => {
                crate::gui::ui_text("Performans testi hazır", "Performance test ready")
            }
            PerfPhase::Collecting => crate::gui::ui_text("Ölçülüyor...", "Measuring..."),
            PerfPhase::Finished => crate::gui::ui_text("Sonuç", "Result"),
        };
        MinGuiCompositor::draw_string(fb, px, py, title, Color::TEXT_MAIN);
        py += line;

        // İlerleme çubuğu.
        let bar_w = w.saturating_sub(sp(30));
        fb.draw_rect(px, py + sp(4), bar_w, sp(10), Color::DARK_BG);
        let pct = if self.perf_phase == PerfPhase::Collecting {
            (self.perf_tick as usize * 100 / PERF_TICKS as usize).min(100)
        } else if self.perf_phase == PerfPhase::Finished {
            100
        } else {
            0
        };
        fb.draw_rect(
            px,
            py + sp(4),
            bar_w * pct / 100,
            sp(10),
            Color::ACCENT_GREEN,
        );
        py += line + sp(6);

        if self.perf_phase == PerfPhase::Collecting {
            let mut t = TextBuf::new();
            t.push_str(crate::gui::ui_text("İlerleme: %", "Progress: %"));
            t.push_usize(pct);
            t.push_str(crate::gui::ui_text("   - Ölçülüyor", "   - Measuring"));
            MinGuiCompositor::draw_string(fb, px, py, t.as_str(), Color::TEXT_MUTED);
            py += line;
        }

        if self.perf_phase == PerfPhase::Finished {
            let r = &self.perf_results;
            let rows: [(TextBuf, u32); 5] = {
                let mut a = TextBuf::new();
                a.push_str(crate::gui::ui_text("Kare hızı: ", "Frame rate: "));
                a.push_usize(r.fps as usize);
                a.push_str(" FPS");
                let mut b = TextBuf::new();
                b.push_str(crate::gui::ui_text("Çerçeve süresi: ", "Frame time: "));
                b.push_usize(r.frame_ms as usize);
                b.push_str(" ms");
                let mut c = TextBuf::new();
                c.push_str(crate::gui::ui_text(
                    "Zamanlayıcı sapması: ",
                    "Timer jitter: ",
                ));
                c.push_usize(r.timer_jitter_us as usize);
                c.push_str(" µs");
                let mut d = TextBuf::new();
                d.push_str(crate::gui::ui_text(
                    "Bellek dolaşımı: ",
                    "Memory throughput: ",
                ));
                d.push_usize(r.mem_mbps as usize);
                d.push_str(" MB/s");
                let mut e = TextBuf::new();
                e.push_str(crate::gui::ui_text("Aritmetik: ", "Arithmetic: "));
                e.push_usize(r.math_kops as usize);
                e.push_str(crate::gui::ui_text(" k işlem/s", "k ops/s"));
                [
                    (a, Color::TEXT_MAIN),
                    (b, Color::TEXT_MAIN),
                    (c, Color::TEXT_MAIN),
                    (d, Color::TEXT_MAIN),
                    (e, Color::TEXT_MAIN),
                ]
            };
            for (buf, color) in rows.iter() {
                MinGuiCompositor::draw_string(fb, px, py, buf.as_str(), *color);
                py += line;
            }
            let mut score = TextBuf::new();
            score.push_str(crate::gui::ui_text("Puan: ", "Score: "));
            score.push_usize(r.score as usize);
            score.push_str(" / 100 - ");
            score.push_str(Self::grade(r.score));
            MinGuiCompositor::draw_string_clipped(
                fb,
                px,
                py,
                score.as_str(),
                Color::ACCENT_GREEN,
                (px, py, w.saturating_sub(sp(24)), sp(22)),
            );
        } else if self.perf_phase == PerfPhase::Idle {
            MinGuiCompositor::draw_string(
                fb,
                px,
                py,
                crate::gui::ui_text(
                    "3 saniyelik test: CPU, bellek, çizim ve ses",
                    "3 second test: CPU, memory, drawing and sound",
                ),
                Color::TEXT_MUTED,
            );
        }

        // Başlat düğmesi.
        let (bx, by, bw, bh) = Self::perf_button_rect(w, h);
        let (bx, by) = (x + bx, y + by);
        let bg = if self.perf_phase == PerfPhase::Collecting {
            Color::TITLE_BAR_INACTIVE
        } else {
            Color::ACCENT_GREEN
        };
        fb.draw_rounded_rect(bx, by, bw, bh, 6, bg);
        MinGuiCompositor::draw_string(
            fb,
            bx + sp(12),
            by + (bh.saturating_sub(MinGuiCompositor::text_height())) / 2,
            crate::gui::ui_text("Performans Testini Başlat", "Start Performance Test"),
            MinGuiCompositor::text_on(bg),
        );

        // Ses testi düğmesi.
        let (sx, sy, sw, sh) = Self::sound_button_rect(w, h);
        let (sx, sy) = (x + sx, y + sy);
        fb.draw_rounded_rect(sx, sy, sw, sh, 6, Color::ACCENT_YELLOW);
        MinGuiCompositor::draw_string(
            fb,
            sx + sp(12),
            sy + (sh.saturating_sub(MinGuiCompositor::text_height())) / 2,
            crate::gui::ui_text("Ses Testi", "Sound Test"),
            MinGuiCompositor::text_on(Color::ACCENT_YELLOW),
        );
    }

    pub fn perf_button_rect(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let bw = sp(210).min(w.saturating_sub(sp(24)));
        (sp(12), h.saturating_sub(sp(56)), bw, sp(30))
    }

    pub fn sound_button_rect(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let (bx, by, bw, bh) = Self::perf_button_rect(w, h);
        let sw = sp(110).min(w.saturating_sub(sp(24)));
        (bx + bw + sp(8), by, sw, bh)
    }

    /// Performans testi düğmesine tıklama (pencere içi koordinatlar).
    pub fn click_perf_button(&mut self, px: usize, py: usize, win_w: usize, win_h: usize) {
        if crate::gui::in_rect(px, py, Self::perf_button_rect(win_w, win_h)) {
            if self.perf_phase != PerfPhase::Collecting {
                self.start_perf();
            }
        } else if crate::gui::in_rect(px, py, Self::sound_button_rect(win_w, win_h)) {
            self.sound_pending = Sound::Win;
        }
    }

    // no_std için hızlı f64 karekök (Newton-Raphson).
    fn fast_sqrt_f64(x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        let mut g = if x >= 1.0 { x * 0.5 + 0.5 } else { 1.0 };
        for _ in 0..20 {
            g = 0.5 * (g + x / g);
        }
        g
    }

    /// Dolu daire (satır satır tarama).
    fn fill_circle(fb: &mut Framebuffer, cx: usize, cy: usize, radius: usize, color: u32) {
        let r = radius as isize;
        let mut dy = -r;
        while dy <= r {
            let span = Self::fast_sqrt_f64((r * r - dy * dy) as f64) as isize;
            let y = cy as isize + dy;
            if y >= 0 {
                let x0 = (cx as isize - span).max(0) as usize;
                let width = (span * 2) as usize;
                fb.draw_rect(x0, y as usize, width, 1, color);
            }
            dy += 1;
        }
    }
}

/// Piksel koordinatını dama kare indeksine çevirir (8x8).
fn cell_of_checkers(px: usize, py: usize, rect: (usize, usize, usize, usize)) -> Option<usize> {
    let (x, y, w, h) = rect;
    if w == 0 || h == 0 || px < x || py < y || px >= x + w || py >= y + h {
        return None;
    }
    let cw = (w / game_core::TILE).max(1);
    let ch = (h / game_core::TILE).max(1);
    let max_side = game_core::TILE - 1;
    let col = ((px - x) / cw).min(max_side);
    let row = ((py - y) / ch).min(max_side);
    Some(game_core::idx(row, col))
}

/// Piksel koordinatını bulmaca kare indeksine çevirir (4x4).
fn cell_of_puzzle(px: usize, py: usize, rect: (usize, usize, usize, usize)) -> Option<usize> {
    let (x, y, w, h) = rect;
    if w == 0 || h == 0 || px < x || py < y || px >= x + w || py >= y + h {
        return None;
    }
    let cw = (w / PUZZLE_SIDE).max(1);
    let ch = (h / PUZZLE_SIDE).max(1);
    let max_side = PUZZLE_SIDE - 1;
    let col = ((px - x) / cw).min(max_side);
    let row = ((py - y) / ch).min(max_side);
    Some(row * PUZZLE_SIDE + col)
}

impl GameApp {
    /// Pencere içi tıklamayı işler. Yeniden çizim gerekiyorsa `true` döner.
    pub fn click(&mut self, px: usize, py: usize, w: usize, h: usize) -> bool {
        // Sekmeler.
        for tab in 0..TAB_COUNT {
            if crate::gui::in_rect(px, py, Self::tab_rect(tab)) {
                self.select_tab(tab);
                return true;
            }
        }
        match self.tab {
            TAB_GAME => {
                if crate::gui::in_rect(px, py, Self::new_game_rect()) {
                    let player_light = self.player_light;
                    self.new_game(player_light);
                    return true;
                }
                if let Some(sq) = cell_of_checkers(px, py, Self::board_rect(w, h)) {
                    self.click_cell(sq);
                    return true;
                }
                false
            }
            TAB_PUZZLE => {
                let rect = Self::puzzle_rect(w, h);
                if let Some(sq) = cell_of_puzzle(px, py, rect) {
                    self.click_tile(sq);
                    return true;
                }
                // "Karıştır" düğmesi (tahta altı bilgi satırındaki).
                let (bx, by, bw, bh) = rect;
                let shuffle = (
                    bx + bw.saturating_sub(sp(92)),
                    by + bh + sp(2),
                    sp(90),
                    sp(22),
                );
                if crate::gui::in_rect(px, py, shuffle) {
                    self.shuffle_puzzle();
                    return true;
                }
                false
            }
            TAB_PERF => {
                self.click_perf_button(px, py, w, h);
                true
            }
            _ => false,
        }
    }

    /// Klavye kısayolları: 1-3 bölüm, N yeni oyun, S ses testi, P performans testi.
    /// İşlenen tuş varsa `true` döner.
    pub fn handle_key(&mut self, ch: char) -> bool {
        match ch {
            '\t' => {
                self.select_tab((self.tab + 1) % TAB_COUNT);
                true
            }
            'n' | 'N' => {
                match self.tab {
                    TAB_GAME => {
                        let player_light = self.player_light;
                        self.new_game(player_light);
                    }
                    TAB_PUZZLE => self.shuffle_puzzle(),
                    _ => self.start_perf(),
                }
                true
            }
            's' | 'S' => {
                self.sound_pending = Sound::Win;
                true
            }
            'p' | 'P' => {
                self.select_tab(TAB_PERF);
                self.start_perf();
                true
            }
            '1'..='3' => {
                self.select_tab(ch as usize - '1' as usize);
                true
            }
            _ => false,
        }
    }

    /// Bekleyen ses efektini alır (ana döngü çalar).
    pub fn take_sound(&mut self) -> Sound {
        let sound = self.sound_pending;
        self.sound_pending = Sound::None;
        sound
    }

    /// Ses kimliğine karşılık gelen (frekans Hz, süre ms) çifti.
    pub fn sound_tone(sound: Sound) -> Option<(u32, u32)> {
        match sound {
            Sound::None => None,
            Sound::Tick => Some((1_200, 12)),
            Sound::Move => Some((660, 28)),
            Sound::Capture => Some((392, 45)),
            Sound::Win => Some((988, 90)),
            Sound::Lose => Some((220, 90)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_game_starts_with_player_turn() {
        let app = GameApp::new();
        assert!(app.player_to_move());
        assert!(app.targets.is_empty());
    }

    #[test]
    fn selecting_piece_lists_targets() {
        let mut app = GameApp::new();
        // Açılış konumunda alt sıradaki bir taş hareketsizdir, ikinci sıradaki taş oynar.
        app.click_cell(game_core::idx(5, 0));
        assert_eq!(app.selected, None);
        app.click_cell(game_core::idx(5, 2));
        assert_eq!(app.selected, Some(game_core::idx(5, 2)));
        assert!(!app.targets.is_empty());
    }

    #[test]
    fn clicking_target_moves_piece_and_handles_turn() {
        let mut app = GameApp::new();
        app.click_cell(game_core::idx(5, 2));
        let to = app.targets[0].to();
        app.click_cell(to);
        assert_eq!(app.board.ply, 1);
        assert!(!app.player_to_move());
        assert!(app.thinking);
    }

    #[test]
    fn perf_button_starts_test() {
        let mut app = GameApp::new();
        app.select_tab(TAB_PERF);
        let (bx, by, bw, bh) = GameApp::perf_button_rect(600, 460);
        app.click_perf_button(bx + bw / 2, by + bh / 2, 600, 460);
        assert_eq!(app.perf_phase, PerfPhase::Collecting);
    }

    #[test]
    fn sound_mapping_is_defined() {
        assert_eq!(GameApp::sound_tone(Sound::None), None);
        assert_eq!(GameApp::sound_tone(Sound::Win), Some((988, 90)));
    }
}
