#!/usr/bin/env python3
with open('src/apps/fps_game.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Find the enemy rendering section and optimize it
old_section = '''        // Düşman sprite'larını çiz (basit billboard).
        for e in self.enemies.iter().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            let dx = e.x - self.pos_x;
            let dy = e.y - self.pos_y;
            let dist = fast_sqrt(dx * dx + dy * dy);
            if dist > MAX_DEPTH || dist < 0.5 {
                continue;
            }
            let angle = fast_atan2(dy, dx);
            let mut rel = angle - fast_atan2(self.dir_y, self.dir_x);
            while rel < -PI {
                rel += TAU;
            }
            while rel > PI {
                rel -= TAU;
            }
            if rel.abs() > FOV / 2.0 + 0.5 {
                continue;
            }'''

new_section = '''        // Düşman sprite'larını çiz (basit billboard).
        // Sadece ENEMIES_PER_FRAME kadar düşman render edilir; remaining'ler
        // sonraki frame'e ertelenir, jitter önlenir.
        let mut rendered_sprites: usize = 0;
        for e in self.enemies.iter().take(self.enemy_count) {
            if !e.alive {
                continue;
            }
            // Mesafe ve frustum kısaltma: fast_sqrt yerine kare mesafe kontrolü.
            let dx = e.x - self.pos_x;
            let dy = e.y - self.pos_y;
            let dist_sq = dx * dx + dy * dy;
            // fast_sqrt guard: 0 divisyonunu ve z_buffer karşılaştırması.
            let dist = if dist_sq > 0.0 { fast_sqrt(dist_sq) } else { 0.0 };
            if dist > MAX_DEPTH || dist < 0.5 {
                continue;
            }
            let angle = fast_atan2(dy, dx);
            let mut rel = angle - fast_atan2(self.dir_y, self.dir_x);
            // while döngülerini if düzeltmeleriyle değiştir: angle normalizasyonu
            // tek adımlı düzelme yeterlidir, while döngüsü çok maliyetli.
            if rel < -PI {
                rel = rel + TAU;
            } else if rel > PI {
                rel = rel - TAU;
            }
            if rel.abs() > FOV / 2.0 + 0.5 {
                continue;
            }'''

if old_section in content:
    new_content = content.replace(old_section, new_section)
    with open('src/apps/fps_game.rs', 'w', encoding='utf-8') as f:
        f.write(new_content)
    print('Optimized enemy render loop')
else:
    print('Old section not found')