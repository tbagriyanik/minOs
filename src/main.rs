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
    let _fs_mgr = fs::FileSystemManager::new();

    // 3. Ağ ve Ses Sürücülerinin Başlatımı
    let _net_dev = drivers::net::NetworkInterface::new();
    let _audio_dev = drivers::audio::AudioDevice::new();

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

    // 6. MinGUI Pencere Yöneticisi ve Tüm Uygulamalar
    let mut compositor = gui::MinGuiCompositor::new();
    let filemanager_app = apps::filemanager::FileManagerApp::new();
    let controlpanel_app = apps::controlpanel::ControlPanelApp::new();
    let mut notepad_app = apps::notepad::NotepadApp::new();
    let browser_app = apps::browser::BrowserApp::new();
    let imageviewer_app = apps::imageviewer::ImageViewerApp::new();
    let musicplayer_app = apps::musicplayer::MusicPlayerApp::new();
    let videoplayer_app = apps::videoplayer::VideoPlayerApp::new();
    let taskmgr_app = apps::taskmgr::TaskManagerApp::new();

    // 8 Uygulama Penceresini Kaydet
    // 0: Dosya Yöneticisi
    compositor.add_window(gui::Window::new("Dosya Yöneticisi - [VFS / Home]", "[Dosya]", 30, 30, 480, 290, true));
    // 1: Kontrol Paneli
    compositor.add_window(gui::Window::new("Kontrol Paneli (Donanım & Sistem)", "[Panel]", 530, 30, 480, 290, false));
    // 2: Not Defteri
    compositor.add_window(gui::Window::new("Not Defteri - [adsiz.txt]", "[Not]", 80, 70, 460, 270, true));
    // 3: Web Tarayıcısı (CatNet)
    compositor.add_window(gui::Window::new("CatNet Web Gezgini - [v1.0]", "[Web]", 550, 60, 480, 280, false));
    // 4: Resim Gösterici (MinViewer)
    compositor.add_window(gui::Window::new("Resim Gösterici - [kedi_maskotu.bmp]", "[Resim]", 120, 90, 450, 270, false));
    // 5: Müzik Çalar (CatBeats)
    compositor.add_window(gui::Window::new("CatBeats Müzik Çalar - [Ring 0]", "[Müzik]", 560, 80, 460, 245, true));
    // 6: Video Oynatıcı (MinPlayer)
    compositor.add_window(gui::Window::new("MinPlayer Video Oynatıcı - [retro_drive.mp4]", "[Video]", 150, 110, 460, 275, false));
    // 7: Görev Yöneticisi
    compositor.add_window(gui::Window::new("Görev Yöneticisi - [MinOS]", "[Görev]", 600, 100, 400, 220, false));

    // Pencere 2'yi (Not Defteri) başlangıçta odakla
    if let Some(ref mut win) = compositor.windows[2] {
        win.is_focused = true;
    }

    let mut last_mouse_x = mouse.x;
    let mut last_mouse_y = mouse.y;
    let mut last_buttons = (mouse.left_button, mouse.right_button);
    let mut dragging_win: Option<usize> = None;
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;
    let mut anim_tick: usize = 0;
    let mut spin_counter: usize = 0;

    let render_full_frame = |fb: &mut drivers::video::Framebuffer,
                             compositor: &gui::MinGuiCompositor,
                             fm: &apps::filemanager::FileManagerApp,
                             cp: &apps::controlpanel::ControlPanelApp,
                             np: &apps::notepad::NotepadApp,
                             br: &apps::browser::BrowserApp,
                             iv: &apps::imageviewer::ImageViewerApp,
                             mp: &apps::musicplayer::MusicPlayerApp,
                             vp: &apps::videoplayer::VideoPlayerApp,
                             tm: &apps::taskmgr::TaskManagerApp,
                             mouse: &drivers::mouse::MouseState,
                             kbd: &drivers::keyboard::KeyboardState,
                             tick: usize| {
        // 1. Masaüstü Arka Planı
        compositor.render_desktop_bg(fb);

        // 2. Açık ve Küçültülmemiş Pencereleri Çiz
        for (idx, opt_win) in compositor.windows.iter().enumerate() {
            if let Some(win) = opt_win {
                if !win.is_open || win.is_minimized { continue; }

                // Pencere Çerçevesi
                compositor.render_window_frame(fb, win);

                // Uygulama Özel İçerik Render
                match idx {
                    0 => fm.render_content(fb, win.x, win.y, win.width, win.height),
                    1 => cp.render_content(fb, win.x, win.y, win.width, win.height, kbd.layout),
                    2 => np.render_content(fb, win.x, win.y, win.width, win.height, tick),
                    3 => br.render_content(fb, win.x, win.y, win.width, win.height),
                    4 => iv.render_content(fb, win.x, win.y, win.width, win.height),
                    5 => mp.render_content(fb, win.x, win.y, win.width, win.height, tick),
                    6 => vp.render_content(fb, win.x, win.y, win.width, win.height, tick),
                    7 => tm.render_content(fb, win.x, win.y),
                    _ => {}
                }
            }
        }

        // 3. Görev Çubuğu, Başlat Menüsü ve Donanım İmleci
        compositor.render_taskbar_and_overlays(fb, mouse, kbd);
    };

    // İlk kareyi çiz
    render_full_frame(
        &mut fb, &compositor, &filemanager_app, &controlpanel_app,
        &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
        &videoplayer_app, &taskmgr_app, &mouse, &keyboard, anim_tick,
    );

    // 7. Ana Masaüstü Etkileşim ve Olay Döngüsü
    loop {
        let input_occurred = drivers::ps2::poll_input(&mut mouse, &mut keyboard, width as i32, height as i32);
        let mx = mouse.x as usize;
        let my = mouse.y as usize;
        let mouse_clicked = mouse.left_button && !last_buttons.0;
        let mouse_released = !mouse.left_button && last_buttons.0;
        let tb_y = height.saturating_sub(36);

        // 1. Fare Tıklama Olayları (Mouse Down)
        if mouse_clicked {
            // A) Başlat Butonuna Tıklama (x: 6..96, y: tb_y + 4..tb_y + 32)
            if mx >= 6 && mx <= 96 && my >= tb_y + 4 && my <= tb_y + 32 {
                compositor.is_start_menu_open = !compositor.is_start_menu_open;
            }
            // B) Başlat Menüsü Açıkken Menü Öğelerine veya Dışına Tıklama
            else if compositor.is_start_menu_open {
                let menu_w = 230;
                let menu_h = 284;
                let menu_x = 6;
                let menu_y = tb_y.saturating_sub(menu_h + 4);

                if mx >= menu_x && mx <= menu_x + menu_w && my >= menu_y && my <= menu_y + menu_h {
                    for i in 0..8 {
                        let item_y = menu_y + 42 + i * 27;
                        if my >= item_y && my <= item_y + 24 {
                            // Uygulamayı aç ve odakla
                            for j in 0..8 {
                                if let Some(ref mut w) = compositor.windows[j] {
                                    w.is_focused = j == i;
                                }
                            }
                            if let Some(ref mut target_win) = compositor.windows[i] {
                                target_win.is_open = true;
                                target_win.is_minimized = false;
                                target_win.is_focused = true;
                            }
                            compositor.is_start_menu_open = false;
                            break;
                        }
                    }
                } else {
                    compositor.is_start_menu_open = false;
                }
            }
            // C) Görev Çubuğundaki Uygulama Sekmelerine Tıklama
            else if my >= tb_y + 4 && my <= tb_y + 32 && mx >= 104 && mx < 104 + 8 * 80 {
                let mut tab_start_x = 104;
                for i in 0..8 {
                    if let Some(ref mut win) = compositor.windows[i] {
                        if win.is_open {
                            if mx >= tab_start_x && mx <= tab_start_x + 76 {
                                if win.is_minimized {
                                    win.is_minimized = false;
                                    win.is_focused = true;
                                } else if win.is_focused {
                                    win.is_minimized = true;
                                } else {
                                    win.is_focused = true;
                                }
                            }
                            tab_start_x += 80;
                        }
                    }
                }
            }
            // D) Dil Rozetine Tıklama [TR] / [EN]
            else if my >= tb_y + 4 && mx >= width.saturating_sub(130) && mx <= width.saturating_sub(85) {
                keyboard.toggle_layout();
            }
            // E) Pencerelerin Başlıklarına / Butonlarına Tıklama (Önden arkaya arama)
            else {
                let mut hit_window = false;
                for idx in (0..8).rev() {
                    if let Some(ref mut win) = compositor.windows[idx] {
                        if !win.is_open || win.is_minimized { continue; }

                        // Kapat [X] Butonu
                        if mx >= win.x + win.width - 20 && mx <= win.x + win.width - 6 && my >= win.y + 5 && my <= win.y + 19 {
                            win.is_open = false;
                            hit_window = true;
                            break;
                        }

                        // Simge Durumu [-] Butonu
                        if mx >= win.x + win.width - 38 && mx <= win.x + win.width - 24 && my >= win.y + 5 && my <= win.y + 19 {
                            win.is_minimized = true;
                            hit_window = true;
                            break;
                        }

                        // Başlık Çubuğunu Tutma (Sürükleme)
                        if mx >= win.x && mx <= win.x + win.width - 40 && my >= win.y && my <= win.y + 24 {
                            dragging_win = Some(idx);
                            drag_offset_x = mx.saturating_sub(win.x);
                            drag_offset_y = my.saturating_sub(win.y);
                            win.is_focused = true;
                            hit_window = true;
                            break;
                        }

                        // Pencere Gövdesine Tıklayarak Odaklama
                        if mx >= win.x && mx <= win.x + win.width && my >= win.y && my <= win.y + win.height {
                            win.is_focused = true;
                            hit_window = true;
                            break;
                        }
                    }
                }

                // Odaklanan pencere dışındakilerin odağını kaldır
                if hit_window {
                    if let Some(drag_idx) = dragging_win {
                        for j in 0..8 {
                            if j != drag_idx {
                                if let Some(ref mut w) = compositor.windows[j] {
                                    w.is_focused = false;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Pencere Sürükleme (Drag & Drop)
        if mouse.left_button {
            if let Some(idx) = dragging_win {
                if let Some(ref mut win) = compositor.windows[idx] {
                    win.x = mx.saturating_sub(drag_offset_x).min(width.saturating_sub(win.width));
                    win.y = my.saturating_sub(drag_offset_y).min(height.saturating_sub(win.height));
                }
            }
        }

        if mouse_released {
            dragging_win = None;
        }

        // 3. Klavye Girişlerinin İlgili Uygulamaya Gönderimi
        while let Some(ch) = keyboard.take_last_char() {
            // Başlat Menüsü açıkken 1..8 tuşlarıyla hızlı açılış
            if compositor.is_start_menu_open && ch >= '1' && ch <= '8' {
                let app_idx = (ch as usize) - ('1' as usize);
                for j in 0..8 {
                    if let Some(ref mut w) = compositor.windows[j] {
                        w.is_focused = j == app_idx;
                    }
                }
                if let Some(ref mut target_win) = compositor.windows[app_idx] {
                    target_win.is_open = true;
                    target_win.is_minimized = false;
                    target_win.is_focused = true;
                }
                compositor.is_start_menu_open = false;
            } else {
                // Not Defteri odaklı ise doğrudan metin yaz
                let notepad_focused = compositor.windows[2].as_ref().map_or(false, |w| w.is_open && !w.is_minimized && w.is_focused);
                if notepad_focused {
                    if ch == '\x08' {
                        notepad_app.backspace();
                    } else if ch == '\n' || ch >= ' ' {
                        notepad_app.insert_char(ch);
                    }
                }
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

        // Kullanıcı girdisi olduğunda veya animasyon zamanında ekranı güncelle
        if input_occurred || mouse_moved || buttons_changed || timer_tick_fired {
            last_mouse_x = mouse.x;
            last_mouse_y = mouse.y;
            last_buttons = (mouse.left_button, mouse.right_button);

            render_full_frame(
                &mut fb, &compositor, &filemanager_app, &controlpanel_app,
                &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,
                &videoplayer_app, &taskmgr_app, &mouse, &keyboard, anim_tick,
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
