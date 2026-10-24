//! Kontrol Paneli (Control Panel)
//!
//! Sistem, ağ ve sürüm bilgilerini canlı çekirdek API'lerinden okuyup gösteren
//! salt okunur panel; alt kısımdaki düğme ile tema değiştirilebilir.
//! Eski sürümdeki sabit/sahte değerler (128 MB RAM, sabit 1280x720, çelişkili
//! ağ durumu) yerine gerçek veriler kullanılır.

use crate::drivers::keyboard::KeyboardLayout;
use crate::drivers::rtc::DateTime;
use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Sekme sayısı: Sistem, Ağ, Hakkında.
pub const TAB_COUNT: usize = 3;

static ACTIVE_TAB: AtomicUsize = AtomicUsize::new(0);
static REFRESHED_AT_MS: AtomicU64 = AtomicU64::new(0);
/// 0: bos, 1: yanit bekleniyor, 2: basarili, 3: basarisiz.
static NET_TEST_STATE: AtomicUsize = AtomicUsize::new(0);
static NET_TEST_START_MS: AtomicU64 = AtomicU64::new(0);

/// Etkin sekme indeksi (0..TAB_COUNT).
pub fn active_tab() -> usize {
    ACTIVE_TAB.load(Ordering::Relaxed).min(TAB_COUNT - 1)
}

/// Sekmeyi seçer.
pub fn select_tab(tab: usize) {
    if tab < TAB_COUNT {
        ACTIVE_TAB.store(tab, Ordering::Relaxed);
    }
}

/// Pencere kapatıldığında sekmeyi başa alır.
pub fn reset() {
    ACTIVE_TAB.store(0, Ordering::Relaxed);
    NET_TEST_STATE.store(0, Ordering::Relaxed);
    REFRESHED_AT_MS.store(0, Ordering::Relaxed);
}

/// Panel verilerini yeniler (canli degerler bir sonraki cizimde okunur).
pub fn refresh() {
    REFRESHED_AT_MS.store(crate::monotonic_ms(), Ordering::Relaxed);
    NET_TEST_STATE.store(0, Ordering::Relaxed);
}

/// Ağ geçidine ICMP ping testi başlatır (UI kilitlemeden).
pub fn start_network_test() {
    if !crate::drivers::e1000::is_ready() {
        NET_TEST_STATE.store(3, Ordering::Relaxed);
        return;
    }
    crate::drivers::netstack::clear_ping_reply();
    let sent = crate::drivers::netstack::ping(crate::drivers::netstack::gateway_ip());
    NET_TEST_STATE.store(if sent { 1 } else { 3 }, Ordering::Relaxed);
    NET_TEST_START_MS.store(crate::monotonic_ms(), Ordering::Relaxed);
}

fn poll_network_test() {
    if NET_TEST_STATE.load(Ordering::Relaxed) != 1 { return; }
    let now = crate::monotonic_ms();
    if crate::drivers::netstack::has_ping_reply() {
        let _ = crate::drivers::netstack::take_ping_reply();
        NET_TEST_STATE.store(2, Ordering::Relaxed);
    } else if now.wrapping_sub(NET_TEST_START_MS.load(Ordering::Relaxed)) >= 2_500 {
        NET_TEST_STATE.store(3, Ordering::Relaxed);
    }
}

/// Yazı yakınlaştırmasına göre satır/sekme yüksekliği artışı (100/125/150 -> 0/5/10).
fn zoom_extra() -> usize {
    (crate::gui::font_zoom() as usize).saturating_sub(100) / 5
}

/// Sekme düğmesinin tıklanabilir dikdörtgeni.
pub fn tab_rect(win_x: usize, win_y: usize, win_w: usize, index: usize) -> (usize, usize, usize, usize) {
    let tab_w = win_w.saturating_sub(20) / TAB_COUNT;
    (
        win_x + 10 + index * tab_w,
        win_y + 32,
        tab_w.saturating_sub(4),
        24 + zoom_extra(),
    )
}

