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
}
