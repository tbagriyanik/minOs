//! Loading, boot and welcome screens.

use super::*;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Fare imlecinin "meşgul" (kum saati) kipinde olup olmadığı.
/// `true` olduğunda `draw_mouse_cursor` ok yerine kum saati çizer.
/// Sayaç biçimindedir: iç içe uzun işlemler (ör. paket kurulumu içinden
/// küçük bir dosya okuma) birbirini erken kapatmaz.
static CURSOR_BUSY: AtomicUsize = AtomicUsize::new(0);

/// Meşgul durumunu ayarlar. Uzun işlemler (PNG çözme, kopyalama vb.) sırasında
/// `true`, işlem bittiğinde `false` yapılmalıdır.
pub fn set_cursor_busy(busy: bool) {
    if busy {
        CURSOR_BUSY.fetch_add(1, Ordering::SeqCst);
    } else {
        CURSOR_BUSY
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
                Some(v.saturating_sub(1))
            })
            .ok();
    }
}

/// Meşgul durumu döner.
pub fn is_cursor_busy() -> bool {
    CURSOR_BUSY.load(Ordering::SeqCst) > 0
}

/// Kapsam koruyucu: oluşturulduğunda imleci kum saatine alır, `Drop` olduğunda
/// (işlev kapsamı bitince) otomatik olarak normale döndürür. Erken `return`
/// yollarında bile doğru çalışır.
pub struct BusyScope;

impl BusyScope {
    pub fn new() -> Self {
        set_cursor_busy(true);
        Self
    }
}

