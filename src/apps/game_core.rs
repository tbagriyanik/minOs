//! Oyun uygulamasının çekirdekten bağımsız saf oyun mantığı.
//!
//! Bu modül yalnızca `core` kullanır; hem çekirdek içinde (GameApp) hem de host
//! testlerinde (`tests/host_feature_tests.rs`) aynı kod doğrulanır.
//!
//! İçerik:
//! * Dama kural motoru (Türk/uluslararası dama: zorunlu yeme, geriye yeme,
//!   şah çıkınca sürekli yeme, çoklu yeme).
//! * Basit bir "kayıklık" (MCTS tarzı) yapay zekâ.
//! * 15 bulmacalık kayan blok bulmacasının (sliding puzzle) saf üretici/çözücüsü.
//! * Performans testi (benchmark) ölçüm yardımcıları.

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Dama (Checkers / Draughts)
// ---------------------------------------------------------------------------

/// Tahta boyutu (8x8).
pub const TILE: usize = 8;
/// Kare sayısı.
pub const SQUARES: usize = TILE * TILE;

pub const EMPTY: u8 = 0;
pub const LIGHT: u8 = 1;
pub const DARK: u8 = 2;
const LIGHT_KING: u8 = 3;
const DARK_KING: u8 = 4;

/// Sıra kim belirlerken kullanılan oyuncu kimlikleri.
pub const LIGHT_SIDE: u8 = 0;
pub const DARK_SIDE: u8 = 1;

/// Tek hamlede ziyaret edilebilecek en fazla kare izi (çoklu yeme).
pub const MAX_PATH: usize = 24;

#[inline]
pub const fn is_empty(piece: u8) -> bool {
    piece == EMPTY
}
#[inline]
pub const fn is_light(piece: u8) -> bool {
    piece == LIGHT || piece == LIGHT_KING
}
#[inline]
pub const fn is_dark(piece: u8) -> bool {
    piece == DARK || piece == DARK_KING
}
#[inline]
pub const fn is_king(piece: u8) -> bool {
    piece == LIGHT_KING || piece == DARK_KING
}

/// Satır/sütun -> kare indeksi.
#[inline]
pub const fn idx(row: usize, col: usize) -> usize {
    row * TILE + col
}

/// Kare indeksi -> (satır, sütun).
#[inline]
pub const fn square(index: usize) -> (usize, usize) {
    (index / TILE, index % TILE)
}

/// Kenarlıklarında dolaşılan tahta (kapalı kareler).
#[derive(Clone, Copy)]
pub struct Board {
    pub cells: [u8; SQUARES],
    /// Hamle sırası: 0 = açık renk, 1 = koyu renk (`LIGHT_SIDE` / `DARK_SIDE`).
    pub turn: u8,
    /// Toplam yapılan hamle sayısı (oyunun ilerlemesi / tekrar kontrolü).
    pub ply: u32,
    /// Beraberlik sayacı: son yeme ya da ağır taş hareketinden bu yana hamle.
    pub quiet_plies: u16,
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

impl Board {
    /// Dizilmiş başlangıç konumu: açık taşlar altta (satır 5-7), koyu taşlar üstte
    /// (satır 0-2). Açık renk ilk hamleyi yapar.
    pub const fn new() -> Self {
        let mut cells = [EMPTY; SQUARES];
        let mut row = 0;
        while row < 3 {
            let mut col = 0;
            while col < TILE {
                if (row + col) % 2 == 1 {
                    cells[idx(row, col)] = DARK;
                }
                col += 1;
            }
            row += 1;
        }
        row = TILE - 3;
        while row < TILE {
            let mut col = 0;
            while col < TILE {
                if (row + col) % 2 == 1 {
                    cells[idx(row, col)] = LIGHT;
                }
                col += 1;
            }
            row += 1;
        }
        Self {
            cells,
            turn: LIGHT_SIDE,
            ply: 0,
            quiet_plies: 0,
        }
    }

    /// Sıradaki oyuncu (`LIGHT_SIDE` / `DARK_SIDE`).
    #[inline]
    pub fn side_to_move(&self) -> u8 {
        self.turn
    }

    /// Verilen renkteki taş sayısı.
    pub fn count(&self, light: bool) -> (usize, usize) {
        let mut normal = 0;
        let mut kings = 0;
        for &piece in self.cells.iter() {
            if (light && is_light(piece)) || (!light && is_dark(piece)) {
                if is_king(piece) {
                    kings += 1;
                } else {
                    normal += 1;
                }
            }
        }
        (normal, kings)
    }

    /// Oyun bitti mi? (sıradaki oyuncunun hiç hamlesi yoksa biter.)
    pub fn legal_move_count(&self) -> usize {
        let mut list: Vec<Move> = Vec::new();
        self.generate(&mut list);
        list.len()
    }

    /// Sıradaki taraf için beraberlik/belirsiz durum kontrolü.
    /// 120 sakin hamleden sonra oyun berabere ilan edilir.
    pub const DRAW_PLIES: u16 = 120;

    /// Taş uygun son satıra ulaşmışsa şah yapar.
    fn promote(cells: &mut [u8; SQUARES], index: usize, side: u8) {
        let (row, _) = square(index);
        if side == LIGHT_SIDE && row == 0 && cells[index] == LIGHT {
            cells[index] = LIGHT_KING;
        } else if side == DARK_SIDE && row == TILE - 1 && cells[index] == DARK {
            cells[index] = DARK_KING;
        }
    }

    /// Tüm yasal hamleleri (zorunlu yeme kuralı dahil) listeler.
    pub fn generate(&self, out: &mut Vec<Move>) {
        out.clear();
        let side = self.turn;
        // Önce yeme hamleleri aranır; varsa normal hamleler geçersizdir.
        for from in 0..SQUARES {
            let piece = self.cells[from];
            if piece == EMPTY || is_light(piece) != (side == LIGHT_SIDE) {
                continue;
            }
            let mut path = [0usize; MAX_PATH];
            path[0] = from;
            self.find_captures(from, piece, &mut path, 1, out);
        }
        if !out.is_empty() {
            return;
        }
        for from in 0..SQUARES {
            let piece = self.cells[from];
            if piece == EMPTY || is_light(piece) != (side == LIGHT_SIDE) {
                continue;
            }
            let (row, col) = square(from);
            let dirs: &[(isize, isize)] = if is_king(piece) {
                &[(-1, -1), (-1, 1), (1, -1), (1, 1)]
            } else if side == LIGHT_SIDE {
                &[(-1, -1), (-1, 1)]
            } else {
                &[(1, -1), (1, 1)]
            };
            for &(dr, dc) in dirs {
                let Some(tr) = step_row(row, dr) else {
                    continue;
                };
                let Some(tc) = step_col(col, dc) else {
                    continue;
                };
                let to = idx(tr, tc);
                if self.cells[to] == EMPTY {
                    out.push(Move::simple(from, to));
                }
            }
        }
    }

    /// Şah taşları için tüm köşegen boyunca yeme zincirini özyinelemeli arar.
    fn find_captures(
        &self,
        from: usize,
        piece: u8,
        path: &mut [usize; MAX_PATH],
        depth: usize,
        out: &mut Vec<Move>,
    ) {
        let side = if is_light(piece) {
            LIGHT_SIDE
        } else {
            DARK_SIDE
        };
        // Yeme sırasında şah olmaya hak kazanmış ırgat da engel kuralına göre
        // şah gibi (mesafeli) yemeye devam eder.
        let effective_king = is_king(piece) || chain_promotes(self, from, side);
        let forward = if side == LIGHT_SIDE { -1isize } else { 1isize };
        for &(dr, dc) in &[(-1isize, -1isize), (-1, 1), (1, -1), (1, 1)] {
            // Yeme yönü kenar tarafına göre sınırlıdır (şah değilse).
            if !effective_king && dr != forward {
                continue;
            }
            let mut row = square(from).0 as isize + dr;
            let mut col = square(from).1 as isize + dc;
            if !effective_king {
                if !in_board(row, col) {
                    continue;
                }
                let mid = idx(row as usize, col as usize);
                if !occupies_opponent(self.cells[mid], side) {
                    continue;
                }
                row += dr;
                col += dc;
                if !in_board(row, col) {
                    continue;
                }
                let to = idx(row as usize, col as usize);
                if self.cells[to] != EMPTY {
                    continue;
                }
                let before = out.len();
                let mut copy = *self;
                copy.cells[from] = EMPTY;
                copy.cells[mid] = EMPTY;
                copy.cells[to] = piece;
                let mut next_path = *path;
                next_path[depth] = to;
                copy.find_captures(to, piece, &mut next_path, depth + 1, out);
                if out.len() == before {
                    out.push(Move::capture(path, depth, to, mid, false));
                }
            } else {
                // Şah: köşegen boyunca ilk karşılaşılan taş rakip ise ve arkası
                // boşsa yiyebilir; durabileceği her boş kare bir dal açar.
                let mut jumped: Option<usize> = None;
                loop {
                    if !in_board(row, col) {
                        break;
                    }
                    let sq = idx(row as usize, col as usize);
                    if self.cells[sq] != EMPTY {
                        if occupies_opponent(self.cells[sq], side) {
                            jumped = Some(sq);
                        }
                        break;
                    }
                    row += dr;
                    col += dc;
                }
                let Some(mid) = jumped else {
                    continue;
                };
                row += dr;
                col += dc;
                while in_board(row, col) {
                    let to = idx(row as usize, col as usize);
                    if self.cells[to] != EMPTY {
                        break;
                    }
                    let mut copy = *self;
                    copy.cells[from] = EMPTY;
                    copy.cells[mid] = EMPTY;
                    copy.cells[to] = piece;
                    let mut next_path = *path;
                    next_path[depth] = to;
                    let before = out.len();
                    copy.find_captures(to, piece, &mut next_path, depth + 1, out);
                    // Aynı kareye varan aynı uzunlukta bir zincir zaten üretildiyse
                    // (yinelenen köşegen geçişi) tekrar eklemeyelim.
                    let duplicate = out
                        .iter()
                        .any(|m| m.captures == depth as u8 && m.from() == path[0] && m.to() == to);
                    if out.len() == before && !duplicate {
                        out.push(Move::capture(path, depth, to, mid, true));
                    }
                    row += dr;
                    col += dc;
                }
            }
        }
    }

    /// Hamleyi uygular ve sırayı değiştirir. Geçersizse `false` döner.
    pub fn apply(&mut self, mv: &Move) -> bool {
        if mv.len < 2 || mv.path[0] >= SQUARES {
            return false;
        }
        let from = mv.path[0];
        let piece = self.cells[from];
        if piece == EMPTY || is_light(piece) != (self.turn == LIGHT_SIDE) {
            return false;
        }
        self.cells[from] = EMPTY;
        if mv.captures == 0 {
            let to = mv.path[1];
            self.cells[to] = piece;
            Self::promote(&mut self.cells, to, self.turn);
        } else if mv.king_chain {
            // Şah yemi: zincir boyunca taş şah kalır.
            for i in 1..mv.len {
                let to = mv.path[i];
                let from_sq = mv.path[i - 1];
                if let Some(mid) = capture_square(
                    square(from_sq).0,
                    square(from_sq).1,
                    square(to).0,
                    square(to).1,
                ) {
                    self.cells[mid] = EMPTY;
                }
                self.cells[to] = piece;
                self.cells[from_sq] = EMPTY;
            }
        } else {
            // Irgat yemi: taş yalnızca zincirin son karesinde şah olur.
            for i in 1..mv.len {
                let to = mv.path[i];
                let from_sq = mv.path[i - 1];
                if let Some(mid) = capture_square(
                    square(from_sq).0,
                    square(from_sq).1,
                    square(to).0,
                    square(to).1,
                ) {
                    self.cells[mid] = EMPTY;
                }
                self.cells[to] = piece;
                self.cells[from_sq] = EMPTY;
            }
            if let Some(&last) = mv.path.get(mv.len - 1) {
                Self::promote(&mut self.cells, last, self.turn);
            }
        }
        self.ply += 1;
        if mv.captures == 0 && !is_king(piece) {
            self.quiet_plies = self.quiet_plies.saturating_add(1);
        } else {
            self.quiet_plies = 0;
        }
        self.turn ^= 1;
        true
    }

    /// Oyunun bitip bitmediğini döner. `Some(true)` = açık renk kazandı.
    pub fn winner(&self) -> Option<bool> {
        if self.legal_move_count() == 0 {
            return Some(self.turn == DARK_SIDE);
        }
        let (light_n, light_k) = self.count(true);
        let (dark_n, dark_k) = self.count(false);
        if light_n + light_k == 0 {
            return Some(false);
        }
        if dark_n + dark_k == 0 {
            return Some(true);
        }
        None
    }

