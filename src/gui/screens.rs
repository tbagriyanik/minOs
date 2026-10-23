//! Loading, boot and welcome screens.

use super::*;

/// Draw one frame of the centered minOs loading transition.
pub fn render_loading_transition(fb: &mut Framebuffer, width: usize, height: usize, phase: usize, message: &str) {
    fb.draw_rect(0, 0, width, height, Color::DARK_BG);
    let cx = width / 2;
    let cy = height / 2;
    let icon_size = 48 + (phase % 7) * 4;
    MinGuiCompositor::draw_cat_icon_at_size(fb, cx.saturating_sub(icon_size / 2), cy.saturating_sub(78), icon_size);

    let brand = "minOs";
    let brand_w = MinGuiCompositor::text_width(brand, 3);
    MinGuiCompositor::draw_string_scaled(fb, cx.saturating_sub(brand_w / 2), cy.saturating_sub(10), brand, Color::TEXT_MAIN, 3);

    let bar_w = 220.min(width.saturating_sub(48));
    let bar_x = cx.saturating_sub(bar_w / 2);
    let bar_y = cy + 34;
    fb.draw_rounded_rect(bar_x, bar_y, bar_w, 8, 4, Color::TASKBAR_BG);
    let sweep_w = (bar_w / 3).max(12);
    let travel = bar_w.saturating_sub(sweep_w);
    let sweep_x = bar_x + (phase * 19 % (travel + 1));
    fb.draw_rounded_rect(sweep_x, bar_y + 1, sweep_w, 6, 3, Color::ACCENT_BLUE);

    let dots = match phase % 4 { 0 => ".  ", 1 => ".. ", 2 => "...", _ => "   " };
    let text_w = MinGuiCompositor::text_width(message, 1);
    let dots_w = MinGuiCompositor::text_width(dots, 1);
    let group_w = text_w + 4 + dots_w;
    let text_x = cx.saturating_sub(group_w / 2);
    MinGuiCompositor::draw_string(fb, text_x, bar_y + 20, message, Color::TEXT_MUTED);
    let dots_x = text_x + text_w + 4;
    MinGuiCompositor::draw_string(fb, dots_x, bar_y + 20, dots, Color::ACCENT_BLUE);
    fb.flush();
}

/// Draw a short-lived busy panel before a synchronous operation begins.
/// The static indicator remains visible until the current disk call returns.
pub fn draw_busy_overlay(fb: &mut Framebuffer, _mx: usize, _my: usize, message: &str, progress: usize) {
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
    fb.draw_rounded_rect(bx + 1, by + 1, bw.saturating_sub(2) * pct / 100, 12, 3, Color::ACCENT_BLUE);
    // Yüzde etiketi: çubukla tutarlı tek bir değer gösterir.
    let mut label = TextBuf::new();
    label.push_usize(pct);
    label.push_str("%");
    MinGuiCompositor::draw_string(fb, bx, by + 22, label.as_str(), Color::TEXT_MUTED);
    fb.flush_rect((x, y, w, h));
}

