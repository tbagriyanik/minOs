use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::mm::pmm::PhysicalMemoryManager;
use crate::fs::vfs::VirtualFileSystem;

/// Pencere yöneticisindeki uygulama yuvası sayısı.
pub const MAX_PROC: usize = crate::gui::MAX_WINDOWS;
const PROCESS_ROW_STEP: usize = 22;
const PROCESS_ROW_H: usize = 20;
const LAUNCH_ITEM_H: usize = 22;
pub const LAUNCH_APP_COUNT: usize = 13;
const LAUNCH_APP_IDS: [usize; LAUNCH_APP_COUNT] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
const LAUNCH_APP_LABELS: [&str; LAUNCH_APP_COUNT] = [
    "Dosya Yöneticisi", "Sistem Ayarları", "Not Defteri", "Web Gezgini",
    "Resim Gösterici", "Müzik Çalar", "Video Oynatıcı", "Görev Yöneticisi",
    "Hesap Makinesi", "Çizim Tahtası", "Komut İstemi", "Kurulum Sihirbazı",
    "3D Gösterici",
];
const LAUNCH_APP_LABELS_EN: [&str; LAUNCH_APP_COUNT] = [
    "File Manager", "Settings", "Notepad", "Browser", "Image Viewer", "Music Player",
    "Video Player", "Task Manager", "Calculator", "Paint", "Command Prompt",
    "Installer", "3D Viewer",
];

