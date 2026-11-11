import sys

path = 'src/main.rs'
with open(path, 'r', encoding='utf-8', errors='ignore') as f:
    c = f.read()

old = """                    // Aero Snap kenar algilama & onizleme kutusu

                    let old_snap = active_snap_preview;

                    let usable_h = height.saturating_sub(gui::taskbar_h());

                    let half_w = width / 2;

                    if my <= 4 {
                        active_snap_preview = Some((0, 0, width, usable_h));
                    } else if mx <= 4 {
                        active_snap_preview = Some((0, 0, half_w, usable_h));
                    } else if mx >= width.saturating_sub(5) {
                        active_snap_preview =
                            Some((half_w, 0, width.saturating_sub(half_w), usable_h));
                    } else {
                        active_snap_preview = None;
                    }"""

new = """                    // Aero Snap kenar/köşe algılama & önizleme kutusu

                    let old_snap = active_snap_preview;

                    let usable_h = height.saturating_sub(gui::taskbar_h());

                    let half_w = width / 2;
                    let half_h = usable_h / 2;

                    if my <= 4 {
                        active_snap_preview = Some((0, 0, width, usable_h));
                    } else if my >= usable_h.saturating_sub(5) {
                        if mx <= 4 {
                            active_snap_preview = Some((0, half_h, half_w, usable_h.saturating_sub(half_h)));
                        } else if mx >= width.saturating_sub(5) {
                            active_snap_preview = Some((half_w, half_h, width.saturating_sub(half_w), usable_h.saturating_sub(half_h)));
                        } else if mx < half_w {
                            active_snap_preview = Some((0, half_h, half_w, usable_h.saturating_sub(half_h)));
                        } else {
                            active_snap_preview = Some((half_w, half_h, width.saturating_sub(half_w), usable_h.saturating_sub(half_h)));
                        }
                    } else if mx <= 4 {
                        if my < half_h {
                            active_snap_preview = Some((0, 0, half_w, half_h));
                        } else {
                            active_snap_preview = Some((0, half_h, half_w, usable_h.saturating_sub(half_h)));
                        }
                    } else if mx >= width.saturating_sub(5) {
                        if my < half_h {
                            active_snap_preview = Some((half_w, 0, width.saturating_sub(half_w), half_h));
                        } else {
                            active_snap_preview = Some((half_w, half_h, width.saturating_sub(half_w), usable_h.saturating_sub(half_h)));
                        }
                    } else {
                        active_snap_preview = None;
                    }"""

if old in c:
    c2 = c.replace(old, new)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(c2)
    print('done')
else:
    print('not found')
