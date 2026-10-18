use crate::drivers::video::Framebuffer;
use crate::gui::{Color, MinGuiCompositor, TextBuf};
use crate::mm::pmm::PhysicalMemoryManager;
use crate::fs::vfs::VirtualFileSystem;

/// Pencere yöneticisindeki uygulama yuvası sayısı.
pub const MAX_PROC: usize = crate::gui::MAX_WINDOWS;
const PROCESS_ROW_H: usize = 20;
const LAUNCH_ITEM_H: usize = 22;
pub const ALL_APP_COUNT: usize = 14;
const ALL_APP_LABELS: [&str; ALL_APP_COUNT] = [
    "Dosya Yöneticisi", "Sistem Ayarları", "Not Defteri", "Web Gezgini",
    "Resim Gösterici", "Müzik Çalar", "Video Oynatıcı", "Görev Yöneticisi",
    "Hesap Makinesi", "Çizim Tahtası", "Komut İstemi", "Kurulum Sihirbazı",
    "3D Gösterici", "Paket Yöneticisi",
];
const ALL_APP_LABELS_EN: [&str; ALL_APP_COUNT] = [
    "File Manager", "Settings", "Notepad", "Browser", "Image Viewer", "Music Player",
    "Video Player", "Task Manager", "Calculator", "Paint", "Command Prompt",
    "Installer", "3D Viewer", "Package Manager",
];

// "Program Aç" menüsü için Görev Yöneticisi (ID 7) hariç tutulmuş,
// Türkçe alfabetik sıraya göre sıralanmış liste:
// 1. 3D Gösterici (12)
// 2. Çizim Tahtası (9)
// 3. Dosya Yöneticisi (0)
// 4. Hesap Makinesi (8)
// 5. Komut İstemi (10)
// 6. Kurulum Sihirbazı (11)
// 7. Müzik Çalar (5)
// 8. Not Defteri (2)
// 9. Paket Yöneticisi (13)
// 10. Resim Gösterici (4)
// 11. Sistem Ayarları (1)
// 12. Video Oynatıcı (6)
// 13. Web Gezgini (3)
pub const LAUNCH_APP_COUNT: usize = 13;
const LAUNCH_APP_IDS: [usize; LAUNCH_APP_COUNT] = [12, 9, 0, 8, 10, 11, 5, 2, 13, 4, 1, 6, 3];
const LAUNCH_APP_LABELS: [&str; LAUNCH_APP_COUNT] = [
    "3D Gösterici",
    "Çizim Tahtası",
    "Dosya Yöneticisi",
    "Hesap Makinesi",
    "Komut İstemi",
    "Kurulum Sihirbazı",
    "Müzik Çalar",
    "Not Defteri",
    "Paket Yöneticisi",
    "Resim Gösterici",
    "Sistem Ayarları",
    "Video Oynatıcı",
    "Web Gezgini",
];
const LAUNCH_APP_LABELS_EN: [&str; LAUNCH_APP_COUNT] = [
    "3D Viewer",
    "Paint",
    "File Manager",
    "Calculator",
    "Command Prompt",
    "Installer",
    "Music Player",
    "Notepad",
    "Package Manager",
    "Image Viewer",
    "Settings",
    "Video Player",
    "Browser",
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
    pub core_count: usize,
    pub core_usage_pct: [usize; crate::arch::x86_64::smp::MAX_CPUS],
    pub ramdisk_used: usize,
    pub ramdisk_files: usize,
    /// Sonlandırma ve liste durumu mesajı.
    pub status: &'static str,
    pub launch_menu_open: bool,
    pub launch_selected: usize,
    pub active_tab: usize,
    pub focused_control: usize,
}

/// Standart Görev Yöneticisi UI yerleşim sabitleri
pub const TM_TAB_Y: usize = 30;
pub const TM_TAB_H: usize = 24;
pub const TM_ACTION_Y: usize = 64;
pub const TM_ACTION_H: usize = 22;
pub const TM_HEADER_Y: usize = 96;
pub const TM_HEADER_H: usize = 20;
pub const TM_CONTENT_Y: usize = 120;
pub const TM_ROW_STEP: usize = 22;

