//! Labirent çekirdeği: oynanabilir 3D FPS labirenti için harita üretimi,
//! yol bulma ve ses biçimlendirme yardımcıları.
//!
//! Bu modül tamamen `no_std` ve `alloc`-suz çalışır; çizim tarafı
//! `fps_game.rs` içindedir. Ayrıştırmanın amacı, labirent kurallarını
//! (üretim, bağlantılılık, mesafe) host testlerinde doğrulanabilir kılmaktır.

use alloc::vec::Vec;

/// Labirent kenarı (tek sayı olmalıdır).
pub const MAZE_SIZE: usize = 21;
/// Hücre başına dünya ölçüsü (oyuncu yarıçapı bu birime göredir).
pub const CELL_WORLD: f32 = 1.0;
/// Çıkışın yerleştirildiği hücre (sağ alt köşe içi).
pub const EXIT_CELL: (usize, usize) = (MAZE_SIZE - 2, MAZE_SIZE - 2);

/// Haritadaki kare türleri.
pub const TILE_EMPTY: u8 = 0;
pub const TILE_WALL: u8 = 1;
/// Kilitli kapı: anahtar toplanmadan geçilemez.
pub const TILE_LOCKED: u8 = 2;
/// Açılan kapı / bölge (oyun sırasında kilidi açılır).
pub const TILE_OPENED: u8 = 3;
/// Asansör/çıkış işareti (yalnızca zemin boyaması; duvar değildir).
pub const TILE_EXIT: u8 = 4;
/// Anahtar eşyası (kapıları açmak için toplanır).
pub const TILE_KEY: u8 = 5;

/// Maksimum kapı sayısı.
pub const MAX_DOORS: usize = 8;

/// Basit xorshift64* üreticisi (deterministik labirent üretimi için).
#[derive(Clone, Copy)]
pub struct Rng(u64);

impl Rng {
    pub const fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// `0..n` aralığında tekdüze değer (n > 0).
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    /// `[0, 1)` aralığında ondalık.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        ((self.next_u32() >> 8) as f32) / 16_777_216.0
    }

    /// `[-1, 1)` aralığında ondalık.
    #[inline]
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

/// Labirent haritası ve çözüm yolu.
pub struct MazeMap {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<u8>,
    /// Oyuncunun başlangıç hücresi.
    pub start: (usize, usize),
    /// Çıkış hücresi.
    pub exit: (usize, usize),
    /// Başlangıçtan çıkışa en kısa hücre yolu (BFS).
    pub solution: Vec<(usize, usize)>,
    /// Anahtarın konumu (None = henüz yerleştirilmedi veya alındı).
    pub key_pos: Option<(usize, usize)>,
    /// Oyuncunun anahtara sahip olup olmadığı.
    pub has_key: bool,
    /// Kapı konumları.
    pub doors: [(usize, usize); MAX_DOORS],
    pub door_count: usize,
}

