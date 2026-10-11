use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::mm::pmm::PhysicalMemoryManager;
use crate::fs::vfs::VirtualFileSystem;

/// Pencere yöneticisindeki uygulama yuvası sayısı.
pub const MAX_PROC: usize = 11;
pub const LAUNCH_APP_COUNT: usize = 11;
const LAUNCH_APP_IDS: [usize; LAUNCH_APP_COUNT] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const LAUNCH_APP_LABELS: [&str; LAUNCH_APP_COUNT] = [
    "Dosya Yöneticisi", "Sistem Ayarları", "Not Defteri", "Web Gezgini",
    "Resim Gösterici", "Müzik Çalar", "Video Oynatıcı", "Görev Yöneticisi",
    "Hesap Makinesi", "Çizim Tahtası", "Komut İstemi",
];

#[derive(Copy, Clone, Default)]
pub struct ProcessInfo {
    pub app_idx: usize,
    pub name: &'static str,
    pub is_minimized: bool,
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
    /// Sonlandırma ve liste durumu mesajı.
    pub status: &'static str,
    pub launch_menu_open: bool,
    pub launch_selected: usize,
}

impl TaskManagerApp {
    pub const fn new() -> Self {
        Self {
            processes: [ProcessInfo { app_idx: 0, name: "", is_minimized: false }; MAX_PROC],
            proc_count: 0,
            selected: 0,
            total_memory_kb: 0, free_memory_kb: 0, used_memory_kb: 0,
            ramdisk_used: 0, ramdisk_files: 0,
            status: "Hazır",
            launch_menu_open: false,
            launch_selected: 0,
        }
    }

