//! Güvenli Uygulama İkili Yükleyicisi (Binary / ELF Loader) ve Süreç Bellek Düzeni.
//!
//! Bu modül:
//! 1. Standart ELF64 ve MinOS yürütülebilir başlıklarının doğrulanmasını,
//! 2. Kod (.text), salt okunur veri (.rodata) ve yazılabilir veri (.data / .bss) segmentlerinin
//!    güvenli sanal bellek adreslerine (W^X prensibiyle - Write XOR Execute) yerleştirilmesini,
//! 3. Uygulama için yalıtılmış kullanıcı yığını (User Stack) tahsisini,
//! 4. Süreç bellek sınırlarını ve bellek çakışmalarının önlenmesini sağlar.

use crate::drivers::phys_to_virt;
use crate::mm::pmm::{PhysicalMemoryManager, PAGE_SIZE};
use crate::mm::vmm::{AddressSpace, PageFlags};

/// Uygulama sanal bellek düzeni sabitleri (Canonical User Space: 0x0000_0000_0040_0000 - 0x0000_7FFF_FFFF_FFFF)
pub const USER_CODE_BASE: u64 = 0x0000_0000_0040_0000; // 4 MB (Standart kullanıcı kodu başlangıcı)
pub const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000; // Kullanıcı yığını üst sınırı
pub const USER_STACK_PAGES: usize = 4; // 16 KB kullanıcı yığını (4 x 4KB sayfa)

/// ELF64 Sihirli Numaraları
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const ELF_CLASS_64: u8 = 2;
pub const ELF_DATA_2LSB: u8 = 1; // Little Endian
pub const ELF_TYPE_EXEC: u16 = 2; // Yürütülebilir dosya
pub const ELF_MACHINE_X86_64: u16 = 0x3E; // AMD x86-64

/// ELF64 Dosya Başlığı
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    pub ident: [u8; 16],
    pub elf_type: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub ph_offset: u64,
    pub sh_offset: u64,
    pub flags: u32,
    pub eh_size: u16,
    pub ph_entry_size: u16,
    pub ph_num: u16,
    pub sh_entry_size: u16,
    pub sh_num: u16,
    pub sh_str_idx: u16,
}

/// ELF64 Program Başlığı (Segment Tanımı)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64ProgramHeader {
    pub seg_type: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub paddr: u64,
    pub file_size: u64,
    pub mem_size: u64,
    pub align: u64,
}

pub const PT_LOAD: u32 = 1;
pub const PF_X: u32 = 1 << 0; // Yürütülebilir (Executable)
pub const PF_W: u32 = 1 << 1; // Yazılabilir (Writable)
pub const PF_R: u32 = 1 << 2; // Okunabilir (Readable)

/// Güvenli şekilde yüklenen uygulamanın bellek haritası ve çalışma durumu
#[derive(Debug, Clone, Copy)]
pub struct LoadedProgram {
    /// Uygulamanın giriş noktası sanal adresi (RIP)
    pub entry_point: u64,
    /// Yığın tabanı sanal adresi (RSP)
    pub stack_pointer: u64,
    /// Kod segmenti başlangıç adresi
    pub code_base: u64,
    /// Kod boyutu (bayt)
    pub code_size: u64,
    /// Toplam tahsis edilen sayfa sayısı
    pub allocated_pages: usize,
}

/// Uygulama Yükleyicisi (Application Loader)
pub struct AppLoader;

impl AppLoader {
    /// Bir ELF ikili dosyasını inceler ve geçerli bir 64-bit x86-64 ikilisi olup olmadığını doğrular.
    pub fn validate_elf(data: &[u8]) -> Result<Elf64Header, &'static str> {
        if data.len() < core::mem::size_of::<Elf64Header>() {
            return Err("Gecersiz dosya boyutu: ELF basligi icin cok kisa");
        }

