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
const LAUNCH_APP_LABELS_EN: [&str; LAUNCH_APP_COUNT] = [
    "File Manager", "Settings", "Notepad", "Browser", "Image Viewer", "Music Player",
    "Video Player", "Task Manager", "Calculator", "Paint", "Command Prompt",
];
const CPU_USAGE_PCT: usize = 100;

#[derive(Copy, Clone, Default)]
pub struct ProcessInfo {
    pub app_idx: usize,
    pub name: &'static str,
    pub is_minimized: bool,
    pub cpu_pct: usize,
    pub memory_kb: usize,
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
    pub active_tab: usize,
    pub focused_control: usize,
}

impl TaskManagerApp {
    pub const fn new() -> Self {
        Self {
            processes: [ProcessInfo { app_idx: 0, name: "", is_minimized: false, cpu_pct: 0, memory_kb: 0 }; MAX_PROC],
            proc_count: 0,
            selected: 0,
            total_memory_kb: 0, free_memory_kb: 0, used_memory_kb: 0,
            ramdisk_used: 0, ramdisk_files: 0,
            status: "Ready",
            launch_menu_open: false,
            launch_selected: 0,
            active_tab: 0,
            focused_control: 0,
        }
    }

    pub fn focus_next_control(&mut self) {
        let focusable_count = 4 + if self.active_tab == 0 { self.proc_count } else { 0 };
        self.focused_control = (self.focused_control + 1) % focusable_count.max(1);
        self.launch_menu_open = false;
        if self.focused_control >= 4 {
            self.selected = self.focused_control - 4;
        }
    }