impl MazeMap {
    /// Boş ızgara oluşturur.
    pub fn blank(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            cells: alloc::vec![TILE_WALL; w * h],
            start: (1, 1),
            exit: (w.saturating_sub(2).max(1), h.saturating_sub(2).max(1)),
            solution: Vec::new(),
            key_pos: None,
            has_key: false,
            doors: [(0, 0); MAX_DOORS],
            door_count: 0,
        }
    }

    #[inline]
    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> u8 {
        if x >= self.w || y >= self.h {
            TILE_WALL
        } else {
            self.cells[self.idx(x, y)]
        }
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, tile: u8) {
        if x < self.w && y < self.h {
            let i = self.idx(x, y);
            self.cells[i] = tile;
        }
    }

    /// Geçilebilir kare mi? (Duvar ve kilitli kapı geçilmez; anahtar geçilebilir.)
    #[inline]
    pub fn walkable(&self, x: usize, y: usize) -> bool {
        let t = self.get(x, y);
        t == TILE_EMPTY || t == TILE_OPENED || t == TILE_EXIT || t == TILE_KEY
    }

    /// Verilen hücre kapı mı?
    #[inline]
    pub fn is_door(&self, x: usize, y: usize) -> bool {
        self.get(x, y) == TILE_LOCKED
    }

    /// Anahtarı al (üstüne gelindiğinde çağrılır). true dönerse anahtar alındı.
    pub fn pickup_key(&mut self, x: usize, y: usize) -> bool {
        if let Some((kx, ky)) = self.key_pos {
            if kx == x && ky == y && !self.has_key {
                self.has_key = true;
                self.key_pos = None;
                self.set(x, y, TILE_EMPTY);
                return true;
            }
        }
        false
    }

    /// Kapıyı aç (oyuncu anahtara sahipse). true dönerse kapı açıldı.
    pub fn open_door(&mut self, x: usize, y: usize) -> bool {
        if !self.has_key {
            return false;
        }
        if self.get(x, y) == TILE_LOCKED {
            self.set(x, y, TILE_OPENED);
            return true;
        }
        false
    }

    /// İki hücre arasında doğrudan görüş var mı? (Basit ızgara yürüyüşü.)
    pub fn line_of_sight(&self, ax: f32, ay: f32, bx: f32, by: f32) -> bool {
        let dx = bx - ax;
        let dy = by - ay;
        let steps = {
            let len = fast_abs(dx) + fast_abs(dy);
            (len * 4.0) as usize + 1
        };
        let mut i = 1;
        while i < steps {
            let t = i as f32 / steps as f32;
            let x = ax + dx * t;
            let y = ay + dy * t;
            if x < 0.0 || y < 0.0 {
                return false;
            }
            if !self.walkable(x as usize, y as usize) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// BFS ile en kısa yolu hesaplar; `solution` alanına yazar.
    pub fn compute_solution(&mut self) {
        self.solution.clear();
        let n = self.w * self.h;
        let mut prev = alloc::vec![usize::MAX; n];
        let mut seen = alloc::vec![false; n];
        let mut queue = alloc::collections::VecDeque::new();
        let start = self.idx(self.start.0, self.start.1);
        let goal = self.idx(self.exit.0, self.exit.1);
        seen[start] = true;
        queue.push_back(start);
        while let Some(cur) = queue.pop_front() {
            if cur == goal {
                break;
            }
            let cx = cur % self.w;
            let cy = cur / self.w;
            for (nx, ny) in neighbours4(cx, cy, self.w, self.h) {
                if !self.walkable(nx, ny) {
                    continue;
                }
                let ni = self.idx(nx, ny);
                if seen[ni] {
                    continue;
                }
                seen[ni] = true;
                prev[ni] = cur;
                queue.push_back(ni);
            }
        }
        if !seen[goal] {
            return;
        }
        let mut cur = goal;
        let mut path = Vec::new();
        loop {
            path.push((cur % self.w, cur / self.w));
            if cur == start {
                break;
            }
            cur = prev[cur];
            if cur == usize::MAX {
                return;
            }
        }
        path.reverse();
        self.solution = path;
    }

    /// Başlangıçtan çıkışa hücre cinsinden mesafe.
    pub fn path_cells(&self) -> usize {
        self.solution.len()
    }
}

#[inline]
pub fn fast_abs(x: f32) -> f32 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// Dört komşu hücre (harita sınırları içinde).
pub fn neighbours4(x: usize, y: usize, w: usize, h: usize) -> impl Iterator<Item = (usize, usize)> {
    let mut out = [(0usize, 0usize); 4];
    let mut n = 0;
    if x > 0 {
        out[n] = (x - 1, y);
        n += 1;
    }
    if x + 1 < w {
        out[n] = (x + 1, y);
        n += 1;
    }
    if y > 0 {
        out[n] = (x, y - 1);
        n += 1;
    }
    if y + 1 < h {
        out[n] = (x, y + 1);
        n += 1;
    }
    out.into_iter().take(n)
}

/// Kusursuz labirent üretir (recursive backtracker): her hücreye tek yol.
/// Duvarlar `TILE_WALL`, koridorlar `TILE_EMPTY` olarak yazılır.
pub fn generate_maze(w: usize, h: usize, seed: u64) -> MazeMap {
    let mut map = MazeMap::blank(w.max(5) | 1, h.max(5) | 1);
    let mut rng = Rng::new(seed);
    let (mw, mh) = (map.w, map.h);
    // Hücre koordinatları tek sayılardır.
    let cells_x = (mw - 1) / 2;
    let cells_y = (mh - 1) / 2;
    let mut visited = alloc::vec![false; cells_x * cells_y];
    let mut stack: Vec<(usize, usize)> = Vec::with_capacity(cells_x * cells_y);

    let start_cell = (0usize, 0usize);
    visited[0] = true;
    stack.push(start_cell);
    map.set(1, 1, TILE_EMPTY);

    while let Some(&(cx, cy)) = stack.last() {
        let mut options = [(0usize, 0usize); 4];
        let mut n = 0usize;
        if cx > 0 && !visited[(cx - 1) + cy * cells_x] {
            options[n] = (cx - 1, cy);
            n += 1;
        }
        if cx + 1 < cells_x && !visited[(cx + 1) + cy * cells_x] {
            options[n] = (cx + 1, cy);
            n += 1;
        }
        if cy > 0 && !visited[cx + (cy - 1) * cells_x] {
            options[n] = (cx, cy - 1);
            n += 1;
        }
        if cy + 1 < cells_y && !visited[cx + (cy + 1) * cells_x] {
            options[n] = (cx, cy + 1);
            n += 1;
        }
        if n == 0 {
            stack.pop();
            continue;
        }
        let (nx, ny) = options[rng.below(n)];
        // Aradaki duvarı yık ve yeni hücreyi aç (hücre koordinatları tek sayı).
        let wx = cx * 2 + 1 + nx - cx;
        let wy = cy * 2 + 1 + ny - cy;
        map.set(wx, wy, TILE_EMPTY);
        map.set(nx * 2 + 1, ny * 2 + 1, TILE_EMPTY);
        visited[nx + ny * cells_x] = true;
        stack.push((nx, ny));
    }

    // Kusursuz labirentte çıkmaz sokaklar oyunu tek yönlü kılar; birkaç
    // döngü (halka) ekleyerek kaçış yollarını çeşitlendir.
    let loops = cells_x + cells_y;
    for _ in 0..loops {
        let cx = 1 + rng.below(cells_x) * 2;
        let cy = 1 + rng.below(cells_y) * 2;
        let horizontal = rng.below(2) == 0;
        if horizontal && cx + 1 < mw - 1 {
            map.set(cx + 1, cy, TILE_EMPTY);
        } else if cy + 1 < mh - 1 {
            map.set(cx, cy + 1, TILE_EMPTY);
        }
    }

    // Başlangıç odasını genişlet (oyuncu görüşü ve nefes alanı).
    map.set(1, 1, TILE_EMPTY);
    map.set(1, 2, TILE_EMPTY);
    map.set(2, 1, TILE_EMPTY);

    map.start = (1, 1);
    map.exit = ((mw - 2) | 1, (mh - 2) | 1);
    map.set(map.exit.0, map.exit.1, TILE_EXIT);

    // Küçük odalar oluştur: bazı hücreleri 3x3 boş alan olarak aç ve
    // girişine kilitli kapı koy. Anahtar başlangıç bölgesinde yerleştirilir.
    carve_rooms_with_doors(&mut map, &mut rng);

    map.compute_solution();
    map
}

/// Labirente küçük odalar ekler ve odaların girişine kilitli kapılar koyar.
/// Her oda 3x3 boş alandan oluşur; kapı yalnızca anahtar ile açılabilir.
fn carve_rooms_with_doors(map: &mut MazeMap, rng: &mut Rng) {
    let (mw, mh) = (map.w, map.h);
    let room_count = 4usize.min((mw / 6).max(1) * (mh / 6).max(1));

    for _ in 0..room_count {
        // Rastgele bir oda merkezi seç (çift koordinatlar, duvarın içi).
        let rx = 2 + rng.below((mw - 4) / 2) * 2;
        let ry = 2 + rng.below((mh - 4) / 2) * 2;

        // Zaten oda veya kapı varsa atla.
        if map.get(rx, ry) != TILE_EMPTY && map.get(rx, ry) != TILE_WALL {
            continue;
        }

        // 3x3 odayı aç (merkez ve komşuları).
        for dy in 0..3 {
            for dx in 0..3 {
                let x = rx.saturating_sub(1) + dx;
                let y = ry.saturating_sub(1) + dy;
                if x > 0 && x < mw - 1 && y > 0 && y < mh - 1 {
                    map.set(x, y, TILE_EMPTY);
                }
            }
        }

        // Oda girişine bir kapı koy (rastgele bir kenarda).
        let side = rng.below(4);
        let (dx, dy) = match side {
            0 => (0, -1), // üst
            1 => (0, 1),  // alt
            2 => (-1, 0), // sol
            _ => (1, 0),  // sağ
        };
        let door_x = rx as isize + dx;
        let door_y = ry as isize + dy;
        if door_x > 0 && door_x < mw as isize - 1 && door_y > 0 && door_y < mh as isize - 1 {
            let dx = door_x as usize;
            let dy = door_y as usize;
            if (map.get(dx, dy) == TILE_EMPTY || map.get(dx, dy) == TILE_WALL)
                && map.door_count < MAX_DOORS
            {
                map.set(dx, dy, TILE_LOCKED);
                map.doors[map.door_count] = (dx, dy);
                map.door_count += 1;
            }
        }
    }

    // Anahtarı başlangıç bölgesine koy (kapıdan önce erişilebilir).
    if map.key_pos.is_none() {
        map.key_pos = Some((3, 3));
        map.set(3, 3, TILE_KEY);
    }
}

/// Labirentin tamamı başlangıçtan erişilebilir mi? (Bağlantılılık testi.)
/// Kapılar kilitli olduğu için varsayılan olarak erişilebilir sayılmaz.
pub fn all_cells_reachable(map: &MazeMap) -> bool {
    let n = map.w * map.h;
    let mut seen = alloc::vec![false; n];
    let mut queue = alloc::collections::VecDeque::new();
    let s = map.idx(map.start.0, map.start.1);
    seen[s] = true;
    queue.push_back(s);
    let mut open_count = 0usize;
    let mut reached_open = 0usize;
    for &c in map.cells.iter() {
        if c == TILE_EMPTY || c == TILE_EXIT || c == TILE_KEY {
            open_count += 1;
        }
    }
    while let Some(cur) = queue.pop_front() {
        if map.cells[cur] == TILE_EMPTY || map.cells[cur] == TILE_EXIT || map.cells[cur] == TILE_KEY
        {
            reached_open += 1;
        }
        let cx = cur % map.w;
        let cy = cur / map.w;
        for (nx, ny) in neighbours4(cx, cy, map.w, map.h) {
            if !map.walkable(nx, ny) {
                continue;
            }
            let ni = map.idx(nx, ny);
            if !seen[ni] {
                seen[ni] = true;
                queue.push_back(ni);
            }
        }
    }
    reached_open == open_count
}

// ---------------------------------------------------------------------------
// Ses biçimlendirme
// ---------------------------------------------------------------------------

/// Ses isteği: (frekans Hz, PCM genliği 0..=127, süre ms).
pub type Tone = (u32, u8, usize);

/// Ses efektleri.
pub fn sfx(kind: SfxKind) -> Tone {
    match kind {
        SfxKind::Shoot => (1250, 90, 45),
        SfxKind::Hit => (760, 110, 55),
        SfxKind::Step => (170, 45, 22),
        SfxKind::Pickup => (1480, 80, 60),
        SfxKind::Hurt => (320, 120, 70),
        SfxKind::Empty => (480, 60, 30),
        SfxKind::Key => (1760, 85, 75),
        SfxKind::Open => (620, 90, 90),
        SfxKind::Win => (1980, 100, 220),
        SfxKind::Lose => (150, 110, 260),
        SfxKind::Toggle => (980, 55, 35),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SfxKind {
    Shoot,
    Hit,
    Step,
    Pickup,
    Hurt,
    Empty,
    Key,
    Open,
    Win,
    Lose,
    Toggle,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn generated_maze_is_connected() {
        for seed in [1u64, 7, 42, 1337, 0xDEAD_BEEF] {
            let mut map = generate_maze(MAZE_SIZE, MAZE_SIZE, seed);
            assert_eq!(map.w, MAZE_SIZE);
            assert_eq!(map.h, MAZE_SIZE);

            // Anahtarı al ve tüm kapıları aç, ardından bağlantıyı kontrol et.
            if let Some((kx, ky)) = map.key_pos {
                map.pickup_key(kx, ky);
            }
            for d in map.doors.iter().take(map.door_count) {
                map.open_door(d.0, d.1);
            }
            assert!(all_cells_reachable(&map), "seed {seed} kopuk labirent");
            assert!(map.path_cells() > MAZE_SIZE, "seed {seed} çözüm çok kısa");
            let (ex, ey) = map.exit;
            assert!(map.walkable(ex, ey));
        }
    }

    #[test]
    fn maze_start_is_open_and_surrounded() {
        let map = generate_maze(MAZE_SIZE, MAZE_SIZE, 99);
        assert_eq!(map.start, (1, 1));
        assert!(map.walkable(1, 1));
        assert!(!map.walkable(0, 1));
        assert!(!map.walkable(1, 0));
    }

    #[test]
    fn line_of_sight_through_wall_is_blocked() {
        let mut map = MazeMap::blank(5, 5);
        for x in 1..4 {
            map.set(x, 1, TILE_EMPTY);
        }
        map.set(2, 1, TILE_WALL);
        assert!(!map.line_of_sight(1.5, 1.5, 3.5, 1.5));
        map.set(2, 1, TILE_EMPTY);
        assert!(map.line_of_sight(1.5, 1.5, 3.5, 1.5));
    }

    #[test]
    fn solution_path_is_contiguous() {
        let map = generate_maze(MAZE_SIZE, MAZE_SIZE, 12345);
        let path = map.solution.clone();
        assert!(path.len() >= 2);
        assert_eq!(path[0], map.start);
        assert_eq!(path[path.len() - 1], map.exit);
        for pair in path.windows(2) {
            let (ax, ay) = pair[0];
            let (bx, by) = pair[1];
            assert_eq!(ax.abs_diff(bx) + ay.abs_diff(by), 1);
        }
    }

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new(5);
        let mut b = Rng::new(5);
        for _ in 0..16 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn sfx_table_is_sane() {
        let kinds = [
            SfxKind::Shoot,
            SfxKind::Hit,
            SfxKind::Step,
            SfxKind::Pickup,
            SfxKind::Hurt,
            SfxKind::Empty,
            SfxKind::Key,
            SfxKind::Open,
            SfxKind::Win,
            SfxKind::Lose,
            SfxKind::Toggle,
        ];
        let _ = vec![0u8; 0];
        for k in kinds {
            let (f, amp, ms) = sfx(k);
            assert!(f > 0 && amp > 0 && amp <= 127 && ms > 0);
        }
    }

    #[test]
    fn key_and_door_mechanics() {
        let mut map = generate_maze(MAZE_SIZE, MAZE_SIZE, 42);

        // Anahtar konumu olmalı.
        assert!(map.key_pos.is_some());
        let (kx, ky) = map.key_pos.unwrap();
        assert_eq!(map.get(kx, ky), TILE_KEY);

        // Kapılar olmalı ve kilitli olmalı.
        assert!(map.door_count > 0);
        for d in map.doors.iter().take(map.door_count) {
            assert_eq!(map.get(d.0, d.1), TILE_LOCKED);
        }

        // Anahtar yoksa kapı açılamaz.
        for d in map.doors.iter().take(map.door_count) {
            assert!(!map.open_door(d.0, d.1));
        }

        // Anahtarı al.
        assert!(map.pickup_key(kx, ky));
        assert!(map.has_key);
        assert_eq!(map.get(kx, ky), TILE_EMPTY); // anahtar hücresi boşaltıldı

        // Artık kapılar açılebilir.
        for d in map.doors.iter().take(map.door_count) {
            assert!(map.open_door(d.0, d.1));
            assert_eq!(map.get(d.0, d.1), TILE_OPENED);
        }
    }

    #[test]
    fn door_blocks_movement_until_opened() {
        let mut map = generate_maze(MAZE_SIZE, MAZE_SIZE, 7);
        assert!(map.door_count > 0);

        // İlk kapının bir komşusu boş olmalı (koridorda).
        let (dx, dy) = map.doors[0];
        let mut has_walkable_neighbor = false;
        for (nx, ny) in neighbours4(dx, dy, map.w, map.h) {
            if map.walkable(nx, ny) {
                has_walkable_neighbor = true;
                break;
            }
        }
        assert!(
            has_walkable_neighbor,
            "kapının komşusunda geçilebilir hücre olmalı"
        );

        // Kapı kilitli olduğu için geçilemez.
        assert!(!map.walkable(dx, dy));

        // Anahtarı al ve kapıyı aç.
        if let Some((kx, ky)) = map.key_pos {
            map.pickup_key(kx, ky);
        }
        assert!(map.open_door(dx, dy));
        assert!(map.walkable(dx, dy));
    }
}
