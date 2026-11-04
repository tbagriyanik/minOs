//! "Oyunlar" uygulaması — 2D oyunlar tek pencerede.
//!
//! Üç bölüm tek pencerede sunulur:
//! * **Dama** — Türk/uluslararası kurallı, yapay zekâya karşı 2D dama.
//! * **Mayın Tarlası** — klasik mayın tarlası; hücreleri aç, bayrak koy, tüm
//!   güvenli hücreleri açarak kazan.
//! * **Satranç** — 8x8 satranç tahtası, temel hamle kurallarıyla iki oyunculu.
//!
//! Tüm oyun mantığı bu modül içinde yer alır; `game_core` sadece dama motoru
//! için korunur. Ses, ana döngüde (`GameApp::sound_pending`) PC Speaker / HDA
//! üzerinden çalınır.

use crate::drivers::video::Framebuffer;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};

use super::game_core::{self, Board, Move};

/// Bölüm sekmesi sayısı.
pub const TAB_COUNT: usize = 3;
const TAB_GAME: usize = 0;
const TAB_MINES: usize = 1;
const TAB_CHESS: usize = 2;

const TAB_Y: usize = 28;
const TAB_H: usize = 24;
const TAB_W: usize = 96;

const STATUS_H: usize = 26;

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

/// Oyun durumu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    Playing,
    Won,
    Lost,
}

/// Mayın Tarlası hücre durumu.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CellState {
    #[default]
    Hidden,
    Revealed,
    Flagged,
}

/// Mayın Tarlası hücresi.
#[derive(Clone, Copy, Default)]
pub struct MineCell {
    pub state: CellState,
    pub is_mine: bool,
    pub adjacent: u8,
}

/// Mayın Tarlası oyunu.
pub struct Minesweeper {
    pub rows: usize,
    pub cols: usize,
    pub mine_count: usize,
    pub cells: [MineCell; 81], // 9x9 max
    pub revealed: usize,
    pub flagged: usize,
    pub state: GameState,
    pub first_click: bool,
    pub seed: u64,
}

impl Minesweeper {
    pub fn new() -> Self {
        Self {
            rows: 9,
            cols: 9,
            mine_count: 10,
            cells: [MineCell::default(); 81],
            revealed: 0,
            flagged: 0,
            state: GameState::Playing,
            first_click: true,
            seed: 0x5EED_1234,
        }
    }

    pub fn reset(&mut self) {
        self.cells = [MineCell::default(); 81];
        self.revealed = 0;
        self.flagged = 0;
        self.state = GameState::Playing;
        self.first_click = true;
    }

    fn rng_next(&mut self) -> u64 {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        self.seed
    }

    /// Mayınları yerleştir (ilk tıklama hücresi ve komşuları mayın olmasın).
    fn place_mines(&mut self, safe_r: usize, safe_c: usize) {
        let mut placed = 0;
        while placed < self.mine_count {
            let r = (self.rng_next() as usize) % self.rows;
            let c = (self.rng_next() as usize) % self.cols;
            let idx = r * self.cols + c;
            if (r == safe_r && c == safe_c) || self.cells[idx].is_mine {
                continue;
            }
            // Komşu hücreleri de güvenli tut (3x3 etrafı).
            let mut safe = true;
            for dr in 0isize..=2 {
                for dc in 0isize..=2 {
                    let nr = r as isize + dr - 1;
                    let nc = c as isize + dc - 1;
                    if nr == safe_r as isize && nc == safe_c as isize {
                        safe = false;
                    }
                }
            }
            if !safe {
                continue;
            }
            self.cells[idx].is_mine = true;
            placed += 1;
        }
        // Komşu mayın sayılarını hesapla.
        for r in 0..self.rows {
            for c in 0..self.cols {
                let idx = r * self.cols + c;
                if self.cells[idx].is_mine {
                    continue;
                }
                let mut cnt = 0u8;
                for dr in 0isize..=2 {
                    for dc in 0isize..=2 {
                        if dr == 1 && dc == 1 {
                            continue;
                        }
                        let nr = r as isize + dr - 1;
                        let nc = c as isize + dc - 1;
                        if nr < 0 || nr >= self.rows as isize || nc < 0 || nc >= self.cols as isize {
                            continue;
                        }
                        let nidx = nr as usize * self.cols + nc as usize;
                        if self.cells[nidx].is_mine {
                            cnt += 1;
                        }
                    }
                }
                self.cells[idx].adjacent = cnt;
            }
        }
    }

