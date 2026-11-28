//! "Oyunlar" uygulaması — 2D oyunlar tek pencerede.
//!
//! Dört bölüm tek pencerede sunulur:
//! * **Dama** — Türk/uluslararası kurallı, yapay zekâya karşı 2D dama.
//! * **Mayın Tarlası** — klasik mayın tarlası; hücreleri aç, bayrak koy, tüm
//!   güvenli hücreleri açarak kazan.
//! * **Satranç** — 8x8 satranç tahtası; tam yasal hamle kontrolü, rok, geçerken
//!   alma, terfi, şah mat/pat ve iki oyunculu oynanış.
//! * **Bilardo** — 8-ball bilardo; üstten 2D masa, toplar 3D döndürülerek
//!   deliklere girer.
//!
//! Tüm oyun mantığı (dama, mayın tarlası, satranç, bilardo) `game_core` içinde saf
//! olarak tutulur; bu modül yalnızca çizim ve girdi yönetimini yapar. Ses,
//! ana döngüde (`GameApp::sound_pending`) PC Speaker / HDA üzerinden çalınır.

use super::game_core::{
    self, BallKind, BilliardsGame, Board, CellState, ChessBoard, ChessMove, ChessSide, ChessStatus,
    GameState, Minesweeper, Move, PieceKind,
};
use super::game_core::{billiards_clamp, billiards_max, billiards_min};
use super::gfx3d::{fast_atan2, fast_cos, fast_sin, fast_sqrt};
use crate::drivers::video::Framebuffer;
use crate::gui::{scaled_statusbar_h, sp, Color, MinGuiCompositor, TextBuf};

/// Bölüm sekmesi sayısı.
pub const TAB_COUNT: usize = 4;
const TAB_GAME: usize = 0;
const TAB_MINES: usize = 1;
const TAB_CHESS: usize = 2;
const TAB_BILLIARDS: usize = 3;

const TAB_Y: usize = 28;
const TAB_H: usize = 24;
const TAB_W: usize = 96;
const TOOLBAR_Y: usize = TAB_Y + TAB_H + 4;
const TOOLBAR_H: usize = 20;

/// Durum çubuğu iki satır (bilgi + ipucu) gösterebildiği için normal
/// durum çubuğundan biraz daha yüksektir.
fn status_h() -> usize {
    scaled_statusbar_h() + sp(10)
}

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

/// Satranç seçili kare ve hedefleri.
pub struct ChessSel {
    pub selected: Option<(usize, usize)>,
    pub targets: alloc::vec::Vec<ChessMove>,
}

pub struct GameApp {
    pub tab: usize,
    // --- Dama ---
    pub board: Board,
    pub selected: Option<usize>,
    pub targets: alloc::vec::Vec<Move>,
    pub message: &'static str,
    pub thinking: bool,
    pub ai_wait: u8,
    pub player_light: bool,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
    pub last_move: Option<Move>,
    /// Oyuncunun hamlesinden önceki konumlar (geri al için).
    pub history: alloc::vec::Vec<Board>,
    /// Yapay zekâ derinliği: 0 kolay, 1 orta, 2 zor.
    pub ai_level: usize,
    // --- Mayın Tarlası ---
    pub mines: Minesweeper,
    pub mine_started_ms: Option<u64>,
    pub mine_elapsed_ms: u64,
    /// Seviye başına en iyi süre (ms); 0 = henüz yok.
    pub mine_best_ms: [u64; 3],
    // --- Satranç ---
    pub chess: ChessBoard,
    pub chess_sel: ChessSel,
    pub chess_history: alloc::vec::Vec<ChessBoard>,
    pub chess_last: Option<ChessMove>,
    /// Terfi seçimi bekleyen hamle.
    pub chess_pending: Option<ChessMove>,
    // --- Bilardo ---
    pub billiards: BilliardsGame,
    pub billiards_selected_ball: Option<usize>,
    pub billiards_aiming: bool,
    /// AI düşünme beklemesi.
    pub billiards_ai_wait: u8,
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
            history: alloc::vec::Vec::new(),
            ai_level: 1,
            mines: Minesweeper::new(),
            mine_started_ms: None,
            mine_elapsed_ms: 0,
            mine_best_ms: [0; 3],
            chess: ChessBoard::new(),
            chess_sel: ChessSel {
                selected: None,
                targets: alloc::vec::Vec::new(),
            },
            chess_history: alloc::vec::Vec::new(),
            chess_last: None,
            chess_pending: None,
            billiards: BilliardsGame::new(),
            billiards_selected_ball: None,
            billiards_aiming: false,
            billiards_ai_wait: 0,
            sound_pending: Sound::None,
            sound_volume: 60,
            // Oyun sesleri kullanıcı özellikle açana kadar kapalıdır.
            muted: true,
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
            ("Mayın Tarlası", "Minesweeper"),
            ("Satranç", "Chess"),
            ("Bilardo", "Billiards"),
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

    /// Oyun alanı: araç çubuğunun altında kalan kare alan.
    pub fn board_rect(win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
        let top = sp(TOOLBAR_Y + TOOLBAR_H + 6);
        let avail_h = win_h.saturating_sub(top + status_h() + sp(6) + sp(34));
        let avail_w = win_w.saturating_sub(sp(16));
        let side = avail_w.min(avail_h).max(sp(96));
        let x = sp(8) + avail_w.saturating_sub(side) / 2;
        (x, top, side, side)
    }

    /// "Yeni Oyun" düğmesi.
    pub fn new_game_rect() -> (usize, usize, usize, usize) {
        let x = sp(8) + TAB_COUNT * (sp(TAB_W) + sp(4)) + sp(6);
        (x, sp(TAB_Y), sp(86), sp(TAB_H))
    }

    pub fn sound_rect() -> (usize, usize, usize, usize) {
        let (x, y, w, h) = Self::new_game_rect();
        (x + w + sp(4), y, sp(76), h)
    }

    pub fn mine_level_rect(level: usize) -> (usize, usize, usize, usize) {
        (sp(8) + level * sp(58), sp(TOOLBAR_Y), sp(54), sp(TOOLBAR_H))
    }

    /// Dama zorluk düğmeleri (Mayın Tarlası düğmeleriyle aynı konum).
    pub fn ai_level_rect(level: usize) -> (usize, usize, usize, usize) {
        Self::mine_level_rect(level)
    }

    /// "Geri Al" düğmesi: araç çubuğunda zorluk düğmelerinin sağında.
    pub fn undo_rect(tab: usize) -> (usize, usize, usize, usize) {
        let x = if tab == TAB_GAME {
            sp(8) + 3 * sp(58) + sp(6)
        } else {
            sp(8)
        };
        (x, sp(TOOLBAR_Y), sp(72), sp(TOOLBAR_H))
    }

    pub fn set_mine_level(&mut self, level: usize) {
        self.mines.set_level(level);
        self.mine_started_ms = None;
        self.mine_elapsed_ms = 0;
        self.sound_pending = Sound::None;
    }

    pub fn set_ai_level(&mut self, level: usize) {
        self.ai_level = level.min(2);
        self.sound_pending = Sound::Tick;
    }

    pub fn ai_depth(&self) -> u8 {
        match self.ai_level {
            0 => 1,
            2 => 3,
            _ => 2,
        }
    }

    pub fn toggle_sound(&mut self) {
        self.muted = !self.muted;
        if self.muted {
            self.sound_pending = Sound::None;
        }
    }

    // -----------------------------------------------------------------------
    // Dama girdisi
    // -----------------------------------------------------------------------

    pub fn new_game(&mut self, player_light: bool) {
        self.board = Board::new();
        self.player_light = player_light;
        self.selected = None;
        self.targets.clear();
        self.history.clear();
        self.last_move = None;
        self.thinking = false;
        self.ai_wait = 0;
        self.message = "";
        self.sound_pending = Sound::Tick;
    }