    /// Beraberlik sayacı sınırına ulaştı mı?
    pub fn is_draw(&self) -> bool {
        self.quiet_plies >= Self::DRAW_PLIES
    }
}

#[inline]
fn in_board(row: isize, col: isize) -> bool {
    row >= 0 && col >= 0 && (row as usize) < TILE && (col as usize) < TILE
}

#[inline]
fn step_row(row: usize, dr: isize) -> Option<usize> {
    row.checked_add_signed(dr).filter(|r| *r < TILE)
}

#[inline]
fn step_col(col: usize, dc: isize) -> Option<usize> {
    col.checked_add_signed(dc).filter(|c| *c < TILE)
}

#[inline]
fn occupies_opponent(piece: u8, side: u8) -> bool {
    if side == LIGHT_SIDE {
        is_dark(piece)
    } else {
        is_light(piece)
    }
}

/// Bu karede yeme zincirine devam eden taş şah sayılır mı?
/// (Son satıra ulaşmış ırgat, yeme zinciri boyunca şah gibi ilerler.)
#[inline]
fn chain_promotes(_board: &Board, from: usize, side: u8) -> bool {
    let (row, _) = square(from);
    (side == LIGHT_SIDE && row == 0) || (side == DARK_SIDE && row == TILE - 1)
}

/// İki kare arasındaki (köşegen üzerindeki) yenilen taşın karesi.
fn capture_square(fr: usize, fc: usize, tr: usize, tc: usize) -> Option<usize> {
    let dr = tr as isize - fr as isize;
    let dc = tc as isize - fc as isize;
    if dr.abs() <= 1 || dc.abs() != dr.abs() {
        return None;
    }
    let mr = (fr as isize + dr / 2) as usize;
    let mc = (fc as isize + dc / 2) as usize;
    Some(idx(mr, mc))
}

/// Bir dama hamlesi: izlenen kareler, yenen taş sayısı ve yenilen taşın karesi.
#[derive(Clone, Copy)]
pub struct Move {
    pub path: [usize; MAX_PATH],
    pub len: usize,
    pub captures: u8,
    /// Yenilen ilk taşın karesi (yemesiz hamlelerde `SQUARES`).
    pub captured: usize,
    /// `false`: taş zincir boyunca ırgat olarak ilerler; son karede şah olur.
    /// `true`: taş zincir boyunca şah olarak ilerler (şah yemi).
    pub king_chain: bool,
}

impl Move {
    pub fn simple(from: usize, to: usize) -> Self {
        let mut path = [0usize; MAX_PATH];
        path[0] = from;
        path[1] = to;
        Self {
            path,
            len: 2,
            captures: 0,
            captured: SQUARES,
            king_chain: false,
        }
    }

    fn capture(
        path: &[usize; MAX_PATH],
        depth: usize,
        last: usize,
        mid: usize,
        king_chain: bool,
    ) -> Self {
        let mut out = *path;
        out[depth] = last;
        Self {
            path: out,
            len: depth + 1,
            captures: depth as u8,
            captured: mid,
            king_chain,
        }
    }

    #[inline]
    pub fn from(&self) -> usize {
        self.path[0]
    }

    #[inline]
    pub fn to(&self) -> usize {
        self.path[1]
    }