    /// Hücreyi açar. Mayına basarsa kaybet, boşsa komşuları da açar.
    pub fn reveal(&mut self, r: usize, c: usize) {
        if r >= self.rows || c >= self.cols {
            return;
        }
        let idx = r * self.cols + c;
        let cell = &self.cells[idx];
        if cell.state != CellState::Hidden {
            return;
        }
        if self.first_click {
            self.first_click = false;
            self.place_mines(r, c);
            // İlk tıklama mayın olmamalı; eğer mayın çıktıysa taşı.
            if self.cells[idx].is_mine {
                self.cells[idx].is_mine = false;
                // Komşuları yeniden hesapla.
                for rr in 0..self.rows {
                    for cc in 0..self.cols {
                        let i = rr * self.cols + cc;
                        if self.cells[i].is_mine {
                            continue;
                        }
                        let mut cnt = 0u8;
                        for dr in 0isize..=2 {
                            for dc in 0isize..=2 {
                                if dr == 1 && dc == 1 {
                                    continue;
                                }
                                let nr = rr as isize + dr - 1;
                                let nc = cc as isize + dc - 1;
                                if nr < 0 || nr >= self.rows as isize || nc < 0 || nc >= self.cols as isize {
                                    continue;
                                }
                                let ni = nr as usize * self.cols + nc as usize;
                                if self.cells[ni].is_mine {
                                    cnt += 1;
                                }
                            }
                        }
                        self.cells[i].adjacent = cnt;
                    }
                }
            }
        }
        self.cells[idx].state = CellState::Revealed;
        self.revealed += 1;
        if self.cells[idx].is_mine {
            self.state = GameState::Lost;
            // Tüm mayınları göster.
            for rr in 0..self.rows {
                for cc in 0..self.cols {
                    let i = rr * self.cols + cc;
                    if self.cells[i].is_mine {
                        self.cells[i].state = CellState::Revealed;
                    }
                }
            }
            return;
        }
        if self.cells[idx].adjacent == 0 {
            for dr in 0isize..=2 {
                for dc in 0isize..=2 {
                    if dr == 1 && dc == 1 {
                        continue;
                    }
                    let nr = r as isize + dr - 1;
                    let nc = c as isize + dc - 1;
                    if nr < 0 || nr >= self.rows as isize || nc < 0 || nc >= self.cols as isize {
                        continue;
                    }
                    self.reveal(nr as usize, nc as usize);
                }
            }
        }
        self.check_win();
    }

    pub fn toggle_flag(&mut self, r: usize, c: usize) {
        if r >= self.rows || c >= self.cols {
            return;
        }
        let idx = r * self.cols + c;
        let cell = &mut self.cells[idx];
        if cell.state == CellState::Hidden {
            cell.state = CellState::Flagged;
            self.flagged += 1;
        } else if cell.state == CellState::Flagged {
            cell.state = CellState::Hidden;
            self.flagged = self.flagged.saturating_sub(1);
        }
    }

    fn check_win(&mut self) {
        if self.state != GameState::Playing {
            return;
        }
        let safe_cells = self.rows * self.cols - self.mine_count;
        if self.revealed >= safe_cells {
            self.state = GameState::Won;
        }
    }
}

/// Satranç taşı türleri.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum PieceKind {
    #[default]
    Pawn,
    Rook,
    Knight,
    Bishop,
    Queen,
    King,
}

/// Satranç rengi.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ChessSide {
    #[default]
    White,
    Black,
}

/// Satranç taşı.
#[derive(Clone, Copy, Default)]
pub struct ChessPiece {
    pub kind: PieceKind,
    pub side: ChessSide,
    pub has_moved: bool,
}

/// Satranç hamlesi.
#[derive(Clone, Copy)]
pub struct ChessMove {
    pub from: usize,
    pub to: usize,
    pub promotion: Option<PieceKind>,
}

/// Satranç tahtası (8x8).
pub struct ChessBoard {
    pub cells: [[Option<ChessPiece>; 8]; 8],
    pub side_to_move: ChessSide,
    pub state: GameState,
    pub halfmove_clock: u32,
    pub fullmove_number: u32,
}