impl TaskManagerApp {
    pub const fn new() -> Self {
        Self {
            processes: [ProcessInfo { app_idx: 0, app_id: 0, name: "", is_minimized: false, cpu_pct: 0, memory_kb: 0 }; MAX_PROC],
            proc_count: 0,
            selected: 0,
            total_memory_kb: 0, free_memory_kb: 0, used_memory_kb: 0,
            cpu_usage_pct: 0,
            core_count: 1,
            core_usage_pct: [0; crate::arch::x86_64::smp::MAX_CPUS],
            ramdisk_used: 0, ramdisk_files: 0,
            status: "Ready",
            launch_menu_open: false,
            launch_selected: 0,
            active_tab: 0,
            focused_control: 0,
        }
    }

    pub fn focus_next_control(&mut self) {
        if self.active_tab == 0 {
            let focusable_count = 4 + self.proc_count;
            self.focused_control = (self.focused_control + 1) % focusable_count.max(1);
            if self.focused_control >= 4 {
                self.selected = self.focused_control - 4;
            }
        } else {
            // Performans sekmesinde sadece sekme başlıkları (0 ve 1) odaklanabilir
            self.focused_control = (self.focused_control + 1) % 2;
        }
        self.launch_menu_open = false;
    }

    pub fn applications_tab_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + 10, win_y + TM_TAB_Y, 130, TM_TAB_H)
    }

    pub fn performance_tab_rect(win_x: usize, win_y: usize) -> (usize, usize, usize, usize) {
        (win_x + 148, win_y + TM_TAB_Y, 130, TM_TAB_H)
    }

    pub fn launch_button_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        let kill = Self::kill_btn_rect(win_x, win_y, win_w);
        let width = 110.min(win_w.saturating_sub(20) / 2).max(1);
        let gap = 8;
        (kill.0.saturating_sub(gap + width), kill.1, width, kill.3)
    }

    pub fn launch_menu_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + win_w.saturating_sub(270), win_y + TM_ACTION_Y + TM_ACTION_H + 4, 260, 28 + LAUNCH_APP_COUNT * LAUNCH_ITEM_H)
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
                        name: ALL_APP_LABELS[app_id.min(ALL_APP_LABELS.len() - 1)],
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
            13 => core::mem::size_of::<crate::apps::pkgmgr::PackageManagerApp>(),
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
        (win_x + 8, win_y + TM_CONTENT_Y + i * TM_ROW_STEP, win_w.saturating_sub(16), PROCESS_ROW_H)
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

    /// "Sonlandır" düğmesinin dikdörtgeni.
    pub fn kill_btn_rect(win_x: usize, win_y: usize, win_w: usize) -> (usize, usize, usize, usize) {
        let width = 110.min(win_w.saturating_sub(20) / 2).max(1);
        (win_x + win_w.saturating_sub(width + 10), win_y + TM_ACTION_Y, width, TM_ACTION_H)
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, win_h: usize) {
        let english = crate::gui::ui_english();
        // 1. Üst Sekmeler (Uygulamalar & Performans)
        let applications_tab = Self::applications_tab_rect(win_x, win_y);
        let performance_tab = Self::performance_tab_rect(win_x, win_y);
        let app_tab_bg = if self.active_tab == 0 { Color::ACCENT_BLUE } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(applications_tab.0, applications_tab.1, applications_tab.2, applications_tab.3, app_tab_bg);
        MinGuiCompositor::draw_string_clipped(fb, applications_tab.0 + 12, applications_tab.1 + 7, crate::gui::ui_text("Uygulamalar", "Applications"), MinGuiCompositor::text_on(app_tab_bg),
            (applications_tab.0 + 2, applications_tab.1 + 1, applications_tab.2.saturating_sub(4), applications_tab.3.saturating_sub(2)));
        let perf_tab_bg = if self.active_tab == 1 { Color::ACCENT_BLUE } else { Color::TITLE_BAR_INACTIVE };
        fb.draw_rect(performance_tab.0, performance_tab.1, performance_tab.2, performance_tab.3, perf_tab_bg);
        MinGuiCompositor::draw_string_clipped(fb, performance_tab.0 + 12, performance_tab.1 + 7, crate::gui::ui_text("Performans", "Performance"), MinGuiCompositor::text_on(perf_tab_bg),
            (performance_tab.0 + 2, performance_tab.1 + 1, performance_tab.2.saturating_sub(4), performance_tab.3.saturating_sub(2)));
        if self.focused_control == 0 { fb.draw_rect_outline(applications_tab.0, applications_tab.1, applications_tab.2, applications_tab.3, Color::ACCENT_YELLOW); }
        if self.focused_control == 1 { fb.draw_rect_outline(performance_tab.0, performance_tab.1, performance_tab.2, performance_tab.3, Color::ACCENT_YELLOW); }

        if self.active_tab == 0 {
            // 2. Alt Araç Çubuğu: Durum Başlığı ve Aksiyon Düğmeleri
            let mut header = TextBuf::new();
            header.push_str(crate::gui::ui_text("Uygulamalar: ", "Applications: "));
            header.push_u2((self.alive_count().min(99)) as u32);
            MinGuiCompositor::draw_string(fb, win_x + 10, win_y + TM_ACTION_Y + 4, header.as_str(), Color::ACCENT_PINK);

            // "Program Aç" ve "Sonlandır" düğmeleri (yalnızca ilk sekmede gösterilir)
            let br = Self::kill_btn_rect(win_x, win_y, win_w);
            fb.draw_rect(br.0, br.1, br.2, br.3, Color::ACCENT_RED);
            MinGuiCompositor::draw_string_clipped(fb, br.0 + 8, br.1 + 5, crate::gui::ui_text("Sonlandır", "End Task"), MinGuiCompositor::text_on(Color::ACCENT_RED),
                (br.0 + 2, br.1 + 1, br.2.saturating_sub(4), br.3.saturating_sub(2)));

            let launch_button = Self::launch_button_rect(win_x, win_y, win_w);
            fb.draw_rect(launch_button.0, launch_button.1, launch_button.2, launch_button.3, Color::ACCENT_BLUE);
            MinGuiCompositor::draw_string_clipped(fb, launch_button.0 + 8, launch_button.1 + 5, crate::gui::ui_text("Program Aç", "Run Program"), MinGuiCompositor::text_on(Color::ACCENT_BLUE),
                (launch_button.0 + 2, launch_button.1 + 1, launch_button.2.saturating_sub(4), launch_button.3.saturating_sub(2)));

            if self.focused_control == 2 { fb.draw_rect_outline(launch_button.0, launch_button.1, launch_button.2, launch_button.3, Color::ACCENT_YELLOW); }
            if self.focused_control == 3 { fb.draw_rect_outline(br.0, br.1, br.2, br.3, Color::ACCENT_YELLOW); }
            // Sütun başlıkları (genişletilmiş ve ferah aralıklar)
            MinGuiCompositor::draw_string_clipped(fb, win_x + 12, win_y + TM_HEADER_Y + 2, crate::gui::ui_text("Uygulama", "Application"), Color::TEXT_MUTED,
                (win_x + 10, win_y + TM_HEADER_Y, 160, TM_HEADER_H));
            MinGuiCompositor::draw_string_clipped(fb, win_x + 180, win_y + TM_HEADER_Y + 2, "CPU", Color::TEXT_MUTED,
                (win_x + 176, win_y + TM_HEADER_Y, 60, TM_HEADER_H));
            MinGuiCompositor::draw_string_clipped(fb, win_x + 246, win_y + TM_HEADER_Y + 2, crate::gui::ui_text("Bellek", "Memory"), Color::TEXT_MUTED,
                (win_x + 242, win_y + TM_HEADER_Y, 76, TM_HEADER_H));
            MinGuiCompositor::draw_string_clipped(fb, win_x + 328, win_y + TM_HEADER_Y + 2, crate::gui::ui_text("Durum", "Status"), Color::TEXT_MUTED,
                (win_x + 324, win_y + TM_HEADER_Y, 120, TM_HEADER_H));
            fb.draw_rect(win_x + 10, win_y + TM_HEADER_Y + TM_HEADER_H - 2, win_w.saturating_sub(20), 1, Color::BORDER);

            let max_visible_rows = win_h.saturating_sub(TM_CONTENT_Y + 36) / TM_ROW_STEP;
            for (i, proc) in self.processes.iter().take(self.proc_count.min(max_visible_rows)).enumerate() {
                let row = Self::row_rect(win_x, win_y, win_w, i);
                if i == self.selected { fb.draw_rect(row.0, row.1, row.2, row.3, Color::TITLE_BAR); }
                if self.focused_control == 4 + i { fb.draw_rect_outline(row.0, row.1, row.2, row.3, Color::ACCENT_YELLOW); }
                let app_name = if english { ALL_APP_LABELS_EN[proc.app_id.min(ALL_APP_COUNT - 1)] } else { proc.name };
                MinGuiCompositor::draw_string_clipped(fb, win_x + 12, row.1 + 2, app_name,
                    if i == self.selected { MinGuiCompositor::text_on(Color::TITLE_BAR) } else { Color::TEXT_MAIN },
                    (win_x + 10, row.1, 160, row.3));
                let mut cpu = TextBuf::new();
                cpu.push_usize(proc.cpu_pct);
                cpu.push_byte(b'%');
                let cpu_right = win_x + 236;
                let cpu_x = cpu_right.saturating_sub(MinGuiCompositor::text_width(cpu.as_str(), 1));
                MinGuiCompositor::draw_string_clipped(fb, cpu_x, row.1 + 2, cpu.as_str(), Color::ACCENT_YELLOW,
                    (win_x + 176, row.1, 60, row.3));
                let mut state_size = TextBuf::new();
                state_size.push_usize(proc.memory_kb);
                state_size.push_str(" KB");
                let mem_right = win_x + 318;
                let mem_x = mem_right.saturating_sub(MinGuiCompositor::text_width(state_size.as_str(), 1));
                MinGuiCompositor::draw_string_clipped(fb, mem_x, row.1 + 2, state_size.as_str(), Color::TEXT_MUTED,
                    (win_x + 242, row.1, 76, row.3));
                MinGuiCompositor::draw_string_clipped(fb, win_x + 328, row.1 + 2,
                    match (proc.is_minimized, english) { (true, true) => "Paused", (false, true) => "Running", (true, false) => "Bekl.", (false, false) => "Açık" }, Color::ACCENT_GREEN,
                    (win_x + 324, row.1, 120, row.3));
            }
        } else {
            // === PERFORMANS SEKMESİ ===
            let content_x = win_x + 14;
            let content_w = win_w.saturating_sub(28);

            // 1. CPU KULLANIM BÖLÜMÜ
            let mut cpu_y = win_y + 64;
            let mut cpu_text = TextBuf::new();
            cpu_text.push_str(crate::gui::ui_text("CPU Genel Kullanımı: %", "Total CPU Usage: %"));
            cpu_text.push_usize(self.cpu_usage_pct.min(100));
            if self.core_count > 1 {
                cpu_text.push_str("  (");
                cpu_text.push_usize(self.core_count);
                cpu_text.push_str(crate::gui::ui_text(" Çekirdek Çevrimiçi)", " Cores Online)"));
            }
            MinGuiCompositor::draw_string_clipped(fb, content_x, cpu_y, cpu_text.as_str(), Color::ACCENT_BLUE,
                (content_x, cpu_y, content_w, 16));

            cpu_y += 20;
            // Genel CPU İlerleme Çubuğu (Büyük Grafik)
            fb.draw_rect(content_x, cpu_y, content_w, 14, Color::TITLE_BAR_INACTIVE);
            fb.draw_rect_outline(content_x, cpu_y, content_w, 14, Color::BORDER);
            fb.draw_rect(content_x + 1, cpu_y + 1, content_w.saturating_sub(2).saturating_mul(self.cpu_usage_pct.min(100)) / 100, 12, Color::ACCENT_RED);

            cpu_y += 22;

            // Çekirdek Başına Ayrı Çubuklar / Grafikler
            if self.core_count > 1 {
                let cores = self.core_count.min(crate::arch::x86_64::smp::MAX_CPUS);
                let gap = 6;
                let total_gaps = gap * cores.saturating_sub(1);
                let per_core_w = (content_w.saturating_sub(total_gaps) / cores).max(12);

                for c in 0..cores {
                    let cx = content_x + c * (per_core_w + gap);
                    let pct = self.core_usage_pct[c].min(100);

                    // Çekirdek Etiketi (C0, C1...) ve % değeri
                    let mut core_lbl = TextBuf::new();
                    core_lbl.push_str("C");
                    core_lbl.push_usize(c);
                    core_lbl.push_str(":%");
                    core_lbl.push_usize(pct);
                    MinGuiCompositor::draw_string_clipped(fb, cx, cpu_y, core_lbl.as_str(), Color::TEXT_MUTED,
                        (cx, cpu_y, per_core_w, 14));

                    // Dikey/Yatay mini çekirdek barı
                    let bar_top = cpu_y + 16;
                    fb.draw_rect(cx, bar_top, per_core_w, 10, Color::TITLE_BAR_INACTIVE);
                    fb.draw_rect_outline(cx, bar_top, per_core_w, 10, Color::BORDER);
                    fb.draw_rect(cx + 1, bar_top + 1, per_core_w.saturating_sub(2).saturating_mul(pct) / 100, 8, Color::ACCENT_YELLOW);
                }
                cpu_y += 34;
            } else {
                MinGuiCompositor::draw_string_clipped(fb, content_x, cpu_y,
                    crate::gui::ui_text("Tek çekirdek (BSP) aktif.", "Single core (BSP) active."), Color::TEXT_MUTED,
                    (content_x, cpu_y, content_w, 16));
                cpu_y += 22;
            }

            // 2. RAM (BELLEK) BÖLÜMÜ
            let mem_y = cpu_y + 8;
            let memory_pct = if self.total_memory_kb == 0 { 0 }
                else { self.used_memory_kb.saturating_mul(100) / self.total_memory_kb };

            let mut memory_label = TextBuf::new();
            memory_label.push_str(crate::gui::ui_text("RAM Kullanımı: %", "RAM Usage: %"));
            memory_label.push_usize(memory_pct.min(100));
            memory_label.push_str("  (");
            memory_label.push_usize(self.used_memory_kb / 1024);
            memory_label.push_str(" MB / ");
            memory_label.push_usize(self.total_memory_kb / 1024);
            memory_label.push_str(" MB)");
            MinGuiCompositor::draw_string_clipped(fb, content_x, mem_y, memory_label.as_str(), Color::ACCENT_GREEN,
                (content_x, mem_y, content_w, 16));

            let mem_bar_y = mem_y + 20;
            // RAM Doluluk Çubuğu (Büyük Grafik)
            fb.draw_rect(content_x, mem_bar_y, content_w, 14, Color::TITLE_BAR_INACTIVE);
            fb.draw_rect_outline(content_x, mem_bar_y, content_w, 14, Color::BORDER);
            fb.draw_rect(content_x + 1, mem_bar_y + 1, content_w.saturating_sub(2).saturating_mul(memory_pct.min(100)) / 100, 12, Color::ACCENT_GREEN);

            let mut free_label = TextBuf::new();
            free_label.push_str(crate::gui::ui_text("Boş Fiziksel Bellek: ", "Free Physical Memory: "));
            free_label.push_usize(self.free_memory_kb / 1024);
            free_label.push_str(" MB");
            MinGuiCompositor::draw_string_clipped(fb, content_x, mem_bar_y + 18, free_label.as_str(), Color::TEXT_MUTED,
                (content_x, mem_bar_y + 18, content_w, 14));

            // 3. VFS / DEPOLAMA BİLGİSİ
            let vfs_y = mem_bar_y + 40;
            let mut storage_label = TextBuf::new();
            storage_label.push_str("Ramdisk VFS: ");
            storage_label.push_usize(self.ramdisk_files);
            storage_label.push_str(crate::gui::ui_text(" dosya  |  Kullanılan: ", " files  |  Used: "));
            storage_label.push_usize(self.ramdisk_used);
            storage_label.push_str(" B");
            MinGuiCompositor::draw_string_clipped(fb, content_x, vfs_y, storage_label.as_str(), Color::TEXT_MAIN,
                (content_x, vfs_y, content_w, 14));
        }

        // PENCERE EN ALTINA YAPIŞIK DURUM ÇUBUĞU (STATUS BAR)
        let status_h = 24;
        let status_y = win_y + win_h.saturating_sub(status_h);
        fb.draw_rect(win_x, status_y, win_w, status_h, Color::TITLE_BAR);
        fb.draw_rect(win_x, status_y, win_w, 1, Color::BORDER);

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
        MinGuiCompositor::draw_string_clipped(fb, win_x + 10, status_y + 5, resources.as_str(), Color::TEXT_MUTED,
            (win_x + 8, status_y + 2, win_w.saturating_sub(16), status_h.saturating_sub(4)));

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