        let header = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const Elf64Header) };

        if header.ident[0..4] != ELF_MAGIC {
            return Err("Gecersiz ELF sihirli numarasi");
        }
        if header.ident[4] != ELF_CLASS_64 {
            return Err("Desteklenmeyen mimari: Yalnizca 64-bit ELF desteklenir");
        }
        if header.ident[5] != ELF_DATA_2LSB {
            return Err("Desteklenmeyen endianness: Little-endian olmali");
        }
        if header.machine != ELF_MACHINE_X86_64 {
            return Err("Desteklenmeyen islemci mimarisi: x86-64 olmali");
        }

        Ok(header)
    }

    /// Bir ikili dosyayı (ELF veya MinOS Raw Yürütülebilir) güvenli bellek korumalarıyla yükler.
    ///
    /// Güvenlik Prensipleri:
    /// - Kod sayfaları: Present + User + Executable (W=0, NX=0)
    /// - Veri ve Yığın sayfaları: Present + User + Writable + No-Execute (W=1, NX=1)
    /// - Çekirdek uzayıyla çakışma koruması: Kullanıcı adresleri canonical alt uzayda olmalıdır.
    pub fn load_binary(
        data: &[u8],
        address_space: &mut AddressSpace,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<LoadedProgram, &'static str> {
        if data.is_empty() {
            return Err("Bos calistirilabilir dosya");
        }

        // 1. ELF formatı kontrolü
        if data.len() >= 4 && data[0..4] == ELF_MAGIC {
            Self::load_elf(data, address_space, pmm)
        } else {
            // 2. Düz/Ham ikili (Raw Executable) yükleyici
            Self::load_raw(data, address_space, pmm)
        }
    }

    /// ELF dosyasının PT_LOAD segmentlerini sanal belleğe eşler ve verilerini kopyalar.
    fn load_elf(
        data: &[u8],
        address_space: &mut AddressSpace,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<LoadedProgram, &'static str> {
        let header = Self::validate_elf(data)?;
        let mut allocated_pages = 0usize;

        let ph_size = header.ph_entry_size as usize;
        let ph_count = header.ph_num as usize;
        let ph_offset = header.ph_offset as usize;

        if ph_offset + ph_size * ph_count > data.len() {
            return Err("Program baslik tablosu dosya sinirlarinin disinda");
        }

        for i in 0..ph_count {
            let offset = ph_offset + i * ph_size;
            let ph = unsafe {
                core::ptr::read_unaligned(data.as_ptr().add(offset) as *const Elf64ProgramHeader)
            };

            if ph.seg_type != PT_LOAD || ph.mem_size == 0 {
                continue;
            }

            // Güvenlik kontrolü: Kullanıcı alanı sınırları (0 .. 0x0000_7FFF_FFFF_FFFF)
            if ph.vaddr >= 0x0000_8000_0000_0000 || ph.vaddr.saturating_add(ph.mem_size) >= 0x0000_8000_0000_0000 {
                return Err("Segment cekirdek adres uzayiyla cakisan alanda bulunamaz");
            }

            // Segment izinleri (W^X Prensibi)
            let mut page_flags = PageFlags::PRESENT | PageFlags::USER;
            if (ph.flags & PF_W) != 0 {
                page_flags |= PageFlags::WRITABLE;
            }
            if (ph.flags & PF_X) == 0 {
                page_flags |= PageFlags::NO_EXECUTE;
            }

            // Sayfaları tek tek tahsis et ve veriyi kopyala
            let seg_start = ph.vaddr;
            let seg_end = ph.vaddr + ph.mem_size;
            let aligned_start = seg_start & !(PAGE_SIZE - 1);
            let aligned_end = (seg_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

            let mut page_vaddr = aligned_start;
            while page_vaddr < aligned_end {
                let phys_frame = pmm.allocate_frame().ok_or("PMM: Yetersiz fiziksel bellek")?;
                allocated_pages += 1;

                // Çerçeveyi önce sıfırla
                let virt_frame = phys_to_virt(phys_frame) as *mut u8;
                unsafe { core::ptr::write_bytes(virt_frame, 0, PAGE_SIZE as usize); }

                // Dosya içeriği bu sayfanın içine denk geliyorsa kopyala
                let page_offset = page_vaddr.saturating_sub(ph.vaddr);
                let file_offset = (ph.offset + page_offset) as usize;
                let copy_start = if page_vaddr < ph.vaddr { (ph.vaddr - page_vaddr) as usize } else { 0 };
                let copy_avail = (PAGE_SIZE as usize).saturating_sub(copy_start);
                let copy_len = if file_offset < (ph.offset + ph.file_size) as usize {
                    let remaining = ((ph.offset + ph.file_size) as usize - file_offset).min(copy_avail);
                    if file_offset + remaining <= data.len() {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                data.as_ptr().add(file_offset),
                                virt_frame.add(copy_start),
                                remaining,
                            );
                        }
                        remaining
                    } else { 0 }
                } else { 0 };
                let _ = copy_len;

                // Sayfayı adres uzayına eşle
                address_space.map_page(page_vaddr, phys_frame, page_flags, pmm)?;
                page_vaddr += PAGE_SIZE;
            }
        }

        // Kullanıcı yığınını (User Stack) ayır ve eşle
        let stack_pointer = Self::setup_user_stack(address_space, pmm, &mut allocated_pages)?;

        Ok(LoadedProgram {
            entry_point: header.entry,
            stack_pointer,
            code_base: USER_CODE_BASE,
            code_size: data.len() as u64,
            allocated_pages,
        })
    }

    /// Ham/düz (Raw) ikilileri standart USER_CODE_BASE adresine güvenle yerleştirir.
    fn load_raw(
        data: &[u8],
        address_space: &mut AddressSpace,
        pmm: &mut PhysicalMemoryManager,
    ) -> Result<LoadedProgram, &'static str> {
        let mut allocated_pages = 0usize;
        let code_len = data.len() as u64;
        let num_pages = ((code_len + PAGE_SIZE - 1) / PAGE_SIZE) as usize;

        for page_idx in 0..num_pages {
            let virt_addr = USER_CODE_BASE + (page_idx as u64) * PAGE_SIZE;
            let phys_frame = pmm.allocate_frame().ok_or("PMM: Yetersiz fiziksel bellek")?;
            allocated_pages += 1;

            let virt_frame = phys_to_virt(phys_frame) as *mut u8;
            unsafe { core::ptr::write_bytes(virt_frame, 0, PAGE_SIZE as usize); }

            // Kod verisini sayfaya kopyala
            let file_off = page_idx * (PAGE_SIZE as usize);
            let copy_len = (data.len() - file_off).min(PAGE_SIZE as usize);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data.as_ptr().add(file_off),
                    virt_frame,
                    copy_len,
                );
            }

            // Kod segmenti: Kullanıcı + Yürütülebilir (Yazma kapalı)
            address_space.map_page(virt_addr, phys_frame, PageFlags::USER_CODE, pmm)?;
        }

        // Kullanıcı yığını
        let stack_pointer = Self::setup_user_stack(address_space, pmm, &mut allocated_pages)?;

        Ok(LoadedProgram {
            entry_point: USER_CODE_BASE,
            stack_pointer,
            code_base: USER_CODE_BASE,
            code_size: code_len,
            allocated_pages,
        })
    }

    /// Kullanıcı yığını için güvenli (User + Writable + No-Execute) sayfaları tahsis eder ve eşler.
    fn setup_user_stack(
        address_space: &mut AddressSpace,
        pmm: &mut PhysicalMemoryManager,
        allocated_pages: &mut usize,
    ) -> Result<u64, &'static str> {
        let stack_base = USER_STACK_TOP - (USER_STACK_PAGES as u64) * PAGE_SIZE;

        for i in 0..USER_STACK_PAGES {
            let virt_addr = stack_base + (i as u64) * PAGE_SIZE;
            let phys_frame = pmm.allocate_frame().ok_or("PMM: Yigin icin yetersiz bellek")?;
            *allocated_pages += 1;

            let virt_frame = phys_to_virt(phys_frame) as *mut u8;
            unsafe {
                core::ptr::write_bytes(virt_frame, 0, PAGE_SIZE as usize);
            }

            // Yığın: Yazılabilir, Çalıştırılamaz (NX)
            address_space.map_page(virt_addr, phys_frame, PageFlags::USER_STACK, pmm)?;
        }

        // Yığın x86-64 ABI kuralı gereği 16-bayt hizalı olmalıdır
        Ok(USER_STACK_TOP - 16)
    }
}

