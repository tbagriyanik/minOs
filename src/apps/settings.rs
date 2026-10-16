use crate::drivers::video::Framebuffer;
use crate::drivers::keyboard::KeyboardLayout;
use crate::drivers::rtc::DateTime;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};
use crate::gui::theme::{self, ThemeId};

use crate::gui::WindowState;
use crate::fs::vfs::VirtualFileSystem;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UiLanguage { Turkish, English }
impl UiLanguage { pub fn toggle(&mut self) { *self = match *self { Self::Turkish => Self::English, Self::English => Self::Turkish }; crate::gui::set_ui_language_english(self.is_english()); } pub fn is_english(&self) -> bool { matches!(self, Self::English) } }

struct DeviceLine { buf: [u8; 128], len: usize }
impl DeviceLine {
    fn new() -> Self { Self { buf: [0; 128], len: 0 } }
    fn push(&mut self, s: &str) { for b in s.bytes() { if self.len < self.buf.len() { self.buf[self.len] = b; self.len += 1; } } }
    fn number(&mut self, mut n: u64) {
        let mut tmp = [0u8; 20]; let mut len = 0;
        if n == 0 { self.push("0"); return; }
        while n > 0 { tmp[len] = b'0' + (n % 10) as u8; len += 1; n /= 10; }
        while len > 0 { len -= 1; if self.len < self.buf.len() { self.buf[self.len] = tmp[len]; self.len += 1; } }
    }
    fn hex_byte(&mut self, value: u8) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for digit in [value >> 4, value & 0x0F] {
            if self.len < self.buf.len() { self.buf[self.len] = HEX[digit as usize]; self.len += 1; }
        }
    }
    fn finish(&self) -> &str { core::str::from_utf8(&self.buf[..self.len]).unwrap_or("") }
}

fn draw_device_line(fb: &mut Framebuffer, x: usize, y: usize, width: usize, line: &DeviceLine, color: u32) {
    MinGuiCompositor::draw_string_clipped(fb, x, y, line.finish(), color, (x, y, width, 20));
}

fn render_device_inventory(
    fb: &mut Framebuffer, x: usize, y: usize, width: usize, language: UiLanguage,
    layout: KeyboardLayout, cpu: &str, cores: u32, ram_bytes: u64,
) {
    let english = language.is_english();
    let (fb_width, fb_height, fb_bpp, fb_pitch) = (fb.width, fb.height, fb.bpp, fb.pitch);
    let mut row = 0usize;
    let mut draw = |line: &DeviceLine, color| {
        draw_device_line(fb, x, y + row * sp(24), width, line, color);
        row += 1;
    };

    let mut line = DeviceLine::new();
    line.push(if english { "CPU: " } else { "İşlemci: " }); line.push(cpu); line.push(" | x86_64 | "); line.number(cores as u64);
    line.push(if english { " logical core(s) | SSE2" } else { " mantıksal çekirdek | SSE2" });
    draw(&line, Color::ACCENT_BLUE);

    let mut line = DeviceLine::new();
    line.push(if english { "RAM usable by kernel: " } else { "Çekirdeğin kullanabildiği RAM: " });
    line.number(ram_bytes / (1024 * 1024)); line.push(" MB");
    draw(&line, Color::ACCENT_YELLOW);

    let mut line = DeviceLine::new();
    line.push(if english { "Display: Limine framebuffer | " } else { "Ekran: Limine framebuffer | " });
    line.number(fb_width as u64); line.push("x"); line.number(fb_height as u64); line.push(" | ");
    line.number(fb_bpp as u64); line.push(" bpp | "); line.number(fb_pitch as u64); line.push(" B pitch");
    draw(&line, Color::TEXT_MAIN);

    let mut line = DeviceLine::new();
    line.push(if english { "Keyboard: PS/2 | " } else { "Klavye: PS/2 | " });
    line.push(match layout { KeyboardLayout::TurkishQ => "TR-Q", KeyboardLayout::EnglishUS => "EN-US" });
    line.push(if english { " | polling driver active" } else { " | yoklama sürücüsü etkin" });
    draw(&line, Color::ACCENT_GREEN);

    let mut line = DeviceLine::new();
    line.push(if english { "Mouse: PS/2 | 3 buttons | " } else { "Fare: PS/2 | 3 düğme | " });
    line.push(if crate::drivers::ps2::mouse_wheel_enabled() {
        if english { "wheel detected" } else { "tekerlek algılandı" }
    } else if english { "wheel not detected" } else { "tekerlek algılanmadı" });
    draw(&line, Color::ACCENT_PINK);

    let mut line = DeviceLine::new();
    if crate::drivers::hda::is_ready() {
        if crate::drivers::hda::is_dma_progressing() {
            line.push(if english { "Audio: Intel HDA | DMA active | 48 kHz stereo | PC Speaker standby" } else { "Ses: Intel HDA | DMA akışı aktif | 48 kHz stereo | PC Speaker yedeği hazır" });
            draw(&line, Color::ACCENT_GREEN);
        } else {
            line.push(if english { "Audio: Intel HDA ready | DMA standby / PC Speaker fallback enabled" } else { "Ses: Intel HDA hazır | DMA beklemede / PC Speaker yedeği devrede" });
            draw(&line, Color::ACCENT_BLUE);
        }
    } else {
        line.push(if english { "Audio: PC Speaker PWM fallback active (HDA not detected)" } else { "Ses: PC Speaker PWM yedeği aktif (HDA bulunamadı)" });
        draw(&line, Color::ACCENT_YELLOW);
    }

    if crate::drivers::e1000::is_ready() {
        let mut line = DeviceLine::new();
        line.push("Ethernet: Intel e1000 82540EM | ");
        line.push(if crate::drivers::e1000::link_up() { if english { "link up | MAC " } else { "bağlı | MAC " } }
            else { if english { "link down | MAC " } else { "bağlantı yok | MAC " } });
        let mac = unsafe { crate::drivers::e1000::MAC_ADDR };
        for (i, byte) in mac.iter().enumerate() {
            if i > 0 { line.push(":"); }
            line.hex_byte(*byte);
        }
        draw(&line, Color::ACCENT_BLUE);
    }

    if let Some(controller) = crate::drivers::usb::controller_info() {
        let mut line = DeviceLine::new();
        line.push(if english { "USB host controller: " } else { "USB ana denetleyicisi: " });
        line.push(match controller.controller_type {
            crate::drivers::usb::UsbControllerType::Uhci => "UHCI (USB 1.1)",
            crate::drivers::usb::UsbControllerType::Ohci => "OHCI (USB 1.1)",
            crate::drivers::usb::UsbControllerType::Ehci => "EHCI (USB 2.0)",
            crate::drivers::usb::UsbControllerType::Xhci => "xHCI (USB 3.x)",
            crate::drivers::usb::UsbControllerType::Other(_) => "USB host controller",
            crate::drivers::usb::UsbControllerType::None => "USB",
        });
        line.push(" | PCI "); line.number(controller.bus as u64); line.push(":"); line.number(controller.dev as u64);
        line.push("."); line.number(controller.func as u64); line.push(" | ");
        line.push(if controller.is_mmio { "MMIO base " } else { "I/O base " });
        line.number(controller.base_addr);
        draw(&line, Color::ACCENT_GREEN);
    }

    for drive in 0..4u8 {
        let Some(device) = crate::drivers::ata::device(drive) else { continue; };
        let mut line = DeviceLine::new();
        line.push(if english { "Storage " } else { "Depolama " }); line.number(drive as u64); line.push(": ");
        line.push(device.model_str()); line.push(" | ");
        line.push(match device.kind {
            crate::drivers::ata::DeviceKind::Ata => "ATA",
            crate::drivers::ata::DeviceKind::Atapi => "ATAPI",
            crate::drivers::ata::DeviceKind::None => "",
        });
        line.push(" | ");
        line.number(device.size_mb()); line.push(" MB | "); line.number(device.sectors); line.push(" sectors x ");
        line.number(if device.kind == crate::drivers::ata::DeviceKind::Atapi { 2048 } else { 512 }); line.push(" B");
        draw(&line, Color::TEXT_MAIN);
    }

    drop(draw);
    if row == 5 {
        let mut line = DeviceLine::new();
        line.push(if english { "No additional active devices detected." } else { "Başka etkin aygıt algılanmadı." });
        draw_device_line(fb, x, y + row * sp(24), width, &line, Color::TEXT_MUTED);
    }
}
/// Yardımcı düğme/geometri tanımları (çizim ve tıklama testi ortak kullanır).
const TAB_X: usize = 10;
const TAB_W: usize = 116;
const TAB_H: usize = 26;
const CONTENT_X: usize = 140;

