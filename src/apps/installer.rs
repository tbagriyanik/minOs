use crate::drivers::ata;
use crate::drivers::video::Framebuffer;
use crate::fs::diskfs;
use crate::fs::vfs::VirtualFileSystem;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};

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
    #[cfg(feature = "exfat")]
    Exfat,
}

impl FormatFsType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Fat32 => "FAT32",
            Self::Ext4 => "ext4",
            #[cfg(feature = "exfat")]
            Self::Exfat => "exFAT",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InstallerAction {
    None,
    Format,
    Install,
    Cancel,     // İptal/geri dönüş
    ConfirmYes, // Onay: Evet
    ConfirmNo,  // Onay: Hayır
}

pub struct InstallerApp {
    pub selected_drive: u8,
    pub format_type: FormatFsType,
    pub state: InstallState,
    pub status_msg: (&'static str, &'static str),
    pub progress_pct: usize,
    pub drive_count: usize,
    pub focused_control: usize,
    /// Secili disk bicimlendirildi mi? (Biçimlendir adımı)
    pub disk_formatted: bool,
    /// Kullanicinin "Tek Tıkla Kur" icin verdigi onay (2. tiklamada kurar).
    pub confirm_install: bool,
    /// Onay diyalog durumu: 0=yok, 1=Biçimlendirme onayı, 2=Kurulum onayı
    pub confirm_dialog: u8,
    /// Onay diyalogunda odaklı buton: 0=Evet, 1=Hayır
    pub confirm_focus: usize,
    /// Güçlü (yazılı) biçimlendirme onayı tamponu: diyaloğa yazılan metin.
    /// Kesin onay için `format_confirm_word()` ile birebir eşleşmesi gerekir.
    confirm_typed: [u8; Self::CONFIRM_WORD_MAX],
    confirm_typed_len: usize,
    /// Onay kelimesi hatalı yazıldığında/Enter erken basıldığında gösterilir.
    confirm_error: bool,
}