    fn player_to_move(&self) -> bool {
        (self.board.side_to_move() == game_core::LIGHT_SIDE) == self.player_light
    }

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
        if let Some(mv) = self.targets.iter().find(|m| m.to() == index).copied() {
            let previous = self.board;
            if self.board.apply(&mv) {
                self.history.push(previous);
                if self.history.len() > 64 {
                    self.history.remove(0);
                }
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

    /// Oyuncunun son hamlesini (ve varsa bilgisayarın cevabını) geri alır.
    pub fn undo_checkers(&mut self) {
        let Some(previous) = self.history.pop() else {
            self.message = crate::gui::ui_text("Geri alınacak hamle yok", "No move to undo");
            self.sound_pending = Sound::Tick;
            return;
        };
        // Bitmiş bir oyun geri alınıyorsa istatistiği de geri al.
        if let Some(light_won) = self.board.winner() {
            if light_won == self.player_light {
                self.wins = self.wins.saturating_sub(1);
            } else {
                self.losses = self.losses.saturating_sub(1);
            }
        } else if self.board.is_draw() {
            self.draws = self.draws.saturating_sub(1);
        }
        self.board = previous;
        self.selected = None;
        self.targets.clear();
        self.last_move = None;
        self.thinking = false;
        self.ai_wait = 0;
        self.message = crate::gui::ui_text("Hamle geri alındı", "Move undone");
        self.sound_pending = Sound::Tick;
    }

    // -----------------------------------------------------------------------
    // Mayın Tarlası girdisi
    // -----------------------------------------------------------------------

    pub fn click_mine_cell(&mut self, r: usize, c: usize) {
        if self.tab != TAB_MINES || self.mines.state != GameState::Playing {
            return;
        }
        let idx = r * self.mines.cols + c;
        let cell_state = self
            .mines
            .cells
            .get(idx)
            .map(|cell| cell.state)
            .unwrap_or(CellState::Hidden);
        match cell_state {
            CellState::Revealed => {
                if self.mines.chord(r, c) {
                    self.after_mine_action();
                } else {
                    self.sound_pending = Sound::Tick;
                }
                return;
            }
            CellState::Flagged => {
                self.sound_pending = Sound::Tick;
                return;
            }
            CellState::Hidden => {}
        }
        if self.mine_started_ms.is_none() {
            self.mine_started_ms = Some(crate::monotonic_ms());
        }
        if self.mines.reveal(r, c) {
            self.after_mine_action();
        }
    }

    fn after_mine_action(&mut self) {
        match self.mines.state {
            GameState::Won => {
                self.stop_mine_timer();
                self.sound_pending = Sound::Win;
            }
            GameState::Lost => {
                self.stop_mine_timer();
                self.sound_pending = Sound::Lose;
            }
            GameState::Playing => self.sound_pending = Sound::Move,
        }
    }

    fn stop_mine_timer(&mut self) {
        let Some(start) = self.mine_started_ms.take() else {
            return;
        };
        let elapsed = crate::monotonic_ms().saturating_sub(start);
        self.mine_elapsed_ms = elapsed;
        if self.mines.state == GameState::Won {
            let level = self.mines.level.min(2);
            let best = &mut self.mine_best_ms[level];
            if *best == 0 || elapsed < *best {
                *best = elapsed;
            }
        }
    }

    pub fn right_click_mine_cell(&mut self, r: usize, c: usize) {
        if self.tab != TAB_MINES || self.mines.state != GameState::Playing {
            return;
        }
        if self.mines.toggle_flag(r, c) {
            self.sound_pending = Sound::Tick;
        }
    }

    pub fn reset_mines(&mut self) {
        self.mines.reset();
        self.mine_started_ms = None;
        self.mine_elapsed_ms = 0;
        self.sound_pending = Sound::Tick;
    }

    /// Piksel koordinatını mayın hücresine çevirir.
    fn cell_of_mines(
        px: usize,
        py: usize,
        rect: (usize, usize, usize, usize),
        rows: usize,
        cols: usize,
    ) -> Option<(usize, usize)> {
        let (x, y, w, h) = rect;
        if w == 0 || h == 0 || px < x || py < y || px >= x + w || py >= y + h {
            return None;
        }
        let cw = (w / cols).max(1);
        let ch = (h / rows).max(1);
        let col = ((px - x) / cw).min(cols - 1);
        let row = ((py - y) / ch).min(rows - 1);
        Some((row, col))
    }

    /// Pencere içi sağ tıklama: mayın tarlasında bayrak koyar/kaldırır.
    pub fn right_click(&mut self, px: usize, py: usize, w: usize, h: usize) -> bool {
        if self.tab != TAB_MINES {
            return false;
        }
        let Some((r, c)) = Self::cell_of_mines(
            px,
            py,
            Self::board_rect(w, h),
            self.mines.rows,
            self.mines.cols,
        ) else {
            return false;
        };
        self.right_click_mine_cell(r, c);
        true
    }

    // -----------------------------------------------------------------------
    // Satranç girdisi
    // -----------------------------------------------------------------------

    pub fn click_chess_cell(&mut self, r: usize, c: usize) {
        if self.tab != TAB_CHESS || self.chess_pending.is_some() {
            return;
        }
        if self.chess.status != ChessStatus::Playing {
            return;
        }
        let idx = r * 8 + c;
        // Seçili karenin hedefi mi?
        if let Some(_sel) = self.chess_sel.selected {
            if let Some(mv) = self.chess_sel.targets.iter().find(|m| m.to == idx).copied() {
                if mv.promotion.is_some() {
                    // Terfi seçimi için hamleyi beklet.
                    self.chess_pending = Some(mv);
                    self.sound_pending = Sound::Tick;
                } else {
                    self.commit_chess_move(mv, None);
                }
                return;
            }
        }
        // Yeni taş seçimi (sırası gelen oyuncunun taşı).
        self.chess_sel.selected = None;
        self.chess_sel.targets.clear();
        if let Some(piece) = self.chess.cells[r][c] {
            if piece.side == self.chess.side_to_move {
                let list = self.chess.legal_from(r, c);
                if list.is_empty() {
                    self.message =
                        crate::gui::ui_text("Bu taşın oynayabileceği hamle yok", "No legal move");
                } else {
                    self.chess_sel.selected = Some((r, c));
                    self.chess_sel.targets = list;
                    self.message =
                        crate::gui::ui_text("Hedef kareye tıkla", "Click a target square");
                }
                self.sound_pending = Sound::Tick;
            }
        }
    }

    /// Doğrulanmış satranç hamlesini uygular ve geçmişe kaydeder.
    pub fn commit_chess_move(&mut self, mv: ChessMove, promotion: Option<PieceKind>) {
        if self.chess.status != ChessStatus::Playing {
            return;
        }
        let mut mv = mv;
        if mv.promotion.is_some() {
            mv.promotion = Some(promotion.unwrap_or(PieceKind::Queen));
        }
        let (fr, fc) = ChessBoard::coords(mv.from);
        let (tr, tc) = ChessBoard::coords(mv.to);
        let capture = self.chess.cells[tr][tc].is_some()
            || self.chess.cells[fr][fc].is_some_and(|p| p.kind == PieceKind::Pawn && fc != tc);
        let previous = self.chess;
        if self.chess.apply(&mv) {
            self.chess_history.push(previous);
            if self.chess_history.len() > 128 {
                self.chess_history.remove(0);
            }
            self.chess_last = Some(mv);
            self.chess_pending = None;
            self.chess_sel.selected = None;
            self.chess_sel.targets.clear();
            self.update_chess_message();
            self.sound_pending = match self.chess.status {
                ChessStatus::Checkmate(_) => Sound::Win,
                _ if capture => Sound::Capture,
                _ => Sound::Move,
            };
        }
    }

    /// Terfi panelinde seçilen taşı uygular.
    pub fn choose_promotion(&mut self, kind: PieceKind) {
        if let Some(mv) = self.chess_pending.take() {
            self.commit_chess_move(mv, Some(kind));
        }
    }

    pub fn cancel_promotion(&mut self) {
        self.chess_pending = None;
        self.message = crate::gui::ui_text("Terfi iptal edildi", "Promotion cancelled");
        self.sound_pending = Sound::Tick;
    }

    fn update_chess_message(&mut self) {
        self.message = match self.chess.status {
            ChessStatus::Checkmate(ChessSide::White) => {
                crate::gui::ui_text("Şah mat! Beyaz kazandı", "Checkmate! White wins")
            }
            ChessStatus::Checkmate(ChessSide::Black) => {
                crate::gui::ui_text("Şah mat! Siyah kazandı", "Checkmate! Black wins")
            }
            ChessStatus::Stalemate => crate::gui::ui_text("Pat! Berabere", "Stalemate! Draw"),
            ChessStatus::Draw => crate::gui::ui_text("Berabere", "Draw"),
            ChessStatus::Playing => {
                if self.chess.is_in_check(self.chess.side_to_move) {
                    crate::gui::ui_text("Şah!", "Check!")
                } else {
                    crate::gui::ui_text("Hedef kareye tıkla", "Click a target square")
                }
            }
        };
    }

    /// Satrançta son hamleyi geri alır.
    pub fn undo_chess(&mut self) {
        if self.chess_pending.take().is_some() {
            self.message = crate::gui::ui_text("Terfi iptal edildi", "Promotion cancelled");
            self.sound_pending = Sound::Tick;
            return;
        }
        let Some(previous) = self.chess_history.pop() else {
            self.message = crate::gui::ui_text("Geri alınacak hamle yok", "No move to undo");
            self.sound_pending = Sound::Tick;
            return;
        };
        self.chess = previous;
        self.chess_sel.selected = None;
        self.chess_sel.targets.clear();
        self.chess_last = None;
        self.message = crate::gui::ui_text("Hamle geri alındı", "Move undone");
        self.sound_pending = Sound::Tick;
    }

    pub fn reset_chess(&mut self) {
        self.chess.reset();
        self.chess_sel.selected = None;
        self.chess_sel.targets.clear();
        self.chess_history.clear();
        self.chess_last = None;
        self.chess_pending = None;
        self.message = "";
        self.sound_pending = Sound::Tick;
    }

    /// Terfi panelindeki dört düğmenin dikdörtgenleri (Vezir, Kale, Fil, At).
    pub fn promotion_rects(win_w: usize, win_h: usize) -> [(usize, usize, usize, usize); 4] {
        let (bx, by, bw, bh) = Self::board_rect(win_w, win_h);
        let cell = (bw / 8).max(sp(20));
        let gap = sp(6);
        let total = 4 * cell + 3 * gap;
        let x = bx + bw.saturating_sub(total) / 2;
        let y = by + bh.saturating_sub(cell) / 2;
        [
            (x, y, cell, cell),
            (x + cell + gap, y, cell, cell),
            (x + 2 * (cell + gap), y, cell, cell),
            (x + 3 * (cell + gap), y, cell, cell),
        ]
    }

    /// Terfi panelinde tıklamayı işler. Panel açıkken tüm tıklamaları tüketir.
    pub fn click_promotion(&mut self, px: usize, py: usize, w: usize, h: usize) -> bool {
        if self.chess_pending.is_none() {
            return false;
        }
        let kinds = [
            PieceKind::Queen,
            PieceKind::Rook,
            PieceKind::Bishop,
            PieceKind::Knight,
        ];
        for (i, rect) in Self::promotion_rects(w, h).iter().enumerate() {
            if crate::gui::in_rect(px, py, *rect) {
                self.choose_promotion(kinds[i]);
                return true;
            }
        }
        // Panel dışına tıklamak terfiyi iptal eder.
        self.cancel_promotion();
        true
    }

    pub fn undo_current(&mut self) {
        match self.tab {
            TAB_GAME => self.undo_checkers(),
            TAB_CHESS => self.undo_chess(),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Tick
    // -----------------------------------------------------------------------

    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        if self.tab == TAB_GAME
            && self.thinking
            && !self.player_to_move()
            && self.board.winner().is_none()
        {
            if self.ai_wait > 0 {
                self.ai_wait -= 1;
            } else if let Some(mv) = game_core::choose_move(&self.board, self.ai_depth()) {
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
        // Mayın Tarlası süresi: yalnızca saniye değiştiğinde yeniden çiz.
        if self.mines.state == GameState::Playing {
            if let Some(start) = self.mine_started_ms {
                let elapsed = crate::monotonic_ms().saturating_sub(start);
                if self.tab == TAB_MINES && elapsed / 1000 != self.mine_elapsed_ms / 1000 {
                    changed = true;
                }
                self.mine_elapsed_ms = elapsed;
            }
        }
        // Bilardo simülasyonu ve AI
        if self.tab == TAB_BILLIARDS {
            changed |= self.billiards.tick();
            // AI vuruşu (basit: sıra stripes oyuncusunda ve aiming durumunda)
            if self.billiards.state == super::game_core::BilliardsState::Aiming
                && self.billiards.current_player == super::game_core::BilliardsPlayerType::Stripes
            {
                if self.billiards_ai_wait > 0 {
                    self.billiards_ai_wait -= 1;
                } else {
                    self.billiards.ai_shoot();
                    self.billiards_ai_wait = 30; // ~1 saniye bekle
                    changed = true;
                }
            }
        }
        changed
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    // -----------------------------------------------------------------------
    // Çizim
    // -----------------------------------------------------------------------

    pub fn render_content(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        self.render_tabs(fb, x, y, w);
        match self.tab {
            TAB_GAME => self.render_checkers(fb, x, y, w, h),
            TAB_MINES => self.render_mines(fb, x, y, w, h),
            TAB_CHESS => self.render_chess(fb, x, y, w, h),
            TAB_BILLIARDS => self.render_billiards(fb, x, y, w, h),
            _ => {}
        }
        self.render_status(fb, x, y, w, h);
    }

    fn render_tabs(&self, fb: &mut Framebuffer, x: usize, y: usize, _w: usize) {
        let names = Self::tab_names();
        let tab_icons = [6usize, 7, 8, 9]; // checker, mine, chess king, billiard ball
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
            // Sekme simgesi (sol tarafa).
            let icon_sz = MinGuiCompositor::icon_size();
            let icon_x = x + tx + sp(4);
            let icon_y = y + ty + (th.saturating_sub(icon_sz)) / 2;
            MinGuiCompositor::draw_icon_by_index(fb, icon_x, icon_y, tab_icons[i]);
            // Sekme metni (simgenin sağına).
            let label = crate::gui::ui_text(pair.0, pair.1);
            let lw = MinGuiCompositor::text_width(label, 1);
            let text_x = icon_x + icon_sz + sp(4);
            MinGuiCompositor::draw_string(
                fb,
                text_x,
                y + ty + (th.saturating_sub(MinGuiCompositor::text_height())) / 2,
                label,
                if active {
                    MinGuiCompositor::text_on(bg)
                } else {
                    Color::TEXT_MAIN
                },
            );
            let _ = lw;
        }
        // "Yeni Oyun" düğmesi.
        if self.tab == TAB_GAME || self.tab == TAB_MINES || self.tab == TAB_CHESS {
            let (bx, by, bw, bh) = Self::new_game_rect();
            fb.draw_rounded_rect(x + bx, y + by, bw, bh, 5, Color::TITLE_BAR);
            MinGuiCompositor::draw_string(
                fb,
                x + bx + sp(10),
                y + by + (bh.saturating_sub(MinGuiCompositor::text_height())) / 2,
                crate::gui::ui_text("Yeni Oyun", "New Game"),
                Color::TEXT_MAIN,
            );
            let (sx, sy, sw, sh) = Self::sound_rect();
            fb.draw_rounded_rect(
                x + sx,
                y + sy,
                sw,
                sh,
                5,
                if self.muted {
                    Color::TITLE_BAR
                } else {
                    Color::ACCENT_GREEN
                },
            );
            MinGuiCompositor::draw_string(
                fb,
                x + sx + sp(8),
                y + sy + (sh.saturating_sub(MinGuiCompositor::text_height())) / 2,
                if self.muted {
                    crate::gui::ui_text("Ses Aç", "Sound On")
                } else {
                    crate::gui::ui_text("Sesi Kapat", "Mute")
                },
                Color::TEXT_MAIN,
            );
            if self.tab == TAB_MINES || self.tab == TAB_GAME {
                let labels = [
                    crate::gui::ui_text("Kolay", "Easy"),
                    crate::gui::ui_text("Orta", "Medium"),
                    crate::gui::ui_text("Zor", "Hard"),
                ];
                for (level, label) in labels.iter().enumerate() {
                    let (lx, ly, lw, lh) = if self.tab == TAB_MINES {
                        Self::mine_level_rect(level)
                    } else {
                        Self::ai_level_rect(level)
                    };
                    let active = if self.tab == TAB_MINES {
                        self.mines.level == level
                    } else {
                        self.ai_level == level
                    };
                    let bg = if active {
                        Color::ACCENT_BLUE
                    } else {
                        Color::TITLE_BAR
                    };
                    fb.draw_rounded_rect(x + lx, y + ly, lw, lh, 5, bg);
                    MinGuiCompositor::draw_string(
                        fb,
                        x + lx + sp(7),
                        y + ly + (lh.saturating_sub(MinGuiCompositor::text_height())) / 2,
                        label,
                        Color::TEXT_MAIN,
                    );
                }
            }
            if self.tab == TAB_GAME || self.tab == TAB_CHESS {
                let (ux, uy, uw, uh) = Self::undo_rect(self.tab);
                fb.draw_rounded_rect(x + ux, y + uy, uw, uh, 5, Color::TITLE_BAR);
                MinGuiCompositor::draw_string(
                    fb,
                    x + ux + sp(7),
                    y + uy + (uh.saturating_sub(MinGuiCompositor::text_height())) / 2,
                    crate::gui::ui_text("Geri Al", "Undo"),
                    Color::TEXT_MAIN,
                );
            }
        }
    }

    fn push_time(buf: &mut TextBuf, ms: u64) {
        let total = ms / 1000;
        let minutes = total / 60;
        let seconds = total % 60;
        if minutes > 0 {
            buf.push_usize(minutes as usize);
            buf.push_byte(b':');
            if seconds < 10 {
                buf.push_byte(b'0');
            }
        }
        buf.push_usize(seconds as usize);
        buf.push_byte(b's');
    }

    fn render_status(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let sh = status_h();
        if h < sh + sp(8) {
            return;
        }
        let sy = y + h.saturating_sub(sh + sp(4));
        fb.draw_rect(
            x + sp(4),
            sy,
            w.saturating_sub(sp(8)),
            sh,
            Color::TASKBAR_BG,
        );
        let mut left = TextBuf::new();
        match self.tab {
            TAB_GAME => {
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
                left.push_str("   ");
                left.push_str(crate::gui::ui_text("Siz: ", "You: "));
                left.push_usize(self.wins as usize);
                left.push_str(crate::gui::ui_text("  Bilgisayar: ", "  Computer: "));
                left.push_usize(self.losses as usize);
                left.push_str(crate::gui::ui_text("  Berabere: ", "  Draws: "));
                left.push_usize(self.draws as usize);
            }
            TAB_MINES => {
                left.push_str(crate::gui::ui_text("Mayın: ", "Mines: "));
                left.push_usize(self.mines.mine_count.saturating_sub(self.mines.flagged));
                left.push_str("   ");
                left.push_str(crate::gui::ui_text("Açık: ", "Revealed: "));
                left.push_usize(self.mines.revealed);
                left.push_str("   ");
                left.push_str(crate::gui::ui_text("Süre: ", "Time: "));
                Self::push_time(&mut left, self.mine_elapsed_ms);
                let best = self.mine_best_ms[self.mines.level.min(2)];
                if best > 0 {
                    left.push_str("   ");
                    left.push_str(crate::gui::ui_text("En iyi: ", "Best: "));
                    Self::push_time(&mut left, best);
                }
                left.push_str("   ");
                match self.mines.state {
                    GameState::Won => left.push_str(crate::gui::ui_text("Kazandınız!", "You win!")),
                    GameState::Lost => {
                        left.push_str(crate::gui::ui_text("Mayına bastınız!", "Mine hit!"))
                    }
                    _ => {}
                }
            }
            TAB_CHESS => {
                let side = match self.chess.side_to_move {
                    ChessSide::White => crate::gui::ui_text("Beyaz", "White"),
                    ChessSide::Black => crate::gui::ui_text("Siyah", "Black"),
                };
                left.push_str(side);
                left.push_str(crate::gui::ui_text(" sırası", " to move"));
                left.push_str(" - ");
                match self.chess.status {
                    ChessStatus::Checkmate(ChessSide::White) => left.push_str(crate::gui::ui_text(
                        "Şah mat! Beyaz kazandı",
                        "Checkmate! White wins",
                    )),
                    ChessStatus::Checkmate(ChessSide::Black) => left.push_str(crate::gui::ui_text(
                        "Şah mat! Siyah kazandı",
                        "Checkmate! Black wins",
                    )),
                    ChessStatus::Stalemate => {
                        left.push_str(crate::gui::ui_text("Pat! Berabere", "Stalemate! Draw"))
                    }
                    ChessStatus::Draw => left.push_str(crate::gui::ui_text("Berabere", "Draw")),
                    ChessStatus::Playing => {
                        if self.chess.is_in_check(self.chess.side_to_move) {
                            left.push_str(crate::gui::ui_text("ŞAH!", "CHECK!"));
                        } else if !self.message.is_empty() {
                            left.push_str(self.message);
                        } else {
                            left.push_str(crate::gui::ui_text("Oynanıyor", "Playing"));
                        }
                    }
                }
            }
            TAB_BILLIARDS => {
                use super::game_core::{BilliardsPlayerType, BilliardsState};
                let player_str = match self.billiards.current_player {
                    BilliardsPlayerType::Solids => crate::gui::ui_text("Düz (1-7)", "Solids (1-7)"),
                    BilliardsPlayerType::Stripes => {
                        crate::gui::ui_text("Çizgili (9-15)", "Stripes (9-15)")
                    }
                };
                left.push_str(player_str);
                left.push_str(crate::gui::ui_text(" sırası", " to move"));
                left.push_str(" - ");
                match self.billiards.state {
                    BilliardsState::Aiming => {
                        left.push_str(crate::gui::ui_text("Vuruş hazırlanıyor", "Aiming"))
                    }
                    BilliardsState::Shooting => {
                        left.push_str(crate::gui::ui_text("Toplar hareket ediyor", "Balls moving"))
                    }
                    BilliardsState::BallInHand => {
                        left.push_str(crate::gui::ui_text("Beyaz top elinizde", "Ball in hand"))
                    }
                    BilliardsState::GameOver => {
                        if let Some(w) = self.billiards.winner {
                            if w == 0 {
                                left.push_str(crate::gui::ui_text("DÜZ KAZANDI!", "SOLIDS WIN!"))
                            } else {
                                left.push_str(crate::gui::ui_text(
                                    "ÇİZGİLİ KAZANDI!",
                                    "STRIPES WIN!",
                                ))
                            }
                        } else {
                            left.push_str(crate::gui::ui_text("OYUN BİTTİ", "GAME OVER"))
                        }
                    }
                }
                if self.billiards.foul {
                    left.push_str(" - ");
                    left.push_str(crate::gui::ui_text("FOUL!", "FOUL!"));
                }
            }
            _ => {}
        }
        MinGuiCompositor::draw_string_clipped(
            fb,
            x + sp(10),
            sy + sp(7),
            left.as_str(),
            Color::TEXT_MAIN,
            (x + sp(4), sy, w.saturating_sub(sp(8)), status_h()),
        );
        let hint = match self.tab {
            TAB_GAME => crate::gui::ui_text(
                "Taşı seç, hedefe tıkla; Geri Al ile son hamleyi geri al",
                "Select and click target; Undo reverts the last move",
            ),
            TAB_MINES => crate::gui::ui_text(
                "Sol tık aç, sağ tık bayrak; açık sayıya tıkla çevresini aç",
                "Left click reveal, right click flag; click a number to chord",
            ),
            TAB_CHESS => crate::gui::ui_text(
                "Taşı seç, hedefe tıkla; piyon terfisinde taş seç",
                "Select piece, click target; choose a piece on promotion",
            ),
            TAB_BILLIARDS => crate::gui::ui_text(
                "Masa alanına tıkla vuruş yönü/gücü ayarla; Space ile vur",
                "Click table to aim; Space to shoot",
            ),
            _ => crate::gui::ui_text("", ""),
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            x + sp(10),
            sy + sp(7) + sp(14),
            hint,
            Color::TEXT_MUTED,
            (x + sp(4), sy, w.saturating_sub(sp(8)), status_h()),
        );
    }

    fn render_checkers(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let board = Self::board_rect(w, h);
        let (bx, by, bw, bh) = (x + board.0, y + board.1, board.2, board.3);
        let cell_w = bw / game_core::TILE;
        let cell_h = bh / game_core::TILE;
        fb.draw_rounded_rect(bx - 4, by - 4, bw + 8, bh + 8, 6, Color::BORDER);

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

    fn render_mines(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let board = Self::board_rect(w, h);
        let (bx, by, bw, bh) = (x + board.0, y + board.1, board.2, board.3);
        let cell_w = bw / self.mines.cols;
        let cell_h = bh / self.mines.rows;
        fb.draw_rounded_rect(bx - 4, by - 4, bw + 8, bh + 8, 6, Color::BORDER);

        for r in 0..self.mines.rows {
            for c in 0..self.mines.cols {
                let idx = r * self.mines.cols + c;
                let cell = self.mines.cells[idx];
                let cx = bx + c * cell_w;
                let cy = by + r * cell_h;
                let bg = match cell.state {
                    CellState::Revealed => {
                        if cell.is_mine {
                            Color::ACCENT_RED
                        } else {
                            MinGuiCompositor::shade(Color::WINDOW_BG, 10)
                        }
                    }
                    CellState::Flagged => Color::ACCENT_YELLOW,
                    CellState::Hidden => Color::TITLE_BAR_INACTIVE,
                };
                fb.draw_rect(cx, cy, cell_w, cell_h, bg);
                fb.draw_rect_outline(cx, cy, cell_w, cell_h, Color::BORDER);

                match cell.state {
                    CellState::Revealed => {
                        if cell.is_mine {
                            // Mayın: koyu daire.
                            let r_ = (cell_w.min(cell_h) / 3).max(2);
                            Self::fill_circle(
                                fb,
                                cx + cell_w / 2,
                                cy + cell_h / 2,
                                r_,
                                Color::DARK_BG,
                            );
                        } else if cell.adjacent > 0 {
                            let mut label = TextBuf::new();
                            label.push_usize(cell.adjacent as usize);
                            let lw = MinGuiCompositor::text_width(label.as_str(), 1);
                            let color = match cell.adjacent {
                                1 => Color::ACCENT_BLUE,
                                2 => Color::ACCENT_GREEN,
                                3 => Color::ACCENT_RED,
                                _ => Color::TEXT_MAIN,
                            };
                            MinGuiCompositor::draw_string(
                                fb,
                                cx + cell_w.saturating_sub(lw) / 2,
                                cy + cell_h.saturating_sub(MinGuiCompositor::text_height()) / 2,
                                label.as_str(),
                                color,
                            );
                        }
                    }
                    CellState::Flagged => {
                        // Bayrak: dikdörtgen işareti.
                        let fw = (cell_w / 3).max(2);
                        let fh = (cell_h / 2).max(2);
                        let fx = cx + cell_w / 2 - fw / 2;
                        let fy = cy + cell_h / 2 - fh / 2;
                        fb.draw_rect(fx, fy, fw, fh, Color::ACCENT_RED);
                    }
                    CellState::Hidden => {}
                }
            }
        }
    }

    fn render_chess(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let board = Self::board_rect(w, h);
        let (bx, by, bw, bh) = (x + board.0, y + board.1, board.2, board.3);
        let cell_w = bw / 8;
        let cell_h = bh / 8;
        fb.draw_rounded_rect(bx - 4, by - 4, bw + 8, bh + 8, 6, Color::BORDER);
        let check_square = if self.chess.is_in_check(self.chess.side_to_move) {
            self.chess.king_square(self.chess.side_to_move)
        } else {
            None
        };

        for r in 0..8 {
            for c in 0..8 {
                let cx = bx + c * cell_w;
                let cy = by + r * cell_h;
                let light_square = (r + c) % 2 == 0;
                let color = if light_square {
                    MinGuiCompositor::shade(Color::WINDOW_BG, 30)
                } else {
                    MinGuiCompositor::shade(Color::TITLE_BAR, -20)
                };
                fb.draw_rect(cx, cy, cell_w, cell_h, color);

                // Son hamlenin kareleri.
                if let Some(mv) = self.chess_last {
                    if mv.from == r * 8 + c || mv.to == r * 8 + c {
                        fb.draw_rect(
                            cx,
                            cy,
                            cell_w,
                            cell_h,
                            MinGuiCompositor::shade(Color::ACCENT_YELLOW, -70),
                        );
                    }
                }
                // Şah tehdidi altındaki şah.
                if check_square == Some((r, c)) {
                    fb.draw_rect_outline(cx, cy, cell_w, cell_h, Color::ACCENT_RED);
                    fb.draw_rect_outline(
                        cx + 1,
                        cy + 1,
                        cell_w.saturating_sub(2),
                        cell_h.saturating_sub(2),
                        Color::ACCENT_RED,
                    );
                }

                // Seçili kare.
                if self.chess_sel.selected == Some((r, c)) {
                    fb.draw_rect_outline(cx, cy, cell_w, cell_h, Color::ACCENT_GREEN);
                    fb.draw_rect_outline(
                        cx + 1,
                        cy + 1,
                        cell_w.saturating_sub(2),
                        cell_h.saturating_sub(2),
                        Color::ACCENT_GREEN,
                    );
                }
                // Geçerli hedefler.
                if self.chess_sel.targets.iter().any(|m| m.to == r * 8 + c) {
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

                // Taş: disk yerine klasik satranç silüeti. Beyaz taşlar koyu,
                // siyah taşlar açık konturla çizilir; böylece taş her iki kare
                // renginde de okunaklı kalır.
                if let Some(piece) = self.chess.cells[r][c] {
                    let icon_idx = match piece.kind {
                        PieceKind::King => 8usize,
                        PieceKind::Queen => 9,
                        PieceKind::Rook => 10,
                        PieceKind::Knight => 11,
                        PieceKind::Bishop => 12,
                        PieceKind::Pawn => 13,
                    };
                    let icon_sz = ((cell_w.min(cell_h) * 92) / 100).max(12);
                    let icon_x = cx + cell_w.saturating_sub(icon_sz) / 2;
                    let icon_y = cy + cell_h.saturating_sub(icon_sz) / 2;
                    let (fill, outline) = match piece.side {
                        ChessSide::White => (0xF4F1EA, 0x14141C),
                        ChessSide::Black => (0x1A1A22, 0xF4F1EA),
                    };
                    MinGuiCompositor::draw_chess_piece_styled(
                        fb, icon_x, icon_y, icon_sz, icon_idx, fill, outline,
                    );
                }
            }
        }

        // Piyon terfisi paneli.
        if self.chess_pending.is_some() {
            let rects = Self::promotion_rects(w, h);
            let (fx, fy, _, _) = rects[0];
            let (lx, _ly, lw, lh) = rects[3];
            let pad = sp(8);
            let label_h = sp(16);
            let px = x + fx.saturating_sub(pad);
            let py = y + fy.saturating_sub(pad + label_h);
            let pw = (lx + lw).saturating_sub(fx) + 2 * pad;
            let ph = lh + 2 * pad + label_h;
            fb.draw_rounded_rect(px, py, pw, ph, 8, Color::TITLE_BAR);
            fb.draw_rounded_rect_outline(px, py, pw, ph, 8, Color::ACCENT_YELLOW);
            MinGuiCompositor::draw_string(
                fb,
                px + pad,
                py + sp(3),
                crate::gui::ui_text("Terfi: taş seç", "Promotion: choose"),
                Color::TEXT_MAIN,
            );
            let kinds = [
                PieceKind::Queen,
                PieceKind::Rook,
                PieceKind::Bishop,
                PieceKind::Knight,
            ];
            let (fill, outline) = match self.chess.side_to_move {
                ChessSide::White => (0xF4F1EA, 0x14141C),
                ChessSide::Black => (0x1A1A22, 0xF4F1EA),
            };
            for (i, kind) in kinds.iter().enumerate() {
                let (rx, ry, rw, rh) = rects[i];
                fb.draw_rounded_rect(
                    x + rx,
                    y + ry,
                    rw,
                    rh,
                    5,
                    MinGuiCompositor::shade(Color::WINDOW_BG, 18),
                );
                fb.draw_rounded_rect_outline(x + rx, y + ry, rw, rh, 5, Color::BORDER);
                let icon_idx = match kind {
                    PieceKind::King => 8usize,
                    PieceKind::Queen => 9,
                    PieceKind::Rook => 10,
                    PieceKind::Knight => 11,
                    PieceKind::Bishop => 12,
                    PieceKind::Pawn => 13,
                };
                let icon_sz = ((rw.min(rh) * 86) / 100).max(12);
                let icon_x = x + rx + rw.saturating_sub(icon_sz) / 2;
                let icon_y = y + ry + rh.saturating_sub(icon_sz) / 2;
                MinGuiCompositor::draw_chess_piece_styled(
                    fb, icon_x, icon_y, icon_sz, icon_idx, fill, outline,
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Bilardo çizimi
    // -----------------------------------------------------------------------

    fn render_billiards(&self, fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        let board = Self::board_rect(w, h);
        let (bx, by, bw, bh) = (x + board.0, y + board.1, board.2, board.3);

        // Masa çerçevesi
        fb.draw_rounded_rect(bx - 6, by - 6, bw + 12, bh + 12, 8, Color::BORDER);

        // Masa yüzeyi (yeşil bez)
        let felt_color = 0x1B5E20; // Koyu yeşil
        fb.draw_rounded_rect(bx - 2, by - 2, bw + 4, bh + 4, 6, felt_color);

        // Çapalar (kenarlar)
        let cushion_w =
            bw as f32 * (super::game_core::BILLIARDS_CUSHION / super::game_core::BILLIARDS_TABLE_W);
        let cushion_w = billiards_max(cushion_w, 4.0) as usize;
        let cushion_h =
            bh as f32 * (super::game_core::BILLIARDS_CUSHION / super::game_core::BILLIARDS_TABLE_H);
        let cushion_h = billiards_max(cushion_h, 4.0) as usize;
        let cushion_color = 0x3E2723; // Koyu kahverengi

        // Üst çapa
        fb.draw_rect(bx, by - cushion_h, bw, cushion_h, cushion_color);
        // Alt çapa
        fb.draw_rect(bx, by + bh, bw, cushion_h, cushion_color);
        // Sol çapa
        fb.draw_rect(
            bx - cushion_w,
            by - cushion_h,
            cushion_w,
            bh + 2 * cushion_h,
            cushion_color,
        );
        // Sağ çapa
        fb.draw_rect(
            bx + bw,
            by - cushion_w,
            cushion_w,
            bh + 2 * cushion_h,
            cushion_color,
        );

        // Delikler (6 delik)
        let table_min_dim = billiards_min(
            super::game_core::BILLIARDS_TABLE_W,
            super::game_core::BILLIARDS_TABLE_H,
        );
        let pocket_r =
            (bw.min(bh) as f32 * (super::game_core::BILLIARDS_POCKET_R / table_min_dim)) as usize;
        let pocket_r = billiards_max(pocket_r as f32, 6.0) as usize;
        let pockets = [
            (bx - cushion_w / 2, by - cushion_h / 2),      // Sol üst
            (bx + bw / 2, by - cushion_h / 2),             // Üst orta
            (bx + bw + cushion_w / 2, by - cushion_h / 2), // Sağ üst
            (bx - cushion_w / 2, by + bh + cushion_h / 2), // Sol alt
            (bx + bw / 2, by + bh + cushion_h / 2),        // Alt orta
            (bx + bw + cushion_w / 2, by + bh + cushion_h / 2), // Sağ alt
        ];
        for (px, py) in pockets {
            Self::fill_circle(fb, px, py, pocket_r, 0x000000);
        }

        // Topları çiz (gfx3d ile 3D görünümlü)
        self.render_billiards_balls_3d(fb, bx, by, bw, bh);

        // Vuruş çizgisi (aiming)
        if self.billiards.state == super::game_core::BilliardsState::Aiming
            || self.billiards.state == super::game_core::BilliardsState::BallInHand
        {
            self.render_aim_line(fb, bx, by, bw, bh);
        }

        // Güç çubuğu
        self.render_power_bar(fb, bx, by, bw, bh);

        // Oyuncu bilgisi
        self.render_billiards_info(fb, x, by, bh);
    }

    /// 3D görünümlü topları çizer (gfx3d kullanarak basit küre projeksiyonu).
    fn render_billiards_balls_3d(
        &self,
        fb: &mut Framebuffer,
        bx: usize,
        by: usize,
        bw: usize,
        bh: usize,
    ) {
        use super::game_core::{BILLIARDS_BALL_R, BILLIARDS_TABLE_H, BILLIARDS_TABLE_W};

        // Basit 3D küre çizimi: üstten görünümde daire, ama gölgelendirme ile 3D hissi
        for ball in &self.billiards.balls {
            if !ball.in_play || ball.pocketed {
                continue;
            }

            // Dünya koordinatlarını ekran koordinatlarına çevir
            let scale_x = bw as f32 / BILLIARDS_TABLE_W;
            let scale_y = bh as f32 / BILLIARDS_TABLE_H;
            let scale_min = billiards_min(scale_x, scale_y);
            let screen_x = bx + (ball.x * scale_x) as usize;
            let screen_y = by + (ball.y * scale_y) as usize;
            let radius = ((BILLIARDS_BALL_R * scale_min) as usize).max(4);

            // Top rengi
            let base_color = ball.color();

            // 3D küre gölgelendirmesi: üstten ışık geliyormuş gibi
            // Merkez daha açık, kenarlar daha koyu
            let radius_isize = radius as isize;
            for dy in -radius_isize..=radius_isize {
                let row = screen_y as isize + dy;
                if row < 0 {
                    continue;
                }
                let dx_max = fast_sqrt((radius_isize * radius_isize - dy * dy) as f32) as isize;
                for dx in -dx_max..=dx_max {
                    let col = screen_x as isize + dx;
                    if col < 0 {
                        continue;
                    }
                    let dist_sq = dx * dx + dy * dy;
                    let radius_sq = radius_isize * radius_isize;
                    if dist_sq > radius_sq {
                        continue;
                    }
                    let dist = fast_sqrt(dist_sq as f32);
                    let norm_dist = dist / radius as f32;

                    // Küre normali (z bileşeni)
                    let nz_sq = 1.0 - norm_dist * norm_dist;
                    let nz = if nz_sq > 0.0 { fast_sqrt(nz_sq) } else { 0.0 };
                    // Işık yönü (yukarıdan hafif sağdan)
                    let lx = 0.3;
                    let ly = -0.2;
                    let lz = 0.9;
                    let l_len = fast_sqrt(lx * lx + ly * ly + lz * lz);
                    let diffuse_raw = lx * 0.0 + ly * 0.0 + lz * nz / l_len;
                    let diffuse = if diffuse_raw > 0.0 { diffuse_raw } else { 0.0 };
                    let ambient = 0.3;
                    let intensity_raw = ambient + 0.7 * diffuse;
                    let intensity = if intensity_raw < 1.0 {
                        intensity_raw
                    } else {
                        1.0
                    };

                    let r = (((base_color >> 16) & 0xFF) as f32 * intensity) as u32;
                    let g = (((base_color >> 8) & 0xFF) as f32 * intensity) as u32;
                    let b = ((base_color & 0xFF) as f32 * intensity) as u32;
                    let color = (r << 16) | (g << 8) | b;

                    // Çizgili toplar için beyaz bant
                    if ball.kind == BallKind::Stripe && ball.in_play {
                        // Basit bant: yatay eksende
                        let angle = ball.angle;
                        let ca = fast_cos(angle);
                        let sa = fast_sin(angle);
                        let _local_x = dx as f32 * ca - dy as f32 * sa;
                        let local_y = dx as f32 * sa + dy as f32 * ca;
                        let stripe_width = radius as f32 * 0.4;
                        if local_y.abs() < stripe_width {
                            let stripe_color = ball.stripe_color();
                            let sr = (stripe_color >> 16) & 0xFF;
                            let sg = (stripe_color >> 8) & 0xFF;
                            let sb = stripe_color & 0xFF;
                            fb.draw_rect(
                                col as usize,
                                row as usize,
                                1,
                                1,
                                (sr << 16) | (sg << 8) | sb,
                            );
                            continue;
                        }
                    }

                    // 8 topu için numara
                    if ball.kind == BallKind::Eight && dist < (radius as f32 * 0.3) {
                        fb.draw_rect(col as usize, row as usize, 1, 1, 0xFFFFFF);
                        continue;
                    }

                    fb.draw_rect(col as usize, row as usize, 1, 1, color);
                }
            }

            // Seçili top vurgusu
            if self.billiards_selected_ball
                == Some(
                    self.billiards
                        .balls
                        .iter()
                        .position(|b| core::ptr::eq(b, ball))
                        .unwrap_or(0),
                )
            {
                fb.draw_rect_outline(
                    screen_x.saturating_sub(radius + 2),
                    screen_y.saturating_sub(radius + 2),
                    radius * 2 + 4,
                    radius * 2 + 4,
                    Color::ACCENT_YELLOW,
                );
            }
        }
    }

    /// Vuruş yön çizgisi (aiming line).
    fn render_aim_line(&self, fb: &mut Framebuffer, bx: usize, by: usize, bw: usize, bh: usize) {
        use super::game_core::{BILLIARDS_TABLE_H, BILLIARDS_TABLE_W};

        let cue = self.billiards.cue_ball_ref();
        if !cue.in_play {
            return;
        }

        let scale_x = bw as f32 / BILLIARDS_TABLE_W;
        let scale_y = bh as f32 / BILLIARDS_TABLE_H;
        let start_x = bx + (cue.x * scale_x) as usize;
        let start_y = by + (cue.y * scale_y) as usize;

        let angle = self.billiards.aim_angle;
        let ca = fast_cos(angle);
        let sa = fast_sin(angle);
        let max_dist = (bw.max(bh) as f32) * 1.5;

        // Ana çizgi (kesikli)
        let steps = 30;
        for i in 0..steps {
            let t = i as f32 / steps as f32;
            let dist = t * max_dist;
            let x = start_x as f32 + ca * dist;
            let y = start_y as f32 + sa * dist;

            // Masa sınırları kontrolü
            if x < bx as f32 || x > (bx + bw) as f32 || y < by as f32 || y > (by + bh) as f32 {
                break;
            }

            if i % 3 < 2 {
                fb.draw_rect(x as usize, y as usize, 1, 1, 0xFFFFFF);
            }
        }

        // Hedef top vurgusu (ilk çarpışacak top)
        let mut closest_dist = f32::MAX;
        let mut target_ball: Option<&super::game_core::BilliardsBall> = None;
        for ball in &self.billiards.balls {
            if !ball.in_play || ball.pocketed || ball.kind == super::game_core::BallKind::Cue {
                continue;
            }
            let bx_ = ball.x;
            let by_ = ball.y;
            // Işın-top çarpışması
            let dx = bx_ - cue.x;
            let dy = by_ - cue.y;
            let proj = dx * ca + dy * sa;
            if proj > 0.0 {
                let perp_sq = dx * dx + dy * dy - proj * proj;
                let ball_r = super::game_core::BILLIARDS_BALL_R * 2.0;
                if perp_sq < ball_r * ball_r && proj < closest_dist {
                    closest_dist = proj;
                    target_ball = Some(ball);
                }
            }
        }

        if let Some(target) = target_ball {
            let tx = bx + (target.x * scale_x) as usize;
            let ty = by + (target.y * scale_y) as usize;
            let tr = ((super::game_core::BILLIARDS_BALL_R * scale_x.min(scale_y)) as usize).max(4);
            fb.draw_rect_outline(
                tx.saturating_sub(tr + 2),
                ty.saturating_sub(tr + 2),
                tr * 2 + 4,
                tr * 2 + 4,
                Color::ACCENT_GREEN,
            );
        }
    }

    /// Güç çubuğu (power bar).
    fn render_power_bar(&self, fb: &mut Framebuffer, bx: usize, by: usize, bw: usize, bh: usize) {
        if self.billiards.state != super::game_core::BilliardsState::Aiming
            && self.billiards.state != super::game_core::BilliardsState::BallInHand
        {
            return;
        }
        let bar_w = bw / 3;
        let bar_h = 8;
        let bar_x = bx + (bw - bar_w) / 2;
        let bar_y = by + bh + 20;
        fb.draw_rect(bar_x, bar_y, bar_w, bar_h, Color::TITLE_BAR);
        fb.draw_rect_outline(bar_x, bar_y, bar_w, bar_h, Color::BORDER);
        let fill_w = (bar_w as f32 * self.billiards.aim_power) as usize;
        let fill_color = if self.billiards.aim_power > 0.7 {
            Color::ACCENT_RED
        } else if self.billiards.aim_power > 0.4 {
            Color::ACCENT_YELLOW
        } else {
            Color::ACCENT_GREEN
        };
        fb.draw_rect(bar_x, bar_y, fill_w, bar_h, fill_color);
    }

    /// Bilardo bilgi paneli.
    fn render_billiards_info(&self, fb: &mut Framebuffer, x: usize, by: usize, bh: usize) {
        let info_y = by + bh + 40;
        let mut buf = TextBuf::new();

        // Mevcut oyuncu
        let player_str = match self.billiards.current_player {
            super::game_core::BilliardsPlayerType::Solids => {
                crate::gui::ui_text("Oyuncu: Düz (1-7)", "Player: Solids (1-7)")
            }
            super::game_core::BilliardsPlayerType::Stripes => {
                crate::gui::ui_text("Oyuncu: Çizgili (9-15)", "Player: Stripes (9-15)")
            }
        };
        buf.push_str(player_str);
        MinGuiCompositor::draw_string(fb, x + 10, info_y, buf.as_str(), Color::TEXT_MAIN);

        // Durum
        let mut buf2 = TextBuf::new();
        let state_str = match self.billiards.state {
            super::game_core::BilliardsState::Aiming => {
                crate::gui::ui_text("Vuruş hazırlanıyor...", "Aiming...")
            }
            super::game_core::BilliardsState::Shooting => {
                crate::gui::ui_text("Toplar hareket halinde", "Balls moving")
            }
            super::game_core::BilliardsState::BallInHand => crate::gui::ui_text(
                "Beyaz top elinizde - yerleştirin",
                "Ball in hand - place cue ball",
            ),
            super::game_core::BilliardsState::GameOver => {
                if let Some(w) = self.billiards.winner {
                    if w == 0 {
                        crate::gui::ui_text("OYUN BİTTİ - Düz kazandı!", "GAME OVER - Solids win!")
                    } else {
                        crate::gui::ui_text(
                            "OYUN BİTTİ - Çizgili kazandı!",
                            "GAME OVER - Stripes win!",
                        )
                    }
                } else {
                    crate::gui::ui_text("OYUN BİTTİ", "GAME OVER")
                }
            }
        };
        buf2.push_str(state_str);
        MinGuiCompositor::draw_string(fb, x + 10, info_y + 18, buf2.as_str(), Color::TEXT_MUTED);

        // Foul uyarısı
        if self.billiards.foul {
            let mut buf3 = TextBuf::new();
            buf3.push_str(crate::gui::ui_text(
                "FOUL! Beyaz top elinizde.",
                "FOUL! Ball in hand.",
            ));
            MinGuiCompositor::draw_string(
                fb,
                x + 10,
                info_y + 36,
                buf3.as_str(),
                Color::ACCENT_RED,
            );
        }
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

    // -----------------------------------------------------------------------
    // Tıklama
    // -----------------------------------------------------------------------

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

    /// Piksel koordinatını satranç karesine çevirir (8x8).
    fn cell_of_chess(
        px: usize,
        py: usize,
        rect: (usize, usize, usize, usize),
    ) -> Option<(usize, usize)> {
        let (x, y, w, h) = rect;
        if w == 0 || h == 0 || px < x || py < y || px >= x + w || py >= y + h {
            return None;
        }
        let cw = (w / 8).max(1);
        let ch = (h / 8).max(1);
        let col = ((px - x) / cw).min(7);
        let row = ((py - y) / ch).min(7);
        Some((row, col))
    }

    /// Pencere içi tıklamayı işler. Yeniden çizim gerekiyorsa `true` döner.
    pub fn click(&mut self, px: usize, py: usize, w: usize, h: usize) -> bool {
        // Terfi paneli açıkken tıklamalar yalnızca panele gider.
        if self.chess_pending.is_some() {
            return self.click_promotion(px, py, w, h);
        }
        // Sekmeler.
        for tab in 0..TAB_COUNT {
            if crate::gui::in_rect(px, py, Self::tab_rect(tab)) {
                self.select_tab(tab);
                return true;
            }
        }
        if crate::gui::in_rect(px, py, Self::sound_rect()) {
            self.toggle_sound();
            return true;
        }
        if self.tab == TAB_MINES {
            for level in 0..3 {
                if crate::gui::in_rect(px, py, Self::mine_level_rect(level)) {
                    self.set_mine_level(level);
                    return true;
                }
            }
        }
        if self.tab == TAB_GAME {
            for level in 0..3 {
                if crate::gui::in_rect(px, py, Self::ai_level_rect(level)) {
                    self.set_ai_level(level);
                    return true;
                }
            }
        }
        if (self.tab == TAB_GAME || self.tab == TAB_CHESS)
            && crate::gui::in_rect(px, py, Self::undo_rect(self.tab))
        {
            self.undo_current();
            return true;
        }
        match self.tab {
            TAB_GAME => {
                if crate::gui::in_rect(px, py, Self::new_game_rect()) {
                    let player_light = self.player_light;
                    self.new_game(player_light);
                    return true;
                }
                if let Some(sq) = Self::cell_of_checkers(px, py, Self::board_rect(w, h)) {
                    self.click_cell(sq);
                    return true;
                }
                false
            }
            TAB_MINES => {
                if crate::gui::in_rect(px, py, Self::new_game_rect()) {
                    self.reset_mines();
                    return true;
                }
                let rect = Self::board_rect(w, h);
                if let Some((r, c)) =
                    Self::cell_of_mines(px, py, rect, self.mines.rows, self.mines.cols)
                {
                    self.click_mine_cell(r, c);
                    return true;
                }
                false
            }
            TAB_CHESS => {
                if crate::gui::in_rect(px, py, Self::new_game_rect()) {
                    self.reset_chess();
                    return true;
                }
                let rect = Self::board_rect(w, h);
                if let Some((r, c)) = Self::cell_of_chess(px, py, rect) {
                    self.click_chess_cell(r, c);
                    return true;
                }
                false
            }
            TAB_BILLIARDS => self.click_billiards(px, py, w, h),
            _ => false,
        }
    }

    /// Bilardo tıklama işleme.
    fn click_billiards(&mut self, px: usize, py: usize, w: usize, h: usize) -> bool {
        use super::game_core::{BilliardsState, BILLIARDS_TABLE_H, BILLIARDS_TABLE_W};

        // Yeni oyun butonu
        if crate::gui::in_rect(px, py, Self::new_game_rect()) {
            self.billiards.reset();
            return true;
        }

        let board = Self::board_rect(w, h);
        let (bx, by, bw, bh) = (board.0, board.1, board.2, board.3);

        // Masa alanına tıklama
        if px >= bx && py >= by && px < bx + bw && py < by + bh {
            // Ekran koordinatlarını dünya koordinatlarına çevir
            let world_x = (px - bx) as f32 / bw as f32 * BILLIARDS_TABLE_W;
            let world_y = (py - by) as f32 / bh as f32 * BILLIARDS_TABLE_H;

            match self.billiards.state {
                BilliardsState::BallInHand => {
                    // Çap topunu yerleştir
                    self.billiards.place_cue_ball(world_x, world_y);
                    return true;
                }
                BilliardsState::Aiming => {
                    // Vuruş yönünü ayarla (çap topundan tıklanan noktaya)
                    let cue = self.billiards.cue_ball_ref();
                    if cue.in_play {
                        let dx = world_x - cue.x;
                        let dy = world_y - cue.y;
                        let angle = fast_atan2(dy, dx);
                        let dist = fast_sqrt(dx * dx + dy * dy);
                        let max_dist = billiards_max(BILLIARDS_TABLE_W, BILLIARDS_TABLE_H);
                        let power = billiards_clamp(dist / max_dist, 0.1, 1.0);
                        self.billiards.set_aim(angle, power);
                        return true;
                    }
                }
                _ => {}
            }
        }

        // Güç çubuğuna tıklama (aiming sırasında)
        if self.billiards.state == BilliardsState::Aiming {
            let bar_w = bw / 3;
            let bar_h = 8;
            let bar_x = bx + (bw - bar_w) / 2;
            let bar_y = by + bh + 20;
            if px >= bar_x && px < bar_x + bar_w && py >= bar_y && py < bar_y + bar_h {
                let power = (px - bar_x) as f32 / bar_w as f32;
                self.billiards
                    .set_aim(self.billiards.aim_angle, power.clamp(0.1, 1.0));
                return true;
            }
        }

        false
    }

    /// Klavye kısayolları: 1-4 bölüm, N yeni oyun, S ses, U geri al.
    pub fn handle_key(&mut self, ch: char) -> bool {
        // Terfi paneli açıkken Q/R/B/N ile taş seç, Esc ile iptal et.
        if self.chess_pending.is_some() {
            match ch {
                'q' | 'Q' => self.choose_promotion(PieceKind::Queen),
                'r' | 'R' => self.choose_promotion(PieceKind::Rook),
                'b' | 'B' => self.choose_promotion(PieceKind::Bishop),
                'n' | 'N' => self.choose_promotion(PieceKind::Knight),
                '\x1b' => self.cancel_promotion(),
                _ => {}
            }
            return true;
        }
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
                    TAB_MINES => self.reset_mines(),
                    TAB_CHESS => self.reset_chess(),
                    TAB_BILLIARDS => self.billiards.reset(),
                    _ => {}
                }
                true
            }
            's' | 'S' => {
                self.toggle_sound();
                true
            }
            'u' | 'U' => {
                self.undo_current();
                true
            }
            ' ' => {
                // Space: bilardo vuruşu
                if self.tab == TAB_BILLIARDS
                    && (self.billiards.state == super::game_core::BilliardsState::Aiming
                        || self.billiards.state == super::game_core::BilliardsState::BallInHand)
                {
                    if self.billiards.state == super::game_core::BilliardsState::BallInHand {
                        // Çap topu masa ortasına yerleştir
                        self.billiards.place_cue_ball(
                            super::game_core::BILLIARDS_TABLE_W * 0.25,
                            super::game_core::BILLIARDS_TABLE_H * 0.5,
                        );
                    } else {
                        self.billiards.shoot();
                    }
                    true
                } else {
                    false
                }
            }
            '1'..='4' => {
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
        if self.muted {
            Sound::None
        } else {
            sound
        }
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
        app.click_cell(game_core::idx(5, 2));
        assert_eq!(app.selected, Some(game_core::idx(5, 2)));
        assert!(!app.targets.is_empty());
    }

    #[test]
    fn mines_reveal_safe_cell() {
        let mut app = GameApp::new();
        app.select_tab(TAB_MINES);
        app.click_mine_cell(4, 4);
        assert!(app.mines.state == GameState::Playing || app.mines.state == GameState::Won);
        assert!(app.mines.revealed > 0);
    }

    #[test]
    fn chess_white_pawn_can_move() {
        let mut app = GameApp::new();
        app.select_tab(TAB_CHESS);
        // Beyaz piyon (6, 4) -> (5, 4) veya (4, 4).
        app.click_chess_cell(6, 4);
        assert_eq!(app.chess_sel.selected, Some((6, 4)));
        assert!(!app.chess_sel.targets.is_empty());
    }
}