    pub fn launch_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(206), win_y + 30, 100, 18)
    }

    pub fn launch_menu_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(180), win_y + 52, 170, 228)
    }

    pub fn launch_item_rect(win_x: usize, win_y: usize, win_w: usize, index: usize) -> (usize, usize, usize, usize) {
        let (x, y, w, _) = Self::launch_menu_rect(win_x, win_y, win_w);
        (x + 4, y + 4 + index * 20, w - 8, 20)
    }

    pub fn launch_item_at(mx: usize, my: usize, win_x: usize, win_y: usize, win_w: usize) -> Option<usize> {
        for index in 0..LAUNCH_APP_COUNT {
            let rect = Self::launch_item_rect(win_x, win_y, win_w, index);
            if mx >= rect.0 && mx < rect.0 + rect.2 && my >= rect.1 && my < rect.1 + rect.3 {
                return Some(index);
            }
        }
        None
    }

    pub fn launch_app_id(&self) -> usize {
        LAUNCH_APP_IDS[self.launch_selected.min(LAUNCH_APP_COUNT - 1)]
    }

    pub fn select_launch_prev(&mut self) {
        self.launch_selected = if self.launch_selected == 0 { LAUNCH_APP_COUNT - 1 } else { self.launch_selected - 1 };
    }

    pub fn select_launch_next(&mut self) {
        self.launch_selected = (self.launch_selected + 1) % LAUNCH_APP_COUNT;
    }

    /// Açık pencere tablosundan çalışan uygulamaları ve gerçek sistem kullanımını alır.
    pub fn update(
        &mut self,
        pmm: &PhysicalMemoryManager,
        vfs: &VirtualFileSystem,
        windows: &[Option<crate::gui::Window>; MAX_PROC],
    ) {
        self.total_memory_kb = (pmm.total_memory_bytes / 1024) as usize;
        self.free_memory_kb = (pmm.free_memory_bytes / 1024) as usize;
        self.used_memory_kb = (pmm.used_memory_bytes / 1024) as usize;
        self.ramdisk_used = 0;
        self.ramdisk_files = 0;
        for node in vfs.nodes.iter().flatten() {
            self.ramdisk_used += node.size;
            self.ramdisk_files += 1;
        }
        let selected_app = self.processes.get(self.selected).map(|entry| entry.app_idx);
        let mut count = 0;
        for (app_idx, window) in windows.iter().enumerate() {
            if let Some(window) = window {
                if window.is_open {
                    self.processes[count] = ProcessInfo {
                        app_idx,
                        name: LAUNCH_APP_LABELS[app_idx],
                        is_minimized: window.is_minimized,
                    };
                    count += 1;
                }
            }
        }
        for entry in &mut self.processes[count..] {
            *entry = ProcessInfo::default();
        }
        self.proc_count = count;
        self.selected = selected_app
            .and_then(|app_idx| self.processes[..count].iter().position(|entry| entry.app_idx == app_idx))
            .unwrap_or_else(|| self.selected.min(count.saturating_sub(1)));
    }

    /// Canlı (alive) süreç sayısı.
    pub fn alive_count(&self) -> usize {
        self.proc_count
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

    pub fn selected_app_idx(&self) -> Option<usize> {
        self.processes.get(self.selected).filter(|_| self.selected < self.proc_count).map(|entry| entry.app_idx)
    }

    /// i. süreç satırının tıklanabilir dikdörtgeni (çizim ile birebir aynı).
    pub fn row_rect(win_x: usize, win_y: usize, win_w: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + 8, win_y + 66 + i * 18, win_w.saturating_sub(16), 16)
    }

    /// Fare konumunun denk geldiği süreç satırı indeksi.
    pub fn row_at(&self, mx: usize, my: usize, win_x: usize, win_y: usize, win_w: usize) -> Option<usize> {
        for i in 0..self.proc_count {
            let r = Self::row_rect(win_x, win_y, win_w, i);
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
        // Başlık ve süreç eylemleri
        let mut header = TextBuf::new();
        header.push_str("Uygulamalar: ");
        header.push_u2((self.alive_count().min(99)) as u32);
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 30, header.as_str(), Color::ACCENT_PINK);

        // "Sonlandır" düğmesi
        let br = Self::kill_btn_rect(win_x, win_y, win_w);
        fb.draw_rect(br.0, br.1, br.2, br.3, Color::ACCENT_RED);
        MinGuiCompositor::draw_string(fb, br.0 + 8, br.1 + 5, "Sonlandır", Color::WHITE);

        let launch_button = Self::launch_button_rect(win_x, win_y, win_w);
        fb.draw_rect(launch_button.0, launch_button.1, launch_button.2, launch_button.3, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, launch_button.0 + 8, launch_button.1 + 5, "Program Aç", Color::WHITE);

        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 50, "Uygulama", Color::TEXT_MUTED);
        MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(112), win_y + 50, "Durum", Color::TEXT_MUTED);
        fb.draw_rect(win_x + 10, win_y + 62, win_w.saturating_sub(20), 1, Color::BORDER);

        for (i, proc) in self.processes.iter().take(self.proc_count).enumerate() {
            let row = Self::row_rect(win_x, win_y, win_w, i);
            if i == self.selected { fb.draw_rect(row.0, row.1, row.2, row.3, Color::TITLE_BAR); }
            MinGuiCompositor::draw_string(fb, row.0 + 4, row.1 + 4, proc.name,
                if i == self.selected { Color::ACCENT_BLUE } else { Color::TEXT_MAIN });
            MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(112), row.1 + 4,
                if proc.is_minimized { "Küçültüldü" } else { "Çalışıyor" }, Color::ACCENT_GREEN);
        }

        fb.draw_rect(win_x + 8, win_y + 270, win_w.saturating_sub(16), 1, Color::BORDER);
        let mut memory = TextBuf::new();
        memory.push_str("RAM: ");
        memory.push_usize(self.used_memory_kb);
        memory.push_str(" / ");
        memory.push_usize(self.total_memory_kb);
        memory.push_str(" KB  |  ");
        memory.push_str(self.status);
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 278, memory.as_str(), Color::TEXT_MUTED);

        if self.launch_menu_open {
            let (menu_x, menu_y, menu_w, menu_h) = Self::launch_menu_rect(win_x, win_y, win_w);
            fb.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::TASKBAR_BG);
            fb.draw_rect_outline(menu_x, menu_y, menu_w, menu_h, Color::BORDER);
            for index in 0..LAUNCH_APP_COUNT {
                let item = Self::launch_item_rect(win_x, win_y, win_w, index);
                let color = if index == self.launch_selected { Color::ACCENT_BLUE } else { Color::WINDOW_BG };
                fb.draw_rect(item.0, item.1, item.2, item.3, color);
                MinGuiCompositor::draw_string(fb, item.0 + 5, item.1 + 5, LAUNCH_APP_LABELS[index], Color::TEXT_MAIN);
            }
        }
    }
}