impl InstallerApp {
    /// Yazılı onay kelimesinin tampon uzunluğu.
    pub const CONFIRM_WORD_MAX: usize = 12;

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
            focused_control: 0,
            disk_formatted: false,
            confirm_install: false,
            confirm_dialog: 0,
            confirm_focus: 0,
            confirm_typed: [0; Self::CONFIRM_WORD_MAX],
            confirm_typed_len: 0,
            confirm_error: false,
        }
    }

    /// Biçimlendirme için yazılması gereken onay kelimesi (TR/EN).
    pub fn format_confirm_word(english: bool) -> &'static str {
        if english {
            "ERASE"
        } else {
            "SIL"
        }
    }

    /// Yazılan onay metnini döndürür.
    pub fn confirm_typed(&self) -> &str {
        core::str::from_utf8(&self.confirm_typed[..self.confirm_typed_len]).unwrap_or("")
    }

    /// Güçlü onay tamamlandı mı? (kelime birebir eşleşiyor)
    pub fn format_confirm_satisfied(&self) -> bool {
        self.confirm_typed_len == Self::format_confirm_word(crate::gui::ui_english()).len()
            && self
                .confirm_typed()
                .eq_ignore_ascii_case(Self::format_confirm_word(crate::gui::ui_english()))
    }

    /// Biçimlendirme onay diyaloğuna karakter yazar.
    pub fn confirm_typed_push(&mut self, ch: char) {
        if !ch.is_ascii_alphanumeric() {
            return;
        }
        if self.confirm_typed_len < Self::CONFIRM_WORD_MAX {
            self.confirm_typed[self.confirm_typed_len] = ch.to_ascii_uppercase() as u8;
            self.confirm_typed_len += 1;
            self.confirm_error = false;
        }
    }

    /// Yazılan son karakteri siler.
    pub fn confirm_typed_backspace(&mut self) {
        if self.confirm_typed_len > 0 {
            self.confirm_typed_len -= 1;
            self.confirm_error = false;
        }
    }

    /// Yazılı onayı temizler (diyalog kapanırken/sıfırlanırken).
    pub fn confirm_typed_clear(&mut self) {
        self.confirm_typed_len = 0;
        self.confirm_error = false;
    }

    pub fn set_format_type(&mut self, fmt: FormatFsType) {
        if self.format_type != fmt {
            self.format_type = fmt;
            self.disk_formatted = false;
            self.confirm_install = false;
            self.confirm_dialog = 0;
            self.confirm_focus = 0;
            self.confirm_typed_clear();
        }
    }

    /// Etkin onay diyaloğu var mı (Esc birincil durumu kapatır)?
    pub fn has_primary_dialog(&self) -> bool {
        self.confirm_dialog != 0 || self.confirm_install
    }

    pub fn cycle_format_type(&mut self) {
        let next = match self.format_type {
            FormatFsType::Fat32 => FormatFsType::Ext4,
            #[cfg(feature = "exfat")]
            FormatFsType::Ext4 => FormatFsType::Exfat,
            #[cfg(not(feature = "exfat"))]
            FormatFsType::Ext4 => FormatFsType::Fat32,
            #[cfg(feature = "exfat")]
            FormatFsType::Exfat => FormatFsType::Fat32,
        };
        self.set_format_type(next);
    }

    fn format_choice_count() -> usize {
        if cfg!(feature = "exfat") {
            3
        } else {
            2
        }
    }

    pub fn focus_next_control(&mut self, reverse: bool) {
        let count = Self::format_choice_count() + 3;
        self.focused_control = if reverse {
            (self.focused_control + count - 1) % count
        } else {
            (self.focused_control + 1) % count
        };
    }

    pub fn activate_focused_control(&mut self) -> InstallerAction {
        // Eğer onay diyalogu açıksa, Enter/Space ile odaklı butonu tetikle
        if self.confirm_dialog != 0 {
            if self.confirm_focus == 0 {
                // Güçlü onay: biçimlendirmede yazılı kelime zorunlu.
                if self.confirm_dialog == 1 && !self.format_confirm_satisfied() {
                    self.confirm_error = true;
                    self.status_msg = (
                        "Onay için istenen kelimeyi yazın (biçimlendirme verileri siler).",
                        "Type the requested word to confirm (formatting erases data).",
                    );
                    return InstallerAction::None;
                }
                let dialog = self.confirm_dialog;
                self.confirm_dialog = 0;
                self.confirm_focus = 0;
                self.confirm_typed_clear();
                if dialog == 1 {
                    // Biçimlendirme onayı
                    return InstallerAction::Format;
                } else {
                    // Kurulum onayı
                    return InstallerAction::Install;
                }
            } else {
                // Hayır - iptal
                self.confirm_dialog = 0;
                self.confirm_focus = 0;
                self.confirm_install = false;
                self.confirm_typed_clear();
                return InstallerAction::Cancel;
            }
        }

        // Disk seçimi (focused_control == 0)
        if self.focused_control == 0 {
            if !self.disk_formatted {
                // Disk seçiliyken ve biçimlendirilmemişse: disk değiştir
                self.select_next_drive();
                return InstallerAction::None;
            }
            // Disk zaten biçimlendirilmişse: disk değiştir
            self.select_next_drive();
            return InstallerAction::None;
        }

        let format_index = self.focused_control - 1;
        // Dosya sistemi seçimi
        if format_index < Self::format_choice_count() {
            match format_index {
                0 => self.set_format_type(FormatFsType::Fat32),
                1 => self.set_format_type(FormatFsType::Ext4),
                #[cfg(feature = "exfat")]
                2 => self.set_format_type(FormatFsType::Exfat),
                _ => {}
            }
            return InstallerAction::None;
        }

        // Biçimlendir butonu
        if format_index == Self::format_choice_count() {
            if !self.disk_formatted
                && self.state != InstallState::Formatting
                && self.state != InstallState::Copying
            {
                self.open_format_confirm();
                return InstallerAction::None;
            }
            return InstallerAction::None; // Zaten biçimlendirme yapılıyorsa veya formatlıysa bir şey yapma
        }

        // Tek Tıkla Kur butonu
        if self.state != InstallState::Formatting && self.state != InstallState::Copying {
            if self.disk_formatted {
                // Disk zaten biçimlendirilmiş: doğrudan kur
                return InstallerAction::Install;
            } else if self.confirm_install {
                // İkinci tıklama: onay diyaloğu aç
                self.confirm_dialog = 2; // 2 = kurulum onayı (biçimlendirme + kurulum)
                self.confirm_focus = 0;
                self.confirm_typed_clear();
                return InstallerAction::None;
            } else {
                // İlk tıklama: onay bayrağını aç ve mesaj göster
                self.confirm_install = true;
                self.status_msg = (
                    "UYARI: Disk silinip biçimlendirilecek. Onaylamak için tekrar tıklayın veya Enter basın.",
                    "WARNING: Disk will be erased and formatted. Click again or press Enter to confirm.",
                );
                return InstallerAction::None;
            }
        }

        InstallerAction::None
    }

    /// Güçlü biçimlendirme onayı diyaloğunu açar (yazılı kelime zorunlu).
    pub fn open_format_confirm(&mut self) {
        self.confirm_dialog = 1;
        self.confirm_focus = 0;
        self.confirm_typed_clear();
    }

    // Onay diyaloğu için klavye navigasyonu
    pub fn confirm_focus_next(&mut self, reverse: bool) {
        if self.confirm_dialog != 0 {
            self.confirm_focus = if reverse { 1 } else { 0 };
        }
    }

    // Onay diyaloğu kapat (Esc ile)
    pub fn confirm_cancel(&mut self) {
        if self.confirm_dialog != 0 {
            self.confirm_dialog = 0;
            self.confirm_focus = 0;
            self.confirm_install = false;
            self.confirm_typed_clear();
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
        self.confirm_dialog = 0;
        self.confirm_focus = 0;
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
        let english = crate::gui::ui_english();
        // Disk seçeneği için ikinci onay:Henüz 'Tek Tıkla Kur' yapılmadıysa biçimlendirme uyarısı
        if !self.confirm_install && !self.disk_formatted {
            let mut confirm = TextBuf::new();
            confirm.push_str(if english {
                "\n⚠ Are you sure you want to format drive "
            } else {
                "\n⚠ Bu diske biçimlendirmek istiyor musunuz?"
            });
            confirm.push_usize(self.selected_drive as usize);
            confirm.push_str(if english {
                "? This will erase all data! (y/n)"
            } else {
                "? Tüm verileri silecek! (e/h)"
            });
            // Wait for user confirmation
            return false;
        }

        // Disk bölümleme/biçimlendirme uzun sürer; imleç kum saati olsun.
        let _busy = crate::gui::screens::BusyScope::new();
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
        // Her sektör yazımı gerçek bir adımdır: ilerleme yüzdesi aşama
        // sayısından değil, yazılan sektör sayısından hesaplanır (%10→%35).
        let stage_sectors = Self::LIMINE_MBR_STAGE.len().div_ceil(512);
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
            let done = s + 1;
            self.progress_pct = 10 + (done * 25 / stage_sectors.max(1));
        }
        self.progress_pct = 35;

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
        let part_sectors = cap.saturating_sub(part_lba as u64) as u32;
        let o = 0x1BE; // Birincil bölüm girdisi
        mbr[o] = 0x80; // Aktif / Bootable
        mbr[o + 1] = 0x20;
        mbr[o + 2] = 0x21;
        mbr[o + 3] = 0x00; // CHS start
        mbr[o + 4] = match self.format_type {
            FormatFsType::Fat32 => 0x0C, // FAT32 LBA
            FormatFsType::Ext4 => 0x83,  // Linux native
            #[cfg(feature = "exfat")]
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
        self.progress_pct = 40;

        // 3. Bölüm 1 içine (LBA 2048) seçilen dosya sistemini kur
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
            #[cfg(feature = "exfat")]
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
            self.progress_pct = 0;
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
            #[cfg(feature = "exfat")]
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
        // Kurulum dosya kopyalama içerir; imleç kum saati olsun.
        let _busy = crate::gui::screens::BusyScope::new();
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
        if !self.disk_formatted && !self.format_selected_drive() {
            return false;
        }

        self.state = InstallState::Copying;
        self.progress_pct = 65;
        self.status_msg = (
            "Sistem dosyaları ve bağımlılıklar kopyalanıyor...",
            "Copying system files and dependencies...",
        );

        // exFAT okuma/yazma sürücüsü olmadığı için kurulum desteklenmez.
        // Biçimlendirme başarılı olabilir ama dosya yazılamaz.
        #[cfg(feature = "exfat")]
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

    /// Seçili hedef diskin toplam baytı (yoksa `None`).
    pub fn selected_drive_bytes(drive: u8) -> Option<u64> {
        ata::device(drive)
            .filter(|d| d.kind == ata::DeviceKind::Ata)
            .map(|d| d.sectors * 512)
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

        // 3. Disk Seçim Kartı - Tüm mevcut ATA sürücüleri listele
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

        // Tüm ATA sürücülerini topla
        let mut drives = alloc::vec::Vec::new();
        for i in 0..4 {
            if let Some(dev) = ata::device(i) {
                if dev.kind == ata::DeviceKind::Ata {
                    let total_bytes = dev.sectors * 512;
                    let total_mb = total_bytes / 1024 / 1024;
                    let model = dev.model_str();
                    let model_display: alloc::string::String = if model.is_empty() {
                        if english {
                            alloc::string::String::from("ATA Hard Disk")
                        } else {
                            alloc::string::String::from("ATA Sabit Disk")
                        }
                    } else {
                        let bytes = model.as_bytes();
                        let take = bytes.len().min(24);
                        alloc::string::String::from(
                            core::str::from_utf8(&bytes[..take]).unwrap_or("Unknown"),
                        )
                    };
                    let is_selected = i == self.selected_drive;
                    drives.push((i, total_mb, model_display, is_selected));
                }
            }
        }

        if drives.is_empty() {
            let msg = if english {
                "No ATA drives detected"
            } else {
                "ATA sürücü bulunamadı"
            };
            MinGuiCompositor::draw_string(
                fb,
                sel_rect.0
                    + (sel_rect
                        .2
                        .saturating_sub(MinGuiCompositor::text_width(msg, 1)))
                        / 2,
                sel_rect.1 + (sel_rect.3.saturating_sub(MinGuiCompositor::text_height())) / 2,
                msg,
                Color::ACCENT_RED,
            );
        } else {
            // Sürücüleri yatay kartlar olarak çiz
            let card_w = (sel_rect.2.saturating_sub(sp(20) * (drives.len() + 1))) / drives.len();
            let card_h = sel_rect.3 - sp(10);
            let start_x = sel_rect.0 + sp(10);
            let card_y = sel_rect.1 + sp(5);

            for (idx, (drive_id, total_mb, model, is_selected)) in drives.iter().enumerate() {
                let card_x = start_x + idx * (card_w + sp(10));

                // Kart arka planı
                let bg = if *is_selected {
                    Color::ACCENT_BLUE
                } else {
                    Color::WINDOW_BG
                };
                fb.draw_rounded_rect(card_x, card_y, card_w, card_h, 4, bg);
                fb.draw_rounded_rect_outline(
                    card_x,
                    card_y,
                    card_w,
                    card_h,
                    4,
                    if *is_selected {
                        Color::WHITE
                    } else {
                        Color::BORDER
                    },
                );

                // Sürücü numarası
                let mut num_buf = TextBuf::new();
                num_buf.push_str(if english { "Drive " } else { "Sürücü " });
                num_buf.push_usize(*drive_id as usize);
                let num_color = if *is_selected {
                    Color::WHITE
                } else {
                    Color::TEXT_MAIN
                };
                MinGuiCompositor::draw_string(
                    fb,
                    card_x + sp(8),
                    card_y + sp(4),
                    num_buf.as_str(),
                    num_color,
                );

                // Boyut
                let mut size_buf = TextBuf::new();
                size_buf.push_usize(*total_mb as usize);
                size_buf.push_str(" MB");
                MinGuiCompositor::draw_string(
                    fb,
                    card_x + sp(8),
                    card_y + sp(22),
                    size_buf.as_str(),
                    if *is_selected {
                        Color::ACCENT_YELLOW
                    } else {
                        Color::TEXT_MAIN
                    },
                );

                // Model
                let model_x = card_x + sp(8);
                let model_w = card_w - sp(16);
                MinGuiCompositor::draw_string_clipped(
                    fb,
                    model_x,
                    card_y + sp(38),
                    model,
                    Color::TEXT_MUTED,
                    (model_x, card_y + sp(36), model_w, sp(18)),
                );

                // Seçili ise alt çizgi
                if *is_selected {
                    fb.draw_rect(
                        card_x + sp(4),
                        card_y + card_h - 2,
                        card_w - sp(8),
                        2,
                        Color::WHITE,
                    );
                }
            }

            // İpucu
            let hint = if english {
                "(Click or Tab to switch drive)"
            } else {
                "(Diski değiştirmek için tıklayın veya Tab)"
            };
            let hint_w = MinGuiCompositor::text_width(hint, 1);
            let hint_x = sel_rect.0 + (sel_rect.2.saturating_sub(hint_w)) / 2;
            MinGuiCompositor::draw_string(
                fb,
                hint_x,
                sel_rect.1 + sel_rect.3 - sp(16),
                hint,
                Color::ACCENT_YELLOW,
            );
        }

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
        let switch_hint = if english {
            "(Click to switch drive)"
        } else {
            "(Diski değiştirmek için tıklayın)"
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
            #[cfg(feature = "exfat")]
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
            #[cfg(feature = "exfat")]
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

        // 4b. Tablo: hedef disk özeti (kapasite, biçim, kullanılabilir alan)
        let summary_y = win_y + 198;
        let summary_w = win_w.saturating_sub(40);
        fb.draw_rounded_rect(win_x + 20, summary_y, summary_w, 24, 4, Color::TASKBAR_BG);
        fb.draw_rounded_rect_outline(win_x + 20, summary_y, summary_w, 24, 4, Color::BORDER);
        let mut summary = TextBuf::new();
        summary.push_str(if english {
            "Target: Drive "
        } else {
            "Hedef: Sürücü "
        });
        summary.push_usize(self.selected_drive as usize);
        match Self::selected_drive_bytes(self.selected_drive) {
            Some(bytes) => {
                summary.push_str(" • ");
                summary.push_usize((bytes / (1024 * 1024)) as usize);
                summary.push_str(" MB • ");
            }
            None => {
                summary.push_str(" • ");
                summary.push_str(if english { "no disk" } else { "disk yok" });
                summary.push_str(" • ");
            }
        }
        summary.push_str(self.format_type.name());
        summary.push_str(" • ");
        if self.disk_formatted {
            summary.push_str(if english { "ready" } else { "hazır" });
        } else {
            summary.push_str(if english {
                "will be erased"
            } else {
                "silinecek"
            });
        }
        MinGuiCompositor::draw_string_clipped(
            fb,
            win_x + 28,
            summary_y + (24usize.saturating_sub(MinGuiCompositor::text_height())) / 2,
            summary.as_str(),
            if self.disk_formatted {
                Color::ACCENT_GREEN
            } else {
                Color::ACCENT_YELLOW
            },
            (win_x + 24, summary_y, summary_w.saturating_sub(8), 24),
        );

        // 5. İlerleme Çubuğu (yüzde metni ile)
        let prog_y = win_y + 280;
        let prog_w = win_w.saturating_sub(40);
        let bar_col = match self.state {
            InstallState::Complete => Color::ACCENT_GREEN,
            InstallState::Failed => Color::ACCENT_RED,
            _ => Color::ACCENT_BLUE,
        };
        MinGuiCompositor::render_toast_progress_bar(
            fb,
            (win_x + 20, prog_y, prog_w, 14),
            self.progress_pct.min(100) as u8,
            bar_col,
        );
        let mut pct = TextBuf::new();
        pct.push_usize(self.progress_pct.min(100));
        pct.push_str("%");
        MinGuiCompositor::draw_string(
            fb,
            win_x + win_w.saturating_sub(20 + MinGuiCompositor::text_width(pct.as_str(), 1)),
            prog_y.saturating_sub(16),
            pct.as_str(),
            Color::TEXT_MUTED,
        );

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

        let focused_rect = if self.focused_control == 0 {
            Some(Self::drive_select_rect(win_x, win_y, win_w))
        } else if self.focused_control <= Self::format_choice_count() {
            Some(Self::radio_btn_rect(
                win_x,
                win_y + 20,
                self.focused_control - 1,
            ))
        } else if self.focused_control == Self::format_choice_count() + 1 {
            Some((fx, fy, fw, fh))
        } else {
            Some((bx, by, bw, bh))
        };
        if let Some((x, y, w, h)) = focused_rect {
            MinGuiCompositor::draw_focus_ring(fb, x, y, w, h);
        }

        // Onay diyaloğu overlay'i
        if self.confirm_dialog != 0 {
            self.render_confirm_dialog(fb, win_x, win_y, win_w, win_h, english);
        }
    }

    /// Onay diyaloğunu çizer (modal overlay)
    fn render_confirm_dialog(
        &self,
        fb: &mut Framebuffer,
        win_x: usize,
        win_y: usize,
        win_w: usize,
        win_h: usize,
        english: bool,
    ) {
        // Yarı saydam arka plan
        fb.draw_rect(win_x, win_y, win_w, win_h, Color::DARK_BG);

        // Diyalog kutusu
        let dlg_w = win_w.min(480);
        let dlg_h = sp(190);
        let dlg_x = win_x + (win_w.saturating_sub(dlg_w)) / 2;
        let dlg_y = win_y + (win_h.saturating_sub(dlg_h)) / 2;

        fb.draw_rounded_rect(dlg_x, dlg_y, dlg_w, dlg_h, 8, Color::WINDOW_BG);
        fb.draw_rounded_rect_outline(dlg_x, dlg_y, dlg_w, dlg_h, 8, Color::ACCENT_YELLOW);

        // Başlık
        let title = if self.confirm_dialog == 1 {
            if english {
                "⚠ Confirm Format"
            } else {
                "⚠ Biçimlendirme Onayı"
            }
        } else {
            if english {
                "⚠ Confirm Install"
            } else {
                "⚠ Kurulum Onayı"
            }
        };
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + (dlg_w.saturating_sub(MinGuiCompositor::text_width(title, 1))) / 2,
            dlg_y + sp(16),
            title,
            Color::ACCENT_RED,
        );

        // Mesaj
        let msg1 = if self.confirm_dialog == 1 {
            if english {
                "This will ERASE ALL DATA on drive"
            } else {
                "Bu işlem sürücüdeki TÜM VERİLERİ SİLECEK"
            }
        } else {
            if english {
                "Format drive and install minOs?"
            } else {
                "Diski biçimlendirip minOs kurulsun mu?"
            }
        };
        let msg2 = if english {
            "Press Enter/Y for Yes, Esc/N for No"
        } else {
            "Enter/E=Evet, Esc/H=Hayır"
        };

        MinGuiCompositor::draw_string(fb, dlg_x + sp(20), dlg_y + sp(50), msg1, Color::TEXT_MAIN);

        let mut drive_msg = TextBuf::new();
        drive_msg.push_str(if english { "Drive " } else { "Sürücü " });
        drive_msg.push_usize(self.selected_drive as usize);
        drive_msg.push_str(if english {
            " will be formatted as "
        } else {
            " "
        });
        drive_msg.push_str(self.format_type.name());
        if let Some(bytes) = Self::selected_drive_bytes(self.selected_drive) {
            drive_msg.push_str(" • ");
            drive_msg.push_usize((bytes / (1024 * 1024)) as usize);
            drive_msg.push_str(" MB");
        }
        MinGuiCompositor::draw_string(
            fb,
            dlg_x + sp(20),
            dlg_y + sp(70),
            drive_msg.as_str(),
            Color::ACCENT_YELLOW,
        );

        // Güçlü onay: yazılması gereken kelime ve ilerleme göstergesi.
        // (Yazılı onay yalnızca biçimlendirme diyaloğunda zorunludur.)
        let mut hint_y = dlg_y + sp(92);
        if self.confirm_dialog == 1 {
            let word = Self::format_confirm_word(english);
            let mut type_line = TextBuf::new();
            type_line.push_str(if english {
                "Type to confirm: "
            } else {
                "Onay için yazın: "
            });
            type_line.push_str(word);
            type_line.push_str("  ");
            MinGuiCompositor::draw_string(
                fb,
                dlg_x + sp(20),
                hint_y,
                type_line.as_str(),
                Color::TEXT_MAIN,
            );
            let typed_x = dlg_x + sp(20) + MinGuiCompositor::text_width(type_line.as_str(), 1);
            let box_w = sp(110);
            let box_h = sp(20);
            let box_bg = if self.confirm_error {
                Color::ACCENT_RED
            } else {
                Color::DARK_BG
            };
            fb.draw_rect(typed_x, hint_y - 3, box_w, box_h, box_bg);
            fb.draw_rect_outline(typed_x, hint_y - 3, box_w, box_h, Color::BORDER);
            MinGuiCompositor::draw_string_clipped(
                fb,
                typed_x + sp(6),
                hint_y,
                self.confirm_typed(),
                if self.confirm_error {
                    Color::WHITE
                } else {
                    Color::ACCENT_GREEN
                },
                (typed_x, hint_y - 3, box_w, box_h),
            );
            hint_y += sp(22);
        }

        MinGuiCompositor::draw_string(fb, dlg_x + sp(20), hint_y, msg2, Color::TEXT_MUTED);

        // Evet / Hayır butonları
        let btn_w = sp(80);
        let btn_h = sp(28);
        let btn_gap = sp(20);
        let btn_y = dlg_y + dlg_h - btn_h - sp(16);
        let total_btn_w = btn_w * 2 + btn_gap;
        let btn_start_x = dlg_x + (dlg_w.saturating_sub(total_btn_w)) / 2;

        // Evet butonu (sol)
        let yes_x = btn_start_x;
        let yes_bg = if self.confirm_focus == 0 {
            Color::ACCENT_GREEN
        } else {
            Color::WINDOW_BG
        };
        fb.draw_rounded_rect(yes_x, btn_y, btn_w, btn_h, 6, yes_bg);
        fb.draw_rounded_rect_outline(yes_x, btn_y, btn_w, btn_h, 6, Color::WHITE);
        let yes_text = if english { "Yes (Y)" } else { "Evet (E)" };
        let yes_tw = MinGuiCompositor::text_width(yes_text, 1);
        MinGuiCompositor::draw_string(
            fb,
            yes_x + (btn_w.saturating_sub(yes_tw)) / 2,
            btn_y + (btn_h.saturating_sub(MinGuiCompositor::text_height())) / 2,
            yes_text,
            MinGuiCompositor::text_on(yes_bg),
        );

        // Hayır butonu (sağ)
        let no_x = btn_start_x + btn_w + btn_gap;
        let no_bg = if self.confirm_focus == 1 {
            Color::ACCENT_RED
        } else {
            Color::WINDOW_BG
        };
        fb.draw_rounded_rect(no_x, btn_y, btn_w, btn_h, 6, no_bg);
        fb.draw_rounded_rect_outline(no_x, btn_y, btn_w, btn_h, 6, Color::WHITE);
        let no_text = if english { "No (N)" } else { "Hayır (H)" };
        let no_tw = MinGuiCompositor::text_width(no_text, 1);
        MinGuiCompositor::draw_string(
            fb,
            no_x + (btn_w.saturating_sub(no_tw)) / 2,
            btn_y + (btn_h.saturating_sub(MinGuiCompositor::text_height())) / 2,
            no_text,
            MinGuiCompositor::text_on(no_bg),
        );
    }
}

impl Default for InstallerApp {
    fn default() -> Self {
        Self::new()
    }
}