    pub fn applications_tab_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + 10, win_y + 52, 100, 22)
    }

    pub fn performance_tab_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + 114, win_y + 52, 100, 22)
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
        render_cycles: &[u64; MAX_PROC],
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
        let total_render_cycles = windows.iter().enumerate().fold(0u64, |total, (app_idx, window)| {
            if window.map_or(false, |window| window.is_open && !window.is_minimized) {
                total.saturating_add(render_cycles[app_idx])
            } else {
                total
            }
        });
        let mut count = 0;
        for (app_idx, window) in windows.iter().enumerate() {
            if let Some(window) = window {
                if window.is_open {
                    let state_bytes = Self::app_state_bytes(app_idx);
                    self.processes[count] = ProcessInfo {
                        app_idx,
                        name: LAUNCH_APP_LABELS[app_idx],
                        is_minimized: window.is_minimized,
                        cpu_pct: if window.is_minimized || total_render_cycles == 0 { 0 }
                            else { (render_cycles[app_idx].saturating_mul(100) / total_render_cycles).min(100) as usize },
                        memory_kb: state_bytes.saturating_add(1023) / 1024,
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

    fn app_state_bytes(app_idx: usize) -> usize {
        match app_idx {
            0 => core::mem::size_of::<crate::apps::filemanager::FileManagerApp>(),
            1 => core::mem::size_of::<crate::apps::settings::SystemSettings>(),
            2 => core::mem::size_of::<crate::apps::notepad::NotepadApp>(),
            3 => core::mem::size_of::<crate::apps::browser::BrowserApp>(),
            4 => core::mem::size_of::<crate::apps::imageviewer::ImageViewerApp>(),
            5 => core::mem::size_of::<crate::apps::musicplayer::MusicPlayerApp>(),
            6 => core::mem::size_of::<crate::apps::videoplayer::VideoPlayerApp>(),
            7 => core::mem::size_of::<crate::apps::taskmgr::TaskManagerApp>(),
            8 => core::mem::size_of::<crate::apps::calculator::CalculatorApp>(),
            9 => core::mem::size_of::<crate::apps::paint::PaintApp>(),
            10 => core::mem::size_of::<crate::apps::terminal::TerminalApp>(),
            _ => 0,
        }
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
        (win_x + 8, win_y + 100 + i * 18, win_w.saturating_sub(16), 16)
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
        let english = crate::gui::ui_english();
        // Başlık ve süreç eylemleri
        let mut header = TextBuf::new();
        header.push_str(crate::gui::ui_text("Uygulamalar: ", "Applications: "));
        header.push_u2((self.alive_count().min(99)) as u32);
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 30, header.as_str(), Color::ACCENT_PINK);

        // "Sonlandır" düğmesi
        let br = Self::kill_btn_rect(win_x, win_y, win_w);
        fb.draw_rect(br.0, br.1, br.2, br.3, Color::ACCENT_RED);
        MinGuiCompositor::draw_string_clipped(fb, br.0 + 8, br.1 + 5, crate::gui::ui_text("Sonlandır", "End Task"), Color::WHITE,
            (br.0 + 2, br.1 + 1, br.2.saturating_sub(4), br.3.saturating_sub(2)));

        let launch_button = Self::launch_button_rect(win_x, win_y, win_w);
        fb.draw_rect(launch_button.0, launch_button.1, launch_button.2, launch_button.3, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string_clipped(fb, launch_button.0 + 8, launch_button.1 + 5, crate::gui::ui_text("Program Aç", "Run Program"), Color::WHITE,
            (launch_button.0 + 2, launch_button.1 + 1, launch_button.2.saturating_sub(4), launch_button.3.saturating_sub(2)));

        let applications_tab = Self::applications_tab_rect(win_x, win_y);
        let performance_tab = Self::performance_tab_rect(win_x, win_y);
        fb.draw_rect(applications_tab.0, applications_tab.1, applications_tab.2, applications_tab.3,
            if self.active_tab == 0 { Color::ACCENT_BLUE } else { Color::TITLE_BAR_INACTIVE });
        MinGuiCompositor::draw_string_clipped(fb, applications_tab.0 + 12, applications_tab.1 + 7, crate::gui::ui_text("Uygulamalar", "Applications"), Color::TEXT_MAIN,
            (applications_tab.0 + 2, applications_tab.1 + 1, applications_tab.2.saturating_sub(4), applications_tab.3.saturating_sub(2)));
        fb.draw_rect(performance_tab.0, performance_tab.1, performance_tab.2, performance_tab.3,
            if self.active_tab == 1 { Color::ACCENT_BLUE } else { Color::TITLE_BAR_INACTIVE });
        MinGuiCompositor::draw_string_clipped(fb, performance_tab.0 + 12, performance_tab.1 + 7, crate::gui::ui_text("Performans", "Performance"), Color::TEXT_MAIN,
            (performance_tab.0 + 2, performance_tab.1 + 1, performance_tab.2.saturating_sub(4), performance_tab.3.saturating_sub(2)));
        if self.focused_control == 0 { fb.draw_rect_outline(applications_tab.0, applications_tab.1, applications_tab.2, applications_tab.3, Color::ACCENT_YELLOW); }
        if self.focused_control == 1 { fb.draw_rect_outline(performance_tab.0, performance_tab.1, performance_tab.2, performance_tab.3, Color::ACCENT_YELLOW); }
        if self.focused_control == 2 { fb.draw_rect_outline(launch_button.0, launch_button.1, launch_button.2, launch_button.3, Color::ACCENT_YELLOW); }
        if self.focused_control == 3 { fb.draw_rect_outline(br.0, br.1, br.2, br.3, Color::ACCENT_YELLOW); }

        if self.active_tab == 0 {
            MinGuiCompositor::draw_string(fb, win_x + 12, win_y + 82, crate::gui::ui_text("Uygulama", "App"), Color::TEXT_MUTED);
            MinGuiCompositor::draw_string(fb, win_x + 205, win_y + 82, crate::gui::ui_text("Çizim CPU", "Render CPU"), Color::TEXT_MUTED);
            MinGuiCompositor::draw_string(fb, win_x + 278, win_y + 82, crate::gui::ui_text("RAM durumu", "Memory"), Color::TEXT_MUTED);
            fb.draw_rect(win_x + 10, win_y + 94, win_w.saturating_sub(20), 1, Color::BORDER);

            for (i, proc) in self.processes.iter().take(self.proc_count).enumerate() {
                let row = Self::row_rect(win_x, win_y, win_w, i);
                if i == self.selected { fb.draw_rect(row.0, row.1, row.2, row.3, Color::TITLE_BAR); }
                if self.focused_control == 4 + i { fb.draw_rect_outline(row.0, row.1, row.2, row.3, Color::ACCENT_YELLOW); }
                let app_name = if english { LAUNCH_APP_LABELS_EN[proc.app_idx.min(LAUNCH_APP_COUNT - 1)] } else { proc.name };
                MinGuiCompositor::draw_string(fb, row.0 + 4, row.1 + 4, app_name,
                    if i == self.selected { Color::ACCENT_BLUE } else { Color::TEXT_MAIN });
                let mut cpu = TextBuf::new();
                cpu.push_usize(proc.cpu_pct);
                cpu.push_byte(b'%');
                MinGuiCompositor::draw_string(fb, win_x + 212, row.1 + 4, cpu.as_str(), Color::ACCENT_YELLOW);
                let mut ram = TextBuf::new();
                ram.push_usize(proc.memory_kb);
                ram.push_str(" KB");
                MinGuiCompositor::draw_string(fb, win_x + 278, row.1 + 4, ram.as_str(), Color::TEXT_MUTED);
                MinGuiCompositor::draw_string(fb, win_x + win_w.saturating_sub(76), row.1 + 4,
                    match (proc.is_minimized, english) { (true, true) => "Minimized", (false, true) => "Running", (true, false) => "Beklemede", (false, false) => "Açık" }, Color::ACCENT_GREEN);
            }
        } else {
            let bar_x = win_x + 132;
            let bar_w = win_w.saturating_sub(152);
            let mut cpu_label = TextBuf::new();
            cpu_label.push_str("CPU %");
            cpu_label.push_usize(CPU_USAGE_PCT);
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 100, cpu_label.as_str(), Color::TEXT_MAIN);
            fb.draw_rect(bar_x, win_y + 100, bar_w, 12, Color::TITLE_BAR_INACTIVE);
            fb.draw_rect(bar_x, win_y + 100, bar_w.saturating_mul(CPU_USAGE_PCT) / 100, 12, Color::ACCENT_RED);
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 120,
                crate::gui::ui_text("Uygulama payı son ekran çizim süresine göre hesaplanır.", "App share is estimated from the last frame render time."), Color::TEXT_MUTED);
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 136,
                crate::gui::ui_text("Çekirdek boşta döngüsü olmadığı için toplam CPU %100.", "Total CPU is 100% because the kernel has no idle loop."), Color::TEXT_MUTED);

            let memory_pct = if self.total_memory_kb == 0 { 0 }
                else { self.used_memory_kb.saturating_mul(100) / self.total_memory_kb };
            let mut memory_label = TextBuf::new();
            memory_label.push_str("RAM ");
            memory_label.push_usize(self.used_memory_kb);
            memory_label.push_str(" / ");
            memory_label.push_usize(self.total_memory_kb);
            memory_label.push_str(" KB");
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 170, memory_label.as_str(), Color::TEXT_MAIN);
            fb.draw_rect(bar_x, win_y + 170, bar_w, 12, Color::TITLE_BAR_INACTIVE);
            fb.draw_rect(bar_x, win_y + 170, bar_w.saturating_mul(memory_pct.min(100)) / 100, 12, Color::ACCENT_GREEN);
            let mut free_label = TextBuf::new();
            free_label.push_str(crate::gui::ui_text("Boş RAM: ", "Free RAM: "));
            free_label.push_usize(self.free_memory_kb);
            free_label.push_str(" KB");
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 190, free_label.as_str(), Color::TEXT_MUTED);

            let mut storage_label = TextBuf::new();
            storage_label.push_str("VFS: ");
            storage_label.push_usize(self.ramdisk_files);
            storage_label.push_str(crate::gui::ui_text(" dosya, ", " files, "));
            storage_label.push_usize(self.ramdisk_used);
            storage_label.push_str(" B");
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 230, storage_label.as_str(), Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 260,
                crate::gui::ui_text("Uygulama RAM değeri uygulama durum yapısının yaklaşık boyutudur.", "App memory is an estimate of its state structure size."), Color::TEXT_MUTED);
        }

        fb.draw_rect(win_x + 8, win_y + 350, win_w.saturating_sub(16), 1, Color::BORDER);
        let mut resources = TextBuf::new();
        resources.push_str("VFS: ");
        resources.push_usize(self.ramdisk_files);
        resources.push_str(crate::gui::ui_text(" dosya / ", " files / "));
        resources.push_usize(self.ramdisk_used);
        resources.push_str(" B  |  ");
        let status = match self.status {
            "Ready" | "Hazır" => crate::gui::ui_text("Hazır", "Ready"),
            "Uygulama sonlandırıldı." | "Application terminated." => crate::gui::ui_text("Uygulama sonlandırıldı.", "Application terminated."),
            other => other,
        };
        resources.push_str(status);
        MinGuiCompositor::draw_string(fb, win_x + 10, win_y + 358, resources.as_str(), Color::TEXT_MUTED);

        if self.launch_menu_open {
            let (menu_x, menu_y, menu_w, menu_h) = Self::launch_menu_rect(win_x, win_y, win_w);
            fb.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::TASKBAR_BG);
            fb.draw_rect_outline(menu_x, menu_y, menu_w, menu_h, Color::BORDER);
            for index in 0..LAUNCH_APP_COUNT {
                let item = Self::launch_item_rect(win_x, win_y, win_w, index);
                let color = if index == self.launch_selected { Color::ACCENT_BLUE } else { Color::WINDOW_BG };
                fb.draw_rect(item.0, item.1, item.2, item.3, color);
                MinGuiCompositor::draw_string_clipped(fb, item.0 + 5, item.1 + 5,
                    if english { LAUNCH_APP_LABELS_EN[index] } else { LAUNCH_APP_LABELS[index] }, Color::TEXT_MAIN,
                    (item.0 + 2, item.1 + 1, item.2.saturating_sub(4), item.3.saturating_sub(2)));
            }
        }
    }
}
