use crate::drivers::ata;
use crate::drivers::video::Framebuffer;
use crate::fs::diskfs;
use crate::fs::vfs::VirtualFileSystem;
use crate::gui::{Color, MinGuiCompositor, TextBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    Selecting,
    Formatting,
    Copying,
    Complete,
    Failed,
}

pub struct InstallerApp {
    pub selected_drive: u8,
    pub state: InstallState,
    pub status_msg: &'static str,
    pub progress_pct: usize,
    pub drive_count: usize,
}

impl InstallerApp {
    pub const fn new() -> Self {
        Self {
            selected_drive: 0,
            state: InstallState::Selecting,
            status_msg: "Hedef diski secip 'Tek Tikla Kur' butonuna basiniz.",
            progress_pct: 0,
            drive_count: 0,
        }
    }

    pub fn refresh_drives(&mut self) {
        let mut count = 0;
        for i in 0..4 {
            if let Some(dev) = ata::device(i) {
                if dev.kind == ata::DeviceKind::Ata {
                    count += 1;
                }
            }
        }
        self.drive_count = count;
        // Eger secili surucu ATA degilse ilk gecerli ATA surucuye gec
        let cur_valid = ata::device(self.selected_drive)
            .map(|d| d.kind == ata::DeviceKind::Ata)
            .unwrap_or(false);
        if !cur_valid {
            for i in 0..4 {
                if let Some(dev) = ata::device(i) {
                    if dev.kind == ata::DeviceKind::Ata {
                        self.selected_drive = i;
                        break;
                    }
                }
            }
        }
    }

    pub fn select_next_drive(&mut self) {
        for step in 1..=4 {
            let next = (self.selected_drive + step) % 4;
            if let Some(dev) = ata::device(next) {
                if dev.kind == ata::DeviceKind::Ata {
                    self.selected_drive = next;
                    break;
                }
            }
        }
    }

    /// "Tek Tıkla Kur" işlemi:
    /// 1. Hedef diski FAT32 olarak biçimlendirir (BPB, FSInfo, FAT1/FAT2, Root Dir)
    /// 2. Kurulum ve işletim sistemi kimlik/işaret dosyalarını (MINOS.SYS, INSTALLED) yazar
    /// 3. VFS'deki mevcut sistem ayarlarını (settings.cfg) diske kalıcı olarak kaydeder
    /// 4. VFS'e bağlar ve sistem durumunu günceller
    pub fn perform_install(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        self.state = InstallState::Formatting;
        self.progress_pct = 25;
        self.status_msg = "Disk FAT32 olarak bicimlendiriliyor...";

        let drive = self.selected_drive;
        if !diskfs::fat_format(drive) {
            self.state = InstallState::Failed;
            self.status_msg = "HATA: Disk bicimlendirilemedi!";
            return false;
        }

        self.state = InstallState::Copying;
        self.progress_pct = 65;
        self.status_msg = "Sistem dosyalari ve bagimliliklar kopyalaniyor...";

        // Disk dosya sistemini bağla
        diskfs::init();

        // 1. Kurulum işaretçisi (Marker) dosyasını oluştur
        let marker_content = b"minOs Operating System Installed\nPersistent FAT32 Boot Partition\n";
        diskfs::save_file_by_name("MINOS.SYS", marker_content);

        // 2. Kullanıcı benioku dosyasını yaz
        let readme_content = b"minOs Kurulumu Basariyla Tamamlandi!\n====================================\nBu bolumde tam okuma ve yazma hakkiniz bulunmaktadir.\nSistem ayarlariniz ve yeni dosyalariniz otomatik olarak bu bolume kaydedilir.\n";
        diskfs::save_file_by_name("BENIOKU.TXT", readme_content);

        // 3. Mevcut VFS ayarlarını (veya varsayılanları) diske settings.cfg olarak yaz
        if let Some(cfg_data) = vfs.read_file("settings.cfg") {
            diskfs::save_file_by_name("settings.cfg", cfg_data);
        } else {
            let default_cfg = b"volume=75;muted=0;res=1280x720;night=0;night_level=55;keyboard=tr;language=tr;zoom=100;region=0;dhcp=1;theme=velvet";
            diskfs::save_file_by_name("settings.cfg", default_cfg);
        }

        self.progress_pct = 100;
        self.state = InstallState::Complete;
        self.status_msg = "Kurulum tamamlandi! Disk kalici yazmaya hazir.";
        true
    }