/// Dolgulu kum saati işlem imleci. İmlecin altındaki arka planı saklar;
/// böylece bir sonraki karede `restore_cursor_background` ile temizlenebilir.
pub fn draw_busy_hourglass(fb: &mut Framebuffer, mx: usize, my: usize) {
    fb.save_cursor_background(mx, my);
    // 8x9 kum saati deseni (bit 7 = sol sütun).
    const HOURGLASS: [u8; 8] = [
        0b11111110,
        0b01111100,
        0b00111000,
        0b00010000,
        0b00111000,
        0b01111100,
        0b11111110,
        0b00000000,
    ];
    for (row, bits) in HOURGLASS.iter().enumerate() {
        for col in 0..8 {
            if bits & (1 << (7 - col)) != 0 {
                fb.put_pixel(mx + col, my + row, Color::ACCENT_YELLOW);
            }
        }
    }
    fb.flush_rect((mx, my, 10, 10));
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

    // Bilgi kutusu
    let box_w = 640.min(width.saturating_sub(48)).max(320);
    let box_h = 300.min(height.saturating_sub(96)).max(180);
    let box_x = width.saturating_sub(box_w) / 2;
    let box_y = 52 + (height.saturating_sub(52 + box_h)) / 2;
    fb.draw_rounded_rect(box_x, box_y, box_w, box_h, 10, Color::WINDOW_BG);
    fb.draw_rounded_rect_outline(box_x, box_y, box_w, box_h, 10, Color::ACCENT_RED);

    let lines: [(&str, u32); 8] = [
        ("Diskte geçerli bir minOs kurulumu bulunamadı.", Color::TEXT_MAIN),
        ("No valid minOs installation was found on disk.", Color::TEXT_MUTED),
        ("", Color::TEXT_MUTED),
        ("Bu önyükleme seçeneği yalnızca kurulu sistem içindir.", Color::TEXT_MAIN),
        ("This boot option is only for the installed system.", Color::TEXT_MUTED),
        ("", Color::TEXT_MUTED),
        ("Kurulum için 'minOs Live Environment' ile açıp sihirbazı tamamlayın.", Color::ACCENT_YELLOW),
        ("Boot 'minOs Live Environment' and complete the setup wizard.", Color::ACCENT_YELLOW),
    ];

    let mut y = box_y + 24;
    for (text, color) in lines.iter() {
        if !text.is_empty() {
            MinGuiCompositor::draw_string(fb, box_x + 24, y, text, *color);
        }
        y += 26;
    }

    let footer = "Yeniden başlatmak için herhangi bir tuşa basın.  /  Press any key to restart.";
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
        MinGuiCompositor::draw_string(fb, 24, box_y + box_h + 18, "Yeniden başlatmak için bir tuşa basın / Press any key to restart", Color::TEXT_MAIN);
    }

    fb.flush();
}

/// Hoş geldin ekranı seçim alanları; çizim ve giriş işleme aynı geometriyi kullanır.
fn welcome_panel(width: usize, height: usize) -> (usize, usize, usize, usize) {
    let w = 480.min(width.saturating_sub(24)).max(320);
    let h = 290.min(height.saturating_sub(24)).max(240);
    ((width.saturating_sub(w)) / 2, (height.saturating_sub(h)) / 2, w, h)
}

pub fn welcome_language_rect(width: usize, height: usize, index: usize) -> (usize, usize, usize, usize) {
    let (x, y, _, _) = welcome_panel(width, height);
    (x + 28 + index * 140, y + 94, 126, 32)
}

pub fn welcome_theme_rect(width: usize, height: usize, index: usize) -> (usize, usize, usize, usize) {
    let (x, y, _, _) = welcome_panel(width, height);
    (x + 28 + index * 82, y + 164, 74, 50)
}

pub fn welcome_continue_rect(width: usize, height: usize) -> (usize, usize, usize, usize) {
    let (x, y, w, h) = welcome_panel(width, height);
    (x + w.saturating_sub(168), y + h.saturating_sub(50), 140, 36)
}