impl Default for BusyScope {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BusyScope {
    fn drop(&mut self) {
        set_cursor_busy(false);
    }
}

///Draw one frame of the centered minOs loading transition.
///
///Aşama metni, yüzde ve çubuk geometrisi `gui::screen_layout` üzerinden
///alınır; böylece açılış ekranı host GUI smoke testlerinde de doğrulanır.
pub fn render_loading_transition(fb: &mut Framebuffer, width: usize, height: usize, phase: usize) {
    let english = ui_english();
    let stage = screen_layout::loading_stage(phase, english);
    let percent = screen_layout::loading_percent(phase);
    let bar = screen_layout::loading_bar(width, height, phase);

    fb.draw_rect(0, 0, width, height, Color::DARK_BG);
    let cx = width / 2;
    let cy = height / 2;
    let icon_size = 48 + (phase % 7) * 4;
    MinGuiCompositor::draw_cat_icon_at_size(
        fb,
        cx.saturating_sub(icon_size / 2),
        cy.saturating_sub(78),
        icon_size,
    );

    let brand = "minOs";
    let brand_w = MinGuiCompositor::text_width(brand, 3);
    MinGuiCompositor::draw_string_scaled(
        fb,
        cx.saturating_sub(brand_w / 2),
        cy.saturating_sub(10),
        brand,
        Color::TEXT_MAIN,
        3,
    );

    fb.draw_rounded_rect(
        bar.track.0,
        bar.track.1,
        bar.track.2,
        bar.track.3,
        4,
        Color::TASKBAR_BG,
    );
    fb.draw_rounded_rect(
        bar.sweep.0,
        bar.sweep.1,
        bar.sweep.2,
        bar.sweep.3,
        3,
        Color::ACCENT_BLUE,
    );

    let stage_w = MinGuiCompositor::text_width(stage, 1);
    MinGuiCompositor::draw_string(
        fb,
        cx.saturating_sub(stage_w / 2),
        bar.track.1 + 20,
        stage,
        Color::TEXT_MUTED,
    );

    let mut percent_text = TextBuf::new();
    percent_text.push_usize(percent);
    percent_text.push_str("%");
    let percent_w = MinGuiCompositor::text_width(percent_text.as_str(), 1);
    MinGuiCompositor::draw_string(
        fb,
        bar.track.0 + bar.track.2.saturating_sub(percent_w),
        bar.track.1 + 20,
        percent_text.as_str(),
        Color::ACCENT_GREEN,
    );

    let hint = screen_layout::loading_hint(english);
    let hint_w = MinGuiCompositor::text_width(hint, 1);
    MinGuiCompositor::draw_string(
        fb,
        cx.saturating_sub(hint_w / 2),
        bar.track.1 + 38,
        hint,
        Color::TEXT_MUTED,
    );
    fb.flush();
}

/// Draw a short-lived busy panel before a synchronous operation begins.
/// The static indicator remains visible until the current disk call returns.
pub fn draw_busy_overlay(
    fb: &mut Framebuffer,
    _mx: usize,
    _my: usize,
    message: &str,
    progress: usize,
) {
    let w = 340.min(fb.width.saturating_sub(24));
    let h = 104;
    let x = fb.width.saturating_sub(w) / 2;
    let y = fb.height.saturating_sub(h) / 2;
    fb.draw_rounded_rect(x, y, w, h, 8, Color::WINDOW_BG);
    fb.draw_rounded_rect_outline(x, y, w, h, 8, Color::ACCENT_BLUE);
    MinGuiCompositor::draw_string(fb, x + 18, y + 16, message, Color::TEXT_MAIN);
    let bx = x + 18;
    let by = y + 52;
    let bw = w.saturating_sub(36);
    let pct = progress.min(100);
    fb.draw_rounded_rect(bx, by, bw, 14, 4, Color::DARK_BG);
    fb.draw_rounded_rect(
        bx + 1,
        by + 1,
        bw.saturating_sub(2) * pct / 100,
        12,
        3,
        Color::ACCENT_BLUE,
    );
    // Yüzde etiketi: çubukla tutarlı tek bir değer gösterir.
    let mut label = TextBuf::new();
    label.push_usize(pct);
    label.push_str("%");
    MinGuiCompositor::draw_string(fb, bx, by + 22, label.as_str(), Color::TEXT_MUTED);
    fb.flush_rect((x, y, w, h));
}

/// Dolgulu kum saati işlem imleci. Yalnızca imleç desenini çizer.
///
/// Arka planı saklamak ve ekrana aktarmak çağırının sorumluluğundadır
/// (`Framebuffer::save_cursor_background` + `flush_rect`). Böylece aynı
/// yardımcı hem gövde çiziminde hem de önyükleme ilerleme göstergesinde
/// kullanılabilir; çizim sırasında ekrana yazmak, henüz uygulanmamış gece
/// ışığı filtresini atlayan pikseller bırakır.
pub fn draw_busy_hourglass(fb: &mut Framebuffer, mx: usize, my: usize) {
    // 8x9 kum saati deseni (bit 7 = sol sütun).
    const HOURGLASS: [u8; 8] = [
        0b11111110, 0b01111100, 0b00111000, 0b00010000, 0b00111000, 0b01111100, 0b11111110,
        0b00000000,
    ];
    for (row, bits) in HOURGLASS.iter().enumerate() {
        for col in 0..8 {
            if bits & (1 << (7 - col)) != 0 {
                fb.put_pixel(mx + col, my + row, Color::ACCENT_YELLOW);
            }
        }
    }
}

/// Önyükleme hatası ekranı. Kernel `installed` modunda başlatılmak istendiğinde
/// ancak diskte geçerli bir minOs kurulumu (MINOS.SYS) bulunamazsa gösterilir.
/// Metin TR ve EN olarak birlikte çizilir; bu noktada ayarlar yüklenmemiş
/// olduğundan dil tercihi bilinemez.
pub fn render_boot_error(fb: &mut Framebuffer, width: usize, height: usize) {
    fb.draw_rect(0, 0, width, height, Color::DARK_BG);

    // Üst hata şeridi
    fb.draw_rect(0, 0, width, 52, Color::ACCENT_RED);
    let title = "Önyükleme Hatası  /  Boot Error";
    let title_w = MinGuiCompositor::text_width(title, 2);
    MinGuiCompositor::draw_string_scaled(
        fb,
        width.saturating_sub(title_w) / 2,
        14,
        title,
        Color::WHITE,
        2,
    );

    // Bilgi kutusu (geometri GUI smoke testleriyle paylaşılır).
    let (box_x, box_y, box_w, box_h) = screen_layout::boot_error_box(width, height);
    fb.draw_rounded_rect(box_x, box_y, box_w, box_h, 10, Color::WINDOW_BG);
    fb.draw_rounded_rect_outline(box_x, box_y, box_w, box_h, 10, Color::ACCENT_RED);

    let code = screen_layout::boot_error_code();
    MinGuiCompositor::draw_string(fb, box_x + 24, box_y + 18, code, Color::ACCENT_RED);

    let lines: [(&str, u32); 10] = [
        (
            "Diskte geçerli bir minOs kurulumu bulunamadı.",
            Color::TEXT_MAIN,
        ),
        (
            "No valid minOs installation was found on disk.",
            Color::TEXT_MUTED,
        ),
        ("", Color::TEXT_MUTED),
        (
            "Bu önyükleme seçeneği yalnızca kurulu sistem içindir.",
            Color::TEXT_MAIN,
        ),
        (
            "This boot option is only for the installed system.",
            Color::TEXT_MUTED,
        ),
        ("", Color::TEXT_MUTED),
        (
            "Kurulum için 'minOs Live Environment' ile açıp sihirbazı tamamlayın.",
            Color::ACCENT_YELLOW,
        ),
        (
            "Boot 'minOs Live Environment' and complete the setup wizard.",
            Color::ACCENT_YELLOW,
        ),
        (
            screen_layout::boot_error_recovery(false),
            Color::ACCENT_GREEN,
        ),
        (
            screen_layout::boot_error_recovery(true),
            Color::ACCENT_GREEN,
        ),
    ];

    let mut y = box_y + 42;
    for (text, color) in lines.iter() {
        if !text.is_empty() {
            MinGuiCompositor::draw_string(fb, box_x + 24, y, text, *color);
        }
        y += 24;
    }

    let footer = screen_layout::boot_error_footer();
    let footer_w = MinGuiCompositor::text_width(footer, 1);
    if footer_w <= width.saturating_sub(24) {
        MinGuiCompositor::draw_string(
            fb,
            width.saturating_sub(footer_w) / 2,
            box_y + box_h + 18,
            footer,
            Color::TEXT_MAIN,
        );
    } else {
        MinGuiCompositor::draw_string(
            fb,
            24,
            box_y + box_h + 18,
            "Yeniden başlatmak için bir tuşa basın / Press any key to restart",
            Color::TEXT_MAIN,
        );
    }

    fb.flush();
}

/// Hoş geldin ekranı seçim alanları; çizim, giriş işleme ve host smoke
/// testleri aynı geometri kaynağını (`screen_layout`) kullanır.
fn welcome_panel(width: usize, height: usize) -> screen_layout::Rect {
    screen_layout::welcome_panel(width, height)
}

pub fn welcome_language_rect(width: usize, height: usize, index: usize) -> screen_layout::Rect {
    screen_layout::welcome_language_rect(width, height, index)
}

pub fn welcome_theme_rect(width: usize, height: usize, index: usize) -> screen_layout::Rect {
    screen_layout::welcome_theme_rect(width, height, index)
}

pub fn welcome_continue_rect(width: usize, height: usize) -> screen_layout::Rect {
    screen_layout::welcome_continue_rect(width, height)
}

pub fn render_welcome_screen(
    fb: &mut Framebuffer,
    width: usize,
    height: usize,
    english: bool,
    selected: theme::ThemeId,
    focused_control: usize,
    mouse: &MouseState,
) {
    let (x, y, w, h) = welcome_panel(width, height);
    fb.draw_rect(0, 0, width, height, Color::DARK_BG);
    fb.draw_shadow(x, y, w, h, Color::SHADOW);
    fb.draw_rounded_rect(x, y, w, h, 10, Color::WINDOW_BG);
    fb.draw_rounded_vgradient(x, y, w, 48, 10, Color::TITLE_BAR, Color::TITLE_BAR_INACTIVE);
    MinGuiCompositor::draw_cat_icon_scaled(fb, x + 18, y + 12);
    MinGuiCompositor::draw_string(
        fb,
        x + 50,
        y + 16,
        if english {
            "Welcome to minOs"
        } else {
            "minOs'a hoş geldiniz"
        },
        Color::TEXT_MAIN,
    );

    // 1. Dil Seçimi Bölümü
    MinGuiCompositor::draw_string(
        fb,
        x + 28,
        y + 70,
        if english {
            "Interface Language"
        } else {
            "Arayüz Dili"
        },
        Color::ACCENT_BLUE,
    );

    for i in 0..2 {
        let rect = welcome_language_rect(width, height, i);
        let active = (i == 0 && !english) || (i == 1 && english);
        fb.draw_rounded_rect(
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            6,
            if active {
                Color::ACCENT_BLUE
            } else {
                Color::TASKBAR_BG
            },
        );
        fb.draw_rect_outline(
            rect.0,
            rect.1,
            rect.2,
            rect.3,
            if active {
                Color::ACCENT_BLUE
            } else {
                Color::BORDER
            },
        );
        MinGuiCompositor::draw_string(
            fb,
            rect.0 + 26,
            rect.1 + 8,
            if i == 0 { "Türkçe" } else { "English" },
            if active {
                MinGuiCompositor::text_on(Color::ACCENT_BLUE)
            } else {
                Color::TEXT_MAIN
            },
        );
        if focused_control == i {
            fb.draw_rect_outline(
                rect.0.saturating_sub(2),
                rect.1.saturating_sub(2),
                rect.2 + 4,
                rect.3 + 4,
                Color::ACCENT_YELLOW,
            );
        }
    }

    // 2. Masaüstü Teması Bölümü
    MinGuiCompositor::draw_string(
        fb,
        x + 28,
        y + 130,
        if english {
            "Desktop Theme"
        } else {
            "Masaüstü Teması"
        },
        Color::ACCENT_BLUE,
    );
    for (i, id) in theme::ThemeId::ALL.iter().enumerate() {
        let rect = welcome_theme_rect(width, height, i);
        let (background, accent) = theme::preview_colors(*id);
        fb.draw_rounded_rect(rect.0, rect.1, rect.2, rect.3, 6, background);
        fb.draw_rounded_rect(
            rect.0 + 6,
            rect.1 + 6,
            rect.2.saturating_sub(12),
            6,
            2,
            accent,
        );
        let name = if english {
            match id {
                theme::ThemeId::VelvetDark => "Velvet",
                theme::ThemeId::Daylight => "Daylight",
                theme::ThemeId::Nord => "Nord",
                theme::ThemeId::TerminalGreen => "Terminal",
                theme::ThemeId::Sunset => "Sunset",
                theme::ThemeId::Dracula => "Dracula",
                theme::ThemeId::Monokai => "Monokai",
                theme::ThemeId::SolarizedDark => "Solarized",
                theme::ThemeId::HighContrast => "High Contrast",
            }
        } else {
            id.short_name()
        };
        MinGuiCompositor::draw_string(
            fb,
            rect.0 + 8,
            rect.1 + 25,
            name,
            MinGuiCompositor::text_on(background),
        );
        if *id == selected {
            fb.draw_rect_outline(rect.0, rect.1, rect.2, rect.3, Color::ACCENT_BLUE);
            fb.draw_rect_outline(
                rect.0.saturating_sub(1),
                rect.1.saturating_sub(1),
                rect.2 + 2,
                rect.3 + 2,
                Color::ACCENT_BLUE,
            );
        } else {
            fb.draw_rect_outline(rect.0, rect.1, rect.2, rect.3, Color::BORDER);
        }
        if focused_control == i + 2 {
            fb.draw_rect_outline(
                rect.0.saturating_sub(2),
                rect.1.saturating_sub(2),
                rect.2 + 4,
                rect.3 + 4,
                Color::ACCENT_YELLOW,
            );
        }
    }

    // 3. Alt Kısım: Bilgi İpucu (Zaman Aşımı Göstergesi) & Başla Düğmesi
    MinGuiCompositor::draw_string(
        fb,
        x + 28,
        y + h.saturating_sub(40),
        screen_layout::welcome_hint(english),
        Color::TEXT_MUTED,
    );

    let button = welcome_continue_rect(width, height);
    fb.draw_rounded_rect(
        button.0,
        button.1,
        button.2,
        button.3,
        6,
        Color::ACCENT_GREEN,
    );
    if focused_control == 2 + theme::ThemeId::ALL.len() {
        fb.draw_rect_outline(
            button.0.saturating_sub(2),
            button.1.saturating_sub(2),
            button.2 + 4,
            button.3 + 4,
            Color::ACCENT_YELLOW,
        );
    }
    let btn_text = if english { "Get Started" } else { "Başla" };
    let text_col = MinGuiCompositor::text_on(Color::ACCENT_GREEN);
    MinGuiCompositor::draw_string(fb, button.0 + 20, button.1 + 10, btn_text, text_col);
    // Başlat menüsündeki gibi zarif antialiased sağ ok (chevron) simgesi
    let arrow_x = (button.0 + button.2 - 20) as f32;
    let arrow_y = (button.1 + button.3 / 2) as f32;
    MinGuiCompositor::draw_antialiased_line(
        fb,
        arrow_x - 3.0,
        arrow_y - 4.5,
        arrow_x + 1.5,
        arrow_y,
        text_col,
        2.0,
    );
    MinGuiCompositor::draw_antialiased_line(
        fb,
        arrow_x + 1.5,
        arrow_y,
        arrow_x - 3.0,
        arrow_y + 4.5,
        text_col,
        2.0,
    );

    let pointer_x = mouse.x.max(0) as usize;
    let pointer_y = mouse.y.max(0) as usize;
    fb.draw_rect(pointer_x, pointer_y, 5, 5, Color::WHITE);
    fb.draw_rect_outline(pointer_x, pointer_y, 5, 5, Color::BLACK);
    fb.flush();
}

/// Sistem kapanış/yeniden başlatma ekranı. Başlık, adım metni ve ipucu
/// `screen_layout` üzerinden yerelleştirilir; `phase` kapanış adımını ve
/// spinner fazını birlikte belirler.
pub fn render_shutdown_screen(
    fb: &mut Framebuffer,
    width: usize,
    height: usize,
    phase: usize,
    restart: bool,
) {
    let english = ui_english();
    let title = screen_layout::shutdown_title(restart, english);
    let message = screen_layout::shutdown_step_text(phase, restart, english);
    let hint = screen_layout::shutdown_hint(english);

    fb.draw_rect(0, 0, width, height, Color::DARK_BG);

    let cx = width / 2;

    // Üst kapanış şeridi
    fb.draw_rect(0, 0, width, 52, Color::ACCENT_BLUE);
    let title_w = MinGuiCompositor::text_width(title, 2);
    MinGuiCompositor::draw_string_scaled(
        fb,
        cx.saturating_sub(title_w / 2),
        14,
        title,
        Color::WHITE,
        2,
    );

    // Bilgi kutusu (geometri GUI smoke testleriyle paylaşılır).
    let (box_x, box_y, box_w, box_h) = screen_layout::shutdown_box(width, height);
    fb.draw_rounded_rect(box_x, box_y, box_w, box_h, 10, Color::WINDOW_BG);
    fb.draw_rounded_rect_outline(box_x, box_y, box_w, box_h, 10, Color::ACCENT_BLUE);

    // Basit dönen imleç/spinner (ASCII, her karakter birincil Latin yüzde var).
    const SPINNER: [&str; 4] = ["|", "/", "-", "\\"];
    let spinner = SPINNER[phase % SPINNER.len()];
    let spin_w = MinGuiCompositor::text_width(spinner, 1);
    MinGuiCompositor::draw_string(
        fb,
        cx.saturating_sub(spin_w / 2),
        box_y + 30,
        spinner,
        Color::ACCENT_BLUE,
    );

    MinGuiCompositor::draw_string(fb, box_x + 24, box_y + 68, message, Color::TEXT_MAIN);

    let hint_w = MinGuiCompositor::text_width(hint, 1);
    MinGuiCompositor::draw_string(
        fb,
        cx.saturating_sub(hint_w / 2),
        box_y + box_h - 48,
        hint,
        Color::TEXT_MUTED,
    );

    let bar_x = box_x + 24;
    let bar_w = box_w.saturating_sub(48);
    let bar_y = box_y + box_h - 24;
    let steps = 4usize;
    let done = phase.saturating_add(1).min(steps);
    fb.draw_rounded_rect(bar_x, bar_y, bar_w, 6, 3, Color::TASKBAR_BG);
    fb.draw_rounded_rect(
        bar_x,
        bar_y,
        bar_w.saturating_mul(done) / steps,
        6,
        3,
        Color::ACCENT_BLUE,
    );

    fb.flush();
}
