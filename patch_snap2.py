path = 'src/main.rs'
with open(path, 'r', encoding='utf-8', errors='ignore') as f:
    c = f.read()

old = """            // Modern Aero Snap: baslik cubugu ekran kenarlarina suruklenip birakildiginda:

            // - Ekranin ustu: Tam ekran (Maximize)

            // - Ekranin solu: Sol 50% bolunmus ekran

            // - Ekranin sagi: Sag 50% bolunmus ekran

            if let Some(idx) = dragging_win {
                compositor.focus_window(idx);

                if my <= 4 {
                    compositor.snap_window(idx, gui::SnapPosition::Top, width, height);
                } else if mx <= 4 {
                    compositor.snap_window(idx, gui::SnapPosition::Left, width, height);
                } else if mx >= width.saturating_sub(5) {
                    compositor.snap_window(idx, gui::SnapPosition::Right, width, height);
                }
            }"""

new = """            // Modern Aero Snap: baslik cubugu ekran kenarlarina suruklenip birakildiginda:

            // - Ekranin ustu: Tam ekran (Maximize)
            // - Ekranin kenarları: Yarı ekran
            // - Ekranin köşeleri: Çeyrek ekran

            if let Some(idx) = dragging_win {
                compositor.focus_window(idx);
                let usable_h = height.saturating_sub(gui::taskbar_h());
                let half_w = width / 2;
                let half_h = usable_h / 2;

                if my <= 4 {
                    compositor.snap_window(idx, gui::SnapPosition::Top, width, height);
                } else if my >= usable_h.saturating_sub(5) {
                    if mx <= 4 {
                        compositor.snap_window(idx, gui::SnapPosition::BottomLeft, width, height);
                    } else if mx >= width.saturating_sub(5) {
                        compositor.snap_window(idx, gui::SnapPosition::BottomRight, width, height);
                    } else if mx < half_w {
                        compositor.snap_window(idx, gui::SnapPosition::BottomLeft, width, height);
                    } else {
                        compositor.snap_window(idx, gui::SnapPosition::BottomRight, width, height);
                    }
                } else if mx <= 4 {
                    if my < half_h {
                        compositor.snap_window(idx, gui::SnapPosition::TopLeft, width, height);
                    } else {
                        compositor.snap_window(idx, gui::SnapPosition::BottomLeft, width, height);
                    }
                } else if mx >= width.saturating_sub(5) {
                    if my < half_h {
                        compositor.snap_window(idx, gui::SnapPosition::TopRight, width, height);
                    } else {
                        compositor.snap_window(idx, gui::SnapPosition::BottomRight, width, height);
                    }
                }
            }"""

if old in c:
    c2 = c.replace(old, new)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(c2)
    print('done')
else:
    print('not found')