pub struct SystemSettings {
    // Temel Ayarlar
    pub active_tab: usize, // 0: Genel, 1: Aygıt, 2: Gelişmiş (Ağ ve Hakkında bilgileri Aygıt sekmesinde toplanmıştır)
    pub focused_control: usize,
    pub theme: ThemeId,
    pub keyboard_layout: KeyboardLayout,
    pub language: UiLanguage,
    pub region: u8,
    pub volume: u8,
    pub is_muted: bool,
    pub mouse_speed: u8,
    pub double_click_ms: u16,
    pub reverse_scroll: bool,
    pub screen_res: (usize, usize),
    pub display_res: (usize, usize),
    pub zoom: u8,
    pub night_light: bool,
    pub night_light_level: u8,

    // Saklanan Pencere Konum ve Durumları (8 Pencere)
    pub saved_window_states: [Option<WindowState>; 11],

    // İleri Seviye Ayarlar
    pub dhcp_enabled: bool,
    pub static_ip: [u8; 4],
    pub vfs_mounted: bool,
    pub kernel_debug_log: bool,

    // Donanım Testleri (Testler sekmesi)
    pub test_screen_pattern: u8,
    pub test_ram_tested: bool,
    pub test_ram_ok: bool,
    pub test_ram_kib: u32,
    pub test_ram_ms: u32,
    pub test_ram_errors: u32,
    pub test_disk_tested: bool,
    pub test_disk_ok: bool,
    pub test_disk_sectors: u32,
    pub test_disk_ms: u32,
    pub test_disk_kbps: u32,
    pub test_audio_tested: bool,
    pub test_audio_ok: bool,
    pub test_audio_msg: &'static str,
    pub test_mouse_x: usize,
    pub test_mouse_y: usize,
    pub test_mouse_over: bool,
    pub test_mouse_buttons: u8,
    pub test_wheel: u32,
    pub test_trail: [u32; 96],
    pub test_trail_len: u8,
    pub test_trail_head: u8,

    pub status_msg: &'static str,
}

impl SystemSettings {
    pub const fn default() -> Self {
        Self {
            active_tab: 0,
            focused_control: 0,
            theme: ThemeId::VelvetDark,
            keyboard_layout: KeyboardLayout::TurkishQ,
            language: UiLanguage::English,
            region: 0,
            volume: 75,
            is_muted: false,
            mouse_speed: 5,
            double_click_ms: 300,
            reverse_scroll: false,
            screen_res: (1280, 720),
            display_res: (1280, 720),
            zoom: 100,
            night_light: false,
            night_light_level: 55,
            saved_window_states: [None; 11],
            dhcp_enabled: true,
            static_ip: [192, 168, 1, 100],
            vfs_mounted: true,
            kernel_debug_log: true,
            test_screen_pattern: 0,
            test_ram_tested: false,
            test_ram_ok: false,
            test_ram_kib: 0,
            test_ram_ms: 0,
            test_ram_errors: 0,
            test_disk_tested: false,
            test_disk_ok: false,
            test_disk_sectors: 0,
            test_disk_ms: 0,
            test_disk_kbps: 0,
            test_audio_tested: false,
            test_audio_ok: false,
            test_audio_msg: "",
            test_mouse_x: 0,
            test_mouse_y: 0,
            test_mouse_over: false,
            test_mouse_buttons: 0,
            test_wheel: 0,
            test_trail: [0u32; 96],
            test_trail_len: 0,
            test_trail_head: 0,
            status_msg: "Live CD Persistent Storage: Ready",
        }
    }

    pub fn load_from_vfs(&mut self, vfs: &VirtualFileSystem) {
        let data = match vfs.read_file("settings.cfg") { Some(d) => d, None => return };
        let text = match core::str::from_utf8(data) { Ok(s) => s, Err(_) => return };
        for item in text.split(';') {
            let mut p = item.split('='); let key=p.next().unwrap_or(""); let val=p.next().unwrap_or("");
            match key {
                "volume" => if let Ok(v)=val.parse::<u8>() { self.volume=v.min(100); },
                "mouse_speed" => if let Ok(v)=val.parse::<u8>() { self.mouse_speed=v.clamp(1,10); },
                "double_click" => if let Ok(v)=val.parse::<u16>() { self.double_click_ms=v.clamp(150,600); },
                "reverse_scroll" => self.reverse_scroll=val=="1",
                "muted" => self.is_muted=val=="1",
                "night" => self.night_light=val=="1",
                "night_level" => if let Ok(v)=val.parse::<u8>() { self.night_light_level=v.min(100); },
                "dhcp" => self.dhcp_enabled=val!="0",
                "keyboard" => self.keyboard_layout=if val=="en" { KeyboardLayout::EnglishUS } else { KeyboardLayout::TurkishQ },
                "language" => { self.language=if val=="en" { UiLanguage::English } else { UiLanguage::Turkish }; crate::gui::set_ui_language_english(self.language.is_english()); },
                "zoom" => { self.zoom=match val { "125"=>125, "150"=>150, _=>100 }; crate::gui::set_font_zoom(self.zoom); },
                "region" => if let Ok(v)=val.parse::<u8>() { self.region=v.min(2); },
                "res" => { let mut r=val.split('x'); if let (Some(a),Some(b))=(r.next(),r.next()) { if let (Ok(w),Ok(h))=(a.parse(),b.parse()) { self.screen_res=(w,h); } } },
                "theme" => { self.theme = match val { "daylight"=>ThemeId::Daylight, "nord"=>ThemeId::Nord, "terminal"=>ThemeId::TerminalGreen, "sunset"=>ThemeId::Sunset, _=>ThemeId::VelvetDark }; theme::set_theme(self.theme); },
                _ => {}
            }
        }
        self.status_msg=crate::gui::ui_text("Ayarlar kalıcı depolamadan yüklendi.", "Settings loaded from persistent storage.");
    }

