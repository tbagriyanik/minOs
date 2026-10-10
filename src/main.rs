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

#[inline(never)]
fn serial_print_hex(mut val: u64) {
    let hex_chars = b"0123456789ABCDEF";
    let mut buf = [b'0'; 16];
    for i in (0..16).rev() {
        buf[i] = hex_chars[(val & 0xF) as usize];
        val >>= 4;
    }
    serial_print("0x");
    for b in buf {
        unsafe {
            core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b);
        }
    }
    serial_print("\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    serial_print("[MinOS] Kernel _start reached!\n");
    arch::x86_64::enable_sse();
    arch::x86_64::disable_pic();
    serial_print("[MinOS] SSE enabled & PIC IRQs masked!\n");
    let base_rev_val = unsafe {
        core::ptr::read_volatile(core::ptr::addr_of!(LIMINE_BASE_REVISION[2]))
    };
    let resp_ptr = unsafe {
        core::ptr::read_volatile(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response))
    };
    serial_print("[MinOS] Base revision[2]: ");
    serial_print_hex(base_rev_val);
    serial_print("[MinOS] FB Request addr: ");
    serial_print_hex(core::ptr::addr_of!(FRAMEBUFFER_REQUEST) as u64);
    serial_print("[MinOS] FB Request response ptr: ");
    serial_print_hex(resp_ptr as u64);

    // 1. Donanım Kesmeleri (IDT) ve PMM Başlatımı
    serial_print("[MinOS] Step 1: IDT & PMM\n");
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
    serial_print("[MinOS] Step 2: VFS\n");
    let _fs_mgr = fs::FileSystemManager::new();

    // 3. Ağ ve Ses Sürücülerinin Başlatımı
    serial_print("[MinOS] Step 3: Drivers\n");
    let _net_dev = drivers::net::NetworkInterface::new();
    let _audio_dev = drivers::audio::AudioDevice::new();

    // 4. Limine Framebuffer Alımı
    serial_print("[MinOS] Step 4: Framebuffer\n");
    let (fb_ptr, width, height, pitch) = unsafe {
        let resp = core::ptr::read_volatile(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response));
        if !resp.is_null() && (*resp).framebuffer_count > 0 {
            serial_print("[MinOS] Limine Framebuffer response OK!\n");
            let fb_ptr_ptr = (*resp).framebuffers;
            let fb_info = **fb_ptr_ptr;
            serial_print("[MinOS] FB Address: ");
            serial_print_hex(fb_info.address as u64);
            serial_print("[MinOS] FB Width: ");
            serial_print_hex(fb_info.width);
            serial_print("[MinOS] FB Height: ");
            serial_print_hex(fb_info.height);
            serial_print("[MinOS] FB Pitch: ");
            serial_print_hex(fb_info.pitch);
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


    serial_print("[MinOS] Step 5: Compositor\n");

    serial_print("[MinOS] Step 5.1: Mouse & Kbd (PS/2 Init)\n");
    let mut mouse = drivers::mouse::MouseState::new();
    let mut keyboard = drivers::keyboard::KeyboardState::new();
    drivers::ps2::init();

    // 5. MinGUI Pencere Yöneticisi ve Uygulamalar
    serial_print("[MinOS] Step 5.2: MinGuiCompositor\n");
    let mut compositor = gui::MinGuiCompositor::new();
    let taskmgr_app = apps::taskmgr::TaskManagerApp::new();
    let mut settings_app = apps::settings::SystemSettings::default();
    settings_app.keyboard_layout = keyboard.layout;

    // Uygulama Pencerelerini Ekle
    serial_print("[MinOS] Step 5.3: Add windows\n");
    compositor.add_window(gui::Window::new("Görev Yöneticisi - [MinOS]", 60, 60, 400, 220));
    compositor.add_window(gui::Window::new("Sistem Ayarları (TR & ENG)", 500, 80, 440, 230));

    serial_print("[MinOS] Step 6: Rendering Initial Frame\n");
    compositor.render_desktop(&mut fb, &mouse, &keyboard);
    taskmgr_app.render_content(&mut fb, 60, 60);
    settings_app.render_content(&mut fb, 500, 80);
    serial_print("[MinOS] MinOS GUI Desktop is now running with full interaction!\n");

    let mut last_mouse_x = mouse.x;
    let mut last_mouse_y = mouse.y;
    let mut last_buttons = (mouse.left_button, mouse.right_button);
    let mut last_layout = keyboard.layout;
    let mut dragging_win: Option<usize> = None;
    let mut drag_offset_x = 0;
    let mut drag_offset_y = 0;

    // 6. Ana Masaüstü Etkileşim ve Olay Döngüsü
    loop {
        let input_occurred = drivers::ps2::poll_input(&mut mouse, &mut keyboard, width as i32, height as i32);

        // Pencere Sürükleme (Drag & Drop)
        if mouse.left_button {
            if let Some(idx) = dragging_win {
                if let Some(ref mut win) = compositor.windows[idx] {
                    win.x = (mouse.x as usize).saturating_sub(drag_offset_x).min(width.saturating_sub(win.width));
                    win.y = (mouse.y as usize).saturating_sub(drag_offset_y).min(height.saturating_sub(win.height));
                }
            } else {
                for (idx, win_opt) in compositor.windows.iter_mut().enumerate() {
                    if let Some(win) = win_opt {
                        let mx = mouse.x as usize;
                        let my = mouse.y as usize;
                        if mx >= win.x && mx <= win.x + win.width && my >= win.y && my <= win.y + 24 {
                            dragging_win = Some(idx);
                            drag_offset_x = mx.saturating_sub(win.x);
                            drag_offset_y = my.saturating_sub(win.y);
                            win.is_focused = true;
                            break;
                        }
                    }
                }
            }
        } else {
            dragging_win = None;
        }

        // Görev Çubuğundaki [TR] / [EN] Rozetine Tıklama
        let tb_y = height.saturating_sub(36);
        if mouse.left_button && (mouse.y as usize) >= tb_y {
            let mx = mouse.x as usize;
            if mx >= width.saturating_sub(130) && mx <= width.saturating_sub(85) {
                if !last_buttons.0 {
                    keyboard.toggle_layout();
                }
            }
        }

        // Ayarlar Uygulamasını Güncel Klavye Düzeniyle Eşitle
        if keyboard.layout != last_layout {
            settings_app.keyboard_layout = keyboard.layout;
            last_layout = keyboard.layout;
        }

        let mouse_moved = mouse.x != last_mouse_x || mouse.y != last_mouse_y;
        let buttons_changed = (mouse.left_button, mouse.right_button) != last_buttons;

        // Herhangi bir fare hareketi, tıklama veya tuş basımında ekranı güncelle
        if input_occurred || mouse_moved || buttons_changed {
            last_mouse_x = mouse.x;
            last_mouse_y = mouse.y;
            last_buttons = (mouse.left_button, mouse.right_button);

            compositor.render_desktop(&mut fb, &mouse, &keyboard);
            let w0_x = compositor.windows[0].as_ref().map_or(60, |w| w.x);
            let w0_y = compositor.windows[0].as_ref().map_or(60, |w| w.y);
            let w1_x = compositor.windows[1].as_ref().map_or(500, |w| w.x);
            let w1_y = compositor.windows[1].as_ref().map_or(80, |w| w.y);
            taskmgr_app.render_content(&mut fb, w0_x, w0_y);
            settings_app.render_content(&mut fb, w1_x, w1_y);
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
