#![no_std]
#![no_main]

use core::panic::PanicInfo;

pub mod arch;
pub mod mm;
pub mod fs;
pub mod drivers;
pub mod gui;
pub mod apps;

// Limine Protokolü İstek Yapıları (Sıfır Dış Bağımlılık)
#[repr(C, align(8))]
pub struct LimineFramebufferRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineFramebufferResponse,
}

#[repr(C, align(8))]
pub struct LimineFramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: *const *const LimineFramebuffer,
}

#[derive(Clone, Copy)]
#[repr(C, align(8))]
pub struct LimineFramebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

unsafe impl Sync for LimineFramebufferRequest {}

#[used]
#[link_section = ".requests_start_marker"]
static LIMINE_REQUESTS_START_MARKER: [u64; 4] = [
    0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf,
    0x785c6ed015d3e316, 0x181e920a7852b9d9,
];

#[used]
#[link_section = ".requests_base_revision"]
static LIMINE_BASE_REVISION: [u64; 3] = [
    0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 3,
];

#[used]
#[link_section = ".requests"]
static FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x9d5827dcd881dd75,
        0xa3148604f6fab11b,
    ],
    revision: 0,
    response: core::ptr::null(),
};

#[used]
#[link_section = ".requests_end_marker"]
static LIMINE_REQUESTS_END_MARKER: [u64; 2] = [
    0xadc0e0531bb10d03, 0x9572709f31764c62,
];

#[inline(never)]
fn serial_print(s: &str) {
    for b in s.bytes() {
        unsafe {
            core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b);
        }
    }
}

/// Verilen uygulama penceresini açar, küçültme durumunu kaldırır ve odaklar.
fn open_app(compositor: &mut gui::MinGuiCompositor, idx: usize) {
    if let Some(win) = compositor.windows[idx].as_mut() {
        win.is_open = true;
        win.is_minimized = false;
        win.is_maximized = false;
    }
    compositor.focus_window(idx);
}

/// Bir pencerenin (uygulama indeksine göre) dikey içerik yüksekliğini hesaplar.
/// Kaydırma çubuğunun gerekip gerekmediğini ve tutamaç boyutunu belirlemek için
/// hem çizim hem de tıklama kod yollarında ortak kullanılır.
fn window_content_h(
    idx: usize,
    win_h: usize,
    win_w: usize,
    fm: &apps::filemanager::FileManagerApp,
    np: &apps::notepad::NotepadApp,
) -> usize {
    match idx {
        0 => apps::filemanager::LIST_TOP + fm.list_content_h() + 30,
        2 => {
            let text_w = win_w.saturating_sub(12 + 32 + 16);
            let wrap_cols = (text_w / 7).max(1);
            let (lines, _) = np.text_extent(wrap_cols);
            lines * 16 + 58 + 22
        }
        _ => win_h,
    }
}