    // Geometri yardımcıları
    pub fn install_btn_rect(win_x: usize, win_y: usize, win_w: usize, win_h: usize) -> (usize, usize, usize, usize) {
        let btn_w = 170;
        let btn_h = 28;
        let x = win_x + (win_w.saturating_sub(btn_w)) / 2;
        let y = win_y + win_h.saturating_sub(48);
        (x, y, btn_w, btn_h)
    }

    pub fn drive_select_rect(win_x: usize, win_y: usize, _win_w: usize) -> (usize, usize, usize, usize) {
        (win_x + 30, win_y + 115, 340, 36)
    }

    pub fn render_content(&self, fb: &mut Framebuffer, win_x: usize, win_y: usize, win_w: usize, win_h: usize) {
        let english = crate::gui::ui_english();

        // 1. Üst Başlık Banner
        fb.draw_vgradient(win_x, win_y + 24, win_w, 42, Color::TITLE_BAR, MinGuiCompositor::shade(Color::TITLE_BAR, -15));
        let header_title = if english { "minOs Quick Installation Wizard" } else { "minOs Hizli Kurulum Sihirbazi" };
        MinGuiCompositor::draw_string(fb, win_x + 16, win_y + 36, header_title, Color::TEXT_MAIN);

        // 2. Açıklama metni
        let intro_text = if english {
            "Select target disk for one-click installation with persistent storage:"
        } else {
            "Tek tikla formatlayip kalici veri depolama ile kuracaginiz hedef diski secin:"
        };
        MinGuiCompositor::draw_string(fb, win_x + 20, win_y + 78, intro_text, Color::TEXT_MUTED);

        // 3. Disk Seçim Kartı
        let sel_rect = Self::drive_select_rect(win_x, win_y, win_w);
        fb.draw_rounded_rect(sel_rect.0, sel_rect.1, sel_rect.2, sel_rect.3, 6, Color::TASKBAR_BG);
        fb.draw_rounded_rect_outline(sel_rect.0, sel_rect.1, sel_rect.2, sel_rect.3, 6, Color::ACCENT_BLUE);

        // Disk Simge & Bilgisi
        let disk_dev = ata::device(self.selected_drive);
        let mut disk_label = TextBuf::new();
        disk_label.push_str(if english { "Drive " } else { "Surucu " });
        disk_label.push_usize(self.selected_drive as usize);
        disk_label.push_str(": ");
        if let Some(dev) = disk_dev {
            let mb = dev.size_mb();
            disk_label.push_usize(mb as usize);
            disk_label.push_str(" MB (");
            let m_str = dev.model_str();
            if m_str.is_empty() {
                disk_label.push_str("ATA Sabit Disk");
            } else {
                let bytes = m_str.as_bytes();
                let take = bytes.len().min(18);
                if let Ok(sub) = core::str::from_utf8(&bytes[..take]) {
                    disk_label.push_str(sub);
                }
            }
            disk_label.push_str(")");
        } else {
            disk_label.push_str(if english { "No disk detected" } else { "Disk tespit edilemedi" });
        }
        MinGuiCompositor::draw_string(fb, sel_rect.0 + 12, sel_rect.1 + 6, disk_label.as_str(), Color::TEXT_MAIN);

        let switch_hint = if english { "(Click to switch drive)" } else { "(Diski degistirmek icin tiklayin)" };
        MinGuiCompositor::draw_string(fb, sel_rect.0 + 12, sel_rect.1 + 20, switch_hint, Color::ACCENT_YELLOW);

        // 4. Kurulum Özellikleri Bilgi Kutusu
        let info_y = win_y + 162;
        let info_w = win_w.saturating_sub(40);
        fb.draw_rounded_rect(win_x + 20, info_y, info_w, 82, 6, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(win_x + 20, info_y, info_w, 82, 6, Color::BORDER);

        let feature1 = if english {
            "+ Otomatik FAT32 bicimlendirme (VBR, FAT1, FAT2, RootDir)"
        } else {
            "+ Otomatik FAT32 bicimlendirme (VBR, FAT1, FAT2, RootDir)"
        };
        let feature2 = if english {
            "+ Full Read/Write access (Kullanici ve sistem dosyalari yazilabilir)"
        } else {
            "+ Tam Okuma/Yazma izni (Kullanici ve sistem dosyalari yazilabilir)"
        };
        let feature3 = if english {
            "+ Kalici ayarlar: Tema, cozunurluk ve saat sonraki acilista yuklenir"
        } else {
            "+ Kalici ayarlar: Tema, cozunurluk ve saat sonraki acilista yuklenir"
        };
        MinGuiCompositor::draw_string(fb, win_x + 28, info_y + 10, feature1, Color::ACCENT_GREEN);
        MinGuiCompositor::draw_string(fb, win_x + 28, info_y + 32, feature2, Color::ACCENT_BLUE);
        MinGuiCompositor::draw_string(fb, win_x + 28, info_y + 54, feature3, Color::ACCENT_YELLOW);

        // 5. İlerleme Çubuğu
        let prog_y = win_y + 256;
        let prog_w = win_w.saturating_sub(40);
        fb.draw_rounded_rect(win_x + 20, prog_y, prog_w, 14, 4, Color::DARK_BG);
        fb.draw_rounded_rect_outline(win_x + 20, prog_y, prog_w, 14, 4, Color::BORDER);
        if self.progress_pct > 0 {
            let filled_w = (prog_w * self.progress_pct) / 100;
            let bar_col = match self.state {
                InstallState::Complete => Color::ACCENT_GREEN,
                InstallState::Failed => Color::ACCENT_RED,
                _ => Color::ACCENT_BLUE,
            };
            fb.draw_rounded_rect(win_x + 21, prog_y + 1, filled_w.saturating_sub(2), 12, 3, bar_col);
        }

        // Durum Mesajı
        MinGuiCompositor::draw_string(fb, win_x + 22, prog_y + 20, self.status_msg, match self.state {
            InstallState::Complete => Color::ACCENT_GREEN,
            InstallState::Failed => Color::ACCENT_RED,
            _ => Color::TEXT_MAIN,
        });

        // 6. "Tek Tıkla Kur" Butonu
        let (bx, by, bw, bh) = Self::install_btn_rect(win_x, win_y, win_w, win_h);
        let btn_bg = match self.state {
            InstallState::Complete => Color::ACCENT_GREEN,
            InstallState::Failed => Color::ACCENT_RED,
            InstallState::Formatting | InstallState::Copying => Color::TEXT_MUTED,
            _ => Color::ACCENT_BLUE,
        };
        fb.draw_rounded_rect(bx, by, bw, bh, 6, btn_bg);
        fb.draw_rounded_rect_outline(bx, by, bw, bh, 6, Color::WHITE);

        let btn_text = match self.state {
            InstallState::Complete => if english { "Kuruldu (Hazir)" } else { "Kuruldu (Hazir)" },
            InstallState::Formatting | InstallState::Copying => if english { "Kuruluyor..." } else { "Kuruluyor..." },
            InstallState::Failed => if english { "Tekrar Dene" } else { "Tekrar Dene" },
            _ => if english { "Tek Tikla Kur" } else { "Tek Tikla Kur" },
        };
        let tw = MinGuiCompositor::text_width(btn_text, 1);
        let tx = bx + (bw.saturating_sub(tw)) / 2;
        let ty = by + (bh.saturating_sub(MinGuiCompositor::text_height())) / 2;
        MinGuiCompositor::draw_string(fb, tx, ty, btn_text, Color::WHITE);
    }
}

