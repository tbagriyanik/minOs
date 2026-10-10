use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor};

pub struct ProcessInfo {
    pub pid: usize,
    pub name: &'static str,
    pub memory_kb: usize,
    pub cpu_usage_pct: u8,
}

pub struct TaskManagerApp {
    pub processes: [ProcessInfo; 4],
}

impl TaskManagerApp {
    pub const fn new() -> Self {
        Self {
            processes: [
                ProcessInfo { pid: 1, name: "kernel_core", memory_kb: 4096, cpu_usage_pct: 1 },
                ProcessInfo { pid: 2, name: "mingui_compositor", memory_kb: 8192, cpu_usage_pct: 3 },
                ProcessInfo { pid: 3, name: "net_stack", memory_kb: 2048, cpu_usage_pct: 0 },
                ProcessInfo { pid: 4, name: "audio_mixer", memory_kb: 1024, cpu_usage_pct: 0 },
            ],
        }
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize) {
        // Tablo Başlıkları
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 35, "PID   NAME               RAM (KB)   CPU", Color::TEXT_MUTED);
        
        let mut row_y = win_y + 55;
        for proc in self.processes.iter() {
            MinGuiCompositor::draw_string(fb, win_x + 10, row_y, proc.name, Color::TEXT_MAIN);
            
            // CPU kullanım çubuğu
            let bar_w = (proc.cpu_usage_pct as usize) * 20;
            fb.draw_rect(win_x + 280, row_y + 2, bar_w.max(6), 6, Color::ACCENT_GREEN);
            
            row_y += 20;
        }

        // Alt Durum Bilgisi (Toplam RAM ve CPU)
        fb.draw_rect(win_x + 10, win_y + 160, 360, 2, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 170, "Total RAM: 15.3MB / 128MB (12%)", Color::ACCENT_BLUE);
    }
}