    // -----------------------------------------------------------------
    // Geometri yardımcıları (main.rs tıklama testi de bunları kullanır)
    // -----------------------------------------------------------------
    pub fn tab_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + TAB_X, win_y + sp(36) + i * sp(30), sp(TAB_W), sp(TAB_H))
    }
    // Sağ içerik sütununun genişliği (metinlerin butonlarla çakışmaması için).
    #[allow(dead_code)]
    const CONTENT_W: usize = 400;
    #[allow(dead_code)]
    const BTN_X: usize = CONTENT_X + 280; // sağdaki kontrol butonlarının sol kenarı

    // --- Tab 0 (Temel) Geometri ---
    pub fn theme_btn_rect(win_x: usize, win_y: usize, _win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 260, win_y + sp(56), sp(96), sp(22))
    }
    pub fn kbd_btn_rect(win_x: usize, win_y: usize, _win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 260, win_y + sp(80), sp(96), sp(22))
    }
    pub fn language_btn_rect(win_x: usize, win_y: usize, _win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 260, win_y + sp(104), sp(96), sp(22))
    }
    pub fn volume_track_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + sp(200), sp(200), sp(10))
    }
    pub fn mute_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 260, win_y + sp(194), sp(96), sp(22))
    }
    pub fn theme_swatch_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + i * sp(26), win_y + sp(252), sp(22), sp(18))
    }
    pub fn theme_swatch_count() -> usize {
        ThemeId::ALL.len()
    }
    pub fn mouse_speed_track_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (x + CONTENT_X, y + sp(332), sp(200), sp(10))
    }
    pub fn double_click_dec_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (x + CONTENT_X + 160, y + sp(364), sp(30), sp(22))
    }
    pub fn double_click_inc_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (x + CONTENT_X + 270, y + sp(364), sp(30), sp(22))
    }
    pub fn reverse_scroll_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (x + CONTENT_X + 160, y + sp(400), sp(120), sp(22))
    }

    // --- Tab 1 (İleri / Gelişmiş) Geometri ---
    pub fn resolution_btn_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + (i % 2) * sp(150), win_y + sp(164) + (i / 2) * sp(28), sp(140), sp(22))
    }
    pub fn resolution_name(i: usize) -> &'static str { ["800x600", "1024x768", "1280x720", "1920x1080"][i.min(3)] }
    pub fn zoom_btn_rect(x: usize, y: usize, i: usize) -> (usize, usize, usize, usize) {
        (x + CONTENT_X + 160 + i * sp(70), y + sp(232), sp(64), sp(22))
    }
    pub fn night_light_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (x + CONTENT_X + 260, y + sp(270), sp(96), sp(22))
    }
    pub fn night_light_track_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (x + CONTENT_X, y + sp(328), sp(200), sp(10))
    }
    pub fn region_btn_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (x + CONTENT_X + 160, y + sp(360), sp(200), sp(22))
    }
    pub fn region_offset(&self) -> i8 { match self.region { 1 => 0, 2 => -5, _ => 3 } }
    pub fn region_name(&self, english: bool) -> &'static str {
        match (self.region, english) { (1,true)=>"UTC", (1,false)=>"UTC", (2,true)=>"US Eastern (UTC-5)", (2,false)=>"ABD Doğu (UTC-5)", (_,true)=>"Türkiye (UTC+3)", (_,false)=>"Türkiye (UTC+3)" }
    }

    // --- Tab 2 (Ağ) Geometri ---
    pub fn dhcp_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + sp(186), sp(140), sp(24))
    }

    // --- Ortak Butonlar ---
    pub fn save_btn_rect(win_x: usize, win_y: usize, win_h: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + win_h.saturating_sub(sp(48)), sp(88), sp(24))
    }
    pub fn reset_btn_rect(win_x: usize, win_y: usize, win_h: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + 104, win_y + win_h.saturating_sub(sp(48)), sp(88), sp(24))
    }

    // -----------------------------------------------------------------
    // Testler sekmesi (4) geometri yardımcıları
    // -----------------------------------------------------------------
    /// Ekran testi desen düğmeleri.
    pub fn test_screen_btn_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X + i * sp(70), win_y + sp(82), sp(64), sp(22))
    }
    pub fn test_screen_btn_count() -> usize { 6 }
    /// Seçili deseni gösteren önizleme dikdörtgeni.
    pub fn test_screen_preview_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        let w = win_w.saturating_sub(CONTENT_X + 16).max(120);
        (win_x + CONTENT_X, win_y + sp(110), w, sp(40))
    }
    /// Fare test alanı: imleç içine girdiğinde konum/iz ve düğmeler sınanır.
    pub fn test_mouse_pad_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + sp(184), sp(280), sp(96))
    }
    pub fn test_ram_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + sp(328), sp(100), sp(22))
    }
    pub fn test_disk_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + sp(384), sp(100), sp(22))
    }
    pub fn test_audio_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + CONTENT_X, win_y + sp(440), sp(100), sp(22))
    }

    // -----------------------------------------------------------------
    // Aksiyonlar
    // -----------------------------------------------------------------
    pub fn reset_defaults(&mut self) {
        // Ekran çözünürlüğü açılışta Limine tarafından belirlenir ve çalışma
        // zamanında değiştirilemediğinden gerçek donanım çözünürlüğü korunur.
        // Bunun dışındaki TÜM ayarlar varsayılan değerlerine döndürülür.
        let screen_res = self.screen_res;
        let display_res = self.display_res;
        *self = Self::default();
        self.screen_res = screen_res;
        self.display_res = display_res;
        self.test_screen_pattern = 0;
        self.test_ram_tested = false;
        self.test_ram_ok = false;
        self.test_ram_kib = 0;
        self.test_ram_ms = 0;
        self.test_ram_errors = 0;
        self.test_disk_tested = false;
        self.test_disk_ok = false;
        self.test_disk_sectors = 0;
        self.test_disk_ms = 0;
        self.test_disk_kbps = 0;
        self.test_audio_tested = false;
        self.test_audio_ok = false;
        self.test_audio_msg = "";
        self.test_mouse_x = 0;
        self.test_mouse_y = 0;
        self.test_mouse_over = false;
        self.test_mouse_buttons = 0;
        self.test_wheel = 0;
        self.test_trail = [0u32; 96];
        self.test_trail_len = 0;
        self.test_trail_head = 0;
        self.status_msg = crate::gui::ui_text("Tüm ayarlar varsayılana döndürüldü!", "All settings restored to defaults!");
    }

    pub fn run_audio_test(&mut self, audio_dev: &mut crate::drivers::audio::AudioDevice) {
        self.test_audio_tested = true;
        let (msg, ok) = audio_dev.test_audio_subsystem();
        self.test_audio_ok = ok;
        self.test_audio_msg = msg;
        self.status_msg = msg;
    }

    /// Ayarları aktif sistem bileşenlerine uygular.
    pub fn apply_runtime(&self, keyboard: &mut crate::drivers::keyboard::KeyboardState, compositor: &mut MinGuiCompositor, now: &mut DateTime) {
        crate::gui::set_font_zoom(self.zoom);
        crate::gui::theme::set_theme(self.theme);
        keyboard.layout = self.keyboard_layout;
        crate::gui::set_ui_language_english(self.language.is_english());
        *now = DateTime::read().with_hour_offset(self.region_offset());
        compositor.cal_year = now.year;
        compositor.cal_month = now.month;
    }

    // -----------------------------------------------------------------
    // Donanım Testleri
    // -----------------------------------------------------------------

    /// RAM testi: heap'ten bir blok ayırıp desen yazıp tersini doğrular.
    pub fn run_ram_test(&mut self) {
        const SIZE: usize = 256 * 1024; // 256 KiB
        self.test_ram_tested = true;
        let mut v: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(SIZE);
        if v.try_reserve_exact(SIZE).is_err() {
            self.test_ram_ok = false;
            self.test_ram_kib = 0;
            self.test_ram_ms = 0;
            self.test_ram_errors = 0;
            self.status_msg = crate::gui::ui_text("RAM testi: yeterli bellek yok.", "RAM test: not enough memory.");
            return;
        }
        v.resize(SIZE, 0);
        let start = crate::monotonic_ms();
        let mut errors = 0u32;
        for (i, b) in v.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(31).wrapping_add(7) ^ 0xA5;
        }
        for (i, b) in v.iter().enumerate() {
            let expect = (i as u8).wrapping_mul(31).wrapping_add(7) ^ 0xA5;
            if *b != expect { errors = errors.saturating_add(1); }
        }
        for b in v.iter_mut() { *b = !*b; }
        for (i, b) in v.iter().enumerate() {
            let expect = ((i as u8).wrapping_mul(31).wrapping_add(7) ^ 0xA5) ^ 0xFF;
            if *b != expect { errors = errors.saturating_add(1); }
        }
        let ms = crate::monotonic_ms().saturating_sub(start).min(u32::MAX as u64) as u32;
        drop(v);
        self.test_ram_ok = errors == 0;
        self.test_ram_kib = (SIZE / 1024) as u32;
        self.test_ram_ms = ms;
        self.test_ram_errors = errors;
        self.status_msg = if errors == 0 {
            crate::gui::ui_text("RAM testi başarılı.", "RAM test passed.")
        } else {
            crate::gui::ui_text("RAM testi hataları buldu!", "RAM test found errors!")
        };
    }

    /// Disk testi: toplam ~1 MiB kesim okuyup süre/hatayı ölçer (salt okunur).
    pub fn run_disk_test(&mut self) {
        const MAX_SECTORS: u64 = 2048;
        const CHUNK: usize = 64;
        self.test_disk_tested = true;
        let mut buf = alloc::vec![0u8; CHUNK * 512];
        let start = crate::monotonic_ms();
        let mut total = 0u32;
        let mut ok = true;
        let mut found = false;
        for drive in 0..4u8 {
            let Some(dev) = crate::drivers::ata::device(drive) else { continue; };
            if dev.kind == crate::drivers::ata::DeviceKind::None || dev.sectors == 0 { continue; }
            found = true;
            let max = dev.sectors.min(MAX_SECTORS) as u32;
            let mut lba = 0u32;
            while lba < max {
                let want = ((max - lba).min(CHUNK as u32)) as usize;
                let got = crate::drivers::ata::read_sectors(drive, lba, want, &mut buf[..want * 512]);
                if got == 0 { ok = false; break; }
                total = total.saturating_add(got as u32);
                if got < want { break; }
                lba += got as u32;
            }
            if !ok { break; }
        }
        let ms = crate::monotonic_ms().saturating_sub(start).max(1);
        drop(buf);
        let kb = total as u64 * 512 / 1024;
        self.test_disk_ok = ok && found;
        self.test_disk_sectors = total;
        self.test_disk_ms = ms.min(u32::MAX as u64) as u32;
        self.test_disk_kbps = (kb.saturating_mul(1000) / ms).min(u32::MAX as u64) as u32;
        self.status_msg = if !found {
            crate::gui::ui_text("Disk bulunamadı.", "No disk detected.")
        } else if ok {
            crate::gui::ui_text("Disk testi başarılı.", "Disk test passed.")
        } else {
            crate::gui::ui_text("Disk okuma hatası.", "Disk read error.")
        };
    }

    /// Fare testi: her karede main döngüsünden çağrılır. İmleç test alanı
    /// üzerindeyken konum, iz, düğmeler kaydedilir.
    pub fn capture_mouse_test(
        &mut self, mx: usize, my: usize,
        left: bool, right: bool, middle: bool,
        win_x: usize, win_y: usize,
    ) {
        let pad = Self::test_mouse_pad_rect(win_x, win_y);
        let inside = mx >= pad.0 && my >= pad.1 && mx < pad.0 + pad.2 && my < pad.1 + pad.3;
        self.test_mouse_over = inside;
        if inside {
            let rx = mx - pad.0;
            let ry = my - pad.1;
            let changed = rx != self.test_mouse_x || ry != self.test_mouse_y;
            self.test_mouse_x = rx;
            self.test_mouse_y = ry;
            if changed {
                let packed = ((rx as u32 & 0xFFFF) << 16) | (ry as u32 & 0xFFFF);
                let head = (self.test_trail_head as usize) % self.test_trail.len();
                self.test_trail[head] = packed;
                self.test_trail_head = ((head + 1) % self.test_trail.len()) as u8;
                if (self.test_trail_len as usize) < self.test_trail.len() {
                    self.test_trail_len += 1;
                }
            }
        }
        let mut b = 0u8;
        if left { b |= 1; }
        if right { b |= 2; }
        if middle { b |= 4; }
        self.test_mouse_buttons = b;
    }

    /// Tekerlek testi sayacını büyütür (main döngüsünden çağrılır).
    pub fn note_scroll_test(&mut self, amount: u32) {
        self.test_wheel = self.test_wheel.wrapping_add(amount);
    }

    pub fn cycle_theme(&mut self) {
        self.theme = theme::cycle();
        self.status_msg = crate::gui::ui_text("Tema değiştirildi: Sarman temayı sever!", "Theme changed: Sarman likes this one!");
    }

    pub fn set_volume_from_click(&mut self, x: usize) {
        let t = Self::volume_track_rect(0, 0);
        self.set_volume_from_range(x, t.0, t.2);
    }
    pub fn set_volume_from_range(&mut self, x: usize, start: usize, width: usize) {
        let span = width.saturating_sub(1).max(1);
        let position = x.saturating_sub(start).min(span);
        let ratio = (position * 100 / span).min(100);
        self.volume = ratio as u8;
        if self.is_muted && self.volume > 0 {
            self.is_muted = false;
        }
        self.status_msg = crate::gui::ui_text("Ses düzeyi güncellendi.", "Volume updated.");
    }

    /// Ayarları VFS'e ve kalıcı diske yazar. Pencere yerleşimine dokunmaz;
    /// bu yüzden hem "Kaydet" hem de "Sıfırla" akışından çağrılabilir.
    pub fn write_config(&self, vfs: &mut VirtualFileSystem) {
        let mut cfg=[0u8;384]; let mut n=0usize;
        fn put(d:&mut [u8], n:&mut usize, s:&[u8]) { let l=core::cmp::min(s.len(),d.len()-*n); d[*n..*n+l].copy_from_slice(&s[..l]); *n+=l; }
        fn put_num(d:&mut [u8], n:&mut usize, value:u8) {
            if value >= 100 { put(d,n,&[b'1',b'0'+(value-100)/10,b'0'+(value-100)%10]); }
            else if value >= 10 { put(d,n,&[b'0'+value/10,b'0'+value%10]); }
            else { put(d,n,&[b'0'+value]); }
        }
        let put_u16 = |d: &mut [u8], n: &mut usize, value: usize| {
            let value = value.min(9999);
            let mut digits = [0u8; 4]; let mut len = 0;
            let mut v = value;
            loop { digits[3-len] = b'0' + (v % 10) as u8; len += 1; v /= 10; if v == 0 || len == 4 { break; } }
            put(d, n, &digits[4-len..]);
        };
        put(&mut cfg,&mut n,b"volume="); put_num(&mut cfg,&mut n,self.volume);
        put(&mut cfg,&mut n,b";mouse_speed="); put_num(&mut cfg,&mut n,self.mouse_speed);
        put(&mut cfg,&mut n,b";double_click="); put_u16(&mut cfg,&mut n,self.double_click_ms as usize);
        put(&mut cfg,&mut n,b";reverse_scroll="); put(&mut cfg,&mut n,if self.reverse_scroll{b"1"}else{b"0"});
        put(&mut cfg,&mut n,b";muted="); put(&mut cfg,&mut n,if self.is_muted{b"1"}else{b"0"}); put(&mut cfg,&mut n,b";res=");
        put_u16(&mut cfg,&mut n,self.screen_res.0); put(&mut cfg,&mut n,b"x"); put_u16(&mut cfg,&mut n,self.screen_res.1);
        put(&mut cfg,&mut n,b";night="); put(&mut cfg,&mut n,if self.night_light{b"1"}else{b"0"});
        put(&mut cfg,&mut n,b";night_level="); put_num(&mut cfg,&mut n,self.night_light_level);
        put(&mut cfg,&mut n,b";keyboard="); put(&mut cfg,&mut n,if self.keyboard_layout==KeyboardLayout::EnglishUS{b"en"}else{b"tr"});
        put(&mut cfg,&mut n,b";language="); put(&mut cfg,&mut n,if self.language.is_english(){b"en"}else{b"tr"});
        put(&mut cfg,&mut n,b";zoom="); put(&mut cfg,&mut n,match self.zoom { 125=>b"125" as &[u8], 150=>b"150", _=>b"100" });
        put(&mut cfg,&mut n,b";region="); put(&mut cfg,&mut n,&[b'0'+self.region.min(2)]);
        put(&mut cfg,&mut n,b";dhcp="); put(&mut cfg,&mut n,if self.dhcp_enabled{b"1"}else{b"0"});
        put(&mut cfg,&mut n,b";theme="); put(&mut cfg,&mut n,match self.theme { ThemeId::VelvetDark=>b"velvet" as &[u8], ThemeId::Daylight=>b"daylight", ThemeId::Nord=>b"nord", ThemeId::TerminalGreen=>b"terminal", ThemeId::Sunset=>b"sunset" });
        vfs.write_file("settings.cfg",&cfg[..n]);
        // Diske/FAT32 bolumune de fiziksel olarak kaydet (kalici ayarlar icin)
        crate::fs::diskfs::save_file_by_name("settings.cfg", &cfg[..n]);
    }

    pub fn save_all(&mut self, compositor: &MinGuiCompositor, vfs: &mut VirtualFileSystem) {
        for (i, opt_win) in compositor.windows.iter().take(self.saved_window_states.len()).enumerate() {
            if let Some(ref win) = opt_win {
                self.saved_window_states[i] = Some(win.get_state());
            }
        }
        self.write_config(vfs);
        self.status_msg = crate::gui::ui_text("Ayarlar ve pencere düzeni diske kalıcı olarak kaydedildi.", "Settings and window layout saved to disk.");
    }

    pub fn restore_windows(&self, compositor: &mut MinGuiCompositor) {
        for (i, opt_state) in self.saved_window_states.iter().enumerate() {
            if let Some(state) = opt_state {
                if let Some(ref mut win) = compositor.windows[i] {
                    win.apply_state(*state);
                }
            }
        }
    }

    pub fn focus_next_control(&mut self) {
        // Sekme 0 (Genel) 10 kontrol: 3 sekme + tema + kbd + dil + sustur + kaydet + sıfırla
        // Sekme 1 (Aygıt) 19 kontrol: 3 sekme + 4 çözünürlük + 3 yakınlaştırma + 4 bölge + dhcp + kaydet + sıfırla
        // Sekme 2 (Gelişmiş) 8 kontrol: 3 sekme + mavi ışık + şiddet + tema + kaydet + sıfırla
        let total = match self.active_tab {
            0 => 10,
            1 => 15,
            2 => 7,
            3 => 7,
            4 => 8,
            _ => 8,
        };
        self.focused_control = (self.focused_control + 1) % total;
        if self.focused_control < 5 {
            self.active_tab = self.focused_control;
        }
    }

    pub fn select_tab_prev(&mut self) {
        self.active_tab = (self.active_tab + 5 - 1) % 5;
        self.focused_control = self.active_tab;
    }

    pub fn select_tab_next(&mut self) {
        self.active_tab = (self.active_tab + 1) % 5;
        self.focused_control = self.active_tab;
    }

    // -----------------------------------------------------------------
    // Çizim
    // -----------------------------------------------------------------
    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        kbd_layout: KeyboardLayout,
        now: &DateTime,
        total_ram_bytes: u64,
        cpu_vendor: &'static str,
        cpu_logical_count: u32,
        audio_driver: &'static str,
        network_driver: &'static str,
        network_status: &'static str,
    ) {
        // Sol Menü / Sekmeler
        fb.draw_rect(win_x + sp(6), win_y + sp(32), sp(TAB_W + 8), win_h.saturating_sub(sp(62)), Color::TITLE_BAR_INACTIVE);
        let tabs = if self.language.is_english() { ["Basic", "Advanced", "Network", "Devices", "Tests"] } else { ["Temel", "İleri", "Ağ", "Aygıtlar", "Testler"] };
        for (i, t) in tabs.iter().enumerate() {
            let r = Self::tab_rect(win_x, win_y, i);
            let is_cur = i == self.active_tab;
            let bg = if is_cur { Color::TITLE_BAR } else { Color::TITLE_BAR_INACTIVE };
            fb.draw_rect(r.0, r.1, r.2, r.3, bg);
            if is_cur {
                fb.draw_rect(r.0, r.1, 3, r.3, Color::ACCENT_BLUE);
            }
            if self.focused_control == i {
                fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::ACCENT_YELLOW);
            }
            let col = if is_cur { Color::ACCENT_BLUE } else { Color::TEXT_MUTED };
            MinGuiCompositor::draw_string_clipped(fb, r.0 + 8, r.1 + 5, t, col,
                (r.0 + 4, r.1 + 2, r.2.saturating_sub(8), r.3.saturating_sub(4)));
        }

        // Sağ İçerik Alanı
        match self.active_tab {
            0 => {
                // --- minOs Sistem ve Sürüm Bilgisi ---
                let ver_str = if self.language.is_english() {
                    "minOs Sarman Edition v1.3.0 (x86_64)"
                } else {
                    "minOs Sarman Sürümü v1.3.0 (x86_64)"
                };
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(36), ver_str, Color::ACCENT_GREEN);

                // --- Tema ---
                let mut theme_str = TextBuf::new();
                theme_str.push_str(if self.language.is_english() { "Theme: " } else { "Tema: " });
                theme_str.push_str(self.theme.name());
                MinGuiCompositor::draw_string_clipped(fb, win_x + CONTENT_X, win_y + sp(60), theme_str.as_str(), Color::TEXT_MAIN,
                    (win_x + CONTENT_X, win_y + sp(56), sp(250), sp(22)));
                let theme_r = Self::theme_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(
                    fb,
                    theme_r,
                    Color::ACCENT_BLUE,
                    if self.language.is_english() { "Change" } else { "Değiştir" },
                    Color::DARK_BG,
                    false,
                );
                if self.focused_control == 4 { fb.draw_rect_outline(theme_r.0, theme_r.1, theme_r.2, theme_r.3, Color::ACCENT_YELLOW); }

                // --- Klavye / Dil ---
                let kbd_str = match kbd_layout {
                    KeyboardLayout::TurkishQ => if self.language.is_english() { "Keyboard: Turkish (TR-Q)" } else { "Klavye: Türkçe (TR-Q)" },
                    KeyboardLayout::EnglishUS => "Keyboard: English (EN-US)",
                };
                MinGuiCompositor::draw_string_clipped(fb, win_x + CONTENT_X, win_y + sp(84), kbd_str, Color::ACCENT_PINK,
                    (win_x + CONTENT_X, win_y + sp(80), sp(250), sp(22)));
                let kbd_r = Self::kbd_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(
                    fb,
                    kbd_r,
                    Color::ACCENT_PINK,
                    if self.language.is_english() { "Layout" } else { "Düzen" },
                    Color::DARK_BG,
                    false,
                );
                if self.focused_control == 5 { fb.draw_rect_outline(kbd_r.0, kbd_r.1, kbd_r.2, kbd_r.3, Color::ACCENT_YELLOW); }

                MinGuiCompositor::draw_string_clipped(fb, win_x + CONTENT_X, win_y + sp(108),
                    if self.language.is_english() { "Interface language:" } else { "Arayüz dili:" }, Color::ACCENT_PINK,
                    (win_x + CONTENT_X, win_y + sp(104), sp(250), sp(22)));
                let lang_r = Self::language_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(fb, lang_r, Color::ACCENT_PINK,
                    if self.language.is_english() { "Turkish" } else { "English" }, Color::DARK_BG, false);
                if self.focused_control == 6 { fb.draw_rect_outline(lang_r.0, lang_r.1, lang_r.2, lang_r.3, Color::ACCENT_YELLOW); }

                // --- Canlı Saat & Tarih ---
                let mut clock = TextBuf::new();
                clock.push_str(if self.language.is_english() { "Time: " } else { "Saat: " });
                clock.push_u2(now.hour as u32);
                clock.push_byte(b':');
                clock.push_u2(now.minute as u32);
                clock.push_byte(b':');
                clock.push_u2(now.second as u32);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(138), clock.as_str(), Color::ACCENT_GREEN);

                let mut date = TextBuf::new();
                date.push_str(if self.language.is_english() { "Date: " } else { "Tarih: " });
                date.push_u2(now.day as u32);
                date.push_byte(b' ');
                date.push_str(if self.language.is_english() { DateTime::month_name_en(now.month) } else { DateTime::month_name(now.month) });
                date.push_byte(b' ');
                date.push_u4(now.year as u32);
                date.push_str(", ");
                let weekday = DateTime::weekday(now.year, now.month, now.day);
                date.push_str(if self.language.is_english() { DateTime::weekday_name_en(weekday) } else { DateTime::weekday_name(weekday) });
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X + 160, win_y + sp(138), date.as_str(), Color::TEXT_MAIN);

                // --- Ses ---
                let mut vol = TextBuf::new();
                vol.push_str(if self.language.is_english() { "Volume: %" } else { "Ses Düzeyi: %" });
                vol.push_usize(self.volume as usize);
                if self.is_muted {
                    vol.push_str(if self.language.is_english() { " (MUTED)" } else { " (SUSTURULDU)" });
                }
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(180), vol.as_str(), Color::TEXT_MAIN);

                let track = Self::volume_track_rect(win_x, win_y);
                fb.draw_rect(track.0, track.1, track.2, track.3, Color::TITLE_BAR_INACTIVE);
                let fill_w = (track.2 * self.volume as usize) / 100;
                let fill_col = if self.is_muted { Color::TEXT_MUTED } else { Color::ACCENT_GREEN };
                fb.draw_rect(track.0, track.1, fill_w, track.3, fill_col);
                fb.draw_rect(track.0 + fill_w.saturating_sub(4), track.1 - 4, 8, track.3 + 8, Color::TEXT_MAIN);

                let mute_r = Self::mute_btn_rect(win_x, win_y);
                let mute_bg = if self.is_muted { Color::ACCENT_RED } else { Color::TITLE_BAR };
                let mute_lbl = match (self.is_muted, self.language.is_english()) { (true,true)=>"Unmute", (false,true)=>"Mute", (true,false)=>"Sesi Aç", (false,false)=>"Sustur" };
                MinGuiCompositor::draw_button(fb, mute_r, mute_bg, mute_lbl, Color::TEXT_MAIN, false);
                if self.focused_control == 8 { fb.draw_rect_outline(mute_r.0, mute_r.1, mute_r.2, mute_r.3, Color::ACCENT_YELLOW); }

                // --- Tema Palet Seridi (dogrudan secim icin renk ornekleri) ---
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(232),
                    if self.language.is_english() { "Color Themes:" } else { "Renk Paletleri:" }, Color::TEXT_MUTED);
                for (i, tid) in ThemeId::ALL.iter().enumerate() {
                    let r = Self::theme_swatch_rect(win_x, win_y, i);
                    let (bg, accent) = theme::preview_colors(*tid);
                    fb.draw_rect(r.0, r.1, r.2, r.3, bg);
                    fb.draw_rect(r.0 + 3, r.1 + 3, r.2 - 6, r.3 - 6, accent);
                    let border = if *tid == self.theme { Color::ACCENT_GREEN } else { Color::BORDER };
                    fb.draw_rect_outline(r.0, r.1, r.2, r.3, border);
                }

                // --- Fare Ayarları Bölümü ---
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(288),
                    crate::gui::ui_text("Fare Ayarları", "Mouse Settings"), Color::ACCENT_PINK);

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(312),
                    crate::gui::ui_text("İşaretçi hızı:", "Pointer speed:"), Color::TEXT_MAIN);
                let speed_track = Self::mouse_speed_track_rect(win_x, win_y);
                fb.draw_rect(speed_track.0, speed_track.1, speed_track.2, speed_track.3, Color::TITLE_BAR_INACTIVE);
                let speed_fill = speed_track.2 * (self.mouse_speed.saturating_sub(1)) as usize / 9;
                fb.draw_rect(speed_track.0, speed_track.1, speed_fill, speed_track.3, Color::ACCENT_BLUE);
                fb.draw_rect(speed_track.0 + speed_fill.saturating_sub(4), speed_track.1.saturating_sub(4), 8, speed_track.3 + 8, Color::TEXT_MAIN);
                let mut speed_label = TextBuf::new(); speed_label.push_usize(self.mouse_speed as usize);
                MinGuiCompositor::draw_string(fb, speed_track.0 + speed_track.2 + 12, speed_track.1 - 2, speed_label.as_str(), Color::TEXT_MAIN);

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(368),
                    crate::gui::ui_text("Çift tıklama:", "Double-click:"), Color::TEXT_MAIN);
                let dec = Self::double_click_dec_rect(win_x, win_y);
                let inc = Self::double_click_inc_rect(win_x, win_y);
                MinGuiCompositor::draw_button(fb, dec, Color::TITLE_BAR, "-", Color::TEXT_MAIN, false);
                MinGuiCompositor::draw_button(fb, inc, Color::TITLE_BAR, "+", Color::TEXT_MAIN, false);
                let mut click_label = TextBuf::new();
                click_label.push_usize(self.double_click_ms as usize);
                click_label.push_str(" ms");
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X + 200, win_y + sp(368), click_label.as_str(), Color::ACCENT_GREEN);

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(404),
                    crate::gui::ui_text("Tekerlek yönü:", "Scroll direction:"), Color::TEXT_MAIN);
                let reverse = Self::reverse_scroll_rect(win_x, win_y);
                MinGuiCompositor::draw_button(fb, reverse, if self.reverse_scroll { Color::ACCENT_GREEN } else { Color::TITLE_BAR },
                    match (self.reverse_scroll, self.language.is_english()) {
                        (true, true) => "Reversed", (false, true) => "Normal",
                        (true, false) => "Ters", (false, false) => "Normal",
                    }, Color::TEXT_MAIN, false);
            }
            1 => {
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(38), crate::gui::ui_text("Çekirdek Günlüğü: Aktif (/dev/klog)", "Kernel Log: Active (/dev/klog)"), Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(58), crate::gui::ui_text("VFS Sürücüsü: RamFS TarFS v1.0", "VFS Driver: RamFS TarFS v1.0"), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(78), crate::gui::ui_text("Donanım Hızlandırma: SSE2 Aktif", "Hardware Acceleration: SSE2 Active"), Color::ACCENT_PINK);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(98), crate::gui::ui_text("Önyükleme: Limine Stivale2", "Bootloader: Limine Stivale2"), Color::TEXT_MUTED);
                let mut audio_status = TextBuf::new();
                audio_status.push_str(crate::gui::ui_text("Ses sürücüsü: ", "Audio driver: "));
                audio_status.push_str(audio_driver);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(118), audio_status.as_str(), Color::ACCENT_BLUE);

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(144),
                    if self.language.is_english() { "Target resolution:" } else { "Hedef çözünürlük:" }, Color::ACCENT_YELLOW);
                for i in 0..4 {
                    let r = Self::resolution_btn_rect(win_x,win_y,i);
                    let active = self.screen_res == match i {0=>(800,600),1=>(1024,768),2=>(1280,720),_=>(1920,1080)};
                    MinGuiCompositor::draw_button(fb, r, if active{Color::ACCENT_GREEN}else{Color::TITLE_BAR}, Self::resolution_name(i), Color::TEXT_MAIN, false);
                    if self.focused_control == 4 + i { fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::ACCENT_YELLOW); }
                }

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(236),
                    if self.language.is_english() { "Font zoom:" } else { "Yazı boyutu:" }, Color::TEXT_MAIN);
                for (i, value) in [100u8,125,150].iter().enumerate() {
                    let label = match value { 125=>"125%", 150=>"150%", _=>"100%" };
                    let r = Self::zoom_btn_rect(win_x,win_y,i);
                    MinGuiCompositor::draw_button(fb, r, if self.zoom==*value{Color::ACCENT_GREEN}else{Color::TITLE_BAR}, label, Color::TEXT_MAIN, false);
                    if self.focused_control == 8 + i { fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::ACCENT_YELLOW); }
                }

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(274),
                    match (self.night_light,self.language.is_english()) { (true,true)=>"Blue light: On", (false,true)=>"Blue light: Off", (true,false)=>"Mavi ışık: Açık", (false,false)=>"Mavi ışık: Kapalı" }, Color::ACCENT_YELLOW);
                let nl_btn = Self::night_light_rect(win_x,win_y);
                MinGuiCompositor::draw_button(fb, nl_btn, Color::ACCENT_YELLOW, match (self.night_light,self.language.is_english()) { (true,true)=>"Off", (false,true)=>"On", (true,false)=>"Kapat", (false,false)=>"Aç" }, Color::DARK_BG, false);
                if self.focused_control == 11 { fb.draw_rect_outline(nl_btn.0, nl_btn.1, nl_btn.2, nl_btn.3, Color::ACCENT_YELLOW); }

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(308),
                    if self.language.is_english() { "Filter strength:" } else { "Filtre şiddeti:" }, Color::TEXT_MAIN);
                let nl = Self::night_light_track_rect(win_x,win_y);
                fb.draw_rect(nl.0,nl.1,nl.2,nl.3,Color::TITLE_BAR_INACTIVE);
                let fill = nl.2*self.night_light_level as usize/100;
                fb.draw_rect(nl.0,nl.1,fill,nl.3,Color::ACCENT_YELLOW);
                fb.draw_rect(nl.0+fill.saturating_sub(4),nl.1.saturating_sub(4),8,nl.3+8,Color::TEXT_MAIN);
                let mut level_text = TextBuf::new(); level_text.push_usize(self.night_light_level as usize); level_text.push_byte(b'%');
                MinGuiCompositor::draw_string(fb,nl.0+nl.2+12,nl.1 - 2,level_text.as_str(),Color::TEXT_MAIN);

                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(364),
                    if self.language.is_english() { "Time Zone:" } else { "Bölge / Saat:" }, Color::TEXT_MAIN);
                let reg_r = Self::region_btn_rect(win_x,win_y);
                MinGuiCompositor::draw_button(fb, reg_r, Color::ACCENT_BLUE, self.region_name(self.language.is_english()), Color::TEXT_MAIN, false);
                if self.focused_control == 12 { fb.draw_rect_outline(reg_r.0, reg_r.1, reg_r.2, reg_r.3, Color::ACCENT_YELLOW); }
            }
            2 => {
                let mut driver = TextBuf::new();
                driver.push_str(crate::gui::ui_text("Ağ sürücüsü: ", "Network driver: "));
                driver.push_str(network_driver);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(42), driver.as_str(), Color::ACCENT_BLUE);
                let mut status = TextBuf::new();
                status.push_str(crate::gui::ui_text("Durum: ", "Status: "));
                status.push_str(network_status);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(68), status.as_str(), Color::ACCENT_GREEN);
                MinGuiCompositor::draw_string_clipped(fb, win_x + CONTENT_X, win_y + sp(94),
                    crate::gui::ui_text("Yığın: Ethernet / IPv4 / ARP / ICMP / UDP / TCP / DHCP / DNS", "Stack: Ethernet / IPv4 / ARP / ICMP / UDP / TCP / DHCP / DNS"),
                    Color::TEXT_MUTED, (win_x + CONTENT_X, win_y + sp(92), win_w.saturating_sub(CONTENT_X + 12), sp(18)));
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(120), if self.dhcp_enabled { crate::gui::ui_text("IP yöntemi: DHCP", "IP mode: DHCP") } else { crate::gui::ui_text("IP yöntemi: Statik", "IP mode: Static") }, Color::ACCENT_YELLOW);
                let mut ip = TextBuf::new(); ip.push_str("IP: "); for (i, b) in self.static_ip.iter().enumerate() { if i > 0 { ip.push_byte(b'.'); } ip.push_usize(*b as usize); }
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(142), ip.as_str(), Color::TEXT_MAIN);
                MinGuiCompositor::draw_string(fb, win_x + CONTENT_X, win_y + sp(164), "Gateway: 10.0.2.2    DNS: 10.0.2.3", Color::TEXT_MAIN);
                let dhcp_btn = Self::dhcp_btn_rect(win_x, win_y);
                MinGuiCompositor::draw_button(fb, dhcp_btn, Color::ACCENT_BLUE, if self.dhcp_enabled { "Static IP" } else { "DHCP" }, Color::DARK_BG, false);
                if self.focused_control == 4 { fb.draw_rect_outline(dhcp_btn.0, dhcp_btn.1, dhcp_btn.2, dhcp_btn.3, Color::ACCENT_YELLOW); }
            }
            3 => {
                render_device_inventory(
                    fb, win_x + CONTENT_X, win_y + sp(42),
                    win_w.saturating_sub(CONTENT_X + 16), self.language,
                    kbd_layout, cpu_vendor, cpu_logical_count, total_ram_bytes,
                );
            }
            _ => {
                self.render_tests(fb, win_x, win_y, win_w, total_ram_bytes);
            }
        }

        // Alt Aksiyon Butonları (Kaydet & Sıfırla)
        let save_r = Self::save_btn_rect(win_x, win_y, win_h);
        let reset_r = Self::reset_btn_rect(win_x, win_y, win_h);
        MinGuiCompositor::draw_button(fb, save_r, Color::ACCENT_GREEN, crate::gui::ui_text("Kaydet", "Save"), Color::DARK_BG, false);
        MinGuiCompositor::draw_button(fb, reset_r, Color::ACCENT_RED, crate::gui::ui_text("Sıfırla", "Reset"), Color::WHITE, false);

        let (save_idx, reset_idx) = match self.active_tab {
            0 => (9, 10),
            1 => (13, 14),
            _ => (5, 6),
        };
        if self.focused_control == save_idx { fb.draw_rect_outline(save_r.0, save_r.1, save_r.2, save_r.3, Color::ACCENT_YELLOW); }
        if self.focused_control == reset_idx { fb.draw_rect_outline(reset_r.0, reset_r.1, reset_r.2, reset_r.3, Color::ACCENT_YELLOW); }

        // Durum Çubuğu
        let bar_y = win_y + win_h.saturating_sub(sp(22));
        fb.draw_rect(win_x + sp(6), bar_y, win_w.saturating_sub(sp(12)), sp(20), Color::TASKBAR_BG);
        MinGuiCompositor::draw_string(fb, win_x + sp(12), bar_y + sp(6), self.status_msg, Color::ACCENT_YELLOW);
    }

    /// Testler sekmesi: ekran deseni, fare (konum/iz/düğme/tekerlek), RAM ve disk.
    fn render_tests(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        total_ram_bytes: u64,
    ) {
        let english = self.language.is_english();
        let cx = win_x + CONTENT_X;

        MinGuiCompositor::draw_string(fb, cx, win_y + sp(40),
            crate::gui::ui_text("Donanım Testleri", "Hardware Tests"), Color::ACCENT_PINK);

        // ---- Ekran testi ----
        MinGuiCompositor::draw_string(fb, cx, win_y + sp(64),
            crate::gui::ui_text("Ekran: renk deseni seçin", "Display: choose a colour pattern"), Color::TEXT_MAIN);
        let labels = if english {
            ["Red", "Green", "Blue", "White", "Black", "Bars"]
        } else {
            ["Kırmızı", "Yeşil", "Mavi", "Beyaz", "Siyah", "Çubuk"]
        };
        for (i, label) in labels.iter().enumerate() {
            let active = self.test_screen_pattern as usize == i;
            let r = Self::test_screen_btn_rect(win_x, win_y, i);
            MinGuiCompositor::draw_button(fb, r,
                if active { Color::ACCENT_GREEN } else { Color::TITLE_BAR },
                *label, Color::TEXT_MAIN, false);
        }
        let p = Self::test_screen_preview_rect(win_x, win_y, win_w);
        fb.draw_rect_outline(p.0, p.1, p.2, p.3, Color::BORDER);
        let (px, py, pw, ph) = (p.0 + 1, p.1 + 1, p.2.saturating_sub(2), p.3.saturating_sub(2));
        match self.test_screen_pattern {
            0 => fb.draw_rect(px, py, pw, ph, 0xFF0000),
            1 => fb.draw_rect(px, py, pw, ph, 0x00C000),
            2 => fb.draw_rect(px, py, pw, ph, 0x0000FF),
            3 => fb.draw_rect(px, py, pw, ph, Color::WHITE),
            4 => fb.draw_rect(px, py, pw, ph, Color::BLACK),
            _ => {
                // Renk çubukları + gri ton rampası (ekran kalibrasyonu).
                let colors = [0xFF0000u32, 0x00C000, 0x0000FF, 0x00FFFF, 0xFF00FF, 0xFFFF00, Color::WHITE, Color::BLACK];
                let bar_w = (pw / colors.len()).max(1);
                for (i, c) in colors.iter().enumerate() {
                    fb.draw_rect(px + i * bar_w, py, bar_w, ph / 2, *c);
                }
                let top = py + ph / 2;
                let bottom_h = ph - ph / 2;
                let seg = (pw / 16).max(1);
                for i in 0..16 {
                    let v = (i * 255 / 15) as u32;
                    let c = (v << 16) | (v << 8) | v;
                    fb.draw_rect(px + i * seg, top, seg, bottom_h, c);
                }
            }
        }

        // ---- Fare testi ----
        // Not: alan zemini önce boyanır, başlık sonra çizilir; böylece
        // büyük yazı tipinde bile metnin altı alan zeminince kapanmaz.
        let pad = Self::test_mouse_pad_rect(win_x, win_y);
        fb.draw_rect(pad.0, pad.1, pad.2, pad.3, Color::DARK_BG);
        fb.draw_rect_outline(pad.0, pad.1, pad.2, pad.3, Color::BORDER);
        MinGuiCompositor::draw_string(fb, cx, win_y + sp(174),
            crate::gui::ui_text("Fare: imleci alana getirin, düğmeleri/tekerleği deneyin",
                "Mouse: move into the pad, try buttons/wheel"), Color::TEXT_MAIN);
        if self.test_trail_len > 0 {
            let len = self.test_trail.len();
            let count = self.test_trail_len as usize;
            let start = (self.test_trail_head as usize + len - count) % len;
            for k in 0..count {
                let packed = self.test_trail[(start + k) % len];
                let tx = pad.0 + (packed >> 16) as usize;
                let ty = pad.1 + (packed & 0xFFFF) as usize;
                if tx + 2 < pad.0 + pad.2 && ty + 2 < pad.1 + pad.3 {
                    fb.draw_rect(tx, ty, 2, 2, Color::ACCENT_PINK);
                }
            }
        }
        if self.test_mouse_over {
            let mx = pad.0 + self.test_mouse_x.min(pad.2.saturating_sub(1));
            let my = pad.1 + self.test_mouse_y.min(pad.3.saturating_sub(1));
            fb.draw_rect(mx.saturating_sub(2), my.saturating_sub(2), 5, 5, Color::ACCENT_GREEN);
        } else {
            MinGuiCompositor::draw_string(fb, pad.0 + 8, pad.1 + 8,
                crate::gui::ui_text("imleç alanın dışında", "pointer outside pad"), Color::TEXT_MUTED);
        }

        // Düğme göstergeleri (L/M/R)
        let ix = pad.0 + pad.2 + 12;
        let iy = pad.1;
        MinGuiCompositor::draw_string(fb, ix, iy,
            crate::gui::ui_text("Düğmeler", "Buttons"), Color::ACCENT_YELLOW);
        let items: [(&str, u8); 3] = [
            (if english { "Left" } else { "Sol" }, 1),
            (if english { "Middle" } else { "Orta" }, 4),
            (if english { "Right" } else { "Sağ" }, 2),
        ];
        for (i, (name, bit)) in items.iter().enumerate() {
            let y = iy + sp(20) + i * sp(22);
            let on = self.test_mouse_buttons & *bit != 0;
            fb.draw_rect(ix, y, 14, 14, if on { Color::ACCENT_GREEN } else { Color::TITLE_BAR_INACTIVE });
            fb.draw_rect_outline(ix, y, 14, 14, Color::BORDER);
            MinGuiCompositor::draw_string(fb, ix + 22, y + sp(1), *name,
                if on { Color::TEXT_MAIN } else { Color::TEXT_MUTED });
        }

        let mut info = TextBuf::new();
        info.push_str("X: ");
        info.push_usize(if self.test_mouse_over { self.test_mouse_x } else { 0 });
        info.push_str("  Y: ");
        info.push_usize(if self.test_mouse_over { self.test_mouse_y } else { 0 });
        MinGuiCompositor::draw_string(fb, pad.0 + 8, pad.1 + pad.3.saturating_sub(18), info.as_str(), Color::TEXT_MUTED);

        let mut wheel = TextBuf::new();
        wheel.push_str(if english { "Wheel: " } else { "Tekerlek: " });
        wheel.push_usize(self.test_wheel as usize);
        MinGuiCompositor::draw_string(fb, ix, iy + sp(90), wheel.as_str(), Color::ACCENT_BLUE);

        // ---- RAM testi ----
        MinGuiCompositor::draw_string(fb, cx, win_y + sp(306),
            crate::gui::ui_text("Bellek (RAM) testi", "Memory (RAM) test"), Color::ACCENT_YELLOW);
        let ram_btn = Self::test_ram_btn_rect(win_x, win_y);
        MinGuiCompositor::draw_button(fb, ram_btn, Color::ACCENT_BLUE,
            crate::gui::ui_text("Çalıştır", "Run"), Color::DARK_BG, false);
        let mut ram = DeviceLine::new();
        ram.push(if english { "Usable: " } else { "Kullanılabilir: " });
        ram.number(total_ram_bytes / (1024 * 1024));
        ram.push(" MB");
        if self.test_ram_tested {
            let status: &str = if self.test_ram_ok {
                if english { "PASS" } else { "BAŞARILI" }
            } else if english { "FAIL: " } else { "HATA: " };
            ram.push(" | ");
            ram.push(status);
            if self.test_ram_ok {
                ram.number(self.test_ram_kib as u64);
                ram.push(" KiB / ");
                ram.number(self.test_ram_ms as u64);
                ram.push(" ms");
            } else {
                ram.number(self.test_ram_errors as u64);
                ram.push(if english { " errors" } else { " hata" });
            }
        }
        draw_device_line(fb, cx + 120, ram_btn.1 + 4, win_w.saturating_sub(CONTENT_X + 132), &ram,
            if self.test_ram_tested && !self.test_ram_ok { Color::ACCENT_RED } else { Color::TEXT_MAIN });

        // ---- Disk testi ----
        MinGuiCompositor::draw_string(fb, cx, win_y + sp(362),
            crate::gui::ui_text("Disk testi (salt okunur)", "Disk test (read-only)"), Color::ACCENT_YELLOW);
        let disk_btn = Self::test_disk_btn_rect(win_x, win_y);
        MinGuiCompositor::draw_button(fb, disk_btn, Color::ACCENT_BLUE,
            crate::gui::ui_text("Çalıştır", "Run"), Color::DARK_BG, false);
        let mut disk = DeviceLine::new();
        if self.test_disk_tested {
            disk.push(if self.test_disk_ok {
                if english { "PASS | " } else { "BAŞARILI | " }
            } else if english { "FAIL | " } else { "HATA | " });
            disk.number(self.test_disk_sectors as u64);
            disk.push(if english { " sectors in " } else { " sektör / " });
            disk.number(self.test_disk_ms as u64);
            disk.push(" ms | ");
            disk.number(self.test_disk_kbps as u64);
            disk.push(" KB/s");
        } else {
            disk.push(if english { "Not run yet." } else { "Henüz çalıştırılmadı." });
        }
        draw_device_line(fb, cx + 120, disk_btn.1 + 4, win_w.saturating_sub(CONTENT_X + 132), &disk,
            if self.test_disk_tested && !self.test_disk_ok { Color::ACCENT_RED } else { Color::TEXT_MAIN });

        // ---- Ses alt sistemi testi ----
        MinGuiCompositor::draw_string(fb, cx, win_y + sp(418),
            crate::gui::ui_text("Ses testi (HDA DMA / PC Speaker Fallback)", "Audio test (HDA DMA / PC Speaker Fallback)"), Color::ACCENT_YELLOW);
        let audio_btn = Self::test_audio_btn_rect(win_x, win_y);
        MinGuiCompositor::draw_button(fb, audio_btn, Color::ACCENT_BLUE,
            crate::gui::ui_text("Test Et", "Test"), Color::DARK_BG, false);
        let mut audio = DeviceLine::new();
        if self.test_audio_tested {
            audio.push(if self.test_audio_ok {
                if english { "PASS | " } else { "BAŞARILI | " }
            } else {
                if english { "FALLBACK | " } else { "YEDEK | " }
            });
            audio.push(self.test_audio_msg);
        } else {
            audio.push(if english { "HDA DMA + PC Speaker test ready." } else { "HDA DMA + PC Speaker testi hazır." });
        }
        draw_device_line(fb, cx + 120, audio_btn.1 + 4, win_w.saturating_sub(CONTENT_X + 132), &audio,
            if self.test_audio_tested { if self.test_audio_ok { Color::ACCENT_GREEN } else { Color::ACCENT_YELLOW } } else { Color::TEXT_MAIN });
    }
}
