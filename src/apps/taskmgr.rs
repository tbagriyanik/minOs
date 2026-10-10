use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::mm::pmm::PhysicalMemoryManager;
use crate::fs::vfs::VirtualFileSystem;

pub struct ProcessInfo {
    pub pid: usize,
    pub name: &'static str,
    pub memory_kb: usize,
    pub cpu_usage_pct: u8,
}

pub struct TaskManagerApp {
    pub processes: [ProcessInfo; 4],
    pub total_memory_kb: usize,
    pub free_memory_kb: usize,
    pub used_memory_kb: usize,
    pub ramdisk_used: usize,
    pub ramdisk_files: usize,
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
            total_memory_kb: 0, free_memory_kb: 0, used_memory_kb: 0,
            ramdisk_used: 0, ramdisk_files: 0,
        }
    }

    pub fn update(&mut self, pmm: &PhysicalMemoryManager, vfs: &VirtualFileSystem) {
        self.total_memory_kb = (pmm.total_memory_bytes / 1024) as usize;
        self.free_memory_kb = (pmm.free_memory_bytes / 1024) as usize;
        self.used_memory_kb = (pmm.used_memory_bytes / 1024) as usize;
        self.ramdisk_used = 0; self.ramdisk_files = 0;
        for node in vfs.nodes.iter().flatten() { self.ramdisk_used += node.size; self.ramdisk_files += 1; }
        self.processes[0].memory_kb = self.used_memory_kb.min(usize::MAX);
        self.processes[1].memory_kb = self.ramdisk_used / 1024 + 1;
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize) {
        // Tablo Başlıkları
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 35, crate::gui::ui_text("PID   ISIM              RAM (KB)   CPU","PID   NAME              RAM (KB)   CPU"), Color::TEXT_MUTED);
        
        let mut row_y = win_y + 55;
        for proc in self.processes.iter() {
            // Süreç adı — sabit boyutlu metin tamponu ile güvenli biçimde çizilir
            // (taşma/indeks paniklerini önler).
            let mut name = TextBuf::new();
            name.push_u2(proc.pid as u32);
            name.push_byte(b' ');
            name.push_str(proc.name);
            MinGuiCompositor::draw_string(fb, win_x + 10, row_y, name.as_str(), Color::TEXT_MAIN);

            // RAM (KB) değeri
            let mut ram = TextBuf::new();
            ram.push_usize(proc.memory_kb);
            MinGuiCompositor::draw_string(fb, win_x + 190, row_y, ram.as_str(), Color::TEXT_MUTED);

            // CPU kullanım çubuğu
            let bar_w = (proc.cpu_usage_pct as usize) * 20;
            fb.draw_rect(win_x + 280, row_y + 2, bar_w.max(6), 6, Color::ACCENT_GREEN);

            row_y += 20;
        }

        // Alt Durum Bilgisi (Toplam RAM ve CPU)
        fb.draw_rect(win_x + 10, win_y + 160, 360, 2, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 170, crate::gui::ui_text("RAM / VFS durumu","RAM / VFS status"), Color::ACCENT_BLUE);

        let mut info = TextBuf::new();
        info.push_str("RAM KB free/total: ");
        info.push_usize(self.free_memory_kb);
        info.push_byte(b'/');
        info.push_usize(self.total_memory_kb);
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 188, info.as_str(), Color::TEXT_MAIN);

        let mut disk = TextBuf::new();
        disk.push_str("RAM Disk: ");
        disk.push_usize(self.ramdisk_used);
        disk.push_str(" B / ");
        disk.push_usize(self.ramdisk_files);
        disk.push_str(" dosya");
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 206, disk.as_str(), Color::TEXT_MUTED);
    }
}