/// Tema değiştirme düğmesinin tıklanabilir dikdörtgeni.
pub fn theme_btn_rect(win_x: usize, win_y: usize, win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
    let w = 158 + zoom_extra() * 8;
    (
        win_x + win_w.saturating_sub(w + 12),
        win_y + win_h.saturating_sub(38),
        w,
        26,
    )
}

/// "Yenile" düğmesinin tıklanabilir dikdörtgeni.
pub fn refresh_btn_rect(win_x: usize, win_y: usize, win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
    let theme = theme_btn_rect(win_x, win_y, win_w, win_h);
    (theme.0.saturating_sub(100 + 8), theme.1, 100, theme.3)
}

/// "Ağ Testi" düğmesinin tıklanabilir dikdörtgeni.
pub fn net_test_btn_rect(win_x: usize, win_y: usize, win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
    let refresh = refresh_btn_rect(win_x, win_y, win_w, win_h);
    (refresh.0.saturating_sub(124 + 8), refresh.1, 124, refresh.3)
}

fn draw_row(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    w: usize,
    label: &str,
    value: &str,
    color: u32,
) {
    MinGuiCompositor::draw_string(fb, x, y, label, Color::TEXT_MUTED);
    let value_x = x + crate::gui::sp(150);
    let clip = (value_x, y.saturating_sub(2), w.saturating_sub(crate::gui::sp(150)), crate::gui::sp(20));
    MinGuiCompositor::draw_string_clipped(fb, value_x, y, value, color, clip);
}

fn push_time(buf: &mut TextBuf, now: &DateTime) {
    buf.push_u2(now.hour as u32);
    buf.push_str(":");
    buf.push_u2(now.minute as u32);
    buf.push_str(":");
    buf.push_u2(now.second as u32);
}

fn push_date(buf: &mut TextBuf, now: &DateTime) {
    buf.push_u2(now.day as u32);
    buf.push_str(".");
    buf.push_u2(now.month as u32);
    buf.push_str(".");
    buf.push_u4(now.year as u32);
}

fn push_uptime(buf: &mut TextBuf, total_seconds: usize) {
    let mut text = [0u8; crate::gui::timefmt::MAX_TEXT];
    let len = crate::gui::timefmt::format_duration(
        total_seconds as u64,
        crate::gui::ui_english(),
        &mut text,
    );
    for &byte in &text[..len] {
        buf.push_byte(byte);
    }
}

fn push_ip(buf: &mut TextBuf, ip: crate::drivers::netstack::Ipv4Address) {
    buf.push_usize(ip.0[0] as usize);
    buf.push_str(".");
    buf.push_usize(ip.0[1] as usize);
    buf.push_str(".");
    buf.push_usize(ip.0[2] as usize);
    buf.push_str(".");
    buf.push_usize(ip.0[3] as usize);
}

