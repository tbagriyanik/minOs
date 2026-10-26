use crate::drivers::keyboard::KeyboardLayout;
use crate::drivers::rtc::DateTime;
use crate::drivers::video::Framebuffer;
use crate::gui::theme::{self, ThemeId};
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};

use crate::fs::vfs::VirtualFileSystem;
use crate::gui::WindowState;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum UiLanguage {
    Turkish,
    English,
}
impl UiLanguage {
    pub fn toggle(&mut self) {
        *self = match *self {
            Self::Turkish => Self::English,
            Self::English => Self::Turkish,
        };
        crate::gui::set_ui_language_english(self.is_english());
    }
    pub fn is_english(&self) -> bool {
        matches!(self, Self::English)
    }
}

struct DeviceLine {
    buf: [u8; 128],
    len: usize,
}
impl DeviceLine {
    fn new() -> Self {
        Self {
            buf: [0; 128],
            len: 0,
        }
    }
    fn push(&mut self, s: &str) {
        for b in s.bytes() {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }
    fn number(&mut self, mut n: u64) {
        let mut tmp = [0u8; 20];
        let mut len = 0;
        if n == 0 {
            self.push("0");
            return;
        }
        while n > 0 {
            tmp[len] = b'0' + (n % 10) as u8;
            len += 1;
            n /= 10;
        }
        while len > 0 {
            len -= 1;
            if self.len < self.buf.len() {
                self.buf[self.len] = tmp[len];
                self.len += 1;
            }
        }
    }
    fn hex_byte(&mut self, value: u8) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        for digit in [value >> 4, value & 0x0F] {
            if self.len < self.buf.len() {
                self.buf[self.len] = HEX[digit as usize];
                self.len += 1;
            }
        }
    }
    fn finish(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

fn draw_device_line(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    width: usize,
    line: &DeviceLine,
    color: u32,
) {
    MinGuiCompositor::draw_string_clipped(fb, x, y, line.finish(), color, (x, y, width, 20));
}

fn render_device_inventory(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    width: usize,
    language: UiLanguage,
    layout: KeyboardLayout,
    cpu: &str,
    cores: u32,
    ram_bytes: u64,
) {
    let english = language.is_english();
    let (fb_width, fb_height, fb_bpp, fb_pitch) = (fb.width, fb.height, fb.bpp, fb.pitch);
    let mut row = 0usize;
    let mut draw = |line: &DeviceLine, color| {
        draw_device_line(fb, x, y + row * sp(24), width, line, color);
        row += 1;
    };

    let mut line = DeviceLine::new();
    line.push(if english { "CPU: " } else { "İşlemci: " });
    line.push(cpu);
    line.push(" | x86_64 | ");
    line.number(cores as u64);
    line.push(if english {
        " logical core(s) | SSE2"
    } else {
        " mantıksal çekirdek | SSE2"
    });
    draw(&line, Color::ACCENT_BLUE);

    let mut line = DeviceLine::new();
    line.push(if english {
        "RAM usable by kernel: "
    } else {
        "Çekirdeğin kullanabildiği RAM: "
    });
    line.number(ram_bytes / (1024 * 1024));
    line.push(" MB");
    draw(&line, Color::ACCENT_YELLOW);

    let mut line = DeviceLine::new();
    let is_virtio = crate::drivers::virtio_gpu::is_detected();
    line.push(if english {
        if is_virtio {
            "Display: VirtIO-GPU (Default 2D Acceleration) | "
        } else {
            "Display: Limine framebuffer | "
        }
    } else {
        if is_virtio {
            "Ekran: VirtIO-GPU (Varsayılan 2D Hızlandırma) | "
        } else {
            "Ekran: Limine framebuffer | "
        }
    });
    line.number(fb_width as u64);
    line.push("x");
    line.number(fb_height as u64);
    line.push(" | ");
    line.number(fb_bpp as u64);
    line.push(" bpp | ");
    line.number(fb_pitch as u64);
    line.push(" B pitch");
    draw(&line, Color::TEXT_MAIN);

    let mut line = DeviceLine::new();
    line.push(if english {
        "Keyboard: PS/2"
    } else {
        "Klavye: PS/2"
    });
    if crate::drivers::usb_hid::keyboard_present() {
        line.push(" + USB HID");
    }
    line.push(" | ");
    line.push(match layout {
        KeyboardLayout::TurkishQ => "TR-Q",
        KeyboardLayout::EnglishUS => "EN-US",
    });
    line.push(if english {
        " | polling driver active"
    } else {
        " | yoklama sürücüsü etkin"
    });
    draw(&line, Color::ACCENT_GREEN);

    let mut line = DeviceLine::new();
    line.push(if english { "Mouse: PS/2" } else { "Fare: PS/2" });
    if crate::drivers::usb_hid::mouse_present() {
        line.push(" + USB HID");
    }
    line.push(if english {
        " | 3 buttons | "
    } else {
        " | 3 düğme | "
    });
    line.push(
        if crate::drivers::ps2::mouse_wheel_enabled() || crate::drivers::usb_hid::mouse_present() {
            if english {
                "wheel detected"
            } else {
                "tekerlek algılandı"
            }
        } else if english {
            "wheel not detected"
        } else {
            "tekerlek algılanmadı"
        },
    );
    draw(&line, Color::ACCENT_PINK);

    let mut line = DeviceLine::new();
    if crate::drivers::hda::is_ready() {
        if crate::drivers::hda::is_dma_progressing() {
            line.push(if english {
                "Audio: Intel HDA | DMA active | 48 kHz stereo | PC Speaker standby"
            } else {
                "Ses: Intel HDA | DMA akışı aktif | 48 kHz stereo | PC Speaker yedeği hazır"
            });
            draw(&line, Color::ACCENT_GREEN);
        } else {
            line.push(if english {
                "Audio: Intel HDA ready | DMA standby / PC Speaker fallback enabled"
            } else {
                "Ses: Intel HDA hazır | DMA beklemede / PC Speaker yedeği devrede"
            });
            draw(&line, Color::ACCENT_BLUE);
        }
    } else {
        line.push(if english {
            "Audio: PC Speaker PWM fallback active (HDA not detected)"
        } else {
            "Ses: PC Speaker PWM yedeği aktif (HDA bulunamadı)"
        });
        draw(&line, Color::ACCENT_YELLOW);
    }

    if crate::drivers::e1000::is_ready() {
        let mut line = DeviceLine::new();
        line.push(
            if crate::drivers::e1000::nic_type() == crate::drivers::e1000::NicType::VmwareVmxnet3 {
                "Ethernet: VMware VMXNET3 | "
            } else {
                "Ethernet: Intel e1000 | "
            },
        );
        line.push(if crate::drivers::e1000::link_up() {
            if english {
                "link up | MAC "
            } else {
                "bağlı | MAC "
            }
        } else {
            if english {
                "link down | MAC "
            } else {
                "bağlantı yok | MAC "
            }
        });
        let mac = unsafe { crate::drivers::e1000::MAC_ADDR };
        for (i, byte) in mac.iter().enumerate() {
            if i > 0 {
                line.push(":");
            }
            line.hex_byte(*byte);
        }
        draw(&line, Color::ACCENT_BLUE);
    }

    if let Some(controller) = crate::drivers::usb::controller_info() {
        let mut line = DeviceLine::new();
        line.push(if english {
            "USB host controller: "
        } else {
            "USB ana denetleyicisi: "
        });
        line.push(match controller.controller_type {
            crate::drivers::usb::UsbControllerType::Uhci => "UHCI (USB 1.1)",
            crate::drivers::usb::UsbControllerType::Ohci => "OHCI (USB 1.1)",
            crate::drivers::usb::UsbControllerType::Ehci => "EHCI (USB 2.0)",
            crate::drivers::usb::UsbControllerType::Xhci => "xHCI (USB 3.x)",
            crate::drivers::usb::UsbControllerType::Other(_) => "USB host controller",
            crate::drivers::usb::UsbControllerType::None => "USB",
        });
        line.push(" | PCI ");
        line.number(controller.bus as u64);
        line.push(":");
        line.number(controller.dev as u64);
        line.push(".");
        line.number(controller.func as u64);
        line.push(" | ");
        line.push(if controller.is_mmio {
            "MMIO base "
        } else {
            "I/O base "
        });
        line.number(controller.base_addr);
        draw(&line, Color::ACCENT_GREEN);
    }

    if crate::drivers::usb_hid::ready() {
        let mut line = DeviceLine::new();
        line.push(if english {
            "USB HID (native xHCI): "
        } else {
            "USB HID (yerel xHCI): "
        });
        line.number(crate::drivers::usb_hid::controller_count() as u64);
        line.push(if english {
            " controller(s) | "
        } else {
            " denetleyici | "
        });
        let dev_count = crate::drivers::usb_hid::device_count();
        line.number(dev_count as u64);
        line.push(if english {
            " active HID interface(s)"
        } else {
            " etkin HID arayüzü"
        });
        draw(&line, Color::ACCENT_GREEN);

        for i in 0..dev_count {
            if let Some((kind, port)) = crate::drivers::usb_hid::get_active_device_info(i) {
                let mut dev_line = DeviceLine::new();
                dev_line.push("  -> ");
                match kind {
                    crate::drivers::usb_hid::HidKind::Keyboard => {
                        dev_line.push(if english {
                            "USB Keyboard"
                        } else {
                            "USB Klavye"
                        });
                    }
                    crate::drivers::usb_hid::HidKind::Mouse => {
                        dev_line.push(if english { "USB Mouse" } else { "USB Fare" });
                    }
                }
                dev_line.push(" | Port: ");
                dev_line.number(port as u64);
                dev_line.push(if english {
                    " (Hotplug Ready)"
                } else {
                    " (Canlı Tak/Çıkar Aktif)"
                });
                draw(&dev_line, Color::TEXT_MAIN);
            }
        }
    }

    for drive in 0..4u8 {
        let Some(device) = crate::drivers::ata::device(drive) else {
            continue;
        };
        let mut line = DeviceLine::new();
        line.push(if english { "Storage " } else { "Depolama " });
        line.number(drive as u64);
        line.push(": ");
        line.push(device.model_str());
        line.push(" | ");
        line.push(match device.kind {
            crate::drivers::ata::DeviceKind::Ata => "ATA",
            crate::drivers::ata::DeviceKind::Atapi => "ATAPI",
            crate::drivers::ata::DeviceKind::None => "",
        });
        line.push(" | ");
        line.number(device.size_mb());
        line.push(" MB | ");
        line.number(device.sectors);
        line.push(" sectors x ");
        line.number(if device.kind == crate::drivers::ata::DeviceKind::Atapi {
            2048
        } else {
            512
        });
        line.push(" B");
        draw(&line, Color::TEXT_MAIN);
    }

    drop(draw);
    if row == 5 {
        let mut line = DeviceLine::new();
        line.push(if english {
            "No additional active devices detected."
        } else {
            "Başka etkin aygıt algılanmadı."
        });
        draw_device_line(fb, x, y + row * sp(24), width, &line, Color::TEXT_MUTED);
    }
}
/// Yardımcı düğme/geometri tanımları (çizim ve tıklama testi ortak kullanır).
const TAB_X: usize = 10;
const TAB_W: usize = 116;
const TAB_H: usize = 26;
const CONTENT_X: usize = 140;

/// Standart dikey ritim ve marjin sabitleri:
/// Tüm sekmelerde başlıklar, metinler, kontroller ve gruplar bu ızgaraya oturur.
pub const UI_TOP_Y: usize = 36; // İçerik başlangıç Y konumu
pub const UI_ROW_STEP: usize = 36; // Satır aralığı (row spacing)
pub const UI_BTN_H: usize = 22; // Standart buton yüksekliği
pub const UI_LABEL_OFFSET: usize = 4; // Butonla aynı satırdaki metnin dikey hizası

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
    pub wallpaper: u8,
    pub wall_color: u32,

    // Saklanan Pencere Konum ve Durumları (8 Pencere)
    pub saved_window_states: [Option<WindowState>; 11],

    // İleri Seviye Ayarlar
    pub dhcp_enabled: bool,
    pub static_ip: [u8; 4],
    pub static_ip_octet: usize,
    pub static_ip_edit: bool,
    pub static_ip_value: u16,
    pub static_ip_digits: u8,
    pub vfs_mounted: bool,
    pub kernel_debug_log: bool,

    // Donanım Testleri (Testler sekmesi)
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
    pub test_net_tested: bool,
    pub test_net_ok: bool,
    pub test_net_ping_ok: bool,
    pub test_net_http_ok: bool,
    pub test_net_msg: &'static str,
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
            wallpaper: 0,
            wall_color: 0,
            saved_window_states: [None; 11],
            dhcp_enabled: true,
            static_ip: [192, 168, 1, 100],
            static_ip_octet: 0,
            static_ip_edit: false,
            static_ip_value: 0,
            static_ip_digits: 0,
            vfs_mounted: true,
            kernel_debug_log: true,
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
            test_net_tested: false,
            test_net_ok: false,
            test_net_ping_ok: false,
            test_net_http_ok: false,
            test_net_msg: "",
            test_mouse_x: 0,
            test_mouse_y: 0,
            test_mouse_over: false,
            test_mouse_buttons: 0,
            test_wheel: 0,
            test_trail: [0u32; 96],
            test_trail_len: 0,
            test_trail_head: 0,
            status_msg: "",
        }
    }

    pub fn load_from_vfs(&mut self, vfs: &VirtualFileSystem) {
        let data = match vfs.read_file("settings.cfg") {
            Some(d) => d,
            None => return,
        };
        let text = match core::str::from_utf8(data) {
            Ok(s) => s,
            Err(_) => return,
        };
        for item in text.split(';') {
            let mut p = item.split('=');
            let key = p.next().unwrap_or("");
            let val = p.next().unwrap_or("");
            match key {
                "volume" => {
                    if let Ok(v) = val.parse::<u8>() {
                        self.volume = v.min(100);
                    }
                }
                "mouse_speed" => {
                    if let Ok(v) = val.parse::<u8>() {
                        self.mouse_speed = v.clamp(1, 10);
                    }
                }
                "double_click" => {
                    if let Ok(v) = val.parse::<u16>() {
                        self.double_click_ms = v.clamp(150, 600);
                    }
                }
                "reverse_scroll" => self.reverse_scroll = val == "1",
                "muted" => self.is_muted = val == "1",
                "night" => self.night_light = val == "1",
                "night_level" => {
                    if let Ok(v) = val.parse::<u8>() {
                        self.night_light_level = v.min(100);
                    }
                }
                "dhcp" => self.dhcp_enabled = val != "0",
                "static_ip" => {
                    let mut parts = val.split('.');
                    let mut address = [0u8; 4];
                    let mut valid = true;
                    for octet in &mut address {
                        match parts.next().and_then(|s| s.parse::<u8>().ok()) {
                            Some(v) => *octet = v,
                            None => valid = false,
                        }
                    }
                    if valid && parts.next().is_none() {
                        self.static_ip = address;
                    }
                }
                "keyboard" => {
                    self.keyboard_layout = if val == "en" {
                        KeyboardLayout::EnglishUS
                    } else {
                        KeyboardLayout::TurkishQ
                    }
                }
                "language" => {
                    self.language = if val == "en" {
                        UiLanguage::English
                    } else {
                        UiLanguage::Turkish
                    };
                    crate::gui::set_ui_language_english(self.language.is_english());
                }
                "zoom" => {
                    self.zoom = match val {
                        "125" => 125,
                        "150" => 150,
                        _ => 100,
                    };
                    crate::gui::set_font_zoom(self.zoom);
                }
                "region" => {
                    if let Ok(v) = val.parse::<u8>() {
                        self.region = v.min(2);
                    }
                }
                "res" => {
                    let mut r = val.split('x');
                    if let (Some(a), Some(b)) = (r.next(), r.next()) {
                        if let (Ok(w), Ok(h)) = (a.parse(), b.parse()) {
                            self.screen_res = (w, h);
                        }
                    }
                }
                "theme" => {
                    self.theme = match val {
                        "daylight" => ThemeId::Daylight,
                        "nord" => ThemeId::Nord,
                        "terminal" => ThemeId::TerminalGreen,
                        "sunset" => ThemeId::Sunset,
                        _ => ThemeId::VelvetDark,
                    };
                    theme::set_theme(self.theme);
                }
                "wallpaper" => {
                    if let Ok(v) = val.parse::<u8>() {
                        self.wallpaper = v.min(9);
                        crate::gui::wallpaper::apply(self.wallpaper, self.wall_color);
                    }
                }
                "wallcolor" => {
                    if let Ok(v) = u32::from_str_radix(val.trim_start_matches("0x"), 16) {
                        self.wall_color = v & 0xFFFFFF;
                        crate::gui::wallpaper::apply(self.wallpaper, self.wall_color);
                    }
                }
                _ => {}
            }
        }
        crate::gui::wallpaper::apply(self.wallpaper, self.wall_color);
        self.status_msg = crate::gui::ui_text(
            "Ayarlar kalıcı depolamadan yüklendi.",
            "Settings loaded from persistent storage.",
        );
    }

    // -----------------------------------------------------------------
    // Geometri yardımcıları (main.rs tıklama testi de bunları kullanır)
    // -----------------------------------------------------------------
    pub fn tab_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (
            win_x + TAB_X,
            win_y + sp(36) + i * sp(30),
            sp(TAB_W),
            sp(TAB_H),
        )
    }
    // Sağ içerik sütununun genişliği (metinlerin butonlarla çakışmaması için).
    #[allow(dead_code)]
    const CONTENT_W: usize = 400;
    #[allow(dead_code)]
    const BTN_X: usize = CONTENT_X + 280; // sağdaki kontrol butonlarının sol kenarı

    // --- Tab 0 (Temel) Geometri ---
    pub fn kbd_btn_rect(win_x: usize, win_y: usize, _win_w: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + 270,
            win_y + sp(UI_TOP_Y + UI_ROW_STEP),
            sp(110),
            sp(UI_BTN_H),
        )
    }
    pub fn language_btn_rect(
        win_x: usize,
        win_y: usize,
        _win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + 270,
            win_y + sp(UI_TOP_Y + 2 * UI_ROW_STEP),
            sp(110),
            sp(UI_BTN_H),
        )
    }
    pub fn region_btn_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (
            x + CONTENT_X + 170,
            y + sp(UI_TOP_Y + 3 * UI_ROW_STEP),
            sp(210),
            sp(UI_BTN_H),
        )
    }
    pub fn volume_track_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X,
            win_y + sp(UI_TOP_Y + 5 * UI_ROW_STEP) + sp(6),
            sp(200),
            sp(10),
        )
    }
    pub fn mute_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + 270,
            win_y + sp(UI_TOP_Y + 5 * UI_ROW_STEP),
            sp(110),
            sp(UI_BTN_H),
        )
    }
    pub fn mouse_speed_track_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (
            x + CONTENT_X,
            y + sp(UI_TOP_Y + 7 * UI_ROW_STEP) + sp(6),
            sp(200),
            sp(10),
        )
    }
    pub fn double_click_dec_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (
            x + CONTENT_X + 170,
            y + sp(UI_TOP_Y + 8 * UI_ROW_STEP),
            sp(32),
            sp(UI_BTN_H),
        )
    }
    pub fn double_click_inc_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (
            x + CONTENT_X + 290,
            y + sp(UI_TOP_Y + 8 * UI_ROW_STEP),
            sp(32),
            sp(UI_BTN_H),
        )
    }
    pub fn reverse_scroll_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (
            x + CONTENT_X + 170,
            y + sp(UI_TOP_Y + 9 * UI_ROW_STEP),
            sp(140),
            sp(UI_BTN_H),
        )
    }

    // --- Tab 1 (Ekran) Geometri: tum ekran/gorunum ayarlari burada toplanir ---
    pub fn theme_btn_rect(
        win_x: usize,
        win_y: usize,
        _win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + 270,
            win_y + sp(UI_TOP_Y),
            sp(110),
            sp(UI_BTN_H),
        )
    }
    pub fn theme_swatch_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + 170 + i * sp(28),
            win_y + sp(UI_TOP_Y + UI_ROW_STEP),
            sp(24),
            sp(20),
        )
    }
    pub fn theme_swatch_count() -> usize {
        ThemeId::ALL.len()
    }
    pub fn resolution_btn_rect(
        win_x: usize,
        win_y: usize,
        i: usize,
    ) -> (usize, usize, usize, usize) {
        let col = i % 4;
        let row = i / 4;
        (
            win_x + CONTENT_X + col * sp(112),
            win_y + sp(UI_TOP_Y + 3 * UI_ROW_STEP) + row * sp(28),
            sp(108),
            sp(UI_BTN_H),
        )
    }
    pub fn resolution_name(i: usize) -> &'static str {
        [
            "800x600",
            "1024x768",
            "1280x720",
            "1920x1080",
            "1920x1200",
            "2560x1440",
            "2560x1600",
        ][i.min(6)]
    }
    pub fn zoom_btn_rect(x: usize, y: usize, i: usize) -> (usize, usize, usize, usize) {
        (
            x + CONTENT_X + 170 + i * sp(76),
            y + sp(UI_TOP_Y + 5 * UI_ROW_STEP),
            sp(70),
            sp(UI_BTN_H),
        )
    }
    pub fn night_light_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (
            x + CONTENT_X + 270,
            y + sp(UI_TOP_Y + 6 * UI_ROW_STEP),
            sp(110),
            sp(UI_BTN_H),
        )
    }
    pub fn night_light_track_rect(x: usize, y: usize) -> (usize, usize, usize, usize) {
        (
            x + CONTENT_X,
            y + sp(UI_TOP_Y + 7 * UI_ROW_STEP) + sp(6),
            sp(200),
            sp(10),
        )
    }
    pub fn wallpaper_btn_rect(x: usize, y: usize, i: usize) -> (usize, usize, usize, usize) {
        let col = i % 5;
        let row = i / 5;
        (
            x + CONTENT_X + col * sp(82),
            y + sp(UI_TOP_Y + 9 * UI_ROW_STEP) + row * sp(38),
            sp(74),
            sp(30),
        )
    }
    pub fn wallpaper_count() -> usize {
        crate::gui::wallpaper::COUNT
    }
    pub fn region_offset(&self) -> i8 {
        match self.region {
            1 => 0,
            2 => -5,
            _ => 3,
        }
    }
    pub fn region_name(&self, english: bool) -> &'static str {
        match (self.region, english) {
            (1, true) => "UTC",
            (1, false) => "UTC",
            (2, true) => "US Eastern (UTC-5)",
            (2, false) => "ABD Doğu (UTC-5)",
            (_, true) => "Türkiye (UTC+3)",
            (_, false) => "Türkiye (UTC+3)",
        }
    }

    // --- Tab 2 (Ağ) Geometri ---
    pub fn dhcp_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X,
            win_y + sp(UI_TOP_Y + 5 * UI_ROW_STEP),
            sp(150),
            sp(24),
        )
    }

    pub fn static_ip_octet_rect(
        win_x: usize,
        win_y: usize,
        octet: usize,
    ) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + sp(34 + octet * 58),
            win_y + sp(UI_TOP_Y + 4 * UI_ROW_STEP - 2),
            sp(48),
            sp(24),
        )
    }

    pub fn static_ip_input_char(&mut self, ch: char) {
        if !self.static_ip_edit || !ch.is_ascii_digit() || self.static_ip_digits >= 3 {
            return;
        }
        if self.static_ip_digits == 0 {
            self.static_ip_value = 0;
        }
        self.static_ip_value =
            (self.static_ip_value * 10 + ch.to_digit(10).unwrap_or(0) as u16).min(255);
        self.static_ip_digits = self.static_ip_digits.saturating_add(1).min(3);
        self.static_ip[self.static_ip_octet] = self.static_ip_value as u8;
    }

    pub fn static_ip_backspace(&mut self) {
        if !self.static_ip_edit {
            return;
        }
        self.static_ip_value /= 10;
        self.static_ip_digits = self.static_ip_digits.saturating_sub(1);
        self.static_ip[self.static_ip_octet] = self.static_ip_value as u8;
    }

    pub fn static_ip_next_octet(&mut self) {
        self.static_ip_octet = (self.static_ip_octet + 1) % 4;
        self.static_ip_value = 0;
        self.static_ip_digits = 0;
    }

    // --- Ortak Butonlar ---
    pub fn save_btn_rect(win_x: usize, win_y: usize, win_h: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X,
            win_y + win_h.saturating_sub(sp(54)),
            sp(96),
            sp(26),
        )
    }
    pub fn reset_btn_rect(
        win_x: usize,
        win_y: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + 112,
            win_y + win_h.saturating_sub(sp(54)),
            sp(96),
            sp(26),
        )
    }

    // -----------------------------------------------------------------
    // Testler sekmesi (4) geometri yardımcıları
    // -----------------------------------------------------------------
    /// Fare test alanı: imleç içine girdiğinde konum/iz ve düğmeler sınanır.
    pub fn test_mouse_pad_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X,
            win_y + sp(UI_TOP_Y + UI_ROW_STEP),
            sp(260),
            sp(84),
        )
    }
    pub fn test_ram_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X,
            win_y + sp(UI_TOP_Y + 4 * UI_ROW_STEP),
            sp(100),
            sp(UI_BTN_H),
        )
    }
    pub fn test_disk_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X,
            win_y + sp(UI_TOP_Y + 5 * UI_ROW_STEP + 16),
            sp(100),
            sp(UI_BTN_H),
        )
    }
    pub fn test_audio_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X,
            win_y + sp(UI_TOP_Y + 6 * UI_ROW_STEP + 16),
            sp(100),
            sp(UI_BTN_H),
        )
    }
    pub fn test_net_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X,
            win_y + sp(UI_TOP_Y + 7 * UI_ROW_STEP + 8),
            sp(110),
            sp(UI_BTN_H),
        )
    }
    pub fn test_ping_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + sp(118),
            win_y + sp(UI_TOP_Y + 7 * UI_ROW_STEP + 8),
            sp(90),
            sp(UI_BTN_H),
        )
    }
    pub fn test_http_btn_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (
            win_x + CONTENT_X + sp(216),
            win_y + sp(UI_TOP_Y + 7 * UI_ROW_STEP + 8),
            sp(90),
            sp(UI_BTN_H),
        )
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
        self.test_net_tested = false;
        self.test_net_ok = false;
        self.test_net_ping_ok = false;
        self.test_net_http_ok = false;
        self.test_net_msg = "";
        self.test_mouse_x = 0;
        self.test_mouse_y = 0;
        self.test_mouse_over = false;
        self.test_mouse_buttons = 0;
        self.test_wheel = 0;
        self.test_trail = [0u32; 96];
        self.test_trail_len = 0;
        self.test_trail_head = 0;
        self.status_msg = crate::gui::ui_text(
            "Tüm ayarlar varsayılana döndürüldü!",
            "All settings restored to defaults!",
        );
    }

    pub fn run_audio_test(&mut self, audio_dev: &mut crate::drivers::audio::AudioDevice) {
        self.test_audio_tested = true;
        let (msg, ok) = audio_dev.test_audio_subsystem();
        self.test_audio_ok = ok;
        self.test_audio_msg = msg;
        self.status_msg = msg;
    }

    /// Ağ alt sistemi testi: Link durumu, Gateway ARP ve Ping doğrulaması yapar.
    pub fn run_network_test(&mut self) {
        self.test_net_tested = true;
        if !crate::drivers::e1000::is_ready() {
            self.test_net_ok = false;
            self.test_net_ping_ok = false;
            self.test_net_http_ok = false;
            self.test_net_msg =
                crate::gui::ui_text("e1000 NIC bulunamadı!", "e1000 NIC not found!");
            self.status_msg = self.test_net_msg;
            return;
        }

        if !crate::drivers::e1000::link_up() {
            self.test_net_ok = false;
            self.test_net_ping_ok = false;
            self.test_net_http_ok = false;
            self.test_net_msg =
                crate::gui::ui_text("Link Down (Kablo/ağ yok)", "Link Down (No cable/network)");
            self.status_msg = self.test_net_msg;
            return;
        }

        let passed = crate::drivers::netstack::run_network_self_test();
        self.test_net_ok = passed;
        self.test_net_ping_ok = passed;
        self.test_net_msg = if passed {
            crate::gui::ui_text(
                "Ağ kartı ve TCP/IP hazır (10.0.2.15)",
                "NIC and TCP/IP ready (10.0.2.15)",
            )
        } else {
            crate::gui::ui_text("Ağ testi başarısız!", "Network test failed!")
        };
        self.status_msg = self.test_net_msg;
    }

    /// Yalnızca Gateway (10.0.2.2) ICMP Ping testi gönderir.
    pub fn run_ping_test(&mut self) {
        self.test_net_tested = true;
        if !crate::drivers::e1000::is_ready() || !crate::drivers::e1000::link_up() {
            self.test_net_ok = false;
            self.test_net_ping_ok = false;
            self.test_net_msg = crate::gui::ui_text(
                "Ağ bağlantısı yok (Link Down)",
                "No network link (Link Down)",
            );
            self.status_msg = self.test_net_msg;
            return;
        }

        crate::drivers::netstack::clear_ping_reply();
        let gw = crate::drivers::netstack::gateway_ip();
        let sent = crate::drivers::netstack::ping(gw);
        if !sent {
            self.test_net_ok = false;
            self.test_net_ping_ok = false;
            self.test_net_msg =
                crate::gui::ui_text("Ping iletim hatası (TX)", "Ping transmit failure (TX)");
            self.status_msg = self.test_net_msg;
            return;
        }

        let mut replied = false;
        for _ in 0..5_000 {
            crate::drivers::e1000::poll();
            if crate::drivers::netstack::has_ping_reply() {
                replied = true;
                break;
            }
            core::hint::spin_loop();
        }

        self.test_net_ping_ok = replied;
        self.test_net_ok = replied;
        self.test_net_msg = if replied {
            crate::gui::ui_text(
                "Ping 10.0.2.2: Yanıt alındı (OK)",
                "Ping 10.0.2.2: Echo Reply OK",
            )
        } else {
            crate::gui::ui_text(
                "Ping 10.0.2.2: Zaman aşımı (Timeout)",
                "Ping 10.0.2.2: Request Timeout",
            )
        };
        self.status_msg = self.test_net_msg;
    }

    /// Gateway HTTP (10.0.2.2:80) SYN handshake testi.
    pub fn run_http_test(&mut self) {
        self.test_net_tested = true;
        if !crate::drivers::e1000::is_ready() || !crate::drivers::e1000::link_up() {
            self.test_net_ok = false;
            self.test_net_http_ok = false;
            self.test_net_msg = crate::gui::ui_text("Ağ bağlantısı yok", "No network link");
            self.status_msg = self.test_net_msg;
            return;
        }

        let gw = crate::drivers::netstack::gateway_ip();
        let init = crate::drivers::netstack::tcp_begin(gw, 80, 49210);
        if !init {
            self.test_net_http_ok = false;
            self.test_net_msg = crate::gui::ui_text("TCP SYN iletilemedi", "TCP SYN send failed");
            self.status_msg = self.test_net_msg;
            return;
        }

        let mut connected = false;
        for _ in 0..6_000 {
            crate::drivers::e1000::poll();
            if crate::drivers::netstack::tcp_connected() {
                connected = true;
                break;
            }
            core::hint::spin_loop();
        }

        if connected {
            crate::drivers::netstack::tcp_close();
            self.test_net_http_ok = true;
            self.test_net_ok = true;
            self.test_net_msg = crate::gui::ui_text(
                "HTTP 10.0.2.2:80: Bağlantı kuruldu (OK)",
                "HTTP 10.0.2.2:80: Connected (OK)",
            );
        } else {
            self.test_net_http_ok = false;
            self.test_net_msg = crate::gui::ui_text(
                "HTTP 10.0.2.2:80: Yanıt yok (Port kapalı)",
                "HTTP 10.0.2.2:80: No response (Closed)",
            );
        }
        self.status_msg = self.test_net_msg;
    }

    /// Ayarları aktif sistem bileşenlerine uygular.
    pub fn apply_runtime(
        &self,
        keyboard: &mut crate::drivers::keyboard::KeyboardState,
        compositor: &mut MinGuiCompositor,
        now: &mut DateTime,
    ) {
        crate::gui::set_font_zoom(self.zoom);
        crate::gui::theme::set_theme(self.theme);
        crate::gui::wallpaper::apply(self.wallpaper, self.wall_color);
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
            self.status_msg = crate::gui::ui_text(
                "RAM testi: yeterli bellek yok.",
                "RAM test: not enough memory.",
            );
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
            if *b != expect {
                errors = errors.saturating_add(1);
            }
        }
        for b in v.iter_mut() {
            *b = !*b;
        }
        for (i, b) in v.iter().enumerate() {
            let expect = ((i as u8).wrapping_mul(31).wrapping_add(7) ^ 0xA5) ^ 0xFF;
            if *b != expect {
                errors = errors.saturating_add(1);
            }
        }
        let ms = crate::monotonic_ms()
            .saturating_sub(start)
            .min(u32::MAX as u64) as u32;
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
            let Some(dev) = crate::drivers::ata::device(drive) else {
                continue;
            };
            if dev.kind == crate::drivers::ata::DeviceKind::None || dev.sectors == 0 {
                continue;
            }
            found = true;
            let max = dev.sectors.min(MAX_SECTORS) as u32;
            let mut lba = 0u32;
            while lba < max {
                let want = ((max - lba).min(CHUNK as u32)) as usize;
                let got =
                    crate::drivers::ata::read_sectors(drive, lba, want, &mut buf[..want * 512]);
                if got == 0 {
                    ok = false;
                    break;
                }
                total = total.saturating_add(got as u32);
                if got < want {
                    break;
                }
                lba += got as u32;
            }
            if !ok {
                break;
            }
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
        &mut self,
        mx: usize,
        my: usize,
        left: bool,
        right: bool,
        middle: bool,
        win_x: usize,
        win_y: usize,
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
        if left {
            b |= 1;
        }
        if right {
            b |= 2;
        }
        if middle {
            b |= 4;
        }
        self.test_mouse_buttons = b;
    }

    /// Tekerlek testi sayacını büyütür (main döngüsünden çağrılır).
    pub fn note_scroll_test(&mut self, amount: u32) {
        self.test_wheel = self.test_wheel.wrapping_add(amount);
    }

    pub fn cycle_theme(&mut self) {
        self.theme = theme::cycle();
        self.status_msg = crate::gui::ui_text(
            "Tema değiştirildi: Sarman temayı sever!",
            "Theme changed: Sarman likes this one!",
        );
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
        let mut cfg = [0u8; 448];
        let mut n = 0usize;
        fn put(d: &mut [u8], n: &mut usize, s: &[u8]) {
            let l = core::cmp::min(s.len(), d.len() - *n);
            d[*n..*n + l].copy_from_slice(&s[..l]);
            *n += l;
        }
        fn put_num(d: &mut [u8], n: &mut usize, value: u8) {
            if value >= 100 {
                put(
                    d,
                    n,
                    &[b'1', b'0' + (value - 100) / 10, b'0' + (value - 100) % 10],
                );
            } else if value >= 10 {
                put(d, n, &[b'0' + value / 10, b'0' + value % 10]);
            } else {
                put(d, n, &[b'0' + value]);
            }
        }
        let put_u16 = |d: &mut [u8], n: &mut usize, value: usize| {
            let value = value.min(9999);
            let mut digits = [0u8; 4];
            let mut len = 0;
            let mut v = value;
            loop {
                digits[3 - len] = b'0' + (v % 10) as u8;
                len += 1;
                v /= 10;
                if v == 0 || len == 4 {
                    break;
                }
            }
            put(d, n, &digits[4 - len..]);
        };
        put(&mut cfg, &mut n, b"volume=");
        put_num(&mut cfg, &mut n, self.volume);
        put(&mut cfg, &mut n, b";mouse_speed=");
        put_num(&mut cfg, &mut n, self.mouse_speed);
        put(&mut cfg, &mut n, b";double_click=");
        put_u16(&mut cfg, &mut n, self.double_click_ms as usize);
        put(&mut cfg, &mut n, b";reverse_scroll=");
        put(
            &mut cfg,
            &mut n,
            if self.reverse_scroll { b"1" } else { b"0" },
        );
        put(&mut cfg, &mut n, b";muted=");
        put(&mut cfg, &mut n, if self.is_muted { b"1" } else { b"0" });
        put(&mut cfg, &mut n, b";res=");
        put_u16(&mut cfg, &mut n, self.screen_res.0);
        put(&mut cfg, &mut n, b"x");
        put_u16(&mut cfg, &mut n, self.screen_res.1);
        put(&mut cfg, &mut n, b";night=");
        put(&mut cfg, &mut n, if self.night_light { b"1" } else { b"0" });
        put(&mut cfg, &mut n, b";night_level=");
        put_num(&mut cfg, &mut n, self.night_light_level);
        put(&mut cfg, &mut n, b";keyboard=");
        put(
            &mut cfg,
            &mut n,
            if self.keyboard_layout == KeyboardLayout::EnglishUS {
                b"en"
            } else {
                b"tr"
            },
        );
        put(&mut cfg, &mut n, b";language=");
        put(
            &mut cfg,
            &mut n,
            if self.language.is_english() {
                b"en"
            } else {
                b"tr"
            },
        );
        put(&mut cfg, &mut n, b";zoom=");
        put(
            &mut cfg,
            &mut n,
            match self.zoom {
                125 => b"125" as &[u8],
                150 => b"150",
                _ => b"100",
            },
        );
        put(&mut cfg, &mut n, b";region=");
        put(&mut cfg, &mut n, &[b'0' + self.region.min(2)]);
        put(&mut cfg, &mut n, b";dhcp=");
        put(
            &mut cfg,
            &mut n,
            if self.dhcp_enabled { b"1" } else { b"0" },
        );
        put(&mut cfg, &mut n, b";static_ip=");
        for (i, octet) in self.static_ip.iter().enumerate() {
            if i > 0 {
                put(&mut cfg, &mut n, b".");
            }
            put_num(&mut cfg, &mut n, *octet);
        }
        put(&mut cfg, &mut n, b";theme=");
        put(
            &mut cfg,
            &mut n,
            match self.theme {
                ThemeId::VelvetDark => b"velvet" as &[u8],
                ThemeId::Daylight => b"daylight",
                ThemeId::Nord => b"nord",
                ThemeId::TerminalGreen => b"terminal",
                ThemeId::Sunset => b"sunset",
            },
        );
        put(&mut cfg, &mut n, b";wallpaper=");
        put_num(&mut cfg, &mut n, self.wallpaper);
        put(&mut cfg, &mut n, b";wallcolor=");
        {
            let v = self.wall_color & 0xFFFFFF;
            const HX: &[u8; 16] = b"0123456789ABCDEF";
            let hex = [
                HX[((v >> 20) & 0xF) as usize],
                HX[((v >> 16) & 0xF) as usize],
                HX[((v >> 12) & 0xF) as usize],
                HX[((v >> 8) & 0xF) as usize],
                HX[((v >> 4) & 0xF) as usize],
                HX[(v & 0xF) as usize],
            ];
            put(&mut cfg, &mut n, &hex);
        }
        vfs.write_file("settings.cfg", &cfg[..n]);
        // Diske/FAT32 bolumune de fiziksel olarak kaydet (kalici ayarlar icin)
        crate::fs::diskfs::save_file_by_name("settings.cfg", &cfg[..n]);
    }

    pub fn save_all(&mut self, compositor: &MinGuiCompositor, vfs: &mut VirtualFileSystem) {
        for (i, opt_win) in compositor
            .windows
            .iter()
            .take(self.saved_window_states.len())
            .enumerate()
        {
            if let Some(ref win) = opt_win {
                self.saved_window_states[i] = Some(win.get_state());
            }
        }
        self.write_config(vfs);
        self.status_msg = crate::gui::ui_text(
            "Ayarlar ve pencere düzeni diske kalıcı olarak kaydedildi.",
            "Settings and window layout saved to disk.",
        );
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
        // Sekme 0 (Temel) 9 kontrol: 5 sekme + kbd + dil + bolge + sustur + kaydet + sifirla
        // Sekme 1 (Ekran) 19 kontrol: 5 sekme + tema + 5 swatch + 4 cozunurluk + 3 zoom + mavi isik + 6 desen -> sadeleştirilmiş
        // Sekme 2 (Ag) 7 kontrol, digerleri 7 kontrol
        let total = match self.active_tab {
            0 => 9,
            1 => 32,
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
        fb.draw_rect(
            win_x + sp(6),
            win_y + sp(32),
            sp(TAB_W + 8),
            win_h.saturating_sub(sp(62)),
            Color::TITLE_BAR_INACTIVE,
        );
        let tabs = if self.language.is_english() {
            ["Basic", "Display", "Network", "Devices", "Tests"]
        } else {
            ["Temel", "Ekran", "Ağ", "Aygıtlar", "Testler"]
        };
        for (i, t) in tabs.iter().enumerate() {
            let r = Self::tab_rect(win_x, win_y, i);
            let is_cur = i == self.active_tab;
            let bg = if is_cur {
                Color::TITLE_BAR
            } else {
                Color::TITLE_BAR_INACTIVE
            };
            fb.draw_rect(r.0, r.1, r.2, r.3, bg);
            if is_cur {
                fb.draw_rect(r.0, r.1, 3, r.3, Color::ACCENT_BLUE);
            }
            if self.focused_control == i {
                fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::ACCENT_YELLOW);
            }
            let col = if is_cur {
                MinGuiCompositor::text_on(bg)
            } else {
                Color::TEXT_MUTED
            };
            MinGuiCompositor::draw_string_clipped(
                fb,
                r.0 + 8,
                r.1 + 5,
                t,
                col,
                (
                    r.0 + 4,
                    r.1 + 2,
                    r.2.saturating_sub(8),
                    r.3.saturating_sub(4),
                ),
            );
        }

        // Sağ İçerik Alanı
        match self.active_tab {
            0 => {
                // --- minOs Sistem ve Sürüm Bilgisi (Satır 0) ---
                let ver_str = if self.language.is_english() {
                    "minOs Sarman Edition v2.0.0 (x86_64)"
                } else {
                    "minOs Sarman Sürümü v2.0.0 (x86_64)"
                };
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + UI_LABEL_OFFSET),
                    ver_str,
                    Color::ACCENT_GREEN,
                );

                // --- Klavye / Dil (Satır 1 & 2) ---
                let kbd_str = match kbd_layout {
                    KeyboardLayout::TurkishQ => {
                        if self.language.is_english() {
                            "Keyboard: Turkish (TR-Q)"
                        } else {
                            "Klavye: Türkçe (TR-Q)"
                        }
                    }
                    KeyboardLayout::EnglishUS => "Keyboard: English (EN-US)",
                };
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + UI_ROW_STEP + UI_LABEL_OFFSET),
                    kbd_str,
                    Color::ACCENT_PINK,
                    (
                        win_x + CONTENT_X,
                        win_y + sp(UI_TOP_Y + UI_ROW_STEP),
                        sp(260),
                        sp(UI_BTN_H),
                    ),
                );
                let kbd_r = Self::kbd_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(
                    fb,
                    kbd_r,
                    Color::ACCENT_PINK,
                    if self.language.is_english() {
                        "Layout"
                    } else {
                        "Düzen"
                    },
                    Color::DARK_BG,
                    false,
                );
                if self.focused_control == 5 {
                    fb.draw_rect_outline(kbd_r.0, kbd_r.1, kbd_r.2, kbd_r.3, Color::ACCENT_YELLOW);
                }

                MinGuiCompositor::draw_string_clipped(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 2 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    if self.language.is_english() {
                        "Interface language:"
                    } else {
                        "Arayüz dili:"
                    },
                    Color::ACCENT_PINK,
                    (
                        win_x + CONTENT_X,
                        win_y + sp(UI_TOP_Y + 2 * UI_ROW_STEP),
                        sp(260),
                        sp(UI_BTN_H),
                    ),
                );
                let lang_r = Self::language_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(
                    fb,
                    lang_r,
                    Color::ACCENT_PINK,
                    if self.language.is_english() {
                        "Turkish"
                    } else {
                        "English"
                    },
                    Color::DARK_BG,
                    false,
                );
                if self.focused_control == 6 {
                    fb.draw_rect_outline(
                        lang_r.0,
                        lang_r.1,
                        lang_r.2,
                        lang_r.3,
                        Color::ACCENT_YELLOW,
                    );
                }

                // --- Saat Dilimi (Satır 3) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 3 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    if self.language.is_english() {
                        "Time Zone:"
                    } else {
                        "Bölge / Saat:"
                    },
                    Color::TEXT_MAIN,
                );
                let reg_r = Self::region_btn_rect(win_x, win_y);
                MinGuiCompositor::draw_button(
                    fb,
                    reg_r,
                    Color::ACCENT_BLUE,
                    self.region_name(self.language.is_english()),
                    Color::TEXT_MAIN,
                    false,
                );

                // --- Canlı Saat & Tarih (Satır 4) ---
                let mut clock = TextBuf::new();
                clock.push_str(if self.language.is_english() {
                    "Time: "
                } else {
                    "Saat: "
                });
                clock.push_u2(now.hour as u32);
                clock.push_byte(b':');
                clock.push_u2(now.minute as u32);
                clock.push_byte(b':');
                clock.push_u2(now.second as u32);
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 4 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    clock.as_str(),
                    Color::ACCENT_GREEN,
                );

                let mut date = TextBuf::new();
                date.push_str(if self.language.is_english() {
                    "Date: "
                } else {
                    "Tarih: "
                });
                date.push_u2(now.day as u32);
                date.push_byte(b' ');
                date.push_str(if self.language.is_english() {
                    DateTime::month_name_en(now.month)
                } else {
                    DateTime::month_name(now.month)
                });
                date.push_byte(b' ');
                date.push_u4(now.year as u32);
                date.push_str(", ");
                let weekday = DateTime::weekday(now.year, now.month, now.day);
                date.push_str(if self.language.is_english() {
                    DateTime::weekday_name_en(weekday)
                } else {
                    DateTime::weekday_name(weekday)
                });
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X + 170,
                    win_y + sp(UI_TOP_Y + 4 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    date.as_str(),
                    Color::TEXT_MAIN,
                );

                // --- Ses (Satır 5) ---
                let mut vol = TextBuf::new();
                vol.push_str(if self.language.is_english() {
                    "Volume: %"
                } else {
                    "Ses Düzeyi: %"
                });
                vol.push_usize(self.volume as usize);
                if self.is_muted {
                    vol.push_str(if self.language.is_english() {
                        " (MUTED)"
                    } else {
                        " (SUSTURULDU)"
                    });
                }
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 5 * UI_ROW_STEP - 14),
                    vol.as_str(),
                    Color::TEXT_MAIN,
                );

                let track = Self::volume_track_rect(win_x, win_y);
                fb.draw_rect(
                    track.0,
                    track.1,
                    track.2,
                    track.3,
                    Color::TITLE_BAR_INACTIVE,
                );
                let fill_w = (track.2 * self.volume as usize) / 100;
                let fill_col = if self.is_muted {
                    Color::TEXT_MUTED
                } else {
                    Color::ACCENT_GREEN
                };
                fb.draw_rect(track.0, track.1, fill_w, track.3, fill_col);
                fb.draw_rect(
                    track.0 + fill_w.saturating_sub(4),
                    track.1 - 4,
                    8,
                    track.3 + 8,
                    Color::TEXT_MAIN,
                );

                let mute_r = Self::mute_btn_rect(win_x, win_y);
                let mute_bg = if self.is_muted {
                    Color::ACCENT_RED
                } else {
                    Color::TITLE_BAR
                };
                let mute_lbl = match (self.is_muted, self.language.is_english()) {
                    (true, true) => "Unmute",
                    (false, true) => "Mute",
                    (true, false) => "Sesi Aç",
                    (false, false) => "Sustur",
                };
                MinGuiCompositor::draw_button(
                    fb,
                    mute_r,
                    mute_bg,
                    mute_lbl,
                    Color::TEXT_MAIN,
                    false,
                );
                if self.focused_control == 8 {
                    fb.draw_rect_outline(
                        mute_r.0,
                        mute_r.1,
                        mute_r.2,
                        mute_r.3,
                        Color::ACCENT_YELLOW,
                    );
                }

                // --- Fare Ayarları Başlığı (Satır 6) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 6 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    crate::gui::ui_text("Fare Ayarları", "Mouse Settings"),
                    Color::ACCENT_PINK,
                );

                // --- İşaretçi Hızı (Satır 7) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 7 * UI_ROW_STEP - 14),
                    crate::gui::ui_text("İşaretçi hızı:", "Pointer speed:"),
                    Color::TEXT_MAIN,
                );
                let speed_track = Self::mouse_speed_track_rect(win_x, win_y);
                fb.draw_rect(
                    speed_track.0,
                    speed_track.1,
                    speed_track.2,
                    speed_track.3,
                    Color::TITLE_BAR_INACTIVE,
                );
                let speed_fill = speed_track.2 * (self.mouse_speed.saturating_sub(1)) as usize / 9;
                fb.draw_rect(
                    speed_track.0,
                    speed_track.1,
                    speed_fill,
                    speed_track.3,
                    Color::ACCENT_BLUE,
                );
                fb.draw_rect(
                    speed_track.0 + speed_fill.saturating_sub(4),
                    speed_track.1.saturating_sub(4),
                    8,
                    speed_track.3 + 8,
                    Color::TEXT_MAIN,
                );
                let mut speed_label = TextBuf::new();
                speed_label.push_usize(self.mouse_speed as usize);
                MinGuiCompositor::draw_string(
                    fb,
                    speed_track.0 + speed_track.2 + 12,
                    speed_track.1 - 2,
                    speed_label.as_str(),
                    Color::TEXT_MAIN,
                );

                // --- Çift Tıklama (Satır 8) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 8 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    crate::gui::ui_text("Çift tıklama:", "Double-click:"),
                    Color::TEXT_MAIN,
                );
                let dec = Self::double_click_dec_rect(win_x, win_y);
                let inc = Self::double_click_inc_rect(win_x, win_y);
                MinGuiCompositor::draw_button(
                    fb,
                    dec,
                    Color::TITLE_BAR,
                    "-",
                    Color::TEXT_MAIN,
                    false,
                );
                MinGuiCompositor::draw_button(
                    fb,
                    inc,
                    Color::TITLE_BAR,
                    "+",
                    Color::TEXT_MAIN,
                    false,
                );
                let mut click_label = TextBuf::new();
                click_label.push_usize(self.double_click_ms as usize);
                click_label.push_str(" ms");
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X + 215,
                    win_y + sp(UI_TOP_Y + 8 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    click_label.as_str(),
                    Color::ACCENT_GREEN,
                );

                // --- Tekerlek Yönü (Satır 9) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 9 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    crate::gui::ui_text("Tekerlek yönü:", "Scroll direction:"),
                    Color::TEXT_MAIN,
                );
                let reverse = Self::reverse_scroll_rect(win_x, win_y);
                MinGuiCompositor::draw_button(
                    fb,
                    reverse,
                    if self.reverse_scroll {
                        Color::ACCENT_GREEN
                    } else {
                        Color::TITLE_BAR
                    },
                    match (self.reverse_scroll, self.language.is_english()) {
                        (true, true) => "Reversed",
                        (false, true) => "Normal",
                        (true, false) => "Ters",
                        (false, false) => "Normal",
                    },
                    Color::TEXT_MAIN,
                    false,
                );
            }
            1 => {
                // --- Ekran sekmesi: Tema (Satır 0 & 1) ---
                let mut theme_str = TextBuf::new();
                theme_str.push_str(if self.language.is_english() {
                    "Theme: "
                } else {
                    "Tema: "
                });
                theme_str.push_str(self.theme.name());
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + UI_LABEL_OFFSET),
                    theme_str.as_str(),
                    Color::TEXT_MAIN,
                    (
                        win_x + CONTENT_X,
                        win_y + sp(UI_TOP_Y),
                        sp(260),
                        sp(UI_BTN_H),
                    ),
                );
                let theme_r = Self::theme_btn_rect(win_x, win_y, win_w);
                MinGuiCompositor::draw_button(
                    fb,
                    theme_r,
                    Color::ACCENT_BLUE,
                    if self.language.is_english() {
                        "Change"
                    } else {
                        "Değiştir"
                    },
                    Color::DARK_BG,
                    false,
                );
                for (i, tid) in ThemeId::ALL.iter().enumerate() {
                    let r = Self::theme_swatch_rect(win_x, win_y, i);
                    let (bg, accent) = theme::preview_colors(*tid);
                    fb.draw_rect(r.0, r.1, r.2, r.3, bg);
                    fb.draw_rect(r.0 + 3, r.1 + 3, r.2 - 6, r.3 - 6, accent);
                    let border = if *tid == self.theme {
                        Color::ACCENT_GREEN
                    } else {
                        Color::BORDER
                    };
                    fb.draw_rect_outline(r.0, r.1, r.2, r.3, border);
                }

                // --- Ses Sürücüsü Bilgisi (Satır 2) ---
                let mut audio_status = TextBuf::new();
                audio_status.push_str(crate::gui::ui_text("Ses sürücüsü: ", "Audio driver: "));
                audio_status.push_str(audio_driver);
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 2 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    audio_status.as_str(),
                    Color::ACCENT_BLUE,
                );

                // --- Hedef Çözünürlük (Satır 3 & 4) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 3 * UI_ROW_STEP - 14),
                    if self.language.is_english() {
                        "Target resolution:"
                    } else {
                        "Hedef çözünürlük:"
                    },
                    Color::ACCENT_YELLOW,
                );
                for i in 0..7 {
                    let r = Self::resolution_btn_rect(win_x, win_y, i);
                    let active = self.screen_res
                        == match i {
                            0 => (800, 600),
                            1 => (1024, 768),
                            2 => (1280, 720),
                            3 => (1920, 1080),
                            4 => (1920, 1200),
                            5 => (2560, 1440),
                            6 => (2560, 1600),
                            _ => (1920, 1080),
                        };
                    MinGuiCompositor::draw_button(
                        fb,
                        r,
                        if active {
                            Color::ACCENT_GREEN
                        } else {
                            Color::TITLE_BAR
                        },
                        Self::resolution_name(i),
                        Color::TEXT_MAIN,
                        false,
                    );
                }

                // --- Yazı Boyutu (Satır 5) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 5 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    if self.language.is_english() {
                        "Font zoom:"
                    } else {
                        "Yazı boyutu:"
                    },
                    Color::TEXT_MAIN,
                );
                for (i, value) in [100u8, 125, 150].iter().enumerate() {
                    let label = match value {
                        125 => "125%",
                        150 => "150%",
                        _ => "100%",
                    };
                    let r = Self::zoom_btn_rect(win_x, win_y, i);
                    MinGuiCompositor::draw_button(
                        fb,
                        r,
                        if self.zoom == *value {
                            Color::ACCENT_GREEN
                        } else {
                            Color::TITLE_BAR
                        },
                        label,
                        Color::TEXT_MAIN,
                        false,
                    );
                }

                // --- Mavi Işık / Gece Işığı (Satır 6 & 7) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 6 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    match (self.night_light, self.language.is_english()) {
                        (true, true) => "Blue light: On",
                        (false, true) => "Blue light: Off",
                        (true, false) => "Mavi ışık: Açık",
                        (false, false) => "Mavi ışık: Kapalı",
                    },
                    Color::ACCENT_YELLOW,
                );
                let nl_btn = Self::night_light_rect(win_x, win_y);
                MinGuiCompositor::draw_button(
                    fb,
                    nl_btn,
                    Color::ACCENT_YELLOW,
                    match (self.night_light, self.language.is_english()) {
                        (true, true) => "Off",
                        (false, true) => "On",
                        (true, false) => "Kapat",
                        (false, false) => "Aç",
                    },
                    Color::DARK_BG,
                    false,
                );

                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 7 * UI_ROW_STEP - 14),
                    if self.language.is_english() {
                        "Filter strength:"
                    } else {
                        "Filtre şiddeti:"
                    },
                    Color::TEXT_MAIN,
                );
                let nl = Self::night_light_track_rect(win_x, win_y);
                fb.draw_rect(nl.0, nl.1, nl.2, nl.3, Color::TITLE_BAR_INACTIVE);
                let fill = nl.2 * self.night_light_level as usize / 100;
                fb.draw_rect(nl.0, nl.1, fill, nl.3, Color::ACCENT_YELLOW);
                fb.draw_rect(
                    nl.0 + fill.saturating_sub(4),
                    nl.1.saturating_sub(4),
                    8,
                    nl.3 + 8,
                    Color::TEXT_MAIN,
                );
                let mut level_text = TextBuf::new();
                level_text.push_usize(self.night_light_level as usize);
                level_text.push_byte(b'%');
                MinGuiCompositor::draw_string(
                    fb,
                    nl.0 + nl.2 + 12,
                    nl.1 - 2,
                    level_text.as_str(),
                    Color::TEXT_MAIN,
                );

                // --- Arka Plan / Duvar Kağıdı Seçimi (Satır 8 & 9) ---
                let mut wall_lbl = TextBuf::new();
                wall_lbl.push_str(if self.language.is_english() {
                    "Wallpaper: "
                } else {
                    "Duvar Kağıdı: "
                });
                wall_lbl.push_str(crate::gui::wallpaper::name(
                    self.wallpaper,
                    self.language.is_english(),
                ));
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 8 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    wall_lbl.as_str(),
                    Color::ACCENT_BLUE,
                );
                for i in 0..crate::gui::wallpaper::COUNT {
                    let r = Self::wallpaper_btn_rect(win_x, win_y, i);
                    if r.1 + r.3 > win_y + win_h {
                        continue;
                    }
                    crate::gui::wallpaper::draw_preview(
                        fb,
                        r.0 + 2,
                        r.1 + 2,
                        r.2 - 4,
                        r.3 - 4,
                        i as u8,
                    );
                    if i == self.wallpaper as usize {
                        fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::ACCENT_GREEN);
                        fb.draw_rect_outline(
                            r.0.saturating_sub(1),
                            r.1.saturating_sub(1),
                            r.2 + 2,
                            r.3 + 2,
                            Color::ACCENT_GREEN,
                        );
                    } else {
                        fb.draw_rect_outline(r.0, r.1, r.2, r.3, Color::BORDER);
                    }
                }
            }
            2 => {
                let mut driver = TextBuf::new();
                driver.push_str(crate::gui::ui_text("Ağ sürücüsü: ", "Network driver: "));
                driver.push_str(network_driver);
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + UI_LABEL_OFFSET),
                    driver.as_str(),
                    Color::ACCENT_BLUE,
                );

                let mut status = TextBuf::new();
                status.push_str(crate::gui::ui_text("Durum: ", "Status: "));
                status.push_str(network_status);
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + UI_ROW_STEP + UI_LABEL_OFFSET),
                    status.as_str(),
                    Color::ACCENT_GREEN,
                );

                MinGuiCompositor::draw_string_clipped(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 2 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    crate::gui::ui_text(
                        "Yığın: Ethernet / IPv4 / ARP / ICMP / UDP / TCP / DHCP / DNS",
                        "Stack: Ethernet / IPv4 / ARP / ICMP / UDP / TCP / DHCP / DNS",
                    ),
                    Color::TEXT_MUTED,
                    (
                        win_x + CONTENT_X,
                        win_y + sp(UI_TOP_Y + 2 * UI_ROW_STEP),
                        win_w.saturating_sub(CONTENT_X + 12),
                        sp(18),
                    ),
                );

                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 3 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    if self.dhcp_enabled {
                        crate::gui::ui_text("IP yöntemi: DHCP", "IP mode: DHCP")
                    } else {
                        crate::gui::ui_text("IP yöntemi: Statik", "IP mode: Static")
                    },
                    Color::ACCENT_YELLOW,
                );

                let ip_y = win_y + sp(UI_TOP_Y + 4 * UI_ROW_STEP - 2);
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    ip_y + sp(6),
                    "IP:",
                    Color::TEXT_MAIN,
                );
                for i in 0..4 {
                    let r = Self::static_ip_octet_rect(win_x, win_y, i);
                    fb.draw_rect_outline(
                        r.0,
                        r.1,
                        r.2,
                        r.3,
                        if self.static_ip_edit && self.static_ip_octet == i {
                            Color::ACCENT_YELLOW
                        } else {
                            Color::BORDER
                        },
                    );
                    let mut octet = TextBuf::new();
                    octet.push_usize(self.static_ip[i] as usize);
                    MinGuiCompositor::draw_string(
                        fb,
                        r.0 + (r
                            .2
                            .saturating_sub(MinGuiCompositor::text_width(octet.as_str(), 1)))
                            / 2,
                        r.1 + 6,
                        octet.as_str(),
                        Color::TEXT_MAIN,
                    );
                    if i < 3 {
                        MinGuiCompositor::draw_string(
                            fb,
                            r.0 + r.2 + sp(4),
                            r.1 + 6,
                            ".",
                            Color::TEXT_MUTED,
                        );
                    }
                }
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X + sp(294),
                    win_y + sp(UI_TOP_Y + 4 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    crate::gui::ui_text(
                        "Düzenlemek için bir oktete tıkla",
                        "Click an octet to edit",
                    ),
                    Color::TEXT_MUTED,
                );

                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 5 * UI_ROW_STEP + UI_LABEL_OFFSET),
                    "Gateway: 10.0.2.2    DNS: 10.0.2.3",
                    Color::TEXT_MAIN,
                );

                let dhcp_btn = Self::dhcp_btn_rect(win_x, win_y);
                MinGuiCompositor::draw_button(
                    fb,
                    dhcp_btn,
                    Color::ACCENT_BLUE,
                    if self.dhcp_enabled {
                        "Static IP"
                    } else {
                        "DHCP"
                    },
                    Color::DARK_BG,
                    false,
                );
                if self.focused_control == 4 {
                    fb.draw_rect_outline(
                        dhcp_btn.0,
                        dhcp_btn.1,
                        dhcp_btn.2,
                        dhcp_btn.3,
                        Color::ACCENT_YELLOW,
                    );
                }
                if self.static_ip_edit {
                    MinGuiCompositor::draw_string(
                        fb,
                        win_x + CONTENT_X,
                        dhcp_btn.1 + sp(28),
                        crate::gui::ui_text(
                            "Rakam gir, Tab ile sonraki oktet",
                            "Type digits, Tab for next octet",
                        ),
                        Color::TEXT_MUTED,
                    );
                }

                // --- Ağ Tanılama ve Testleri (Satır 6 & 7) ---
                MinGuiCompositor::draw_string(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + 6 * UI_ROW_STEP + UI_LABEL_OFFSET + 4),
                    crate::gui::ui_text(
                        "Ağ Testleri ve Bağlantı Sınama:",
                        "Network Diagnostics & Tests:",
                    ),
                    Color::ACCENT_YELLOW,
                );

                let net_btn = Self::test_net_btn_rect(win_x, win_y);
                let ping_btn = Self::test_ping_btn_rect(win_x, win_y);
                let http_btn = Self::test_http_btn_rect(win_x, win_y);

                MinGuiCompositor::draw_button(
                    fb,
                    net_btn,
                    Color::ACCENT_BLUE,
                    crate::gui::ui_text("Tümünü Sına", "Self-Test"),
                    Color::DARK_BG,
                    false,
                );
                MinGuiCompositor::draw_button(
                    fb,
                    ping_btn,
                    Color::ACCENT_BLUE,
                    "Ping GW",
                    Color::DARK_BG,
                    false,
                );
                MinGuiCompositor::draw_button(
                    fb,
                    http_btn,
                    Color::ACCENT_GREEN,
                    "HTTP GET",
                    Color::DARK_BG,
                    false,
                );

                let english = self.language.is_english();
                let mut net_line = DeviceLine::new();
                if self.test_net_tested {
                    net_line.push(if self.test_net_ok {
                        if english {
                            "PASS | "
                        } else {
                            "BAŞARILI | "
                        }
                    } else if english {
                        "FAIL | "
                    } else {
                        "UYARI | "
                    });
                    net_line.push(self.test_net_msg);
                } else {
                    let ready = crate::drivers::e1000::is_ready();
                    let link = crate::drivers::e1000::link_up();
                    if ready && link {
                        net_line.push(if english {
                            "Intel 82540EM Link UP | 10.0.2.15 ready"
                        } else {
                            "Intel 82540EM Link UP | 10.0.2.15 hazır"
                        });
                    } else if ready {
                        net_line.push(if english {
                            "Intel 82540EM Link DOWN"
                        } else {
                            "Intel 82540EM Bağlantı Yok"
                        });
                    } else {
                        net_line.push(if english {
                            "NIC not detected"
                        } else {
                            "Ağ kartı algılanmadı"
                        });
                    }
                }
                draw_device_line(
                    fb,
                    win_x + CONTENT_X + sp(316),
                    net_btn.1 + 4,
                    win_w.saturating_sub(CONTENT_X + sp(326)),
                    &net_line,
                    if self.test_net_tested {
                        if self.test_net_ok {
                            Color::ACCENT_GREEN
                        } else {
                            Color::ACCENT_RED
                        }
                    } else {
                        Color::TEXT_MAIN
                    },
                );
            }
            3 => {
                render_device_inventory(
                    fb,
                    win_x + CONTENT_X,
                    win_y + sp(UI_TOP_Y + UI_LABEL_OFFSET),
                    win_w.saturating_sub(CONTENT_X + 16),
                    self.language,
                    kbd_layout,
                    cpu_vendor,
                    cpu_logical_count,
                    total_ram_bytes,
                );
            }
            _ => {
                self.render_tests(fb, win_x, win_y, win_w, total_ram_bytes);
            }
        }

        // Alt Aksiyon Butonları (Kaydet & Sıfırla)
        let save_r = Self::save_btn_rect(win_x, win_y, win_h);
        let reset_r = Self::reset_btn_rect(win_x, win_y, win_h);
        MinGuiCompositor::draw_button(
            fb,
            save_r,
            Color::ACCENT_GREEN,
            crate::gui::ui_text("Kaydet", "Save"),
            Color::DARK_BG,
            false,
        );
        MinGuiCompositor::draw_button(
            fb,
            reset_r,
            Color::ACCENT_RED,
            crate::gui::ui_text("Sıfırla", "Reset"),
            Color::WHITE,
            false,
        );

        let (save_idx, reset_idx) = match self.active_tab {
            0 => (7, 8),
            _ => (5, 6),
        };
        if self.focused_control == save_idx {
            fb.draw_rect_outline(save_r.0, save_r.1, save_r.2, save_r.3, Color::ACCENT_YELLOW);
        }
        if self.focused_control == reset_idx {
            fb.draw_rect_outline(
                reset_r.0,
                reset_r.1,
                reset_r.2,
                reset_r.3,
                Color::ACCENT_YELLOW,
            );
        }

        // Durum Çubuğu
        let bar_y = win_y + win_h.saturating_sub(sp(22));
        fb.draw_rect(
            win_x + sp(6),
            bar_y,
            win_w.saturating_sub(sp(12)),
            sp(20),
            Color::TASKBAR_BG,
        );
        let cur_status = if self.status_msg.is_empty() {
            crate::gui::ui_text(
                "Canlı Ortam Kalıcı Depolama: Hazır",
                "Live CD Persistent Storage: Ready",
            )
        } else {
            self.status_msg
        };
        MinGuiCompositor::draw_string(
            fb,
            win_x + sp(12),
            bar_y + sp(6),
            cur_status,
            Color::ACCENT_YELLOW,
        );
    }

    /// Testler sekmesi: fare, RAM, disk ve ses donanım testleri.
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

        // ---- Fare testi (Satır 0: Başlık, Satır 1..3: Pad) ----
        MinGuiCompositor::draw_string(
            fb,
            cx,
            win_y + sp(UI_TOP_Y + UI_LABEL_OFFSET),
            crate::gui::ui_text(
                "Fare testi: imleci alana getirin, düğmeleri deneyin",
                "Mouse test: move into pad, try buttons/wheel",
            ),
            Color::TEXT_MAIN,
        );
        let pad = Self::test_mouse_pad_rect(win_x, win_y);
        fb.draw_rect(pad.0, pad.1, pad.2, pad.3, Color::DARK_BG);
        fb.draw_rect_outline(pad.0, pad.1, pad.2, pad.3, Color::BORDER);
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
            fb.draw_rect(
                mx.saturating_sub(2),
                my.saturating_sub(2),
                5,
                5,
                Color::ACCENT_GREEN,
            );
        } else {
            MinGuiCompositor::draw_string(
                fb,
                pad.0 + 8,
                pad.1 + 8,
                crate::gui::ui_text("imleç alanın dışında", "pointer outside pad"),
                Color::TEXT_MUTED,
            );
        }

        // Düğme göstergeleri (L/M/R)
        let ix = pad.0 + pad.2 + 16;
        let iy = pad.1;
        MinGuiCompositor::draw_string(
            fb,
            ix,
            iy,
            crate::gui::ui_text("Düğmeler", "Buttons"),
            Color::ACCENT_YELLOW,
        );
        let items: [(&str, u8); 3] = [
            (if english { "Left" } else { "Sol" }, 1),
            (if english { "Middle" } else { "Orta" }, 4),
            (if english { "Right" } else { "Sağ" }, 2),
        ];
        for (i, (name, bit)) in items.iter().enumerate() {
            let y = iy + sp(18) + i * sp(20);
            let on = self.test_mouse_buttons & *bit != 0;
            fb.draw_rect(
                ix,
                y,
                14,
                14,
                if on {
                    Color::ACCENT_GREEN
                } else {
                    Color::TITLE_BAR_INACTIVE
                },
            );
            fb.draw_rect_outline(ix, y, 14, 14, Color::BORDER);
            MinGuiCompositor::draw_string(
                fb,
                ix + 22,
                y + sp(1),
                *name,
                if on {
                    Color::TEXT_MAIN
                } else {
                    Color::TEXT_MUTED
                },
            );
        }

        let mut info = TextBuf::new();
        info.push_str("X: ");
        info.push_usize(if self.test_mouse_over {
            self.test_mouse_x
        } else {
            0
        });
        info.push_str("  Y: ");
        info.push_usize(if self.test_mouse_over {
            self.test_mouse_y
        } else {
            0
        });
        MinGuiCompositor::draw_string(
            fb,
            pad.0 + 8,
            pad.1 + pad.3.saturating_sub(18),
            info.as_str(),
            Color::TEXT_MUTED,
        );

        let mut wheel = TextBuf::new();
        wheel.push_str(if english { "Wheel: " } else { "Tekerlek: " });
        wheel.push_usize(self.test_wheel as usize);
        MinGuiCompositor::draw_string(fb, ix, iy + sp(80), wheel.as_str(), Color::ACCENT_BLUE);

        // ---- RAM testi (Satır 4) ----
        MinGuiCompositor::draw_string(
            fb,
            cx,
            win_y + sp(UI_TOP_Y + 4 * UI_ROW_STEP + UI_LABEL_OFFSET),
            crate::gui::ui_text("RAM testi:", "RAM test:"),
            Color::ACCENT_YELLOW,
        );
        let ram_btn = Self::test_ram_btn_rect(win_x, win_y);
        MinGuiCompositor::draw_button(
            fb,
            ram_btn,
            Color::ACCENT_BLUE,
            crate::gui::ui_text("Çalıştır", "Run"),
            Color::DARK_BG,
            false,
        );
        let mut ram = DeviceLine::new();
        ram.push(if english {
            "Usable: "
        } else {
            "Kullanılabilir: "
        });
        ram.number(total_ram_bytes / (1024 * 1024));
        ram.push(" MB");
        if self.test_ram_tested {
            let status: &str = if self.test_ram_ok {
                if english {
                    "PASS"
                } else {
                    "BAŞARILI"
                }
            } else if english {
                "FAIL: "
            } else {
                "HATA: "
            };
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
        draw_device_line(
            fb,
            cx + 112,
            ram_btn.1 + 4,
            win_w.saturating_sub(CONTENT_X + 124),
            &ram,
            if self.test_ram_tested && !self.test_ram_ok {
                Color::ACCENT_RED
            } else {
                Color::TEXT_MAIN
            },
        );

        // ---- Disk testi (Satır 5) ----
        MinGuiCompositor::draw_string(
            fb,
            cx,
            win_y + sp(UI_TOP_Y + 5 * UI_ROW_STEP + UI_LABEL_OFFSET),
            crate::gui::ui_text("Disk testi:", "Disk test:"),
            Color::ACCENT_YELLOW,
        );
        let disk_btn = Self::test_disk_btn_rect(win_x, win_y);
        MinGuiCompositor::draw_button(
            fb,
            disk_btn,
            Color::ACCENT_BLUE,
            crate::gui::ui_text("Çalıştır", "Run"),
            Color::DARK_BG,
            false,
        );
        let mut disk = DeviceLine::new();
        if self.test_disk_tested {
            disk.push(if self.test_disk_ok {
                if english {
                    "PASS | "
                } else {
                    "BAŞARILI | "
                }
            } else if english {
                "FAIL | "
            } else {
                "HATA | "
            });
            disk.number(self.test_disk_sectors as u64);
            disk.push(if english {
                " sectors in "
            } else {
                " sektör / "
            });
            disk.number(self.test_disk_ms as u64);
            disk.push(" ms | ");
            disk.number(self.test_disk_kbps as u64);
            disk.push(" KB/s");
        } else {
            disk.push(if english {
                "Not run yet."
            } else {
                "Henüz çalıştırılmadı."
            });
        }
        draw_device_line(
            fb,
            cx + 112,
            disk_btn.1 + 4,
            win_w.saturating_sub(CONTENT_X + 124),
            &disk,
            if self.test_disk_tested && !self.test_disk_ok {
                Color::ACCENT_RED
            } else {
                Color::TEXT_MAIN
            },
        );

        // ---- Ses alt sistemi testi (Satır 6) ----
        MinGuiCompositor::draw_string(
            fb,
            cx,
            win_y + sp(UI_TOP_Y + 6 * UI_ROW_STEP + UI_LABEL_OFFSET),
            crate::gui::ui_text("Ses testi:", "Audio test:"),
            Color::ACCENT_YELLOW,
        );
        let audio_btn = Self::test_audio_btn_rect(win_x, win_y);
        MinGuiCompositor::draw_button(
            fb,
            audio_btn,
            Color::ACCENT_BLUE,
            crate::gui::ui_text("Test Et", "Test"),
            Color::DARK_BG,
            false,
        );
        let mut audio = DeviceLine::new();
        if self.test_audio_tested {
            audio.push(if self.test_audio_ok {
                if english {
                    "PASS | "
                } else {
                    "BAŞARILI | "
                }
            } else {
                if english {
                    "FALLBACK | "
                } else {
                    "YEDEK | "
                }
            });
            audio.push(self.test_audio_msg);
        } else {
            audio.push(if english {
                "HDA DMA + PC Speaker ready."
            } else {
                "HDA DMA + PC Speaker hazır."
            });
        }
        draw_device_line(
            fb,
            cx + 112,
            audio_btn.1 + 4,
            win_w.saturating_sub(CONTENT_X + 124),
            &audio,
            if self.test_audio_tested {
                if self.test_audio_ok {
                    Color::ACCENT_GREEN
                } else {
                    Color::ACCENT_YELLOW
                }
            } else {
                Color::TEXT_MAIN
            },
        );
    }
}
