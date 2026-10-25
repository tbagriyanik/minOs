use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use alloc::vec::Vec;

const PI: f32 = core::f32::consts::PI;
const TWO_PI: f32 = core::f32::consts::TAU;
const HALF_PI: f32 = core::f32::consts::FRAC_PI_2;

/// no_std ortamı için hızlı ve hassas sinüs fonksiyonu (Taylor serisi).
pub fn sin(mut x: f32) -> f32 {
    while x > PI {
        x -= TWO_PI;
    }
    while x < -PI {
        x += TWO_PI;
    }
    let x2 = x * x;
    let x3 = x * x2;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    x - (x3 / 6.0) + (x5 / 120.0) - (x7 / 5040.0)
}

/// no_std ortamı için kosinüs fonksiyonu.
pub fn cos(x: f32) -> f32 {
    sin(x + HALF_PI)
}

/// no_std ortamı için Newton-Raphson karekök yaklaşımı (SSE `sqrtss` yok).
pub fn sqrt(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    // Başlangıç tahmini: 1.0 veya x'in kendisi (x>1 ise daha hızlı yakınsar).
    let mut g = if x >= 1.0 { x * 0.5 + 0.5 } else { 1.0 };
    for _ in 0..20 {
        g = 0.5 * (g + x / g);
    }
    g
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Shape3D {
    Cube,
    Cylinder,
    Sphere,
    Pyramid,
}
pub type ShapeType = Shape3D;

pub struct Viewer3DApp {
    pub shape: Shape3D,
    pub rot_x: f32,
    pub rot_y: f32,
    pub rot_z: f32,
    pub zoom: f32,
    pub auto_rotate: bool,
    pub wireframe: bool,
    pub drag_active: bool,
    pub last_mouse: (usize, usize),
    pub file_name: [u8; 48],
    pub file_name_len: usize,
    pub status_msg: &'static str,
    // Önbellek mesh geometrisi
    vertices: Vec<[f32; 3]>,
    edges: Vec<(usize, usize)>,
    faces: Vec<Vec<usize>>,
}

impl Viewer3DApp {
    pub fn new() -> Self {
        let mut app = Self {
            shape: Shape3D::Cube,
            rot_x: 0.4,
            rot_y: 0.6,
            rot_z: 0.0,
            zoom: 1.0,
            auto_rotate: true,
            wireframe: false,
            drag_active: false,
            last_mouse: (0, 0),
            file_name: [0; 48],
            file_name_len: 0,
            status_msg: "3D Sahne Hazır",
            vertices: Vec::new(),
            edges: Vec::new(),
            faces: Vec::new(),
        };
        app.set_shape(Shape3D::Cube);
        app
    }

    pub fn set_file_name(&mut self, name: &str) {
        self.file_name_len = 0;
        for b in name.bytes().take(48) {
            self.file_name[self.file_name_len] = b;
            self.file_name_len += 1;
        }
    }

    pub fn file_name_str(&self) -> &str {
        if self.file_name_len > 0 {
            core::str::from_utf8(&self.file_name[..self.file_name_len]).unwrap_or("model.3d")
        } else {
            "model.3d"
        }
    }

    /// .3d dosyasını yükler ve parametrelerini ayrıştırır.
    pub fn load_file(&mut self, path: &str, content: &[u8]) {
        // Dosya ismini ayıkla
        let file_name = if let Some(idx) = path.rfind('/') {
            &path[idx + 1..]
        } else {
            path
        };
        self.set_file_name(file_name);

        if let Ok(text) = core::str::from_utf8(content) {
            let lower = text.to_ascii_lowercase();
            if lower.contains("cylinder") || lower.contains("silindir") {
                self.set_shape(Shape3D::Cylinder);
            } else if lower.contains("sphere") || lower.contains("kure") || lower.contains("küre")
            {
                self.set_shape(Shape3D::Sphere);
            } else if lower.contains("pyramid") || lower.contains("piramit") {
                self.set_shape(Shape3D::Pyramid);
            } else if lower.contains("cube") || lower.contains("kup") || lower.contains("küp") {
                self.set_shape(Shape3D::Cube);
            }

            if lower.contains("auto_rotate=false") || lower.contains("auto_rotate=0") {
                self.auto_rotate = false;
            } else if lower.contains("auto_rotate=true") || lower.contains("auto_rotate=1") {
                self.auto_rotate = true;
            }

            if lower.contains("wireframe=true") || lower.contains("wireframe=1") {
                self.wireframe = true;
            } else if lower.contains("wireframe=false") || lower.contains("wireframe=0") {
                self.wireframe = false;
            }
        }
        self.status_msg = crate::gui::ui_text("3D Model yüklendi", "3D Model loaded");
    }

    /// Şekil modelini değiştirir ve mesh geometrisini yeniden üretir.
    pub fn set_shape(&mut self, shape: Shape3D) {
        self.shape = shape;
        self.vertices.clear();
        self.edges.clear();
        self.faces.clear();

        match shape {
            Shape3D::Cube => {
                // 8 Köşe
                let s = 1.0f32;
                self.vertices = alloc::vec![
                    [-s, -s, -s],
                    [s, -s, -s],
                    [s, s, -s],
                    [-s, s, -s],
                    [-s, -s, s],
                    [s, -s, s],
                    [s, s, s],
                    [-s, s, s],
                ];
                // 12 Kenar
                self.edges = alloc::vec![
                    (0, 1),
                    (1, 2),
                    (2, 3),
                    (3, 0),
                    (4, 5),
                    (5, 6),
                    (6, 7),
                    (7, 4),
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7),
                ];
                // 6 Yüzey (Quad) — dışa bakan normaller için saat yönünün tersi (CCW) sarım
                self.faces = alloc::vec![
                    alloc::vec![0, 3, 2, 1], // Arka (-Z)
                    alloc::vec![4, 5, 6, 7], // Ön (+Z)
                    alloc::vec![0, 4, 7, 3], // Sol (-X)
                    alloc::vec![1, 2, 6, 5], // Sağ (+X)
                    alloc::vec![3, 7, 6, 2], // Üst (+Y)
                    alloc::vec![0, 1, 5, 4], // Alt (-Y)
                ];
            }
            Shape3D::Cylinder => {
                const SEGMENTS: usize = 16;
                let r = 1.0f32;
                let h = 1.2f32;

                // Üst ve alt çember köşeleri
                for i in 0..SEGMENTS {
                    let angle = (i as f32) * TWO_PI / (SEGMENTS as f32);
                    let x = cos(angle) * r;
                    let z = sin(angle) * r;
                    self.vertices.push([x, h, z]); // Üst (0..SEGMENTS)
                }
                for i in 0..SEGMENTS {
                    let angle = (i as f32) * TWO_PI / (SEGMENTS as f32);
                    let x = cos(angle) * r;
                    let z = sin(angle) * r;
                    self.vertices.push([x, -h, z]); // Alt (SEGMENTS..2*SEGMENTS)
                }

                // Kenarlar
                for i in 0..SEGMENTS {
                    let next = (i + 1) % SEGMENTS;
                    self.edges.push((i, next)); // Üst çember
                    self.edges.push((SEGMENTS + i, SEGMENTS + next)); // Alt çember
                    self.edges.push((i, SEGMENTS + i)); // Dikey bağlantı
                }

                // Yan Yüzeyler (Quad)
                for i in 0..SEGMENTS {
                    let next = (i + 1) % SEGMENTS;
                    self.faces
                        .push(alloc::vec![i, next, SEGMENTS + next, SEGMENTS + i]);
                }
                // Üst ve alt kapaklar — dışa bakan (+Y / -Y) CCW sarım
                let mut top_cap = Vec::new();
                let mut bottom_cap = Vec::new();
                for i in 0..SEGMENTS {
                    top_cap.push(SEGMENTS - 1 - i);
                    bottom_cap.push(SEGMENTS + i);
                }
                self.faces.push(top_cap);
                self.faces.push(bottom_cap);
            }
            Shape3D::Sphere => {
                const RINGS: usize = 8;
                const SEGS: usize = 14;
                let r = 1.1f32;

                for ring in 0..=RINGS {
                    let phi = -HALF_PI + PI * (ring as f32) / (RINGS as f32);
                    let cp = cos(phi);
                    let sp = sin(phi);
                    for seg in 0..SEGS {
                        let theta = TWO_PI * (seg as f32) / (SEGS as f32);
                        let x = cp * cos(theta) * r;
                        let y = sp * r;
                        let z = cp * sin(theta) * r;
                        self.vertices.push([x, y, z]);
                    }
                }

                // Kenarlar ve Yüzeyler
                for ring in 0..RINGS {
                    let cur_row = ring * SEGS;
                    let next_row = (ring + 1) * SEGS;
                    for seg in 0..SEGS {
                        let next_seg = (seg + 1) % SEGS;
                        let i0 = cur_row + seg;
                        let i1 = cur_row + next_seg;
                        let i2 = next_row + next_seg;
                        let i3 = next_row + seg;

                        self.edges.push((i0, i1));
                        self.edges.push((i0, i3));

                        // Dışa bakan CCW sarım: i0 -> i3 -> i2 -> i1
                        self.faces.push(alloc::vec![i0, i3, i2, i1]);
                    }
                }
            }
            Shape3D::Pyramid => {
                let s = 1.1f32;
                let h = 1.3f32;
                self.vertices = alloc::vec![
                    [-s, -h * 0.7, -s],
                    [s, -h * 0.7, -s],
                    [s, -h * 0.7, s],
                    [-s, -h * 0.7, s],
                    [0.0, h * 0.9, 0.0], // Tepe
                ];

                self.edges = alloc::vec![
                    (0, 1),
                    (1, 2),
                    (2, 3),
                    (3, 0), // Taban
                    (0, 4),
                    (1, 4),
                    (2, 4),
                    (3, 4), // Kenarlar
                ];

                // Dışa bakan CCW sarımlar: taban -Y, yanlar tepeye doğru
                self.faces = alloc::vec![
                    alloc::vec![0, 1, 2, 3], // Taban (-Y)
                    alloc::vec![1, 0, 4],    // Ön (-Z)
                    alloc::vec![2, 1, 4],    // Sağ (+X)
                    alloc::vec![3, 2, 4],    // Arka (+Z)
                    alloc::vec![0, 3, 4],    // Sol (-X)
                ];
            }
        }
    }

    /// Zamanlayıcı veya frame güncellemesinde otomatik döndürme uygular.
    pub fn update(&mut self) {
        if self.auto_rotate && !self.drag_active {
            self.rot_y += 0.022;
            if self.rot_y > TWO_PI {
                self.rot_y -= TWO_PI;
            }
            self.rot_x += 0.009;
            if self.rot_x > TWO_PI {
                self.rot_x -= TWO_PI;
            }
        }
    }

    pub fn reset_view(&mut self) {
        self.rot_x = 0.4;
        self.rot_y = 0.6;
        self.rot_z = 0.0;
        self.zoom = 1.0;
        self.status_msg = crate::gui::ui_text("Görünüm sıfırlandı", "View reset");
    }

    pub fn toggle_auto_rotate(&mut self) {
        self.auto_rotate = !self.auto_rotate;
        self.status_msg = if self.auto_rotate {
            crate::gui::ui_text("Otomatik Döndürme: AÇIK", "Auto Rotate: ON")
        } else {
            crate::gui::ui_text("Otomatik Döndürme: KAPALI", "Auto Rotate: OFF")
        };
    }

    pub fn toggle_wireframe(&mut self) {
        self.wireframe = !self.wireframe;
        self.status_msg = if self.wireframe {
            crate::gui::ui_text("Mod: Tel Kafes", "Mode: Wireframe")
        } else {
            crate::gui::ui_text("Mod: Dolgulu Gölgelendirme", "Mode: Solid Shaded")
        };
    }

    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom + 0.15).min(3.5);
    }

    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom - 0.15).max(0.4);
    }

    pub fn tick(&mut self) {
        self.update();
    }

    pub fn handle_scroll(&mut self, scroll_val: i8) {
        if scroll_val > 0 {
            self.zoom_in();
        } else if scroll_val < 0 {
            self.zoom_out();
        }
    }

    /// Fare tıklaması işleme
    pub fn handle_mouse_down(
        &mut self,
        rel_x: usize,
        rel_y: usize,
        win_w: usize,
        _win_h: usize,
    ) -> bool {
        // Araç çubuğu düğmeleri: Y aralığı 30..54
        if rel_y >= 30 && rel_y <= 54 {
            // Şekil butonları
            if rel_x >= 10 && rel_x <= 60 {
                self.set_shape(Shape3D::Cube);
                return true;
            } else if rel_x >= 64 && rel_x <= 126 {
                self.set_shape(Shape3D::Cylinder);
                return true;
            } else if rel_x >= 130 && rel_x <= 186 {
                self.set_shape(Shape3D::Sphere);
                return true;
            } else if rel_x >= 190 && rel_x <= 254 {
                self.set_shape(Shape3D::Pyramid);
                return true;
            } else if rel_x >= 262 && rel_x <= 376 {
                self.toggle_auto_rotate();
                return true;
            } else if rel_x >= 380 && rel_x <= 456 {
                self.toggle_wireframe();
                return true;
            } else if rel_x >= win_w.saturating_sub(110) && rel_x <= win_w.saturating_sub(82) {
                self.zoom_in();
                return true;
            } else if rel_x >= win_w.saturating_sub(80) && rel_x <= win_w.saturating_sub(52) {
                self.zoom_out();
                return true;
            } else if rel_x >= win_w.saturating_sub(50) && rel_x <= win_w.saturating_sub(8) {
                self.reset_view();
                return true;
            }
        }

        // 3D Render Alanı içinde sol tık -> sürükleme başlat
        if rel_y > 56 {
            self.drag_active = true;
            self.last_mouse = (rel_x, rel_y);
            return true;
        }
        false
    }

    pub fn handle_mouse_move(&mut self, rel_x: usize, rel_y: usize) {
        if self.drag_active {
            let dx = (rel_x as isize) - (self.last_mouse.0 as isize);
            let dy = (rel_y as isize) - (self.last_mouse.1 as isize);

            self.rot_y += (dx as f32) * 0.018;
            self.rot_x += (dy as f32) * 0.018;
            self.last_mouse = (rel_x, rel_y);
        }
    }

    pub fn handle_mouse_up(&mut self) {
        self.drag_active = false;
    }

    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        // 1. Araç Çubuğu
        fb.draw_rect(
            win_x + 6,
            win_y + 30,
            win_w - 12,
            crate::gui::sp(24),
            Color::TITLE_BAR,
        );

        let btn_h = 20usize;
        let btn_y = win_y + 32;

        let draw_btn = |fb: &mut Framebuffer, x: usize, w: usize, text: &str, active: bool| {
            let bg = if active {
                Color::ACCENT_BLUE
            } else {
                Color::WINDOW_BG
            };
            let fg = MinGuiCompositor::text_on(bg);
            fb.draw_rect(x, btn_y, w, btn_h, bg);
            fb.draw_rect_outline(x, btn_y, w, btn_h, Color::BORDER);
            MinGuiCompositor::draw_string(fb, x + 6, btn_y + 3, text, fg);
        };

        // Şekil Butonları
        draw_btn(
            fb,
            win_x + 10,
            50,
            crate::gui::ui_text("Küp", "Cube"),
            self.shape == Shape3D::Cube,
        );
        draw_btn(
            fb,
            win_x + 64,
            62,
            crate::gui::ui_text("Silindir", "Cylinder"),
            self.shape == Shape3D::Cylinder,
        );
        draw_btn(
            fb,
            win_x + 130,
            56,
            crate::gui::ui_text("Küre", "Sphere"),
            self.shape == Shape3D::Sphere,
        );
        draw_btn(
            fb,
            win_x + 190,
            64,
            crate::gui::ui_text("Piramit", "Pyramid"),
            self.shape == Shape3D::Pyramid,
        );

        // Otomatik Döndür Butonu
        let rot_text = if self.auto_rotate {
            crate::gui::ui_text("Döndür: AÇIK", "Rotate: ON")
        } else {
            crate::gui::ui_text("Döndür: KAPALI", "Rotate: OFF")
        };
        draw_btn(fb, win_x + 262, 114, rot_text, self.auto_rotate);

        // Mod Butonu (Tel / Dolu)
        let mode_text = if self.wireframe {
            crate::gui::ui_text("Mod: Tel", "Mode: Wire")
        } else {
            crate::gui::ui_text("Mod: Dolu", "Mode: Solid")
        };
        draw_btn(fb, win_x + 380, 76, mode_text, self.wireframe);

        // Sağdaki Zoom ve Sıfırla Düğmeleri
        draw_btn(fb, win_x + win_w.saturating_sub(110), 28, "+", false);
        draw_btn(fb, win_x + win_w.saturating_sub(80), 28, "-", false);
        draw_btn(
            fb,
            win_x + win_w.saturating_sub(50),
            42,
            crate::gui::ui_text("Sıfırla", "Reset"),
            false,
        );

        // 2. 3D Render Alanı
        let render_x = win_x + 6;
        let render_y = win_y + 56;
        let status_h = 24usize;
        let render_w = win_w.saturating_sub(12);
        let render_h = win_h.saturating_sub(56 + status_h);

        // Koyu fütüristik arka plan
        fb.draw_rect(render_x, render_y, render_w, render_h, Color::DARK_BG);
        fb.draw_rect_outline(render_x, render_y, render_w, render_h, Color::BORDER);

        // Grid (Izgara) Zemin Çizimi — Perspektif derinlik hissi
        let grid_center_x = (render_x + render_w / 2) as isize;
        let grid_center_y = (render_y + render_h * 4 / 5) as isize;
        let grid_color = MinGuiCompositor::shade(Color::BORDER, -30);
        for i in -4..=4 {
            let gx0 = grid_center_x + i * 28;
            let gy0 = grid_center_y;
            let gx1 = grid_center_x + i * 50;
            let gy1 = grid_center_y + 40;
            if gx0 >= render_x as isize && gx1 < (render_x + render_w) as isize {
                fb.draw_line(
                    gx0 as usize,
                    gy0 as usize,
                    gx1 as usize,
                    gy1 as usize,
                    grid_color,
                );
            }
        }

        // 3. 3D Dönüşüm ve Projeksiyon
        let cx = (render_x + render_w / 2) as f32;
        let cy = (render_y + render_h / 2) as f32;
        let scale_factor = (render_w.min(render_h) as f32) * 0.42 * self.zoom;
        let camera_dist = 3.6f32;

        let cos_x = cos(self.rot_x);
        let sin_x = sin(self.rot_x);
        let cos_y = cos(self.rot_y);
        let sin_y = sin(self.rot_y);

        // Dönüştürülmüş tepe noktaları (Screen coords + Depth)
        let mut proj_points: Vec<(isize, isize, f32)> = Vec::with_capacity(self.vertices.len());
        for v in &self.vertices {
            let x0 = v[0];
            let y0 = v[1];
            let z0 = v[2];

            // Y ekseni etrafında döndürme
            let x1 = x0 * cos_y + z0 * sin_y;
            let z1 = -x0 * sin_y + z0 * cos_y;

            // X ekseni etrafında döndürme
            let y2 = y0 * cos_x - z1 * sin_x;
            let z2 = y0 * sin_x + z1 * cos_x;

            let depth = z2 + camera_dist;
            if depth > 0.1 {
                let px = cx + (x1 / depth) * scale_factor;
                let py = cy - (y2 / depth) * scale_factor;
                proj_points.push((px as isize, py as isize, z2));
            } else {
                proj_points.push((cx as isize, cy as isize, -999.0));
            }
        }

        // 4. Çizim: Dolgulu (Solid) veya Tel Kafes (Wireframe)
        if !self.wireframe && !self.faces.is_empty() {
            // Yüzey derinlik sıralaması (Painter's Algorithm)
            let mut face_order: Vec<(usize, f32)> = self
                .faces
                .iter()
                .enumerate()
                .map(|(idx, face)| {
                    let mut avg_z = 0.0f32;
                    for &v_idx in face {
                        if v_idx < proj_points.len() {
                            avg_z += proj_points[v_idx].2;
                        }
                    }
                    (idx, avg_z / (face.len() as f32))
                })
                .collect();

            // Derinliğe göre uzaktan yakına sırala (kamera -Z'de: büyük z = uzak).
            // Uzak yüz önce boyanır, yakın yüz üstüne gelir; tersi iç-dış görüntü verir.
            face_order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));

            // Işık vektörü (Normalize: [0.577, 0.577, 0.577])
            let lx = 0.577f32;
            let ly = 0.577f32;
            let lz = 0.577f32;

            for (face_idx, _) in face_order {
                let face = &self.faces[face_idx];
                if face.len() < 3 {
                    continue;
                }

                // Yüzey normali hesabı (Döndürülmüş 3D noktalar üzerinden)
                let v0 = self.vertices[face[0]];
                let v1 = self.vertices[face[1]];
                let v2 = self.vertices[face[2]];

                let ux = v1[0] - v0[0];
                let uy = v1[1] - v0[1];
                let uz = v1[2] - v0[2];
                let vx = v2[0] - v0[0];
                let vy = v2[1] - v0[1];
                let vz = v2[2] - v0[2];

                let nx = uy * vz - uz * vy;
                let ny = uz * vx - ux * vz;
                let nz = ux * vy - uy * vx;

                // Normali döndür
                let nxr = nx * cos_y + nz * sin_y;
                let nzr = -nx * sin_y + nz * cos_y;
                let nyr = ny * cos_x - nzr * sin_x;
                let nzr2 = ny * sin_x + nzr * cos_x;

                // Arka-yüz eleme: kamera -Z yönünde baktığı için yalnızca
                // normali kameraya dönen (nz < 0) yüzler çizilir.
                if nzr2 >= 0.0 {
                    continue;
                }
                let (nx1, ny2, nz2) = (nxr, nyr, nzr2);

                // Normalize: uzunluk karekökü (Newton-Raphson) ile böl; önceki
                // 1/|n|² yaklaşımı büyük yüzeylerde aydınlatmayı olduğundan karanlık yapıyordu.
                let len_sq = nx1 * nx1 + ny2 * ny2 + nz2 * nz2;
                let inv_len = 1.0 / sqrt(len_sq).max(0.0001);
                let dot = (nx1 * inv_len * lx + ny2 * inv_len * ly + nz2 * inv_len * lz).max(0.0);

                // Temel tema rengi (ACCENT_BLUE / ACCENT_CYAN tonları)
                let base_color = match self.shape {
                    Shape3D::Cube => 0x2A75D3,
                    Shape3D::Cylinder => 0x00A896,
                    Shape3D::Sphere => 0x9B51E0,
                    Shape3D::Pyramid => 0xE2841A,
                };

                let intensity = (0.35 + 0.65 * dot).min(1.0);
                let r = (((base_color >> 16) & 0xFF) as f32 * intensity) as u32;
                let g = (((base_color >> 8) & 0xFF) as f32 * intensity) as u32;
                let b = ((base_color & 0xFF) as f32 * intensity) as u32;
                let shaded_color = (r << 16) | (g << 8) | b;

                // Yüzeyi üçgenlere ayırarak çiz
                for k in 1..face.len() - 1 {
                    let p0 = proj_points[face[0]];
                    let p1 = proj_points[face[k]];
                    let p2 = proj_points[face[k + 1]];
                    Self::draw_filled_triangle(
                        fb,
                        p0.0,
                        p0.1,
                        p1.0,
                        p1.1,
                        p2.0,
                        p2.1,
                        shaded_color,
                        render_x,
                        render_y,
                        render_w,
                        render_h,
                    );
                }

                // Yüzey kenarlarını belirgin çiz
                for k in 0..face.len() {
                    let next = (k + 1) % face.len();
                    let p0 = proj_points[face[k]];
                    let p1 = proj_points[face[next]];
                    Self::draw_clipped_line(
                        fb,
                        p0.0,
                        p0.1,
                        p1.0,
                        p1.1,
                        Color::WHITE,
                        render_x,
                        render_y,
                        render_w,
                        render_h,
                    );
                }
            }
        } else {
            // Tel Kafes Çizimi (Wireframe)
            let line_color = match self.shape {
                Shape3D::Cube => Color::ACCENT_BLUE,
                Shape3D::Cylinder => Color::ACCENT_GREEN,
                Shape3D::Sphere => Color::ACCENT_PINK,
                Shape3D::Pyramid => Color::ACCENT_YELLOW,
            };

            for &(i0, i1) in &self.edges {
                if i0 < proj_points.len() && i1 < proj_points.len() {
                    let p0 = proj_points[i0];
                    let p1 = proj_points[i1];
                    Self::draw_clipped_line(
                        fb, p0.0, p0.1, p1.0, p1.1, line_color, render_x, render_y, render_w,
                        render_h,
                    );
                }
            }
        }

        // Köşe Noktaları (Görsel parlaklık)
        for p in &proj_points {
            if p.0 >= render_x as isize
                && p.0 < (render_x + render_w) as isize
                && p.1 >= render_y as isize
                && p.1 < (render_y + render_h) as isize
            {
                fb.draw_rect(p.0 as usize, p.1 as usize, 2, 2, Color::WHITE);
            }
        }

        // 5. Alt Durum Çubuğu
        let status_y = win_y + win_h - status_h;
        fb.draw_rect(win_x + 6, status_y, win_w - 12, status_h, Color::TASKBAR_BG);

        let mut status = TextBuf::new();
        status.push_str(self.status_msg);
        status.push_str("  |  ");
        status.push_str(crate::gui::ui_text("Dosya: ", "File: "));
        status.push_str(self.file_name_str());
        status.push_str("  |  ");
        let shape_name = match self.shape {
            Shape3D::Cube => crate::gui::ui_text("Küp", "Cube"),
            Shape3D::Cylinder => crate::gui::ui_text("Silindir", "Cylinder"),
            Shape3D::Sphere => crate::gui::ui_text("Küre", "Sphere"),
            Shape3D::Pyramid => crate::gui::ui_text("Piramit", "Pyramid"),
        };
        status.push_str(shape_name);
        status.push_str("  |  ");
        status.push_str(crate::gui::ui_text("Köşe: ", "Verts: "));
        status.push_usize(self.vertices.len());
        status.push_str(crate::gui::ui_text(" Kenar: ", " Edges: "));
        status.push_usize(self.edges.len());

        MinGuiCompositor::draw_string(
            fb,
            win_x + 12,
            status_y + 4,
            status.as_str(),
            Color::TEXT_MUTED,
        );
    }

    /// Kırpılmış çizgi çizimi
    fn draw_clipped_line(
        fb: &mut Framebuffer,
        x0: isize,
        y0: isize,
        x1: isize,
        y1: isize,
        color: u32,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
    ) {
        let min_x = rx as isize;
        let max_x = (rx + rw) as isize;
        let min_y = ry as isize;
        let max_y = (ry + rh) as isize;

        // Basit kırpma kontrolü
        if (x0 < min_x && x1 < min_x)
            || (x0 >= max_x && x1 >= max_x)
            || (y0 < min_y && y1 < min_y)
            || (y0 >= max_y && y1 >= max_y)
        {
            return;
        }

        let clamped_x0 = x0.clamp(min_x, max_x - 1) as usize;
        let clamped_y0 = y0.clamp(min_y, max_y - 1) as usize;
        let clamped_x1 = x1.clamp(min_x, max_x - 1) as usize;
        let clamped_y1 = y1.clamp(min_y, max_y - 1) as usize;

        fb.draw_line(clamped_x0, clamped_y0, clamped_x1, clamped_y1, color);
    }

    /// Basit taranmış dolu üçgen çizimi
    fn draw_filled_triangle(
        fb: &mut Framebuffer,
        mut x0: isize,
        mut y0: isize,
        mut x1: isize,
        mut y1: isize,
        mut x2: isize,
        mut y2: isize,
        color: u32,
        rx: usize,
        ry: usize,
        rw: usize,
        rh: usize,
    ) {
        let min_x = rx as isize;
        let max_x = (rx + rw) as isize;
        let min_y = ry as isize;
        let max_y = (ry + rh) as isize;

        // Y'ye göre sırala: y0 <= y1 <= y2
        if y0 > y1 {
            core::mem::swap(&mut y0, &mut y1);
            core::mem::swap(&mut x0, &mut x1);
        }
        if y0 > y2 {
            core::mem::swap(&mut y0, &mut y2);
            core::mem::swap(&mut x0, &mut x2);
        }
        if y1 > y2 {
            core::mem::swap(&mut y1, &mut y2);
            core::mem::swap(&mut x1, &mut x2);
        }

        if y2 < min_y || y0 >= max_y {
            return;
        }

        let total_h = y2 - y0;
        if total_h <= 0 {
            return;
        }

        for y in y0..=y2 {
            if y < min_y || y >= max_y {
                continue;
            }
            let second_half = y > y1 || y1 == y0;
            let segment_h = if second_half { y2 - y1 } else { y1 - y0 };
            if segment_h <= 0 {
                continue;
            }

            let alpha = (y - y0) as f32 / total_h as f32;
            let beta = if second_half {
                (y - y1) as f32 / (y2 - y1) as f32
            } else {
                (y - y0) as f32 / (y1 - y0) as f32
            };

            let mut ax = x0 + ((x2 - x0) as f32 * alpha) as isize;
            let mut bx = if second_half {
                x1 + ((x2 - x1) as f32 * beta) as isize
            } else {
                x0 + ((x1 - x0) as f32 * beta) as isize
            };

            if ax > bx {
                core::mem::swap(&mut ax, &mut bx);
            }

            let start_x = ax.clamp(min_x, max_x - 1) as usize;
            let end_x = bx.clamp(min_x, max_x - 1) as usize;
            if end_x >= start_x {
                fb.draw_rect(start_x, y as usize, end_x - start_x + 1, 1, color);
            }
        }
    }
}
