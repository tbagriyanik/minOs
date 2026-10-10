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
    let mut filemanager_app = apps::filemanager::FileManagerApp::new();
    let controlpanel_app = apps::controlpanel::ControlPanelApp::new();
    let mut settings_app = apps::settings::SystemSettings::default();
    let mut notepad_app = apps::notepad::NotepadApp::new();
    let mut browser_app = apps::browser::BrowserApp::new();
    let mut imageviewer_app = apps::imageviewer::ImageViewerApp::new();
    let mut musicplayer_app = apps::musicplayer::MusicPlayerApp::new();
    let mut videoplayer_app = apps::videoplayer::VideoPlayerApp::new();
    let taskmgr_app = apps::taskmgr::TaskManagerApp::new();

    // 8 Uygulama Penceresini Kaydet
    // 0: Dosya Yöneticisi
    compositor.add_window(gui::Window::new("Dosya Yöneticisi - [VFS / Home]", "[Dosya]", 30, 30, 480, 290, true));
    // 1: Kontrol Paneli
    compositor.add_window(gui::Window::new("Kontrol Paneli (Donanım & Sistem)", "[Panel]", 530, 30, 480, 290, false));
    // 2: Not Defteri
    compositor.add_window(gui::Window::new("Not Defteri - [adsiz.txt]", "[Not]", 80, 70, 460, 270, true));
    // 3: Web Tarayıcısı (SarmanNet)
    compositor.add_window(gui::Window::new("SarmanNet Web Gezgini - [v1.0]", "[Web]", 550, 60, 480, 280, false));
    // 4: Resim Gösterici (MinViewer)
    compositor.add_window(gui::Window::new("Resim Gösterici - [kedi_maskotu.bmp]", "[Resim]", 120, 90, 450, 270, false));
    // 5: Müzik Çalar (SarmanBeats)
    compositor.add_window(gui::Window::new("SarmanBeats Müzik Çalar - [Ring 0]", "[Müzik]", 560, 80, 460, 245, true));
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
    let mut resizing_win: Option<usize> = None;
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;
    let mut anim_tick: usize = 0;
    let mut spin_counter: usize = 0;

    let render_full_frame = |fb: &mut drivers::video::Framebuffer,
                             compositor: &gui::MinGuiCompositor,
                             fm: &apps::filemanager::FileManagerApp,
                             _cp: &apps::controlpanel::ControlPanelApp,
                             st: &apps::settings::SystemSettings,
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
                    1 => st.render_content(fb, win.x, win.y),
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

        // 4. Backbuffer'ı VRAM'e Tek Seferde Kopyala (Flicker-Free Double Buffering)
        fb.flush();
    };

    // İlk kareyi çiz
    render_full_frame(
        &mut fb, &compositor, &filemanager_app, &controlpanel_app, &settings_app,
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

        let right_clicked = mouse.right_button && !last_buttons.1;

        // 1. Fare Sağ Tık (Right Click) Olayları - Sağ Tık Bağlam Menüsünü Aç
        if right_clicked {
            compositor.is_context_menu_open = true;
            compositor.context_menu_x = mx;
            compositor.context_menu_y = my;

            // Dosya Yöneticisi içinde mi tıklatıldı kontrol et
            let mut found_file = None;
            if let Some(ref win) = compositor.windows[0] {
                if win.is_open && !win.is_minimized && mx >= win.x && mx <= win.x + win.width && my >= win.y && my <= win.y + win.height {
                    let list_x = win.x + 110 + 14;
                    let row_y_base = win.y + 86;
                    if mx >= list_x && mx <= win.x + win.width - 10 {
                        for i in 0..filemanager_app.files.len() {
                            let item_y = row_y_base + i * 24;
                            if my >= item_y.saturating_sub(2) && my <= item_y + 20 {
                                found_file = Some(i);
                                filemanager_app.selected_idx = i;
                                break;
                            }
                        }
                    }
                }
            }
            compositor.context_file_idx = found_file;
        }

        // 2. Fare Sol Tıklama Olayları (Mouse Down)
        if mouse_clicked {
            compositor.is_alt_tab_open = false;

            // Sağ tık menüsü açıkken tıklama yapıldıysa
            if compositor.is_context_menu_open {
                let menu_w = 170;
                let menu_h = 138;
                let menu_x = compositor.context_menu_x.min(width.saturating_sub(menu_w));
                let menu_y = compositor.context_menu_y.min(height.saturating_sub(menu_h));

                if mx >= menu_x && mx <= menu_x + menu_w && my >= menu_y && my <= menu_y + menu_h {
                    let clicked_item = (my.saturating_sub(menu_y + 4)) / 26;
                    
                    if let Some(file_idx) = compositor.context_file_idx {
                        let selected_file = &filemanager_app.files[file_idx];
                        let target_app_idx = match clicked_item {
                            0 => match selected_file.icon_tag { // Otomatik İlişkili Program
                                "[TXT]" | "[LOG]" | "[CFG]" => 2, // Not Defteri
                                "[IMG]" => 4, // Resim Gösterici
                                "[SND]" => 5, // Müzik Çalar
                                "[VID]" => 6, // Video Oynatıcı
                                "[WEB]" => 3, // Web Tarayıcı
                                _ => 2,
                            },
                            1 => 2, // Not Defteri
                            2 => 4, // Resim Gösterici
                            3 => 5, // Müzik Çalar
                            _ => 0,
                        };

                        // Hedef uygulamayı aç ve odakla
                        for j in 0..8 {
                            if let Some(ref mut w) = compositor.windows[j] {
                                w.is_focused = j == target_app_idx;
                            }
                        }
                        if let Some(ref mut target_win) = compositor.windows[target_app_idx] {
                            target_win.is_open = true;
                            target_win.is_minimized = false;
                            target_win.is_focused = true;
                        }
                    } else {
                        // Masaüstü sağ tık menü seçenekleri
                        let target_app_idx = match clicked_item {
                            0 => 0, // Dosya Yöneticisi
                            1 => 2, // Not Defteri
                            2 => 1, // Ayarlar
                            3 => 7, // Görev Yöneticisi
                            _ => 99,
                        };
                        if target_app_idx < 8 {
                            for j in 0..8 {
                                if let Some(ref mut w) = compositor.windows[j] {
                                    w.is_focused = j == target_app_idx;
                                }
                            }
                            if let Some(ref mut target_win) = compositor.windows[target_app_idx] {
                                target_win.is_open = true;
                                target_win.is_minimized = false;
                                target_win.is_focused = true;
                            }
                        }
                    }
                }
                compositor.is_context_menu_open = false;
// Scroll handling for Notepad
let scroll = mouse.take_scroll_delta();
if scroll != 0 {
    if let Some(ref mut win) = compositor.windows[2] {
        if win.is_open && !win.is_minimized {
            let max_offset = if notepad_app.len > 0 { notepad_app.len / 16 } else { 0 };
            let new_offset = (notepad_app.scroll_offset as i32 + scroll as i32).max(0).min(max_offset as i32) as usize;
            notepad_app.scroll_offset = new_offset;
        }
    }
}
            }
            // A) Başlat Butonuna Tıklama (x: 6..96, y: tb_y + 4..tb_y + 32)
            else if mx >= 6 && mx <= 96 && my >= tb_y + 4 && my <= tb_y + 32 {
                compositor.is_start_menu_open = !compositor.is_start_menu_open;
            }
            // B) Başlat Menüsü Açıkken Menü Öğelerine veya Dışına Tıklama
            else if compositor.is_start_menu_open {
                let menu_w = 230;
                let menu_h = 312;
                let menu_x = 6;
                let menu_y = tb_y.saturating_sub(menu_h + 4);

                if mx >= menu_x && mx <= menu_x + menu_w && my >= menu_y && my <= menu_y + menu_h {
                    for i in 0..9 {
                        let item_y = menu_y + 42 + i * 26;
                        if my >= item_y && my <= item_y + 24 {
                            if i == 8 {
                                // 9. Sistemi Kapat (ACPI QEMU / VirtualBox Port 0x604 / 0x404)
                                serial_print("[MinOS] System Shutdown requested via Start Menu!\n");
                                unsafe {
                                    core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16); // QEMU ACPI
                                    core::arch::asm!("out dx, ax", in("dx") 0x404u16, in("ax") 0x3400u16); // VirtualBox
                                    core::arch::asm!("cli; hlt");
                                }
                            }
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
            // E) Masaüstü Simgelerine Tıklama (Sol taraftaki 5 kısayol ikonu)
            else if mx >= 20 && mx <= 200 {
                let desk_icons = [0, 2, 3, 5, 1]; // 0:FileManager, 2:Notepad, 3:Browser, 5:Music, 1:ControlPanel
                for (i, &app_idx) in desk_icons.iter().enumerate() {
                    let dy = 30 + i * 50;
                    if my >= dy && my <= dy + 32 {
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
                        break;
                    }
                }
            }
            // F) Pencerelerin Başlıklarına / Butonlarına / İçeriğine Tıklama
            else {
                let mut hit_window = false;
                for idx in (0..8).rev() {
                    let (win_x, win_y, win_w, win_h, win_open, win_min) = match compositor.windows[idx] {
                        Some(ref w) => (w.x, w.y, w.width, w.height, w.is_open, w.is_minimized),
                        None => continue,
                    };

                    if !win_open || win_min { continue; }

                    // Kapat [X] Butonu
                    if mx >= win_x + win_w - 20 && mx <= win_x + win_w - 6 && my >= win_y + 5 && my <= win_y + 19 {
                        if let Some(ref mut win) = compositor.windows[idx] {
                            win.is_open = false;
                        }
                        hit_window = true;
                        break;
                    }

                    // Simge Durumu [-] Butonu
                    if mx >= win_x + win_w - 38 && mx <= win_x + win_w - 24 && my >= win_y + 5 && my <= win_y + 19 {
                        if let Some(ref mut win) = compositor.windows[idx] {
                            win.is_minimized = true;
                        }
                        hit_window = true;
                        break;
                    }

                    // Sağ Alt Köşe Yeniden Boyutlandırma (Resize Grip) Algılama
                    if mx >= win_x + win_w - 18 && mx <= win_x + win_w && my >= win_y + win_h - 18 && my <= win_y + win_h {
                        resizing_win = Some(idx);
                        if let Some(ref mut win) = compositor.windows[idx] {
                            win.is_focused = true;
                        }
                        hit_window = true;
                        break;
                    }

                    // Başlık Çubuğunu Tutma (Sürükleme)
                    if mx >= win_x && mx <= win_x + win_w - 40 && my >= win_y && my <= win_y + 24 {
                        dragging_win = Some(idx);
                        drag_offset_x = mx.saturating_sub(win_x);
                        drag_offset_y = my.saturating_sub(win_y);
                        if let Some(ref mut win) = compositor.windows[idx] {
                            win.is_focused = true;
                        }
                        hit_window = true;
                        break;
                    }

                    // --- PENCERE İÇİ İNTERAKTİF DÜĞME VE EYLEM HANDLER'LARI ---
                    match idx {
                        0 => {
                            // Dosya Yöneticisi: Çift Tıklama / Aç Butonu / Yenile Butonu
                            if mx >= win_x + win_w - 120 && mx <= win_x + win_w - 70 && my >= win_y + 32 && my <= win_y + 54 {
                                // [Yenile]
                                filemanager_app.selected_idx = 0;
                                hit_window = true;
                            } else if mx >= win_x + win_w - 62 && mx <= win_x + win_w - 12 && my >= win_y + 32 && my <= win_y + 54 {
                                // [Aç]
                                let selected_file = &filemanager_app.files[filemanager_app.selected_idx];
                                let target_app_idx = match selected_file.icon_tag {
                                    "[TXT]" | "[LOG]" | "[CFG]" => 2,
                                    "[IMG]" => 4,
                                    "[SND]" => 5,
                                    "[VID]" => 6,
                                    "[WEB]" => 3,
                                    _ => 2,
                                };
                                for j in 0..8 {
                                    if let Some(ref mut w) = compositor.windows[j] { w.is_focused = j == target_app_idx; }
                                }
                                if let Some(ref mut target_win) = compositor.windows[target_app_idx] {
                                    target_win.is_open = true;
                                    target_win.is_minimized = false;
                                    target_win.is_focused = true;
                                }
                                hit_window = true;
                            } else if mx >= win_x + 124 && mx <= win_x + win_w - 10 && my >= win_y + 84 && my <= win_y + 250 {
                                let row_y_base = win_y + 86;
                                for i in 0..filemanager_app.files.len() {
                                    let item_y = row_y_base + i * 24;
                                    if my >= item_y.saturating_sub(2) && my <= item_y + 20 {
                                        filemanager_app.selected_idx = i;
                                        let selected_file = &filemanager_app.files[i];
                                        let target_app_idx = match selected_file.icon_tag {
                                            "[TXT]" | "[LOG]" | "[CFG]" => 2,
                                            "[IMG]" => 4,
                                            "[SND]" => 5,
                                            "[VID]" => 6,
                                            "[WEB]" => 3,
                                            _ => 2,
                                        };
                                        for j in 0..8 {
                                            if let Some(ref mut w) = compositor.windows[j] { w.is_focused = j == target_app_idx; }
                                        }
                                        if let Some(ref mut target_win) = compositor.windows[target_app_idx] {
                                            target_win.is_open = true;
                                            target_win.is_minimized = false;
                                            target_win.is_focused = true;
                                        }
                                        hit_window = true;
                                        break;
                                    }
                                }
                            }
                        }
                        1 => {
                            // Ayarlar Penceresi: Sekmeler, Kaydet ve Sıfırla Düğmeleri
                            if mx >= win_x + 10 && mx <= win_x + 100 {
                                if my >= win_y + 40 && my <= win_y + 60 { settings_app.active_tab = 0; hit_window = true; }
                                else if my >= win_y + 65 && my <= win_y + 85 { settings_app.active_tab = 1; hit_window = true; }
                                else if my >= win_y + 90 && my <= win_y + 110 { settings_app.active_tab = 2; hit_window = true; }
                                else if my >= win_y + 115 && my <= win_y + 135 { settings_app.active_tab = 3; hit_window = true; }
                            }

                            // Kaydet Butonu (Tüm pencere boyutları, konumları ve açık/kapalı durumları saklanır)
                            if mx >= win_x + 115 && mx <= win_x + 205 && my >= win_y + 160 && my <= win_y + 182 {
                                settings_app.save_all(&compositor);
                                hit_window = true;
                            }
                            // Sıfırla Butonu
                            else if mx >= win_x + 215 && mx <= win_x + 310 && my >= win_y + 160 && my <= win_y + 182 {
                                settings_app.reset_defaults();
                                hit_window = true;
                            }
                        }
                        2 => {
                            // Not Defteri: Temizle ve Kaydet Butonları
                            if mx >= win_x + win_w - 130 && mx <= win_x + win_w - 74 && my >= win_y + 32 && my <= win_y + 52 {
                                notepad_app.clear();
                                hit_window = true;
                            } else if mx >= win_x + win_w - 68 && mx <= win_x + win_w - 12 && my >= win_y + 32 && my <= win_y + 52 {
                                // Notu Kaydet (Simüle)
                                notepad_app.insert_char(' ');
                                hit_window = true;
                            }
                        }
                        3 => {
                            // Web Tarayıcı: Sekmeler ve Gezinti Butonları
                            if my >= win_y + 30 && my <= win_y + 50 {
                                if mx >= win_x + 8 && mx <= win_x + 168 { browser_app.active_tab = 0; hit_window = true; }
                                else if mx >= win_x + 172 && mx <= win_x + 272 { browser_app.active_tab = 1; hit_window = true; }
                            }
                            if my >= win_y + 56 && my <= win_y + 76 {
                                if mx >= win_x + 12 && mx <= win_x + 34 { browser_app.current_url = "https://minos.internal/home"; hit_window = true; }
                                else if mx >= win_x + 64 && mx <= win_x + 86 { hit_window = true; } // Yenile
                                else if mx >= win_x + win_w - 60 && mx <= win_x + win_w - 10 { browser_app.current_url = "https://github.com/tbagriyanik/minos"; hit_window = true; }
                            }
                        }
                        4 => {
                            // Resim Gösterici: Yakınlaştır / Uzaklaştır / Oran Düğmeleri
                            if my >= win_y + 32 && my <= win_y + 52 {
                                if mx >= win_x + win_w - 180 && mx <= win_x + win_w - 130 {
                                    imageviewer_app.zoom_level = (imageviewer_app.zoom_level + 1).min(8);
                                    hit_window = true;
                                } else if mx >= win_x + win_w - 124 && mx <= win_x + win_w - 74 {
                                    imageviewer_app.zoom_level = (imageviewer_app.zoom_level.saturating_sub(1)).max(1);
                                    hit_window = true;
                                }
                            }
                        }
                        5 => {
                            // Müzik Çalar: Oynat / Duraklat ve Parça Değiştirme
                            let ctrl_y = win_y + 180;
                            if my >= ctrl_y && my <= ctrl_y + 24 {
                                if mx >= win_x + 10 && mx <= win_x + 46 {
                                    musicplayer_app.current_track = musicplayer_app.current_track.saturating_sub(1);
                                    hit_window = true;
                                } else if mx >= win_x + 52 && mx <= win_x + 98 {
                                    musicplayer_app.is_playing = !musicplayer_app.is_playing;
                                    hit_window = true;
                                } else if mx >= win_x + 104 && mx <= win_x + 140 {
                                    musicplayer_app.current_track = (musicplayer_app.current_track + 1) % 5;
                                    hit_window = true;
                                }
                            }
                        }
                        6 => {
                            // Video Oynatıcı: Oynat / Duraklat / Durdur
                            let ctrl_y = win_y + 178;
                            if my >= ctrl_y && my <= ctrl_y + 24 {
                                if mx >= win_x + 12 && mx <= win_x + 54 {
                                    videoplayer_app.is_playing = !videoplayer_app.is_playing;
                                    hit_window = true;
                                } else if mx >= win_x + 58 && mx <= win_x + 98 {
                                    videoplayer_app.is_playing = false;
                                    videoplayer_app.frame = 0;
                                    hit_window = true;
                                }
                            }
                        }
                        _ => {}
                    }

                    // Pencere Gövdesine Tıklayarak Odaklama
                    if mx >= win_x && mx <= win_x + win_w && my >= win_y && my <= win_y + win_h {
                        if let Some(ref mut win) = compositor.windows[idx] {
                            win.is_focused = true;
                            // Move focused window to the front (draw last)
                            let focused_win = compositor.windows[idx].take();
                            // Shift others forward
                            for shift in idx..7 {
                                compositor.windows[shift] = compositor.windows[shift + 1].take();
                            }
                            compositor.windows[7] = focused_win;
                        }
                        hit_window = true;
                        break;
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

        // 2. Özel Kısayol Tuşu Eylemleri (Super / Windows, Alt+F4, Alt+Tab, Ctrl+Alt+Del)
        while let Some(action) = keyboard.take_last_action() {
            match action {
                drivers::keyboard::KeyAction::ToggleStartMenu => {
                    compositor.is_start_menu_open = !compositor.is_start_menu_open;
                }
                drivers::keyboard::KeyAction::CloseActiveWindow => {
                    for win in compositor.windows.iter_mut().flatten() {
                        if win.is_open && win.is_focused {
                            win.is_open = false;
                            break;
                        }
                    }
                }
                drivers::keyboard::KeyAction::CycleWindowFocus => {
                    compositor.is_alt_tab_open = true;

                    let mut current_focused = None;
                    for i in 0..8 {
                        if let Some(ref win) = compositor.windows[i] {
                            if win.is_open && !win.is_minimized && win.is_focused {
                                current_focused = Some(i);
                                break;
                            }
                        }
                    }

                    let start_idx = current_focused.map_or(0, |idx| (idx + 1) % 8);
                    let mut next_idx = None;

                    for step in 0..8 {
                        let i = (start_idx + step) % 8;
                        if let Some(ref win) = compositor.windows[i] {
                            if win.is_open && !win.is_minimized {
                                next_idx = Some(i);
                                break;
                            }
                        }
                    }

                    if let Some(target) = next_idx {
                        // Move the selected window to the front (last slot)
                        let selected_win = compositor.windows[target].take();
                        for shift in target..7 {
                            compositor.windows[shift] = compositor.windows[shift + 1].take();
                        }
                        compositor.windows[7] = selected_win;
                        // Update focus flags
                        for j in 0..8 {
                            if let Some(ref mut w) = compositor.windows[j] {
                                w.is_focused = j == 7; // last slot is now focused
                            }
                        }
                    }
                }
                drivers::keyboard::KeyAction::OpenTaskManager => {
                    // Pencere 7: Görev Yöneticisi
                    for j in 0..8 {
                        if let Some(ref mut w) = compositor.windows[j] {
                            w.is_focused = j == 7;
                        }
                    }
                    if let Some(ref mut tm_win) = compositor.windows[7] {
                        tm_win.is_open = true;
                        tm_win.is_minimized = false;
                        tm_win.is_focused = true;
                    }
                    compositor.is_start_menu_open = false;
                }
                drivers::keyboard::KeyAction::ArrowUp => {
                    if let Some(ref win) = compositor.windows[0] {
                        if win.is_open && win.is_focused && !win.is_minimized {
                            if filemanager_app.selected_idx > 0 {
                                filemanager_app.selected_idx -= 1;
                            }
                        }
                    }
                }
                drivers::keyboard::KeyAction::ArrowDown => {
                    if let Some(ref win) = compositor.windows[0] {
                        if win.is_open && win.is_focused && !win.is_minimized {
                            if filemanager_app.selected_idx + 1 < filemanager_app.files.len() {
                                filemanager_app.selected_idx += 1;
                            }
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
                },
                // Clipboard and edit actions (placeholders)
                drivers::keyboard::KeyAction::CopyText => { /* TODO: copy selected text to clipboard */ },
                drivers::keyboard::KeyAction::PasteText => { /* TODO: paste clipboard text */ },
                drivers::keyboard::KeyAction::CutText => { /* TODO: cut selected text */ },
                drivers::keyboard::KeyAction::SelectAllText => { /* TODO: select all text */ },
                drivers::keyboard::KeyAction::Delete => { /* TODO: delete selected text or character */ },
                drivers::keyboard::KeyAction::Refresh => { /* TODO: refresh UI or buffer */ }
            }
        }

        // Fare Scroll (Tekerlek) Girişi ile Listelerde Gezinti
        let scroll_val = mouse.take_scroll_delta();
        if scroll_val != 0 {
            if let Some(ref win) = compositor.windows[0] {
                if win.is_open && win.is_focused && !win.is_minimized {
                    if scroll_val > 0 {
                        if filemanager_app.selected_idx > 0 {
                            filemanager_app.selected_idx -= 1;
                        }
                    } else {
                        if filemanager_app.selected_idx + 1 < filemanager_app.files.len() {
                            filemanager_app.selected_idx += 1;
                        }
                    }
                }
            }
        }

        // 3. Pencere Sürükleme ve Yeniden Boyutlandırma (Drag & Resize)
        if mouse.left_button {
            if let Some(idx) = dragging_win {
                if let Some(ref mut win) = compositor.windows[idx] {
                    win.x = mx.saturating_sub(drag_offset_x).min(width.saturating_sub(win.width));
                    win.y = my.saturating_sub(drag_offset_y).min(height.saturating_sub(win.height));
                }
            } else if let Some(idx) = resizing_win {
                if let Some(ref mut win) = compositor.windows[idx] {
                    let mut new_w = mx.saturating_sub(win.x).max(win.min_width);
                    let mut new_h = my.saturating_sub(win.y).max(win.min_height);
                    // Apply max limits defined in Window
                    new_w = new_w.min(win.max_width.min(width.saturating_sub(win.x)));
                    new_h = new_h.min(win.max_height.min(height.saturating_sub(win.y)));
                    win.width = new_w;
                    win.height = new_h;
                }
            }
        }

        if mouse_released {
            dragging_win = None;
            resizing_win = None;
        }

        // Alt Tuşu Bırakıldığında Alt+Tab Penceresini Otomatik Kapat ve Seçilen Pencereyi Öne Getir
        if !keyboard.alt && compositor.is_alt_tab_open {
            compositor.is_alt_tab_open = false;
            for opt_win in compositor.windows.iter_mut() {
                if let Some(ref mut win) = opt_win {
                    if win.is_open && win.is_focused {
                        win.is_minimized = false;
                    }
                }
            }
        }

        // 4. Klavye Girişlerinin İlgili Uygulamaya Gönderimi
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
                &mut fb, &compositor, &filemanager_app, &controlpanel_app, &settings_app,
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