#[derive(Copy, Clone, Default)]
pub struct ProcessInfo {
    pub app_idx: usize,
    pub app_id: usize,
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
    pub cpu_usage_pct: usize,
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
            processes: [ProcessInfo { app_idx: 0, app_id: 0, name: "", is_minimized: false, cpu_pct: 0, memory_kb: 0 }; MAX_PROC],
            proc_count: 0,
            selected: 0,
            total_memory_kb: 0, free_memory_kb: 0, used_memory_kb: 0,
            cpu_usage_pct: 0,
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
        (win_x + 10, win_y + 52, 142, 24)
    }

    pub fn performance_tab_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + 158, win_y + 52, 142, 24)
    }

    pub fn launch_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        // Action buttons share a row without overlap, including narrow windows.
        let kill = Self::kill_btn_rect(win_x, win_y, win_w);
        let width = 122.min(win_w.saturating_sub(20) / 2).max(1);
        let gap = 8;
        (kill.0.saturating_sub(gap + width), kill.1, width, kill.3)
    }

    pub fn launch_menu_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(270), win_y + 52, 260, 28 + LAUNCH_APP_COUNT * LAUNCH_ITEM_H)
    }

    pub fn launch_item_rect(win_x: usize, win_y: usize, win_w: usize, index: usize) -> (usize, usize, usize, usize) {
        let (x, y, w, _) = Self::launch_menu_rect(win_x, win_y, win_w);
        (x + 4, y + 4 + index * LAUNCH_ITEM_H, w - 8, LAUNCH_ITEM_H)
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
                    let app_id = window.app_id as usize;
                    let state_bytes = Self::app_state_bytes(app_id);
                    self.processes[count] = ProcessInfo {
                        app_idx,
                        app_id,
                        name: LAUNCH_APP_LABELS[app_id.min(LAUNCH_APP_LABELS.len() - 1)],
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
            11 => core::mem::size_of::<crate::apps::installer::InstallerApp>(),
            12 => core::mem::size_of::<crate::apps::viewer3d::Viewer3DApp>(),
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

    /// İlk süreci seçer (Home).
    pub fn select_first(&mut self) { self.selected = 0; }

    /// Son süreci seçer (End).
    pub fn select_last(&mut self) {
        if self.proc_count > 0 { self.selected = self.proc_count - 1; }
    }

    /// Sayfa sayfa gezinme (PageUp / PageDown).
    pub fn select_page(&mut self, down: bool, rows: usize) {
        if self.proc_count == 0 { self.selected = 0; return; }
        let step = rows.max(1);
        if down {
            self.selected = (self.selected + step).min(self.proc_count - 1);
        } else {
            self.selected = self.selected.saturating_sub(step);
        }
    }

    pub fn selected_app_idx(&self) -> Option<usize> {
        self.processes.get(self.selected).filter(|_| self.selected < self.proc_count).map(|entry| entry.app_idx)
    }

    /// i. süreç satırının tıklanabilir dikdörtgeni (çizim ile birebir aynı).
    pub fn row_rect(win_x: usize, win_y: usize, win_w: usize, i: usize) -> (usize, usize, usize, usize) {
        (win_x + 8, win_y + 100 + i * PROCESS_ROW_STEP, win_w.saturating_sub(16), PROCESS_ROW_H)
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
        let width = 122.min(win_w.saturating_sub(20) / 2).max(1);
        (win_x + win_w.saturating_sub(width + 10), win_y + 30, width, 22)
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
            MinGuiCompositor::draw_string_clipped(fb, win_x + 12, win_y + 82, crate::gui::ui_text("Uygulama", "Application"), Color::TEXT_MUTED,
                (win_x + 10, win_y + 80, 154, 20));
            MinGuiCompositor::draw_string_clipped(fb, win_x + 170, win_y + 82, "CPU", Color::TEXT_MUTED,
                (win_x + 168, win_y + 80, 54, 20));
            MinGuiCompositor::draw_string_clipped(fb, win_x + 230, win_y + 82, crate::gui::ui_text("Yapı", "State"), Color::TEXT_MUTED,
                (win_x + 228, win_y + 80, 68, 20));
            MinGuiCompositor::draw_string_clipped(fb, win_x + 304, win_y + 82, crate::gui::ui_text("Durum", "Status"), Color::TEXT_MUTED,
                (win_x + 302, win_y + 80, 106, 20));
            fb.draw_rect(win_x + 10, win_y + 94, win_w.saturating_sub(20), 1, Color::BORDER);

            for (i, proc) in self.processes.iter().take(self.proc_count).enumerate() {
                let row = Self::row_rect(win_x, win_y, win_w, i);
                if i == self.selected { fb.draw_rect(row.0, row.1, row.2, row.3, Color::TITLE_BAR); }
                if self.focused_control == 4 + i { fb.draw_rect_outline(row.0, row.1, row.2, row.3, Color::ACCENT_YELLOW); }
                let app_name = if english { LAUNCH_APP_LABELS_EN[proc.app_id.min(LAUNCH_APP_COUNT - 1)] } else { proc.name };
                MinGuiCompositor::draw_string_clipped(fb, win_x + 12, row.1 + 2, app_name,
                    if i == self.selected { MinGuiCompositor::text_on(Color::TITLE_BAR) } else { Color::TEXT_MAIN },
                    (win_x + 10, row.1, 154, row.3));
                let mut cpu = TextBuf::new();
                cpu.push_usize(proc.cpu_pct);
                cpu.push_byte(b'%');
                // Sayıları sütunun sağ kenarına dayalı hizala.
                let cpu_right = win_x + 222;
                let cpu_x = cpu_right.saturating_sub(MinGuiCompositor::text_width(cpu.as_str(), 1));
                MinGuiCompositor::draw_string_clipped(fb, cpu_x, row.1 + 2, cpu.as_str(), Color::ACCENT_YELLOW,
                    (win_x + 168, row.1, 54, row.3));
                let mut state_size = TextBuf::new();
                state_size.push_usize(proc.memory_kb);
                state_size.push_str(" KB");
                let mem_right = win_x + 296;
                let mem_x = mem_right.saturating_sub(MinGuiCompositor::text_width(state_size.as_str(), 1));
                MinGuiCompositor::draw_string_clipped(fb, mem_x, row.1 + 2, state_size.as_str(), Color::TEXT_MUTED,
                    (win_x + 228, row.1, 68, row.3));
                MinGuiCompositor::draw_string_clipped(fb, win_x + 304, row.1 + 2,
                    match (proc.is_minimized, english) { (true, true) => "Paused", (false, true) => "Running", (true, false) => "Bekl.", (false, false) => "Açık" }, Color::ACCENT_GREEN,
                    (win_x + 302, row.1, 106, row.3));
            }
        } else {
            let bar_x = win_x + 132;
            let bar_w = win_w.saturating_sub(152);
            let mut cpu_text = TextBuf::new();
            cpu_text.push_str(crate::gui::ui_text("CPU etkin: %", "CPU active: %"));
            cpu_text.push_usize(self.cpu_usage_pct.min(100));
            MinGuiCompositor::draw_string_clipped(fb, win_x + 14, win_y + 100, cpu_text.as_str(), Color::TEXT_MAIN,
                (win_x + 12, win_y + 98, win_w.saturating_sub(24), 16));
            fb.draw_rect(bar_x, win_y + 100, bar_w, 12, Color::TITLE_BAR_INACTIVE);
            fb.draw_rect(bar_x, win_y + 100, bar_w.saturating_mul(self.cpu_usage_pct.min(100)) / 100, 12, Color::ACCENT_RED);
            MinGuiCompositor::draw_string_clipped(fb, win_x + 14, win_y + 120,
                crate::gui::ui_text("Ana döngü ölçümü; boşta bekleme hariç, tek çekirdek.", "Main-loop estimate; idle wait excluded, one core."), Color::TEXT_MUTED,
                (win_x + 12, win_y + 118, win_w.saturating_sub(24), 16));

            let memory_pct = if self.total_memory_kb == 0 { 0 }
                else { self.used_memory_kb.saturating_mul(100) / self.total_memory_kb };
            // Etiket ve dolum çubuğu ayrı satırlarda; metin çubuğun üzerine binmez.
            let mut memory_label = TextBuf::new();
            memory_label.push_str(crate::gui::ui_text("Fiziksel RAM: ", "Physical RAM: "));
            memory_label.push_usize(self.used_memory_kb / 1024);
            memory_label.push_str(" / ");
            memory_label.push_usize(self.total_memory_kb / 1024);
            memory_label.push_str(" MB");
            MinGuiCompositor::draw_string_clipped(fb, win_x + 14, win_y + 142, memory_label.as_str(), Color::TEXT_MAIN,
                (win_x + 12, win_y + 140, win_w.saturating_sub(24), 16));
            fb.draw_rect(bar_x, win_y + 158, bar_w, 12, Color::TITLE_BAR_INACTIVE);
            fb.draw_rect(bar_x, win_y + 158, bar_w.saturating_mul(memory_pct.min(100)) / 100, 12, Color::ACCENT_GREEN);
            let mut free_label = TextBuf::new();
            free_label.push_str(crate::gui::ui_text("Kullanılabilir RAM: ", "Available RAM: "));
            free_label.push_usize(self.free_memory_kb / 1024);
            free_label.push_str(" MB");
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 176, free_label.as_str(), Color::TEXT_MUTED);

            let mut storage_label = TextBuf::new();
            storage_label.push_str("VFS: ");
            storage_label.push_usize(self.ramdisk_files);
            storage_label.push_str(crate::gui::ui_text(" dosya, ", " files, "));
            storage_label.push_usize(self.ramdisk_used);
            storage_label.push_str(" B");
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 230, storage_label.as_str(), Color::TEXT_MAIN);
            MinGuiCompositor::draw_string(fb, win_x + 14, win_y + 260,
                crate::gui::ui_text("Süreç başına RAM ölçümü çekirdekte henüz yok.", "Per-process RAM accounting is not available yet."), Color::TEXT_MUTED);
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