/// Takvimde ay ileri/geri gezinme.
fn step_month(year: u16, month: u8, forward: bool) -> (u16, u8) {
    if forward {
        if month >= 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        }
    } else if month <= 1 {
        (year.saturating_sub(1), 12)
    } else {
        (year, month - 1)
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    serial_print("[MinOS] Kernel _start reached!\n");
    arch::x86_64::enable_sse();
    arch::x86_64::disable_pic();
    serial_print("[MinOS] SSE enabled & PIC IRQs masked!\n");

    // 1. Donanım Kesmeleri (IDT) ve PMM Başlatımı
    arch::x86_64::init_interrupts();
    let mut memory_mgr = mm::MemoryManager::new();
    let sample_memory_map = [
        mm::pmm::MemoryMapEntry {
            base: 0x100000,
            length: 128 * 1024 * 1024,
            entry_type: mm::pmm::MemoryEntryType::Usable,
        }
    ];
    memory_mgr.pmm.init(&sample_memory_map);

    // 2. Sanal Dosya Sistemi (VFS / TarFS) Başlatımı
    let fs_mgr = fs::FileSystemManager::new();

    // 3. Ağ ve Ses Sürücülerinin Başlatımı
    let _net_dev = drivers::net::NetworkInterface::new();
    let mut audio_dev = drivers::audio::AudioDevice::new();

    // 4. Limine Framebuffer Alımı
    let (fb_ptr, width, height, pitch) = unsafe {
        let resp = core::ptr::read_volatile(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response));
        if !resp.is_null() && (*resp).framebuffer_count > 0 {
            let fb_ptr_ptr = (*resp).framebuffers;
            let fb_info = **fb_ptr_ptr;
            (
                fb_info.address as *mut u32,
                fb_info.width as usize,
                fb_info.height as usize,
                fb_info.pitch as usize,
            )
        } else {
            serial_print("[MinOS] ERROR: Limine Framebuffer response is NULL!\n");
            loop {
                core::hint::spin_loop();
            }
        }
    };

    let mut fb = unsafe {
        drivers::video::Framebuffer::new(fb_ptr, width, height, pitch)
    };

    // 5. PS/2 Donanım Sürücüleri
    let mut mouse = drivers::mouse::MouseState::new();
    let mut keyboard = drivers::keyboard::KeyboardState::new();
    drivers::ps2::init();

    // 5b. Tema motorunu başlat
    gui::theme::init();
    serial_print("[MinOS] Theme engine initialized.\n");

    // 6. MinGUI Pencere Yöneticisi ve Tüm Uygulamalar
    let mut compositor = gui::MinGuiCompositor::new();
    let mut filemanager_app = apps::filemanager::FileManagerApp::new();
    let mut settings_app = apps::settings::SystemSettings::default();
    let mut notepad_app = apps::notepad::NotepadApp::new();
    let browser_app = apps::browser::BrowserApp::new();
    let mut imageviewer_app = apps::imageviewer::ImageViewerApp::new();
    let mut musicplayer_app = apps::musicplayer::MusicPlayerApp::new();
    let mut videoplayer_app = apps::videoplayer::VideoPlayerApp::new();
    let mut taskmgr_app = apps::taskmgr::TaskManagerApp::new();
    let mut calculator_app = apps::calculator::CalculatorApp::new();
    let mut paint_app = apps::paint::PaintApp::new();
    let mut terminal_app = apps::terminal::TerminalApp::new();

    // 8 Uygulama Penceresini Kaydet (indeks = sabit uygulama kimliği)
    // 0: Dosya Yöneticisi
    compositor.add_window(gui::Window::new("Dosya Yöneticisi - [VFS / Home]", "[Dosya]", 30, 30, 480, 290, true));
    // 1: Sistem Ayarları
    compositor.add_window(gui::Window::new("Sistem Ayarları - [Tema, Ses, Saat]", "[Ayarlar]", 530, 30, 480, 290, false));
    // 2: Not Defteri
    compositor.add_window(gui::Window::new("Not Defteri - [adsiz.txt]", "[Not]", 80, 70, 460, 270, true));
    // 3: Web Tarayıcısı (SarmanNet)
    compositor.add_window(gui::Window::new("SarmanNet Web Gezgini - [v1.1]", "[Web]", 550, 60, 480, 280, false));
    // 4: Resim Gösterici (MinViewer)
    compositor.add_window(gui::Window::new("Resim Gösterici - [kedi_maskotu.bmp]", "[Resim]", 120, 90, 450, 270, false));
    // 5: Müzik Çalar (SarmanBeats)
    compositor.add_window(gui::Window::new("SarmanBeats Müzik Çalar - [Ring 0]", "[Müzik]", 560, 80, 460, 245, true));
    // 6: Video Oynatıcı (MinPlayer)
    compositor.add_window(gui::Window::new("MinPlayer Video Oynatıcı - [retro_drive.mp4]", "[Video]", 150, 110, 460, 275, false));
    // 7: Görev Yöneticisi
    compositor.add_window(gui::Window::new("Görev Yöneticisi - [MinOS]", "[Görev]", 600, 100, 400, 220, false));
    compositor.add_window(gui::Window::new("Hesap Makinesi - [MinCalc]", "[Hesap]", 260, 120, 360, 220, false));
    compositor.add_window(gui::Window::new("Çizim Tahtası - [MinPaint]", "[Çizim]", 180, 80, 520, 360, false));
    compositor.add_window(gui::Window::new("Komut İstemi - [MinShell]", "[Komut]", 220, 100, 560, 300, false));

    // Not Defteri'ni başlangıçta odakla
    compositor.focus_window(2);

    // RTC - gerçek zamanlı saat
    let mut now = drivers::rtc::DateTime::read();
    compositor.cal_year = now.year;
    compositor.cal_month = now.month;
    serial_print("[MinOS] RTC clock online.\n");

    // Etkileşim durumu
    let mut last_mouse_x = mouse.x;
    let mut last_mouse_y = mouse.y;
    let mut last_buttons = (mouse.left_button, mouse.right_button);
    let mut dragging_win: Option<usize> = None;
    let mut resizing_win: Option<usize> = None;
    let mut resize_edge: Option<gui::ResizeEdge> = None;
    // Kenar yeniden boyutlandırma başlangıcında pencere geometrisi (sol/üst kenarlar için)
    let mut resize_orig: (usize, usize, usize, usize) = (0, 0, 0, 0);
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;
    // Kaydırma çubuğu sürükleme: (idx, thumb_üst_ofset, thumb_h, track_h, content_h, view_h)
    let mut vscroll_drag: Option<(usize, usize, usize, usize, usize, usize)> = None;
    let mut anim_tick: usize = 0;
    let mut spin_counter: usize = 0;
    let mut loop_counter: u64 = 0;
    let mut clock_spin: usize = 0;
    let mut last_title_idx: Option<usize> = None;
    let mut last_title_click: u64 = 0;
    

    taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs);
    let render_full_frame = |fb: &mut drivers::video::Framebuffer,
                             compositor: &gui::MinGuiCompositor,
                             fm: &apps::filemanager::FileManagerApp,
                             st: &apps::settings::SystemSettings,
                             np: &apps::notepad::NotepadApp,
                             br: &apps::browser::BrowserApp,
                             iv: &apps::imageviewer::ImageViewerApp,
                             mp: &apps::musicplayer::MusicPlayerApp,
                             vp: &apps::videoplayer::VideoPlayerApp,
                             tm: &apps::taskmgr::TaskManagerApp,
                             calc: &apps::calculator::CalculatorApp,
                             paint: &apps::paint::PaintApp,
                             term: &apps::terminal::TerminalApp,
                             now: &drivers::rtc::DateTime,
                             mouse: &drivers::mouse::MouseState,
                             kbd: &drivers::keyboard::KeyboardState,
                             tick: usize| {
        // 1. Masaüstü Arka Planı
        compositor.render_desktop_bg(fb);

        // 2. Açık pencereleri z-sırasına göre (arkadan öne) çiz
        let (order, count) = compositor.draw_order();
        for k in 0..count {
            let idx = order[k];
            if let Some(win) = compositor.windows[idx] {
                if !win.is_open || win.is_minimized {
                    continue;
                }
                compositor.render_window_frame(fb, &win, mouse);

                // İçerik alanını pencere çerçevesine kırp (taşma önleme).
                let (cx, cy, cw, ch) = win.content_rect();
                fb.set_clip(cx, cy, cw, ch);

                // Uygulamaya göre ölçülen içerik boyutu (kaydırma çubuğu gerekli mi?).
                let content_h = window_content_h(idx, win.height, win.width, fm, np);
                let content_w = cw;
                let want_v = content_h > ch;
                let want_h = content_w > cw;
                let v_gutter = if want_v { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };
                let h_gutter = if want_h { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };
                // İçerik alanı (kaydırma çubukları kadar daraltılmış).
                let content_area = (cx, cy, cw.saturating_sub(v_gutter), ch.saturating_sub(h_gutter));
                fb.set_clip(content_area.0, content_area.1, content_area.2.max(1), content_area.3.max(1));

                match idx {
                    0 => fm.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll),
                    1 => st.render_content(fb, win.x, win.y, win.width, win.height, kbd.layout, now),
                    2 => np.render_content(fb, win.x, win.y, win.width, win.height, tick, &win.scroll),
                    3 => br.render_content(fb, win.x, win.y, win.width, win.height),
                    4 => iv.render_content(fb, win.x, win.y, win.width, win.height),
                    5 => mp.render_content(fb, win.x, win.y, win.width, win.height, tick),
                    6 => vp.render_content(fb, win.x, win.y, win.width, win.height, tick),
                    7 => tm.render_content(fb, win.x, win.y),
                    8 => calc.render_content(fb, win.x, win.y, win.width, win.height),
                    9 => paint.render_content(fb, win.x, win.y, win.width, win.height),
                    10 => term.render_content(fb, win.x, win.y, win.width, win.height),
                    _ => {}
                }

                // Kırpmayı kaldır ve kaydırma çubuklarını pencerenin içinde çiz.
                fb.reset_clip();
                if want_v {
                    let track = (cx + cw - crate::gui::scrollbar::SCROLLBAR_THICKNESS, cy,
                                 crate::gui::scrollbar::SCROLLBAR_THICKNESS, content_area.3);
                    let g = crate::gui::scrollbar::v_geom(track, content_h, content_area.3, win.scroll.y);
                    crate::gui::scrollbar::draw_vertical(fb, &g);
                }
                if want_h {
                    let track = (cx, cy + ch - crate::gui::scrollbar::SCROLLBAR_THICKNESS,
                                 content_area.2, crate::gui::scrollbar::SCROLLBAR_THICKNESS);
                    let g = crate::gui::scrollbar::h_geom(track, content_w, content_area.2, win.scroll.x);
                    crate::gui::scrollbar::draw_horizontal(fb, &g);
                }
            }
        }
        fb.reset_clip();

        // 3. Görev Çubuğu, sistem tepsisi, popup'lar ve fare imleci
        compositor.render_taskbar_and_overlays(fb, mouse, kbd, now, st.volume, st.is_muted, st.language.is_english());

        // 4. Backbuffer'ı VRAM'e tek seferde kopyala (flicker-free)
        fb.apply_night_light(st.night_light);
        fb.flush();
    };

    // İlk kareyi çiz
    render_full_frame(
        &mut fb, &compositor, &filemanager_app, &settings_app,
        &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
        &videoplayer_app, &taskmgr_app, &calculator_app, &paint_app, &terminal_app, &now, &mouse, &keyboard, anim_tick,
    );

    // 7. Ana Masaüstü Etkileşim ve Olay Döngüsü
    loop {
        let input_occurred = drivers::ps2::poll_input(&mut mouse, &mut keyboard, width as i32, height as i32);
        loop_counter = loop_counter.wrapping_add(1);
        let mx = mouse.x as usize;
        let my = mouse.y as usize;
        let mouse_clicked = mouse.left_button && !last_buttons.0;
        let mouse_released = !mouse.left_button && last_buttons.0;
        let right_clicked = mouse.right_button && !last_buttons.1;
        let l = gui::MinGuiCompositor::tray_layout(width, height);
        let tb_y = l.tb_y;

        // Saati periyodik olarak yenile
        let mut force_render = false;
        clock_spin += 1;
        if clock_spin >= 3_000 {
            clock_spin = 0;
            now = drivers::rtc::DateTime::read();
            force_render = true;
        }

        // Ses cihazını ayarlarla senkronla
        audio_dev.set_volume(settings_app.volume);
        audio_dev.is_muted = settings_app.is_muted;

        // 1. Fare Sağ Tık — tıklanan yere göre bağlam menüsü
        if right_clicked {
            compositor.is_calendar_open = false;
            compositor.is_volume_popup_open = false;
            compositor.is_start_menu_open = false;
            compositor.is_context_menu_open = true;
            compositor.context_menu_x = mx;
            compositor.context_menu_y = my;
            compositor.context_file_idx = None;
            compositor.context_window = None;

            // Tıklanan pencereyi en üstten aşağıya ara
            let (order, count) = compositor.draw_order();
            let mut kind = gui::ContextKind::Desktop;
            for k in (0..count).rev() {
                let idx = order[k];
                if let Some(win) = compositor.windows[idx] {
                    let inside = mx >= win.x && mx < win.x + win.width
                        && my >= win.y && my < win.y + win.height;
                    if !inside {
                        continue;
                    }
                    compositor.context_window = Some(idx);
                    if my < win.y + gui::TITLEBAR_H {
                        // Başlık çubuğu → pencere menüsü
                        kind = gui::ContextKind::Window;
                    } else if idx == 2 {
                        // Not Defteri metin alanı → düzenleme menüsü
                        kind = gui::ContextKind::Notepad;
                        compositor.focus_window(idx);
                    } else if idx == 0 {
                        // Dosya Yöneticisi → dosya üzerinde mi?
                        let list_x = win.x + 124;
                        if mx >= list_x && mx <= win.x + win.width - 10 {
                            let row_y_base = win.y + 86;
                            for i in 0..filemanager_app.files.len() {
                                let item_y = row_y_base + i * 24;
                                if my >= item_y.saturating_sub(2) && my <= item_y + 20 {
                                    compositor.context_file_idx = Some(i);
                                    filemanager_app.selected_idx = i;
                                    break;
                                }
                            }
                        }
                        kind = if compositor.context_file_idx.is_some() {
                            gui::ContextKind::FileManager
                        } else {
                            gui::ContextKind::Window
                        };
                        compositor.focus_window(idx);
                    } else {
                        // Diğer uygulamalar → pencere menüsü
                        kind = gui::ContextKind::Window;
                        compositor.focus_window(idx);
                    }
                    break;
                }
            }
            compositor.context_kind = kind;
        }

        // 2. Fare Sol Tıklama Olayları (Mouse Down)
        if mouse_clicked {
            compositor.is_alt_tab_open = false;
            let mut consumed = false;
            if let Some(w) = compositor.windows[2] { if w.is_focused && gui::in_rect(mx, my, (w.x+6,w.y+58,w.width-12,180)) { let at=notepad_app.index_at(mx,my,w.x,w.y); notepad_app.selection_start=Some(at); notepad_app.selection_end=Some(at); consumed=true; } }

            // A) Takvim açıkken
            if compositor.is_calendar_open {
                let crect = gui::MinGuiCompositor::calendar_rect(width, &l);
                if gui::in_rect(mx, my, crect) {
                    let (cx, cy, cw, _ch) = crect;
                    if my < cy + 34 {
                        if mx < cx + 34 {
                            let (y, m) = step_month(compositor.cal_year, compositor.cal_month, false);
                            compositor.cal_year = y;
                            compositor.cal_month = m;
                        } else if mx > cx + cw - 34 {
                            let (y, m) = step_month(compositor.cal_year, compositor.cal_month, true);
                            compositor.cal_year = y;
                            compositor.cal_month = m;
                        }
                    }
                    consumed = true;
                } else {
                    compositor.is_calendar_open = false;
                }
            }

            // B) Ses paneli açıkken
            if !consumed && compositor.is_volume_popup_open {
                let panel = compositor.volume_slider_rect(&l);
                if gui::in_rect(mx, my, panel) {
                    if gui::in_rect(mx, my, compositor.volume_mute_rect(&l)) {
                        settings_app.is_muted = !settings_app.is_muted;
                    } else {
                        let track = compositor.volume_track_rect(&l);
                        if gui::in_rect(mx, my, track) { settings_app.set_volume_from_range(mx, track.0, track.2); }
                    }
                    consumed = true;
                } else {
                    compositor.is_volume_popup_open = false;
                }
            }

            // C) Bağlam menüsü açıkken
            if !consumed && compositor.is_context_menu_open {
                if let Some(clicked_item) = compositor.context_hit(width, height, mx, my) {
                    match compositor.context_kind {
                        gui::ContextKind::Desktop => match clicked_item {
                            0 => open_app(&mut compositor, 0),
                            1 => open_app(&mut compositor, 2),
                            2 => open_app(&mut compositor, 1),
                            3 => open_app(&mut compositor, 7),
                            4 => settings_app.cycle_theme(),
                            _ => {}
                        },
                        gui::ContextKind::FileManager => {
                            if let Some(file_idx) = compositor.context_file_idx {
                                let tag = filemanager_app.files[file_idx].icon_tag;
                                let target_app_idx = match clicked_item {
                                    0 => match tag {
                                        "[TXT]" | "[LOG]" | "[CFG]" => 2,
                                        "[IMG]" => 4,
                                        "[SND]" => 5,
                                        "[VID]" => 6,
                                        "[WEB]" => 3,
                                        _ => 2,
                                    },
                                    1 => 2,
                                    2 => 4,
                                    3 => 5,
                                    4 => 6,
                                    _ => 0,
                                };
                                open_app(&mut compositor, target_app_idx);
                            }
                        }
                        gui::ContextKind::Notepad => match clicked_item {
                            0 => notepad_app.cut_selection(),
                            1 => notepad_app.copy_selection(),
                            2 => notepad_app.paste_clipboard(),
                            3 => notepad_app.delete_selection(),
                            4 => notepad_app.select_all(),
                            _ => notepad_app.clear(),
                        },
                        gui::ContextKind::Window => {
                            if let Some(wi) = compositor.context_window {
                                match clicked_item {
                                    0 => {
                                        if let Some(w) = compositor.windows[wi].as_mut() {
                                            w.is_minimized = true;
                                        }
                                        compositor.focus_top_window();
                                    }
                                    1 => {
                                        compositor.focus_window(wi);
                                        compositor.toggle_maximize(wi, width, height);
                                    }
                                    2 => {
                                        if let Some(w) = compositor.windows[wi].as_mut() {
                                            w.x = width.saturating_sub(w.width) / 2;
                                            w.y = height
                                                .saturating_sub(gui::TASKBAR_H)
                                                .saturating_sub(w.height)
                                                / 2;
                                        }
                                    }
                                    _ => {
                                        if let Some(w) = compositor.windows[wi].as_mut() {
                                            w.is_open = false;
                                        }
                                        compositor.focus_top_window();
                                    }
                                }
                            }
                        }
                    }
                }
                compositor.is_context_menu_open = false;
                consumed = true;
            }

            // D) Başlat menüsü açıkken
            if !consumed && compositor.is_start_menu_open {
                let menu_w = 240;
                let menu_h = 430;
                let menu_x = 6;
                let menu_y = tb_y.saturating_sub(menu_h + 4);
                if gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h)) {
                    for i in 0..12 {
                        let item_y = gui::MinGuiCompositor::start_menu_item_y(menu_y, i);
                        if my >= item_y && my <= item_y + 26 {
                            if i == 11 {
                                serial_print("[MinOS] System Shutdown requested via Start Menu!\n");
                                unsafe {
                                    core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
                                    core::arch::asm!("out dx, ax", in("dx") 0x404u16, in("ax") 0x3400u16);
                                    core::arch::asm!("cli; hlt");
                                }
                            }
                            if i < 11 { open_app(&mut compositor, i); }
                            compositor.is_start_menu_open = false;
                            break;
                        }
                    }
                } else {
                    compositor.is_start_menu_open = false;
                }
                consumed = true;
            }

            // E) Sistem tepsisi düğmeleri
            if !consumed {
                if gui::in_rect(mx, my, l.start) {
                    compositor.is_start_menu_open = !compositor.is_start_menu_open;
                    consumed = true;
                } else if gui::in_rect(mx, my, l.clock) {
                    compositor.is_calendar_open = !compositor.is_calendar_open;
                    if compositor.is_calendar_open {
                        compositor.cal_year = now.year;
                        compositor.cal_month = now.month;
                    }
                    consumed = true;
                } else if gui::in_rect(mx, my, l.volume) {
                    compositor.is_volume_popup_open = !compositor.is_volume_popup_open;
                    consumed = true;
                } else if gui::in_rect(mx, my, l.theme) {
                    settings_app.cycle_theme();
                    consumed = true;
                } else if gui::in_rect(mx, my, l.kbd) {
                    keyboard.toggle_layout();
                    settings_app.keyboard_layout = keyboard.layout;
                    consumed = true;
                }
            }

            // F) Görev çubuğu uygulama sekmeleri
            if !consumed && my >= tb_y + 5 && my < tb_y + 31 {
                let mut tab_x = 118;
                for idx in 0..11 {
                    if let Some(win) = compositor.windows[idx] {
                        if win.is_open {
                            if mx >= tab_x && mx < tab_x + 84 {
                                if win.is_minimized {
                                    open_app(&mut compositor, idx);
                                } else if win.is_focused {
                                    if let Some(w) = compositor.windows[idx].as_mut() {
                                        w.is_minimized = true;
                                    }
                                    compositor.focus_top_window();
                                } else {
                                    compositor.focus_window(idx);
                                }
                                consumed = true;
                                break;
                            }
                            tab_x += 88;
                        }
                    }
                }
            }

            // G) Masaüstü simgeleri
            if !consumed && mx >= 20 && mx <= 220 {
                let desk_icons = [0, 2, 3, 5, 1];
                for (i, &app_idx) in desk_icons.iter().enumerate() {
                    let dy = 30 + i * 54;
                    if my >= dy && my <= dy + 30 {
                        open_app(&mut compositor, app_idx);
                        compositor.is_start_menu_open = false;
                        consumed = true;
                        break;
                    }
                }
            }

            // H) Pencereler (başlık, butonlar, içerik)
            if !consumed {
                let (order, count) = compositor.draw_order();
                for k in (0..count).rev() {
                    let idx = order[k];
                    let win = match compositor.windows[idx] {
                        Some(w) => w,
                        None => continue,
                    };
                    let (wx, wy, ww, wh) = (win.x, win.y, win.width, win.height);
                    // Standart pencere davranışı: üstteki pencere gövdesine tıklama odağı alır.
                    if gui::in_rect(mx, my, (wx, wy, ww, wh)) { compositor.focus_window(idx); }

                    // Kapat [X]
                    if gui::in_rect(mx, my, win.close_rect()) {
                        if let Some(w) = compositor.windows[idx].as_mut() {
                            w.is_open = false;
                        }
                        compositor.focus_top_window();
                        break;
                    }
                    // Maksimize / Geri Yükle
                    if gui::in_rect(mx, my, win.max_rect()) {
                        compositor.focus_window(idx);
                        compositor.toggle_maximize(idx, width, height);
                        break;
                    }
                    // Simge durumuna küçült [-]
                    if gui::in_rect(mx, my, win.min_rect()) {
                        if let Some(w) = compositor.windows[idx].as_mut() {
                            w.is_minimized = true;
                        }
                        compositor.focus_top_window();
                        break;
                    }
                    // Tüm kenarlardan/köşelerden yeniden boyutlandırma (modern
                    // kenar tutamaçları) — içerik tıklamalarından ÖNCE kontrol edilir.
                    if !win.is_maximized {
                        if let Some(edge) = win.resize_edge_at(mx, my) {
                            compositor.focus_window(idx);
                            resizing_win = Some(idx);
                            resize_edge = Some(edge);
                            resize_orig = (wx, wy, ww, wh);
                            break;
                        }
                    }
                    // Geriye dönük uyumluluk: sağ alt köşe görsel tutamacı
                    if gui::in_rect(mx, my, win.resize_grip_rect()) {
                        resizing_win = Some(idx);
                        resize_edge = Some(gui::ResizeEdge::BottomRight);
                        resize_orig = (wx, wy, ww, wh);
                        compositor.focus_window(idx);
                        break;
                    }
                    // Dikey kaydırma çubuğu (pencere sağ kenarı) — tıklama/sürükleme.
                    {
                        let (ccx, ccy, ccw, cch) = win.content_rect();
                        let c_h = window_content_h(idx, wh, ww, &filemanager_app, &notepad_app);
                        if c_h > cch {
                            let t = gui::scrollbar::SCROLLBAR_THICKNESS;
                            let track = (ccx + ccw - t, ccy, t, cch);
                            if gui::in_rect(mx, my, track) {
                                let g = gui::scrollbar::v_geom(track, c_h, cch, win.scroll.y);
                                // Tutamaç yakınındaysa tutamacın üstünü koru, değilse anında zıpla.
                                if gui::in_rect(mx, my, g.thumb) {
                                    vscroll_drag = Some((idx, g.thumb.1.saturating_sub(my), g.thumb.3, track.3, c_h, cch));
                                } else {
                                    // Track üzerinde tıklanan konumdan kaydırmayı hesapla.
                                    let ratio = (my.saturating_sub(track.1)) as f32 / track.3.max(1) as f32;
                                    let max_off = c_h.saturating_sub(cch);
                                    if let Some(wm) = compositor.windows[idx].as_mut() {
                                        wm.scroll.y = ((ratio * max_off as f32) as usize).min(max_off);
                                    }
                                    vscroll_drag = Some((idx, g.thumb.3 / 2, g.thumb.3, track.3, c_h, cch));
                                }
                                compositor.focus_window(idx);
                                break;
                            }
                        }
                    }
                    // Başlık çubuğu — sürükle / çift tık ile maksimize
                    if gui::in_rect(mx, my, win.title_drag_rect()) {
                        let dbl = last_title_idx == Some(idx)
                            && loop_counter.wrapping_sub(last_title_click) < 300_000;
                        last_title_idx = Some(idx);
                        last_title_click = loop_counter;
                        compositor.focus_window(idx);
                        if dbl {
                            compositor.toggle_maximize(idx, width, height);
                        } else if !win.is_maximized {
                            dragging_win = Some(idx);
                            drag_offset_x = mx.saturating_sub(wx);
                            drag_offset_y = my.saturating_sub(wy);
                        }
                        break;
                    }

                    // Pencere gövdesi içindeki uygulama düğmeleri
                    if mx >= wx && mx < wx + ww && my >= wy && my < wy + wh {
                        match idx {
                            0 => {
                                // Dosya Yöneticisi: [Yenile], [Aç], liste
                                if mx >= wx + ww - 120 && mx <= wx + ww - 70 && my >= wy + 32 && my <= wy + 54 {
                                    filemanager_app.selected_idx = 0;
                                } else if mx >= wx + ww - 62 && mx <= wx + ww - 12 && my >= wy + 32 && my <= wy + 54 {
                                    let selected_file = &filemanager_app.files[filemanager_app.selected_idx];
                                    let target = match selected_file.icon_tag {
                                        "[TXT]" | "[LOG]" | "[CFG]" => 2,
                                        "[IMG]" => 4,
                                        "[SND]" => 5,
                                        "[VID]" => 6,
                                        "[WEB]" => 3,
                                        _ => 2,
                                    };
                                    open_app(&mut compositor, target);
                                } else if mx >= wx + 124 && mx <= wx + ww - 10 && my >= wy + 84 && my <= wy + 250 {
                                    let row_y_base = wy + 86;
                                    for i in 0..filemanager_app.files.len() {
                                        let item_y = row_y_base + i * 24;
                                        if my >= item_y.saturating_sub(2) && my <= item_y + 20 {
                                            filemanager_app.selected_idx = i;
                                            let selected_file = &filemanager_app.files[i];
                                            let target = match selected_file.icon_tag {
                                                "[TXT]" | "[LOG]" | "[CFG]" => 2,
                                                "[IMG]" => 4,
                                                "[SND]" => 5,
                                                "[VID]" => 6,
                                                "[WEB]" => 3,
                                                _ => 2,
                                            };
                                            open_app(&mut compositor, target);
                                            break;
                                        }
                                    }
                                }
                            }
                            1 => {
                                // Sistem Ayarları
                                for t in 0..4 {
                                    if gui::in_rect(mx, my, apps::settings::SystemSettings::tab_rect(wx, wy, t)) {
                                        settings_app.active_tab = t;
                                    }
                                }
                                if settings_app.active_tab == 0 {
                                    let mut swatch_hit = false;
                                    for i in 0..apps::settings::SystemSettings::theme_swatch_count() {
                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::theme_swatch_rect(wx, wy, i)) {
                                            settings_app.theme = gui::theme::ThemeId::ALL[i];
                                            gui::theme::set_theme(settings_app.theme);
                                            swatch_hit = true;
                                            break;
                                        }
                                    }
                                    if swatch_hit {
                                        // Tema paletinden dogrudan secim yapildi.
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::theme_btn_rect(wx, wy, ww)) {
                                        settings_app.cycle_theme();
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::kbd_btn_rect(wx, wy, ww)) {
                                        keyboard.toggle_layout();
                                        settings_app.language.toggle();
                                        settings_app.keyboard_layout = keyboard.layout;
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::font_btn_rect(wx, wy, ww)) {
                                        settings_app.font.cycle();
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::mute_btn_rect(wx, wy)) {
                                        settings_app.is_muted = !settings_app.is_muted;
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::volume_track_rect(wx, wy)) {
                                        let track=apps::settings::SystemSettings::volume_track_rect(wx,wy); settings_app.set_volume_from_range(mx,track.0,track.2);
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::save_btn_rect(wx, wy, wh)) {
                                        settings_app.save_all(&compositor);
                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::reset_btn_rect(wx, wy, wh)) {
                                        settings_app.restore_windows(&mut compositor);
                                        settings_app.reset_defaults();
                                    }
                                }
                                if settings_app.active_tab == 1 {
                                    for i in 0..4 { if gui::in_rect(mx,my,apps::settings::SystemSettings::resolution_btn_rect(wx,wy,i)) { settings_app.screen_res=match i {0=>(800,600),1=>(1024,768),2=>(1280,720),_=>(1920,1080)}; settings_app.status_msg="Cozunurluk hedefi secildi."; } }
                                    if gui::in_rect(mx,my,apps::settings::SystemSettings::night_light_rect(wx,wy)) { settings_app.night_light=!settings_app.night_light; settings_app.status_msg=if settings_app.night_light { "Gece isigi acildi." } else { "Gece isigi kapatildi." }; }
                                }
                            }
                            2 => {
                                // Not Defteri: Dosya/Düzen araç çubuğu ve hızlı düğmeler
                                if my >= wy + 30 && my <= wy + 56 && mx >= wx + 8 && mx < wx + 60 { notepad_app.menu = 1; }
                                else if my >= wy + 30 && my <= wy + 56 && mx >= wx + 68 && mx < wx + 128 { notepad_app.menu = 2; }
                                else if my >= wy + 30 && my <= wy + 56 && mx >= wx + 128 && mx < wx + 188 { notepad_app.menu = 3; }
                                else if my >= wy + 30 && my <= wy + 56 && mx >= wx + 188 && mx < wx + 250 { notepad_app.menu = 4; }
                                else if notepad_app.menu != 0 && mx >= wx + 8 && mx < wx + 230 && my >= wy + 54 && my < wy + 150 {
                                    let item=(my-wy-54)/20; match (notepad_app.menu,item) { (1,0)=>notepad_app.clear(), (1,2)=>notepad_app.insert_char(' '), (2,0)=>{}, (2,1)=>notepad_app.cut_selection(), (2,2)=>notepad_app.copy_selection(), (2,3)=>notepad_app.paste_clipboard(), (3,0)=>notepad_app.select_all(), (3,1)=>notepad_app.delete_forward(), (3,2)=>notepad_app.clear(), _=>{} } notepad_app.menu=0;
                                }
                                if mx >= wx + ww - 130 && mx <= wx + ww - 74 && my >= wy + 32 && my <= wy + 52 {
                                    notepad_app.clear();
                                } else if mx >= wx + ww - 68 && mx <= wx + ww - 12 && my >= wy + 32 && my <= wy + 52 {
                                    notepad_app.insert_char(' ');
                                }
                            }
                            3 => {
                                // Web Tarayıcı: sekme/gezinti
                                if my >= wy + 30 && my <= wy + 50 {
                                    if mx >= wx + 8 && mx <= wx + 168 {
                                        // aktif sekme (immutable app) - atla
                                    }
                                }
                            }
                            4 => {
                                // Resim: yakınlaştır / uzaklaştır
                                if my >= wy + 32 && my <= wy + 52 {
                                    if mx >= wx + ww - 180 && mx <= wx + ww - 130 {
                                        imageviewer_app.zoom_level = (imageviewer_app.zoom_level + 1).min(8);
                                    } else if mx >= wx + ww - 124 && mx <= wx + ww - 74 {
                                        imageviewer_app.zoom_level = (imageviewer_app.zoom_level.saturating_sub(1)).max(1);
                                    }
                                }
                            }
                            5 => {
                                // Müzik çalar: önceki / oynat / sonraki
                                let ctrl_y = wy + 180;
                                if my >= ctrl_y && my <= ctrl_y + 24 {
                                    if mx >= wx + 10 && mx <= wx + 46 {
                                        musicplayer_app.current_track = musicplayer_app.current_track.saturating_sub(1);
                                    } else if mx >= wx + 52 && mx <= wx + 98 {
                                        musicplayer_app.is_playing = !musicplayer_app.is_playing;
                                    } else if mx >= wx + 104 && mx <= wx + 140 {
                                        musicplayer_app.current_track = (musicplayer_app.current_track + 1) % 5;
                                    }
                                }
                            }
                            6 => {
                                // Video: oynat / durdur
                                let ctrl_y = wy + 178;
                                if my >= ctrl_y && my <= ctrl_y + 24 {
                                    if mx >= wx + 12 && mx <= wx + 54 {
                                        videoplayer_app.is_playing = !videoplayer_app.is_playing;
                                    } else if mx >= wx + 58 && mx <= wx + 98 {
                                        videoplayer_app.is_playing = false;
                                        videoplayer_app.frame = 0;
                                    }
                                }
                            }
                            8 => { if let Some(key) = apps::calculator::CalculatorApp::key_at(mx, my, wx, wy) { calculator_app.press(key); } }
                            9 => {
                                if my >= wy + 34 && my < wy + 56 {
                                    if mx >= wx+58 && mx < wx+80 { paint_app.color=gui::Color::ACCENT_BLUE; paint_app.eraser=false; }
                                    else if mx >= wx+80 && mx < wx+102 { paint_app.color=gui::Color::ACCENT_RED; paint_app.eraser=false; }
                                    else if mx >= wx+102 && mx < wx+124 { paint_app.color=gui::Color::ACCENT_GREEN; paint_app.eraser=false; }
                                    else if mx >= wx+124 && mx < wx+146 { paint_app.color=gui::Color::BLACK; paint_app.eraser=false; }
                                    else if mx >= wx+150 && mx < wx+208 { paint_app.eraser=true; }
                                } else if my >= wy + 58 && mx >= wx + 8 && mx < wx + ww - 8 {
                                    paint_app.paint(mx.saturating_sub(wx+8), my.saturating_sub(wy+58));
                                }
                            }
                            _ => {}
                        }
                        compositor.focus_window(idx);
                        break;
                    }
                }
            }
        }

        // 3. Klavye kısayolları
        while let Some(action) = keyboard.take_last_action() {
            match action {
                drivers::keyboard::KeyAction::ToggleStartMenu => {
                    compositor.is_start_menu_open = !compositor.is_start_menu_open;
                }
                drivers::keyboard::KeyAction::CloseActiveWindow => {
                    if let Some(idx) = compositor.active_window_idx {
                        if let Some(w) = compositor.windows[idx].as_mut() {
                            if w.is_open {
                                w.is_open = false;
                            }
                        }
                        compositor.focus_top_window();
                    }
                }
                drivers::keyboard::KeyAction::CycleWindowFocus => {
                    compositor.is_alt_tab_open = true;
                    compositor.cycle_focus();
                }
                drivers::keyboard::KeyAction::OpenTaskManager => {
                    open_app(&mut compositor, 7);
                    compositor.is_start_menu_open = false;
                }
                drivers::keyboard::KeyAction::ArrowUp => {
                    if let Some(w) = compositor.windows[0] {
                        if w.is_open && w.is_focused && !w.is_minimized && filemanager_app.selected_idx > 0 {
                            filemanager_app.selected_idx -= 1;
                        }
                    }
                }
                drivers::keyboard::KeyAction::ArrowDown => {
                    if let Some(w) = compositor.windows[0] {
                        if w.is_open && w.is_focused && !w.is_minimized
                            && filemanager_app.selected_idx + 1 < filemanager_app.files.len()
                        {
                            filemanager_app.selected_idx += 1;
                        }
                    }
                }
                drivers::keyboard::KeyAction::ArrowLeft | drivers::keyboard::KeyAction::ArrowRight => {}
                drivers::keyboard::KeyAction::ShutdownOS => {
                    serial_print("[MinOS] System Shutdown requested!\n");
                    unsafe {
                        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);
                        core::arch::asm!("cli; hlt");
                    }
                }
                drivers::keyboard::KeyAction::CopyText => { notepad_app.copy_selection(); }
                drivers::keyboard::KeyAction::PasteText => { notepad_app.paste_clipboard(); }
                drivers::keyboard::KeyAction::CutText => { notepad_app.cut_selection(); }
                drivers::keyboard::KeyAction::SelectAllText => { /* TODO */ }
                drivers::keyboard::KeyAction::Delete => { notepad_app.delete_forward(); }
                drivers::keyboard::KeyAction::Refresh => { /* TODO */ }
            }
        }

        // 4. Fare Tekerlek (Scroll)
        let scroll_val = mouse.take_scroll_delta();
        if scroll_val != 0 {
            // Dosya Yöneticisi (pencere 0): listeyi kaydır ve imleci de taşı.
            if let Some(w) = compositor.windows[0] {
                if w.is_open && w.is_focused && !w.is_minimized {
                    if scroll_val > 0 {
                        if filemanager_app.selected_idx > 0 { filemanager_app.selected_idx -= 1; }
                    } else if filemanager_app.selected_idx + 1 < filemanager_app.files.len() {
                        filemanager_app.selected_idx += 1;
                    }
                }
            }
            // Not Defteri (pencere 2): satır bazlı dikey kaydırma.
            if let Some(w) = compositor.windows[2] {
                if w.is_open && w.is_focused && !w.is_minimized {
                    let c_h = window_content_h(2, w.height, w.width, &filemanager_app, &notepad_app);
                    let view_h = w.content_rect().3;
                    let max_y = c_h.saturating_sub(view_h);
                    let delta = (scroll_val as isize) * 16; // 1 satır = 16px
                    let new_off = ((w.scroll.y as isize - delta).max(0) as usize).min(max_y);
                    if let Some(wm) = compositor.windows[2].as_mut() {
                        wm.scroll.y = new_off;
                    }
                }
            }
            // Dosya Yöneticisi listesi de kaydırılabilir (içerik sığmıyorsa).
            if let Some(w) = compositor.windows[0] {
                if w.is_open && w.is_focused && !w.is_minimized {
                    let c_h = window_content_h(0, w.height, w.width, &filemanager_app, &notepad_app);
                    let view_h = w.content_rect().3;
                    let max_y = c_h.saturating_sub(view_h);
                    let delta = (scroll_val as isize) * 24;
                    let new_off = ((w.scroll.y as isize - delta).max(0) as usize).min(max_y);
                    if let Some(wm) = compositor.windows[0].as_mut() {
                        wm.scroll.y = new_off;
                    }
                }
            }
        }

        // 5. Pencere Sürükleme ve Yeniden Boyutlandırma
        if mouse.left_button {
            if let Some(w)=compositor.windows[9] { if w.is_focused && mx>=w.x+8 && mx<w.x+w.width-8 && my>=w.y+58 && my<w.y+w.height-10 { paint_app.paint(mx.saturating_sub(w.x+8),my.saturating_sub(w.y+58)); } }
            if compositor.is_volume_popup_open {
                let track = compositor.volume_track_rect(&l);
                if gui::in_rect(mx, my, (track.0.saturating_sub(8), track.1.saturating_sub(8), track.2+16, track.3+16)) {
                    settings_app.set_volume_from_range(mx, track.0, track.2);
                }
            }
            if let Some(w) = compositor.windows[2] { if w.is_focused && notepad_app.selection_start.is_some() { let at=notepad_app.index_at(mx,my,w.x,w.y); notepad_app.selection_end=Some(at); } }
            if let Some((idx, grab, thumb_h, track_h, content_h, view_h)) = vscroll_drag {
                // Tutamacı fare ile sürükle → kaydırma ofseti.
                let track_top = compositor.windows[idx]
                    .map(|w| w.content_rect().1)
                    .unwrap_or(0);
                let max_thumb_travel = track_h.saturating_sub(thumb_h);
                let desired_top = my.saturating_sub(grab).max(track_top).min(track_top + max_thumb_travel);
                let ratio = if max_thumb_travel == 0 { 0.0 }
                    else { (desired_top - track_top) as f32 / max_thumb_travel as f32 };
                let max_off = content_h.saturating_sub(view_h);
                let new_off = ((ratio * max_off as f32) as usize).min(max_off);
                if let Some(wm) = compositor.windows[idx].as_mut() {
                    wm.scroll.y = new_off;
                }
            } else if let Some(idx) = dragging_win {
                if let Some(w) = compositor.windows[idx].as_mut() {
                    let max_x = width.saturating_sub(w.width);
                    let max_y = height.saturating_sub(w.height + gui::TASKBAR_H);
                    w.x = mx.saturating_sub(drag_offset_x).min(max_x);
                    w.y = my.saturating_sub(drag_offset_y).min(max_y);
                }
            } else if let Some(idx) = resizing_win {
                let edge = resize_edge.unwrap_or(gui::ResizeEdge::BottomRight);
                if let Some(w) = compositor.windows[idx].as_mut() {
                    use gui::ResizeEdge::*;
                    let (ox, oy, ow, oh) = resize_orig;
                    let left = ox + ow;   // sol kenarın sabit sağ referansı
                    let right = ox;       // sağ kenarın sabit sol referansı
                    let top = oy + oh;    // üst kenarın sabit alt referansı
                    let bottom = oy;      // alt kenarın sabit üst referansı

                    let mut nx = w.x;
                    let mut ny = w.y;
                    let mut nw = w.width;
                    let mut nh = w.height;

                    // Sağ kenar sürükleniyor
                    if matches!(edge, Right | TopRight | BottomRight) {
                        nw = mx.saturating_sub(right).max(w.min_width)
                            .min(w.max_width.min(width.saturating_sub(w.x)));
                    }
                    // Sol kenar sürükleniyor
                    if matches!(edge, Left | TopLeft | BottomLeft) {
                        let desired = mx.min(left);
                        nw = left.saturating_sub(desired).max(w.min_width);
                        nx = left.saturating_sub(nw);
                    }
                    // Alt kenar sürükleniyor
                    if matches!(edge, Bottom | BottomLeft | BottomRight) {
                        let max_h = height.saturating_sub(gui::TASKBAR_H).saturating_sub(w.y);
                        nh = my.saturating_sub(bottom).max(w.min_height).min(w.max_height).min(max_h);
                    }
                    // Üst kenar sürükleniyor
                    if matches!(edge, Top | TopLeft | TopRight) {
                        let desired = my.min(top);
                        nh = top.saturating_sub(desired).max(w.min_height);
                        ny = top.saturating_sub(nh);
                    }

                    w.x = nx;
                    w.y = ny;
                    w.width = nw;
                    w.height = nh;
                    w.is_maximized = false;
                }
            }
        }

        if mouse_released {
            paint_app.end_stroke();
            // Modern pencere "snap": başlık çubuğu ekranın en üstüne sürüklenip
            // bırakılırsa pencere maksimize edilir (Windows/KDE davranışı).
            if let Some(idx) = dragging_win {
                if my <= 4 {
                    compositor.focus_window(idx);
                    compositor.toggle_maximize(idx, width, height);
                }
            }
            dragging_win = None;
            resizing_win = None;
            resize_edge = None;
            vscroll_drag = None;
        }

        // Alt tuşu bırakıldığında Alt+Tab penceresini kapat
        if !keyboard.alt && compositor.is_alt_tab_open {
            compositor.is_alt_tab_open = false;
        }

        // 6. Klavye karakter girişleri
        while let Some(ch) = keyboard.take_last_char() {
            if compositor.is_start_menu_open && ch >= '1' && ch <= '9' {
                let app_idx = (ch as usize) - ('1' as usize);
                open_app(&mut compositor, app_idx);
                compositor.is_start_menu_open = false;
            } else if compositor.is_start_menu_open && (ch == 'a' || ch == 'A' || ch == 'b' || ch == 'B') {
                open_app(&mut compositor, if ch == 'a' || ch == 'A' { 9 } else { 10 });
                compositor.is_start_menu_open = false;
            } else {
                let notepad_focused = compositor.windows[2]
                    .as_ref()
                    .map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                if notepad_focused {
                    if ch == '\x08' {
                        notepad_app.backspace();
                    } else if ch == '\n' || ch >= ' ' {
                        notepad_app.insert_char(ch);
                    }
                }
                let calc_focused = compositor.windows[8].as_ref().map_or(false, |w| w.is_focused);
                let term_focused = compositor.windows[10].as_ref().map_or(false, |w| w.is_focused);
                if calc_focused { if ch == '\x08' { calculator_app.backspace(); } else if ch == '\x1b' { calculator_app.clear(); } else if ch >= '0' && ch <= '9' || ch == '+' || ch == '-' { calculator_app.insert(ch); } }
                if term_focused { if ch == '\x08' { terminal_app.backspace(); } else if ch >= ' ' { terminal_app.insert(ch); } }
                let paint_focused = compositor.windows[9].as_ref().map_or(false, |w| w.is_focused);
                if paint_focused && (ch == 'c' || ch == 'C') { paint_app.clear(); }
            }
        }

        let mouse_moved = mouse.x != last_mouse_x || mouse.y != last_mouse_y;
        let buttons_changed = (mouse.left_button, mouse.right_button) != last_buttons;

        spin_counter += 1;
        let timer_tick_fired = spin_counter >= 15_000;
        if timer_tick_fired {
            spin_counter = 0;
            anim_tick = anim_tick.wrapping_add(1);
        }

        if input_occurred || mouse_moved || buttons_changed || timer_tick_fired || force_render {
            last_mouse_x = mouse.x;
            last_mouse_y = mouse.y;
            last_buttons = (mouse.left_button, mouse.right_button);

            taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs);
            render_full_frame(
                &mut fb, &compositor, &filemanager_app, &settings_app,
                &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
                &videoplayer_app, &taskmgr_app, &calculator_app, &paint_app, &terminal_app,
                &now, &mouse, &keyboard, anim_tick,
            );
        }

        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