/// Kontrol Paneli içeriğini çizer.
pub fn render_content(
    fb: &mut Framebuffer,
    win_x: usize,
    win_y: usize,
    win_w: usize,
    win_h: usize,
    now: &DateTime,
    total_ram_bytes: u64,
    used_ram_bytes: u64,
    kbd_layout: KeyboardLayout,
) {
    poll_network_test();
    let names_tr = ["Sistem", "Ağ", "Hakkında"];
    let names_en = ["System", "Network", "About"];

    for i in 0..TAB_COUNT {
        let r = tab_rect(win_x, win_y, win_w, i);
        let is_active = i == active_tab();
        fb.draw_rect(
            r.0,
            r.1,
            r.2,
            r.3,
            if is_active { Color::TITLE_BAR } else { Color::TITLE_BAR_INACTIVE },
        );
        let name = crate::gui::ui_text(names_tr[i], names_en[i]);
        let tw = MinGuiCompositor::text_width(name, 1);
        let th = MinGuiCompositor::text_height();
        MinGuiCompositor::draw_string(
            fb,
            r.0 + r.2.saturating_sub(tw) / 2,
            r.1 + r.3.saturating_sub(th) / 2,
            name,
            if is_active { Color::ACCENT_BLUE } else { Color::TEXT_MUTED },
        );
    }

    let panel_x = win_x + 10;
    let panel_y = win_y + 62 + zoom_extra();
    let panel_w = win_w.saturating_sub(20);
    let panel_h = win_h.saturating_sub(62 + zoom_extra() + 52);
    fb.draw_rect(panel_x, panel_y, panel_w, panel_h, Color::TITLE_BAR_INACTIVE);

    let left = panel_x + 16;
    let row_w = panel_w.saturating_sub(32);
    let mut y = panel_y + 12;
    let step = 24 + zoom_extra();

    match active_tab() {
        0 => {
            draw_row(fb, left, y, row_w, crate::gui::ui_text("İşletim Sistemi:", "Operating System:"),
                concat!("minOs v", env!("CARGO_PKG_VERSION"), " (x86_64)"), Color::TEXT_MAIN);
            y += step;
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Çekirdek:", "Kernel:"),
                "Rust no_std • Limine v8", Color::TEXT_MAIN);
            y += step;

            let mut cpu = TextBuf::new();
            cpu.push_str(crate::boot::cpu::cpu_vendor());
            cpu.push_str(" • ");
            cpu.push_usize(crate::boot::cpu::cpu_logical_count() as usize);
            cpu.push_str(crate::gui::ui_text(" mantıksal çekirdek", " logical cores"));
            draw_row(fb, left, y, row_w, crate::gui::ui_text("İşlemci:", "Processor:"), cpu.as_str(), Color::ACCENT_GREEN);
            y += step;

            let total_mb = total_ram_bytes / (1024 * 1024);
            let used_mb = used_ram_bytes / (1024 * 1024);
            let percent = if total_ram_bytes > 0 {
                (used_ram_bytes.saturating_mul(100) / total_ram_bytes) as usize
            } else {
                0
            };
            let mut ram = TextBuf::new();
            ram.push_usize(used_mb as usize);
            ram.push_str(" / ");
            ram.push_usize(total_mb as usize);
            ram.push_str(" MB (%");
            ram.push_usize(percent);
            ram.push_str(")");
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Bellek:", "Memory:"), ram.as_str(), Color::TEXT_MAIN);
            y += step;

            // Bellek doluluk çubuğu.
            let bar_x = left + crate::gui::sp(150);
            let bar_space = (panel_x + panel_w).saturating_sub(16 + bar_x);
            let bar_w = row_w.min(360).min(bar_space);
            fb.draw_rect(bar_x, y - 4, bar_w, 8, Color::DARK_BG);
            let fill = bar_w.saturating_mul(percent.min(100)) / 100;
            let bar_color = if percent >= 90 { Color::ACCENT_RED } else if percent >= 70 { Color::ACCENT_YELLOW } else { Color::ACCENT_GREEN };
            fb.draw_rect(bar_x, y - 4, fill, 8, bar_color);
            y += step;

            let mut screen = TextBuf::new();
            screen.push_usize(fb.width);
            screen.push_str("x");
            screen.push_usize(fb.height);
            screen.push_str(", 32-bit");
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Ekran:", "Display:"), screen.as_str(), Color::TEXT_MAIN);
            y += step;

            draw_row(fb, left, y, row_w, crate::gui::ui_text("Tema:", "Theme:"),
                crate::gui::theme::current().name(), Color::ACCENT_PINK);
            y += step;

            let kb = match kbd_layout {
                KeyboardLayout::TurkishQ => crate::gui::ui_text("Türkçe Q (TR)", "Turkish Q (TR)"),
                KeyboardLayout::EnglishUS => crate::gui::ui_text("İngilizce (US)", "English (US)"),
            };
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Klavye:", "Keyboard:"), kb, Color::TEXT_MAIN);
            y += step;

            let mut info = TextBuf::new();
            push_uptime(&mut info, (crate::monotonic_ms() / 1000) as usize);
            info.push_str(" • ");
            push_date(&mut info, now);
            info.push_str(" ");
            push_time(&mut info, now);
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Süre / Tarih:", "Uptime / Date:"), info.as_str(), Color::TEXT_MAIN);
        }
        1 => {
            let nic_ok = crate::drivers::e1000::is_ready();
            let mut nic = TextBuf::new();
            if nic_ok {
                nic.push_str(crate::drivers::e1000::nic_name());
                nic.push_str(crate::gui::ui_text(" (hazır)", " (ready)"));
            } else {
                nic.push_str(crate::gui::ui_text("Sürücü bulunamadı", "Driver not found"));
            }
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Ağ sürücüsü:", "Network driver:"), nic.as_str(),
                if nic_ok { Color::ACCENT_GREEN } else { Color::ACCENT_RED });
            y += step;

            let link = if !nic_ok {
                crate::gui::ui_text("Yok", "None")
            } else if crate::drivers::e1000::link_up() {
                crate::gui::ui_text("Bağlı (link up)", "Connected (link up)")
            } else {
                crate::gui::ui_text("Kopuk (link down)", "Disconnected (link down)")
            };
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Bağlantı:", "Link:"), link,
                if nic_ok && crate::drivers::e1000::link_up() { Color::ACCENT_GREEN } else { Color::ACCENT_RED });
            y += step;

            let ready = crate::drivers::netstack::dhcp_ready();
            let mut ip = TextBuf::new();
            if nic_ok {
                push_ip(&mut ip, crate::drivers::netstack::local_ip());
            } else {
                ip.push_str("—");
            }
            draw_row(fb, left, y, row_w, crate::gui::ui_text("IP adresi:", "IP address:"), ip.as_str(), Color::TEXT_MAIN);
            y += step;

            let mut mask = TextBuf::new();
            push_ip(&mut mask, crate::drivers::netstack::subnet_mask());
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Alt ağ maskesi:", "Subnet mask:"), mask.as_str(), Color::TEXT_MAIN);
            y += step;

            let mut gw = TextBuf::new();
            push_ip(&mut gw, crate::drivers::netstack::gateway_ip());
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Ağ geçidi:", "Gateway:"), gw.as_str(), Color::TEXT_MAIN);
            y += step;

            let mut dns = TextBuf::new();
            push_ip(&mut dns, crate::drivers::netstack::dns_server());
            draw_row(fb, left, y, row_w, crate::gui::ui_text("DNS sunucusu:", "DNS server:"), dns.as_str(), Color::TEXT_MAIN);
            y += step;

            draw_row(fb, left, y, row_w, crate::gui::ui_text("DHCP:", "DHCP:"),
                if ready { crate::gui::ui_text("Hazır", "Ready") } else { crate::gui::ui_text("Bekliyor", "Pending") },
                if ready { Color::ACCENT_GREEN } else { Color::ACCENT_YELLOW });
            y += step;

            let (audio, audio_ok) = if crate::drivers::hda::is_ready() {
                if crate::drivers::hda::is_dma_progressing() {
                    (crate::gui::ui_text("Intel HDA (DMA aktif)", "Intel HDA (DMA active)"), true)
                } else {
                    (crate::gui::ui_text("Intel HDA (DMA beklemede)", "Intel HDA (DMA standby)"), true)
                }
            } else {
                (crate::gui::ui_text("PC Speaker (PWM yedeği)", "PC Speaker (PWM fallback)"), false)
            };
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Ses:", "Audio:"), audio,
                if audio_ok { Color::ACCENT_GREEN } else { Color::ACCENT_YELLOW });
            y += step;

            let (net_test, net_color) = match NET_TEST_STATE.load(Ordering::Relaxed) {
                1 => (crate::gui::ui_text("Ağ testi: yanıt bekleniyor...", "Network test: waiting for reply..."), Color::ACCENT_YELLOW),
                2 => (crate::gui::ui_text("Ağ testi: BAŞARILI (ICMP yanıtı alındı)", "Network test: SUCCESS (ICMP reply received)"), Color::ACCENT_GREEN),
                3 => (crate::gui::ui_text("Ağ testi: BAŞARISIZ (yanıt yok)", "Network test: FAILED (no reply)"), Color::ACCENT_RED),
                _ => (crate::gui::ui_text("Ağ testi: hazır", "Network test: ready"), Color::TEXT_MUTED),
            };
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Test:", "Test:"), net_test, net_color);
        }
        _ => {
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Sürüm:", "Version:"),
                concat!("minOs v", env!("CARGO_PKG_VERSION")), Color::TEXT_MAIN);
            y += step;
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Lisans:", "License:"),
                crate::gui::ui_text("Açık kaynak (eğitim/araştırma)", "Open source (education/research)"), Color::TEXT_MAIN);
            y += step;
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Kaynak:", "Source:"),
                "github.com/tbagriyanik/minos", Color::ACCENT_BLUE);
            y += step;
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Kısayollar:", "Shortcuts:"),
                crate::gui::ui_text("F5 yenile • Super+R çalıştır • Super+D masaüstü", "F5 refresh • Super+R run • Super+D desktop"), Color::TEXT_MAIN);
            y += step;
            draw_row(fb, left, y, row_w, "",
                crate::gui::ui_text("Alt+Tab pencere • Ctrl+R sayfayı yenile", "Alt+Tab windows • Ctrl+R reload page"), Color::TEXT_MUTED);
            y += step;
            draw_row(fb, left, y, row_w, crate::gui::ui_text("Kılavuz:", "Docs:"),
                "docs/usage.md • README.md", Color::TEXT_MUTED);
        }
    }

    // Yenile ve Ağ Testi kısayolları (her sekmede görünür).
    let refresh_btn = refresh_btn_rect(win_x, win_y, win_w, win_h);
    fb.draw_rect(refresh_btn.0, refresh_btn.1, refresh_btn.2, refresh_btn.3, Color::WINDOW_BG);
    fb.draw_rect_outline(refresh_btn.0, refresh_btn.1, refresh_btn.2, refresh_btn.3, Color::BORDER);
    let refresh_label = crate::gui::ui_text("Yenile", "Refresh");
    MinGuiCompositor::draw_string(fb, refresh_btn.0 + 10, refresh_btn.1 + 7, refresh_label, Color::TEXT_MAIN);

    let net_btn = net_test_btn_rect(win_x, win_y, win_w, win_h);
    fb.draw_rect(net_btn.0, net_btn.1, net_btn.2, net_btn.3, Color::ACCENT_GREEN);
    let net_label = crate::gui::ui_text("Ağ Testi", "Network Test");
    let ntw = MinGuiCompositor::text_width(net_label, 1);
    MinGuiCompositor::draw_string(fb, net_btn.0 + net_btn.2.saturating_sub(ntw) / 2, net_btn.1 + 7, net_label,
        MinGuiCompositor::text_on(Color::ACCENT_GREEN));

    // Tema değiştirme düğmesi (her sekmede görünür).
    let b = theme_btn_rect(win_x, win_y, win_w, win_h);
    fb.draw_rect(b.0, b.1, b.2, b.3, Color::ACCENT_BLUE);
    let label = crate::gui::ui_text("Temayı Değiştir", "Change Theme");
    let tw = MinGuiCompositor::text_width(label, 1);
    let th = MinGuiCompositor::text_height();
    MinGuiCompositor::draw_string(
        fb,
        b.0 + b.2.saturating_sub(tw) / 2,
        b.1 + b.3.saturating_sub(th) / 2,
        label,
        MinGuiCompositor::text_on(Color::ACCENT_BLUE),
    );
}
