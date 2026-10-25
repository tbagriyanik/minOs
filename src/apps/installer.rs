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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FormatFsType {
    Fat32,
    Ext4,
    Exfat,
}

impl FormatFsType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fat32 => "FAT32",
            Self::Ext4 => "ext4",
            Self::Exfat => "exFAT",
        }
    }
}

pub struct InstallerApp {
    pub selected_drive: u8,
    pub format_type: FormatFsType,
    pub state: InstallState,
    pub status_msg: (&'static str, &'static str),
    pub progress_pct: usize,
    pub drive_count: usize,
    /// Secili disk bicimlendirildi mi? (Biçimlendir adımı)
    pub disk_formatted: bool,
    /// Kullanicinin "Tek Tıkla Kur" icin verdigi onay (2. tiklamada kurar).
    pub confirm_install: bool,
}

impl InstallerApp {
    pub const fn new() -> Self {
        Self {
            selected_drive: 0,
            format_type: FormatFsType::Fat32,
            state: InstallState::Selecting,
            status_msg: (
                "Hedef diski seçip önce 'Biçimlendir', sonra 'Tek Tıkla Kur' basınız.",
                "Select target disk, click 'Format' first, then 'One-Click Install'.",
            ),
            progress_pct: 0,
            drive_count: 0,
            disk_formatted: false,
            confirm_install: false,
        }
    }

    pub fn set_format_type(&mut self, fmt: FormatFsType) {
        if self.format_type != fmt {
            self.format_type = fmt;
            self.disk_formatted = false;
            self.confirm_install = false;
        }
    }

    pub fn cycle_format_type(&mut self) {
        let next = match self.format_type {
            FormatFsType::Fat32 => FormatFsType::Ext4,
            FormatFsType::Ext4 => FormatFsType::Exfat,
            FormatFsType::Exfat => FormatFsType::Fat32,
        };
        self.set_format_type(next);
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
        self.disk_formatted = diskfs::fat32_mounted_on(self.selected_drive);
        if self.disk_formatted && diskfs::is_installed() {
            self.state = InstallState::Selecting;
            self.progress_pct = 100;
            self.status_msg = (
                "minOs kurulu. Yeniden biçimlendirebilir veya tekrar kurabilirsiniz.",
                "minOs installed. You can reformat or reinstall.",
            );
        } else if self.disk_formatted {
            self.state = InstallState::Selecting;
            self.progress_pct = 55;
            self.status_msg = (
                "Disk biçimlendirilmiş. 'Tek Tıkla Kur' ile kurulum yapabilirsiniz.",
                "Disk formatted. You can proceed with 'One-Click Install'.",
            );
        } else {
            self.state = InstallState::Selecting;
            self.progress_pct = 0;
            self.status_msg = (
                "Hedef diski seçip önce 'Biçimlendir', sonra 'Tek Tıkla Kur' basınız.",
                "Select target disk, click 'Format' first, then 'One-Click Install'.",
            );
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
        // Disk degisti: biçimlendirme ve kurulum durumu sifirlanir.
        self.confirm_install = false;
        self.refresh_drives();
    }

    /// Gömülü Limine MBR Stage 1 & 2 (22016 bayt, 43 sektör)
    const LIMINE_MBR_STAGE: &'static [u8] = include_bytes!("../../boot/limine/limine-mbr.bin");
    /// Gömülü Limine BIOS sistem dosyası (/boot/limine/limine-bios.sys)
    const LIMINE_BIOS_SYS: &'static [u8] = include_bytes!("../../boot/limine/limine-bios.sys");
    /// Gömülü Limine kedi duvar kağıdı (/boot/cat_small.png)
    const CAT_WALLPAPER_PNG: &'static [u8] = include_bytes!("../../boot/cat_small.png");

    /// Hedef diski MBR + dosya sistemi (LBA 2048) olarak bölümler, Limine MBR önyükleyicisini kurar
    /// ve seçili dosya sistemini hazırlar.
    pub fn format_selected_drive(&mut self) -> bool {
        self.state = InstallState::Formatting;
        self.progress_pct = 20;
        self.status_msg = (
            "Disk bölümleniyor ve MBR önyükleyici kuruluyor...",
            "Partitioning disk and installing MBR bootloader...",
        );

        let drive = self.selected_drive;
        if !ata::device(drive)
            .map(|d| d.kind == ata::DeviceKind::Ata)
            .unwrap_or(false)
        {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: Seçili sürücüde hedef disk yok!",
                "ERROR: No target disk on selected drive!",
            );
            return false;
        }

        let cap = ata::drive_capacity_sectors(drive);
        if cap < 65_600 {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: Disk kapasitesi kurulum için yetersiz!",
                "ERROR: Disk capacity insufficient for installation!",
            );
            return false;
        }

