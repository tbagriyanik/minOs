//! Developer Tools (DevTools) panel for the minOs browser.
//!
//! Provides an in-browser developer tools interface with four tabs:
//! - Elements: DOM/tree inspector for page elements (links, images, canvases, iframes, media, wasm, workers)
//! - Console: JavaScript console log output (error_logs, js_alert)
//! - Network: HTTP request/response monitoring (downloads, request state)
//! - Storage: Cookie, localStorage, form autofill inspection

extern crate alloc;
use alloc::format;

use crate::gui::Color;

/// DevTools tab selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevToolsTab {
    Elements,
    Console,
    Network,
    Storage,
}

/// DevTools state: whether open and which tab is active.
#[derive(Clone, Debug)]
pub struct DevToolsState {
    pub open: bool,
    pub active_tab: DevToolsTab,
    /// Scroll offset within the DevTools content area.
    pub scroll_y: usize,
}

impl DevToolsState {
    pub fn new() -> Self {
        DevToolsState {
            open: false,
            active_tab: DevToolsTab::Elements,
            scroll_y: 0,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.scroll_y = 0;
        }
    }

    pub fn cycle_tab(&mut self) {
        self.active_tab = match self.active_tab {
            DevToolsTab::Elements => DevToolsTab::Console,
            DevToolsTab::Console => DevToolsTab::Network,
            DevToolsTab::Network => DevToolsTab::Storage,
            DevToolsTab::Storage => DevToolsTab::Elements,
        };
        self.scroll_y = 0;
    }
}