impl ChessBoard {
    pub fn new() -> Self {
        let mut cells = [[None; 8]; 8];
        // Siyah arka sıra (satır 0).
        cells[0][0] = Some(ChessPiece { kind: PieceKind::Rook, side: ChessSide::Black, has_moved: false });
        cells[0][1] = Some(ChessPiece { kind: PieceKind::Knight, side: ChessSide::Black, has_moved: false });
        cells[0][2] = Some(ChessPiece { kind: PieceKind::Bishop, side: ChessSide::Black, has_moved: false });
        cells[0][3] = Some(ChessPiece { kind: PieceKind::Queen, side: ChessSide::Black, has_moved: false });
        cells[0][4] = Some(ChessPiece { kind: PieceKind::King, side: ChessSide::Black, has_moved: false });
        cells[0][5] = Some(ChessPiece { kind: PieceKind::Bishop, side: ChessSide::Black, has_moved: false });
        cells[0][6] = Some(ChessPiece { kind: PieceKind::Knight, side: ChessSide::Black, has_moved: false });
        cells[0][7] = Some(ChessPiece { kind: PieceKind::Rook, side: ChessSide::Black, has_moved: false });
        for c in 0..8 {
            cells[1][c] = Some(ChessPiece { kind: PieceKind::Pawn, side: ChessSide::Black, has_moved: false });
        }
        // Beyaz arka sıra (satır 7).
        cells[7][0] = Some(ChessPiece { kind: PieceKind::Rook, side: ChessSide::White, has_moved: false });
        cells[7][1] = Some(ChessPiece { kind: PieceKind::Knight, side: ChessSide::White, has_moved: false });
        cells[7][2] = Some(ChessPiece { kind: PieceKind::Bishop, side: ChessSide::White, has_moved: false });
        cells[7][3] = Some(ChessPiece { kind: PieceKind::Queen, side: ChessSide::White, has_moved: false });
        cells[7][4] = Some(ChessPiece { kind: PieceKind::King, side: ChessSide::White, has_moved: false });
        cells[7][5] = Some(ChessPiece { kind: PieceKind::Bishop, side: ChessSide::White, has_moved: false });
        cells[7][6] = Some(ChessPiece { kind: PieceKind::Knight, side: ChessSide::White, has_moved: false });
        cells[7][7] = Some(ChessPiece { kind: PieceKind::Rook, side: ChessSide::White, has_moved: false });
        for c in 0..8 {
            cells[6][c] = Some(ChessPiece { kind: PieceKind::Pawn, side: ChessSide::White, has_moved: false });
        }
        Self {
            cells,
            side_to_move: ChessSide::White,
            state: GameState::Playing,
            halfmove_clock: 0,
            fullmove_number: 1,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    fn in_bounds(r: isize, c: isize) -> bool {
        r >= 0 && r < 8 && c >= 0 && c < 8
    }

    /// Verilen karedeki taşı hareket ettirebilecek hedefleri listeler (şah kontrolü hariç basit).
    fn generate_pseudo_legal(&self, r: usize, c: usize, out: &mut alloc::vec::Vec<ChessMove>) {
        let piece = match self.cells[r][c] {
            Some(p) => p,
            None => return,
        };
        match piece.kind {
            PieceKind::Pawn => {
                let dir = if piece.side == ChessSide::White { -1 } else { 1 };
                let start_row = if piece.side == ChessSide::White { 6 } else { 1 };
                // İleri adım.
                let nr = r as isize + dir;
                if Self::in_bounds(nr, c as isize) && self.cells[nr as usize][c].is_none() {
                    out.push(ChessMove {
                        from: r * 8 + c,
                        to: nr as usize * 8 + c,
                        promotion: None,
                    });
                    // İki kare ileri.
                    if r == start_row {
                        let nr2 = r as isize + 2 * dir;
                        if Self::in_bounds(nr2, c as isize) && self.cells[nr2 as usize][c].is_none() {
                            out.push(ChessMove {
                                from: r * 8 + c,
                                to: nr2 as usize * 8 + c,
                                promotion: None,
                            });
                        }
                    }
                }
                // Çapraz yeme.
                for dc in [-1isize, 1] {
                    let nr = r as isize + dir;
                    let nc = c as isize + dc;
                    if Self::in_bounds(nr, nc) {
                        if let Some(p) = self.cells[nr as usize][nc as usize] {
                            if p.side != piece.side {
                                out.push(ChessMove {
                                    from: r * 8 + c,
                                    to: nr as usize * 8 + nc as usize,
                                    promotion: None,
                                });
                            }
                        }
                    }
                }
            }
            PieceKind::Rook => {
                let dirs = [(-1isize, 0), (1, 0), (0, -1), (0, 1)];
                for (dr, dc) in dirs.iter() {
                    let mut nr = r as isize;
                    let mut nc = c as isize;
                    for _ in 0..7 {
                        nr += dr;
                        nc += dc;
                        if !Self::in_bounds(nr, nc) {
                            break;
                        }
                        match self.cells[nr as usize][nc as usize] {
                            None => {
                                out.push(ChessMove {
                                    from: r * 8 + c,
                                    to: nr as usize * 8 + nc as usize,
                                    promotion: None,
                                });
                            }
                            Some(p) => {
                                if p.side != piece.side {
                                    out.push(ChessMove {
                                        from: r * 8 + c,
                                        to: nr as usize * 8 + nc as usize,
                                        promotion: None,
                                    });
                                }
                                break;
                            }
                        }
                    }
                }
            }
            PieceKind::Knight => {
                let dirs = [(-2, -1), (-2, 1), (-1, -2), (-1, 2), (1, -2), (1, 2), (2, -1), (2, 1)];
                for (dr, dc) in dirs.iter() {
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    if !Self::in_bounds(nr, nc) {
                        continue;
                    }
                    match self.cells[nr as usize][nc as usize] {
                        None => {
                            out.push(ChessMove {
                                from: r * 8 + c,
                                to: nr as usize * 8 + nc as usize,
                                promotion: None,
                            });
                        }
                        Some(p) => {
                            if p.side != piece.side {
                                out.push(ChessMove {
                                    from: r * 8 + c,
                                    to: nr as usize * 8 + nc as usize,
                                    promotion: None,
                                });
                            }
                        }
                    }
                }
            }
            PieceKind::Bishop => {
                let dirs = [(-1, -1), (-1, 1), (1, -1), (1, 1)];
                for (dr, dc) in dirs.iter() {
                    let mut nr = r as isize;
                    let mut nc = c as isize;
                    for _ in 0..7 {
                        nr += dr;
                        nc += dc;
                        if !Self::in_bounds(nr, nc) {
                            break;
                        }
                        match self.cells[nr as usize][nc as usize] {
                            None => {
                                out.push(ChessMove {
                                    from: r * 8 + c,
                                    to: nr as usize * 8 + nc as usize,
                                    promotion: None,
                                });
                            }
                            Some(p) => {
                                if p.side != piece.side {
                                    out.push(ChessMove {
                                        from: r * 8 + c,
                                        to: nr as usize * 8 + nc as usize,
                                        promotion: None,
                                    });
                                }
                                break;
                            }
                        }
                    }
                }
            }
            PieceKind::Queen => {
                let dirs = [
                    (-1, -1), (-1, 0), (-1, 1),
                    (0, -1), (0, 1),
                    (1, -1), (1, 0), (1, 1),
                ];
                for (dr, dc) in dirs.iter() {
                    let mut nr = r as isize;
                    let mut nc = c as isize;
                    for _ in 0..7 {
                        nr += dr;
                        nc += dc;
                        if !Self::in_bounds(nr, nc) {
                            break;
                        }
                        match self.cells[nr as usize][nc as usize] {
                            None => {
                                out.push(ChessMove {
                                    from: r * 8 + c,
                                    to: nr as usize * 8 + nc as usize,
                                    promotion: None,
                                });
                            }
                            Some(p) => {
                                if p.side != piece.side {
                                    out.push(ChessMove {
                                        from: r * 8 + c,
                                        to: nr as usize * 8 + nc as usize,
                                        promotion: None,
                                    });
                                }
                                break;
                            }
                        }
                    }
                }
            }
            PieceKind::King => {
                let dirs = [
                    (-1, -1), (-1, 0), (-1, 1),
                    (0, -1), (0, 1),
                    (1, -1), (1, 0), (1, 1),
                ];
                for (dr, dc) in dirs.iter() {
                    let nr = r as isize + dr;
                    let nc = c as isize + dc;
                    if !Self::in_bounds(nr, nc) {
                        continue;
                    }
                    match self.cells[nr as usize][nc as usize] {
                        None => {
                            out.push(ChessMove {
                                from: r * 8 + c,
                                to: nr as usize * 8 + nc as usize,
                                promotion: None,
                            });
                        }
                        Some(p) => {
                            if p.side != piece.side {
                                out.push(ChessMove {
                                    from: r * 8 + c,
                                    to: nr as usize * 8 + nc as usize,
                                    promotion: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    /// Tüm pseudo-legal hamleleri listeler.
    pub fn generate_all(&self, side: ChessSide, out: &mut alloc::vec::Vec<ChessMove>) {
        for r in 0..8 {
            for c in 0..8 {
                if let Some(p) = self.cells[r][c] {
                    if p.side == side {
                        self.generate_pseudo_legal(r, c, out);
                    }
                }
            }
        }
    }

    /// Hamleyi uygular (şah kontrolü kontrolü yapılmadan basit uygulama).
    pub fn apply(&mut self, mv: &ChessMove) -> bool {
        let (fr, fc) = (mv.from / 8, mv.from % 8);
        let (tr, tc) = (mv.to / 8, mv.to % 8);
        let piece = match self.cells[fr][fc] {
            Some(p) => p,
            None => return false,
        };
        if piece.side != self.side_to_move {
            return false;
        }
        // Hedef kendi taşı mı?
        if let Some(p) = self.cells[tr][tc] {
            if p.side == piece.side {
                return false;
            }
        }
        // Geçerli mi?
        let mut legal: alloc::vec::Vec<ChessMove> = alloc::vec::Vec::new();
        self.generate_pseudo_legal(fr, fc, &mut legal);
        if !legal.iter().any(|m| m.to == mv.to) {
            return false;
        }
        self.cells[tr][tc] = Some(ChessPiece {
            kind: piece.kind,
            side: piece.side,
            has_moved: true,
        });
        self.cells[fr][fc] = None;
        self.side_to_move = match piece.side {
            ChessSide::White => ChessSide::Black,
            ChessSide::Black => ChessSide::White,
        };
        self.fullmove_number += 1;
        true
    }

    /// Şahın konumu.
    fn find_king(&self, side: ChessSide) -> Option<(usize, usize)> {
        for r in 0..8 {
            for c in 0..8 {
                if let Some(p) = self.cells[r][c] {
                    if p.kind == PieceKind::King && p.side == side {
                        return Some((r, c));
                    }
                }
            }
        }
        None
    }

    /// Belirli tarafın şahı tehdit altında mı?
    pub fn is_in_check(&self, side: ChessSide) -> bool {
        let king = match self.find_king(side) {
            Some(k) => k,
            None => return false,
        };
        let enemy = match side {
            ChessSide::White => ChessSide::Black,
            ChessSide::Black => ChessSide::White,
        };
        for r in 0..8 {
            for c in 0..8 {
                if let Some(p) = self.cells[r][c] {
                    if p.side == enemy {
                        let mut targets: alloc::vec::Vec<ChessMove> = alloc::vec::Vec::new();
                        self.generate_pseudo_legal(r, c, &mut targets);
                        if targets.iter().any(|m| m.to == king.0 * 8 + king.1) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
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
    // --- Mayın Tarlası ---
    pub mines: Minesweeper,
    // --- Satranç ---
    pub chess: ChessBoard,
    pub chess_sel: ChessSel,
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
            mines: Minesweeper::new(),
            chess: ChessBoard::new(),
            chess_sel: ChessSel {
                selected: None,
                targets: alloc::vec::Vec::new(),
            },
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
            ("Mayın Tarlası", "Minesweeper"),
            ("Satranç", "Chess"),
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

    /// Oyun alanı: sekmenin altında kalan kare alan.
    pub fn board_rect(win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
        let top = sp(TAB_Y + TAB_H + 8);
        let avail_h = win_h.saturating_sub(top + sp(STATUS_H + 6) + sp(34));
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

    // -----------------------------------------------------------------------
    // Dama girdisi
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Mayın Tarlası girdisi
    // -----------------------------------------------------------------------

    pub fn click_mine_cell(&mut self, r: usize, c: usize) {
        if self.tab != TAB_MINES {
            return;
        }
        if self.mines.state != GameState::Playing {
            return;
        }
        self.mines.reveal(r, c);
        self.sound_pending = if self.mines.state == GameState::Won {
            Sound::Win
        } else if self.mines.state == GameState::Lost {
            Sound::Lose
        } else {
            Sound::Move
        };
    }

    pub fn right_click_mine_cell(&mut self, r: usize, c: usize) {
        if self.tab != TAB_MINES {
            return;
        }
        if self.mines.state != GameState::Playing {
            return;
        }
        self.mines.toggle_flag(r, c);
        self.sound_pending = Sound::Tick;
    }

    pub fn reset_mines(&mut self) {
        self.mines.reset();
        self.sound_pending = Sound::Tick;
    }

    // -----------------------------------------------------------------------
    // Satranç girdisi
    // -----------------------------------------------------------------------

    pub fn click_chess_cell(&mut self, r: usize, c: usize) {
        if self.tab != TAB_CHESS {
            return;
        }
        if self.chess.state != GameState::Playing {
            return;
        }
        let idx = r * 8 + c;
        // Seçili karenin hedefi mi?
        if let Some(_sel) = self.chess_sel.selected {
            if let Some(mv) = self.chess_sel.targets.iter().find(|m| m.to == idx).copied() {
                if self.chess.apply(&mv) {
                    self.sound_pending = Sound::Move;
                    if self.chess.is_in_check(self.chess.side_to_move) {
                        self.message = crate::gui::ui_text("Şah!", "Check!");
                    }
                }
                self.chess_sel.selected = None;
                self.chess_sel.targets.clear();
                return;
            }
        }
        // Yeni taş seçimi (sıraındaki oyuncunun taşı).
        self.chess_sel.selected = None;
        self.chess_sel.targets.clear();
        if let Some(piece) = self.chess.cells[r][c] {
            let is_player = match self.chess.side_to_move {
                ChessSide::White => piece.side == ChessSide::White,
                ChessSide::Black => piece.side == ChessSide::Black,
            };
            if is_player {
                let mut list: alloc::vec::Vec<ChessMove> = alloc::vec::Vec::new();
                self.chess.generate_pseudo_legal(r, c, &mut list);
                self.chess_sel.selected = Some((r, c));
                self.chess_sel.targets = list;
                self.sound_pending = Sound::Tick;
            }
        }
    }

    pub fn reset_chess(&mut self) {
        self.chess.reset();
        self.chess_sel.selected = None;
        self.chess_sel.targets.clear();
        self.message = "";
        self.sound_pending = Sound::Tick;
    }

    // -----------------------------------------------------------------------
    // Tick
    // -----------------------------------------------------------------------

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
            _ => {}
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
            _ => {}
        }
        self.render_status(fb, x, y, w, h);
    }

    fn render_tabs(&self, fb: &mut Framebuffer, x: usize, y: usize, _w: usize) {
        let names = Self::tab_names();
        let tab_icons = [6usize, 7, 8]; // checker, mine, chess king
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
        }
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
            }
            TAB_MINES => {
                left.push_str(crate::gui::ui_text("Mayın: ", "Mines: "));
                left.push_usize(self.mines.mine_count.saturating_sub(self.mines.flagged));
                left.push_str("   ");
                left.push_str(crate::gui::ui_text("Açık: ", "Revealed: "));
                left.push_usize(self.mines.revealed);
                left.push_str("   ");
                match self.mines.state {
                    GameState::Won => left.push_str(crate::gui::ui_text("Kazandınız!", "You win!")),
                    GameState::Lost => left.push_str(crate::gui::ui_text("Mayına bastınız!", "Mine hit!")),
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
                if self.chess.state == GameState::Won {
                    left.push_str(" - ");
                    left.push_str(crate::gui::ui_text("Kazandı!", "Won!"));
                }
                if self.chess.is_in_check(self.chess.side_to_move) {
                    left.push_str(" - ");
                    left.push_str(crate::gui::ui_text("ŞAH!", "CHECK!"));
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
            (x + sp(4), sy, w.saturating_sub(sp(8)), sp(STATUS_H)),
        );
        let hint = match self.tab {
            TAB_GAME => crate::gui::ui_text("Tıkla taşı seç, hedefe tıkla oynayın", "Click to select, click target to move"),
            TAB_MINES => crate::gui::ui_text("Sol tık aç, sağ tık bayrak", "Left click reveal, right click flag"),
            TAB_CHESS => crate::gui::ui_text("Taşı seç, hedefe tıkla oynayın", "Select piece, click target to move"),
            _ => crate::gui::ui_text("", ""),
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            x + sp(10),
            sy + sp(7) + sp(14),
            hint,
            Color::TEXT_MUTED,
            (x + sp(4), sy, w.saturating_sub(sp(8)), sp(STATUS_H)),
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
                            Self::fill_circle(fb, cx + cell_w / 2, cy + cell_h / 2, r_, Color::DARK_BG);
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
                if let Some((r, c)) = Self::cell_of_mines(px, py, rect, self.mines.rows, self.mines.cols) {
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
            _ => false,
        }
    }

    /// Klavye kısayolları: 1-3 bölüm, N yeni oyun, S ses testi.
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
                    TAB_MINES => self.reset_mines(),
                    TAB_CHESS => self.reset_chess(),
                    _ => {}
                }
                true
            }
            's' | 'S' => {
                self.sound_pending = Sound::Win;
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