        // 1. Limine MBR Stage sektörlerini (LBA 0..42) diske yaz
        let stage_sectors = (Self::LIMINE_MBR_STAGE.len() + 511) / 512;
        for s in 0..stage_sectors {
            let mut sec = [0u8; 512];
            let start = s * 512;
            let end = (start + 512).min(Self::LIMINE_MBR_STAGE.len());
            sec[..end - start].copy_from_slice(&Self::LIMINE_MBR_STAGE[start..end]);
            if !ata::write_sector(drive, s as u32, &sec) {
                self.state = InstallState::Failed;
                self.status_msg = (
                    "HATA: Limine MBR kodları diske yazılamadı!",
                    "ERROR: Failed to write Limine MBR code to disk!",
                );
                return false;
            }
        }

        // 2. LBA 0 üzerine MBR Bölümleme Tablosunu mühürle
        // Bölüm 1: LBA 2048 (1MiB hizalı), seçili türe göre bölüm ID'si, Bootable (0x80)
        let mut mbr = [0u8; 512];
        if !ata::read_sector(drive, 0, &mut mbr) {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: MBR sektörü okunamadı!",
                "ERROR: Failed to read MBR sector!",
            );
            return false;
        }
        let part_lba: u32 = 2048;
        let part_sectors = (cap.saturating_sub(part_lba as u64) as u32).min(0xFFFF_FFFF);
        let o = 0x1BE; // Birincil bölüm girdisi
        mbr[o] = 0x80; // Aktif / Bootable
        mbr[o + 1] = 0x20;
        mbr[o + 2] = 0x21;
        mbr[o + 3] = 0x00; // CHS start
        mbr[o + 4] = match self.format_type {
            FormatFsType::Fat32 => 0x0C, // FAT32 LBA
            FormatFsType::Ext4 => 0x83,  // Linux native
            FormatFsType::Exfat => 0x07, // IFS / exFAT
        };
        mbr[o + 5] = 0xFE;
        mbr[o + 6] = 0x3F;
        mbr[o + 7] = 0x1F; // CHS end
        mbr[o + 8..o + 12].copy_from_slice(&part_lba.to_le_bytes());
        mbr[o + 12..o + 16].copy_from_slice(&part_sectors.to_le_bytes());
        mbr[510] = 0x55;
        mbr[511] = 0xAA; // MBR imza
        if !ata::write_sector(drive, 0, &mbr) {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: MBR bölüm tablosu yazılamadı!",
                "ERROR: Failed to write MBR partition table!",
            );
            return false;
        }
        ata::flush_cache(drive);

        // 3. Bölüm 1 içine (LBA 2048) seçilen dosya sistemini kur
        self.progress_pct = 40;
        self.status_msg = ("Dosya sistemi oluşturuluyor...", "Creating filesystem...");
        let success = match self.format_type {
            FormatFsType::Fat32 => {
                diskfs::fat_format_at(drive, part_lba) && diskfs::mount_drive(drive)
            }
            FormatFsType::Ext4 => {
                // ext4::format sonunda ext2 sürücüsüyle mount doğrulaması yapar.
                crate::fs::ext4::format(drive, part_lba, part_sectors as u64)
                    && diskfs::mount_drive(drive)
            }
            FormatFsType::Exfat => {
                // exFAT için okuma/yazma sürücüsü yok: VBR imzası doğrulanır,
                // mount beklenmez. Kurulum bu biçimde desteklenmez.
                if !crate::fs::exfat::format(drive, part_lba, part_sectors as u64) {
                    false
                } else {
                    let mut vbr = [0u8; 512];
                    ata::read_sector(drive, part_lba, &mut vbr)
                        && vbr[3..11] == *b"EXFAT   "
                        && vbr[510] == 0x55
                        && vbr[511] == 0xAA
                }
            }
        };

        if !success {
            self.state = InstallState::Failed;
            self.disk_formatted = false;
            self.status_msg = (
                "HATA: Disk biçimlendirilemedi!",
                "ERROR: Failed to format disk!",
            );
            return false;
        }

        self.disk_formatted = true;
        self.confirm_install = false;
        self.progress_pct = 55;
        self.state = InstallState::Selecting;
        self.status_msg = match self.format_type {
            FormatFsType::Fat32 => (
                "Disk önyüklenebilir yapıldı. 'Tek Tıkla Kur' basınız.",
                "Disk made bootable. Click 'One-Click Install' to proceed.",
            ),
            FormatFsType::Ext4 => (
                "Disk başarıyla biçimlendirildi.",
                "Disk formatted successfully.",
            ),
            FormatFsType::Exfat => (
                "Disk başarıyla biçimlendirildi.",
                "Disk formatted successfully.",
            ),
        };
        true
    }

    /// "Tek Tıkla Kur" işlemi:
    /// 1. (Gerekirse) Hedef diski seçili türde biçimlendirir
    /// 2. Kurulum ve işletim sistemi kimlik/işaret dosyalarını (MINOS.SYS) yazar
    /// 3. VFS'deki mevcut sistem ayarlarını (settings.cfg) diske kalıcı olarak kaydeder
    /// 4. VFS'e bağlar ve sistem durumunu günceller
    pub fn perform_install(&mut self, vfs: &mut VirtualFileSystem) -> bool {
        let drive = self.selected_drive;
        if !ata::device(drive)
            .map(|d| d.kind == ata::DeviceKind::Ata)
            .unwrap_or(false)
        {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: Seçili sürücüde hedef disk yok!",
                "ERROR: No target disk on selected drive!",
            );
            return false;
        }

        // 1. Adim: Disk henuz bicimlendirilmediyse simdi bicimlendir.
        if !self.disk_formatted {
            if !self.format_selected_drive() {
                return false;
            }
        }

        self.state = InstallState::Copying;
        self.progress_pct = 65;
        self.status_msg = (
            "Sistem dosyaları ve bağımlılıklar kopyalanıyor...",
            "Copying system files and dependencies...",
        );

        // exFAT okuma/yazma sürücüsü olmadığı için kurulum desteklenmez.
        // Biçimlendirme başarılı olabilir ama dosya yazılamaz.
        if self.format_type == FormatFsType::Exfat {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: exFAT'e kurulum desteklenmiyor! FAT32 seçip yeniden biçimlendirin.",
                "ERROR: Installation to exFAT not supported! Select FAT32 and reformat.",
            );
            return false;
        }

        // Biçimlendirme sonrası seçili hedefi açıkça bağla; init() daha önce
        // çalıştıysa READY nedeniyle erken dönebilir.
        if !diskfs::mount_drive(drive) {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: Hedef disk dosya sistemi bağlanamadı!",
                "ERROR: Failed to mount target disk filesystem!",
            );
            return false;
        }

        // 2. Kullanıcı benioku dosyasını yaz
        let readme_content = "minOs Kurulumu Başarıyla Tamamlandı!\n====================================\nBu bölümde tam okuma ve yazma hakkınız bulunmaktadır.\nSistem ayarlarınız ve yeni dosyalarınız otomatik olarak bu bölüme kaydedilir.\n".as_bytes();
        if !diskfs::save_file_by_name("BENIOKU.TXT", readme_content) {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: BENIOKU.TXT diske yazılamadı!",
                "ERROR: Failed to write BENIOKU.TXT to disk!",
            );
            return false;
        }

        // 3. Mevcut VFS ayarlarını (veya varsayılanları) diske settings.cfg olarak yaz
        if let Some(cfg_data) = vfs.read_file("settings.cfg") {
            if !diskfs::save_file_by_name("settings.cfg", cfg_data) {
                self.state = InstallState::Failed;
                self.status_msg = (
                    "HATA: settings.cfg diske yazılamadı!",
                    "ERROR: Failed to write settings.cfg to disk!",
                );
                return false;
            }
        } else {
            let default_cfg = b"volume=75;muted=0;res=1280x720;night=0;night_level=55;keyboard=tr;language=tr;zoom=100;region=0;dhcp=1;theme=velvet";
            if !diskfs::save_file_by_name("settings.cfg", default_cfg) {
                self.state = InstallState::Failed;
                self.status_msg = (
                    "HATA: varsayılan settings.cfg yazılamadı!",
                    "ERROR: Failed to write default settings.cfg!",
                );
                return false;
            }
        }

        // 4. Mevcut VFS ağacının tamamını (dosyalar, klasörler, masaüstü,
        // çöp kutusu) VFS.DAT anlık görüntüsü olarak diske yaz; böylece
        // sonraki açılışta tüm dosya/klasör işlemleri geri gelir.
        // Başarısız olursa kurulumu engellemez (en iyi çaba).
        // Önce tam ağacı disk köküne yansıt (gerçek klasör/dosya girişi),
        // sonra anlık görüntüyü kalıcı hale getir.
        // Kurulum işaretini VFS ağacı yansıtılmasından ÖNCE root'a yaz.
        let marker_content = b"minOs Persistent Install\ndata and settings installed\n";
        if !diskfs::save_file_by_name("MINOS.SYS", marker_content) {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: MINOS.SYS kurulum işareti yazılamadı!",
                "ERROR: Failed to write MINOS.SYS installation marker!",
            );
            return false;
        }

        let snapshot_ok = vfs.persist_snapshot();
        let (_mir_dirs, _mir_files) = vfs.sync_all_to_disk();

        // Independently Bootable: Limine 8.x configuration, bios.sys, kernel, and cat_small.png wallpaper.
        let limine_conf_content = b"wallpaper: boot():/boot/cat_small.png\ntimeout: 5\n\n/minOs Installed System\n    protocol: limine\n    kernel_path: boot():/boot/minos-kernel\n    kernel_cmdline: installed\n    resolution: 1280x720x32\n";
        if self.format_type == FormatFsType::Ext4 {
            let _ = diskfs::ext2_mkdir("/boot");
            let _ = diskfs::ext2_mkdir("/boot/limine");
            diskfs::ext2_write("/boot/limine/limine.conf", limine_conf_content);
            diskfs::ext2_write("/boot/limine/limine-bios.sys", Self::LIMINE_BIOS_SYS);
            diskfs::ext2_write("/boot/cat_small.png", Self::CAT_WALLPAPER_PNG);
            if let Some(kernel_bytes) = crate::limine_kernel_binary_slice() {
                diskfs::ext2_write("/boot/minos-kernel", kernel_bytes);
            }
        } else {
            diskfs::fat_write_file_path("/boot/limine/limine.conf", limine_conf_content);
            diskfs::fat_write_file_path("/boot/limine/limine-bios.sys", Self::LIMINE_BIOS_SYS);
            diskfs::fat_write_file_path("/boot/cat_small.png", Self::CAT_WALLPAPER_PNG);
            diskfs::save_file_by_name("limine.conf", limine_conf_content);
            diskfs::save_file_by_name("limine-bios.sys", Self::LIMINE_BIOS_SYS);
            if let Some(kernel_bytes) = crate::limine_kernel_binary_slice() {
                diskfs::fat_write_file_path("/boot/minos-kernel", kernel_bytes);
                diskfs::save_file_by_name("minos-kernel", kernel_bytes);
            }
        }
        diskfs::save_file_by_name("limine.conf", limine_conf_content);
        if let Some(kernel_bytes) = crate::limine_kernel_binary_slice() {
            diskfs::save_file_by_name("minos-kernel", kernel_bytes);
        }

        // Marker en son yazılır: yalnızca gerekli tüm dosyalar diske başarıyla
        // kaydedildiyse sonraki açılış bunu kurulum olarak tanısın.
        let marker_content = b"minOs Persistent Install\ndata and settings installed\n";
        let marker_verified = diskfs::load_file_alloc("MINOS.SYS")
            .map(|bytes| bytes.as_slice() == marker_content)
            .unwrap_or(false);
        if !marker_verified {
            self.state = InstallState::Failed;
            self.status_msg = (
                "HATA: Kurulum diske yazıldı ancak geri okuma doğrulaması başarısız!",
                "ERROR: Installation written to disk but readback verification failed!",
            );
            return false;
        }

        self.progress_pct = 100;
        self.state = InstallState::Complete;
        self.confirm_install = false;
        self.disk_formatted = true;
        self.status_msg = if snapshot_ok {
            (
                "Kurulum tamamlandı! Disk kalıcı yazmaya hazır.",
                "Installation complete! Disk is ready for persistent storage.",
            )
        } else {
            (
                "Kurulum tamamlandı (UYARI: VFS.DAT yazılamadı).",
                "Installation complete (WARNING: Failed to save VFS.DAT).",
            )
        };
        true
    }

    // Geometri yardımcıları

    /// Dosya sistemi radyo butonları için koordinat (0: ilk seçenek, 1: ikinci, 2: üçüncü)
    pub fn radio_btn_rect(
        win_x: usize,
        win_y: usize,
        index: usize,
    ) -> (usize, usize, usize, usize) {
        let rx = win_x + 24 + index * 105;
        let ry = win_y + 180;
        (rx, ry, 95, 20)
    }

    /// "Biçimlendir" butonu — alt satırın solunda.
    pub fn format_btn_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let fmt_w = 175;
        let inst_w = 175;
        let gap = 14;
        let total = fmt_w + gap + inst_w;
        let x = win_x + (win_w.saturating_sub(total)) / 2;
        let y = win_y + win_h.saturating_sub(48);
        (x, y, fmt_w, 28)
    }

    /// "Tek Tıkla Kur" butonu — alt satırın sağında.
    pub fn install_btn_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) -> (usize, usize, usize, usize) {
        let fmt_w = 175;
        let inst_w = 175;
        let gap = 14;
        let total = fmt_w + gap + inst_w;
        let x = win_x + (win_w.saturating_sub(total)) / 2 + fmt_w + gap;
        let y = win_y + win_h.saturating_sub(48);
        (x, y, inst_w, 28)
    }

    pub fn drive_select_rect(
        win_x: usize,
        win_y: usize,
        win_w: usize,
    ) -> (usize, usize, usize, usize) {
        (win_x + 20, win_y + 122, win_w.saturating_sub(40), 48)
    }

    pub fn render_content(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
    ) {
        let english = crate::gui::ui_english();

        // 1. Üst Başlık Banner
        fb.draw_vgradient(
            win_x,
            win_y + 24,
            win_w,
            42,
            Color::TITLE_BAR,
            MinGuiCompositor::shade(Color::TITLE_BAR, -15),
        );
        let header_title = if english {
            "minOs Quick Installation Wizard"
        } else {
            "minOs Hızlı Kurulum Sihirbazı"
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            win_x + 16,
            win_y + 36,
            header_title,
            Color::TEXT_MAIN,
            (win_x + 12, win_y + 28, win_w.saturating_sub(24), 30),
        );

        // 2. Açıklama metni
        let intro_text = if english {
            "Select the target disk, choose format, then click 'One-Click Install':"
        } else {
            "Hedef diski seçin, biçimlendirin ve 'Tek Tıkla Kur'a basın:"
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            win_x + 20,
            win_y + 74,
            intro_text,
            Color::TEXT_MUTED,
            (win_x + 16, win_y + 68, win_w.saturating_sub(32), 20),
        );

        // 2b. Adim göstergesi (1. Disk Seç -> 2. Biçimlendir -> 3. Kur)
        let step_y = win_y + 94;
        let step_fmt_done = self.disk_formatted;
        let step_inst_done = self.state == InstallState::Complete;
        let step_items: [(&str, bool); 3] = if english {
            [
                ("1. Select Disk", true),
                ("2. Format", step_fmt_done),
                ("3. Install", step_inst_done),
            ]
        } else {
            [
                ("1. Disk Seç", true),
                ("2. Biçimlendir", step_fmt_done),
                ("3. Kur", step_inst_done),
            ]
        };
        let mut step_x = win_x + 20;
        for (label, done) in step_items.iter() {
            let col = if *done {
                Color::ACCENT_GREEN
            } else {
                Color::TEXT_MUTED
            };
            MinGuiCompositor::draw_string_clipped(
                fb,
                step_x,
                step_y,
                label,
                col,
                (win_x + 16, step_y - 2, win_w.saturating_sub(32), 20),
            );
            step_x += MinGuiCompositor::text_width(label, 1) + 10;
        }

        // 3. Disk Seçim Kartı
        let sel_rect = Self::drive_select_rect(win_x, win_y, win_w);
        fb.draw_rounded_rect(
            sel_rect.0,
            sel_rect.1,
            sel_rect.2,
            sel_rect.3,
            6,
            Color::TASKBAR_BG,
        );
        fb.draw_rounded_rect_outline(
            sel_rect.0,
            sel_rect.1,
            sel_rect.2,
            sel_rect.3,
            6,
            Color::ACCENT_BLUE,
        );

        // Disk Simge & Bilgisi
        let disk_dev = ata::device(self.selected_drive);
        let mut disk_label = TextBuf::new();
        disk_label.push_str(if english { "Drive " } else { "Sürücü " });
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
            disk_label.push_str(if english {
                "No disk detected"
            } else {
                "Disk tespit edilemedi"
            });
        }
        MinGuiCompositor::draw_string_clipped(
            fb,
            sel_rect.0 + 12,
            sel_rect.1 + 5,
            disk_label.as_str(),
            Color::TEXT_MAIN,
            (
                sel_rect.0 + 10,
                sel_rect.1 + 2,
                sel_rect.2.saturating_sub(20),
                20,
            ),
        );

        let switch_hint = if english {
            "(Click to switch drive)"
        } else {
            "(Diski değiştirmek için tıklayın)"
        };
        // Seçili diskin biçimlendirme/kurulum durumu
        let fmt_state = if self.state == InstallState::Complete {
            if english {
                "[Installed]"
            } else {
                "[Kuruldu]"
            }
        } else if self.disk_formatted {
            if english {
                "[Formatted]"
            } else {
                "[Biçimlendirildi]"
            }
        } else {
            if english {
                "[Not formatted]"
            } else {
                "[Biçimlendirilmedi]"
            }
        };
        let fmt_col = if self.disk_formatted {
            Color::ACCENT_GREEN
        } else {
            Color::ACCENT_RED
        };
        let state_y = sel_rect.1 + 27;
        MinGuiCompositor::draw_string_clipped(
            fb,
            sel_rect.0 + 12,
            state_y,
            fmt_state,
            fmt_col,
            (sel_rect.0 + 10, sel_rect.1 + 23, sel_rect.2 / 2, 20),
        );
        let hint_w = MinGuiCompositor::text_width(switch_hint, 1);
        let hint_x = sel_rect.0 + sel_rect.2.saturating_sub(hint_w + 12);
        MinGuiCompositor::draw_string_clipped(
            fb,
            hint_x,
            state_y,
            switch_hint,
            Color::ACCENT_YELLOW,
            (
                sel_rect.0 + sel_rect.2 / 2,
                sel_rect.1 + 23,
                sel_rect.2 / 2 - 8,
                20,
            ),
        );

        // 4. Dosya Sistemi Seçim Radyo Düğmeleri
        let radio_title = if english {
            "Filesystem Format:"
        } else {
            "Dosya Sistemi Biçimi:"
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            win_x + 22,
            win_y + 178,
            radio_title,
            Color::TEXT_MAIN,
            (win_x + 20, win_y + 176, win_w.saturating_sub(40), 20),
        );

        let fs_options = [
            (FormatFsType::Fat32, "FAT32"),
            (FormatFsType::Ext4, "ext4"),
            (FormatFsType::Exfat, "exFAT"),
        ];

        for (idx, (ftype, fname)) in fs_options.iter().enumerate() {
            let (rx, ry, _rw, _rh) = Self::radio_btn_rect(win_x, win_y + 20, idx);
            let is_selected = self.format_type == *ftype;

            // Radyo dış dairesi (14x14 yuvarlak)
            fb.draw_rounded_rect(rx, ry + 2, 14, 14, 7, Color::DARK_BG);
            let border_col = if is_selected {
                Color::ACCENT_BLUE
            } else {
                Color::BORDER
            };
            fb.draw_rounded_rect_outline(rx, ry + 2, 14, 14, 7, border_col);

            // Radyo seçili iç noktası (8x8 yuvarlak)
            if is_selected {
                fb.draw_rounded_rect(rx + 3, ry + 5, 8, 8, 4, Color::ACCENT_BLUE);
            }

            // Seçenek metni
            let text_col = if is_selected {
                Color::ACCENT_BLUE
            } else {
                Color::TEXT_MUTED
            };
            MinGuiCompositor::draw_string(fb, rx + 18, ry + 2, fname, text_col);
        }

        // 4b. Kurulum Bilgi Kutusu
        let info_y = win_y + 225;
        let info_w = win_w.saturating_sub(40);
        fb.draw_rounded_rect(win_x + 20, info_y, info_w, 42, 6, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(win_x + 20, info_y, info_w, 42, 6, Color::BORDER);

        let info_desc = match self.format_type {
            FormatFsType::Fat32 => {
                if english {
                    "Standard compatibility, bootable Limine MBR partition."
                } else {
                    "Standart uyumluluk, önyüklenebilir Limine MBR bölümü."
                }
            }
            FormatFsType::Ext4 => {
                if english {
                    "Modern Linux filesystem, 256B inodes & extent tree."
                } else {
                    "Modern Linux dosya sistemi, 256B inode ve extent ağacı."
                }
            }
            FormatFsType::Exfat => {
                if english {
                    "Microsoft high-capacity format (VBR only; installation not supported yet)."
                } else {
                    "Microsoft yüksek kapasiteli biçim (yalnızca VBR; kurulum henüz desteklenmiyor)."
                }
            }
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            win_x + 28,
            info_y + 12,
            info_desc,
            Color::ACCENT_GREEN,
            (win_x + 26, info_y + 6, info_w.saturating_sub(12), 24),
        );

        // 5. İlerleme Çubuğu
        let prog_y = win_y + 280;
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
            fb.draw_rounded_rect(
                win_x + 21,
                prog_y + 1,
                filled_w.saturating_sub(2),
                12,
                3,
                bar_col,
            );
        }

        // Durum Mesajı
        let cur_status = if english {
            self.status_msg.1
        } else {
            self.status_msg.0
        };
        MinGuiCompositor::draw_string_clipped(
            fb,
            win_x + 22,
            prog_y + 20,
            cur_status,
            match self.state {
                InstallState::Complete => Color::ACCENT_GREEN,
                InstallState::Failed => Color::ACCENT_RED,
                _ => Color::TEXT_MAIN,
            },
            (win_x + 18, prog_y + 16, win_w.saturating_sub(36), 20),
        );

        // 6. "Biçimlendir" Butonu (sol)
        let (fx, fy, fw, fh) = Self::format_btn_rect(win_x, win_y, win_w, win_h);
        let fmt_bg = if self.disk_formatted {
            Color::ACCENT_GREEN
        } else {
            Color::ACCENT_YELLOW
        };
        fb.draw_rounded_rect(fx, fy, fw, fh, 6, fmt_bg);
        fb.draw_rounded_rect_outline(fx, fy, fw, fh, 6, Color::WHITE);
        let fmt_text = if self.state == InstallState::Formatting {
            if english {
                "Formatting..."
            } else {
                "Biçimlendiriliyor..."
            }
        } else if self.disk_formatted {
            if english {
                "Format (Ready)"
            } else {
                "Biçimlendir (Hazır)"
            }
        } else if english {
            "Format Disk"
        } else {
            "Biçimlendir"
        };
        let ftw = MinGuiCompositor::text_width(fmt_text, 1);
        let ftx = fx + (fw.saturating_sub(ftw)) / 2;
        let fty = fy + (fh.saturating_sub(MinGuiCompositor::text_height())) / 2;
        MinGuiCompositor::draw_string(fb, ftx, fty, fmt_text, MinGuiCompositor::text_on(fmt_bg));

        // 7. "Tek Tıkla Kur" Butonu (sağ)
        let (bx, by, bw, bh) = Self::install_btn_rect(win_x, win_y, win_w, win_h);
        let btn_bg = match self.state {
            InstallState::Complete => Color::ACCENT_GREEN,
            InstallState::Failed => Color::ACCENT_RED,
            InstallState::Formatting | InstallState::Copying => Color::TEXT_MUTED,
            _ if self.confirm_install => Color::ACCENT_YELLOW,
            _ if self.disk_formatted => Color::ACCENT_BLUE,
            _ => Color::ACCENT_BLUE,
        };
        fb.draw_rounded_rect(bx, by, bw, bh, 6, btn_bg);
        fb.draw_rounded_rect_outline(bx, by, bw, bh, 6, MinGuiCompositor::text_on(btn_bg));

        let btn_text = match self.state {
            InstallState::Complete => {
                if english {
                    "Reinstall"
                } else {
                    "Tekrar Kur"
                }
            }
            InstallState::Formatting | InstallState::Copying => {
                if english {
                    "Installing..."
                } else {
                    "Kuruluyor..."
                }
            }
            InstallState::Failed => {
                if english {
                    "Retry"
                } else {
                    "Tekrar Dene"
                }
            }
            _ if self.confirm_install => {
                if english {
                    "Confirm Install"
                } else {
                    "Kurulumu Onayla"
                }
            }
            _ if !self.disk_formatted => {
                if english {
                    "One-Click Install"
                } else {
                    "Tek Tıkla Kur"
                }
            }
            _ => {
                if english {
                    "One-Click Install"
                } else {
                    "Tek Tıkla Kur"
                }
            }
        };
        let tw = MinGuiCompositor::text_width(btn_text, 1);
        let tx = bx + (bw.saturating_sub(tw)) / 2;
        let ty = by + (bh.saturating_sub(MinGuiCompositor::text_height())) / 2;
        MinGuiCompositor::draw_string(fb, tx, ty, btn_text, MinGuiCompositor::text_on(btn_bg));
    }
}