/// Render the DevTools panel to the framebuffer.
/// `fb` - framebuffer to draw on
/// `dx, dy` - top-left position of the DevTools panel
/// `dw, dh` - width and height of the DevTools panel
/// `state` - current DevTools state
/// `browser` - reference to BrowserApp for inspecting data
pub fn render_devtools(
    fb: &mut crate::drivers::video::Framebuffer,
    dx: usize,
    dy: usize,
    dw: usize,
    dh: usize,
    state: &DevToolsState,
    browser: &crate::apps::browser::BrowserApp,
) {
    if !state.open {
        return;
    }

    // Draw DevTools panel background and border
    fb.draw_rect(dx, dy, dw, dh, 0x1E1E1E);
    fb.draw_rect_outline(dx, dy, dw, dh, Color::ACCENT_BLUE);

    // Draw tab bar
    let tab_h = 20;
    let tab_w = dw / 4;
    let tabs = [
        ("Elements", DevToolsTab::Elements),
        ("Console", DevToolsTab::Console),
        ("Network", DevToolsTab::Network),
        ("Storage", DevToolsTab::Storage),
    ];

    for (i, (label, tab)) in tabs.iter().enumerate() {
        let tx = dx + i * tab_w;
        let is_active = state.active_tab == *tab;
        let tab_bg = if is_active { 0x2D2D2D } else { 0x252525 };
        let tab_fg = if is_active {
            Color::ACCENT_BLUE
        } else {
            Color::TEXT_MUTED
        };

        fb.draw_rect(tx, dy + tab_h, tab_w, tab_h, tab_bg);
        if i > 0 {
            fb.draw_rect(tx, dy + tab_h, 1, tab_h, Color::BORDER);
        }
        crate::gui::MinGuiCompositor::draw_string_clipped(
            fb,
            tx + 6,
            dy + tab_h + 4,
            *label,
            tab_fg,
            (
                tx + 6,
                dy + tab_h + 2,
                tab_w.saturating_sub(12),
                tab_h.saturating_sub(4),
            ),
        );
    }

    // Draw content area
    let content_y = dy + tab_h * 2;
    let content_h = dh.saturating_sub(tab_h * 2);
    let content_x = dx + 4;
    let content_w = dw.saturating_sub(8);

    // Draw content background
    fb.draw_rect(content_x, content_y, content_w, content_h, 0x1A1A1A);

    let mut cy = content_y + 4;

    match state.active_tab {
        DevToolsTab::Elements => {
            // Elements inspector: list page elements
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                "Elements Inspector",
                Color::ACCENT_BLUE,
                (content_x, cy, content_w, 16),
            );
            cy += 20;

            // Links
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Links: {}", browser.rendered_links.borrow().len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // Images
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Images: {}", browser.rendered_images.borrow().len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // Canvases
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Canvases: {}", browser.canvases.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // Media players
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Media Players: {}", browser.media_players.players.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // Iframes
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Iframes: {}", browser.iframes.frames.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // WASM modules
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("WASM Modules: {}", browser.wasm_modules.modules.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // Workers
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Workers: {}", browser.workers.workers.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // Page lines count
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Page Lines: {}", browser.page_lines.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
        }

        DevToolsTab::Console => {
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                "Console",
                Color::ACCENT_BLUE,
                (content_x, cy, content_w, 16),
            );
            cy += 20;

            // JS alert
            if let Some(ref alert) = browser.js_alert {
                crate::gui::MinGuiCompositor::draw_string_clipped(
                    fb,
                    content_x,
                    cy,
                    &format!("alert: {}", alert),
                    Color::ACCENT_YELLOW,
                    (content_x, cy, content_w, 16),
                );
                cy += 18;
            }

            // Error logs
            let log_count = browser.error_logs.len();
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Error Logs: {}", log_count),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // Show last few error logs
            let start = if log_count > 10 { log_count - 10 } else { 0 };
            for log in browser.error_logs.iter().skip(start).take(10) {
                let truncated = if log.len() > 60 {
                    format!("{}...", &log[..57])
                } else {
                    log.clone()
                };
                crate::gui::MinGuiCompositor::draw_string_clipped(
                    fb,
                    content_x,
                    cy,
                    &truncated,
                    Color::ACCENT_RED,
                    (content_x, cy, content_w, 16),
                );
                cy += 16;
                if cy + 16 > content_y + content_h {
                    break;
                }
            }
        }

        DevToolsTab::Network => {
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                "Network",
                Color::ACCENT_BLUE,
                (content_x, cy, content_w, 16),
            );
            cy += 20;

            // Request state
            let state_str = match browser.request_state {
                0 => "Idle",
                1 => "Connecting",
                2 => "Receiving",
                3 => "TLS Handshake",
                4 => "DNS Resolving",
                _ => "Unknown",
            };
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Request State: {}", state_str),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            // Downloads
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Downloads: {}", browser.downloads.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            for dl in browser.downloads.iter().take(8) {
                let progress = if dl.total_bytes > 0 {
                    dl.received_bytes * 100 / dl.total_bytes
                } else {
                    0
                };
                let status_str = match dl.status {
                    crate::apps::browser::DownloadStatus::Queued => "Queued",
                    crate::apps::browser::DownloadStatus::Downloading => "Downloading",
                    crate::apps::browser::DownloadStatus::Completed => "Completed",
                    crate::apps::browser::DownloadStatus::Failed => "Failed",
                    crate::apps::browser::DownloadStatus::Cancelled => "Cancelled",
                };
                let line = format!("{} {}% {}", dl.file_name, progress, status_str);
                let truncated = if line.len() > 50 {
                    format!("{}...", &line[..47])
                } else {
                    line
                };
                crate::gui::MinGuiCompositor::draw_string_clipped(
                    fb,
                    content_x,
                    cy,
                    &truncated,
                    Color::TEXT_MAIN,
                    (content_x, cy, content_w, 16),
                );
                cy += 16;
                if cy + 16 > content_y + content_h {
                    break;
                }
            }
        }

        DevToolsTab::Storage => {
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                "Storage",
                Color::ACCENT_BLUE,
                (content_x, cy, content_w, 16),
            );
            cy += 20;

            // Cookies
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Cookies: {}", browser.cookies.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            for cookie in browser.cookies.iter().take(8) {
                let line = format!("{}={}", cookie.name, cookie.value);
                let truncated = if line.len() > 50 {
                    format!("{}...", &line[..47])
                } else {
                    line
                };
                crate::gui::MinGuiCompositor::draw_string_clipped(
                    fb,
                    content_x,
                    cy,
                    &truncated,
                    Color::TEXT_MAIN,
                    (content_x, cy, content_w, 16),
                );
                cy += 16;
                if cy + 16 > content_y + content_h {
                    break;
                }
            }

            cy += 8;

            // Form autofill
            crate::gui::MinGuiCompositor::draw_string_clipped(
                fb,
                content_x,
                cy,
                &format!("Form Autofill: {}", browser.form_autofill.entries.len()),
                Color::TEXT_MAIN,
                (content_x, cy, content_w, 16),
            );
            cy += 18;

            for entry in browser.form_autofill.entries.iter().take(8) {
                let line = format!(
                    "{}: {} = {}",
                    entry.domain, entry.field_name, entry.field_value
                );
                let truncated = if line.len() > 50 {
                    format!("{}...", &line[..47])
                } else {
                    line
                };
                crate::gui::MinGuiCompositor::draw_string_clipped(
                    fb,
                    content_x,
                    cy,
                    &truncated,
                    Color::TEXT_MAIN,
                    (content_x, cy, content_w, 16),
                );
                cy += 16;
                if cy + 16 > content_y + content_h {
                    break;
                }
            }
        }
    }
}
