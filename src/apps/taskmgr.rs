use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::mm::pmm::PhysicalMemoryManager;
use crate::fs::vfs::VirtualFileSystem;

/// Sabit süreç tablosu kapasitesi.
pub const MAX_PROC: usize = 6;

#[derive(Copy, Clone, Default)]
pub struct ProcessInfo {
    pub pid: usize,
    pub name: &'static str,
    pub memory_kb: usize,
    pub cpu_usage_pct: u8,
    /// Süreç aktif mi (kill edilirse false olur).
    pub alive: bool,
}

pub struct TaskManagerApp {
    pub processes: [ProcessInfo; MAX_PROC],
    /// Kaç süreç slotu dolu (canlı olan + ölü olan).
    pub proc_count: usize,
    /// Seçili süreç indeksi (ok tuşları / fare ile değişir).
    pub selected: usize,
    pub total_memory_kb: usize,
    pub free_memory_kb: usize,
    pub used_memory_kb: usize,
    pub ramdisk_used: usize,
    pub ramdisk_files: usize,
    /// CPU dalgalanması için animasyon sayacı.
    pub tick: usize,
    /// Son bilinen toplam CPU kullanımı (yüzde).
    pub total_cpu_pct: u8,
    /// Sürekli artan yukarı/aşağı animasyon için yön.
    pub trend: i8,
    /// Durum mesajı (ör. "PID 4 sonlandirildi").
    pub status: &'static str,
}

impl TaskManagerApp {
    pub const fn new() -> Self {
        Self {
            processes: [
                ProcessInfo { pid: 1, name: "kernel_core", memory_kb: 4096, cpu_usage_pct: 1, alive: true },
                ProcessInfo { pid: 2, name: "mingui_compositor", memory_kb: 8192, cpu_usage_pct: 3, alive: true },
                ProcessInfo { pid: 3, name: "net_stack", memory_kb: 2048, cpu_usage_pct: 0, alive: true },
                ProcessInfo { pid: 4, name: "audio_mixer", memory_kb: 1024, cpu_usage_pct: 0, alive: true },
                ProcessInfo { pid: 5, name: "minviewer", memory_kb: 1536, cpu_usage_pct: 1, alive: true },
                ProcessInfo { pid: 6, name: "minshell", memory_kb: 768, cpu_usage_pct: 0, alive: true },
            ],
            proc_count: 6,
            selected: 0,
            total_memory_kb: 0, free_memory_kb: 0, used_memory_kb: 0,
            ramdisk_used: 0, ramdisk_files: 0,
            tick: 0, total_cpu_pct: 0, trend: 1,
            status: "Hazır",
        }
    }

    /// Canlı güncelleme: PMM/VFS'den gerçek bellek, süreçlere CPU dalgalanması uygular.
    pub fn update(&mut self, pmm: &PhysicalMemoryManager, vfs: &VirtualFileSystem) {
        self.total_memory_kb = (pmm.total_memory_bytes / 1024) as usize;
        self.free_memory_kb = (pmm.free_memory_bytes / 1024) as usize;
        self.used_memory_kb = (pmm.used_memory_bytes / 1024) as usize;
        self.ramdisk_used = 0;
        self.ramdisk_files = 0;
        for node in vfs.nodes.iter().flatten() {
            self.ramdisk_used += node.size;
            self.ramdisk_files += 1;
        }
        self.processes[0].memory_kb = self.used_memory_kb.max(4096);
        self.processes[1].memory_kb = self.ramdisk_used / 1024 + 8192;

        // Canlı CPU dalgalanması (deterministik ama sürekli değişen değerler).
        self.tick = self.tick.wrapping_add(1);
        let mut total: u32 = 0;
        for (i, p) in self.processes.iter_mut().enumerate() {
            if !p.alive {
                p.cpu_usage_pct = 0;
                continue;
            }
            let phase = (self.tick.wrapping_mul(7) + i * 13) % 100;
            let base = match i {
                0 => 3,
                1 => 8,
                2 => 2,
                3 => 1,
                4 => 4,
                _ => 2,
            };
            p.cpu_usage_pct = (base + (phase / 25)) as u8;
            total += p.cpu_usage_pct as u32;
        }
        self.total_cpu_pct = total.min(100) as u8;
    }

    /// Canlı (alive) süreç sayısı.
    pub fn alive_count(&self) -> usize {
        self.processes.iter().take(self.proc_count).filter(|p| p.alive).count()
    }

    /// Seçimi yukarı taşır.
    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Seçimi aşağı taşır.
    pub fn select_down(&mut self) {
        if self.selected + 1 < self.proc_count {
            self.selected += 1;
        }
    }

    /// Seçili süreci sonlandırır (kill). Çekirdek (PID 1) korunur.
    pub fn kill_selected(&mut self) {
        let i = self.selected;
        if i >= self.proc_count {
            return;
        }
        if self.processes[i].pid == 1 {
            self.status = "kernel_core sonlandırılamaz (korumalı)";
            return;
        }
        if self.processes[i].alive {
            self.processes[i].alive = false;
            self.processes[i].cpu_usage_pct = 0;
            self.status = "Süreç sonlandırıldı.";
        } else {
            self.status = "Süreç zaten kapalı.";
        }
    }