pub fn render_welcome_screen(fb: &mut Framebuffer, width: usize, height: usize, english: bool, selected: theme::ThemeId, mouse: &MouseState) {
    let (x, y, w, h) = welcome_panel(width, height);
    fb.draw_rect(0, 0, width, height, Color::DARK_BG);
    fb.draw_shadow(x, y, w, h, Color::SHADOW);
    fb.draw_rounded_rect(x, y, w, h, 10, Color::WINDOW_BG);
    fb.draw_rounded_vgradient(x, y, w, 48, 10, Color::TITLE_BAR, Color::TITLE_BAR_INACTIVE);
    MinGuiCompositor::draw_cat_icon_scaled(fb, x + 18, y + 12);
    MinGuiCompositor::draw_string(fb, x + 50, y + 16, if english { "Welcome to minOs" } else { "minOs'a hoş geldiniz" }, Color::TEXT_MAIN);

    // 1. Dil Seçimi Bölümü
    MinGuiCompositor::draw_string(fb, x + 28, y + 70,
        if english { "Interface Language" } else { "Arayüz Dili" }, Color::ACCENT_BLUE);

    for i in 0..2 {
        let rect = welcome_language_rect(width, height, i);
        let active = (i == 0 && !english) || (i == 1 && english);
        fb.draw_rounded_rect(rect.0, rect.1, rect.2, rect.3, 6,
            if active { Color::ACCENT_BLUE } else { Color::TASKBAR_BG });
        fb.draw_rect_outline(rect.0, rect.1, rect.2, rect.3,
            if active { Color::ACCENT_BLUE } else { Color::BORDER });
        MinGuiCompositor::draw_string(fb, rect.0 + 26, rect.1 + 8,
            if i == 0 { "Türkçe" } else { "English" },
            if active { MinGuiCompositor::text_on(Color::ACCENT_BLUE) } else { Color::TEXT_MAIN });
    }

    // 2. Masaüstü Teması Bölümü
    MinGuiCompositor::draw_string(fb, x + 28, y + 140,
        if english { "Desktop Theme" } else { "Masaüstü Teması" }, Color::ACCENT_BLUE);
    for (i, id) in theme::ThemeId::ALL.iter().enumerate() {
        let rect = welcome_theme_rect(width, height, i);
        let (background, accent) = theme::preview_colors(*id);
        fb.draw_rounded_rect(rect.0, rect.1, rect.2, rect.3, 6, background);
        fb.draw_rounded_rect(rect.0 + 6, rect.1 + 6, rect.2.saturating_sub(12), 6, 2, accent);
        let name = if english {
            match id {
                theme::ThemeId::VelvetDark => "Velvet",
                theme::ThemeId::Daylight => "Daylight",
                theme::ThemeId::Nord => "Nord",
                theme::ThemeId::TerminalGreen => "Terminal",
                theme::ThemeId::Sunset => "Sunset",
            }
        } else { id.short_name() };
        MinGuiCompositor::draw_string(fb, rect.0 + 8, rect.1 + 25, name, MinGuiCompositor::text_on(background));
        if *id == selected {
            fb.draw_rect_outline(rect.0, rect.1, rect.2, rect.3, Color::ACCENT_BLUE);
            fb.draw_rect_outline(rect.0.saturating_sub(1), rect.1.saturating_sub(1), rect.2 + 2, rect.3 + 2, Color::ACCENT_BLUE);
        } else {
            fb.draw_rect_outline(rect.0, rect.1, rect.2, rect.3, Color::BORDER);
        }
    }

    // 3. Alt Kısım: Bilgi İpucu (Zaman Aşımı Göstergesi) & Başla Düğmesi
    let hint_text = if english {
        "Press Enter or wait to start..."
    } else {
        "Enter'a basın veya bekleyin..."
    };
    MinGuiCompositor::draw_string(fb, x + 28, y + h.saturating_sub(40), hint_text, Color::TEXT_MUTED);

    let button = welcome_continue_rect(width, height);
    fb.draw_rounded_rect(button.0, button.1, button.2, button.3, 6, Color::ACCENT_GREEN);
    let btn_text = if english { "Get Started" } else { "Başla" };
    let text_col = MinGuiCompositor::text_on(Color::ACCENT_GREEN);
    MinGuiCompositor::draw_string(fb, button.0 + 20, button.1 + 10, btn_text, text_col);
    // Başlat menüsündeki gibi zarif antialiased sağ ok (chevron) simgesi
    let arrow_x = (button.0 + button.2 - 20) as f32;
    let arrow_y = (button.1 + button.3 / 2) as f32;
    MinGuiCompositor::draw_antialiased_line(fb, arrow_x - 3.0, arrow_y - 4.5, arrow_x + 1.5, arrow_y, text_col, 2.0);
    MinGuiCompositor::draw_antialiased_line(fb, arrow_x + 1.5, arrow_y, arrow_x - 3.0, arrow_y + 4.5, text_col, 2.0);

    let pointer_x = mouse.x.max(0) as usize;
    let pointer_y = mouse.y.max(0) as usize;
    fb.draw_rect(pointer_x, pointer_y, 5, 5, Color::WHITE);
    fb.draw_rect_outline(pointer_x, pointer_y, 5, 5, Color::BLACK);
    fb.flush();
}