    #[inline]
    pub fn is_capture(&self) -> bool {
        self.captures > 0
    }
}

/// Basit değerlendirme: taş değeri + şah bonusu + ilerleme bonusu.
fn evaluate(board: &Board, side: u8) -> i32 {
    let mut score = 0i32;
    for (sq, &piece) in board.cells.iter().enumerate() {
        if piece == EMPTY {
            continue;
        }
        let (row, _) = square(sq);
        let value = match piece {
            LIGHT => 100 + (TILE - 1 - row) as i32 * 3,
            LIGHT_KING => 175,
            DARK => 100 + row as i32 * 3,
            DARK_KING => 175,
            _ => 0,
        };
        if is_light(piece) == (side == LIGHT_SIDE) {
            score += value;
        } else {
            score -= value;
        }
    }
    score
}

/// Yapay zekâ: verilen derinlikte alfa-beta araması yapar ve en iyi hamleyi döner.
pub fn choose_move(board: &Board, depth: u8) -> Option<Move> {
    let mut list = Vec::new();
    board.generate(&mut list);
    if list.is_empty() {
        return None;
    }
    // En fazla yeme yapan hamleyi öncelikli dene (sıralama).
    let mut best = list[0];
    let mut best_score = i32::MIN;
    for mv in list.iter() {
        let mut copy = *board;
        if !copy.apply(mv) {
            continue;
        }
        let score = -search(&copy, depth.saturating_sub(1), i32::MIN + 1, i32::MAX - 1);
        let score = score + (mv.captures as i32) * 5;
        if score > best_score {
            best_score = score;
            best = *mv;
        }
    }
    Some(best)
}

/// Negamax + alfa-beta budaması.
fn search(board: &Board, depth: u8, mut alpha: i32, beta: i32) -> i32 {
    let mut list = Vec::new();
    board.generate(&mut list);
    if list.is_empty() {
        // Hamlesi olmayan taraf kaybeder.
        return -10_000 - depth as i32;
    }
    if board.is_draw() {
        return 0;
    }
    if depth == 0 {
        return evaluate(board, board.side_to_move());
    }
    let mut best = i32::MIN + 1;
    for mv in list.iter() {
        let mut copy = *board;
        if !copy.apply(mv) {
            continue;
        }
        let score = -search(&copy, depth - 1, -beta, -alpha);
        if score > best {
            best = score;
        }
        if best > alpha {
            alpha = best;
        }
        if alpha >= beta {
            break;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// Ortak oyun durumu
// ---------------------------------------------------------------------------

/// Tek oyunculu oyunların sonucu.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum GameState {
    #[default]
    Playing,
    Won,
    Lost,
}

// ---------------------------------------------------------------------------
// Mayın Tarlası (Minesweeper)
// ---------------------------------------------------------------------------

/// Tahtanın desteklediği en büyük hücre sayısı (16x16).
pub const MINES_MAX_CELLS: usize = 16 * 16;

/// Mayın Tarlası hücre durumu.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
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
///
/// Üç zorluk seviyesi vardır: 9x9/10, 12x12/25 ve 16x16/50. İlk tıklanan
/// hücre ve çevresindeki 3x3 alan her zaman mayınsızdır.
#[derive(Clone, Copy)]
pub struct Minesweeper {
    pub level: usize,
    pub rows: usize,
    pub cols: usize,
    pub mine_count: usize,
    pub cells: [MineCell; MINES_MAX_CELLS],
    pub revealed: usize,
    pub flagged: usize,
    pub state: GameState,
    pub first_click: bool,
    pub seed: u64,
}

impl Minesweeper {
    pub fn new() -> Self {
        Self {
            level: 0,
            rows: 9,
            cols: 9,
            mine_count: 10,
            cells: [MineCell::default(); MINES_MAX_CELLS],
            revealed: 0,
            flagged: 0,
            state: GameState::Playing,
            first_click: true,
            seed: 0x5EED_1234_ABCD_EF01,
        }
    }

    pub fn reset(&mut self) {
        self.cells = [MineCell::default(); MINES_MAX_CELLS];
        self.revealed = 0;
        self.flagged = 0;
        self.state = GameState::Playing;
        self.first_click = true;
        // Her yeni oyunda farklı bir mayın yerleşimi üret.
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
    }

    pub fn set_level(&mut self, level: usize) {
        self.level = level.min(2);
        let (rows, cols, mines) = match self.level {
            1 => (12, 12, 25),
            2 => (16, 16, 50),
            _ => (9, 9, 10),
        };
        self.rows = rows;
        self.cols = cols;
        self.mine_count = mines;
        self.reset();
    }

    #[inline]
    pub fn cell_count(&self) -> usize {
        self.rows * self.cols
    }

    fn rng_next(&mut self) -> u64 {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.seed
    }

    /// Mayınları yerleştirir; ilk tıklanan hücre ve 3x3 çevresi güvenlidir.
    fn place_mines(&mut self, safe_r: usize, safe_c: usize) {
        let mut candidates: Vec<usize> = Vec::with_capacity(self.cell_count());
        for r in 0..self.rows {
            for c in 0..self.cols {
                let near_safe = r.abs_diff(safe_r) <= 1 && c.abs_diff(safe_c) <= 1;
                if !near_safe {
                    candidates.push(r * self.cols + c);
                }
            }
        }
        let count = self.mine_count.min(candidates.len());
        self.mine_count = count;
        for i in 0..count {
            let remaining = candidates.len() - i;
            let pick = i + (self.rng_next() as usize) % remaining;
            candidates.swap(i, pick);
            let idx = candidates[i];
            self.cells[idx].is_mine = true;
        }
        self.recount_adjacent();
    }

    fn recount_adjacent(&mut self) {
        for r in 0..self.rows {
            for c in 0..self.cols {
                let idx = r * self.cols + c;
                if self.cells[idx].is_mine {
                    continue;
                }
                let mut cnt = 0u8;
                for dr in -1isize..=1 {
                    for dc in -1isize..=1 {
                        if dr == 0 && dc == 0 {
                            continue;
                        }
                        let nr = r as isize + dr;
                        let nc = c as isize + dc;
                        if nr < 0 || nr >= self.rows as isize || nc < 0 || nc >= self.cols as isize
                        {
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

    /// Hücreyi açar. Mayına basarsa kaybeder, boşsa komşuları da açar.
    /// En az bir yeni hücre açıldıysa `true` döner.
    pub fn reveal(&mut self, r: usize, c: usize) -> bool {
        if self.state != GameState::Playing || r >= self.rows || c >= self.cols {
            return false;
        }
        let start = r * self.cols + c;
        if self.cells[start].state != CellState::Hidden {
            return false;
        }
        if self.first_click {
            self.first_click = false;
            self.place_mines(r, c);
        }
        if self.cells[start].is_mine {
            self.cells[start].state = CellState::Revealed;
            self.state = GameState::Lost;
            // Tüm mayınları göster.
            for i in 0..self.cell_count() {
                if self.cells[i].is_mine {
                    self.cells[i].state = CellState::Revealed;
                }
            }
            return true;
        }
        // Boş bölgeleri yinelemesiz taşkın doldurma ile aç.
        let mut stack: Vec<usize> = Vec::new();
        stack.push(start);
        while let Some(idx) = stack.pop() {
            if self.cells[idx].state != CellState::Hidden {
                continue;
            }
            self.cells[idx].state = CellState::Revealed;
            self.revealed += 1;
            if self.cells[idx].adjacent != 0 {
                continue;
            }
            let rr = idx / self.cols;
            let cc = idx % self.cols;
            for dr in -1isize..=1 {
                for dc in -1isize..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = rr as isize + dr;
                    let nc = cc as isize + dc;
                    if nr < 0 || nr >= self.rows as isize || nc < 0 || nc >= self.cols as isize {
                        continue;
                    }
                    let nidx = nr as usize * self.cols + nc as usize;
                    if self.cells[nidx].state == CellState::Hidden {
                        stack.push(nidx);
                    }
                }
            }
        }
        self.check_win();
        true
    }

    /// Açılmış bir sayının çevresindeki bayrak sayısı doğruysa komşularını açar.
    pub fn chord(&mut self, r: usize, c: usize) -> bool {
        if self.state != GameState::Playing || r >= self.rows || c >= self.cols {
            return false;
        }
        let idx = r * self.cols + c;
        let cell = self.cells[idx];
        if cell.state != CellState::Revealed || cell.adjacent == 0 {
            return false;
        }
        let mut flags = 0u8;
        for dr in -1isize..=1 {
            for dc in -1isize..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let nr = r as isize + dr;
                let nc = c as isize + dc;
                if nr < 0 || nr >= self.rows as isize || nc < 0 || nc >= self.cols as isize {
                    continue;
                }
                let nidx = nr as usize * self.cols + nc as usize;
                if self.cells[nidx].state == CellState::Flagged {
                    flags += 1;
                }
            }
        }
        if flags != cell.adjacent {
            return false;
        }
        let mut opened = false;
        for dr in -1isize..=1 {
            for dc in -1isize..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let nr = r as isize + dr;
                let nc = c as isize + dc;
                if nr < 0 || nr >= self.rows as isize || nc < 0 || nc >= self.cols as isize {
                    continue;
                }
                let nidx = nr as usize * self.cols + nc as usize;
                if self.cells[nidx].state == CellState::Hidden
                    && self.reveal(nr as usize, nc as usize)
                {
                    opened = true;
                }
                if self.state != GameState::Playing {
                    return opened;
                }
            }
        }
        opened
    }

    /// Bayrağı açar/kaldırır. Durum değiştiyse `true` döner.
    pub fn toggle_flag(&mut self, r: usize, c: usize) -> bool {
        if self.state != GameState::Playing || r >= self.rows || c >= self.cols {
            return false;
        }
        let idx = r * self.cols + c;
        match self.cells[idx].state {
            CellState::Hidden => {
                self.cells[idx].state = CellState::Flagged;
                self.flagged += 1;
                true
            }
            CellState::Flagged => {
                self.cells[idx].state = CellState::Hidden;
                self.flagged = self.flagged.saturating_sub(1);
                true
            }
            CellState::Revealed => false,
        }
    }

    fn check_win(&mut self) {
        if self.state != GameState::Playing {
            return;
        }
        let safe_cells = self.cell_count().saturating_sub(self.mine_count);
        if self.revealed >= safe_cells {
            self.state = GameState::Won;
            // Kazanınca kalan mayınları otomatik bayrakla.
            for i in 0..self.cell_count() {
                if self.cells[i].is_mine && self.cells[i].state == CellState::Hidden {
                    self.cells[i].state = CellState::Flagged;
                }
            }
            self.flagged = self.mine_count;
        }
    }
}

impl Default for Minesweeper {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Satranç (Chess)
// ---------------------------------------------------------------------------

pub const CHESS_TILE: usize = 8;
pub const CHESS_SQUARES: usize = CHESS_TILE * CHESS_TILE;

/// Satranç taşı türleri.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
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
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ChessSide {
    #[default]
    White,
    Black,
}

impl ChessSide {
    #[inline]
    pub fn opposite(self) -> Self {
        match self {
            ChessSide::White => ChessSide::Black,
            ChessSide::Black => ChessSide::White,
        }
    }
}

/// Satranç taşı.
#[derive(Clone, Copy, Default)]
pub struct ChessPiece {
    pub kind: PieceKind,
    pub side: ChessSide,
    /// Şah/kale için rok hakkı; piyon terfisinde de `true` olur.
    pub has_moved: bool,
}

/// Satranç hamlesi.
#[derive(Clone, Copy)]
pub struct ChessMove {
    pub from: usize,
    pub to: usize,
    pub promotion: Option<PieceKind>,
}

/// Satranç oyununun sonucu.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ChessStatus {
    #[default]
    Playing,
    /// Şah mat; kazanan taraf.
    Checkmate(ChessSide),
    /// Pat (hamle yok ama şah yok): berabere.
    Stalemate,
    /// 50 hamle kuralı veya yetersiz materyal: berabere.
    Draw,
}

/// Satranç tahtası (8x8).
#[derive(Clone, Copy)]
pub struct ChessBoard {
    pub cells: [[Option<ChessPiece>; CHESS_TILE]; CHESS_TILE],
    pub side_to_move: ChessSide,
    pub status: ChessStatus,
    pub halfmove_clock: u32,
    pub fullmove_number: u32,
    /// Piyon iki kare ilerlediğinde arkasında bıraktığı "geçiş" karesi.
    pub en_passant: Option<usize>,
}

impl ChessBoard {
    pub fn new() -> Self {
        let mut cells = [[None; CHESS_TILE]; CHESS_TILE];
        let back_rank = [
            PieceKind::Rook,
            PieceKind::Knight,
            PieceKind::Bishop,
            PieceKind::Queen,
            PieceKind::King,
            PieceKind::Bishop,
            PieceKind::Knight,
            PieceKind::Rook,
        ];
        for (c, kind) in back_rank.iter().enumerate() {
            cells[0][c] = Some(ChessPiece {
                kind: *kind,
                side: ChessSide::Black,
                has_moved: false,
            });
            cells[7][c] = Some(ChessPiece {
                kind: *kind,
                side: ChessSide::White,
                has_moved: false,
            });
        }
        for c in 0..CHESS_TILE {
            cells[1][c] = Some(ChessPiece {
                kind: PieceKind::Pawn,
                side: ChessSide::Black,
                has_moved: false,
            });
            cells[6][c] = Some(ChessPiece {
                kind: PieceKind::Pawn,
                side: ChessSide::White,
                has_moved: false,
            });
        }
        Self {
            cells,
            side_to_move: ChessSide::White,
            status: ChessStatus::Playing,
            halfmove_clock: 0,
            fullmove_number: 1,
            en_passant: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    #[inline]
    fn in_bounds(r: isize, c: isize) -> bool {
        (0..CHESS_TILE as isize).contains(&r) && (0..CHESS_TILE as isize).contains(&c)
    }

    #[inline]
    fn square(r: usize, c: usize) -> usize {
        r * CHESS_TILE + c
    }

    /// Karedeki taşın konumu (satır, sütun).
    #[inline]
    pub fn coords(sq: usize) -> (usize, usize) {
        (sq / CHESS_TILE, sq % CHESS_TILE)
    }

    fn push_move(out: &mut Vec<ChessMove>, from: usize, to: usize) {
        out.push(ChessMove {
            from,
            to,
            promotion: None,
        });
    }

    /// Piyon hamlesi: son satıra ulaşırsa dört terfi seçeneği üretir.
    fn push_pawn_move(out: &mut Vec<ChessMove>, from: usize, to: usize) {
        let row = to / CHESS_TILE;
        if row == 0 || row == CHESS_TILE - 1 {
            for kind in [
                PieceKind::Queen,
                PieceKind::Rook,
                PieceKind::Bishop,
                PieceKind::Knight,
            ] {
                out.push(ChessMove {
                    from,
                    to,
                    promotion: Some(kind),
                });
            }
        } else {
            Self::push_move(out, from, to);
        }
    }

    fn slide(
        &self,
        from_r: usize,
        from_c: usize,
        side: ChessSide,
        dirs: &[(isize, isize)],
        out: &mut Vec<ChessMove>,
    ) {
        let from = Self::square(from_r, from_c);
        for &(dr, dc) in dirs {
            let mut r = from_r as isize + dr;
            let mut c = from_c as isize + dc;
            while Self::in_bounds(r, c) {
                match self.cells[r as usize][c as usize] {
                    None => Self::push_move(out, from, Self::square(r as usize, c as usize)),
                    Some(p) => {
                        if p.side != side {
                            Self::push_move(out, from, Self::square(r as usize, c as usize));
                        }
                        break;
                    }
                }
                r += dr;
                c += dc;
            }
        }
    }

    fn step(
        &self,
        from_r: usize,
        from_c: usize,
        side: ChessSide,
        dirs: &[(isize, isize)],
        out: &mut Vec<ChessMove>,
    ) {
        let from = Self::square(from_r, from_c);
        for &(dr, dc) in dirs {
            let r = from_r as isize + dr;
            let c = from_c as isize + dc;
            if !Self::in_bounds(r, c) {
                continue;
            }
            match self.cells[r as usize][c as usize] {
                None => Self::push_move(out, from, Self::square(r as usize, c as usize)),
                Some(p) if p.side != side => {
                    Self::push_move(out, from, Self::square(r as usize, c as usize))
                }
                _ => {}
            }
        }
    }

    /// Taşın kurallara göre (şah güvenliği hariç) hamlelerini üretir.
    fn generate_pseudo(&self, r: usize, c: usize, out: &mut Vec<ChessMove>) {
        let piece = match self.cells[r][c] {
            Some(p) => p,
            None => return,
        };
        let side = piece.side;
        let from = Self::square(r, c);
        match piece.kind {
            PieceKind::Pawn => {
                let dir = if side == ChessSide::White { -1 } else { 1 };
                let start_row = if side == ChessSide::White { 6 } else { 1 };
                let nr = r as isize + dir;
                if Self::in_bounds(nr, c as isize) {
                    let nr = nr as usize;
                    if self.cells[nr][c].is_none() {
                        Self::push_pawn_move(out, from, Self::square(nr, c));
                        let nr2 = r as isize + 2 * dir;
                        if r == start_row
                            && Self::in_bounds(nr2, c as isize)
                            && self.cells[nr2 as usize][c].is_none()
                        {
                            Self::push_move(out, from, Self::square(nr2 as usize, c));
                        }
                    }
                    for dc in [-1isize, 1] {
                        let nc = c as isize + dc;
                        if !Self::in_bounds(nr as isize, nc) {
                            continue;
                        }
                        let nc = nc as usize;
                        match self.cells[nr][nc] {
                            Some(p) if p.side != side => {
                                Self::push_pawn_move(out, from, Self::square(nr, nc))
                            }
                            None if self.en_passant == Some(Self::square(nr, nc)) => {
                                Self::push_move(out, from, Self::square(nr, nc))
                            }
                            _ => {}
                        }
                    }
                }
            }
            PieceKind::Rook => self.slide(r, c, side, &[(-1, 0), (1, 0), (0, -1), (0, 1)], out),
            PieceKind::Bishop => self.slide(r, c, side, &[(-1, -1), (-1, 1), (1, -1), (1, 1)], out),
            PieceKind::Queen => self.slide(
                r,
                c,
                side,
                &[
                    (-1, -1),
                    (-1, 0),
                    (-1, 1),
                    (0, -1),
                    (0, 1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                ],
                out,
            ),
            PieceKind::Knight => self.step(
                r,
                c,
                side,
                &[
                    (-2, -1),
                    (-2, 1),
                    (-1, -2),
                    (-1, 2),
                    (1, -2),
                    (1, 2),
                    (2, -1),
                    (2, 1),
                ],
                out,
            ),
            PieceKind::King => {
                self.step(
                    r,
                    c,
                    side,
                    &[
                        (-1, -1),
                        (-1, 0),
                        (-1, 1),
                        (0, -1),
                        (0, 1),
                        (1, -1),
                        (1, 0),
                        (1, 1),
                    ],
                    out,
                );
                self.generate_castles(r, c, piece, out);
            }
        }
    }

    fn rook_can_castle(&self, row: usize, col: usize, side: ChessSide) -> bool {
        matches!(
            self.cells[row][col],
            Some(ChessPiece {
                kind: PieceKind::Rook,
                side: rook_side,
                has_moved: false,
            }) if rook_side == side
        )
    }

    fn generate_castles(&self, r: usize, c: usize, piece: ChessPiece, out: &mut Vec<ChessMove>) {
        if piece.kind != PieceKind::King || piece.has_moved {
            return;
        }
        let home = if piece.side == ChessSide::White { 7 } else { 0 };
        if r != home || c != 4 {
            return;
        }
        let enemy = piece.side.opposite();
        if self.is_square_attacked(home, 4, enemy) {
            return;
        }
        let from = Self::square(home, 4);
        // Kısa rok: f/g boş, kareler saldırı altında değil.
        if self.rook_can_castle(home, 7, piece.side)
            && self.cells[home][5].is_none()
            && self.cells[home][6].is_none()
            && !self.is_square_attacked(home, 5, enemy)
            && !self.is_square_attacked(home, 6, enemy)
        {
            Self::push_move(out, from, Self::square(home, 6));
        }
        // Uzun rok: b/c/d boş, kareler saldırı altında değil.
        if self.rook_can_castle(home, 0, piece.side)
            && self.cells[home][1].is_none()
            && self.cells[home][2].is_none()
            && self.cells[home][3].is_none()
            && !self.is_square_attacked(home, 3, enemy)
            && !self.is_square_attacked(home, 2, enemy)
        {
            Self::push_move(out, from, Self::square(home, 2));
        }
    }

    /// Verilen taraf için tüm yasal hamleleri üretir (şah güvenliği dahil).
    pub fn generate_legal(&self, side: ChessSide, out: &mut Vec<ChessMove>) {
        out.clear();
        for r in 0..CHESS_TILE {
            for c in 0..CHESS_TILE {
                if self.cells[r][c].is_some_and(|p| p.side == side) {
                    let mut pseudo: Vec<ChessMove> = Vec::new();
                    self.generate_pseudo(r, c, &mut pseudo);
                    for mv in pseudo {
                        if self.is_legal(&mv) {
                            out.push(mv);
                        }
                    }
                }
            }
        }
    }

    /// Belirli bir kareden oynanabilecek yasal hamleler.
    pub fn legal_from(&self, r: usize, c: usize) -> Vec<ChessMove> {
        let mut all = Vec::new();
        self.generate_legal(self.side_to_move, &mut all);
        all.retain(|m| m.from == Self::square(r, c));
        all
    }

    fn is_legal(&self, mv: &ChessMove) -> bool {
        if mv.from >= CHESS_SQUARES || mv.to >= CHESS_SQUARES {
            return false;
        }
        let mover = self.side_to_move;
        let mut copy = *self;
        copy.make_move(mv);
        !copy.is_in_check(mover)
    }

    /// Hamleyi kurallara göre uygular ve sırayı değiştirir. Geçersizse `false`.
    pub fn apply(&mut self, mv: &ChessMove) -> bool {
        if self.status != ChessStatus::Playing {
            return false;
        }
        let mut legal = Vec::new();
        self.generate_legal(self.side_to_move, &mut legal);
        let chosen = legal
            .iter()
            .find(|m| {
                m.from == mv.from
                    && m.to == mv.to
                    && match mv.promotion {
                        Some(kind) => m.promotion == Some(kind),
                        None => m.promotion.is_none() || m.promotion == Some(PieceKind::Queen),
                    }
            })
            .copied();
        let Some(chosen) = chosen else {
            return false;
        };
        self.make_move(&chosen);
        self.refresh_status();
        true
    }

    /// Doğrulanmış hamleyi tahtaya uygular (durum güncellemesi yapmaz).
    fn make_move(&mut self, mv: &ChessMove) {
        let (fr, fc) = Self::coords(mv.from);
        let (tr, tc) = Self::coords(mv.to);
        let Some(piece) = self.cells[fr][fc] else {
            return;
        };
        let is_pawn = piece.kind == PieceKind::Pawn;
        let mut captured = self.cells[tr][tc].is_some();
        let is_castle = piece.kind == PieceKind::King && tc.abs_diff(fc) == 2;
        self.cells[fr][fc] = None;
        if is_castle {
            let (rook_from, rook_to) = if tc > fc { (7, 5) } else { (0, 3) };
            if let Some(mut rook) = self.cells[fr][rook_from].take() {
                rook.has_moved = true;
                self.cells[fr][rook_to] = Some(rook);
            }
        }
        if is_pawn && tc != fc && !captured {
            // Geçerken alma: yenilen piyon hedef sütunun başlangıç satırındadır.
            if self.cells[fr][tc].is_some() {
                captured = true;
            }
            self.cells[fr][tc] = None;
        }
        let new_kind = if is_pawn && (tr == 0 || tr == CHESS_TILE - 1) {
            mv.promotion.unwrap_or(PieceKind::Queen)
        } else {
            piece.kind
        };
        self.cells[tr][tc] = Some(ChessPiece {
            kind: new_kind,
            side: piece.side,
            has_moved: true,
        });
        self.en_passant = if is_pawn && fr.abs_diff(tr) == 2 {
            Some(Self::square((fr + tr) / 2, fc))
        } else {
            None
        };
        if is_pawn || captured {
            self.halfmove_clock = 0;
        } else {
            self.halfmove_clock = self.halfmove_clock.saturating_add(1);
        }
        if self.side_to_move == ChessSide::Black {
            self.fullmove_number = self.fullmove_number.saturating_add(1);
        }
        self.side_to_move = self.side_to_move.opposite();
    }

    /// Hamleden sonra şah mat / pat / beraberlik durumunu tazeler.
    pub fn refresh_status(&mut self) {
        if self.halfmove_clock >= 100 || self.insufficient_material() {
            self.status = ChessStatus::Draw;
            return;
        }
        let side = self.side_to_move;
        let mut legal = Vec::new();
        self.generate_legal(side, &mut legal);
        self.status = if !legal.is_empty() {
            ChessStatus::Playing
        } else if self.is_in_check(side) {
            ChessStatus::Checkmate(side.opposite())
        } else {
            ChessStatus::Stalemate
        };
    }

    pub fn king_square(&self, side: ChessSide) -> Option<(usize, usize)> {
        for (r, row) in self.cells.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                if let Some(p) = *cell {
                    if p.kind == PieceKind::King && p.side == side {
                        return Some((r, c));
                    }
                }
            }
        }
        None
    }

    /// `(r, c)` karesi `by` tarafından saldırı altında mı?
    pub fn is_square_attacked(&self, r: usize, c: usize, by: ChessSide) -> bool {
        // Piyonlar.
        let pawn_row = r as isize + if by == ChessSide::White { 1 } else { -1 };
        for dc in [-1isize, 1] {
            let pc = c as isize + dc;
            if Self::in_bounds(pawn_row, pc) {
                if let Some(p) = self.cells[pawn_row as usize][pc as usize] {
                    if p.kind == PieceKind::Pawn && p.side == by {
                        return true;
                    }
                }
            }
        }
        // Atlar.
        for (dr, dc) in [
            (-2isize, -1isize),
            (-2, 1),
            (-1, -2),
            (-1, 2),
            (1, -2),
            (1, 2),
            (2, -1),
            (2, 1),
        ] {
            let nr = r as isize + dr;
            let nc = c as isize + dc;
            if Self::in_bounds(nr, nc) {
                if let Some(p) = self.cells[nr as usize][nc as usize] {
                    if p.kind == PieceKind::Knight && p.side == by {
                        return true;
                    }
                }
            }
        }
        // Şah.
        for dr in -1isize..=1 {
            for dc in -1isize..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let nr = r as isize + dr;
                let nc = c as isize + dc;
                if Self::in_bounds(nr, nc) {
                    if let Some(p) = self.cells[nr as usize][nc as usize] {
                        if p.kind == PieceKind::King && p.side == by {
                            return true;
                        }
                    }
                }
            }
        }
        // Kale / vezir (yatay-dikey).
        for (dr, dc) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
            let mut nr = r as isize + dr;
            let mut nc = c as isize + dc;
            while Self::in_bounds(nr, nc) {
                if let Some(p) = self.cells[nr as usize][nc as usize] {
                    if p.side == by && (p.kind == PieceKind::Rook || p.kind == PieceKind::Queen) {
                        return true;
                    }
                    break;
                }
                nr += dr;
                nc += dc;
            }
        }
        // Fil / vezir (çapraz).
        for (dr, dc) in [(-1isize, -1isize), (-1, 1), (1, -1), (1, 1)] {
            let mut nr = r as isize + dr;
            let mut nc = c as isize + dc;
            while Self::in_bounds(nr, nc) {
                if let Some(p) = self.cells[nr as usize][nc as usize] {
                    if p.side == by && (p.kind == PieceKind::Bishop || p.kind == PieceKind::Queen) {
                        return true;
                    }
                    break;
                }
                nr += dr;
                nc += dc;
            }
        }
        false
    }

    /// Belirli tarafın şahı tehdit altında mı?
    pub fn is_in_check(&self, side: ChessSide) -> bool {
        match self.king_square(side) {
            Some((r, c)) => self.is_square_attacked(r, c, side.opposite()),
            None => false,
        }
    }

    /// Yetersiz materyal (K-K, K+tek hafif taş, aynı renk karede iki fil).
    pub fn insufficient_material(&self) -> bool {
        let mut minors = 0usize;
        let mut light_bishops = 0usize;
        let mut dark_bishops = 0usize;
        for (r, row) in self.cells.iter().enumerate().take(CHESS_TILE) {
            for (c, cell) in row.iter().enumerate().take(CHESS_TILE) {
                let Some(p) = *cell else { continue };
                match p.kind {
                    PieceKind::Pawn | PieceKind::Rook | PieceKind::Queen => return false,
                    PieceKind::Bishop => {
                        minors += 1;
                        if (r + c) % 2 == 0 {
                            light_bishops += 1;
                        } else {
                            dark_bishops += 1;
                        }
                    }
                    PieceKind::Knight => minors += 1,
                    PieceKind::King => {}
                }
            }
        }
        if minors <= 1 {
            return true;
        }
        minors == 2 && (light_bishops == 2 || dark_bishops == 2)
    }
}

impl Default for ChessBoard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Kayan blok bulmacası (Sliding Puzzle)
// ---------------------------------------------------------------------------

pub const PUZZLE_SIDE: usize = 4;
pub const PUZZLE_TILES: usize = PUZZLE_SIDE * PUZZLE_SIDE;
/// Bulmacada "boş" kareyi temsil eden değer.
pub const PUZZLE_BLANK: u8 = 0;

/// xorshift64 yıldız üreteci — deterministik tohumla tekrarlanabilir karıştırma.
fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    if x == 0 {
        x = 0x9E37_79B9_7F4A_7C15;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Boş karenin yanındaki (en fazla dört) karenin indekslerini doldurur,
/// kaç tanesinin geçerli olduğunu döner.
fn puzzle_neighbours(blank: usize, out: &mut [usize; 4]) -> usize {
    let (r, c) = (blank / PUZZLE_SIDE, blank % PUZZLE_SIDE);
    let mut count = 0;
    if r > 0 {
        out[count] = blank - PUZZLE_SIDE;
        count += 1;
    }
    if r + 1 < PUZZLE_SIDE {
        out[count] = blank + PUZZLE_SIDE;
        count += 1;
    }
    if c > 0 {
        out[count] = blank - 1;
        count += 1;
    }
    if c + 1 < PUZZLE_SIDE {
        out[count] = blank + 1;
        count += 1;
    }
    count
}

/// Karışık fakat çözülebilir bir tahta üretir. Tohum sabitse dağıtım tekrarlanır.
pub fn puzzle_from_seed(seed: u64) -> [u8; PUZZLE_TILES] {
    let mut tiles = [0u8; PUZZLE_TILES];
    for (i, tile) in tiles.iter_mut().enumerate() {
        *tile = i as u8;
    }
    let mut state = seed | 1;
    // Çözülebilirliği garantilemek için yalnızca geçerli (boşlukla komşu) hamleleri uygula.
    let mut blank = PUZZLE_TILES - 1;
    for _ in 0..400 {
        let mut options = [usize::MAX; 4];
        let count = puzzle_neighbours(blank, &mut options);
        let pick = (xorshift64(&mut state) as usize) % count;
        let target = options[pick];
        tiles.swap(blank, target);
        blank = target;
    }
    tiles
}

/// Çözülmüş konum mu?
pub fn puzzle_solved(tiles: &[u8; PUZZLE_TILES]) -> bool {
    tiles.iter().enumerate().all(|(i, &tile)| tile == i as u8)
}

/// Boş karenin indeksi.
pub fn puzzle_blank(tiles: &[u8; PUZZLE_TILES]) -> Option<usize> {
    tiles.iter().position(|&t| t == PUZZLE_BLANK)
}

/// (r, c) karesinin yanındaki boş kareyle yer değiştirir. Geçerliyse taşın yerini döner.
pub fn puzzle_move(tiles: &mut [u8; PUZZLE_TILES], index: usize) -> Option<usize> {
    let blank = puzzle_blank(tiles)?;
    if index >= PUZZLE_TILES || !puzzle_adjacent(blank, index) {
        return None;
    }
    tiles.swap(blank, index);
    Some(blank)
}

fn puzzle_adjacent(a: usize, b: usize) -> bool {
    let (ar, ac) = (a / PUZZLE_SIDE, a % PUZZLE_SIDE);
    let (br, bc) = (b / PUZZLE_SIDE, b % PUZZLE_SIDE);
    ar.abs_diff(br) + ac.abs_diff(bc) == 1
}

/// Manhattan uzaklığı (boşluk hariç) — ilerleme yüzdesi için kullanılır.
pub fn puzzle_distance(tiles: &[u8; PUZZLE_TILES]) -> usize {
    let mut total = 0;
    for (i, &value) in tiles.iter().enumerate() {
        if value == PUZZLE_BLANK {
            continue;
        }
        let home = value as usize;
        let (hr, hc) = (home / PUZZLE_SIDE, home % PUZZLE_SIDE);
        let (r, c) = (i / PUZZLE_SIDE, i % PUZZLE_SIDE);
        total += hr.abs_diff(r) + hc.abs_diff(c);
    }
    total
}

/// Rastgele geçerli bir hamle uygular (gösteri modu).
pub fn puzzle_random_step(tiles: &mut [u8; PUZZLE_TILES], state: &mut u64) -> Option<usize> {
    let blank = puzzle_blank(tiles)?;
    let mut options = [usize::MAX; 4];
    let count = puzzle_neighbours(blank, &mut options);
    let pick = (xorshift64(state) as usize) % count;
    puzzle_move(tiles, options[pick])
}

// ---------------------------------------------------------------------------
// no_std matematik yardımcıları (Bilardo için)
// ---------------------------------------------------------------------------

use core::f32::consts::PI;

/// Taylor serisi ile sinüs.
#[inline(always)]
pub fn billiards_sin(mut x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    for _ in 0..8 {
        if x <= PI {
            break;
        }
        x -= core::f32::consts::TAU;
    }
    for _ in 0..8 {
        if x >= -PI {
            break;
        }
        x += core::f32::consts::TAU;
    }
    let x2 = x * x;
    let x3 = x * x2;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    x - (x3 / 6.0) + (x5 / 120.0) - (x7 / 5040.0)
}

/// Kosinüs.
#[inline(always)]
pub fn billiards_cos(x: f32) -> f32 {
    billiards_sin(x + PI / 2.0)
}

/// Newton-Raphson karekök.
#[inline(always)]
pub fn billiards_sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = if x >= 1.0 { x * 0.5 + 0.5 } else { 1.0 };
    for _ in 0..6 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// atan2 yaklaşımı.
#[inline(always)]
pub fn billiards_atan2(y: f32, x: f32) -> f32 {
    if !x.is_finite() || !y.is_finite() {
        return 0.0;
    }
    if x == 0.0 {
        if y > 0.0 {
            return PI / 2.0;
        }
        if y < 0.0 {
            return -PI / 2.0;
        }
        return 0.0;
    }
    let mut atan = (y / x).atan_approx();
    if x < 0.0 {
        if y >= 0.0 {
            atan += PI;
        } else {
            atan -= PI;
        }
    }
    atan
}

/// atan yaklaşımı (polinom).
trait AtanApprox {
    fn atan_approx(self) -> f32;
}

impl AtanApprox for f32 {
    fn atan_approx(self) -> f32 {
        let x = self.abs();
        let x2 = x * x;
        let result = x / (1.0 + x2) * (1.0 + x2 * (0.3333333 + x2 * (0.2 + x2 * 0.1428571)));
        if self < 0.0 {
            -result
        } else {
            result
        }
    }
}

/// clamp yardımcı (f32 için).
#[inline(always)]
pub fn billiards_clamp(x: f32, min: f32, max: f32) -> f32 {
    if x < min {
        min
    } else if x > max {
        max
    } else {
        x
    }
}

/// max yardımcı (f32 için).
#[inline(always)]
pub fn billiards_max(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

/// min yardımcı (f32 için).
#[inline(always)]
pub fn billiards_min(a: f32, b: f32) -> f32 {
    if a < b {
        a
    } else {
        b
    }
}

/// abs_diff yardımcı (usize için).
#[inline(always)]
pub fn billiards_abs_diff(a: usize, b: usize) -> usize {
    a.abs_diff(b)
}

// ---------------------------------------------------------------------------
// Bilardo (Pool / 8-Ball)
// ---------------------------------------------------------------------------

/// Masa boyutları (dünya birimleri).
pub const BILLIARDS_TABLE_W: f32 = 2.0;
pub const BILLIARDS_TABLE_H: f32 = 1.0;
/// Top yarıçapı.
pub const BILLIARDS_BALL_R: f32 = 0.035;
/// Çapa (kenar) kalınlığı.
pub const BILLIARDS_CUSHION: f32 = 0.04;
/// Delik yarıçapı.
pub const BILLIARDS_POCKET_R: f32 = 0.055;
/// Sürtünme katsayısı (her tick'te v *= 1 - friction).
pub const BILLIARDS_FRICTION: f32 = 0.008;
/// Minimum hız eşiği (altında durdur).
pub const BILLIARDS_MIN_SPEED: f32 = 0.001;
/// Maksimum vuruş hızı (dünya birimi/tick). 2.0x1.0 birimlik masa için
/// tokat hissi veren üst sınır; eski 1.5 değeri topları neredeyse
/// anında delik gibi masadan çıkarıyordu.
pub const BILLIARDS_MAX_SPEED: f32 = 0.14;
/// Bir alt adımda bir topun en fazla bu kadar ilerlemesi hedeflenir.
/// Hızlı topların tek bir adımda diğer topu veya çapayı geçmesini
/// (tunneling) önlemek için hareket ve çarpışma denetimi alt adımlara bölünür.
pub const BILLIARDS_SUB_STEP_MAX_DIST: f32 = BILLIARDS_BALL_R * 0.5;
/// Yan spin (english) yol sapması katsayısı: hız ile çarpılıp ivmeye
/// dönüşür, böylece sert vuruşta spin etkisi de büyür.
const BILLIARDS_SPIN_ACCEL: f32 = 0.012;
/// Üst/alt spin (follow/draw) hız katkısı katsayısı.
const BILLIARDS_SPIN_FOLLOW: f32 = 0.006;
/// Spin sönümlemesi (tick başına çarpan).
const BILLIARDS_SPIN_DECAY: f32 = 0.90;
/// Top sayısı (15 renkli + 1 beyaz).
pub const BILLIARDS_BALL_COUNT: usize = 16;

/// Top türleri (8-ball kuralları).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum BallKind {
    #[default]
    Cue, // 0: Beyaz (çap topu)
    Solid,  // 1-7: Düz renkli (küçük numaralı)
    Stripe, // 9-15: Çizgili (büyük numaralı)
    Eight,  // 8: Siyah 8 topu
}

/// Bilardo topları.
#[derive(Clone, Copy)]
pub struct BilliardsBall {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    /// Döndürme açısı (radyan) — topun son hareket yönüne dik yatay
    /// eksen etrafındaki yuvarlanma fazı; işaretler/doku bunu izler.
    pub angle: f32,
    /// Son hareket yönü (birim vektör); durunca da dokunun ekseni
    /// doğru kalsın diye saklanır.
    pub roll_ux: f32,
    pub roll_uy: f32,
    /// Yan english spin'i (-1..1): çap topunun rotasyonunu yan yanal
    /// büker ( Magnus benzeri basit model ).
    pub spin_x: f32,
    /// Üst/alt english spin'i (-1..1): pozitif topspin ileri iter,
    /// negatif backspin geri çeker.
    pub spin_y: f32,
    pub kind: BallKind,
    pub number: u8, // 1-15, 0 = çap topu
    pub pocketed: bool,
    pub in_play: bool,
}

impl BilliardsBall {
    pub fn new_cue() -> Self {
        Self {
            x: BILLIARDS_TABLE_W * 0.25,
            y: BILLIARDS_TABLE_H * 0.5,
            vx: 0.0,
            vy: 0.0,
            angle: 0.0,
            roll_ux: 1.0,
            roll_uy: 0.0,
            spin_x: 0.0,
            spin_y: 0.0,
            kind: BallKind::Cue,
            number: 0,
            pocketed: false,
            in_play: true,
        }
    }

    pub fn new_object(number: u8, x: f32, y: f32) -> Self {
        let kind = match number {
            8 => BallKind::Eight,
            1..=7 => BallKind::Solid,
            9..=15 => BallKind::Stripe,
            _ => BallKind::Solid,
        };
        Self {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            angle: 0.0,
            roll_ux: 1.0,
            roll_uy: 0.0,
            spin_x: 0.0,
            spin_y: 0.0,
            kind,
            number,
            pocketed: false,
            in_play: true,
        }
    }

    /// Top rengini döndürür (ARGB).
    pub fn color(&self) -> u32 {
        match (self.kind, self.number) {
            (BallKind::Cue, _) => 0xF0F0F0,
            (BallKind::Solid, 1) => 0xE82C2C,   // Kırmızı
            (BallKind::Solid, 2) => 0x2C78E8,   // Mavi
            (BallKind::Solid, 3) => 0xD43ED4,   // Mor
            (BallKind::Solid, 4) => 0xE88C2C,   // Turuncu
            (BallKind::Solid, 5) => 0x2CE86C,   // Yeşil
            (BallKind::Solid, 6) => 0xC82CE8,   // Koyu mor
            (BallKind::Solid, 7) => 0x8C5E2C,   // Kahverengi
            (BallKind::Stripe, 9) => 0xFFE8E8,  // Açık kırmızı (çizgili)
            (BallKind::Stripe, 10) => 0xE8E8FF, // Açık mavi
            (BallKind::Stripe, 11) => 0xFFE8FF, // Açık mor
            (BallKind::Stripe, 12) => 0xFFE8C8, // Açık turuncu
            (BallKind::Stripe, 13) => 0xC8FFC8, // Açık yeşil
            (BallKind::Stripe, 14) => 0xE8C8FF, // Açık mor 2
            (BallKind::Stripe, 15) => 0xE8D4C8, // Açık kahverengi
            (BallKind::Eight, _) => 0x101010,   // Siyah
            _ => 0xFFFFFF,
        }
    }

    /// Çizgili top için beyaz bant rengi.
    pub fn stripe_color(&self) -> u32 {
        0xFFFFFF
    }
}

/// Delik konumları (6 delik: 4 köşe + 2 yan).
pub const BILLIARDS_POCKETS: [(f32, f32); 6] = [
    (BILLIARDS_CUSHION, BILLIARDS_CUSHION),       // Sol üst
    (BILLIARDS_TABLE_W * 0.5, BILLIARDS_CUSHION), // Üst orta
    (BILLIARDS_TABLE_W - BILLIARDS_CUSHION, BILLIARDS_CUSHION), // Sağ üst
    (BILLIARDS_CUSHION, BILLIARDS_TABLE_H - BILLIARDS_CUSHION), // Sol alt
    (
        BILLIARDS_TABLE_W * 0.5,
        BILLIARDS_TABLE_H - BILLIARDS_CUSHION,
    ), // Alt orta
    (
        BILLIARDS_TABLE_W - BILLIARDS_CUSHION,
        BILLIARDS_TABLE_H - BILLIARDS_CUSHION,
    ), // Sağ alt
];

/// Bilardo oyunu durumu.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum BilliardsState {
    #[default]
    Aiming, // Vuruş yönü/power ayarlanıyor
    Shooting,   // Toplar hareket halinde
    BallInHand, // Çap topu elden (foul sonrası)
    GameOver,   // Oyun bitti
}

/// 8-ball oyuncu türü.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum BilliardsPlayerType {
    #[default]
    Solids, // Düz renkli (1-7)
    Stripes, // Çizgili (9-15)
}

/// Bilardo oyunu.
#[derive(Clone, Copy)]
pub struct BilliardsGame {
    pub balls: [BilliardsBall; BILLIARDS_BALL_COUNT],
    pub state: BilliardsState,
    pub current_player: BilliardsPlayerType,
    pub player_assigned: [bool; 2], // 0 = solids, 1 = stripes
    pub eight_ball_pocketed: bool,
    pub cue_ball_in_hand: bool,
    /// Vuruş yönü (radyan).
    pub aim_angle: f32,
    /// Vuruş gücü (0.0 - 1.0).
    pub aim_power: f32,
    /// English spin ayarı: x = yan (sol/sağ), y = üst/alt (follow/draw).
    /// -1.0..1.0 aralığında; 0 ortadır.
    pub english_x: f32,
    pub english_y: f32,
    /// Son vuruş yapan oyuncu (0 = solids, 1 = stripes, 2 = henüz kimse).
    pub last_shooter: u8,
    /// Foul bayrağı.
    pub foul: bool,
    /// Kazanan (None = devam, Some(0) = solids, Some(1) = stripes).
    pub winner: Option<u8>,
    /// Rastgele tohum (AI için).
    pub rng_state: u64,
}

impl Default for BilliardsGame {
    fn default() -> Self {
        Self::new()
    }
}

impl BilliardsGame {
    /// Yeni 8-ball oyunu başlatır.
    pub fn new() -> Self {
        let mut balls = [BilliardsBall::new_cue(); BILLIARDS_BALL_COUNT];
        balls[0] = BilliardsBall::new_cue();

        // Standart 8-ball üçgen dizilimi (sol üstten başlayarak)
        //       1
        //     2   3
        //   4   8   5
        // 6   7   9   10
        //  11  12  13  14  15
        let rack_x = BILLIARDS_TABLE_W * 0.72;
        let rack_y = BILLIARDS_TABLE_H * 0.5;
        let spacing = BILLIARDS_BALL_R * 2.05;

        let positions = [
            (0, rack_x, rack_y),                                    // 1 (apex)
            (1, rack_x + spacing * 0.866, rack_y - spacing * 0.5),  // 2
            (2, rack_x + spacing * 0.866, rack_y + spacing * 0.5),  // 3
            (3, rack_x + spacing * 1.732, rack_y - spacing),        // 4
            (4, rack_x + spacing * 1.732, rack_y),                  // 8 (orta)
            (5, rack_x + spacing * 1.732, rack_y + spacing),        // 5
            (6, rack_x + spacing * 2.598, rack_y - spacing * 1.5),  // 6
            (7, rack_x + spacing * 2.598, rack_y - spacing * 0.5),  // 7
            (8, rack_x + spacing * 2.598, rack_y + spacing * 0.5),  // 9
            (9, rack_x + spacing * 2.598, rack_y + spacing * 1.5),  // 10
            (10, rack_x + spacing * 3.464, rack_y - spacing * 2.0), // 11
            (11, rack_x + spacing * 3.464, rack_y - spacing),       // 12
            (12, rack_x + spacing * 3.464, rack_y),                 // 13
            (13, rack_x + spacing * 3.464, rack_y + spacing),       // 14
            (14, rack_x + spacing * 3.464, rack_y + spacing * 2.0), // 15
        ];

        for &(idx, x, y) in &positions {
            balls[idx + 1] = BilliardsBall::new_object((idx + 1) as u8, x, y);
        }

        Self {
            balls,
            state: BilliardsState::Aiming,
            current_player: BilliardsPlayerType::Solids,
            player_assigned: [false, false],
            eight_ball_pocketed: false,
            cue_ball_in_hand: false,
            aim_angle: 0.0,
            aim_power: 0.5,
            english_x: 0.0,
            english_y: 0.0,
            last_shooter: 2,
            foul: false,
            winner: None,
            rng_state: 0xB111AED5EED1234,
        }
    }

    /// Oyunu sıfırlar.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Rastgele sayı üretir.
    fn rng_next(&mut self) -> u64 {
        self.rng_state = self
            .rng_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.rng_state
    }

    /// Çap topunu (beyaz) döndürür (mutable).
    pub fn cue_ball(&mut self) -> &mut BilliardsBall {
        &mut self.balls[0]
    }

    /// Çap topunu (beyaz) döndürür (immutable).
    pub fn cue_ball_ref(&self) -> &BilliardsBall {
        &self.balls[0]
    }

    /// Tüm toplar durdu mu?
    pub fn all_stopped(&self) -> bool {
        for ball in &self.balls {
            if ball.in_play
                && (ball.vx.abs() > BILLIARDS_MIN_SPEED || ball.vy.abs() > BILLIARDS_MIN_SPEED)
            {
                return false;
            }
        }
        true
    }

    /// Top-masa kenarı çarpışması (index ile).
    fn collide_cushion_index(&mut self, i: usize) {
        let left = BILLIARDS_CUSHION + BILLIARDS_BALL_R;
        let right = BILLIARDS_TABLE_W - BILLIARDS_CUSHION - BILLIARDS_BALL_R;
        let top = BILLIARDS_CUSHION + BILLIARDS_BALL_R;
        let bottom = BILLIARDS_TABLE_H - BILLIARDS_CUSHION - BILLIARDS_BALL_R;

        let ball = &mut self.balls[i];
        if ball.x <= left {
            ball.x = left;
            ball.vx = -ball.vx * 0.85;
        } else if ball.x >= right {
            ball.x = right;
            ball.vx = -ball.vx * 0.85;
        }
        if ball.y <= top {
            ball.y = top;
            ball.vy = -ball.vy * 0.85;
        } else if ball.y >= bottom {
            ball.y = bottom;
            ball.vy = -ball.vy * 0.85;
        }
        // Çarpma yönü değişir; yuvarlanma ekseni tick'te hızdan
        // yeniden hesaplandığı için burada ek iş gerekmez.
    }

    /// Top-top çarpışması (elastik).
    fn collide_balls(&mut self, i: usize, j: usize) {
        let bi = self.balls[i];
        let bj = self.balls[j];
        if !bi.in_play || !bj.in_play {
            return;
        }
        let dx = bj.x - bi.x;
        let dy = bj.y - bi.y;
        let dist_sq = dx * dx + dy * dy;
        let min_dist = BILLIARDS_BALL_R * 2.0;
        if dist_sq >= min_dist * min_dist || dist_sq < 0.000001 {
            return;
        }
        let dist = billiards_sqrt(dist_sq);
        let nx = dx / dist;
        let ny = dy / dist;

        // Hız farkı
        let dvx = bj.vx - bi.vx;
        let dvy = bj.vy - bi.vy;
        let dvn = dvx * nx + dvy * ny;

        // Uzaklaşıyorsa çarpışma yok
        if dvn >= 0.0 {
            return;
        }

        // Kütleler eşit (1.0), elastik çarpışma
        let impulse = 2.0 * dvn / 2.0; // (2 * dvn) / (m1 + m2) = dvn
        let ix = impulse * nx;
        let iy = impulse * ny;

        self.balls[i].vx += ix;
        self.balls[i].vy += iy;
        self.balls[j].vx -= ix;
        self.balls[j].vy -= iy;

        // Çarpışma sonrası ayrışma (yapışmayı önle)
        let overlap = min_dist - dist;
        self.balls[i].x -= nx * overlap * 0.5;
        self.balls[i].y -= ny * overlap * 0.5;
        self.balls[j].x += nx * overlap * 0.5;
        self.balls[j].y += ny * overlap * 0.5;
    }

    /// Delik kontrolü.
    fn check_pockets(&mut self) -> bool {
        let mut any_pocketed = false;
        for i in 0..BILLIARDS_BALL_COUNT {
            if !self.balls[i].in_play || self.balls[i].pocketed {
                continue;
            }
            for &(px, py) in &BILLIARDS_POCKETS {
                let dx = self.balls[i].x - px;
                let dy = self.balls[i].y - py;
                if dx * dx + dy * dy < BILLIARDS_POCKET_R * BILLIARDS_POCKET_R {
                    self.balls[i].pocketed = true;
                    self.balls[i].in_play = false;
                    self.balls[i].vx = 0.0;
                    self.balls[i].vy = 0.0;
                    any_pocketed = true;

                    // Çap topu deliğe girdiyse foul
                    if i == 0 {
                        self.foul = true;
                        self.cue_ball_in_hand = true;
                    }
                    break;
                }
            }
        }
        any_pocketed
    }

    /// 8-ball kuralları: kazananı kontrol et.
    fn check_win(&mut self) {
        if self.eight_ball_pocketed {
            return;
        }

        // 8 topu deliğe girdi mi?
        let eight_ball = &self.balls[8]; // 8 numaralı top (index 8 = number 8)
        if eight_ball.pocketed {
            self.eight_ball_pocketed = true;
            // 8 topu deliğe giren oyuncu kazanır EĞER kendi toplarını bitirmişse
            // Basitleştirme: son vuran kazanır (gerçek kural daha karmaşık)
            if self.last_shooter != 2 {
                self.winner = Some(self.last_shooter);
                self.state = BilliardsState::GameOver;
            }
            return;
        }

        // Kendi toplarından kalan var mı?
        let mut solids_left = 0;
        let mut stripes_left = 0;
        for ball in self.balls.iter().skip(1) {
            if ball.in_play {
                match ball.kind {
                    BallKind::Solid => solids_left += 1,
                    BallKind::Stripe => stripes_left += 1,
                    _ => {}
                }
            }
        }

        // Oyuncu ataması henüz yapılmamışsa, ilk yutulan top türüne göre ata
        if !self.player_assigned[0] && !self.player_assigned[1] {
            // İlk vuruşta atama yok, sadece kontrol et
        } else if self.player_assigned[0] && solids_left == 0 {
            // Solids oyuncusu toplarını bitirdi, 8'i vurduysa kazanır
        } else if self.player_assigned[1] && stripes_left == 0 {
            // Stripes oyuncusu toplarını bitirdi
        }
    }

    /// Vuruşu uygular.
    pub fn shoot(&mut self) {
        if self.state != BilliardsState::Aiming {
            return;
        }
        let angle = self.aim_angle;
        let power = self.aim_power.clamp(0.0, 1.0);
        // Smoothstep güç eğrisi: düşük ve orta güçler ince ayar
        // gerektiren hassas vuruşlar için yumuşatılır, %100 tokat kalır.
        let shaped = power * power * (3.0 - 2.0 * power);
        // Üst spin (follow) vuruşu hafif hızlandırır, backspin yavaşlatır.
        let speed = shaped * (1.0 + self.english_y * 0.10);
        let english_x = self.english_x;
        let english_y = self.english_y;
        let cue = self.cue_ball();
        if !cue.in_play {
            return;
        }
        cue.vx = billiards_cos(angle) * speed * BILLIARDS_MAX_SPEED;
        cue.vy = billiards_sin(angle) * speed * BILLIARDS_MAX_SPEED;
        cue.spin_x = english_x;
        cue.spin_y = english_y;
        self.state = BilliardsState::Shooting;
        self.foul = false;
    }

    /// Vuruş yönünü ayarlar (fare/joystick ile).
    pub fn set_aim(&mut self, angle: f32, power: f32) {
        if self.state == BilliardsState::Aiming || self.state == BilliardsState::BallInHand {
            self.aim_angle = angle;
            self.aim_power = billiards_clamp(power, 0.0, 1.0);
        }
    }

    /// English spin ayarını biriktirir (sürükleme ile).
    /// `dx`/`dy` piksel cinsinden işaretçi hareketidir.
    pub fn adjust_english(&mut self, dx: f32, dy: f32) {
        if self.state == BilliardsState::Aiming || self.state == BilliardsState::BallInHand {
            self.english_x = billiards_clamp(self.english_x + dx * 0.012, -1.0, 1.0);
            self.english_y = billiards_clamp(self.english_y - dy * 0.012, -1.0, 1.0);
        }
    }

    /// English spin'i sıfırlar.
    pub fn clear_english(&mut self) {
        self.english_x = 0.0;
        self.english_y = 0.0;
    }

    /// Çap topunu masa üzerine yerleştirir (ball in hand).
    pub fn place_cue_ball(&mut self, x: f32, y: f32) {
        if self.state == BilliardsState::BallInHand || self.cue_ball_in_hand {
            let cue = self.cue_ball();
            cue.x = x.clamp(
                BILLIARDS_CUSHION + BILLIARDS_BALL_R,
                BILLIARDS_TABLE_W - BILLIARDS_CUSHION - BILLIARDS_BALL_R,
            );
            cue.y = y.clamp(
                BILLIARDS_CUSHION + BILLIARDS_BALL_R,
                BILLIARDS_TABLE_H - BILLIARDS_CUSHION - BILLIARDS_BALL_R,
            );
            cue.vx = 0.0;
            cue.vy = 0.0;
            cue.pocketed = false;
            cue.in_play = true;
            self.cue_ball_in_hand = false;
            self.state = BilliardsState::Aiming;
        }
    }

    /// Ana simülasyon adımı.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;

        match self.state {
            BilliardsState::Shooting => {
                // Hızlı bir topun tek bir adımda diğer topu veya çapayı
                // geçmesini (tunneling) önlemek için hareketi ve çarpışma
                // denetimini alt adımlara böler. Bir alt adımda her top en
                // fazla BILLIARDS_SUB_STEP_MAX_DIST kadar ilerler.
                let mut max_speed = 0.0f32;
                for ball in &self.balls {
                    if ball.in_play {
                        let s = billiards_sqrt(ball.vx * ball.vx + ball.vy * ball.vy);
                        if s > max_speed {
                            max_speed = s;
                        }
                    }
                }
                let sub_steps: usize = if max_speed > 0.0 {
                    let ratio = max_speed / BILLIARDS_SUB_STEP_MAX_DIST;
                    let mut n = ratio as usize;
                    if ratio > n as f32 {
                        n += 1;
                    }
                    n.clamp(1, 128)
                } else {
                    1
                };
                let step = 1.0f32 / sub_steps as f32;

                for _ in 0..sub_steps {
                    // Topları hareket ettir
                    for ball in &mut self.balls {
                        if !ball.in_play {
                            continue;
                        }
                        ball.x += ball.vx * step;
                        ball.y += ball.vy * step;
                    }

                    // Kenar çarpışması (ayrı döngü için borrow sorununu önle)
                    for i in 0..BILLIARDS_BALL_COUNT {
                        if self.balls[i].in_play {
                            self.collide_cushion_index(i);
                        }
                    }

                    // Top-top çarpışmaları
                    for i in 0..BILLIARDS_BALL_COUNT {
                        for j in (i + 1)..BILLIARDS_BALL_COUNT {
                            self.collide_balls(i, j);
                        }
                    }
                }

                // Sürtünme, dönüş, english spin ve durdurma (tick başına bir kez).
                for ball in &mut self.balls {
                    if !ball.in_play {
                        continue;
                    }
                    ball.vx *= 1.0 - BILLIARDS_FRICTION;
                    ball.vy *= 1.0 - BILLIARDS_FRICTION;

                    // Çap topu english: yan spin ilerleme yönüne dik ivme
                    // uygular (top yol boyunca kıvrılır); üst/alt spin
                    // hareket doğrultusunda itme/çekme yapar.
                    if ball.kind == BallKind::Cue {
                        let sp = billiards_sqrt(ball.vx * ball.vx + ball.vy * ball.vy);
                        if sp > BILLIARDS_MIN_SPEED {
                            let nx = -ball.vy / sp;
                            let ny = ball.vx / sp;
                            let side = ball.spin_x * BILLIARDS_SPIN_ACCEL * sp;
                            ball.vx += nx * side;
                            ball.vy += ny * side;
                            let fwd = ball.spin_y * BILLIARDS_SPIN_FOLLOW * sp;
                            ball.vx += (ball.vx / sp) * fwd;
                            ball.vy += (ball.vy / sp) * fwd;
                            ball.spin_x *= BILLIARDS_SPIN_DECAY;
                            ball.spin_y *= BILLIARDS_SPIN_DECAY;
                        }
                    }

                    // Yuvarlanma: top, hızına dik yatay eksen etrafında
                    // döner. Bu tick'teki deplasman `speed` dünya birimi
                    // olduğundan açısal ilerleme = speed / yarıçap.
                    let speed = billiards_sqrt(ball.vx * ball.vx + ball.vy * ball.vy);
                    if speed > BILLIARDS_MIN_SPEED {
                        ball.roll_ux = ball.vx / speed;
                        ball.roll_uy = ball.vy / speed;
                        ball.angle += speed / BILLIARDS_BALL_R;
                        if ball.angle > core::f32::consts::TAU {
                            ball.angle -= core::f32::consts::TAU;
                        }
                    }

                    // Minimum hız altına düşerse durdur
                    if speed < BILLIARDS_MIN_SPEED {
                        ball.vx = 0.0;
                        ball.vy = 0.0;
                        ball.angle = 0.0; // Durmuş top şerit desenini döndürmesin
                    }

                    changed = true;
                }

                // Delik kontrolü
                if self.check_pockets() {
                    self.check_win();
                    changed = true;
                }

                // Tüm toplar durdu mu?
                if self.all_stopped() {
                    if self.foul {
                        self.state = BilliardsState::BallInHand;
                        self.cue_ball_in_hand = true;
                    } else {
                        self.state = BilliardsState::Aiming;
                        // Sıra değişimi (basit: foul olmazsa sıra değişmez, foul olursa ball in hand)
                        // Gerçek 8-ball'da: top yutulmazsa sıra değişir
                        // Basit tutuyoruz: foul olmazsa sıra aynı kalır
                    }
                    self.aim_power = 0.5;
                    changed = true;
                }
            }
            BilliardsState::BallInHand => {
                // Çap topu elden - bekle
            }
            _ => {}
        }

        changed
    }

    /// Basit AI: rastgele bir top hedefle ve vur.
    pub fn ai_shoot(&mut self) {
        if self.state != BilliardsState::Aiming {
            return;
        }
        // Çap topu pozisyonu (kopyala borrow sorununu önlemek için)
        let cue_x = self.balls[0].x;
        let cue_y = self.balls[0].y;
        let cue_in_play = self.balls[0].in_play;
        if !cue_in_play {
            return;
        }

        // Oyun içindeki topları bul
        let mut targets: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
        for (i, ball) in self.balls.iter().enumerate().skip(1) {
            if ball.in_play {
                targets.push(i);
            }
        }
        if targets.is_empty() {
            return;
        }

        // Rastgele bir top seç
        let pick = (self.rng_next() as usize) % targets.len();
        let target_idx = targets[pick];
        let target = self.balls[target_idx];

        // Hedef açısını hesapla
        let dx = target.x - cue_x;
        let dy = target.y - cue_y;
        let angle = billiards_atan2(dy, dx);

        // Güç: mesafeye göre ayarla
        let dist = billiards_sqrt(dx * dx + dy * dy);
        let power = billiards_clamp(0.3 + 0.5 * (dist / BILLIARDS_TABLE_W), 0.2, 1.0);

        self.aim_angle = angle;
        self.aim_power = power;
        self.shoot();
    }
}

#[cfg(test)]
mod billiards_tests {
    use super::*;

    #[test]
    fn new_game_has_16_balls() {
        let game = BilliardsGame::new();
        let mut count = 0;
        for ball in &game.balls {
            if ball.in_play {
                count += 1;
            }
        }
        assert_eq!(count, 16);
    }

    #[test]
    fn cue_ball_starts_at_quarter() {
        let game = BilliardsGame::new();
        let cue = &game.balls[0];
        assert!((cue.x - BILLIARDS_TABLE_W * 0.25).abs() < 0.01);
        assert!((cue.y - BILLIARDS_TABLE_H * 0.5).abs() < 0.01);
    }

    #[test]
    fn eight_ball_at_center_of_rack() {
        let game = BilliardsGame::new();
        let eight = &game.balls[8]; // number 8
        assert_eq!(eight.number, 8);
        assert_eq!(eight.kind, BallKind::Eight);
    }

    #[test]
    fn shoot_moves_cue_ball() {
        let mut game = BilliardsGame::new();
        game.aim_angle = 0.0; // sağa
        game.aim_power = 1.0;
        game.shoot();
        assert!(game.balls[0].vx > 0.0);
        assert_eq!(game.state, BilliardsState::Shooting);
    }

    #[test]
    fn friction_stops_ball() {
        let mut game = BilliardsGame::new();
        game.balls[0].vx = 0.1;
        game.balls[0].vy = 0.0;
        game.state = BilliardsState::Shooting;
        for _ in 0..1000 {
            game.tick();
        }
        assert!(game.balls[0].vx.abs() < BILLIARDS_MIN_SPEED);
    }

    #[test]
    fn cushion_reflects_ball() {
        let mut game = BilliardsGame::new();
        game.balls[1].in_play = true;
        game.balls[1].x = BILLIARDS_TABLE_W - BILLIARDS_CUSHION - BILLIARDS_BALL_R + 0.01;
        game.balls[1].vx = 0.1;
        game.collide_cushion_index(1);
        assert!(game.balls[1].vx < 0.0); // Yön değişti
        assert!(game.balls[1].x <= BILLIARDS_TABLE_W - BILLIARDS_CUSHION - BILLIARDS_BALL_R);
    }

    #[test]
    fn pocket_detection_works() {
        let mut game = BilliardsGame::new();
        game.balls[1].in_play = true;
        game.balls[1].x = BILLIARDS_CUSHION; // Sol üst köşe deliği
        game.balls[1].y = BILLIARDS_CUSHION;
        let result = game.check_pockets();
        assert!(result);
        assert!(game.balls[1].pocketed);
        assert!(!game.balls[1].in_play);
    }

    /// Yüksek hızla hareket eden çap topu, durgun bir topun üzerinden
    /// geçmemeli (tunneling): alt adımlandırma sayesinde çarpışma algılanmalı
    /// ve momentum aktarılmalı. Eşit kütleli elastik çarpışmada çap topu
    /// durur, momentum hedef topa geçer.
    #[test]
    fn fast_cue_ball_transfers_momentum_instead_of_tunneling() {
        let mut game = BilliardsGame::new();
        game.state = BilliardsState::Shooting;
        game.balls[0].x = 0.4;
        game.balls[0].y = 0.5;
        game.balls[0].vx = BILLIARDS_MAX_SPEED;
        game.balls[0].vy = 0.0;

        game.balls[1].in_play = true;
        game.balls[1].x = 0.4 + (BILLIARDS_BALL_R * 2.0) + 0.001;
        game.balls[1].y = 0.5;
        game.balls[1].vx = 0.0;
        game.balls[1].vy = 0.0;

        game.tick();

        // Çarpışma algılanmalı: çap topu momentumunu hedef topa aktarıp durmalı.
        // Alt adımlandırma olmadan çap topu hedefin üzerinden geçerdi (tunneling).
        assert!(
            game.balls[0].vx.abs() < 0.01,
            "çap topu çarpmalı ve durmalı, hedefin üzerinden geçmemeli"
        );
    }

    /// Karşı doğru yüksek hızla gelen iki top, birbirinin üzerinden
    //  geçmemeli; elastik çarpışma sonucu yönleri terselenmeli.
    #[test]
    fn fast_balls_moving_toward_each_other_collide() {
        let mut game = BilliardsGame::new();
        game.state = BilliardsState::Shooting;
        // Merkezde, çapalara temas etmeyecek mesafede karşı doğru hareket.
        game.balls[0].x = 0.9;
        game.balls[0].y = 0.5;
        game.balls[0].vx = 0.8;
        game.balls[0].vy = 0.0;

        game.balls[1].in_play = true;
        game.balls[1].x = 1.0;
        game.balls[1].y = 0.5;
        game.balls[1].vx = -0.8;
        game.balls[1].vy = 0.0;

        game.tick();

        // Eşit kütleli elastik çarpışma: çap topunun yönü terselenecek.
        assert!(
            game.balls[0].vx < 0.0,
            "karşı doğru gelen toplar çarpmalı, çap topu geri dönmeli"
        );
    }
}

// ---------------------------------------------------------------------------
// Performans testi (benchmark / stress)
// ---------------------------------------------------------------------------

/// Kare kare renk geçişi (gradyan) üreten yoğun çizim yükü ölçüsü.
/// Bu sadece hesap yüküdür; çizim tarafı aynı işi framebuffer'a uygular.
pub fn shade_mix(a: u32, b: u32, t: u32) -> u32 {
    let mix = |shift: u32| -> u32 {
        let ca = (a >> shift) & 0xFF;
        let cb = (b >> shift) & 0xFF;
        (ca * (256 - t.min(256)) + cb * t.min(256)) >> 8
    };
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

/// Dama araması yükünü ölçmek için: konumda `moves` adet ardışık AI hamlesi oynar.
pub fn bench_checkers_ai(moves: u16, depth: u8) -> u32 {
    let mut board = Board::new();
    let mut played = 0u32;
    for _ in 0..moves {
        let Some(mv) = choose_move(&board, depth) else {
            break;
        };
        if !board.apply(&mv) {
            break;
        }
        played += 1;
    }
    played
}

/// Bulmaca çözücü yükü: `steps` adet rastgele hamle kilohertz çözümlemesi yapar.
pub fn bench_puzzle_work(steps: u32, seed: u64) -> u32 {
    let mut tiles = puzzle_from_seed(seed);
    let mut state = seed ^ 0xDEAD_BEEF;
    let mut done = 0u32;
    for _ in 0..steps {
        if puzzle_solved(&tiles) {
            tiles = puzzle_from_seed(xorshift64(&mut state));
        }
        if puzzle_random_step(&mut tiles, &mut state).is_some() {
            done += 1;
        }
        // Uzaklık hesabı da yüke dahil edilir (gerçek karede çizim için de gerekir).
        let _ = puzzle_distance(&tiles);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_count(board: &Board) -> usize {
        let mut list = Vec::new();
        board.generate(&mut list);
        list.iter().filter(|m| m.is_capture()).count()
    }

    #[test]
    fn initial_board_is_set_up() {
        let board = Board::new();
        assert_eq!(board.count(true), (12, 0));
        assert_eq!(board.count(false), (12, 0));
        assert_eq!(board.side_to_move(), LIGHT_SIDE);
        // Açılışta hiç yeme yok ve 7 normal hamle var (açık renk).
        assert_eq!(capture_count(&board), 0);
        assert_eq!(board.legal_move_count(), 7);
    }

    #[test]
    fn capture_is_mandatory() {
        let mut board = Board::new();
        // Açık taşı (satır 4, sütun 1) koyu taşın önüne taşı, ardından koyu sıra gelsin.
        for cell in board.cells.iter_mut() {
            *cell = EMPTY;
        }
        board.cells[idx(4, 1)] = LIGHT;
        board.cells[idx(3, 2)] = DARK;
        board.cells[idx(0, 7)] = LIGHT;
        board.turn = LIGHT_SIDE;
        let mut list = Vec::new();
        board.generate(&mut list);
        // Tek yasal hamle yeme hamlesidir.
        assert_eq!(list.len(), 1);
        assert!(list[0].is_capture());
        assert_eq!(list[0].from(), idx(4, 1));
        assert_eq!(list[0].to(), idx(2, 3));
    }

    #[test]
    fn multi_capture_chain_is_found() {
        let mut board = Board::new();
        for cell in board.cells.iter_mut() {
            *cell = EMPTY;
        }
        board.cells[idx(5, 0)] = LIGHT;
        board.cells[idx(4, 1)] = DARK;
        board.cells[idx(2, 3)] = DARK;
        board.cells[idx(0, 7)] = LIGHT;
        board.cells[idx(7, 7)] = DARK;
        board.turn = LIGHT_SIDE;
        let mut list = Vec::new();
        board.generate(&mut list);
        let best = list.iter().map(|m| m.captures).max().unwrap_or(0);
        assert_eq!(best, 2);
    }

    #[test]
    fn king_moves_backwards_and_captures_far() {
        let mut board = Board::new();
        for cell in board.cells.iter_mut() {
            *cell = EMPTY;
        }
        board.cells[idx(3, 3)] = LIGHT_KING;
        board.cells[idx(1, 1)] = DARK;
        board.cells[idx(7, 7)] = DARK;
        board.turn = LIGHT_SIDE;
        let mut list = Vec::new();
        board.generate(&mut list);
        assert!(list.iter().any(|m| m.is_capture() && m.to() == idx(0, 0)));
    }

    #[test]
    fn apply_refreshes_turn_and_promotes() {
        let mut board = Board::new();
        for cell in board.cells.iter_mut() {
            *cell = EMPTY;
        }
        board.cells[idx(1, 1)] = LIGHT;
        board.cells[idx(7, 0)] = DARK;
        board.turn = LIGHT_SIDE;
        let mut list = Vec::new();
        board.generate(&mut list);
        let mv = list.iter().find(|m| m.to() == idx(0, 0)).copied().unwrap();
        assert!(board.apply(&mv));
        assert_eq!(board.cells[idx(0, 0)], LIGHT_KING);
        assert_eq!(board.side_to_move(), DARK_SIDE);
    }

    #[test]
    fn ai_plays_a_legal_move() {
        let board = Board::new();
        let mv = choose_move(&board, 1).expect("opening move");
        let mut copy = board;
        assert!(copy.apply(&mv));
        assert_eq!(copy.ply, 1);
    }

    #[test]
    fn ai_takes_a_free_piece() {
        let mut board = Board::new();
        for cell in board.cells.iter_mut() {
            *cell = EMPTY;
        }
        board.cells[idx(4, 1)] = LIGHT;
        board.cells[idx(3, 2)] = DARK;
        board.cells[idx(7, 6)] = DARK;
        board.turn = LIGHT_SIDE;
        let mv = choose_move(&board, 2).expect("capture available");
        assert!(mv.is_capture());
    }

    #[test]
    fn puzzle_generation_is_solvable_and_shuffled() {
        let tiles = puzzle_from_seed(12345);
        assert!(puzzle_blank(&tiles).is_some());
        let mut sorted = tiles;
        sorted.sort();
        for (i, &v) in sorted.iter().enumerate() {
            assert_eq!(v, i as u8);
        }
        // En az birkaç taş yerinden oynamalı.
        assert!(puzzle_distance(&tiles) > 0);
    }

    #[test]
    fn puzzle_move_only_with_blank_neighbour() {
        let mut tiles = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 0];
        assert_eq!(puzzle_blank(&tiles), Some(15));
        // Boş kareyle komşu olmayan taş hareket etmez.
        assert!(puzzle_move(&mut tiles, 0).is_none());
        // Boş karenin solundaki taş (indeks 14) boşluğa kayar.
        let moved = puzzle_move(&mut tiles, 14);
        assert_eq!(moved, Some(15));
        assert_eq!(puzzle_blank(&tiles), Some(14));
        assert!(!puzzle_solved(&tiles));
    }

    #[test]
    fn solved_puzzle_detected() {
        let mut tiles = [0u8; PUZZLE_TILES];
        for i in 0..PUZZLE_TILES {
            tiles[i] = i as u8;
        }
        assert!(puzzle_solved(&tiles));
        assert_eq!(puzzle_distance(&tiles), 0);
    }

    #[test]
    fn bench_helpers_terminate() {
        assert!(bench_checkers_ai(2, 1) >= 1);
        assert!(bench_puzzle_work(50, 7) > 0);
        assert_eq!(shade_mix(0x000000, 0xFFFFFF, 256), 0x00FF_FFFF);
    }

    // ------------------------------------------------------------------
    // Mayın Tarlası testleri
    // ------------------------------------------------------------------

    #[test]
    fn mines_first_click_opens_safe_area() {
        let mut mines = Minesweeper::new();
        assert!(mines.reveal(4, 4));
        for r in 3..=5 {
            for c in 3..=5 {
                assert!(
                    !mines.cells[r * mines.cols + c].is_mine,
                    "ilk tıklama çevresinde mayın olmamalı"
                );
            }
        }
        assert_eq!(mines.cells[4 * mines.cols + 4].adjacent, 0);
        assert!(mines.revealed > 1);
    }

    #[test]
    fn mines_levels_change_board_size() {
        let mut mines = Minesweeper::new();
        mines.set_level(2);
        assert_eq!((mines.rows, mines.cols, mines.mine_count), (16, 16, 50));
        mines.set_level(1);
        assert_eq!((mines.rows, mines.cols, mines.mine_count), (12, 12, 25));
        mines.set_level(0);
        assert_eq!((mines.rows, mines.cols, mines.mine_count), (9, 9, 10));
    }

    #[test]
    fn mines_chord_reveals_when_flags_match() {
        let mut mines = Minesweeper::new();
        mines.rows = 3;
        mines.cols = 3;
        mines.mine_count = 1;
        mines.first_click = false;
        mines.cells[0].is_mine = true;
        mines.recount_adjacent();
        mines.cells[4].state = CellState::Revealed;
        mines.revealed = 1;
        mines.cells[0].state = CellState::Flagged;
        mines.flagged = 1;
        assert!(mines.chord(1, 1));
        assert_eq!(mines.state, GameState::Won);
        assert_eq!(mines.flagged, 1);
    }

    #[test]
    fn mines_flag_toggles_only_hidden_cells() {
        let mut mines = Minesweeper::new();
        assert!(mines.toggle_flag(0, 0));
        assert_eq!(mines.cells[0].state, CellState::Flagged);
        assert!(!mines.reveal(0, 0));
        assert!(mines.toggle_flag(0, 0));
        assert_eq!(mines.cells[0].state, CellState::Hidden);
        assert!(mines.reveal(0, 0));
        assert!(!mines.toggle_flag(0, 0));
    }

    // ------------------------------------------------------------------
    // Satranç testleri
    // ------------------------------------------------------------------

    fn empty_chess() -> ChessBoard {
        let mut board = ChessBoard::new();
        for row in board.cells.iter_mut() {
            for cell in row.iter_mut() {
                *cell = None;
            }
        }
        board
    }

    fn put(board: &mut ChessBoard, r: usize, c: usize, kind: PieceKind, side: ChessSide) {
        board.cells[r][c] = Some(ChessPiece {
            kind,
            side,
            has_moved: false,
        });
    }

    fn has_move(board: &ChessBoard, from: usize, to: usize) -> bool {
        let mut list = Vec::new();
        board.generate_legal(board.side_to_move, &mut list);
        list.iter().any(|m| m.from == from && m.to == to)
    }

    #[test]
    fn chess_castling_moves_rook() {
        let mut board = empty_chess();
        put(&mut board, 7, 4, PieceKind::King, ChessSide::White);
        put(&mut board, 7, 7, PieceKind::Rook, ChessSide::White);
        put(&mut board, 0, 0, PieceKind::King, ChessSide::Black);
        board.side_to_move = ChessSide::White;
        assert!(has_move(&board, 7 * 8 + 4, 7 * 8 + 6));
        let mv = ChessMove {
            from: 7 * 8 + 4,
            to: 7 * 8 + 6,
            promotion: None,
        };
        assert!(board.apply(&mv));
        assert_eq!(board.cells[7][6].map(|p| p.kind), Some(PieceKind::King));
        assert_eq!(board.cells[7][5].map(|p| p.kind), Some(PieceKind::Rook));
    }

    #[test]
    fn chess_cannot_castle_through_check() {
        let mut board = empty_chess();
        put(&mut board, 7, 4, PieceKind::King, ChessSide::White);
        put(&mut board, 7, 7, PieceKind::Rook, ChessSide::White);
        put(&mut board, 0, 0, PieceKind::King, ChessSide::Black);
        put(&mut board, 0, 5, PieceKind::Rook, ChessSide::Black);
        board.side_to_move = ChessSide::White;
        assert!(!has_move(&board, 7 * 8 + 4, 7 * 8 + 6));
    }

    #[test]
    fn chess_en_passant_captures_pawn() {
        let mut board = empty_chess();
        put(&mut board, 7, 4, PieceKind::King, ChessSide::White);
        put(&mut board, 0, 0, PieceKind::King, ChessSide::Black);
        put(&mut board, 3, 4, PieceKind::Pawn, ChessSide::White);
        put(&mut board, 3, 3, PieceKind::Pawn, ChessSide::Black);
        board.side_to_move = ChessSide::White;
        board.en_passant = Some(2 * 8 + 3);
        let mv = ChessMove {
            from: 3 * 8 + 4,
            to: 2 * 8 + 3,
            promotion: None,
        };
        assert!(has_move(&board, mv.from, mv.to));
        assert!(board.apply(&mv));
        assert!(
            board.cells[3][3].is_none(),
            "geçerken alınan piyon silinmeli"
        );
        assert_eq!(board.cells[2][3].map(|p| p.kind), Some(PieceKind::Pawn));
    }

    #[test]
    fn chess_promotion_offers_all_pieces() {
        let mut board = empty_chess();
        put(&mut board, 7, 4, PieceKind::King, ChessSide::White);
        put(&mut board, 0, 7, PieceKind::King, ChessSide::Black);
        put(&mut board, 1, 0, PieceKind::Pawn, ChessSide::White);
        board.side_to_move = ChessSide::White;
        let mut list = Vec::new();
        board.generate_legal(ChessSide::White, &mut list);
        let promotions = list.iter().filter(|m| m.from == 8 && m.to == 0).count();
        assert_eq!(promotions, 4);
        let mv = ChessMove {
            from: 8,
            to: 0,
            promotion: Some(PieceKind::Knight),
        };
        assert!(board.apply(&mv));
        assert_eq!(board.cells[0][0].map(|p| p.kind), Some(PieceKind::Knight));
    }

    #[test]
    fn chess_pinned_piece_cannot_expose_king() {
        let mut board = empty_chess();
        put(&mut board, 7, 4, PieceKind::King, ChessSide::White);
        put(&mut board, 6, 4, PieceKind::Rook, ChessSide::White);
        put(&mut board, 0, 4, PieceKind::Rook, ChessSide::Black);
        put(&mut board, 0, 0, PieceKind::King, ChessSide::Black);
        board.side_to_move = ChessSide::White;
        // Kale e-hattından ayrılırsa şah açıkta kalır.
        assert!(!has_move(&board, 6 * 8 + 4, 6 * 8 + 3));
        // Hat üzerinde kalmak yasaldır.
        assert!(has_move(&board, 6 * 8 + 4, 5 * 8 + 4));
    }

    #[test]
    fn chess_checkmate_is_detected() {
        let mut board = empty_chess();
        put(&mut board, 0, 0, PieceKind::King, ChessSide::Black);
        put(&mut board, 1, 1, PieceKind::Queen, ChessSide::White);
        put(&mut board, 2, 2, PieceKind::King, ChessSide::White);
        board.side_to_move = ChessSide::Black;
        board.refresh_status();
        assert_eq!(board.status, ChessStatus::Checkmate(ChessSide::White));
    }

    #[test]
    fn chess_stalemate_is_detected() {
        let mut board = empty_chess();
        put(&mut board, 0, 0, PieceKind::King, ChessSide::Black);
        put(&mut board, 2, 1, PieceKind::Queen, ChessSide::White);
        put(&mut board, 2, 2, PieceKind::King, ChessSide::White);
        board.side_to_move = ChessSide::Black;
        board.refresh_status();
        assert_eq!(board.status, ChessStatus::Stalemate);
    }

    #[test]
    fn chess_insufficient_material_is_draw() {
        let mut board = empty_chess();
        put(&mut board, 7, 4, PieceKind::King, ChessSide::White);
        put(&mut board, 0, 0, PieceKind::King, ChessSide::Black);
        put(&mut board, 5, 5, PieceKind::Bishop, ChessSide::White);
        board.side_to_move = ChessSide::Black;
        board.refresh_status();
        assert_eq!(board.status, ChessStatus::Draw);
    }
}