    /// i. süreç satırının tıklanabilir dikdörtgeni (çizim ile birebir aynı).
    pub fn row_rect(win_x: usize, win_y: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + 8, win_y + 64 + i * 20, 374, 18)
    }

    /// Fare konumunun denk geldiği süreç satırı indeksi.
    pub fn row_at(&self, mx: usize, my: usize, win_x: usize, win_y: usize) -> Option<usize> {
        for i in 0..self.proc_count {
            let r = Self::row_rect(win_x, win_y, i);
            if mx >= r.0 && mx < r.0 + r.2 && my >= r.1 && my < r.1 + r.3 {
                return Some(i);
            }
        }
        None
    }

    /// "Sonlandır" düğmesinin dikdörtgeni (pencerenin sağ-alt bölümü).
    pub fn kill_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(96), win_y + 30, 86, 18)
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize) {
        // Başlık + üst özet bandı
        let mut header = TextBuf::new();
        header.push_str("Görevler: ");
        header.push_u2((self.alive_count().min(99)) as u32);
        header.push_str("  CPU: %");
        header.push_u2(self.total_cpu_pct as u32);
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 30, header.as_str(), Color::ACCENT_PINK);

        // "Sonlandır" düğmesi
        let br = Self::kill_btn_rect(win_x, win_y, win_w);
        fb.draw_rect(br.0, br.1, br.2, br.3, Color::ACCENT_RED);
        MinGuiCompositor::draw_string(fb, br.0 + 8, br.1 + 5, "Sonlandır", Color::WHITE);

        // Tablo Başlıkları
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 48, "PID  İSİM               RAM(KB)  CPU", Color::TEXT_MUTED);
        fb.draw_rect(win_x + 10, win_y + 60, 370, 1, Color::BORDER);

        let mut row_y = win_y + 66;
        for (i, proc) in self.processes.iter().take(self.proc_count).enumerate() {
            // Seçili satırı vurgula.
            if i == self.selected {
                fb.draw_rect(win_x + 8, row_y - 2, 374, 16, Color::TITLE_BAR);
            }
            let (name_col, cpu_col) = if !proc.alive {
                (Color::TEXT_MUTED, Color::TEXT_MUTED)
            } else if i == self.selected {
                (Color::ACCENT_BLUE, Color::ACCENT_GREEN)
            } else {
                (Color::TEXT_MAIN, Color::ACCENT_GREEN)
            };

            // PID + isim
            let mut name = TextBuf::new();
            name.push_u2(proc.pid as u32);
            name.push_byte(b' ');
            name.push_str(proc.name);
            if !proc.alive { name.push_str(" (kapalı)"); }
            MinGuiCompositor::draw_string(fb, win_x + 10, row_y, name.as_str(), name_col);

            // RAM (KB)
            let mut ram = TextBuf::new();
            ram.push_usize(proc.memory_kb);
            MinGuiCompositor::draw_string(fb, win_x + 190, row_y, ram.as_str(), Color::TEXT_MUTED);

            // CPU kullanım çubuğu (ölçek: her %1 = 1px, maks 100px)
            let bar_w = (proc.cpu_usage_pct as usize).min(100);
            let bar_col = if proc.cpu_usage_pct > 8 { Color::ACCENT_RED } else { cpu_col };
            fb.draw_rect(win_x + 280, row_y + 3, bar_w.max(if proc.alive { 2 } else { 0 }), 6, bar_col);

            row_y += 20;
        }

        // RAM kullanım grafiği (çubuk)
        let graph_y = win_y + 160;
        fb.draw_rect(win_x + 10, graph_y, 370, 1, Color::TITLE_BAR);
        MinGuiCompositor::draw_string(fb, win_x + 10, graph_y + 6, crate::gui::ui_text("Bellek Kullanımı","Memory Usage"), Color::ACCENT_BLUE);
        let gfx = win_x + 10;
        let gfy = graph_y + 20;
        let gw = 370usize;
        fb.draw_rect(gfx, gfy, gw, 10, Color::TITLE_BAR_INACTIVE);
        let used_ratio = if self.total_memory_kb > 0 {
            (self.used_memory_kb.saturating_mul(gw)) / self.total_memory_kb
        } else { 0 };
        fb.draw_rect(gfx, gfy, used_ratio.min(gw), 10, Color::ACCENT_GREEN);
        fb.draw_rect_outline(gfx, gfy, gw, 10, Color::BORDER);

        // RAM metni
        let mut info = TextBuf::new();
        info.push_str("RAM: ");
        info.push_usize(self.used_memory_kb);
        info.push_str(" / ");
        info.push_usize(self.total_memory_kb);
        info.push_str(" KB");
        MinGuiCompositor::draw_string(fb, win_x + 10, graph_y + 36, info.as_str(), Color::TEXT_MAIN);

        let mut disk = TextBuf::new();
        disk.push_str("VFS: ");
        disk.push_usize(self.ramdisk_files);
        disk.push_str(" dosya / ");
        disk.push_usize(self.ramdisk_used);
        disk.push_str(" B");
        MinGuiCompositor::draw_string(fb, win_x + 10, graph_y + 50, disk.as_str(), Color::TEXT_MUTED);

        // Durum + ipucu
        MinGuiCompositor::draw_string(fb, win_x + 10, graph_y + 64, self.status, Color::ACCENT_YELLOW);
    }
}
