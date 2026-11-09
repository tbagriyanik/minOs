#!/usr/bin/env python3
with open('src/apps/fps_game.rs', 'r') as f:
    lines = f.readlines()

# Find and insert after const ROT_SPEED line
new_lines = []
inserted = False
for i, line in enumerate(lines):
    new_lines.append(line)
    if 'const ROT_SPEED: f32 = 0.10;' in line and not inserted:
        # Add the new constant after this line
        new_lines.append('\n')
        new_lines.append('// Render performansı: her frame'\''de tüm düşmanları çizmek (put_pixel\n')
        new_lines.append('// iç içe döngülerle) maliyetlidir. ENEMIES_PER_FRAME ile sınırlı;\n')
        new_lines.append('// remaining ler sonraki frame'\''e ertelenir; akıcılık için yeterlidir.\n')
        new_lines.append('const ENEMIES_PER_FRAME: usize = 4;\n')
        inserted = True

with open('src/apps/fps_game.rs', 'w') as f:
    f.writelines(new_lines)
print('Done inserting ENEMIES_PER_FRAME')