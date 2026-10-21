#![no_std]

#![no_main]

#![feature(alloc_error_handler)]



extern crate alloc;

use alloc::vec::Vec;



use linked_list_allocator::LockedHeap;



#[allow(dead_code)]

const HEAP_SIZE: usize = 16 * 1024 * 1024;

#[allow(dead_code)]

static mut HEAP_MEMORY: [u8; HEAP_SIZE] = [0; HEAP_SIZE];



#[global_allocator]

static ALLOCATOR: LockedHeap = LockedHeap::empty();



#[alloc_error_handler]

fn alloc_error_handler(layout: core::alloc::Layout) -> ! {

    serial_print("[PANIC] Kernel heap allocation failed: size=");

    let mut v = layout.size() as u64;

    let mut buf = [0u8; 20];

    let mut i = buf.len();

    if v == 0 {

        serial_print("0");

    } else {

        while v > 0 && i > 0 {

            i -= 1;

            buf[i] = b'0' + (v % 10) as u8;

            v /= 10;

        }

        if let Ok(s) = core::str::from_utf8(&buf[i..]) {

            serial_print(s);

        }

    }

    serial_print(" align=");

    let mut a = layout.align() as u64;

    let mut i2 = buf.len();

    while a > 0 && i2 > 0 {

        i2 -= 1;

        buf[i2] = b'0' + (a % 10) as u8;

        a /= 10;

    }

    if let Ok(s) = core::str::from_utf8(&buf[i2..]) {

        serial_print(s);

    }

    serial_print("\n");

    panic!("Allocation error");

}



#[allow(static_mut_refs)]

fn init_heap() {

    unsafe { ALLOCATOR.lock().init(HEAP_MEMORY.as_mut_ptr(), HEAP_SIZE); }

}



fn persist_boot_log(vfs: &mut fs::vfs::VirtualFileSystem, log: &mut Vec<u8>, line: &str) {

    log.extend_from_slice(line.as_bytes());

    log.push(b'\n');

    // FAT32/Ext2 mounted ise kok dizine kalici olarak yazar. ISO9660 ise

    // read-only oldugu icin VFS bellekte tutar ve yazma basarisiz olabilir.

    let _ = vfs.write_file("boot.log", log);

}

pub mod arch;

pub mod mm;

pub mod fs;

pub mod drivers;

pub mod gui;

pub mod apps;

pub mod serial;



// --- Asama 1: ayristirilan moduller -------------------------------------------------

// Tasima yok: bu modullerdeki ogeler crate kokunde yeniden disa aktarilir, boylece

// `crate::serial_print`, `crate::monotonic_ms`, `crate::valid_limine_response` gibi

// cagrilar degismeden calismaya devam eder.

pub mod boot;

pub mod desktop;

pub mod panic_handler;



// These modules were split out of this file, but the rest of the kernel still

// uses their names from the crate root.  A private `use` is sufficient for

// intra-crate lookup; `pub use` silently drops private items and caused the

// compiler to report hundreds of cascading "not found" errors.

use boot::console::*;

use boot::cpu::*;

use boot::devices::*;

use boot::limine::*;

use boot::progress::*;

use boot::time::*;

use desktop::helpers::*;



static mut GLOBAL_PMM_PTR: *mut mm::pmm::PhysicalMemoryManager = core::ptr::null_mut();



fn alloc_pmm_frame_global() -> Option<u64> {

    unsafe {

        if !GLOBAL_PMM_PTR.is_null() {

            (*GLOBAL_PMM_PTR).allocate_frame()

        } else {

            None

        }

    }

}



/// Basit metin çizim fonksiyonu (font sistemi olmadan)

#[no_mangle]

pub extern "C" fn _start() -> ! {

    arch::x86_64::enable_sse();

    arch::x86_64::disable_pic();

    // Seri konsolu ilk iş olarak hazırla: sonraki tüm loglar (PMM/VMM/SMP…)

    // 115200 8N1 ile çerçeveye oturur, VirtualBox yakalaması bozulmaz.

    serial::init();

    // Initialize heap for dynamic allocations

    init_heap();

    // Install early display output before PMM/VMM/SMP and device probes.

    let (fb_ptr, width, height, pitch, bpp) = unsafe {

        let resp = valid_limine_response(core::ptr::addr_of!(FRAMEBUFFER_REQUEST.response))

            .filter(|resp| !resp.is_null())

            .unwrap_or_else(|| boot::text_console::fatal("Limine framebuffer yaniti yok"));

        let count = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffer_count));

        let entries = core::ptr::read_volatile(core::ptr::addr_of!((*resp).framebuffers));

        if count == 0 || !is_mapped_limine_ptr(entries) {

            boot::text_console::fatal("Limine framebuffer listesi gecersiz; GUI acilmadi");

        }

        let entry = core::ptr::read_volatile(entries);

        if !is_mapped_limine_ptr(entry) {

            boot::text_console::fatal("Limine framebuffer girdisi gecersiz; GUI acilmadi");

        }

        let info = core::ptr::read_volatile(entry);

        if info.address.is_null() || !is_canonical_ptr(info.address as *const u8) {

            boot::text_console::fatal("Limine framebuffer adresi gecersiz; GUI acilmadi");

        }

        (info.address as *mut u32, info.width as usize, info.height as usize,

         info.pitch as usize, info.bpp as usize)

    };

    if bpp != 32 || width < 320 || height < 200 || pitch < width.saturating_mul(4)

        || pitch % 4 != 0 || width.checked_mul(height).map_or(true, |n| n > drivers::video::BACKBUFFER_LEN)

    {

        boot::text_console::fatal("Framebuffer dogrulamasi basarisiz; GUI acilmadi");

    }

    let mut fb = unsafe { drivers::video::Framebuffer::new(fb_ptr, width, height, pitch, bpp) };

    if !fb.verify_bounds() {

        boot::text_console::fatal("Framebuffer cizim sinirlari dogrulanamadi; GUI acilmadi");

    }

    unsafe {

        PANIC_FB_ADDR = fb_ptr;

        PANIC_FB_W = width;

        PANIC_FB_H = height;

        PANIC_FB_PITCH = pitch;

        PANIC_FB_BPP = bpp;

        boot::text_console::init(drivers::video::Framebuffer::new(fb_ptr, width, height, pitch, bpp));

    }

    serial_print("[BOOT] UEFI metin konsolu aktif; donanim ve bellek hazirlaniyor.\n");

    serial_print("[BOOT] PIT/TSC zaman tabani baslatiliyor...\n");

    if init_timebase() {

        serial_print("[BOOT] PIT/TSC zaman tabani hazir.\n");

    } else {

        serial_print("[BOOT] UYARI: PIT yanit vermedi; CPUID TSC tahmini kullaniliyor.\n");

    }

    serial_print("[VIDEO] "); serial_print_dec(width as u64); serial_print("x");

    serial_print_dec(height as u64); serial_print(" @ "); serial_print_dec(bpp as u64); serial_print(" bpp.\n");



    #[cfg(minos_boot_log)]

    serial_print("[minOs] Kernel _start reached!\n");

    #[cfg(minos_boot_log)]

    serial_print("[minOs] SSE enabled & PIC IRQs masked!\n");



    // HHDM ofsetini al (fiziksel MMIO ve sayfa tablosu sanal erisimi icin).

    let hhdm_offset = unsafe {

        // SAFETY: the response pointer is validated before being dereferenced.

        if let Some(resp) = valid_limine_response(core::ptr::addr_of!(HHDM_REQUEST.response))

            .filter(|resp| !resp.is_null())

        {

            let off = core::ptr::read_volatile(core::ptr::addr_of!((*resp).offset));

            #[cfg(minos_boot_log)]

            serial_print("[minOs] HHDM offset okundu (0 yoksa gecerli).\n");

            off

        } else {

            #[cfg(minos_boot_log)]

            serial_print("[minOs] UYARI: HHDM response yok; MMIO erisimi calismaz!\n");

            0xFFFF_8000_0000_0000

        }

    };

    drivers::set_hhdm_offset(hhdm_offset);



    // 2. Sanal Dosya Sistemi (VFS / TarFS) Başlatımı

    serial_print("[BOOT] Sanal dosya sistemi başlatılıyor...\n");

    let mut fs_mgr = fs::FileSystemManager::new();

    let mut boot_log = Vec::with_capacity(4096);

    persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[BOOT] kernel filesystem initialized");



    // 1. Donanım Kesmeleri (IDT) ve PMM Başlatımı

    serial_print("[BOOT] Donanım kesmeleri ve bellek yöneticisi başlatiliyor...\n");

    arch::x86_64::init_interrupts();

    let mut memory_mgr = mm::MemoryManager::new();

    unsafe {

        // SAFETY: `response` is validated (non-null, aligned, canonical) by

        // `valid_limine_response` before the response object is dereferenced.

        if let Some(response) = valid_limine_response(core::ptr::addr_of!(MEMORY_MAP_REQUEST.response))

            .filter(|response| !response.is_null())

        {

            let count = core::ptr::read_volatile(core::ptr::addr_of!((*response).entry_count));

            let entries = core::ptr::read_volatile(core::ptr::addr_of!((*response).entries));

            let count = count.min(256) as usize;

            let array_bytes = count.checked_mul(core::mem::size_of::<*const LimineMemoryMapEntry>());

            if count > 0 && is_mapped_limine_ptr(entries) && array_bytes.is_some() {

                let array_end = (entries as usize).checked_add(array_bytes.unwrap());

                if array_end.map_or(false, |end| is_mapped_limine_ptr(end as *const LimineMemoryMapEntry)) {

                for i in 0..count {

                    let slot_addr = match (entries as usize).checked_add(i * core::mem::size_of::<*const LimineMemoryMapEntry>()) {

                        Some(addr) => addr,

                        None => break,

                    };

                    let slot = slot_addr as *const *const LimineMemoryMapEntry;

                    if !is_mapped_limine_ptr(slot) { break; }

                    let entry_ptr = core::ptr::read_volatile(slot);

                    if !is_mapped_limine_ptr(entry_ptr) { continue; }

                    // SAFETY: entry pointer is supplied by Limine and checked

                    // for null, alignment and canonical address before reads.

                    let base = core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).base));

                    let length = core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).length));

                    let entry_type = core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).entry_type));

                    if length == 0 || base.checked_add(length).is_none() { continue; }

                    let kind = match entry_type {

                        0 => mm::pmm::MemoryEntryType::Usable,

                        1 => mm::pmm::MemoryEntryType::Reserved,

                        2 => mm::pmm::MemoryEntryType::AcpiReclaimable,

                        3 => mm::pmm::MemoryEntryType::AcpiNvs,

                        4 => mm::pmm::MemoryEntryType::BadMemory,

                        5 => mm::pmm::MemoryEntryType::BootloaderReclaimable,

                        6 => mm::pmm::MemoryEntryType::KernelAndModules,

                        7 => mm::pmm::MemoryEntryType::Framebuffer,

                        _ => continue,

                    };

                    memory_mgr.pmm.add_region(base, length, kind);

                }

                }

            }

        }

    }

    serial_print("[PMM] Fiziksel bellek yoneticisi baslatildi.\n");

    unsafe {

        GLOBAL_PMM_PTR = &mut memory_mgr.pmm as *mut _;

    }

    arch::x86_64::set_mmio_frame_allocator(alloc_pmm_frame_global);

    // VRAM MMIO aralığını (16MB) sayfa tablolarına eşle; çözünürlük veya çizim asla unmapped #PF üretmesin
    let fb_phys = (fb_ptr as u64).wrapping_sub(drivers::phys_to_virt(0));
    for off in (0..16 * 1024 * 1024).step_by(2 * 1024 * 1024) {
        arch::x86_64::mmio_map_2mb((fb_phys + off as u64) & !0x1F_FFFF);
    }



    // PMM Kendi Kendini Test Etme (Tahsisat ve Geri Birakma Dogrulamasi)

    serial_print("[BOOT] Bellek yöneticisi test ediliyor...\n");

    if let (Some(f1), Some(f2)) = (memory_mgr.pmm.allocate_frame(), memory_mgr.pmm.allocate_frame()) {

        if f1 != f2 && f1 % mm::pmm::PAGE_SIZE == 0 && f2 % mm::pmm::PAGE_SIZE == 0 {

            serial_print("[PMM] Cerceve tahsis testi basarili (cakismayan cerceveler dogrulandi).\n");

        }

        memory_mgr.pmm.free_frame(f2);

        memory_mgr.pmm.free_frame(f1);

    }



    // VMM ve Guvenli Uygulama Yukleyicisi Dogrulamasi (Sanal Bellek Eşleme ve Yükleme Testi)

    serial_print("[BOOT] Sanal bellek yöneticisi test ediliyor...\n");

    let mut address_space = memory_mgr.kernel_address_space();

    let test_vaddr = mm::loader::USER_CODE_BASE;

    if let Some(frame) = memory_mgr.pmm.allocate_frame() {

        if address_space.map_page(test_vaddr, frame, mm::vmm::PageFlags::USER_CODE, &mut memory_mgr.pmm).is_ok() {

            if let Some(translated_phys) = address_space.translate(test_vaddr) {

                if translated_phys == frame {

                    serial_print("[VMM] 4-seviyeli sanal bellek esleme ve adres cozümleme aktif.\n");

                }

            }

            let _ = address_space.unmap_page(test_vaddr);

        }

        memory_mgr.pmm.free_frame(frame);

    }



    // Test amaçlı standart ELF64 ikili dosyasını yükle ve doğrula

    let sample_elf_bin = mm::loader::AppLoader::create_sample_elf();

    if let Ok(loaded) = mm::loader::AppLoader::load_binary(&sample_elf_bin, &mut address_space, &mut memory_mgr.pmm) {

        if loaded.entry_point >= mm::loader::USER_CODE_BASE && loaded.stack_pointer > 0 {

            serial_print("[LOADER] Standart ELF64 ikilisi guvenli bellek izolasyonu ile yuklendi (W^X / NX aktif).\n");

        }

        loaded.unmap_and_free(&mut address_space, &mut memory_mgr.pmm);

    }



    // Surec Yoneticisi ve Sistem Cagrilarinin (Syscalls) Baslatimi

    serial_print("[BOOT] Süreç yöneticisi başlatılıyor...\n");

    memory_mgr.proc_mgr.init_kernel_process();

    // Sistem cagrilarini test et (sys_getpid & sys_alloc_frame)

    let test_pid = mm::process::syscall_dispatch(mm::process::sys_nos::SYS_GETPID, 0, 0, 0, &mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);

    let test_frame = mm::process::syscall_dispatch(mm::process::sys_nos::SYS_ALLOC_FRAME, 0, 0, 0, &mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);

    if test_frame != 0 {

        let _ = mm::process::syscall_dispatch(mm::process::sys_nos::SYS_FREE_FRAME, test_frame, 0, 0, &mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);

    }

    if test_pid == 0 {

        serial_print("[PROC] Surec tablosu ve sistem cagrilar (syscalls) aktif.\n");

    }



    // Bellek tahsisi, sayfalama ve kullanıcı kipi izolasyon doğrulama testi

    serial_print("[BOOT] Bellek testleri çalıştırılıyor...\n");

    mm::vmm::test_memory_and_isolation(&mut memory_mgr.pmm);



    // Süreç yaşam döngüsü, kaza, geçersiz syscall ve kaynak temizleme testi

    serial_print("[BOOT] Süreç testleri çalıştırılıyor...\n");

    mm::process::test_process_lifecycle_and_fault_recovery(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);



    // Hardware-safe mode: do not touch APIC/SMP until the laptop path is stable.

    serial_print("[BOOT] Tek CPU modu secildi; APIC/SMP baslatmasi atlandi.\n");

    serial_print("[SMP] BSP GDT yukleniyor...\n");

    unsafe { arch::x86_64::gdt::load_for_cpu(0) };

    serial_print("[SMP] 1 CPU modunda devam ediliyor.\n");



    // 1c. Donanım Kesmeleri ve APIC Zamanlayıcı (APIC Timer & Hardware Interrupts)

    serial_print("[BOOT] APIC zamanlayıcı başlatılıyor...\n");

    serial_print("[APIC] APIC timer atlandi; TSC monotonic saat kullaniliyor.\n");

    // Paralel sağlama toplamı: AP'ler + BSP aynı tamponu parçalı işler.

    {

        const SUM_FRAMES: usize = 64;

        const U64_PER_FRAME: usize = 512;

        if let Some(buf_phys) = memory_mgr.pmm.allocate_contiguous_frames(SUM_FRAMES) {

            unsafe {

                let base = crate::drivers::phys_to_virt(buf_phys) as *mut u64;

                let mut expected: u64 = 0;

                for i in 0..SUM_FRAMES * U64_PER_FRAME {

                    let v = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x1234_5678);

                    base.add(i).write_volatile(v);

                    expected = expected.wrapping_add(v);

                }

                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

                match arch::x86_64::smp::parallel_checksum(buf_phys, SUM_FRAMES * U64_PER_FRAME, 4 * U64_PER_FRAME) {

                    Some(got) if got == expected => {

                        serial_print("[SMP] Paralel saglama toplami (AP+BSP) eslesti: BASARILI\n");

                    }

                    Some(_) => {

                        serial_print("[SMP] Paralel saglama toplami UYUSMADI: BASARISIZ\n");

                    }

                    None => {

                        serial_print("[SMP] Paralel saglama toplami zaman asimi (tek cekirdek yedegi yok).\n");

                    }

                }

            }

            for i in 0..SUM_FRAMES {

                memory_mgr.pmm.free_frame(buf_phys + (i as u64) * 4096);

            }

        }

    }



    // 3. Donanim suruculeri: Limine boot menusu bir profil secer.

    serial_print("[BOOT] Donanım sürücüleri başlatılıyor...\n");

    let cmdline = limine_boot_cmdline().unwrap_or("");

    let driver_profile = if cmdline_has_token(cmdline, "full") { "full" }

        else if cmdline_has_token(cmdline, "hardware") { "hardware" }

        else if cmdline_has_token(cmdline, "audio") { "audio" }

        else if cmdline_has_token(cmdline, "safe") { "safe" } else { "full" };

    serial_print("[DRIVERS] Profil: ");

    serial_print(driver_profile);

    serial_print("\n");

    let profile_line = alloc::format!("[DRIVERS] profile: {}", driver_profile);

    persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, &profile_line);



    if driver_profile == "audio" || driver_profile == "full" {

        serial_print("[DRIVER] HDA audio sürücüsü başlatılıyor...\n");

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] HDA starting");

        let hda_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(6);

        let Some(hda_dma_phys) = hda_dma_phys else {

            boot::text_console::fatal("HDA DMA bellek tahsisati basarisiz");

        };

        drivers::hda::init(Some(hda_dma_phys));

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] HDA finished");

        serial_print("[DRIVER] HDA audio sürücüsü hazır.\n");

    }



    if driver_profile == "hardware" || driver_profile == "full" {

        serial_print("[DRIVER] e1000 ağ sürücüsü başlatılıyor...\n");

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] e1000 starting");

        let e1000_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(34);

        let Some(e1000_dma_phys) = e1000_dma_phys else {

            boot::text_console::fatal("e1000 DMA bellek tahsisati basarisiz");

        };

        drivers::e1000::init(Some(e1000_dma_phys));

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] e1000 finished");

        serial_print("[DRIVER] e1000 ağ sürücüsü hazır.\n");



        serial_print("[DRIVER] Ağ protokolü başlatılıyor...\n");

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] network starting");

        drivers::netstack::init();

        let _ = drivers::netstack::dhcp_request();

        drivers::netstack::run_network_self_test();

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] network finished");

        serial_print("[DRIVER] Ağ protokolü hazır.\n");



        serial_print("[DRIVER] USB sürücüsü başlatılıyor...\n");

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] USB starting");

        drivers::usb::init();

        drivers::usb::test_usb_subsystems();

        serial_print("[DRIVER] USB HID (xHCI) sürücüsü başlatılıyor...\n");

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] USB HID starting");

        drivers::usb_hid::init(&mut memory_mgr.pmm);

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] USB HID finished");

        serial_print("[DRIVER] USB HID sürücüsü hazır.\n");

        persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[DRIVER] USB finished");

        serial_print("[DRIVER] USB sürücüsü hazır.\n");

    }



    persist_boot_log(&mut fs_mgr.vfs, &mut boot_log, "[BOOT] driver profile finished");

    let _net_dev = drivers::net::NetworkInterface::new();

    let mut audio_dev = drivers::audio::AudioDevice::new();

    serial_print("[DRIVERS] Tüm sürücüler hazır.\n");



    // 4b. Gelişmiş grafik sürücüleri: BGA/VMware/VGA yetenek taraması.

    let gpu_caps = drivers::gpu::init();

    // GPU initialization status is mirrored to the early text console.



    if gpu_caps.bga_present {

        serial_print("[GPU] BGA algilandi, cozunurluk degisimi acik.\n");

        // BGA grafik modda HW sprite yoktur; yazılım imleci çizilmeye devam

        // edilir (görünürlük için şart). CRTC'ye konum bildirimi sürer.

        fb.set_hw_cursor(false);

        serial_print("[GPU] Hizlandirilmis yazilim imleci etkin.\n");

    } else {

        serial_print("[GPU] BGA yok; cozunurluk Limine ile sabit, yazilim imleci.\n");

    }

    // 5. PS/2 Donanım Sürücüleri

    let mouse = unsafe { &mut *core::ptr::addr_of_mut!(GLOBAL_MOUSE) };

    let keyboard = unsafe { &mut *core::ptr::addr_of_mut!(GLOBAL_KEYBOARD) };

    drivers::ps2::init();

    serial_print("[BOOT] PS/2 hazir.\n");

    // Input initialization continues without an artificial delay.



    // 5b. Tema motorunu başlat

    gui::theme::init();

    serial_print("[BOOT] Tema hazir.\n");



    // 5c. Önyükleme modu denetimi: Boot menüsünden "installed" seçildiyse ve

    // diskte kurulu bir minOs (MINOS.SYS) yoksa canlı sisteme düşmeden hata

    // verilir ve sistem bu ekranda bekler.

    // (Disk taraması tek yapılır; sonucu loga yazılır.)

    let wants_installed = boot_wants_installed();

    let installed = fs::diskfs::is_installed();

    serial_print("[BOOT] Kurulum denetimi: wants_installed=");

    serial_print(if wants_installed { "1" } else { "0" });

    serial_print(" installed=");

    serial_print(if installed { "1" } else { "0" });

    serial_print("\n");



    if wants_installed && !installed {

        serial_print("[BOOT] 'installed' modu istendi ancak diskte kurulum bulunamadi.\n");

        boot::text_console::fatal("Kurulu sistem secildi ancak MINOS.SYS bulunamadi");

    }



    serial_print("[BOOT] Tum kritik alt sistemler baslatildi. GUI ve uygulamalar yukleniyor...\n");



    // 5d. Eğer diskte varsa (MINOS.SYS + fat32) — kurulu sistem —

    // welcome (hoş geldin) ekranını atlayıp doğrudan masaüstüye geç.

    // Live ortam ise welcome ekranı gösterilir.

    // 6. MinGUI Pencere Yöneticisi ve Tüm Uygulamalar

    serial_print("[BOOT] Pencere yoneticisi ve uygulamalar hazirlaniyor...\n");

    let mut compositor = gui::MinGuiCompositor::new();

    let mut filemanager_app = apps::filemanager::FileManagerApp::new();

    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

    let mut desktop_files = apps::filemanager::FileManagerApp::new();

    desktop_files.set_path("/desktop");

    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

    let mut desktop_drag_source = false;

    let mut file_clipboard = apps::filemanager::FileClipboard::new();

    let mut settings_app = apps::settings::SystemSettings::default();

    settings_app.display_res = (width, height);

    // Disk üzerinde önceden kaydedilmiş kalıcı ayarlar var mı kontrol et

    if fs::diskfs::load_file("settings.cfg") > 0 {

        let cfg_bytes = fs::diskfs::content();

        let _ = fs_mgr.vfs.write_file("settings.cfg", cfg_bytes);

    }

    settings_app.load_from_vfs(&fs_mgr.vfs);

    crate::gui::wallpaper::apply(settings_app.wallpaper, settings_app.wall_color);

    mouse.speed = settings_app.mouse_speed;

    mouse.reverse_scroll = settings_app.reverse_scroll;

    keyboard.layout = settings_app.keyboard_layout;

    gui::theme::set_theme(settings_app.theme);

    let mut notepad_app = apps::notepad::NotepadApp::new();

    let mut browser_app = apps::browser::BrowserApp::new();

    let mut imageviewer_app = apps::imageviewer::ImageViewerApp::new();

    let mut musicplayer_app = apps::musicplayer::MusicPlayerApp::new();

    let mut videoplayer_app = apps::videoplayer::VideoPlayerApp::new();

    let mut taskmgr_app = apps::taskmgr::TaskManagerApp::new();

    let mut calculator_app = apps::calculator::CalculatorApp::new();

    // Başlangıçta yalnızca sabit uygulama pencereleri için state oluştur.

    // MAX_WINDOWS kadar 4096 noktalı tuval ayırmak 1 MiB kernel heap'ini aşar.

    let mut paint_apps: alloc::vec::Vec<Option<apps::paint::PaintApp>> = (0..gui::MAX_WINDOWS).map(|_| None).collect();

    let mut terminal_app = apps::terminal::TerminalApp::new();

    terminal_app.init_banner();

    let mut installer_app = apps::installer::InstallerApp::new();

    installer_app.refresh_drives();

    let mut viewer3d_app = apps::viewer3d::Viewer3DApp::new();

    let mut pkgmgr_app = apps::pkgmgr::PackageManagerApp::new();

    pkgmgr_app.refresh_installed(&fs_mgr.vfs);



    // 12 Uygulama Penceresini Kaydet (indeks = sabit uygulama kimliği).

    // Açılışta TÜM pencereler kapalıdır; kullanıcı temiz bir masaüstüyle başlar

    // ve uygulamaları Başlat Menüsü, masaüstü simgeleri veya sağ tık ile açar.

    // 0: Dosya Yöneticisi

    let files_width = 720.min(width.saturating_sub(20)).max(200);

    let files_height = 500.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "Dosya Yöneticisi", "Dosyalar",

        width.saturating_sub(files_width) / 2,

        height.saturating_sub(files_height + gui::taskbar_h()) / 2,

        files_width, files_height, false,

    ));

    // 1: Sistem Ayarları

    let settings_width = 700.min(width.saturating_sub(20)).max(200);

    let settings_height = 540.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "Sistem Ayarları", "Ayarlar",

        width.saturating_sub(settings_width) / 2,

        height.saturating_sub(settings_height + gui::taskbar_h()) / 2,

        settings_width, settings_height, false,

    ));

    // 2: Not Defteri

    compositor.add_window(gui::Window::new("Not Defteri", "Notlar", 80, 70, 520, 330, false));

    // 3: Web Tarayıcısı

    let browser_w = 760.min(width.saturating_sub(20)).max(300);

    let browser_h = 490.min(height.saturating_sub(gui::taskbar_h() + 10)).max(200);

    compositor.add_window(gui::Window::new(

        "Web Gezgini", "Web",

        width.saturating_sub(browser_w) / 2,

        height.saturating_sub(browser_h + gui::taskbar_h()) / 2,

        browser_w, browser_h, false,

    ));

    // 4: Resim Gösterici

    compositor.add_window(gui::Window::new("Resim Gösterici", "Resim", 120, 90, 500, 330, false));

    // 5: Müzik Çalar

    compositor.add_window(gui::Window::new("Müzik Çalar", "Müzik", 560, 80, 460, 245, false));

    // 6: Video Oynatıcı

    compositor.add_window(gui::Window::new("Video Oynatıcı", "Video", 150, 110, 540, 330, false));

    // 7: Görev Yöneticisi

    compositor.add_window(gui::Window::new("Görev Yöneticisi", "Görevler", 220, 60, 480, 460, false));

    compositor.add_window(gui::Window::new("Hesap Makinesi", "Hesap", 260, 120, 360, 340, false));

    compositor.add_window(gui::Window::new("Çizim Tahtası", "Çizim", 180, 80, 520, 360, false));

    compositor.add_window(gui::Window::new("Komut İstemi", "Komut", 220, 100, 560, 300, false));

    // 11: minOs Tek Tıkla Kur Sihirbazı

    let inst_w = 540.min(width.saturating_sub(20)).max(200);

    let inst_h = 460.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "minOs Tek Tıkla Kur", "Kurulum",

        width.saturating_sub(inst_w) / 2,

        height.saturating_sub(inst_h + gui::taskbar_h()) / 2,

        inst_w, inst_h, false,

    ));

    // 12: 3D Gösterici

    let v3d_w = 600.min(width.saturating_sub(20)).max(200);

    let v3d_h = 440.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "3D Gösterici", "3D",

        width.saturating_sub(v3d_w) / 2,

        height.saturating_sub(v3d_h + gui::taskbar_h()) / 2,

        v3d_w, v3d_h, false,

    ));

    // 13: Paket Yöneticisi

    let pkg_w = 620.min(width.saturating_sub(20)).max(200);

    let pkg_h = 440.min(height.saturating_sub(gui::taskbar_h() + 10)).max(120);

    compositor.add_window(gui::Window::new(

        "Paket Yöneticisi", "Paketler",

        width.saturating_sub(pkg_w) / 2,

        height.saturating_sub(pkg_h + gui::taskbar_h()) / 2,

        pkg_w, pkg_h, false,

    ));



    // Açılışta hiçbir pencere odaklı değildir (temiz masaüstü).

    compositor.active_window_idx = None;



    // Kurulu sistemse welcome ekranını atla, live ortam ise göster.

    serial_print("[BOOT] Hosgeldin ekrani hazirlaniyor; grafik oz-denetimleri calistiriliyor.\n");

    // Widget/Component ağacı mimarisini (yerleşim, Tab odağı, olaylar) doğrula.

    if !gui::widget::self_test(&mut fb) {

        boot::text_console::fatal("Grafik arayuz oz-denetimi basarisiz; GUI acilmadi");

    }

    serial_print("[BOOT] Grafik arayuz denetimleri tamamlandi.\n");

    serial_print("[BOOT] Kritik denetimler tamamlandi; grafik arayuz baslatiliyor.\n");

    boot::text_console::disable();

    if installed {

        serial_print("[BOOT] Kurulu sistem tespit edildi; masaustu hazirlaniyor...\n");

        for phase in 0..4 {

            gui::render_loading_transition(&mut fb, width, height, phase,

                gui::ui_text("Masaustu hazirlaniyor", "Preparing desktop"));

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 25 { core::hint::spin_loop(); }

        }

    } else {

        for phase in 0..8 {

            gui::render_loading_transition(&mut fb, width, height, phase,

                gui::ui_text("Sistem yukleniyor", "Loading system"));

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 45 { core::hint::spin_loop(); }

        }

    }

    if !installed {

        // Her açılışta dil ve masaüstü teması seçilebilen hoş geldin ekranı.

        let mut welcome_was_left_down = false;

        let mut welcome_dirty = true;

        let mut welcome_mouse_pos = (mouse.x, mouse.y);

        let mut welcome_accepted = false;

        let mut welcome_announced = false;

        let mut welcome_diag_last = monotonic_ms();

        let mut _welcome_diag_iters: u64 = 0;

        let mut welcome_diag_last_trace: u64 = 0;

        let mut welcome_start_time = monotonic_ms();

        let welcome_timeout_ms: u64 = 5_000; // 5 saniye içinde etkileşim olmazsa masaüstüne otomatik geç

        loop {

            if !welcome_announced {

                serial_print("[BOOT] Hosgeldin dongusu: Enter/ESC/tik bekleniyor (5sn zaman asimi aktif).\n");

                welcome_announced = true;

            }

            if welcome_dirty && !boot::text_console::is_active() {

                gui::render_welcome_screen(&mut fb, width, height, settings_app.language.is_english(), settings_app.theme, &mouse);

                welcome_dirty = false;

            }

            let mut welcome_update = drivers::ps2::poll_input(&mut *mouse, &mut *keyboard, width as i32, height as i32);

            let welcome_usb_update = drivers::usb_hid::poll_input(

                &mut *mouse,

                &mut *keyboard,

                width as i32,

                height as i32,

                &mut memory_mgr.pmm,

            );

            welcome_update.keyboard |= welcome_usb_update.keyboard;

            welcome_update.mouse |= welcome_usb_update.mouse;

            if welcome_update.keyboard || welcome_update.mouse {

                // Herhangi bir kullanıcı hareketi algılanırsa zaman aşımını sıfırla/uzat

                welcome_start_time = monotonic_ms();

               /* serial_print("[BOOT] Welcome input algilandi: keyboard=");

                serial_print(if welcome_update.keyboard { "1" } else { "0" });

                serial_print(" mouse=");

                serial_print(if welcome_update.mouse { "1" } else { "0" });

                serial_print("\n");*/

            }

            let (_tr_st, _tr_d, tr_n) = drivers::ps2::trace_last();

            if tr_n != welcome_diag_last_trace {

                welcome_diag_last_trace = tr_n;

            }

            _welcome_diag_iters += 1;

            let welcome_now = monotonic_ms();

            if welcome_now.wrapping_sub(welcome_diag_last) >= 5000 {

                welcome_diag_last = welcome_now;

                serial_print("[BOOT] Welcome loop tick: ms=");

                serial_print_dec(welcome_now.wrapping_sub(welcome_start_time) as u64);

                serial_print(" iters=");

                serial_print_dec(_welcome_diag_iters as u64);

                serial_print("\n");

            }



            // 5 saniye zaman aşımı: USB klavye veya fare algılanamasa bile sistemi kilitleme, doğrudan masaüstüne geç

            if welcome_now.wrapping_sub(welcome_start_time) >= welcome_timeout_ms {

                serial_print("[BOOT] Hosgeldin zamani asildi; otomatik masaustune geciliyor.\n");

                welcome_accepted = true;

            }



            // Enter veya Escape ile hoş geldin ekranını geç.

            while let Some(action) = keyboard.take_last_action() {

                if matches!(action, drivers::keyboard::KeyAction::EnterKey) {

                    welcome_accepted = true;

                }

            }

            while let Some(ch) = keyboard.take_last_char() {

                if ch == 'e' || ch == 'E' {

                    // E tuşu ile boot mesajlarını göster/gizle

                    unsafe {

                        if !boot::text_console::is_active() {

                            boot::text_console::save_screen();

                            boot::text_console::toggle_active();

                            serial_print("[BOOT] Boot mesajları gösteriliyor (E tuşu ile geri dön)...\n");

                        } else {

                            boot::text_console::toggle_active();

                            boot::text_console::restore_screen();

                            serial_print("[BOOT] GUI'ye geri dönüldü.\n");

                            welcome_dirty = true;

                        }

                    }

                }

                if ch == '\x1b' || ch == ' ' {

                    welcome_accepted = true;

                }

            }

            if welcome_accepted {

                settings_app.save_all(&compositor, &mut fs_mgr.vfs);

                break;

            }



            if welcome_mouse_pos != (mouse.x, mouse.y) {

                welcome_mouse_pos = (mouse.x, mouse.y);

                welcome_dirty = true;

            }

            let clicked = mouse.left_button && !welcome_was_left_down;

            welcome_was_left_down = mouse.left_button;

            if !clicked { continue; }



            let mx = mouse.x.max(0) as usize;

            let my = mouse.y.max(0) as usize;

            let point_in = |rect: (usize, usize, usize, usize)| {

                mx >= rect.0 && my >= rect.1 && mx < rect.0 + rect.2 && my < rect.1 + rect.3

            };

            let mut selection_changed = false;

            for i in 0..2 {

                if point_in(gui::welcome_language_rect(width, height, i)) {

                    settings_app.language = if i == 0 { apps::settings::UiLanguage::Turkish } else { apps::settings::UiLanguage::English };

                    gui::set_ui_language_english(settings_app.language.is_english());

                    selection_changed = true;

                    break;

                }

            }

            if !selection_changed {

                for (i, id) in gui::theme::ThemeId::ALL.iter().enumerate() {

                    if point_in(gui::welcome_theme_rect(width, height, i)) {

                        settings_app.theme = *id;

                        gui::theme::set_theme(*id);

                        selection_changed = true;

                        break;

                    }

                }

            }

            if selection_changed {

                welcome_dirty = true;

            } else if point_in(gui::welcome_continue_rect(width, height)) {

                settings_app.save_all(&compositor, &mut fs_mgr.vfs);

                break;

            }



            if !welcome_update.mouse && !welcome_update.keyboard {

                for _ in 0..1_000 {

                    core::hint::spin_loop();

                }

            }

        }

        serial_print("[BOOT] Hosgeldin kabul edildi; masaustune geciliyor.\n");

    }



    // Welcome ekranından masaüstüne geçişi de aynı animasyonla yumuşat.

    // Sadece live ortamda göster; kurulu sistemde atla.

    if !installed {

        for phase in 0..6 {

            gui::render_loading_transition(&mut fb, width, height, phase + 2, gui::ui_text("Masaustu hazirlaniyor", "Preparing desktop"));

            let frame_start = monotonic_ms();

            while monotonic_ms().saturating_sub(frame_start) < 45 { core::hint::spin_loop(); }

        }

    }



    // RTC - gerçek zamanlı saat

    let mut now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());

    compositor.cal_year = now.year;

    compositor.cal_month = now.month;

    #[cfg(minos_boot_log)]

    serial_print("[minOs] RTC clock online.\n");

    // Save once the boot transcript is complete, including numeric fields.

    save_boot_log(&mut fs_mgr.vfs);



    // Açılış melodisi (Chime)

    audio_dev.set_volume(settings_app.volume);

    audio_dev.is_muted = settings_app.is_muted;

    audio_dev.play_chime();



    // Etkileşim durumu

    let mut last_mouse_x = mouse.x;

    let mut last_mouse_y = mouse.y;

    let mut last_buttons = (mouse.left_button, mouse.right_button);

    let mut dragging_win: Option<usize> = None;

    let mut resizing_win: Option<usize> = None;

    let mut resize_edge: Option<gui::ResizeEdge> = None;

    // Kenar yeniden boyutlandırma başlangıcında pencere geometrisi (sol/üst kenarlar için)

    let mut resize_orig: (usize, usize, usize, usize) = (0, 0, 0, 0);

    let mut drag_offset_x = 0;

    let mut drag_offset_y = 0;

    // Aero Snap önizleme dikdörtgeni: (x, y, w, h)

    let mut active_snap_preview: Option<(usize, usize, usize, usize)> = None;

    // Kaydırma çubuğu sürükleme: (idx, thumb_üst_ofset, thumb_h, track_h, content_h, view_h)

    let mut vscroll_drag: Option<(usize, usize, usize, usize, usize, usize)> = None;

    let mut anim_tick: usize = 0;

    let mut last_frame_ms = monotonic_ms();

    let mut loop_counter: u64 = 0;

    let mut last_clock_update_ms = monotonic_ms();

    let mut last_music_beat: usize = 0;

    let mut cpu_sample_start = read_cpu_cycles();

    let mut cpu_sample_total_cycles = 0u64;

    let mut cpu_sample_active_cycles = 0u64;

    let mut last_ap_heartbeats = [0u64; arch::x86_64::smp::MAX_CPUS];

    for c in 0..arch::x86_64::smp::MAX_CPUS {

        last_ap_heartbeats[c] = arch::x86_64::smp::heartbeat(c);

    }

    let mut music_note: usize = 0;

    let mut speaker_on = false;

    let mut last_title_idx: Option<usize> = None;

    let mut last_title_click_ms: u64 = 0;

    // Not Defteri metin seçimi sürükleme durumu.

    let mut notepad_dragging = false;

    let mut image_drag_last: Option<(usize, usize)> = None;

    let mut screenshot_counter: usize = 1;

    let mut run_dialog_open = false;

    let mut run_dialog_input = gui::TextBuf::new();



    // Dosya / Klasör Özellikler (Properties) Modalı

    let mut properties_open = false;

    let mut properties_name = gui::TextBuf::new();

    let mut properties_path = gui::TextBuf::new();

    let mut properties_type = gui::TextBuf::new();

    let mut properties_size = gui::TextBuf::new();

    let mut properties_location = gui::TextBuf::new();



    let mut app_render_cycles = [0u64; gui::MAX_WINDOWS];

    taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs, &compositor.windows, &app_render_cycles);

    let render_full_frame = |fb: &mut drivers::video::Framebuffer,

                             compositor: &gui::MinGuiCompositor,

                             fm: &apps::filemanager::FileManagerApp,

                             desktop_fm: &apps::filemanager::FileManagerApp,

                             st: &apps::settings::SystemSettings,

                             np: &apps::notepad::NotepadApp,

                             br: &apps::browser::BrowserApp,

                             iv: &apps::imageviewer::ImageViewerApp,

                             mp: &apps::musicplayer::MusicPlayerApp,

                             vp: &apps::videoplayer::VideoPlayerApp,

                             tm: &apps::taskmgr::TaskManagerApp,

                             calc: &apps::calculator::CalculatorApp,

                             paint: &[Option<apps::paint::PaintApp>],

                             term: &apps::terminal::TerminalApp,

                             inst: &apps::installer::InstallerApp,

                             v3d: &apps::viewer3d::Viewer3DApp,

                             pkg: &apps::pkgmgr::PackageManagerApp,

                             now: &drivers::rtc::DateTime,

                             mouse: &drivers::mouse::MouseState,

                             kbd: &drivers::keyboard::KeyboardState,

                             total_ram_bytes: u64,

                             tick: usize,

                             app_render_cycles: &mut [u64; gui::MAX_WINDOWS],

                             run_dialog_state: (bool, &gui::TextBuf),

                             properties_state: (bool, &gui::TextBuf, &gui::TextBuf, &gui::TextBuf, &gui::TextBuf, &gui::TextBuf),

                             snap_preview: Option<(usize, usize, usize, usize)>,

                             dirty_rect: Option<(usize, usize, usize, usize)>| {

        // Kirli dikdörtgen verildiyse yalnızca bu bölge çizilip blit edilir;

        // aksi halde tüm ekran yeniden çizilir.

        fb.set_frame_clip(dirty_rect);

        // 1. Masaüstü Arka Planı

        compositor.render_desktop_bg(fb);

        // /desktop içeriğini masaüstü simgeleri olarak çiz.

        for i in 0..desktop_fm.file_count {

            let item = &desktop_fm.files[i];

            let (x, y, w, h) = desktop_item_rect(i);

            if !desktop_item_visible((x, y, w, h), fb.width, fb.height) { continue; }

            if desktop_fm.is_row_selected(i) {

                fb.draw_rounded_rect_alpha(x, y, w, h, 5, gui::MinGuiCompositor::shade(gui::Color::ACCENT_BLUE, 28), 160);

                fb.draw_rounded_rect_outline(x, y, w, h, 5, gui::Color::ACCENT_BLUE);

            } else {

                fb.draw_rounded_rect_alpha(x, y, w, h, 5, gui::MinGuiCompositor::shade(gui::Color::WINDOW_BG, 8), 128);

                fb.draw_rounded_rect_outline(x, y, w, h, 5, gui::Color::BORDER);

            }

            if item.is_dir { apps::filemanager::FileManagerApp::draw_folder_icon(fb, x + 5, y + 5, false); }

            else { apps::filemanager::FileManagerApp::draw_file_icon(fb, x + 5, y + 5, item.icon_tag); }

            let item_fg = if desktop_fm.is_row_selected(i) {

                gui::MinGuiCompositor::text_on(gui::MinGuiCompositor::shade(gui::Color::ACCENT_BLUE, 28))

            } else {

                gui::Color::TEXT_MAIN

            };

            gui::MinGuiCompositor::draw_string_clipped(fb, x + 30, y + 10, item.display_name(), item_fg, (x + 30, y + 2, w - 34, h - 4));

        }



        // 2. Açık pencereleri z-sırasına göre (arkadan öne) çiz

        let (order, count) = compositor.draw_order();

        for k in 0..count {

            let idx = order[k];

            if let Some(win) = compositor.windows[idx] {

                if !win.is_open || win.is_minimized {

                    continue;

                }

                // Off-screen pencereleri tamamen atla (çizim maliyeti yok).

                if win.x >= fb.width || win.y >= fb.height

                    || win.x.saturating_add(win.width) == win.x

                    || win.y.saturating_add(win.height) == win.y {

                    continue;

                }

                // Kirli bölgeyle kesişmeyen pencereyi hiç çizme (gölge payı dahil).

                if let Some((dx, dy, dw, dh)) = dirty_rect {

                    let wx0 = win.x.saturating_sub(8);

                    let wy0 = win.y.saturating_sub(8);

                    let wx1 = win.x.saturating_add(win.width).saturating_add(8);

                    let wy1 = win.y.saturating_add(win.height).saturating_add(8);

                    let overlaps = dx < wx1 && wx0 < dx.saturating_add(dw)

                        && dy < wy1 && wy0 < dy.saturating_add(dh);

                    if !overlaps {

                        continue;

                    }

                }

                compositor.render_window_frame(fb, &win, mouse, idx);



                // İçerik alanını pencere çerçevesine kırp (taşma önleme).

                let (cx, cy, cw, ch) = win.content_rect();

                fb.set_clip(cx, cy, cw, ch);



                // Uygulamaya göre ölçülen içerik boyutu (kaydırma çubuğu gerekli mi?).

                let app_id = compositor.app_id_for_window(idx);

                let content_h = window_content_h(app_id, win.height, win.width, fm, np, br);

                let content_w = cw;

                let want_v = content_h > ch;

                let want_h = content_w > cw;

                let v_gutter = if want_v { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };

                let h_gutter = if want_h { crate::gui::scrollbar::SCROLLBAR_THICKNESS } else { 0 };

                // İçerik alanı (kaydırma çubukları kadar daraltılmış).

                let content_area = (cx, cy, cw.saturating_sub(v_gutter), ch.saturating_sub(h_gutter));

                fb.set_clip(content_area.0, content_area.1, content_area.2.max(1), content_area.3.max(1));



                let render_start = read_cpu_cycles();

                match app_id {

                    0 => fm.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll, kbd.ctrl),

                    1 => st.render_content(fb, win.x, win.y, win.width, win.height, kbd.layout, now, total_ram_bytes,

                        cpu_vendor(), cpu_logical_count(),

                        if drivers::hda::is_ready() { if drivers::hda::is_dma_progressing() { gui::ui_text("Intel HDA (DMA aktif)", "Intel HDA (DMA active)") } else { gui::ui_text("Intel HDA (DMA beklemede)", "Intel HDA (DMA standby)") } } else { gui::ui_text("PC Speaker (PWM fallback)", "PC Speaker (PWM fallback)") },

                        if drivers::e1000::is_ready() { "Intel e1000" } else { gui::ui_text("Bulunamadı", "Not found") },

                        if drivers::e1000::is_ready() { gui::ui_text("Hazır", "Ready") } else { gui::ui_text("Başlatılamadı / aygıt yok", "Initialization failed / device missing") }),

                    2 => np.render_content(fb, win.x, win.y, win.width, win.height, tick, &win.scroll),

                    3 => br.render_content(fb, win.x, win.y, win.width, win.height, &win.scroll),

                    4 => iv.render_content(fb, win.x, win.y, win.width, win.height),

                    5 => mp.render_content(fb, win.x, win.y, win.width, win.height, tick),

                    6 => vp.render_content(fb, win.x, win.y, win.width, win.height, tick),

                    7 => tm.render_content(fb, win.x, win.y, win.width, win.height),

                    8 => calc.render_content(fb, win.x, win.y, win.width, win.height),

                    9 => if let Some(app) = paint[idx].as_ref() { app.render_content(fb, win.x, win.y, win.width, win.height) },

                    10 => term.render_content(fb, win.x, win.y, win.width, win.height, win.is_focused),

                    11 => inst.render_content(fb, win.x, win.y, win.width, win.height),

                    12 => v3d.render_content(fb, win.x, win.y, win.width, win.height),

                    13 => pkg.render_content(fb, win.x, win.y, win.width, win.height),

                    _ => {}

                }

                app_render_cycles[idx] = read_cpu_cycles().wrapping_sub(render_start);



                // Kırpmayı kaldır ve kaydırma çubuklarını pencerenin içinde çiz.

                fb.reset_clip();

                let sb = crate::gui::scrollbar::SCROLLBAR_THICKNESS;

                if want_v {

                    let v_h = ch.saturating_sub(if want_h { sb } else { 0 });

                    if v_h > 0 {

                        // Sağa tam dayalı dikey kaydırma çubuğu

                        let track = (cx + cw - sb, cy, sb, v_h);

                        let g = crate::gui::scrollbar::v_geom(track, content_h, v_h, win.scroll.y);

                        crate::gui::scrollbar::draw_vertical(fb, &g);

                    }

                }

                if want_h {

                    let h_w = cw.saturating_sub(if want_v { sb } else { 0 });

                    if h_w > 0 {

                        // Alta tam dayalı yatay kaydırma çubuğu

                        let track = (cx, cy + ch - sb, h_w, sb);

                        let g = crate::gui::scrollbar::h_geom(track, content_w, h_w, win.scroll.x);

                        crate::gui::scrollbar::draw_horizontal(fb, &g);

                    }

                }

            }

        }

        fb.reset_clip();



        // Aero Snap Önizleme Çerçevesi (Pencere ekrana yaslanırken yarı saydam/kesikli gösterim)

        if let Some((sx, sy, sw, sh)) = snap_preview {

            fb.draw_rounded_rect_alpha(sx, sy, sw, sh, 4, gui::Color::ACCENT_BLUE, 55);

            fb.draw_rounded_rect_outline(sx, sy, sw, sh, 4, gui::Color::ACCENT_BLUE);

            fb.draw_rounded_rect_outline(sx + 1, sy + 1, sw.saturating_sub(2), sh.saturating_sub(2), 4, gui::Color::WHITE);

        }



        // 3. Görev Çubuğu, sistem tepsisi, popup'lar ve fare imleci

        compositor.render_taskbar_and_overlays(

            fb, mouse, kbd, now, st.volume, st.is_muted, st.language.is_english(),

            drivers::e1000::is_ready(),

        );



        if fm.drag_active {

            fm.draw_drag_badge(fb, mouse.x as usize, mouse.y as usize, kbd.ctrl);

        } else if desktop_fm.drag_active {

            desktop_fm.draw_drag_badge(fb, mouse.x as usize, mouse.y as usize, kbd.ctrl);

        }



        // Dosya / Klasör Özellikler (Properties) Modalı

        // NOT: Görev çubuğu/katmanlarından ÖNCE çizilir; böylece fare imleci

        // modalin üstünde görünür ve modal imleci gizlemez.

        if properties_state.0 {

            let pw = 340usize.min(fb.width.saturating_sub(20));

            let ph = 260usize.min(fb.height.saturating_sub(40));

            let px = fb.width.saturating_sub(pw) / 2;

            let py = fb.height.saturating_sub(ph) / 2;



            // Gölge ve modern kart gövdesi

            fb.draw_shadow(px, py, pw, ph, 0x000000);

            fb.draw_rounded_rect(px, py, pw, ph, 8, gui::Color::WINDOW_BG);

            fb.draw_rounded_rect_outline(px, py, pw, ph, 8, gui::Color::ACCENT_BLUE);



            // Başlık çubuğu

            fb.draw_rect(px, py, pw, 28, gui::Color::TITLE_BAR);

            let prop_title = gui::ui_text("Özellikler", "Properties");

            gui::MinGuiCompositor::draw_string(fb, px + 12, py + 7, prop_title, gui::Color::WHITE);



            // Kapatma butonu (Sağ üst 'X')

            let close_r = (px + pw.saturating_sub(24), py + 6, 16, 16);

            let mx = mouse.x as usize;

            let my = mouse.y as usize;

            let close_hover = gui::in_rect(mx, my, close_r);

            fb.draw_rect(close_r.0, close_r.1, close_r.2, close_r.3, if close_hover { gui::Color::ACCENT_RED } else { gui::Color::TITLE_BAR });

            gui::MinGuiCompositor::draw_string(fb, close_r.0 + 4, close_r.1 + 1, "x", gui::Color::WHITE);



            // Dosya / Klasör İkonu

            gui::MinGuiCompositor::draw_gear_icon(fb, px + 18, py + 38);



            // Öğe Adı

            let item_name = properties_state.1.as_str();

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 44, py + 42, item_name, gui::Color::TEXT_MAIN, (px + 44, py + 38, pw.saturating_sub(54), 20));



            // Ayırıcı çizgi

            fb.draw_rect(px + 14, py + 66, pw.saturating_sub(28), 1, gui::Color::BORDER);



            // Detay Satırları

            let label_col = gui::Color::TEXT_MUTED;

            let val_col = gui::Color::TEXT_MAIN;

            let start_y = py + 76;

            let row_gap = 26;



            // Tür:

            gui::MinGuiCompositor::draw_string(fb, px + 16, start_y, gui::ui_text("Tür:", "Type:"), label_col);

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 90, start_y, properties_state.2.as_str(), val_col, (px + 90, start_y, pw.saturating_sub(100), 20));



            // Boyut:

            gui::MinGuiCompositor::draw_string(fb, px + 16, start_y + row_gap, gui::ui_text("Boyut:", "Size:"), label_col);

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 90, start_y + row_gap, properties_state.3.as_str(), val_col, (px + 90, start_y + row_gap, pw.saturating_sub(100), 20));



            // Konum:

            gui::MinGuiCompositor::draw_string(fb, px + 16, start_y + row_gap * 2, gui::ui_text("Konum:", "Location:"), label_col);

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 90, start_y + row_gap * 2, properties_state.4.as_str(), val_col, (px + 90, start_y + row_gap * 2, pw.saturating_sub(100), 20));



            // Tam Yol:

            gui::MinGuiCompositor::draw_string(fb, px + 16, start_y + row_gap * 3, gui::ui_text("Tam Yol:", "Full Path:"), label_col);

            gui::MinGuiCompositor::draw_string_clipped(fb, px + 90, start_y + row_gap * 3, properties_state.5.as_str(), val_col, (px + 90, start_y + row_gap * 3, pw.saturating_sub(100), 20));



            // Tamam (Kapat) Butonu

            let ok_btn_w = 76;

            let ok_btn_h = 24;

            let ok_btn_x = px + (pw.saturating_sub(ok_btn_w)) / 2;

            let ok_btn_y = py + ph.saturating_sub(34);

            let ok_hover = gui::in_rect(mx, my, (ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h));

            fb.draw_rounded_rect(ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h, 4, if ok_hover { gui::Color::ACCENT_BLUE } else { gui::Color::TITLE_BAR });

            fb.draw_rounded_rect_outline(ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h, 4, gui::Color::BORDER);

            let ok_text = gui::ui_text("Tamam", "OK");

            let tw = gui::MinGuiCompositor::text_width(ok_text, 1);

            gui::MinGuiCompositor::draw_string(fb, ok_btn_x + (ok_btn_w.saturating_sub(tw)) / 2, ok_btn_y + 4, ok_text, gui::Color::WHITE);

        }



        // Hızlı Çalıştır (Run Dialog: Win+R) Diyaloğu

        if run_dialog_state.0 {

            let dw = 320usize;

            let dh = 110usize;

            let dx = fb.width.saturating_sub(dw) / 2;

            let dy = fb.height.saturating_sub(dh) / 2;

            fb.draw_shadow(dx, dy, dw, dh, 0x000000);

            fb.draw_rounded_rect(dx, dy, dw, dh, 6, gui::Color::WINDOW_BG);

            fb.draw_rounded_rect_outline(dx, dy, dw, dh, 6, gui::Color::ACCENT_BLUE);

            fb.draw_rect(dx, dy, dw, 24, gui::Color::TITLE_BAR);

            gui::MinGuiCompositor::draw_string(fb, dx + 10, dy + 6,

                gui::ui_text("Çalıştır (Run)", "Run Program"), gui::Color::WHITE);

            gui::MinGuiCompositor::draw_string(fb, dx + 12, dy + 34,

                gui::ui_text("Komut veya uygulama adı:", "Command or app name:"), gui::Color::TEXT_MUTED);

            // Giriş kutusu

            let bx = dx + 12;

            let by = dy + 54;

            let bw = dw - 24;

            let bh = 24;

            fb.draw_rect(bx, by, bw, bh, gui::Color::DARK_BG);

            fb.draw_rect_outline(bx, by, bw, bh, gui::Color::BORDER);

            let input_text = run_dialog_state.1.as_str();

            gui::MinGuiCompositor::draw_string(fb, bx + 6, by + 6, input_text, gui::Color::TEXT_MAIN);

            // İmleç

            let cur_x = bx + 6 + gui::MinGuiCompositor::text_width(input_text, 1);

            if (tick / 20) % 2 == 0 {

                fb.draw_rect(cur_x, by + 4, 2, 16, gui::Color::ACCENT_YELLOW);

            }

            // İpucu

            gui::MinGuiCompositor::draw_string(fb, dx + 12, dy + 88,

                gui::ui_text("cmd, not, ayar, task, calc, web  |  [Enter] Aç  [Esc] Çık",

                              "cmd, not, ayar, task, calc, web  |  [Enter] Open  [Esc] Cancel"),

                gui::Color::TEXT_MUTED);

        }



        // 4. Backbuffer'ı VRAM'e kopyala.

        //    Kirli dikdörtgen verildiyse yalnızca o bölge blit edilir ve mavi

        //    ışık filtresi de sadece o bölgeye uygulanır; böylece kalan bölge

        //    önceki karedeki (filtrelenmiş) hâlini korur.

        let nl = if st.night_light { st.night_light_level } else { 0 };

        let region = fb.base_clip();

        if dirty_rect.is_some() {

            fb.apply_night_light_rect(nl, region);

            fb.flush_rect((region.x0, region.y0, region.x1 - region.x0, region.y1 - region.y0));

        } else {

            fb.apply_night_light(nl);

            fb.flush();

        }

        fb.clear_frame_clip();

    };



    // Karşılama bildirimi (Modern Toast)

    let welcome_title = gui::ui_text("minOs'a Hoş Geldiniz", "Welcome to minOs");

    let welcome_msg = gui::ui_text("Sistem hazır. Başlat menüsü ve kısayollar aktif.", "System is ready. Start menu and shortcuts active.");

    compositor.push_toast(gui::ToastKind::Info, welcome_title, welcome_msg, monotonic_ms(), 5500);



    // İlk kareyi çiz (tüm ekran)

    render_full_frame(

        &mut fb, &compositor, &filemanager_app, &desktop_files, &settings_app,

        &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,

        &videoplayer_app, &taskmgr_app, &calculator_app, &paint_apps, &terminal_app, &installer_app, &viewer3d_app, &pkgmgr_app, &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, anim_tick,

        &mut app_render_cycles,

        (run_dialog_open, &run_dialog_input),

        (properties_open, &properties_name, &properties_type, &properties_size, &properties_location, &properties_path),

        None,

        None,

    );



    let mut drag_prev_box: Option<(usize, usize, usize, usize)> = None;

    let mut drag_dirty_rect: Option<(usize, usize, usize, usize)> = None;

    let mut _app_dirty_rect: Option<(usize, usize, usize, usize)> = None;



    // 7. Ana Masaüstü Etkileşim ve Olay Döngüsü

    serial_print("[BOOT] Masaustu dongusu basladi.\n");

    loop {

        let loop_start_cycles = read_cpu_cycles();

        mouse.speed = settings_app.mouse_speed;

        mouse.reverse_scroll = settings_app.reverse_scroll;

        let mut input_update = drivers::ps2::poll_input(&mut *mouse, &mut *keyboard, width as i32, height as i32);

        let usb_input_update = drivers::usb_hid::poll_input(

            &mut *mouse,

            &mut *keyboard,

            width as i32,

            height as i32,

            &mut memory_mgr.pmm,

        );

        input_update.keyboard |= usb_input_update.keyboard;

        input_update.mouse |= usb_input_update.mouse;

        loop_counter = loop_counter.wrapping_add(1);

        let mx = mouse.x as usize;

        let my = mouse.y as usize;

        let mouse_clicked = mouse.left_button && !last_buttons.0;

        let mouse_released = !mouse.left_button && last_buttons.0;

        let right_clicked = mouse.right_button && !last_buttons.1;

        let l = gui::MinGuiCompositor::tray_layout(width, height);

        let tb_y = l.tb_y;



        // RTC saatini işlem döngüsü sayısına göre değil, geçen gerçek zamana göre yenile.

        let mut force_render = false;

        let clock_now_ms = monotonic_ms();

        if clock_now_ms.wrapping_sub(last_clock_update_ms) >= 1_000 {

            last_clock_update_ms = clock_now_ms;

            now = drivers::rtc::DateTime::read().with_hour_offset(settings_app.region_offset());

            force_render = true;

        }



        // Toast bildirim sürelerini güncelle

        let had_toasts = compositor.toasts.iter().any(|t| t.is_some());

        compositor.update_toasts(clock_now_ms);

        let has_toasts = compositor.toasts.iter().any(|t| t.is_some());

        if had_toasts != has_toasts {

            force_render = true;

        }



        // Ses cihazını ayarlarla senkronla

        audio_dev.set_volume(settings_app.volume);

        audio_dev.is_muted = settings_app.is_muted;

        musicplayer_app.volume = settings_app.volume as usize;



        // 1. Fare Sağ Tık — tıklanan yere göre bağlam menüsü

        if right_clicked {

            compositor.is_calendar_open = false;

            compositor.is_volume_popup_open = false;

            compositor.close_start_menu();

            compositor.is_context_menu_open = true;

            compositor.is_context_submenu_open = false;

            compositor.context_menu_x = mx;

            compositor.context_menu_y = my;

            compositor.context_file_idx = None;

            compositor.context_window = None;



            // Tıklanan pencereyi en üstten aşağıya ara

            let (order, count) = compositor.draw_order();

            let mut hit_window: Option<(usize, gui::ContextKind)> = None;

            for k in (0..count).rev() {

                let idx = order[k];

                if let Some(win) = compositor.windows[idx] {

                    if !win.is_open || win.is_minimized {

                        continue;

                    }

                    let inside = mx >= win.x && mx < win.x + win.width

                        && my >= win.y && my < win.y + win.height;

                    if !inside {

                        continue;

                    }

                    compositor.context_window = Some(idx);

                    if my < win.y + gui::titlebar_h() {

                        hit_window = Some((idx, gui::ContextKind::Window));

                    } else if idx == 2 {

                        compositor.focus_window(idx);

                        hit_window = Some((idx, gui::ContextKind::Notepad));

                    } else if idx == 0 {

                        if let Some(i) = filemanager_app.hit_test(win.x, win.y, win.width, win.height, mx, my, win.scroll.y) {

                            compositor.context_file_idx = Some(i);

                            filemanager_app.selected_idx = i;

                        }

                        compositor.focus_window(idx);

                        hit_window = Some((idx, gui::ContextKind::FileManager));

                    } else if idx == 7 {

                        if taskmgr_app.active_tab == 0 {

                            if let Some(row) = taskmgr_app.row_at(mx, my, win.x, win.y, win.width) {

                                taskmgr_app.selected = row;

                                taskmgr_app.focused_control = 4 + row;

                                hit_window = Some((idx, gui::ContextKind::TaskManager));

                            } else {

                                hit_window = Some((idx, gui::ContextKind::Window));

                            }

                        } else {

                            hit_window = Some((idx, gui::ContextKind::Window));

                        }

                        compositor.focus_window(idx);

                    } else {

                        compositor.focus_window(idx);

                        hit_window = Some((idx, gui::ContextKind::Window));

                    }

                    break;

                }

            }



            if let Some((_, kind)) = hit_window {

                compositor.context_kind = kind;

            } else if !compositor.is_point_covered_by_window(mx, my) && my < tb_y {

                let selected = desktop_files.selected_items;

                let click_idx = desktop_files.last_click_idx;

                let click_ms = desktop_files.last_click_ms;

                desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                desktop_files.selected_items = selected;

                desktop_files.last_click_idx = click_idx;

                desktop_files.last_click_ms = click_ms;

                if let Some((desk_idx, _)) = desktop_item_at(mx, my, &desktop_files, width, height) {

                    compositor.context_file_idx = Some(desk_idx);

                    desktop_files.select_row(desk_idx, keyboard.ctrl, keyboard.shift);

                    compositor.context_kind = gui::ContextKind::DesktopItem;

                } else {

                    compositor.context_kind = gui::ContextKind::Desktop;

                }

            } else {

                compositor.is_context_menu_open = false;

            }

        }



        // 2. Fare Sol Tıklama Olayları (Mouse Down)

        if mouse_clicked {

            compositor.finish_alt_tab();

            let mut consumed = false;



            // Açık menü/popup dışına tıklanınca kapat; tıklamayı tüketmeden

            // bırak ki aynı tıklama alttaki pencere veya görev çubuğuna ulaşsın.

            if compositor.is_start_menu_open {

                let (menu_x, menu_y, menu_w, menu_h) = gui::MinGuiCompositor::start_menu_rect(tb_y);

                let submenu = gui::MinGuiCompositor::start_menu_submenu_rect(

                    menu_y, compositor.active_start_group, tb_y,

                );

                if !gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h))

                    && !gui::in_rect(mx, my, submenu)

                {

                    compositor.close_start_menu();

                }

            }

            if compositor.is_context_menu_open {

                let hit_main = compositor.context_hit(width, height, mx, my).is_some();

                let hit_sub = compositor.context_submenu_hit(width, height, mx, my).is_some();

                if !hit_main && !hit_sub {

                    compositor.is_context_menu_open = false;

                    compositor.is_context_submenu_open = false;

                }

            }

            if compositor.is_volume_popup_open

                && !gui::in_rect(mx, my, compositor.volume_slider_rect(&l))

            {

                compositor.is_volume_popup_open = false;

            }

            if compositor.is_calendar_open

                && !gui::in_rect(mx, my, gui::MinGuiCompositor::calendar_rect(width, &l))

            {

                compositor.is_calendar_open = false;

            }

            if taskmgr_app.launch_menu_open {

                let in_menu = compositor.windows[7].map_or(false, |w| {

                    gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::launch_menu_rect(w.x, w.y, w.width))

                });

                let in_button = compositor.windows[7].map_or(false, |w| {

                    gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::launch_button_rect(w.x, w.y, w.width))

                });

                if !in_menu && !in_button { taskmgr_app.launch_menu_open = false; }

            }

            if run_dialog_open {

                let rbox = (width.saturating_sub(320) / 2, height.saturating_sub(110) / 2, 320, 110);

                if !gui::in_rect(mx, my, rbox) {

                    run_dialog_open = false;

                    run_dialog_input.clear();

                } else {

                    consumed = true;

                }

            }

            // Özellikler (Properties) modalı: modal açıkken tüm tıklamaları modal üstlenir.

            // X / Tamam butonları ve modal dışına tıklama pencereyi kapatır.

            if properties_open {

                let pw = 340usize.min(width.saturating_sub(20));

                let ph = 260usize.min(height.saturating_sub(40));

                let px = width.saturating_sub(pw) / 2;

                let py = height.saturating_sub(ph) / 2;

                let close_r = (px + pw.saturating_sub(24), py + 6, 16, 16);

                let ok_btn_w = 76usize;

                let ok_btn_h = 24usize;

                let ok_btn_x = px + (pw.saturating_sub(ok_btn_w)) / 2;

                let ok_btn_y = py + ph.saturating_sub(34);

                let on_close = gui::in_rect(mx, my, close_r);

                let on_ok = gui::in_rect(mx, my, (ok_btn_x, ok_btn_y, ok_btn_w, ok_btn_h));

                if on_close || on_ok || !gui::in_rect(mx, my, (px, py, pw, ph)) {

                    properties_open = false;

                    properties_name.clear();

                    properties_type.clear();

                    properties_size.clear();

                    properties_location.clear();

                    properties_path.clear();

                }

                consumed = true;

                force_render = true;

            }

            if !consumed && compositor.toast_click(width, tb_y, mx, my) {

                consumed = true;

                force_render = true;

            }

            // Not Defteri metin alanı: imleci konumlandır, çift tıkta kelime seç,

            // tek tıkta seçimi temizle ve sürükleyerek seçimi başlat.

            if let Some((_, w)) = focused_window_for_app(&compositor, 2) {

                if notepad_app.menu != 0 {

                    let in_menu_box = gui::in_rect(

                        mx, my,

                        (

                            w.x + crate::apps::notepad::NotepadApp::menu_box_x(notepad_app.menu),

                            w.y + crate::apps::notepad::NotepadApp::menu_box_y(),

                            crate::apps::notepad::NotepadApp::menu_box_w(),

                            crate::apps::notepad::NotepadApp::menu_box_h(notepad_app.menu),

                        ),

                    );

                    let in_tabs = my >= w.y + 30 && my < w.y + 30 + crate::gui::sp(24);

                    if !in_menu_box && !in_tabs {

                        // Menü açıkken dışarı tıklandı -> menüyü kapat, alttaki metni tıklama

                        notepad_app.menu = 0;

                        notepad_dragging = false;

                        consumed = true;

                    }

                    // Menü kutusu veya sekmeler tıklanmışsa pencere olay işleyicisi işleyecek

                } else if w.is_focused

                    && gui::in_rect(mx, my, (w.x + 6, w.y + 58, w.width.saturating_sub(12), w.height.saturating_sub(86)))

                {

                    let at = notepad_app.index_at_scrolled(mx - w.x, my - w.y, w.width, w.scroll.y);

                    // Çift tık algılama: aynı konuma kısa sürede ikinci tık.

                    let now_ms = monotonic_ms() as usize;

                    let dbl = now_ms.abs_diff(notepad_app.last_click_tick) <= settings_app.double_click_ms as usize

                        && notepad_app.last_click_pos.0.abs_diff(mx) < 4

                        && notepad_app.last_click_pos.1.abs_diff(my) < 4;

                    if dbl {

                        notepad_app.select_word_at(at);

                    } else {

                        notepad_app.cursor = at;

                        if keyboard.shift {

                            let anchor = notepad_app.selection_anchor.unwrap_or(at);

                            if anchor == at {

                                notepad_app.selection_start = None;

                                notepad_app.selection_end = None;

                            } else {

                                notepad_app.selection_start = Some(anchor.min(at));

                                notepad_app.selection_end = Some(anchor.max(at));

                            }

                        } else {

                            notepad_app.selection_anchor = Some(at);

                            notepad_app.selection_start = None;

                            notepad_app.selection_end = None;

                        }

                        notepad_dragging = true;

                    }

                    notepad_app.last_click_tick = now_ms;

                    notepad_app.last_click_pos = (mx, my);

                    consumed = true;

                }

            }



            // A) Takvim açıkken

            if compositor.is_calendar_open {

                let crect = gui::MinGuiCompositor::calendar_rect(width, &l);

                if gui::in_rect(mx, my, crect) {

                    let (cx, cy, cw, _ch) = crect;

                    if my < cy + 34 {

                        if mx < cx + 34 {

                            let (y, m) = step_month(compositor.cal_year, compositor.cal_month, false);

                            compositor.cal_year = y;

                            compositor.cal_month = m;

                        } else if mx > cx + cw - 34 {

                            let (y, m) = step_month(compositor.cal_year, compositor.cal_month, true);

                            compositor.cal_year = y;

                            compositor.cal_month = m;

                        }

                    }

                    consumed = true;

                } else {

                    compositor.is_calendar_open = false;

                }

            }



            // B) Ses paneli açıkken

            if !consumed && compositor.is_volume_popup_open {

                let panel = compositor.volume_slider_rect(&l);

                if gui::in_rect(mx, my, panel) {

                    if gui::in_rect(mx, my, compositor.volume_mute_rect(&l)) {

                        settings_app.is_muted = !settings_app.is_muted;

                    } else {

                        let track = compositor.volume_track_rect(&l);

                        if gui::in_rect(mx, my, track) { settings_app.set_volume_from_range(mx, track.0, track.2); }

                    }

                    consumed = true;

                } else {

                    compositor.is_volume_popup_open = false;

                }

            }



            // C) Bağlam menüsü açıkken

            if !consumed && compositor.is_context_menu_open {

                // Önce varsa açık olan "Yeni ▶" alt menüsündeki tıklamayı kontrol et

                if let Some(sub_item) = compositor.context_submenu_hit(width, height, mx, my) {

                    match compositor.context_kind {

                        gui::ContextKind::Desktop | gui::ContextKind::DesktopItem => {

                            match sub_item {

                                0 => { // Metin Belgesi (.txt)

                                    desktop_files.new_file(&mut fs_mgr.vfs);

                                    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                }

                                1 => { // Klasör

                                    desktop_files.new_folder(&mut fs_mgr.vfs);

                                    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                }

                                2 => { // Çizim / Resim (.bmp)

                                    desktop_files.new_custom_file("cizim_", ".bmp", &[], &mut fs_mgr.vfs);

                                    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                }

                                _ => { // Boş Belge

                                    desktop_files.new_custom_file("belge_", ".dat", &[], &mut fs_mgr.vfs);

                                    desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                }

                            }

                        }

                        gui::ContextKind::FileManager => {

                            match sub_item {

                                0 => { // Metin Belgesi (.txt)

                                    filemanager_app.new_file(&mut fs_mgr.vfs);

                                }

                                1 => { // Klasör

                                    filemanager_app.new_folder(&mut fs_mgr.vfs);

                                }

                                2 => { // Çizim / Resim (.bmp)

                                    filemanager_app.new_custom_file("cizim_", ".bmp", &[], &mut fs_mgr.vfs);

                                }

                                _ => { // Boş Belge

                                    filemanager_app.new_custom_file("belge_", ".dat", &[], &mut fs_mgr.vfs);

                                }

                            }

                        }

                        _ => {}

                    }

                    compositor.is_context_menu_open = false;

                    compositor.is_context_submenu_open = false;

                    consumed = true;

                } else if let Some(clicked_item) = compositor.context_hit(width, height, mx, my) {

                    // "Yeni ▶" öğesine tıklandıysa alt menüyü aç / kapa, ana menüyü kapatma

                    let is_sub_parent = match compositor.context_kind {

                        gui::ContextKind::Desktop => clicked_item == 1,

                        gui::ContextKind::DesktopItem => clicked_item == 4,

                        gui::ContextKind::FileManager => clicked_item == 5,

                        _ => false,

                    };



                    if is_sub_parent {

                        compositor.is_context_submenu_open = !compositor.is_context_submenu_open;

                        consumed = true;

                    } else {

                        match compositor.context_kind {

                            gui::ContextKind::Desktop => match clicked_item {

                                0 => { filemanager_app.set_path("/root/Documents"); filemanager_app.refresh_from_vfs(&fs_mgr.vfs); open_app(&mut compositor, 0); }

                                2 => { desktop_files.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }

                                3 => open_app(&mut compositor, 1),

                                4 => open_app(&mut compositor, 7),

                                5 => settings_app.cycle_theme(),

                                6 => { desktop_files.refresh_from_vfs(&fs_mgr.vfs); }

                                7 => open_app(&mut compositor, 2),

                                _ => {}

                            },

                            gui::ContextKind::DesktopItem => {

                                match clicked_item {

                                    0 => if let Some(i) = compositor.context_file_idx {

                                        let mut path = alloc::string::String::from("/desktop/"); path.push_str(desktop_files.files[i].display_name());

                                        if desktop_files.files[i].is_dir { filemanager_app.set_path(&path); filemanager_app.refresh_from_vfs(&fs_mgr.vfs); open_app(&mut compositor, 0); }

                                        else {

                                            let tag = desktop_files.files[i].icon_tag;

                                            if tag == "TXT" || tag == "LOG" || tag == "CFG" { if let Some(data) = fs_mgr.vfs.read_file(&path) { notepad_app.open_file(&path, data); open_app(&mut compositor, 2); } }

                                            else if tag == "IMG" { open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app); open_app(&mut compositor, 4); }

                                            else if tag == "SND" { open_app(&mut compositor, 5); }

                                            else if tag == "VID" { open_app(&mut compositor, 6); }

                                            else if tag == "WEB" { open_app(&mut compositor, 3); }

                                            else if tag == "3D" { if let Some(data) = fs_mgr.vfs.read_file(&path) { viewer3d_app.load_file(&path, data); open_app(&mut compositor, 12); } }

                                        }

                                    },

                                    1 => if let Some(i) = compositor.context_file_idx { let mut path = alloc::string::String::from("/desktop/"); path.push_str(desktop_files.files[i].display_name()); if let Some(data) = fs_mgr.vfs.read_file(&path) { notepad_app.open_file(&path, data); open_app(&mut compositor, 2); } },

                                    2 => desktop_files.copy_selection(&mut file_clipboard, true),

                                    3 => desktop_files.copy_selection(&mut file_clipboard, false),

                                    4 => {} // Yeni ▶

                                    5 => desktop_files.begin_rename(),

                                    6 => { desktop_files.delete_selected(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); },

                                    _ => {}

                                }

                            }

                            gui::ContextKind::FileManager => {

                                match clicked_item {

                                    0 => { // Aç / Klasöre Gir

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {

                                                if tag == "TXT" || tag == "LOG" || tag == "CFG" {

                                                    let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                                    if let Some(data) = data {

                                                        notepad_app.open_file(&path, data);

                                                    }

                                                    open_app(&mut compositor, 2);

                                                } else if tag == "IMG" {

                                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                                    open_app(&mut compositor, 4);

                                                } else if tag == "SND" {

                                                    open_app(&mut compositor, 5);

                                                } else if tag == "VID" {

                                                    open_app(&mut compositor, 6);

                                                } else if tag == "WEB" {

                                                    open_app(&mut compositor, 3);

                                                } else if tag == "3D" {

                                                    let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));

                                                    if let Some(data) = data {

                                                        viewer3d_app.load_file(&path, data);

                                                    }

                                                    open_app(&mut compositor, 12);

                                                }

                                            }

                                        }

                                    }

                                    1 => { // Not Defterinde Düzenle

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            let path = filemanager_app.selected_full_path();

                                            let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                            if let Some(data) = data {

                                                notepad_app.open_file(&path, data);

                                            }

                                            open_app(&mut compositor, 2);

                                        }

                                    }

                                    2 => { filemanager_app.copy_selection(&mut file_clipboard, true); }

                                    3 => { filemanager_app.copy_selection(&mut file_clipboard, false); }

                                    4 => { filemanager_app.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }

                                    5 => {} // Yeni ▶

                                    6 => { // Adını değiştir

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            filemanager_app.begin_rename();

                                        }

                                    }

                                    7 => { // Dosyayı / Klasörü Sil

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            filemanager_app.delete_selected(&mut fs_mgr.vfs);

                                        }

                                    }

                                    8 => { // Özellikler

                                        if let Some(file_idx) = compositor.context_file_idx {

                                            filemanager_app.selected_idx = file_idx;

                                            if file_idx < filemanager_app.file_count {

                                                let file = &filemanager_app.files[file_idx];

                                                properties_name.clear();

                                                properties_name.push_str(file.display_name());

                                                properties_type.clear();

                                                properties_type.push_str(file.file_type);

                                                properties_size.clear();

                                                properties_size.push_str(file.size_str.as_str());

                                                properties_location.clear();

                                                properties_location.push_str(filemanager_app.path());

                                                properties_path.clear();

                                                let full_path = filemanager_app.selected_full_path();

                                                properties_path.push_str(&full_path);

                                                properties_open = true;

                                            }

                                        }

                                    }

                                    _ => {}

                                }

                            }

                            gui::ContextKind::Notepad => match clicked_item {

                                0 => notepad_app.cut_selection(),

                                1 => notepad_app.copy_selection(),

                                2 => notepad_app.paste_clipboard(),

                                3 => notepad_app.delete_selection(),

                                4 => notepad_app.select_all(),

                                _ => notepad_app.clear(),

                            },

                            gui::ContextKind::TaskManager => match clicked_item {

                                0 => {

                                    // Sonlandır (Del): seçili süreci kapat

                                    if let Some(target) = taskmgr_app.selected_app_idx() {

                                        taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");

                                        if let Some(wi) = compositor.context_window {

                                            terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,

                                                &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,

                                                &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[wi],

                                                &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

                                        }

                                    }

                                }

                                _ => {}

                            },

                            gui::ContextKind::Window => {

                                if let Some(wi) = compositor.context_window {

                                    match clicked_item {

                                        0 => {

                                            if let Some(w) = compositor.windows[wi].as_mut() {

                                                w.is_minimized = true;

                                            }

                                            compositor.focus_top_window();

                                        }

                                        1 => {

                                            compositor.focus_window(wi);

                                            compositor.toggle_maximize(wi, width, height);

                                        }

                                        2 => {

                                            if let Some(w) = compositor.windows[wi].as_mut() {

                                                w.x = width.saturating_sub(w.width) / 2;

                                                w.y = height

                                                    .saturating_sub(gui::taskbar_h())

                                                    .saturating_sub(w.height)

                                                    / 2;

                                            }

                                        }

                                        _ => {

                                            if let Some(w) = compositor.windows[wi].as_mut() {

                                                w.is_open = false;

                                            }

                                            compositor.focus_top_window();

                                        }

                                    }

                                }

                            }

                        }

                        compositor.is_context_menu_open = false;

                        compositor.is_context_submenu_open = false;

                        consumed = true;

                    }

                } else {

                    compositor.is_context_menu_open = false;

                    compositor.is_context_submenu_open = false;

                    consumed = true;

                }

            }



        if compositor.is_start_menu_open {

            let (_, menu_y, _, _) = gui::MinGuiCompositor::start_menu_rect(tb_y);

            for g in 0..4 {

                let cat_r = gui::MinGuiCompositor::start_menu_category_rect(menu_y, g);

                if gui::in_rect(mx, my, cat_r) {

                    compositor.active_start_group = g;

                    break;

                }

            }

        }



        // D) Başlat menüsü açıkken

        if !consumed && compositor.is_start_menu_open {

            let (menu_x, menu_y, menu_w, menu_h) = gui::MinGuiCompositor::start_menu_rect(tb_y);

            let search_rect = gui::MinGuiCompositor::start_search_rect(menu_x, menu_y, menu_w);

            let active_g = compositor.active_start_group;

            let mut sub_rect = gui::MinGuiCompositor::start_menu_submenu_rect(menu_y, active_g, tb_y);

            if compositor.start_search_len > 0 {

                let mut matches = [0; 14];

                let count = compositor.start_search_matches(settings_app.language.is_english(), &mut matches);

                sub_rect = gui::MinGuiCompositor::start_search_results_rect(menu_y, active_g, tb_y, count.max(1));

            }

            if compositor.start_search_len > 0 && gui::in_rect(mx, my, sub_rect) {

                let mut matches = [0; 14];

                let count = compositor.start_search_matches(settings_app.language.is_english(), &mut matches);

                for (row, app_id) in matches[..count].iter().enumerate() {

                    let row_rect = (sub_rect.0 + gui::sp(6), sub_rect.1 + gui::sp(30) + row * gui::sp(28), sub_rect.2.saturating_sub(gui::sp(12)), gui::sp(25));

                    if gui::in_rect(mx, my, row_rect) {

                        open_app(&mut compositor, *app_id);

                        compositor.close_start_menu();

                        break;

                    }

                }

                consumed = true;

            } else if compositor.start_search_len > 0 {

                let menu_inside = gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h));

                if menu_inside && !gui::in_rect(mx, my, search_rect) {

                    consumed = true;

                } else if !menu_inside {

                    compositor.close_start_menu();

                    consumed = true;

                }

            }

            if gui::in_rect(mx, my, search_rect) {

                compositor.clear_start_search();

                consumed = true;

            }



            let mut cat_clicked = consumed;

            if gui::in_rect(mx, my, (menu_x + menu_w - 34, menu_y + 6, 26, 22)) {

                open_app(&mut compositor, 1);

                settings_app.active_tab = 0;

                compositor.close_start_menu();

                consumed = true;

                cat_clicked = true;

            }

            for g in 0..4 {

                if cat_clicked { break; }

                let cat_r = gui::MinGuiCompositor::start_menu_category_rect(menu_y, g);

                if gui::in_rect(mx, my, cat_r) {

                    compositor.active_start_group = g;

                    cat_clicked = true;

                    consumed = true;

                    break;

                }

            }



            if !cat_clicked {

                let (_, first, count) = gui::MENU_GROUPS[active_g];

                let mut item_clicked = false;

                for k in 0..count {

                    let i = first + k;

                    let rect = gui::MinGuiCompositor::start_menu_subitem_rect(menu_y, active_g, k, tb_y);

                    if gui::in_rect(mx, my, rect) {

                        item_clicked = true;

                        if i == 14 {

                            if compositor.confirm_shutdown {

                                #[cfg(minos_boot_log)]

    serial_print("[minOs] System Shutdown confirmed via Start Menu!\n");

                                unsafe {

                                    core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);

                                    core::arch::asm!("out dx, ax", in("dx") 0x404u16, in("ax") 0x3400u16);

                                    core::arch::asm!("cli; hlt");

                                }

                            } else {

                                compositor.confirm_shutdown = true;

                                compositor.confirm_restart = false;

                            }

                            break;

                        }

                        if i == 15 {

                            if compositor.confirm_restart {

                                restart_system();

                            }

                            compositor.confirm_restart = true;

                            compositor.confirm_shutdown = false;

                            break;

                        }

                        compositor.confirm_shutdown = false;

                        compositor.confirm_restart = false;

                        if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(i) {

                            open_app(&mut compositor, app_idx);

                        }

                        compositor.close_start_menu();

                        break;

                    }

                }



                if !item_clicked {

                    if gui::in_rect(mx, my, (menu_x, menu_y, menu_w, menu_h)) || gui::in_rect(mx, my, sub_rect) {

                        consumed = true;

                    } else {

                        compositor.close_start_menu();

                        consumed = true;

                    }

                } else {

                    consumed = true;

                }

            }

        }



        // E) Sistem tepsisi düğmeleri

        if !consumed {

            if gui::in_rect(mx, my, l.start) {

                compositor.is_start_menu_open = !compositor.is_start_menu_open;

                if compositor.is_start_menu_open {

                    compositor.active_start_group = 0;

                    compositor.clear_start_search();

                } else {

                    compositor.close_start_menu();

                }

                consumed = true;

                } else if gui::in_rect(mx, my, l.clock) {

                    compositor.is_calendar_open = !compositor.is_calendar_open;

                    if compositor.is_calendar_open {

                        compositor.cal_year = now.year;

                        compositor.cal_month = now.month;

                    }

                    consumed = true;

                } else if l.volume.2 > 0 && gui::in_rect(mx, my, l.volume) {

                    compositor.is_volume_popup_open = !compositor.is_volume_popup_open;

                    consumed = true;

                } else if l.network.2 > 0 && gui::in_rect(mx, my, l.network) {

                    // Ethernet simgesi ağ ayarları sekmesini açar.

                    open_app(&mut compositor, 1);

                    settings_app.active_tab = 2;

                    settings_app.status_msg = gui::ui_text("Ağ ayarları açıldı.", "Network settings opened.");

                    consumed = true;

                } else if gui::in_rect(mx, my, l.theme) {

                    settings_app.cycle_theme();

                    consumed = true;

                } else if gui::in_rect(mx, my, l.kbd) {

                    keyboard.toggle_layout();

                    settings_app.keyboard_layout = keyboard.layout;

                    consumed = true;

                }

            }



            // F) Görev çubuğu uygulama sekmeleri (font ölçeğine duyarlı)

            let tab_h = gui::MinGuiCompositor::taskbar_tab_rect_at(0, tb_y).3;

            if !consumed && my >= tb_y + 5 && my < tb_y + 5 + tab_h {

                let open_count = compositor.windows.iter().filter(|slot| slot.map_or(false, |win| win.is_open)).count();

                let (tab_start, tab_width, tab_step, shown_tabs, tab_slots) =

                    gui::MinGuiCompositor::taskbar_tabs_layout(width, height, open_count);

                let mut tab_slot = 0usize;

                for idx in 0..gui::MAX_WINDOWS {

                    if let Some(win) = compositor.windows[idx] {

                        if win.is_open {

                            if tab_slot >= shown_tabs { break; }

                            let tab_x = tab_start + tab_slot * tab_step;

                            if mx >= tab_x && mx < tab_x + tab_width {

                                if win.is_minimized {

                                    focus_app_window(&mut compositor, idx);

                                } else if win.is_focused {

                                    if let Some(w) = compositor.windows[idx].as_mut() {

                                        w.is_minimized = true;

                                    }

                                    compositor.focus_top_window();

                                } else {

                                    compositor.focus_window(idx);

                                }

                                consumed = true;

                                break;

                            }

                            tab_slot += 1;

                        }

                    }

                }

                if !consumed && tab_slots > shown_tabs {

                    let badge_x = tab_start + shown_tabs * tab_step;

                    let badge_w = tab_width.min(gui::MinGuiCompositor::tray_layout(width, height).kbd.0

                        .saturating_sub(gui::sp(4)).saturating_sub(badge_x));

                    if mx >= badge_x && mx < badge_x + badge_w {

                        // +N alanı gizli sekmeler arasında sırayla dolaşır.

                        let mut ordinal = 0usize;

                        let mut first_hidden = None;

                        let mut focused_hidden_ordinal = None;

                        for hidden_idx in 0..gui::MAX_WINDOWS {

                            if let Some(hidden) = compositor.windows[hidden_idx] {

                                if hidden.is_open {

                                    if ordinal >= shown_tabs {

                                        if first_hidden.is_none() { first_hidden = Some(hidden_idx); }

                                        if hidden.is_focused { focused_hidden_ordinal = Some(ordinal); }

                                    }

                                    ordinal += 1;

                                }

                            }

                        }

                        let mut next_hidden = None;

                        if let Some(active_ordinal) = focused_hidden_ordinal {

                            let mut ordinal = 0usize;

                            for hidden_idx in 0..gui::MAX_WINDOWS {

                                if let Some(hidden) = compositor.windows[hidden_idx] {

                                    if hidden.is_open {

                                        if ordinal > active_ordinal { next_hidden = Some(hidden_idx); break; }

                                        ordinal += 1;

                                    }

                                }

                            }

                        }

                        if let Some(target) = next_hidden.or(first_hidden) {

                            focus_app_window(&mut compositor, target);

                        }

                        consumed = true;

                    }

                }

            }



            // G) Masaüstü simgeleri

            if !consumed && !compositor.is_point_covered_by_window(mx, my) && my < tb_y {

                // Yenileme tıklama durumunu sıfırlar; çift tık tespiti için koru

                // (yoksa her tıklama durumu siler ve çift tıklama hiç algılanamaz).

                let saved_click_idx = desktop_files.last_click_idx;

                let saved_click_ms = desktop_files.last_click_ms;

                desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                desktop_files.last_click_idx = saved_click_idx;

                desktop_files.last_click_ms = saved_click_ms;

                if let Some((i, _)) = desktop_item_at(mx, my, &desktop_files, width, height) {

                    desktop_files.select_row(i, keyboard.ctrl, keyboard.shift);

                    compositor.active_window_idx = None;

                    let now_ms = monotonic_ms();

                    // Masaüstünün kendi tıklama durumu kullanılır (dosya yöneticisi

                    // penceresinin durumu değil); aksi halde çift tık hiç tutmaz.

                    let is_double = desktop_files.last_click_idx == Some(i)

                        && now_ms.wrapping_sub(desktop_files.last_click_ms) <= settings_app.double_click_ms as u64;

                    if is_double {

                        desktop_files.last_click_idx = None;

                        desktop_files.drag_item = None;

                        let mut p = alloc::string::String::from("/desktop/");

                        p.push_str(desktop_files.files[i].display_name());

                        if desktop_files.files[i].is_dir {

                            filemanager_app.set_path(&p); filemanager_app.refresh_from_vfs(&fs_mgr.vfs); open_app(&mut compositor, 0);

                        } else {

                            let tag = desktop_files.files[i].icon_tag;

                            if tag == "TXT" || tag == "LOG" || tag == "CFG" {

                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&p));

                                if let Some(data) = data { notepad_app.open_file(&p, data); }

                                open_app(&mut compositor, 2);

                            } else if tag == "IMG" {

                                with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&p, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                open_app(&mut compositor, 4);

                            }

                            else if tag == "SND" { open_app(&mut compositor, 5); }

                            else if tag == "VID" { open_app(&mut compositor, 6); }

                            else if tag == "WEB" { open_app(&mut compositor, 3); }

                            else if tag == "3D" {

                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&p));

                                if let Some(data) = data {

                                    viewer3d_app.load_file(&p, data);

                                }

                                open_app(&mut compositor, 12);

                            }

                        }

                    } else {

                        desktop_files.last_click_idx = Some(i);

                        desktop_files.last_click_ms = now_ms;

                        desktop_files.drag_item = Some(i);

                        desktop_files.drag_start = (mx, my);

                        desktop_drag_source = true;

                    }

                    consumed = true;

                }

            }

            // Masaüstü sol şerit hızlı simgeleri (çizimle aynı kanonik geometri).

            if !consumed && !compositor.is_point_covered_by_window(mx, my) && my < tb_y {

                let desk_apps = [0, 2, 3, 5, 1, 11];

                // Kurulu sistemde kaldırılan kurulum kısayolunu (app 11) atla.

                let desk_icon_count = gui::MinGuiCompositor::desktop_quick_count();

                for (i, &app_idx) in desk_apps.iter().enumerate().take(desk_icon_count) {

                    if gui::in_rect(mx, my, gui::MinGuiCompositor::desktop_quick_rect(i)) {

                        open_app(&mut compositor, app_idx);

                        compositor.close_start_menu();

                        consumed = true;

                        break;

                    }

                }

            }



            // H) Pencereler (başlık, butonlar, içerik)

            if !consumed {

                let (order, count) = compositor.draw_order();

                for k in (0..count).rev() {

                    let idx = order[k];

                    let win = match compositor.windows[idx] {

                        Some(w) => w,

                        None => continue,

                    };

                    let (wx, wy, ww, wh) = (win.x, win.y, win.width, win.height);

                    // Standart pencere davranışı: üstteki pencere gövdesine tıklama odağı alır.

                    if gui::in_rect(mx, my, (wx, wy, ww, wh)) { compositor.focus_window(idx); }



                    // Kapat [X]

                    if gui::in_rect(mx, my, win.close_rect()) {

                        terminate_app(idx, &mut compositor, &mut filemanager_app, &mut settings_app,

                            &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,

                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[idx],

                            &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

                        break;

                    }

                    // Maksimize / Geri Yükle

                    if gui::in_rect(mx, my, win.max_rect()) {

                        compositor.focus_window(idx);

                        compositor.toggle_maximize(idx, width, height);

                        break;

                    }

                    // Simge durumuna küçült [-]

                    if gui::in_rect(mx, my, win.min_rect()) {

                        if let Some(w) = compositor.windows[idx].as_mut() {

                            w.is_minimized = true;

                        }

                        compositor.focus_top_window();

                        break;

                    }

                    // Dikey kaydırma çubuğu (pencere sağ kenarı) — tıklama/sürükleme.

                    {

                        let (ccx, ccy, ccw, cch) = win.content_rect();

                        let c_h = window_content_h(win.app_id as usize, wh, ww, &filemanager_app, &notepad_app, &browser_app);

                        let view_h = cch;

                        if c_h > view_h {

                            let t = gui::scrollbar::SCROLLBAR_THICKNESS;

                            // Sağa tam dayalı dikey scrollbar track'i

                            let track = (ccx + ccw - t, ccy, t, view_h);

                            if gui::in_rect(mx, my, track) {

                                let g = gui::scrollbar::v_geom(track, c_h, view_h, win.scroll.y);

                                // Tutamaç yakınındaysa tutamacın üstünü koru, değilse anında zıpla.

                                if gui::in_rect(mx, my, g.thumb) {

                                    vscroll_drag = Some((idx, g.thumb.1.saturating_sub(my), g.thumb.3, track.3, c_h, view_h));

                                } else {

                                    // Track üzerinde tıklanan konumdan kaydırmayı hesapla.

                                    let ratio = (my.saturating_sub(track.1)) as f32 / track.3.max(1) as f32;

                                    let max_off = c_h.saturating_sub(view_h);

                                    if let Some(wm) = compositor.windows[idx].as_mut() {

                                        wm.scroll.y = ((ratio * max_off as f32) as usize).min(max_off);

                                    }

                                    vscroll_drag = Some((idx, g.thumb.3 / 2, g.thumb.3, track.3, c_h, view_h));

                                }

                                compositor.focus_window(idx);

                                break;

                            }

                        }

                    }

                    // Pencere kenar ve köşelerinden boyutlandırma. Scrollbar

                    // alanı yukarıda tüketildi; geri kalan kenar alanı resize'a ait.

                    if !win.is_maximized {

                        if let Some(edge) = win.resize_edge_at(mx, my) {

                            compositor.focus_window(idx);

                            resizing_win = Some(idx);

                            resize_edge = Some(edge);

                            resize_orig = (wx, wy, ww, wh);

                            break;

                        }

                    }

                    // Başlık çubuğu — sürükle / çift tık ile maksimize

                    if gui::in_rect(mx, my, win.title_drag_rect()) {

                        let now_ms = monotonic_ms();

                        let dbl = last_title_idx == Some(idx)

                            && now_ms.wrapping_sub(last_title_click_ms) <= settings_app.double_click_ms as u64;

                        compositor.focus_window(idx);

                        if dbl {

                            compositor.toggle_maximize(idx, width, height);

                            dragging_win = None;

                            // Çift tık tüketildikten sonra eşleşmeyi temizle;

                            // üçüncü kısa tık yeni bir çift tık başlatabilsin.

                            last_title_idx = None;

                        } else {

                            last_title_idx = Some(idx);

                            last_title_click_ms = now_ms;

                            if !win.is_maximized {

                                dragging_win = Some(idx);

                                drag_offset_x = mx.saturating_sub(wx);

                                drag_offset_y = my.saturating_sub(wy);

                            }

                        }

                        break;

                    }



                    // Pencere gövdesi içindeki uygulama düğmeleri

                    if mx >= wx && mx < wx + ww && my >= wy && my < wy + wh {

                        let app_id = compositor.app_id_for_window(idx);

                        match app_id {

                            0 => {

                                // Dosya Yöneticisi: [Yenile], [+Dosya], [+Klasör], [Sil], [Aç], gezinti, liste

                                if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::refresh_button_rect(wx, wy, ww)) {

                                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::new_file_button_rect(wx, wy, ww)) {

                                    filemanager_app.new_file(&mut fs_mgr.vfs);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::new_folder_button_rect(wx, wy, ww)) {

                                    filemanager_app.new_folder(&mut fs_mgr.vfs);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::delete_button_rect(wx, wy, ww)) {

                                    filemanager_app.delete_selected(&mut fs_mgr.vfs);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::open_button_rect(wx, wy, ww)) {

                                    // Geri Dönüşüm Kutusu'ndayken "Aç" düğmesi öğeyi geri yükler.

                                    if filemanager_app.path() == crate::fs::vfs::TRASH_DIR {

                                        filemanager_app.restore_selected(&mut fs_mgr.vfs);

                                    } else if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {

                                        if tag == "TXT" || tag == "LOG" || tag == "CFG" {

                                            let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                            if let Some(data) = data {

                                                notepad_app.open_file(&path, data);

                                            }

                                            open_app(&mut compositor, 2);

                                        } else if tag == "IMG" {

                                            with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                            open_app(&mut compositor, 4);

                                        } else if tag == "SND" {

                                            open_app(&mut compositor, 5);

                                        } else if tag == "VID" {

                                            open_app(&mut compositor, 6);

                                        } else if tag == "WEB" {

                                            open_app(&mut compositor, 3);

                                        } else if tag == "3D" {

                                            let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));

                                            if let Some(data) = data {

                                                viewer3d_app.load_file(&path, data);

                                            }

                                            open_app(&mut compositor, 12);

                                        }

                                    }

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::view_button_rect(wx, wy, ww)) {

                                    filemanager_app.toggle_view_mode();

                                    filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::preview_button_rect(wx, wy, ww)) {

                                    filemanager_app.toggle_preview();

                                    filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                                    filemanager_app.update_preview(&fs_mgr.vfs);

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::header_name_rect(wx, wy, ww)) {

                                    filemanager_app.cycle_sort_by();

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::header_size_rect(wx, wy, ww)) {

                                    filemanager_app.cycle_sort_by();

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if gui::in_rect(mx, my, apps::filemanager::FileManagerApp::header_type_rect(wx, wy, ww)) {

                                    filemanager_app.cycle_sort_by();

                                    fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                                } else if mx >= wx + 6 && mx < wx + 116 && my >= wy + 66 && my < wy + 66 + 10 * 22 {

                                    let nav = (my.saturating_sub(wy + 66)) / 22;

                                    let target_path = match nav {

                                        0 => "/",

                                        1 => "/desktop",

                                        2 => "/root/Documents",

                                        3 => "/root/Media",

                                        4 => "/root/Downloads",

                                        5 => "/boot",

                                        6 => "/media/usb0",

                                        7 => "/media/hdd0",

                                        8 => "/media/cdrom",

                                        _ => "/root/Trash",

                                    };

                                    filemanager_app.set_path(target_path);

                                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                } else if mx >= wx + 124 && mx <= wx + ww - 10 && my >= wy + 84 {

                                    let sy = compositor.windows[idx].map(|w| w.scroll.y).unwrap_or(0);

                                    let list_top = wy + apps::filemanager::LIST_TOP;

                                    let list_bottom = wy + wh.saturating_sub(30);

                                    for i in 0..filemanager_app.file_count {

                                        if my >= list_top && my < list_bottom && gui::in_rect(mx, my, filemanager_app.item_rect(wx, wy, ww, i, sy)) {

                                            filemanager_app.select_row(i, keyboard.ctrl, keyboard.shift);

                                            let now_ms = monotonic_ms();

                                            let is_double_click = filemanager_app.last_click_idx == Some(i)

                                                && now_ms.wrapping_sub(filemanager_app.last_click_ms) <= settings_app.double_click_ms as u64;

                                            if is_double_click {

                                                filemanager_app.last_click_idx = None;

                                                filemanager_app.drag_item = None;

                                                filemanager_app.drag_active = false;

                                                filemanager_app.drag_target = None;

                                                if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {

                                                    if tag == "TXT" || tag == "LOG" || tag == "CFG" {

                                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                                        if let Some(data) = data {

                                                            notepad_app.open_file(&path, data);

                                                        }

                                                        open_app(&mut compositor, 2);

                                                    } else if tag == "IMG" {

                                                        with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                                        open_app(&mut compositor, 4);

                                                    } else if tag == "SND" {

                                                        open_app(&mut compositor, 5);

                                                    } else if tag == "VID" {

                                                        open_app(&mut compositor, 6);

                                                    } else if tag == "WEB" {

                                                        open_app(&mut compositor, 3);

                                                    } else if tag == "3D" {

                                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));

                                                        if let Some(data) = data {

                                                            viewer3d_app.load_file(&path, data);

                                                        }

                                                        open_app(&mut compositor, 12);

                                                    }

                                                }

                                            } else {

                                                filemanager_app.last_click_idx = Some(i);

                                                filemanager_app.last_click_ms = now_ms;

                                                if filemanager_app.files[i].display_name() != ".." {

                                                    filemanager_app.drag_item = Some(i);

                                                    filemanager_app.drag_start = (mx, my);

                                                    desktop_drag_source = false;

                                                    filemanager_app.drag_active = false;

                                                    filemanager_app.drag_target = None;

                                                }

                                            }

                                            break;

                                        }

                                    }

                                }

                            }

                            1 => {

                                // Sistem Ayarları

                                for t in 0..5 {

                                    if gui::in_rect(mx, my, apps::settings::SystemSettings::tab_rect(wx, wy, t)) {

                                        settings_app.active_tab = t;

                                    }

                                }

                                if gui::in_rect(mx, my, apps::settings::SystemSettings::save_btn_rect(wx, wy, wh)) {

                                    settings_app.save_all(&compositor, &mut fs_mgr.vfs);

                                } else if gui::in_rect(mx, my, apps::settings::SystemSettings::reset_btn_rect(wx, wy, wh)) {

                                    settings_app.reset_defaults();

                                    settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now);

                                    settings_app.write_config(&mut fs_mgr.vfs);

                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::dhcp_btn_rect(wx, wy)) {

                                    settings_app.dhcp_enabled = !settings_app.dhcp_enabled;

                                    settings_app.status_msg = if settings_app.dhcp_enabled { gui::ui_text("DHCP etkinleştirildi.", "DHCP enabled.") } else { gui::ui_text("Statik IP seçildi.", "Static IP selected.") };

                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::test_net_btn_rect(wx, wy)) {

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("Ağ testi yapılıyor...", "Running network test..."), || settings_app.run_network_test());

                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::test_ping_btn_rect(wx, wy)) {

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("Ping gönderiliyor...", "Sending ping..."), || settings_app.run_ping_test());

                                } else if settings_app.active_tab == 2 && gui::in_rect(mx, my, apps::settings::SystemSettings::test_http_btn_rect(wx, wy)) {

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("HTTP bağlantısı test ediliyor...", "Testing HTTP connection..."), || settings_app.run_http_test());

                                } else if settings_app.active_tab == 0 {

                                    let mut swatch_hit = false;

                                    for i in 0..apps::settings::SystemSettings::theme_swatch_count() {

                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::theme_swatch_rect(wx, wy, i)) {

                                            settings_app.theme = gui::theme::ThemeId::ALL[i];

                                            gui::theme::set_theme(settings_app.theme);

                                            swatch_hit = true;

                                            break;

                                        }

                                    }

                                    if swatch_hit {

                                        // Tema paletinden dogrudan secim yapildi.

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::theme_btn_rect(wx, wy, ww)) {

                                        settings_app.cycle_theme();

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::kbd_btn_rect(wx, wy, ww)) {

                                        keyboard.toggle_layout();

                                        settings_app.keyboard_layout = keyboard.layout;

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::language_btn_rect(wx, wy, ww)) {

                                        settings_app.language.toggle();

                                        settings_app.status_msg = gui::ui_text("Arayüz dili Türkçe olarak ayarlandı.", "Interface language set to English.");

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::mute_btn_rect(wx, wy)) {

                                        settings_app.is_muted = !settings_app.is_muted;

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::volume_track_rect(wx, wy)) {

                                        let track = apps::settings::SystemSettings::volume_track_rect(wx, wy); settings_app.set_volume_from_range(mx, track.0, track.2);

                                    }

                                }

                                if settings_app.active_tab == 1 {

                                    let mut swatch_hit = false;

                                    for i in 0..apps::settings::SystemSettings::theme_swatch_count() {

                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::theme_swatch_rect(wx, wy, i)) {

                                            settings_app.theme = gui::theme::ThemeId::ALL[i];

                                            gui::theme::set_theme(settings_app.theme);

                                            swatch_hit = true;

                                            break;

                                        }

                                    }

                                    if swatch_hit {

                                        // Tema paletinden dogrudan secim yapildi.

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::theme_btn_rect(wx, wy, ww)) {

                                        settings_app.cycle_theme();

                                    } else {

                                    let mut handled = false;
                                    for i in 0..3 {
                                        if gui::in_rect(mx,my,apps::settings::SystemSettings::zoom_btn_rect(wx,wy,i)) {
                                            settings_app.zoom = [100,125,150][i];
                                            gui::set_font_zoom(settings_app.zoom);
                                            settings_app.status_msg = gui::ui_text("Yazı yakınlaştırması değiştirildi.", "Font zoom changed.");
                                            handled = true;
                                            break;
                                        }
                                    }

                                    if !handled {
                                        for i in 0..4 {
                                            if gui::in_rect(mx,my,apps::settings::SystemSettings::resolution_btn_rect(wx,wy,i)) {
                                                let requested = match i { 0 => (800,600), 1 => (1024,768), 2 => (1280,720), _ => (1920,1080) };
                                                if requested == (fb.width, fb.height) {
                                                    settings_app.screen_res = requested;
                                                    settings_app.display_res = requested;
                                                    settings_app.status_msg = gui::ui_text("Bu çözünürlük zaten etkin.", "This resolution is already active.");
                                                } else if drivers::gpu::mode_supported(requested.0, requested.1) {
                                                    let (switched, pitch) = drivers::gpu::try_set_resolution(requested.0, requested.1);
                                                    if switched {
                                                        let p = pitch.unwrap_or(requested.0 * 4);
                                                        let new_bytes = p.saturating_mul(requested.1);
                                                        let fb_phys = (fb_ptr as u64).wrapping_sub(drivers::phys_to_virt(0));
                                                        for off in (0..new_bytes.max(16 * 1024 * 1024)).step_by(2 * 1024 * 1024) {
                                                            arch::x86_64::mmio_map_2mb((fb_phys + off as u64) & !0x1F_FFFF);
                                                        }
                                                        if fb.resize(requested.0, requested.1, p) {
                                                            settings_app.screen_res = requested;
                                                            settings_app.display_res = requested;
                                                            settings_app.status_msg = gui::ui_text("Çözünürlük değiştirildi (BGA).", "Resolution changed (BGA).");
                                                        } else {
                                                            settings_app.status_msg = gui::ui_text("Çözünürlük uygulanamadı (bellek sınırı).", "Resolution failed (memory limit).");
                                                        }
                                                    } else {
                                                        settings_app.screen_res = (fb.width, fb.height);
                                                        settings_app.status_msg = gui::ui_text("Çözünürlük açılışta Limine tarafından belirlenir.", "Resolution is set by Limine at boot.");
                                                    }
                                                }
                                                handled = true;
                                                break;
                                            }
                                        }
                                    }

                                    if !handled && gui::in_rect(mx,my,apps::settings::SystemSettings::night_light_rect(wx,wy)) {

                                        settings_app.night_light = !settings_app.night_light;

                                        settings_app.status_msg = if settings_app.night_light {

                                            gui::ui_text("Mavi ışık filtresi açıldı.", "Blue light filter enabled.")

                                        } else { gui::ui_text("Mavi ışık filtresi kapatıldı.", "Blue light filter disabled.") };

                                    }

                                    let nl_track = apps::settings::SystemSettings::night_light_track_rect(wx,wy);

                                    if gui::in_rect(mx,my,nl_track) {

                                        let span=nl_track.2.saturating_sub(1).max(1);

                                        settings_app.night_light_level=((mx-nl_track.0).min(span)*100/span).min(100) as u8;

                                        settings_app.night_light=true;

                                    }

                                    for i in 0..apps::settings::SystemSettings::wallpaper_count() {

                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::wallpaper_btn_rect(wx, wy, i)) {

                                            settings_app.wallpaper = i as u8;

                                            crate::gui::wallpaper::apply(settings_app.wallpaper, settings_app.wall_color);

                                            settings_app.status_msg = gui::ui_text("Arka plan değiştirildi.", "Background changed.");

                                        }

                                    }

                                    } // tema else kapatma

                                }

                                if settings_app.active_tab == 0 {

                                    let speed_track = apps::settings::SystemSettings::mouse_speed_track_rect(wx, wy);

                                    let mut mouse_setting_changed = true;

                                    if gui::in_rect(mx, my, speed_track) {

                                        let span = speed_track.2.saturating_sub(1).max(1);

                                        settings_app.mouse_speed = (1 + (mx.saturating_sub(speed_track.0).min(span) * 9 / span)) as u8;

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::double_click_dec_rect(wx, wy)) {

                                        settings_app.double_click_ms = settings_app.double_click_ms.saturating_sub(50).max(150);

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::double_click_inc_rect(wx, wy)) {

                                        settings_app.double_click_ms = settings_app.double_click_ms.saturating_add(50).min(600);

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::reverse_scroll_rect(wx, wy)) {

                                        settings_app.reverse_scroll = !settings_app.reverse_scroll;

                                    } else {

                                        mouse_setting_changed = false;

                                    }

                                    if mouse_setting_changed {

                                        settings_app.status_msg = gui::ui_text("Fare ayarları değiştirildi. Kalıcı olması için Kaydet'e tıklayın.", "Mouse settings changed. Click Save to keep them.");

                                    }

                                }

                                if settings_app.active_tab == 4 {

                                    for i in 0..apps::settings::SystemSettings::test_screen_btn_count() {

                                        if gui::in_rect(mx, my, apps::settings::SystemSettings::test_screen_btn_rect(wx, wy, i)) {

                                            settings_app.test_screen_pattern = i as u8;

                                        }

                                    }

                                    if gui::in_rect(mx, my, apps::settings::SystemSettings::test_ram_btn_rect(wx, wy)) {

                                        settings_app.run_ram_test();

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::test_disk_btn_rect(wx, wy)) {

                                        with_busy_progress(&mut fb, mx, my, gui::ui_text("Disk testi çalışıyor...", "Running disk test..."), || settings_app.run_disk_test());

                                    } else if gui::in_rect(mx, my, apps::settings::SystemSettings::test_audio_btn_rect(wx, wy)) {

                                        settings_app.run_audio_test(&mut audio_dev);

                                    }

                                }

                            }

                            2 => {

                                // Not Defteri: Dosya/Düzen/Yardım araç çubuğu ve menü aksiyonları

                                // Tıklama testleri, menü çizimiyle aynı geometri

                                // yardımcılarını (apps::notepad) kullanır; böylece font

                                // ölçeği 100'den farklıyken de koordinatlar örtüşür.

                                let menu_tb_y = wy + 30;

                                let menu_tb_h = crate::gui::sp(24);

                                let tb_hit = my >= menu_tb_y && my < menu_tb_y + menu_tb_h;

                                let tab1 = crate::apps::notepad::NotepadApp::menu_tab_rect(1);

                                let tab2 = crate::apps::notepad::NotepadApp::menu_tab_rect(2);

                                let tab3 = crate::apps::notepad::NotepadApp::menu_tab_rect(3);



                                if tb_hit && gui::in_rect(mx, my, (wx + tab1.0, menu_tb_y, tab1.1, menu_tb_h)) {

                                    notepad_app.menu = if notepad_app.menu == 1 { 0 } else { 1 };

                                } else if tb_hit && gui::in_rect(mx, my, (wx + tab2.0, menu_tb_y, tab2.1, menu_tb_h)) {

                                    notepad_app.menu = if notepad_app.menu == 2 { 0 } else { 2 };

                                } else if tb_hit && gui::in_rect(mx, my, (wx + tab3.0, menu_tb_y, tab3.1, menu_tb_h)) {

                                    notepad_app.menu = if notepad_app.menu == 3 { 0 } else { 3 };

                                } else if notepad_app.menu != 0

                                    && gui::in_rect(mx, my, (wx + crate::apps::notepad::NotepadApp::menu_box_x(notepad_app.menu), wy + crate::apps::notepad::NotepadApp::menu_box_y(), crate::apps::notepad::NotepadApp::menu_box_w(), crate::apps::notepad::NotepadApp::menu_box_h(notepad_app.menu)))

                                {

                                    let rel_y = my.saturating_sub(wy + crate::apps::notepad::NotepadApp::menu_box_y() + crate::apps::notepad::NotepadApp::menu_pad());

                                    let item_idx = rel_y / crate::apps::notepad::NotepadApp::menu_item_h();

                                    let items = crate::apps::notepad::NotepadApp::menu_items(notepad_app.menu);

                                    if let Some(def) = items.get(item_idx) {

                                        match def.action {

                                            crate::apps::notepad::NotepadMenuAction::Open => {

                                                let path = alloc::string::String::from(notepad_app.file_path_str());

                                                let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                                if let Some(data) = data {

                                                    notepad_app.open_file(&path, data);

                                                }

                                            }

                                            crate::apps::notepad::NotepadMenuAction::Save => {

                                                if with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya kaydediliyor...", "Saving file..."), || notepad_app.save_file(&mut fs_mgr.vfs)) {

                                                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                                }

                                            }

                                            crate::apps::notepad::NotepadMenuAction::About => {

                                                notepad_app.execute_action(def.action, &mut fs_mgr.vfs);

                                                open_app(&mut compositor, 1);

                                                settings_app.active_tab = 3;

                                            }

                                            _ => {

                                                notepad_app.execute_action(def.action, &mut fs_mgr.vfs);

                                            }

                                        }

                                    }

                                    notepad_app.menu = 0;

                                } else if notepad_app.menu != 0 {

                                    // Menü açıkken menü dışına tıklandığında menüyü kapat

                                    notepad_app.menu = 0;

                                } else if mx >= wx + ww.saturating_sub(130) && mx <= wx + ww.saturating_sub(74) && my >= wy + 32 && my <= wy + 52 {

                                    notepad_app.clear();

                                } else if mx >= wx + ww.saturating_sub(68) && mx <= wx + ww.saturating_sub(12) && my >= wy + 32 && my <= wy + 52 {

                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya kaydediliyor...", "Saving file..."), || notepad_app.save_file(&mut fs_mgr.vfs)) {

                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                    }

                                } else if gui::in_rect(mx, my, (wx + 6, wy + 58, ww.saturating_sub(12), wh.saturating_sub(90))) {

                                    let at = notepad_app.index_at_scrolled(mx - wx, my - wy, ww, compositor.windows[idx].map(|w| w.scroll.y).unwrap_or(0));

                                    notepad_app.cursor = at;

                                    if keyboard.shift {

                                        let anchor = notepad_app.selection_anchor.unwrap_or(at);

                                        if anchor == at {

                                            notepad_app.selection_start = None;

                                            notepad_app.selection_end = None;

                                        } else {

                                            notepad_app.selection_start = Some(anchor.min(at));

                                            notepad_app.selection_end = Some(anchor.max(at));

                                        }

                                    } else {

                                        notepad_app.selection_anchor = Some(at);

                                        notepad_app.selection_start = None;

                                        notepad_app.selection_end = None;

                                    }

                                    notepad_app.menu = 0;

                                }

                            }

                            3 => {

                                if gui::in_rect(mx, my, (wx + 12, wy + 32, 22, 20)) {

                                    browser_app.back();

                                } else if gui::in_rect(mx, my, (wx + 38, wy + 32, 22, 20)) {

                                    browser_app.forward();

                                } else if gui::in_rect(mx, my, (wx + 64, wy + 32, 22, 20)) {

                                    browser_app.navigate();

                                } else if gui::in_rect(mx, my, (wx + ww - 124, wy + 32, 58, 20)) {

                                    browser_app.show_history();

                                } else if gui::in_rect(mx, my, (wx + 94, wy + 32, ww.saturating_sub(224), 20)) {

                                    browser_app.focus_address_at(mx, wx + 100);

                                } else if gui::in_rect(mx, my, (wx + ww - 60, wy + 32, 50, 20)) {

                                    browser_app.navigate();

                                } else if let Some(url) = browser_app.click_bookmark_at(mx, my, wx, wy, ww) {

                                    browser_app.set_url(url.as_bytes());

                                    browser_app.navigate();

                                } else if let Some(url) = browser_app.click_link_at(mx, my) {

                                    // Sayfa içindeki bağlantıya tıklandı: geçmişe kaydet ve aç.

                                    browser_app.set_url(url.as_bytes());

                                    browser_app.navigate();

                                } else {

                                    browser_app.address_focused = false;

                                }

                            }

                            4 => {

                                // Resim: yakınlaştır / uzaklaştır

                                if my >= wy + 32 && my <= wy + 52 {

                                    if mx >= wx + ww - 180 && mx <= wx + ww - 130 {

                                        imageviewer_app.zoom_in();

                                    } else if mx >= wx + ww - 124 && mx <= wx + ww - 74 {

                                        imageviewer_app.zoom_out();

                                    }

                                }

                            }

                            5 => {

                                // Müzik çalar: önceki / oynat / sonraki

                                let ctrl_y = wy + 180;

                                if my >= ctrl_y && my <= ctrl_y + 24 {

                                    if mx >= wx + 10 && mx <= wx + 46 {

                                        musicplayer_app.current_track = musicplayer_app.current_track.saturating_sub(1);

                                        musicplayer_app.progress = 0;

                                    } else if mx >= wx + 52 && mx <= wx + 98 {

                                        musicplayer_app.is_playing = !musicplayer_app.is_playing;

                                    } else if mx >= wx + 104 && mx <= wx + 140 {

                                        musicplayer_app.current_track = (musicplayer_app.current_track + 1) % 5;

                                        musicplayer_app.progress = 0;

                                    } else if mx >= wx + 146 && mx < wx + 230 {

                                        let player_volume = (((mx - (wx + 146)) * 100) / 84).min(100) as u8;

                                        settings_app.volume = player_volume;

                                        musicplayer_app.volume = player_volume as usize;

                                    }

                                } else if mx >= wx + 250 && mx < wx + ww - 8 && my >= wy + 96 && my < wy + 194 {

                                    musicplayer_app.current_track = ((my - (wy + 96)) / 18).min(4);

                                    musicplayer_app.progress = 0;

                                    musicplayer_app.is_playing = true;

                                } else if my >= wy + 156 && my <= wy + 170 && mx >= wx + 54 && mx <= wx + 190 {

                                    musicplayer_app.progress = (((mx - (wx + 54)) * 100) / 136).min(100);

                                }

                            }

                            6 => {

                                // Video: oynat / durdur

                                let ctrl_y = wy + 178;

                                if my >= ctrl_y && my <= ctrl_y + 24 {

                                    if mx >= wx + 12 && mx <= wx + 54 {

                                        videoplayer_app.is_playing = !videoplayer_app.is_playing;

                                    } else if mx >= wx + 58 && mx <= wx + 98 {

                                        videoplayer_app.is_playing = false;

                                        videoplayer_app.frame = 0;

                                    }

                                }

                            }

                            7 => {

                                if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::applications_tab_rect(wx, wy)) {

                                    taskmgr_app.active_tab = 0;

                                    taskmgr_app.focused_control = 0;

                                    taskmgr_app.launch_menu_open = false;

                                } else if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::performance_tab_rect(wx, wy)) {

                                    taskmgr_app.active_tab = 1;

                                    taskmgr_app.focused_control = 1;

                                    taskmgr_app.launch_menu_open = false;

                                } else if taskmgr_app.launch_menu_open {

                                    if let Some(item) = apps::taskmgr::TaskManagerApp::launch_item_at(mx, my, wx, wy, ww) {

                                        taskmgr_app.launch_selected = item;

                                        taskmgr_app.focused_control = 2;

                                        let target = taskmgr_app.launch_app_id();

                                        taskmgr_app.launch_menu_open = false;

                                        open_app(&mut compositor, target);

                                    } else {

                                        taskmgr_app.launch_menu_open = false;

                                    }

                                } else if taskmgr_app.active_tab == 0 {

                                    if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::launch_button_rect(wx, wy, ww)) {

                                        taskmgr_app.focused_control = 2;

                                        taskmgr_app.launch_menu_open = true;

                                        taskmgr_app.launch_selected = 0;

                                    } else if gui::in_rect(mx, my, apps::taskmgr::TaskManagerApp::kill_btn_rect(wx, wy, ww)) {

                                        taskmgr_app.focused_control = 3;

                                        if let Some(target) = taskmgr_app.selected_app_idx() {

                                            taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");

                                            terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,

                                                &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,

                                                &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[idx],

                                                &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

                                        }

                                    } else if let Some(i) = taskmgr_app.row_at(mx, my, wx, wy, ww) {

                                        taskmgr_app.selected = i;

                                        taskmgr_app.focused_control = 4 + i;

                                    }

                                }

                            }

                            8 => { if let Some(key) = apps::calculator::CalculatorApp::key_at(mx, my, wx, wy, ww) { calculator_app.press(key); } }

                            9 => {

                                // Tüm dikdörtgenler `PaintApp::*_rect` yardımcılarından gelir;

                                // çizim (`render_content`) ile tıklama testi birebir örtüşür.

                                let r = apps::paint::PaintApp::open_btn_rect(ww);

                                let open_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::save_btn_rect(ww);

                                let save_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::clear_btn_rect(ww);

                                let clear_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::eraser_rect();

                                let eraser_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::brush_minus_rect();

                                let brush_minus_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::brush_plus_rect();

                                let brush_plus_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::zoom_minus_rect(ww);

                                let zoom_minus_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let r = apps::paint::PaintApp::zoom_plus_rect(ww);

                                let zoom_plus_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                let file_btn = apps::paint::PaintApp::file_menu_rect(ww);

                                let file_btn_rect = (wx + file_btn.0, wy + file_btn.1, file_btn.2, file_btn.3);

                                // Açılır menü öğesi seçiliyse önce onu işle ve menüyü kapat.

                                let mut menu_handled = false;

                                if paint_apps[idx].as_ref().map_or(false, |p| p.file_menu_open) {

                                    for item in 0..apps::paint::PaintApp::FILE_MENU_COUNT {

                                        let r = apps::paint::PaintApp::file_menu_item_rect(ww, item);

                                        let item_rect = (wx + r.0, wy + r.1, r.2, r.3);

                                        if gui::in_rect(mx, my, item_rect) {

                                            if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                            paint_apps[idx].as_mut().unwrap().file_menu_open = false;

                                            let (_, _, cw, ch) = apps::paint::PaintApp::canvas_rect(ww, wh);

                                            match item {

                                                0 => paint_apps[idx].as_mut().unwrap().new_document(),

                                                1 => { with_busy_progress(&mut fb, mx, my, gui::ui_text("BMP açılıyor...", "Opening BMP..."), || paint_apps[idx].as_mut().unwrap().open_bmp(&fs_mgr.vfs)); }

                                                2 => {

                                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Çizim kaydediliyor...", "Saving drawing..."), || paint_apps[idx].as_mut().unwrap().save_bmp(&mut fs_mgr.vfs, cw, ch)) {

                                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                                    }

                                                }

                                                _ => {

                                                    // Farklı Kaydet: dosya adını türetip yeni dosyaya yaz.

                                                    let name = {

                                                        let cur = paint_apps[idx].as_ref().unwrap().current_name();

                                                        apps::paint::PaintApp::derive_save_as_name(cur)

                                                    };

                                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Farklı kaydediliyor...", "Saving as..."), || paint_apps[idx].as_mut().unwrap().save_as(&mut fs_mgr.vfs, cw, ch, name.as_str())) {

                                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                                    }

                                                }

                                            }

                                            menu_handled = true;

                                            break;

                                        }

                                    }

                                    // Menü açıkken dışına tıklandıysa sadece menüyü kapat.

                                    if !menu_handled {

                                        paint_apps[idx].as_mut().unwrap().file_menu_open = false;

                                        menu_handled = true;

                                    }

                                }

                                if menu_handled {

                                    // Menü işlemi tamamlandı; diğer isabet testleri atlanır.

                                } else if gui::in_rect(mx, my, file_btn_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let p = paint_apps[idx].as_mut().unwrap();

                                    p.file_menu_open = !p.file_menu_open;

                                } else if gui::in_rect(mx, my, open_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    with_busy_progress(&mut fb, mx, my, gui::ui_text("BMP açılıyor...", "Opening BMP..."), || paint_apps[idx].as_mut().unwrap().open_bmp(&fs_mgr.vfs));

                                } else if gui::in_rect(mx, my, save_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let (_, _, cw, ch) = apps::paint::PaintApp::canvas_rect(ww, wh);

                                    if with_busy_progress(&mut fb, mx, my, gui::ui_text("Çizim kaydediliyor...", "Saving drawing..."), || paint_apps[idx].as_mut().unwrap().save_bmp(&mut fs_mgr.vfs, cw, ch)) {

                                        filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                    }

                                } else if gui::in_rect(mx, my, clear_rect) {

                                    if let Some(paint) = paint_apps[idx].as_mut() { paint.clear(); }

                                } else if gui::in_rect(mx, my, eraser_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().eraser = true;

                                } else if gui::in_rect(mx, my, brush_minus_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().adjust_brush(-1);

                                } else if gui::in_rect(mx, my, brush_plus_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().adjust_brush(1);

                                } else if gui::in_rect(mx, my, zoom_minus_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().adjust_zoom(-1);

                                } else if gui::in_rect(mx, my, zoom_plus_rect) {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    paint_apps[idx].as_mut().unwrap().adjust_zoom(1);

                                } else if {

                                    // Renk kutularından birine basıldı mı?

                                    let mut hit: Option<usize> = None;

                                    for i in 0..4 {

                                        let r = apps::paint::PaintApp::swatch_rect(i);

                                        if gui::in_rect(mx, my, (wx + r.0, wy + r.1, r.2, r.3)) { hit = Some(i); break; }

                                    }

                                    hit.is_some()

                                } {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let paint = paint_apps[idx].as_mut().unwrap();

                                    for i in 0..4 {

                                        let r = apps::paint::PaintApp::swatch_rect(i);

                                        if gui::in_rect(mx, my, (wx + r.0, wy + r.1, r.2, r.3)) {

                                            paint.color = apps::paint::PALETTE[i];

                                            paint.eraser = false;

                                            break;

                                        }

                                    }

                                } else {

                                    if paint_apps[idx].is_none() { paint_apps[idx] = Some(apps::paint::PaintApp::new()); }

                                    let paint = paint_apps[idx].as_mut().unwrap();

                                    if let Some((cxp, cyp)) = paint.to_canvas(ww, wh, mx - wx, my - wy) {

                                    // İlk tıklama noktası (last=None iken tek nokta başlangıcı).

                                    paint.paint(cxp, cyp);

                                    } else {

                                    // Tuval dışına tıklandı: kalem yolunu bitir.

                                    paint.end_stroke();

                                    }

                                }

                            }

                            11 => {

                                // minOs Kurulum Sihirbazı

                                let sel_rect = apps::installer::InstallerApp::drive_select_rect(wx, wy, ww);

                                let radio_fat32 = apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 0);

                                let radio_ext4 = apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 1);

                                let radio_exfat = apps::installer::InstallerApp::radio_btn_rect(wx, wy + 20, 2);

                                let fmt_rect = apps::installer::InstallerApp::format_btn_rect(wx, wy, ww, wh);

                                let btn_rect = apps::installer::InstallerApp::install_btn_rect(wx, wy, ww, wh);

                                if gui::in_rect(mx, my, sel_rect) {

                                    installer_app.select_next_drive();

                                } else if gui::in_rect(mx, my, radio_fat32) {

                                    installer_app.set_format_type(apps::installer::FormatFsType::Fat32);

                                } else if gui::in_rect(mx, my, radio_ext4) {

                                    installer_app.set_format_type(apps::installer::FormatFsType::Ext4);

                                } else if gui::in_rect(mx, my, radio_exfat) {

                                    installer_app.set_format_type(apps::installer::FormatFsType::Exfat);

                                } else if gui::in_rect(mx, my, fmt_rect) {

                                    // 2. Adım: Sadece biçimlendir.

                                    if installer_app.state != apps::installer::InstallState::Formatting

                                        && installer_app.state != apps::installer::InstallState::Copying

                                    {

                                        let formatted = with_busy_progress(&mut fb, mx, my, gui::ui_text("Disk biçimlendiriliyor...", "Formatting disk..."), || installer_app.format_selected_drive());

                                        if formatted {

                                            filemanager_app.set_path("/media/hdd0");

                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                        }

                                    }

                                } else if gui::in_rect(mx, my, btn_rect) {

                                    // 3. Adım: Onaylı tek tıkla kurulum.

                                    if installer_app.state != apps::installer::InstallState::Formatting

                                        && installer_app.state != apps::installer::InstallState::Copying

                                    {

                                        if installer_app.disk_formatted {

                                            let installed = with_busy_progress(&mut fb, mx, my, gui::ui_text("minOs kuruluyor...", "Installing minOs..."), || installer_app.perform_install(&mut fs_mgr.vfs));

                                            if installed { filemanager_app.set_path("/media/hdd0"); }

                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                        } else if installer_app.confirm_install {

                                            // Onay verildi: biçimlendirip kur.

                                            let installed = with_busy_progress(&mut fb, mx, my, gui::ui_text("minOs kuruluyor...", "Installing minOs..."), || installer_app.perform_install(&mut fs_mgr.vfs));

                                            if installed { filemanager_app.set_path("/media/hdd0"); }

                                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                                        } else {

                                            // İlk tıklama: onay iste (diski biçimlendirecek).

                                            installer_app.confirm_install = true;

                                            installer_app.status_msg = (

                                                "UYARI: Disk silinip biçimlendirilecek. Onaylamak için tekrar tıklayın.",

                                                "WARNING: Disk will be erased and formatted. Click again to confirm.",

                                            );

                                        }

                                    }

                                }

                            }

                            12 => {

                                viewer3d_app.handle_mouse_down(mx.saturating_sub(wx), my.saturating_sub(wy), ww, wh);

                            }

                            13 => {

                                pkgmgr_app.handle_click(mx.saturating_sub(wx), my.saturating_sub(wy), ww, wh, &mut fs_mgr.vfs);

                            }

                            _ => {}

                        }

                        break;

                    }

                }

            }

        }



        // 3. Klavye kısayolları

        let mut enter_char_consumed = false;

        while let Some(action) = keyboard.take_last_action() {

            match action {

                drivers::keyboard::KeyAction::ToggleStartMenu => {

                    compositor.is_start_menu_open = !compositor.is_start_menu_open;

                    if compositor.is_start_menu_open {

                        compositor.active_start_group = 0;

                        compositor.clear_start_search();

                    } else {

                        compositor.close_start_menu();

                    }

                }

                drivers::keyboard::KeyAction::CloseActiveWindow => {

                    let active_window = compositor.active_window_idx.filter(|&idx| {

                        compositor.windows[idx].map_or(false, |w| w.is_open && !w.is_minimized)

                    });

                    if let Some(idx) = active_window {

                        terminate_app(idx, &mut compositor, &mut filemanager_app, &mut settings_app,

                            &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,

                            &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[idx],

                            &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

                    } else {

                        // Masaüstündeyken Alt+F4 güç menüsünü açar.

                        compositor.is_start_menu_open = true;

                        compositor.active_start_group = 3;

                        compositor.selected_start_item = 0;

                        compositor.confirm_shutdown = false;

                        compositor.confirm_restart = false;

                        compositor.is_context_menu_open = false;

                        compositor.is_calendar_open = false;

                        compositor.is_volume_popup_open = false;

                    }

                }

                drivers::keyboard::KeyAction::CycleWindowFocus => {

                    compositor.is_alt_tab_open = true;

                    compositor.cycle_focus(keyboard.shift);

                }

                drivers::keyboard::KeyAction::WinShowDesktop => {

                    let show_desktop = compositor.windows.iter().flatten().any(|w| w.is_open && !w.is_minimized);

                    for slot in compositor.windows.iter_mut() {

                        if let Some(w) = slot { if w.is_open { w.is_minimized = show_desktop; w.is_focused = false; } }

                    }

                    if show_desktop { compositor.active_window_idx = None; }

                    else { compositor.focus_top_window(); }

                }

                drivers::keyboard::KeyAction::WinMinimizeAll => {

                    for slot in compositor.windows.iter_mut() {

                        if let Some(w) = slot { if w.is_open { w.is_minimized = true; w.is_focused = false; } }

                    }

                    compositor.active_window_idx = None;

                }

                drivers::keyboard::KeyAction::WinOpenFiles => {

                    open_app(&mut compositor, 0);

                    compositor.close_start_menu();

                }

                drivers::keyboard::KeyAction::RenameSelected => {

                    if app_is_focused(&compositor, 0) { filemanager_app.begin_rename(); }

                    else if desktop_files.selected_idx < desktop_files.file_count && desktop_files.selected_items[desktop_files.selected_idx] { desktop_files.begin_rename(); }

                }

                drivers::keyboard::KeyAction::VolumeUp => {

                    settings_app.volume = settings_app.volume.saturating_add(5).min(100);

                    settings_app.is_muted = false;

                    settings_app.status_msg = gui::ui_text("Ses artırıldı.", "Volume increased.");

                }

                drivers::keyboard::KeyAction::VolumeDown => {

                    settings_app.volume = settings_app.volume.saturating_sub(5);

                    settings_app.status_msg = gui::ui_text("Ses azaltıldı.", "Volume decreased.");

                }

                drivers::keyboard::KeyAction::VolumeMute => {

                    settings_app.is_muted = !settings_app.is_muted;

                    settings_app.status_msg = if settings_app.is_muted { gui::ui_text("Ses kapatıldı.", "Muted.") } else { gui::ui_text("Ses açıldı.", "Unmuted.") };

                }

                drivers::keyboard::KeyAction::MediaPlayPause => {

                    musicplayer_app.is_playing = !musicplayer_app.is_playing;

                }

                drivers::keyboard::KeyAction::MediaPrevious => {

                    musicplayer_app.current_track = musicplayer_app.current_track.saturating_sub(1);

                    musicplayer_app.progress = 0;

                }

                drivers::keyboard::KeyAction::MediaNext => {

                    musicplayer_app.current_track = (musicplayer_app.current_track + 1) % 5;

                    musicplayer_app.progress = 0;

                }

                drivers::keyboard::KeyAction::OpenTaskManager => {

                    open_app(&mut compositor, 7);

                    compositor.close_start_menu();

                }

                drivers::keyboard::KeyAction::ArrowUp => {

                    if compositor.is_start_menu_open {

                        if compositor.start_search_len > 0 {

                            compositor.move_start_search_selection(settings_app.language.is_english(), false);

                        } else if compositor.selected_start_item > 0 {

                            compositor.selected_start_item -= 1;

                        }

                    } else if compositor.is_context_menu_open {

                        // Sağ tık / Bağlam menüsü gezintisi

                        if compositor.context_file_idx.unwrap_or(0) > 0 {

                            compositor.context_file_idx = Some(compositor.context_file_idx.unwrap() - 1);

                        }

                    } else {

                        let np_focused = app_is_focused(&compositor, 2);

                        let terminal_focused = app_is_focused(&compositor, 10);

                        let tm_focused = app_is_focused(&compositor, 7);

                        let fm_focused = app_is_focused(&compositor, 0);

                        let set_focused = app_is_focused(&compositor, 1);



                        if terminal_focused {

                            terminal_app.history_up();

                        } else if tm_focused && taskmgr_app.launch_menu_open {

                            taskmgr_app.select_launch_prev();

                        } else if np_focused {

                            if notepad_app.menu == 0 {

                                notepad_app.move_vertical_ext(true, keyboard.shift);

                                notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                            }

                        } else if tm_focused && taskmgr_app.active_tab == 0 {

                            taskmgr_app.select_up();

                        } else if fm_focused {

                            let win_w = focused_window_for_app(&compositor, 0)

                                .map(|(_, w)| w.width).unwrap_or(0);

                            filemanager_app.move_selection_up(win_w);

                            fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                        } else if set_focused && settings_app.focused_control < 5 {

                            settings_app.select_tab_prev();

                        } else if app_is_focused(&compositor, 12) {

                            viewer3d_app.rot_x -= 0.1;

                        }

                    }

                }

                drivers::keyboard::KeyAction::ArrowDown => {

                    if compositor.is_start_menu_open {

                        if compositor.start_search_len > 0 {

                            compositor.move_start_search_selection(settings_app.language.is_english(), true);

                        } else {

                            let active_g = compositor.active_start_group;

                            let count = gui::MENU_GROUPS[active_g].2;

                            if compositor.selected_start_item + 1 < count {

                                compositor.selected_start_item += 1;

                            }

                        }

                    } else if compositor.is_context_menu_open {

                        let items_len = compositor.context_items().len();

                        let cur = compositor.context_file_idx.unwrap_or(0);

                        if cur + 1 < items_len {

                            compositor.context_file_idx = Some(cur + 1);

                        }

                    } else {

                        let np_focused = app_is_focused(&compositor, 2);

                        let terminal_focused = app_is_focused(&compositor, 10);

                        let tm_focused = app_is_focused(&compositor, 7);

                        let fm_focused = app_is_focused(&compositor, 0);

                        let set_focused = app_is_focused(&compositor, 1);



                        if terminal_focused {

                            terminal_app.history_down();

                        } else if tm_focused && taskmgr_app.launch_menu_open {

                            taskmgr_app.select_launch_next();

                        } else if np_focused {

                            if notepad_app.menu == 0 {

                                notepad_app.move_vertical_ext(false, keyboard.shift);

                                notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                            }

                        } else if tm_focused && taskmgr_app.active_tab == 0 {

                            taskmgr_app.select_down();

                        } else if fm_focused {

                            let win_w = focused_window_for_app(&compositor, 0)

                                .map(|(_, w)| w.width).unwrap_or(0);

                            filemanager_app.move_selection_down(win_w);

                            fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                        } else if set_focused && settings_app.focused_control < 5 {

                            settings_app.select_tab_next();

                        } else if app_is_focused(&compositor, 12) {

                            viewer3d_app.rot_x += 0.1;

                        }

                    }

                }

                drivers::keyboard::KeyAction::ArrowLeft => {

                    if desktop_files.rename_active {

                        desktop_files.rename_move_cursor(false);

                    } else if app_is_focused(&compositor, 0) && filemanager_app.rename_active {

                        filemanager_app.rename_move_cursor(false);

                    } else if compositor.is_start_menu_open {

                        compositor.active_start_group = compositor.active_start_group.saturating_sub(1);

                        compositor.selected_start_item = 0;

                    } else if app_is_focused(&compositor, 0) {

                        filemanager_app.move_selection_left();

                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {

                        if keyboard.ctrl {

                            browser_app.address_move_word_left_ext(keyboard.shift);

                        } else {

                            browser_app.address_move_left_ext(keyboard.shift);

                        }

                    } else if app_is_focused(&compositor, 2) {

                        if notepad_app.menu != 0 {

                            notepad_app.menu = match notepad_app.menu {

                                1 => 3,

                                2 => 1,

                                _ => 2,

                            };

                        } else {

                            if keyboard.ctrl {

                                notepad_app.move_word_left_ext(keyboard.shift);

                            } else {

                                notepad_app.move_left_ext(keyboard.shift);

                            }

                            notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                        }

                    } else if app_is_focused(&compositor, 7) && !taskmgr_app.launch_menu_open {

                        taskmgr_app.active_tab = 0;

                        taskmgr_app.focused_control = 0;

                    } else if app_is_focused(&compositor, 1) {

                        settings_app.select_tab_prev();

                    } else if app_is_focused(&compositor, 3) {

                        if browser_app.address_focused {

                            if keyboard.ctrl {

                                browser_app.address_move_word_left_ext(keyboard.shift);

                            } else {

                                browser_app.address_move_left_ext(keyboard.shift);

                            }

                        } else {

                            browser_app.focus_prev_control();

                        }

                    } else if app_is_focused(&compositor, 12) {

                        viewer3d_app.rot_y -= 0.1;

                    }

                }

                drivers::keyboard::KeyAction::ArrowRight => {

                    if desktop_files.rename_active {

                        desktop_files.rename_move_cursor(true);

                    } else if app_is_focused(&compositor, 0) && filemanager_app.rename_active {

                        filemanager_app.rename_move_cursor(true);

                    } else if compositor.is_start_menu_open {

                        if compositor.active_start_group + 1 < 4 {

                            compositor.active_start_group += 1;

                            compositor.selected_start_item = 0;

                        }

                    } else if app_is_focused(&compositor, 0) {

                        filemanager_app.move_selection_right();

                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {

                        if keyboard.ctrl {

                            browser_app.address_move_word_right_ext(keyboard.shift);

                        } else {

                            browser_app.address_move_right_ext(keyboard.shift);

                        }

                    } else if app_is_focused(&compositor, 2) {

                        if notepad_app.menu != 0 {

                            notepad_app.menu = match notepad_app.menu {

                                1 => 2,

                                2 => 3,

                                _ => 1,

                            };

                        } else {

                            if keyboard.ctrl {

                                notepad_app.move_word_right_ext(keyboard.shift);

                            } else {

                                notepad_app.move_right_ext(keyboard.shift);

                            }

                            notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                        }

                    } else if app_is_focused(&compositor, 12) {

                        viewer3d_app.rot_y += 0.1;

                    } else if app_is_focused(&compositor, 7) && !taskmgr_app.launch_menu_open {

                        taskmgr_app.active_tab = 1;

                        taskmgr_app.focused_control = 1;

                    } else if app_is_focused(&compositor, 1) {

                        settings_app.select_tab_next();

                    } else if app_is_focused(&compositor, 3) {

                        if browser_app.address_focused {

                            if keyboard.ctrl {

                                browser_app.address_move_word_right_ext(keyboard.shift);

                            } else {

                                browser_app.address_move_right_ext(keyboard.shift);

                            }

                        } else {

                            browser_app.focus_next_control();

                        }

                    }

                }

                drivers::keyboard::KeyAction::Properties => {

                    // Alt+Enter: seçili dosyanın özelliklerini durum çubuğunda göster.

                    if app_is_focused(&compositor, 0) {

                        if let Some(item) = filemanager_app.row(filemanager_app.selected_idx) {

                            let mut info = gui::TextBuf::new();

                            info.push_str(item.display_name());

                            info.push_str("  |  ");

                            info.push_str(item.file_type);

                            info.push_str("  |  ");

                            info.push_str(item.size_str.as_str());

                            filemanager_app.set_status_text(info.as_str());

                        }

                    }

                }

                drivers::keyboard::KeyAction::EnterKey => {

                    // Alt+Enter: Özellikler penceresini aç

                    if keyboard.alt && app_is_focused(&compositor, 0) && filemanager_app.selected_idx < filemanager_app.file_count {

                        let file_idx = filemanager_app.selected_idx;

                        let file = &filemanager_app.files[file_idx];

                        properties_name.clear();

                        properties_name.push_str(file.display_name());

                        properties_type.clear();

                        properties_type.push_str(file.file_type);

                        properties_size.clear();

                        properties_size.push_str(file.size_str.as_str());

                        properties_location.clear();

                        properties_location.push_str(filemanager_app.path());

                        properties_path.clear();

                        let full_path = filemanager_app.selected_full_path();

                        properties_path.push_str(&full_path);

                        properties_open = true;

                        enter_char_consumed = true;

                    }

                    if compositor.is_start_menu_open {

                        enter_char_consumed = true;

                        if compositor.start_search_len > 0 {

                            if let Some(app_idx) = compositor.selected_start_search_result(settings_app.language.is_english()) {

                                open_app(&mut compositor, app_idx);

                                compositor.close_start_menu();

                            }

                            continue;

                        }

                        let active_g = compositor.active_start_group;

                        let (_, first, count) = gui::MENU_GROUPS[active_g];

                        let item_k = compositor.selected_start_item.min(count.saturating_sub(1));

                        let i = first + item_k;

                        if i == 14 {

                            if compositor.confirm_shutdown {

                                #[cfg(minos_boot_log)]

    serial_print("[minOs] System Shutdown confirmed via Keyboard!\n");

                                unsafe {

                                    core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);

                                    core::arch::asm!("cli; hlt");

                                }

                            } else {

                                compositor.confirm_shutdown = true;

                            }

                        } else if i == 15 {

                            if compositor.confirm_restart {

                                restart_system();

                            }

                            compositor.confirm_restart = true;

                        } else {

                            if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(i) {

                                open_app(&mut compositor, app_idx);

                            }

                            compositor.close_start_menu();

                        }

                    } else if compositor.is_context_menu_open {

                        enter_char_consumed = true;

                        compositor.is_context_menu_open = false;

                    } else if app_is_focused(&compositor, 7) && taskmgr_app.launch_menu_open {

                        enter_char_consumed = true;

                        let target = taskmgr_app.launch_app_id();

                        taskmgr_app.launch_menu_open = false;

                        open_app(&mut compositor, target);

                    } else if app_is_focused(&compositor, 7) {

                        enter_char_consumed = true;

                        match taskmgr_app.focused_control {

                            0 => taskmgr_app.active_tab = 0,

                            1 => taskmgr_app.active_tab = 1,

                            2 => {

                                taskmgr_app.launch_menu_open = true;

                                taskmgr_app.launch_selected = 0;

                            }

                            3 => {

                                if let Some(target) = taskmgr_app.selected_app_idx() {

                                    taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");

                                    terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,

                                        &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,

                                        &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[target],

                                        &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

                                }

                            }

                            _ => {}

                        }

                    } else if app_is_focused(&compositor, 0) {

                        enter_char_consumed = true;

                        if filemanager_app.rename_active {

                            filemanager_app.commit_rename(&mut fs_mgr.vfs);

                        } else {

                        match filemanager_app.focused_control {

                            0 => filemanager_app.refresh_from_vfs(&fs_mgr.vfs),

                            1 => filemanager_app.new_file(&mut fs_mgr.vfs),

                            2 => filemanager_app.new_folder(&mut fs_mgr.vfs),

                            3 => { filemanager_app.delete_selected(&mut fs_mgr.vfs); },

                            _ => {

                                if filemanager_app.path() == crate::fs::vfs::TRASH_DIR {

                                    filemanager_app.restore_selected(&mut fs_mgr.vfs);

                                } else if let Some((tag, path)) = filemanager_app.enter_selected(&fs_mgr.vfs) {

                                    if tag == "TXT" || tag == "LOG" || tag == "CFG" {

                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                                        if let Some(data) = data {

                                            notepad_app.open_file(&path, data);

                                        }

                                        open_app(&mut compositor, 2);

                                    } else if tag == "IMG" {

                                        with_busy_progress(&mut fb, mx, my, gui::ui_text("Resim açılıyor...", "Opening image..."), || open_image_file(&path, &fs_mgr.vfs, &paint_apps, &mut imageviewer_app));

                                        open_app(&mut compositor, 4);

                                    } else if tag == "SND" {

                                        open_app(&mut compositor, 5);

                                    } else if tag == "VID" {

                                        open_app(&mut compositor, 6);

                                    } else if tag == "WEB" {

                                        open_app(&mut compositor, 3);

                                    } else if tag == "3D" {

                                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("3D model açılıyor...", "Opening 3D model..."), || fs_mgr.vfs.read_file(&path));

                                        if let Some(data) = data {

                                            viewer3d_app.load_file(&path, data);

                                        }

                                        open_app(&mut compositor, 12);

                                    }

                                }

                            }

                        }

                        }

                    } else if app_is_focused(&compositor, 1) {

                        enter_char_consumed = true;

                        if settings_app.focused_control < 5 {

                            settings_app.active_tab = settings_app.focused_control;

                        } else {

                            match settings_app.active_tab {

                                0 => match settings_app.focused_control {

                                    5 => {

                                        keyboard.toggle_layout();

                                        settings_app.keyboard_layout = keyboard.layout;

                                    }

                                    6 => {

                                        settings_app.language.toggle();

                                        settings_app.status_msg = gui::ui_text("Arayüz dili değiştirildi.", "Interface language changed.");

                                    }

                                    7 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    8 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); }

                                    _ => {}

                                },

                                1 => match settings_app.focused_control {

                                    5 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    6 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); },

                                    _ => {}

                                },

                                2 => match settings_app.focused_control {

                                    4 => {

                                        settings_app.dhcp_enabled = !settings_app.dhcp_enabled;

                                        settings_app.status_msg = if settings_app.dhcp_enabled { gui::ui_text("DHCP etkinleştirildi.", "DHCP enabled.") } else { gui::ui_text("Statik IP seçildi.", "Static IP selected.") };

                                    }

                                    5 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    6 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); },

                                    _ => {}

                                },

                                _ => match settings_app.focused_control {

                                    4 => settings_app.save_all(&compositor, &mut fs_mgr.vfs),

                                    5 => { settings_app.reset_defaults(); settings_app.apply_runtime(&mut *keyboard, &mut compositor, &mut now); settings_app.write_config(&mut fs_mgr.vfs); },

                                    _ => {}

                                },

                            }

                        }

                    } else if app_is_focused(&compositor, 3) {

                        enter_char_consumed = true;

                        match browser_app.focused_control {

                            0 => browser_app.back(),

                            1 => browser_app.forward(),

                            2 => browser_app.navigate(),

                            3 => browser_app.show_history(),

                            4 | 5 => browser_app.navigate(),

                            _ => {}

                        }

                    } else if app_is_focused(&compositor, 8) {

                        enter_char_consumed = true;

                        calculator_app.press('=');

                    } else if app_is_focused(&compositor, 10) {

                        enter_char_consumed = true;

                        terminal_app.submit();

                        if let Some(cmd) = terminal_app.take_pending() {

                            let n = terminal_app.pending_len();

                            let line = core::str::from_utf8(&cmd[..n]).unwrap_or("");

                            if let Some(app_idx) = terminal_app.execute(line, &mut fs_mgr.vfs, &mut memory_mgr.pmm, &now) {

                                open_app(&mut compositor, app_idx);

                            }

                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                        }

                    } else if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {

                        enter_char_consumed = true;

                        notepad_app.insert_char('\n');

                    } else if app_is_focused(&compositor, 2) {

                        enter_char_consumed = true;

                        notepad_app.menu = 0;

                    }

                }

                drivers::keyboard::KeyAction::ShutdownOS => {

                    #[cfg(minos_boot_log)]

    serial_print("[minOs] System Shutdown requested!\n");

                    unsafe {

                        core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16);

                        core::arch::asm!("cli; hlt");

                    }

                }

                drivers::keyboard::KeyAction::CopyText => {

                    if app_is_focused(&compositor, 2) { notepad_app.copy_selection(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_copy(); }

                    else if app_is_focused(&compositor, 0) { filemanager_app.copy_selection(&mut file_clipboard, false); }

                    else if desktop_files.selected_items.iter().any(|&s| s) || desktop_files.selected_idx < desktop_files.file_count { desktop_files.copy_selection(&mut file_clipboard, false); }

                    else if app_is_focused(&compositor, 8) { calculator_app.copy_result(); }

                    else if app_is_focused(&compositor, 10) { terminal_app.copy_input(); }

                }

                drivers::keyboard::KeyAction::PasteText => {

                    if app_is_focused(&compositor, 2) { notepad_app.paste_clipboard(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_paste(); }

                    else if app_is_focused(&compositor, 0) { filemanager_app.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); }

                    else if app_is_focused(&compositor, 8) { calculator_app.paste_expression(); }

                    else if app_is_focused(&compositor, 10) { terminal_app.paste_clipboard(); }

                    else { desktop_files.paste_selection(&mut file_clipboard, &mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); }

                }

                drivers::keyboard::KeyAction::CutText => {

                    if app_is_focused(&compositor, 2) { notepad_app.cut_selection(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_cut(); }

                    else if app_is_focused(&compositor, 0) { filemanager_app.copy_selection(&mut file_clipboard, true); }

                    else if desktop_files.selected_items.iter().any(|&s| s) || desktop_files.selected_idx < desktop_files.file_count { desktop_files.copy_selection(&mut file_clipboard, true); }

                    else if app_is_focused(&compositor, 8) { calculator_app.cut_expression(); }

                }

                drivers::keyboard::KeyAction::SelectAllText => {

                    let focused = app_is_focused(&compositor, 2);

                    if focused { notepad_app.select_all(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_select_all(); }

                }

                drivers::keyboard::KeyAction::Undo => {

                    let focused = app_is_focused(&compositor, 2);

                    if focused { notepad_app.undo(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_undo_edit(); }

                }

                drivers::keyboard::KeyAction::Redo => {

                    let focused = app_is_focused(&compositor, 2);

                    if focused { notepad_app.redo(); }

                    else if app_is_focused(&compositor, 3) && browser_app.address_focused { browser_app.address_redo_edit(); }

                }

                drivers::keyboard::KeyAction::SaveDocument => {

                    if app_is_focused(&compositor, 2) {

                        if with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya kaydediliyor...", "Saving file..."), || notepad_app.save_file(&mut fs_mgr.vfs)) {

                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                        }

                    }

                }

                drivers::keyboard::KeyAction::NewDocument => {

                    if app_is_focused(&compositor, 2) {

                        notepad_app.clear();

                        notepad_app.status_msg = gui::ui_text("Yeni belge oluşturuldu", "New document created");

                    }

                }

                drivers::keyboard::KeyAction::OpenDocument => {

                    if app_is_focused(&compositor, 2) {

                        let path = alloc::string::String::from(notepad_app.file_path_str());

                        let data = with_busy_progress(&mut fb, mx, my, gui::ui_text("Dosya açılıyor...", "Opening file..."), || fs_mgr.vfs.read_file(&path));

                        if let Some(data) = data {

                            notepad_app.open_file(&path, data);

                        }

                    }

                }

                drivers::keyboard::KeyAction::HomeKey => {

                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {

                        if keyboard.ctrl { notepad_app.move_document_home(keyboard.shift); }

                        else { notepad_app.move_home_ext(keyboard.shift); }

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {

                        browser_app.address_home_ext(keyboard.shift);

                    } else if app_is_focused(&compositor, 0) {

                        filemanager_app.select_first();

                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                    } else if app_is_focused(&compositor, 7) {

                        taskmgr_app.select_first();

                    } else if app_is_focused(&compositor, 10) {

                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {

                            let vl = terminal_view_lines(&w);

                            terminal_app.scroll_top(vl);

                        }

                    }

                }

                drivers::keyboard::KeyAction::EndKey => {

                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {

                        if keyboard.ctrl { notepad_app.move_document_end(keyboard.shift); }

                        else { notepad_app.move_end_ext(keyboard.shift); }

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {

                        browser_app.address_end_ext(keyboard.shift);

                    } else if app_is_focused(&compositor, 0) {

                        filemanager_app.select_last();

                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                    } else if app_is_focused(&compositor, 7) {

                        taskmgr_app.select_last();

                    } else if app_is_focused(&compositor, 10) {

                        terminal_app.scroll_bottom();

                    }

                }

                drivers::keyboard::KeyAction::PageUpKey => {

                    if app_is_focused(&compositor, 0) {

                        let rows = focused_window_for_app(&compositor, 0)

                            .map(|(_, w)| fm_page_rows(&w, &filemanager_app)).unwrap_or(8);

                        filemanager_app.select_page(false, rows);

                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                    } else if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {

                        let lines = focused_window_for_app(&compositor, 2).map(|(_, w)| notepad_app.page_lines(w.height)).unwrap_or(10);

                        notepad_app.move_page_ext(true, lines, keyboard.shift);

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    } else if app_is_focused(&compositor, 7) {

                        taskmgr_app.select_page(false, 8);

                    } else if app_is_focused(&compositor, 10) {

                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {

                            let vl = terminal_view_lines(&w);

                            terminal_app.scroll_output(vl as isize, vl);

                        }

                    }

                }

                drivers::keyboard::KeyAction::PageDownKey => {

                    if app_is_focused(&compositor, 0) {

                        let rows = focused_window_for_app(&compositor, 0)

                            .map(|(_, w)| fm_page_rows(&w, &filemanager_app)).unwrap_or(8);

                        filemanager_app.select_page(true, rows);

                        fm_ensure_selection_visible(&mut compositor, &filemanager_app);

                    } else if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {

                        let lines = focused_window_for_app(&compositor, 2).map(|(_, w)| notepad_app.page_lines(w.height)).unwrap_or(10);

                        notepad_app.move_page_ext(false, lines, keyboard.shift);

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    } else if app_is_focused(&compositor, 7) {

                        taskmgr_app.select_page(true, 8);

                    } else if app_is_focused(&compositor, 10) {

                        if let Some((_, w)) = focused_window_for_app(&compositor, 10) {

                            let vl = terminal_view_lines(&w);

                            terminal_app.scroll_output(-(vl as isize), vl);

                        }

                    }

                }

                drivers::keyboard::KeyAction::ToggleOverwrite => {

                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 { notepad_app.toggle_overwrite(); }

                }

                drivers::keyboard::KeyAction::Delete => {

                    // Görev Yöneticisi odaklıysa seçili süreci sonlandır.

                    let tm_focused = app_is_focused(&compositor, 7);

                    if tm_focused {

                        if let Some(target) = taskmgr_app.selected_app_idx() {

                            taskmgr_app.status = gui::ui_text("Uygulama sonlandırıldı.", "Application terminated.");

                            terminate_app(target, &mut compositor, &mut filemanager_app, &mut settings_app,

                                &mut notepad_app, &mut browser_app, &mut imageviewer_app, &mut musicplayer_app,

                                &mut videoplayer_app, &mut taskmgr_app, &mut calculator_app, &mut paint_apps[target],

                                &mut terminal_app, &mut installer_app, &mut viewer3d_app, &mut pkgmgr_app);

                        }

                    } else if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {

                        notepad_app.delete_action();

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {

                        browser_app.address_delete();

                    } else if app_is_focused(&compositor, 0) && !filemanager_app.rename_active {

                        filemanager_app.delete_selected(&mut fs_mgr.vfs);

                    } else if app_is_focused(&compositor, 0) && filemanager_app.rename_active {

                        filemanager_app.rename_delete();

                    } else if desktop_files.rename_active {

                        desktop_files.rename_delete();

                    } else if desktop_files.selected_items.iter().any(|&s| s) || (desktop_files.selected_idx < desktop_files.file_count && desktop_files.selected_items[desktop_files.selected_idx]) {

                        desktop_files.delete_selected(&mut fs_mgr.vfs);

                        desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                    }

                }

                drivers::keyboard::KeyAction::DeleteWordBack => {

                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {

                        notepad_app.delete_word_backward();

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {

                        browser_app.address_delete_word_back();

                    } else if app_is_focused(&compositor, 10) {

                        terminal_app.delete_word();

                    }

                }

                drivers::keyboard::KeyAction::DeleteWordForward => {

                    if app_is_focused(&compositor, 2) && notepad_app.menu == 0 {

                        notepad_app.delete_word_forward();

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    } else if app_is_focused(&compositor, 3) && browser_app.address_focused {

                        browser_app.address_delete_word_forward();

                    }

                }

                drivers::keyboard::KeyAction::Refresh => { /* TODO */ }

                drivers::keyboard::KeyAction::BrowserAddress => {

                    let focused = app_is_focused(&compositor, 3);

                    if focused { browser_app.focus_address_end(); }

                }

                drivers::keyboard::KeyAction::BrowserRefresh => {

                    let focused = app_is_focused(&compositor, 3);

                    if focused { browser_app.navigate(); }

                }

                drivers::keyboard::KeyAction::RunDialog => {

                    run_dialog_open = !run_dialog_open;

                    run_dialog_input.clear();

                    compositor.close_start_menu();

                }

                drivers::keyboard::KeyAction::Screenshot => {

                    // Ekran görüntüsü (F12 / PrtScn) -> BMP olarak VFS'ye kaydet

                    audio_dev.play_beep(980, 40);

                    let sw = width;

                    let sh = height;

                    let row_bytes = (sw * 3 + 3) & !3;

                    let image_size = row_bytes * sh;

                    let file_size = 54 + image_size;



                    let mut bmp_data = alloc::vec::Vec::new();

                    if bmp_data.try_reserve_exact(file_size).is_err() {

                        serial_print("[SCREENSHOT] HATA: Yetersiz bellek!\n");

                        terminal_app.push_line(gui::ui_text("Ekran görüntüsü alınamadı: Yetersiz bellek", "Screenshot failed: Out of memory"), gui::Color::ACCENT_RED);

                    } else {

                        // BMP Header (14 bytes)

                        bmp_data.extend_from_slice(b"BM");

                        bmp_data.extend_from_slice(&(file_size as u32).to_le_bytes());

                        bmp_data.extend_from_slice(&0u32.to_le_bytes());

                        bmp_data.extend_from_slice(&54u32.to_le_bytes());

                        // DIB Header (40 bytes)

                        bmp_data.extend_from_slice(&40u32.to_le_bytes());

                        bmp_data.extend_from_slice(&(sw as i32).to_le_bytes());

                        bmp_data.extend_from_slice(&(sh as i32).to_le_bytes());

                        bmp_data.extend_from_slice(&1u16.to_le_bytes());

                        bmp_data.extend_from_slice(&24u16.to_le_bytes());

                        bmp_data.extend_from_slice(&0u32.to_le_bytes());

                        bmp_data.extend_from_slice(&(image_size as u32).to_le_bytes());

                        bmp_data.extend_from_slice(&2835u32.to_le_bytes());

                        bmp_data.extend_from_slice(&2835u32.to_le_bytes());

                        bmp_data.extend_from_slice(&0u32.to_le_bytes());

                        bmp_data.extend_from_slice(&0u32.to_le_bytes());

                        // Pixel Data (Alt satırdan üst satıra BGR)

                        for y in (0..sh).rev() {

                            for x in 0..sw {

                                let p = fb.get_pixel(x, y);

                                let b = (p & 0xFF) as u8;

                                let g = ((p >> 8) & 0xFF) as u8;

                                let r = ((p >> 16) & 0xFF) as u8;

                                bmp_data.push(b);

                                bmp_data.push(g);

                                bmp_data.push(r);

                            }

                            for _ in 0..(row_bytes - sw * 3) {

                                bmp_data.push(0);

                            }

                        }

                        let mut path = alloc::string::String::from("/desktop/ekran_");

                        let mut num = gui::TextBuf::new();

                        num.push_usize(screenshot_counter);

                        path.push_str(num.as_str());

                        path.push_str(".bmp");

                        serial_print("[SCREENSHOT] Ekran goruntusu aliniyor...\n");

                        screenshot_counter += 1;

                        let arc_data: alloc::sync::Arc<[u8]> = bmp_data.into();

                        if fs_mgr.vfs.write_large_file(&path, arc_data) {

                            desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                            terminal_app.push_line(gui::ui_text("Ekran görüntüsü kaydedildi: /desktop/", "Screenshot saved to: /desktop/"), gui::Color::ACCENT_GREEN);

                            compositor.push_toast(

                                gui::ToastKind::Success,

                                gui::ui_text("Ekran Görüntüsü Alındı", "Screenshot Captured"),

                                gui::ui_text("Görüntü /desktop/ klasörüne kaydedildi.", "Image saved to /desktop/ folder."),

                                monotonic_ms(),

                                4000,

                            );

                            force_render = true;

                            serial_print("[SCREENSHOT] Basariyla kaydedildi: ");

                            serial_print(&path);

                            serial_print("\n");

                        } else {

                            compositor.push_toast(

                                gui::ToastKind::Error,

                                gui::ui_text("Ekran Görüntüsü Hatası", "Screenshot Error"),

                                gui::ui_text("Dosya diske yazılamadı!", "Failed to write file to disk!"),

                                monotonic_ms(),

                                4000,

                            );

                            serial_print("[SCREENSHOT] HATA: VFS dosya yazilamadi!\n");

                        }

                    }

                }

            }

        }



        // 4. Fare Tekerlek (Scroll)

        let scroll_val = mouse.take_scroll_delta();

        if scroll_val != 0 {

            // Ayarlar > Testler: tekerlek sınaması sayacı.

            if app_is_focused(&compositor, 1) && settings_app.active_tab == 4 {

                if let Some((_, w)) = focused_window_for_app(&compositor, 1) {

                    let pad = apps::settings::SystemSettings::test_mouse_pad_rect(w.x, w.y);

                    if gui::in_rect(mx, my, pad) {

                        settings_app.note_scroll_test(scroll_val.unsigned_abs() as u32);

                    }

                }

            }

            // Başlat menüsü açıkken scroll wheel desteği

            if compositor.is_start_menu_open {

                let active_g = compositor.active_start_group;

                let count = gui::MENU_GROUPS[active_g].2;

                if scroll_val > 0 {

                    compositor.selected_start_item = compositor.selected_start_item.saturating_sub(1);

                } else if compositor.selected_start_item + 1 < count {

                    compositor.selected_start_item += 1;

                }

            }

            // CMD geçmişi: yalnızca çıktı panelini kaydır; giriş satırı sabit kalır.

            if let Some((_, w)) = focused_window_for_app(&compositor, 10) {

                if w.is_open && w.is_focused && !w.is_minimized

                    && gui::in_rect(mouse.x as usize, mouse.y as usize, (w.x, w.y + gui::titlebar_h(), w.width, w.height.saturating_sub(gui::titlebar_h() + 28))) {

                    let view_lines = w.height.saturating_sub(86) / 13;

                    terminal_app.scroll_output(scroll_val as isize, view_lines);

                }

            }

            // İmlecin altındaki en üst pencerede içerik kaydırmasını güncelle.

            // Scrollbar aynı ScrollState üzerinden çizildiği için tutamaç da

            // tekerlekle birlikte anında hareket eder.

            let (window_order, window_count) = compositor.draw_order();

            for k in (0..window_count).rev() {

                let idx = window_order[k];

                let Some(w) = compositor.windows[idx] else { continue; };

                if !w.is_open || w.is_minimized || !gui::in_rect(mx, my, (w.x, w.y, w.width, w.height)) {

                    continue;

                }

                let app_id = w.app_id as usize;

                let (cx, cy, cw, ch) = w.content_rect();

                let c_h = window_content_h(app_id, w.height, w.width, &filemanager_app, &notepad_app, &browser_app);

                let c_w = cw;

                let over_content = gui::in_rect(mx, my, (cx, cy, cw, ch));

                if over_content && (c_h > ch || c_w > cw) {

                    let max_y = c_h.saturating_sub(ch);

                    let step = if app_id == 2 { gui::sp(16) } else { gui::sp(24) };

                    let delta = (scroll_val as isize) * step.max(1) as isize;

                    if let Some(wm) = compositor.windows[idx].as_mut() {

                        wm.scroll.y = ((wm.scroll.y as isize - delta).max(0) as usize).min(max_y);

                    }

                }

                break;

            }

            // Görüntüleyicide tekerlek zoom kısayoludur (yukarı yakınlaştırır).

            if let Some((_, w)) = focused_window_for_app(&compositor, 4) {

                let (cx, cy, cw, ch) = image_viewer_canvas_rect(&w);

                if gui::in_rect(mx, my, (cx, cy, cw, ch)) {

                    imageviewer_app.pan_y = imageviewer_app.pan_y.saturating_add((scroll_val as isize) * 24);

                }

            }

            // Çizim tuvalinde tekerlek yakınlaştırmayı ayarlar.

            if let Some((_, w)) = focused_window_for_app(&compositor, 12) {

                if gui::in_rect(mx, my, (w.x, w.y, w.width, w.height)) {

                    viewer3d_app.handle_scroll(scroll_val);

                }

            }

            if let Some((paint_idx, w)) = focused_window_for_app(&compositor, 9) {

                if let Some(paint) = paint_apps[paint_idx].as_mut() {

                    if paint.to_canvas(w.width, w.height, mx.saturating_sub(w.x), my.saturating_sub(w.y)).is_some() {

                        paint.adjust_zoom(scroll_val);

                    }

                }

            }

        }



        // 5. Pencere Sürükleme ve Yeniden Boyutlandırma

        if mouse.left_button {

            if let Some((_, w)) = focused_window_for_app(&compositor, 4) {

                let (cx, cy, cw, ch) = image_viewer_canvas_rect(&w);

                if w.is_focused && gui::in_rect(mx, my, (cx, cy, cw, ch)) {

                    if let Some((last_x, last_y)) = image_drag_last {

                        imageviewer_app.pan_x = imageviewer_app.pan_x.saturating_add(mx as isize - last_x as isize);

                        imageviewer_app.pan_y = imageviewer_app.pan_y.saturating_add(my as isize - last_y as isize);

                    }

                    image_drag_last = Some((mx, my));

                } else {

                    image_drag_last = None;

                }

            } else {

                image_drag_last = None;

            }

            if let Some(w) = compositor.windows[1] {

                if w.is_open && w.is_focused && !w.is_minimized {

                    if settings_app.active_tab == 0 {

                        let track = apps::settings::SystemSettings::volume_track_rect(w.x, w.y);

                        if gui::in_rect(mx, my, (track.0, track.1.saturating_sub(5), track.2, track.3 + 10)) {

                            settings_app.set_volume_from_range(mx, track.0, track.2);

                        }

                    } else if settings_app.active_tab == 1 {

                        let track = apps::settings::SystemSettings::night_light_track_rect(w.x, w.y);

                        if gui::in_rect(mx, my, (track.0, track.1.saturating_sub(5), track.2, track.3 + 10)) {

                            let span=track.2.saturating_sub(1).max(1);

                            settings_app.night_light_level = ((mx-track.0).min(span)*100/span).min(100) as u8;

                            settings_app.night_light = true;

                        }

                    }

                }

            }

            if let Some((_, w)) = focused_window_for_app(&compositor, 12) {

                if w.is_focused {

                    viewer3d_app.handle_mouse_move(mx.saturating_sub(w.x), my.saturating_sub(w.y));

                }

            }

            if let Some((paint_idx, w)) = focused_window_for_app(&compositor, 9) {

                if w.is_focused {

                    // Windows Paint davranışı: sol tuş basılı tutuldukça fare

                    // hareketi sürekli çizer. Tuval dışına çıkınca vuruşu bitir.

                    if paint_apps[paint_idx].is_none() { paint_apps[paint_idx] = Some(apps::paint::PaintApp::new()); }

                    let paint = paint_apps[paint_idx].as_mut().unwrap();

                    if let Some((cxp, cyp)) = paint.to_canvas(w.width, w.height, mx - w.x, my - w.y) {

                        paint.paint(cxp, cyp);

                    } else {

                        paint.end_stroke();

                    }

                }

            }

            if compositor.is_volume_popup_open {

                let track = compositor.volume_track_rect(&l);

                if gui::in_rect(mx, my, (track.0.saturating_sub(8), track.1.saturating_sub(8), track.2+16, track.3+16)) {

                    settings_app.set_volume_from_range(mx, track.0, track.2);

                }

            }

            if notepad_dragging && notepad_app.menu == 0 {

                if let Some((_, w)) = focused_window_for_app(&compositor, 2) {

                    let at = notepad_app.index_at_scrolled(mx - w.x, my - w.y, w.width, w.scroll.y);

                    notepad_app.cursor = at;

                    let anchor = notepad_app.selection_anchor.unwrap_or(at);

                    if anchor == at {

                        notepad_app.selection_start = None;

                        notepad_app.selection_end = None;

                    } else {

                        notepad_app.selection_start = Some(anchor.min(at));

                        notepad_app.selection_end = Some(anchor.max(at));

                    }

                }

            }

            let (drag_item, drag_start, drag_active) = if desktop_drag_source {

                (desktop_files.drag_item, desktop_files.drag_start, desktop_files.drag_active)

            } else { (filemanager_app.drag_item, filemanager_app.drag_start, filemanager_app.drag_active) };

            if let Some(src_idx) = drag_item {

                if !drag_active && (mx.abs_diff(drag_start.0) > 4 || my.abs_diff(drag_start.1) > 4) {

                    if desktop_drag_source {

                        desktop_files.drag_active = true;

                        if !desktop_files.selected_items[src_idx] { desktop_files.selected_items = [false; apps::filemanager::MAX_ITEMS]; desktop_files.selected_items[src_idx] = true; }

                    } else {

                        filemanager_app.drag_active = true;

                        filemanager_app.last_click_idx = None;

                        if !filemanager_app.selected_items[src_idx] { filemanager_app.selected_items = [false; apps::filemanager::MAX_ITEMS]; filemanager_app.selected_items[src_idx] = true; }

                    }

                }

                let fm_win_opt = compositor.windows.iter().flatten().find(|w| w.app_id == 0 && w.is_open && !w.is_minimized);

                if let Some(fm_win) = fm_win_opt {

                    let target = filemanager_app.drop_target_at(fm_win.x, fm_win.y, fm_win.width, fm_win.height, fm_win.scroll.y, mx, my);

                    if desktop_drag_source { desktop_files.drag_target = target; }

                    else { filemanager_app.drag_target = target; }

                }

            }

            if let Some((idx, grab, thumb_h, track_h, content_h, view_h)) = vscroll_drag {

                // Tutamacı fare ile sürükle → kaydırma ofseti.

                let track_top = compositor.windows[idx]

                    .map(|w| w.content_rect().1)

                    .unwrap_or(0);

                let max_thumb_travel = track_h.saturating_sub(thumb_h);

                let desired_top = my.saturating_sub(grab).max(track_top).min(track_top + max_thumb_travel);

                let ratio = if max_thumb_travel == 0 { 0.0 }

                    else { (desired_top - track_top) as f32 / max_thumb_travel as f32 };

                let max_off = content_h.saturating_sub(view_h);

                let new_off = ((ratio * max_off as f32) as usize).min(max_off);

                if let Some(wm) = compositor.windows[idx].as_mut() {

                    wm.scroll.y = new_off;

                }

            } else if let Some(idx) = dragging_win {

                if let Some(w) = compositor.windows[idx].as_mut() {

                    let max_x = width.saturating_sub(w.width);

                    let max_y = height.saturating_sub(w.height + gui::taskbar_h());

                    let prev_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);

                    w.x = mx.saturating_sub(drag_offset_x).min(max_x);

                    w.y = my.saturating_sub(drag_offset_y).min(max_y);



                    // Aero Snap kenar algılama & önizleme kutusu

                    let old_snap = active_snap_preview;

                    let usable_h = height.saturating_sub(gui::taskbar_h());

                    let half_w = width / 2;

                    if my <= 4 {

                        active_snap_preview = Some((0, 0, width, usable_h));

                    } else if mx <= 4 {

                        active_snap_preview = Some((0, 0, half_w, usable_h));

                    } else if mx >= width.saturating_sub(5) {

                        active_snap_preview = Some((half_w, 0, width.saturating_sub(half_w), usable_h));

                    } else {

                        active_snap_preview = None;

                    }



                    let new_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);

                    let mut dirty = match drag_prev_box {

                        Some(p) => drivers::video::rect_union(p, new_box),

                        None => drivers::video::rect_union(prev_box, new_box),

                    };

                    dirty = drivers::video::rect_union(dirty, (mx.saturating_sub(8), my.saturating_sub(8), 24, 24));

                    // Hem eski hem yeni Aero Snap önizlemesini kirli bölgeye kat.

                    if let Some(p) = old_snap { dirty = drivers::video::rect_union(dirty, p); }

                    if let Some(p) = active_snap_preview { dirty = drivers::video::rect_union(dirty, p); }

                    drag_dirty_rect = Some(dirty);

                    drag_prev_box = Some(new_box);

                }

            } else if let Some(idx) = resizing_win {

                let edge = resize_edge.unwrap_or(gui::ResizeEdge::BottomRight);

                if let Some(w) = compositor.windows[idx].as_mut() {

                    use gui::ResizeEdge::*;

                    let (ox, oy, ow, oh) = resize_orig;

                    let left = ox + ow;   // sol kenarın sabit sağ referansı

                    let right = ox;       // sağ kenarın sabit sol referansı

                    let top = oy + oh;    // üst kenarın sabit alt referansı

                    let bottom = oy;      // alt kenarın sabit üst referansı



                    let mut nx = w.x;

                    let mut ny = w.y;

                    let mut nw = w.width;

                    let mut nh = w.height;



                    // Sağ kenar sürükleniyor

                    if matches!(edge, Right | TopRight | BottomRight) {

                        nw = mx.saturating_sub(right).max(w.min_width)

                            .min(w.max_width.min(width.saturating_sub(w.x)));

                    }

                    // Sol kenar sürükleniyor

                    if matches!(edge, Left | TopLeft | BottomLeft) {

                        let desired = mx.min(left);

                        nw = left.saturating_sub(desired).max(w.min_width).min(w.max_width).min(left);

                        nx = left.saturating_sub(nw).min(width.saturating_sub(nw));

                    }

                    // Alt kenar sürükleniyor

                    if matches!(edge, Bottom | BottomLeft | BottomRight) {

                        let max_h = height.saturating_sub(gui::taskbar_h()).saturating_sub(w.y);

                        nh = my.saturating_sub(bottom).max(w.min_height).min(w.max_height).min(max_h);

                    }

                    // Üst kenar sürükleniyor

                    if matches!(edge, Top | TopLeft | TopRight) {

                        let desired = my.min(top);

                        nh = top.saturating_sub(desired).max(w.min_height).min(w.max_height).min(top);

                        ny = top.saturating_sub(nh);

                    }



                    let prev_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);

                    w.x = nx;

                    w.y = ny;

                    w.width = nw;

                    w.height = nh;

                    w.is_maximized = false;

                    let new_box = (w.x.saturating_sub(4), w.y.saturating_sub(4), w.width + 12, w.height + 12);

                    let mut dirty = match drag_prev_box {

                        Some(p) => drivers::video::rect_union(p, new_box),

                        None => drivers::video::rect_union(prev_box, new_box),

                    };

                    dirty = drivers::video::rect_union(dirty, (mx.saturating_sub(8), my.saturating_sub(8), 24, 24));

                    if let Some(p) = active_snap_preview { dirty = drivers::video::rect_union(dirty, p); }

                    drag_dirty_rect = Some(dirty);

                    drag_prev_box = Some(new_box);

                }

            }

        }

        if mouse_released { image_drag_last = None; }



        if mouse_released {

            drag_prev_box = None;

            drag_dirty_rect = None;

            viewer3d_app.handle_mouse_up();

            if let Some(paint_idx) = compositor.active_window_idx {

                if let Some(paint) = paint_apps[paint_idx].as_mut() { paint.end_stroke(); }

            }

            notepad_dragging = false;

            let (is_drag_active, drag_item, drag_target) = if desktop_drag_source {

                (desktop_files.drag_active, desktop_files.drag_item, desktop_files.drag_target)

            } else { (filemanager_app.drag_active, filemanager_app.drag_item, filemanager_app.drag_target) };

            if is_drag_active {

                if let Some(src_idx) = drag_item {

                    if let Some(target) = drag_target {

                        let src_app = if desktop_drag_source { &mut desktop_files } else { &mut filemanager_app };

                        match src_app.execute_drop(src_idx, target, keyboard.ctrl, &mut fs_mgr.vfs) {

                            Ok(_) => {

                                desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                                filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                            }

                            Err(err) => {

                                if desktop_drag_source { desktop_files.set_status_text(err); }

                                else { filemanager_app.set_status_text(err); }

                            }

                        }

                    } else if !compositor.is_point_covered_by_window(mx, my) && my < tb_y {

                        let source = if desktop_drag_source { &mut desktop_files } else { &mut filemanager_app };

                        let indices = source.drag_items;

                        let source_count = source.file_count;

                        let mut done_count = 0;

                        for i in 0..source_count {

                            if !(indices[i] || i == src_idx) { continue; }

                            let name = source.files[i].display_name();

                            if name == ".." { continue; }

                            let mut src_path = alloc::string::String::from(source.path());

                            if !src_path.ends_with('/') { src_path.push('/'); }

                            src_path.push_str(name);

                            if keyboard.ctrl {

                                if fs_mgr.vfs.copy_item(&src_path, "/desktop").is_ok() { done_count += 1; }

                            } else {

                                if fs_mgr.vfs.move_item(&src_path, "/desktop").is_ok() { done_count += 1; }

                            }

                        }

                        if done_count > 0 {

                            desktop_files.refresh_from_vfs(&fs_mgr.vfs);

                            filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                            let msg = if keyboard.ctrl {

                                gui::ui_text("Masaüstüne kopyalandı.", "Copied to Desktop.")

                            } else {

                                gui::ui_text("Masaüstüne taşındı.", "Moved to Desktop.")

                            };

                            if desktop_drag_source { desktop_files.set_status_text(msg); }

                            else { filemanager_app.set_status_text(msg); }

                        }

                    }

                }

            }

            desktop_drag_source = false;

            desktop_files.drag_item = None;

            desktop_files.drag_active = false;

            desktop_files.drag_target = None;

            filemanager_app.drag_item = None;

            filemanager_app.drag_active = false;

            filemanager_app.drag_target = None;

            // Modern Aero Snap: başlık çubuğu ekran kenarlarına sürüklenip bırakıldığında:

            // - Ekranın üstü: Tam ekran (Maximize)

            // - Ekranın solu: Sol 50% bölünmüş ekran

            // - Ekranın sağı: Sağ 50% bölünmüş ekran

            if let Some(idx) = dragging_win {

                compositor.focus_window(idx);

                if my <= 4 {

                    compositor.snap_window(idx, gui::SnapPosition::Top, width, height);

                } else if mx <= 4 {

                    compositor.snap_window(idx, gui::SnapPosition::Left, width, height);

                } else if mx >= width.saturating_sub(5) {

                    compositor.snap_window(idx, gui::SnapPosition::Right, width, height);

                }

            }

            active_snap_preview = None;

            dragging_win = None;

            resizing_win = None;

            resize_edge = None;

            vscroll_drag = None;

        }



        // Alt tuşu bırakıldığında Alt+Tab penceresini kapat

        if !keyboard.alt && compositor.is_alt_tab_open {

            compositor.finish_alt_tab();

        }



        // 6. Klavye karakter girişleri

        while let Some(ch) = keyboard.take_last_char() {

            if ch == '\n' && enter_char_consumed {

                enter_char_consumed = false;

                continue;

            }

            // Başlat menüsü açıkken klavye kısayolları menü SIRASINI kullanır

            if run_dialog_open {

                if ch == '\x1b' {

                    run_dialog_open = false;

                    run_dialog_input.clear();

                } else if ch == '\x08' {

                    run_dialog_input.pop();

                } else if ch == '\n' {

                    let cmd = run_dialog_input.as_str().trim();

                    let app_id = match cmd {

                        "cmd" | "terminal" | "term" | "bash" | "sh" => Some(10),

                        "not" | "notepad" | "text" | "edit" => Some(2),

                        "ayar" | "settings" | "config" => Some(1),

                        "task" | "taskmgr" | "top" | "ps" => Some(7),

                        "calc" | "hesap" => Some(8),

                        "web" | "browser" | "www" => Some(3),

                        "file" | "files" | "fm" | "dosya" => Some(0),

                        "paint" | "cizim" => Some(9),

                        "resim" | "image" | "img" => Some(4),

                        "muzik" | "music" => Some(5),

                        "video" => Some(6),

                        "kur" | "install" => Some(11),

                        "3d" => Some(12),

                        "pkg" | "minpak" | "paket" | "pkgmgr" => Some(13),

                        _ => None,

                    };

                    if let Some(target) = app_id {

                        audio_dev.play_beep(1200, 30);

                        open_app(&mut compositor, target);

                    } else {

                        audio_dev.play_beep(440, 60);

                    }

                    run_dialog_open = false;

                    run_dialog_input.clear();

                } else if ch >= ' ' && ch != '\x7f' && run_dialog_input.len() < 30 {

                    let mut b = [0u8; 4];

                    let s = ch.encode_utf8(&mut b);

                    run_dialog_input.push_str(s);

                }

                continue;

            }



            // (pencere indeksini değil) — start_menu_app_idx ile eşlenir.

            // Özellikler modalı açıkken ESC ve Enter modalı kapatır.

            if properties_open {

                if ch == '\x1b' || ch == '\n' {

                    properties_open = false;

                    properties_name.clear();

                    properties_type.clear();

                    properties_size.clear();

                    properties_location.clear();

                    properties_path.clear();

                    force_render = true;

                }

                continue;

            }

            if ch == '\x1b' && compositor.is_start_menu_open {

                compositor.close_start_menu();

            } else if ch == '\x1b' && compositor.is_context_menu_open {

                compositor.is_context_menu_open = false;

                compositor.is_context_submenu_open = false;

            } else if ch == '\x1b' && taskmgr_app.launch_menu_open {

                taskmgr_app.launch_menu_open = false;

            } else if ch == '\x1b' && notepad_app.menu != 0 {

                notepad_app.menu = 0;

            } else if ch == '\x1b' && (filemanager_app.drag_active || desktop_files.drag_active) {

                filemanager_app.drag_item = None;

                filemanager_app.drag_active = false;

                filemanager_app.drag_target = None;

                desktop_files.drag_item = None;

                desktop_files.drag_active = false;

                desktop_files.drag_target = None;

            } else if filemanager_app.rename_active && app_is_focused(&compositor, 0) {

                if ch == '\x1b' { filemanager_app.rename_active = false; }

                else if ch == '\x08' { filemanager_app.rename_backspace(); }

                else if ch == '\n' { filemanager_app.commit_rename(&mut fs_mgr.vfs); }

                else if ch >= ' ' { filemanager_app.rename_char(ch); }

            } else if desktop_files.rename_active {

                if ch == '\x1b' { desktop_files.rename_active = false; }

                else if ch == '\x08' { desktop_files.rename_backspace(); }

                else if ch == '\n' { desktop_files.commit_rename(&mut fs_mgr.vfs); desktop_files.refresh_from_vfs(&fs_mgr.vfs); }

                else if ch >= ' ' { desktop_files.rename_char(ch); }

            } else if (ch == 'g' || ch == 'G') && keyboard.ctrl && app_is_focused(&compositor, 0) {

                // Ctrl+G: liste <-> ızgara görünümü.

                filemanager_app.toggle_view_mode();

                filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                fm_ensure_selection_visible(&mut compositor, &filemanager_app);

            } else if ch == ' ' && app_is_focused(&compositor, 0)

                && filemanager_app.selected_idx < filemanager_app.file_count {

                // Space: seçili metin/resim için hızlı önizlemeyi aç/kapat.

                filemanager_app.toggle_preview();

                filemanager_app.ensure_thumbs(&fs_mgr.vfs);

                filemanager_app.update_preview(&fs_mgr.vfs);

                fm_ensure_selection_visible(&mut compositor, &filemanager_app);

            } else if compositor.is_start_menu_open && ch == '\x08' {

                if compositor.start_search_len > 0 {

                    compositor.start_search_len -= 1;

                    compositor.start_search[compositor.start_search_len] = 0;

                    compositor.selected_start_item = 0;

                }

            } else if compositor.is_start_menu_open && ch.is_ascii_alphanumeric() {

                if compositor.start_search_len == 0 { compositor.selected_start_item = 0; }

                compositor.push_start_search(ch);

            } else if compositor.is_start_menu_open && (ch >= '1' && ch <= '9') {

                let menu_index = (ch as usize) - ('1' as usize);

                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {

                    open_app(&mut compositor, app_idx);

                    compositor.close_start_menu();

                }

            } else if compositor.is_start_menu_open && (ch == 'a' || ch == 'A' || ch == 'b' || ch == 'B') {

                let menu_index = if ch == 'a' || ch == 'A' { 9 } else { 10 };

                if let Some(app_idx) = gui::MinGuiCompositor::start_menu_app_idx(menu_index) {

                    open_app(&mut compositor, app_idx);

                    compositor.close_start_menu();

                }

            } else if compositor.is_start_menu_open && (ch == 'r' || ch == 'R') {

                if compositor.confirm_restart { restart_system(); }

                compositor.confirm_restart = true;

                compositor.confirm_shutdown = false;

            } else if compositor.is_start_menu_open && ch == '0' {

                if compositor.confirm_shutdown {

                    unsafe { core::arch::asm!("out dx, ax", in("dx") 0x604u16, in("ax") 0x2000u16); core::arch::asm!("cli; hlt"); }

                }

                compositor.confirm_shutdown = true;

                compositor.confirm_restart = false;

            } else if ch == '\t' && app_is_focused(&compositor, 10) {

                terminal_app.complete();

            } else if ch == '\t' && app_is_focused(&compositor, 7) {

                taskmgr_app.focus_next_control();

            } else if ch == '\t' && app_is_focused(&compositor, 1) {

                settings_app.focus_next_control();

            } else if ch == '\t' && app_is_focused(&compositor, 3) {

                browser_app.focus_next_control();

            } else if ch == '\t' && app_is_focused(&compositor, 0) {

                filemanager_app.focus_next_control();

            } else {

                let browser_focused = app_is_focused(&compositor, 3);

                if browser_focused && browser_app.address_focused {

                    if ch == '\x08' { browser_app.address_backspace(); }

                    else if ch == '\n' { browser_app.navigate(); }

                    else if ch >= ' ' { browser_app.address_char(ch); }

                }

                let notepad_focused = app_is_focused(&compositor, 2);

                if notepad_focused {

                    if notepad_app.menu != 0 {

                        notepad_app.menu = 0;

                    } else if ch == '\x08' {

                        notepad_app.backspace();

                    } else if ch == '\t' {

                        // Tab = girinti ekle, Shift+Tab = girintiyi geri al.

                        if keyboard.shift { notepad_app.unindent(); } else { notepad_app.insert_indent(); }

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    } else if ch == '\n' || ch >= ' ' {

                        notepad_app.insert_char(ch);

                        notepad_ensure_cursor_visible(&mut compositor, &notepad_app);

                    }

                }

                let calc_focused = app_is_focused(&compositor, 8);

                let term_focused = app_is_focused(&compositor, 10);

                if calc_focused {

                    if ch == '\x08' { calculator_app.backspace(); }

                    else if ch == '\x1b' { calculator_app.clear(); }

                    else if ch == '=' { calculator_app.press('='); }

                    else if ch.is_ascii_digit() || ch == '+' || ch == '-' || ch == '*' || ch == '/' || ch == '.' { calculator_app.insert(ch); }

                }

                if term_focused { if ch == '\x08' { terminal_app.backspace(); } else if ch == '\n' { terminal_app.submit(); } else if ch >= ' ' { terminal_app.insert(ch); } }

                // Terminalde Enter'a basıldıysa bekleyen komutu VFS/PMM bağlamında yürüt.

                if let Some(cmd) = terminal_app.take_pending() {

                    let n = terminal_app.pending_len();

                    let line = core::str::from_utf8(&cmd[..n]).unwrap_or("");

                    if let Some(app_idx) = terminal_app.execute(line, &mut fs_mgr.vfs, &mut memory_mgr.pmm, &now) {

                        open_app(&mut compositor, app_idx);

                    }

                    filemanager_app.refresh_from_vfs(&fs_mgr.vfs);

                }

                if let Some((paint_idx, _)) = focused_window_for_app(&compositor, 9) {

                    if let Some(paint) = paint_apps[paint_idx].as_mut() {

                        if ch == 'c' || ch == 'C' { paint.clear(); }

                        else if ch == 'e' || ch == 'E' { paint.eraser = !paint.eraser; }

                    }

                }

                let image_focused = app_is_focused(&compositor, 4);

                if image_focused {

                    if ch == '+' || ch == '=' { imageviewer_app.zoom_in(); }

                    else if ch == '-' || ch == '_' { imageviewer_app.zoom_out(); }

                    else if ch == '0' { imageviewer_app.zoom_level = 4; }

                }

                let v3d_focused = app_is_focused(&compositor, 12);

                if v3d_focused {

                    if ch == ' ' || ch == 'r' || ch == 'R' {

                        viewer3d_app.toggle_auto_rotate();

                    } else if ch == 'w' || ch == 'W' {

                        viewer3d_app.toggle_wireframe();

                    } else if ch == '+' || ch == '=' {

                        viewer3d_app.zoom_in();

                    } else if ch == '-' || ch == '_' {

                        viewer3d_app.zoom_out();

                    } else if ch == '1' {

                        viewer3d_app.set_shape(apps::viewer3d::ShapeType::Cube);

                    } else if ch == '2' {

                        viewer3d_app.set_shape(apps::viewer3d::ShapeType::Cylinder);

                    } else if ch == '3' {

                        viewer3d_app.set_shape(apps::viewer3d::ShapeType::Sphere);

                    } else if ch == '4' {

                        viewer3d_app.set_shape(apps::viewer3d::ShapeType::Pyramid);

                    }

                }

                let pkg_focused = app_is_focused(&compositor, 13);

                if pkg_focused {

                    if ch == '\x08' {

                        pkgmgr_app.backspace_search();

                    } else if ch == '\x1b' {

                        pkgmgr_app.clear_search();

                    } else if ch >= ' ' {

                        pkgmgr_app.insert_search_char(ch);

                    }

                }

                if !run_dialog_open && !compositor.is_start_menu_open && !compositor.is_context_menu_open {

                    if let Some(act_idx) = compositor.active_window_idx {

                        if let Some(w) = compositor.windows[act_idx] {

                            let dirty = (w.x.saturating_sub(8), w.y.saturating_sub(8), w.width + 16, w.height + 16);

                            _app_dirty_rect = Some(match _app_dirty_rect {

                                Some(d) => drivers::video::rect_union(d, dirty),

                                None => dirty,

                            });

                        }

                    }

                }

            }

        }



        let mouse_moved = mouse.x != last_mouse_x || mouse.y != last_mouse_y;

        let buttons_changed = (mouse.left_button, mouse.right_button) != last_buttons;



        let v3d_dragging = mouse.left_button && viewer3d_app.drag_active;

        let is_dragging = dragging_win.is_some() || resizing_win.is_some() || vscroll_drag.is_some() || notepad_dragging || filemanager_app.drag_active || v3d_dragging;

        let paint_active = mouse.left_button && app_is_focused(&compositor, 9);

        let is_interacting = is_dragging || paint_active;



        let viewer3d_open = compositor.windows.iter().flatten().any(|w| w.app_id == 12 && w.is_open && !w.is_minimized);

        let viewer3d_animating = viewer3d_open && (viewer3d_app.auto_rotate || viewer3d_app.drag_active);

        let media_animating = (musicplayer_app.is_playing && !audio_dev.is_muted) || videoplayer_app.is_playing || viewer3d_animating;

        let frame_interval_ms = if is_interacting || viewer3d_animating { 16 } else { 33 };

        let timer_tick_fired = monotonic_ms().wrapping_sub(last_frame_ms) >= frame_interval_ms;

        if timer_tick_fired {

            last_frame_ms = monotonic_ms();

            anim_tick = anim_tick.wrapping_add(1);

            if viewer3d_open {

                viewer3d_app.tick();

            }

        }



        // Ağ kartından gelen paketleri işle (ARP cevapları, ICMP vs.).

        drivers::e1000::poll();

        drivers::socket::poll_timeouts();

        browser_app.poll_network();



        // Beş parçalı, HDA PCM tabanlı melodi yürütücüsü. (HDA yoksa PIT/PC speaker fallback).

        audio_dev.set_volume(settings_app.volume);

        audio_dev.is_muted = settings_app.is_muted;

        musicplayer_app.volume = settings_app.volume as usize;

        if musicplayer_app.is_playing && !audio_dev.is_muted && audio_dev.volume_left > 0 {

            if anim_tick.wrapping_sub(last_music_beat) >= 24 {

                const NOTES: [u32; 8] = [440, 523, 587, 659, 587, 523, 494, 392];

                let track_offset = musicplayer_app.current_track as u32 * 22;

                let freq = NOTES[music_note % NOTES.len()] + track_offset;

                // HDA hazır ve DMA ilerliyorsa gerçek PCM örneği üret ve gönder.

                if audio_dev.is_hda_active() {

                    let sample_count = 1920; // 40 ms

                    let mut pcm: [i16; 512] = [0; 512]; // 256 L + 256 R çifti

                    let volume_scale = (audio_dev.volume_left as i32).min(100) as i16;

                    let mut idx = 0;

                    while idx < sample_count {

                        let n = (sample_count - idx).min(pcm.len() / 2);

                        for k in 0..n {

                            let t = (idx + k) as f32 / 48_000.0;

                            let angle = 6.28318f32 * freq as f32 * t;

                            let x = angle - 6.28318 * ((angle / 6.28318) as i32) as f32;

                            let x2 = x * x;

                            let v = x - x2 * x / 6.0 + x2 * x2 * x / 120.0 - x2 * x2 * x2 * x / 5040.0;

                            let sample = (v * 0.3 * volume_scale as f32 / 100.0 * 32767.0) as i16;

                            pcm[k * 2] = sample;

                            pcm[k * 2 + 1] = sample;

                        }

                        drivers::hda::push_pcm(&pcm[..n * 2]);

                        idx += n;

                    }

                    if speaker_on {

                        audio_dev.set_pc_speaker_tone(0);

                        speaker_on = false;

                    }

                } else {

                    // Otomatik Fallback: HDA yok veya sanallaştırma ortamında DMA duraksadıysa PC Speaker PWM tonlama yap!

                    audio_dev.set_pc_speaker_tone(freq);

                    speaker_on = true;

                }

                music_note = (music_note + 1) % NOTES.len();

                musicplayer_app.progress = (musicplayer_app.progress + 1) % 101;

                last_music_beat = anim_tick;

            }

        } else if speaker_on || audio_dev.is_muted || audio_dev.volume_left == 0 {

            drivers::hda::silence();

            audio_dev.set_pc_speaker_tone(0);

            speaker_on = false;

            last_music_beat = anim_tick;

        }



        let night_light_active = settings_app.night_light;

        // Ayarlar > Testler: ekran deseni dışında fare test verisini canlı besle.

        let settings_test_active = app_is_focused(&compositor, 1) && settings_app.active_tab == 4;

        if settings_test_active {

            if let Some((_, w)) = focused_window_for_app(&compositor, 1) {

                settings_app.capture_mouse_test(mx, my, mouse.left_button, mouse.right_button, mouse.middle_button, w.x, w.y);

            }

        }

        let busy_dirty = unsafe {

            let dirty = BUSY_DIRTY;

            BUSY_DIRTY = false;

            dirty

        };

        // Seçim/konum değiştiyse Hızlı Önizleme içeriğini tazele. Kimlik anahtarı

        // aynıyken çağrı ucuzdur ve VFS okuması yapmaz.

        if filemanager_app.preview_open {

            filemanager_app.update_preview(&fs_mgr.vfs);

        }

        let full_redraw = input_update.keyboard || buttons_changed || scroll_val != 0

            || force_render || busy_dirty || (mouse_moved && (is_interacting || night_light_active || settings_test_active))

            || (timer_tick_fired && media_animating);



        if full_redraw {

            let tm_open = compositor.windows.iter().flatten().any(|w| w.app_id == 7 && w.is_open && !w.is_minimized);

            if tm_open {

                // Görev yöneticisi istatistiklerini her frame yerine periyodik güncelle.

                use core::sync::atomic::{AtomicUsize, Ordering};

                static TM_UPDATE_TICK: AtomicUsize = AtomicUsize::new(0);

                let tick = TM_UPDATE_TICK.fetch_add(1, Ordering::Relaxed);

                if tick % 30 == 0 {

                    taskmgr_app.update(&memory_mgr.pmm, &fs_mgr.vfs, &compositor.windows, &app_render_cycles);

                }

            }

            keep_windows_on_screen(&mut compositor, width, height);



            // Menüler veya modal pencereler kapalıyken tek pencere içi klavye girişi veya

            // pencere sürükleme yapılıyorsa yalnızca kirli dikdörtgeni (dirty rect) çizerek

            // CPU yükünü ve gereksiz VRAM kopyalamasını %60-70 azalt.

            let target_dirty = if force_render || busy_dirty || compositor.is_start_menu_open || compositor.is_context_menu_open || compositor.is_alt_tab_open || run_dialog_open || night_light_active {

                None

            } else if let Some(drag) = drag_dirty_rect {

                Some(drag)

            } else if let Some(app_dirty) = _app_dirty_rect {

                Some(app_dirty)

            } else {

                None

            };



            render_full_frame(

                &mut fb, &compositor, &filemanager_app, &desktop_files, &settings_app,

                &notepad_app, &browser_app, &imageviewer_app, &musicplayer_app,

                &videoplayer_app, &taskmgr_app, &calculator_app, &paint_apps, &terminal_app,

                &installer_app, &viewer3d_app, &pkgmgr_app,

                &now, &mouse, &keyboard, memory_mgr.pmm.total_memory_bytes, anim_tick, &mut app_render_cycles,

                (run_dialog_open, &run_dialog_input),

                (properties_open, &properties_name, &properties_type, &properties_size, &properties_location, &properties_path),

                active_snap_preview,

                target_dirty,

            );

            drag_dirty_rect = None;

            _app_dirty_rect = None;

        } else if mouse_moved {

            // İmleç hareketinde uygulamaları ve tüm ekranı yeniden çizmek yerine

            // sadece önceki ve yeni imleç alanlarını birleşik tek bir dikdörtgen olarak güncelle.

            let old_rect = fb.restore_cursor_background();

            compositor.draw_mouse_cursor(&mut fb, mx, my);

            let new_rect = fb.cursor_rect();

            match (old_rect, new_rect) {

                (Some(o), Some(n)) => {

                    let u = drivers::video::rect_union(o, n);

                    fb.flush_rect(u);

                }

                (Some(o), None) => fb.flush_rect(o),

                (None, Some(n)) => fb.flush_rect(n),

                _ => {}

            }

        }



        last_mouse_x = mouse.x;

        last_mouse_y = mouse.y;

        last_buttons = (mouse.left_button, mouse.right_button);



        if !input_update.mouse && !input_update.keyboard && !mouse_moved && !buttons_changed && !force_render {

            // Boşta bekleme: port 0x80 VM çıkışları olmadan spin_loop ile bekle.

            // PS/2 giriş tamponunda veri algılandığı anda gecikmesiz döngüden çık.

            let wait_start = read_cpu_cycles();

            let wait_cycles = unsafe { TSC_CYCLES_PER_US }.saturating_mul(2_500);

            while read_cpu_cycles().wrapping_sub(wait_start) < wait_cycles {

                if drivers::ps2::has_input() || drivers::usb_hid::has_event() {

                    break;

                }

                core::hint::spin_loop();

            }

            let loop_end_cycles = read_cpu_cycles();

            let elapsed = loop_end_cycles.wrapping_sub(loop_start_cycles);

            let idle_elapsed = loop_end_cycles.wrapping_sub(wait_start).min(elapsed);

            cpu_sample_total_cycles = cpu_sample_total_cycles.saturating_add(elapsed);

            cpu_sample_active_cycles = cpu_sample_active_cycles.saturating_add(elapsed.saturating_sub(idle_elapsed));

        } else {

            core::hint::spin_loop();

            let elapsed = read_cpu_cycles().wrapping_sub(loop_start_cycles);

            cpu_sample_total_cycles = cpu_sample_total_cycles.saturating_add(elapsed);

            cpu_sample_active_cycles = cpu_sample_active_cycles.saturating_add(elapsed);

        }



        let now_cycles = read_cpu_cycles();

        let sample_window = unsafe { TSC_CYCLES_PER_US }.saturating_mul(250_000);

        if now_cycles.wrapping_sub(cpu_sample_start) >= sample_window {

            let bsp_pct = if cpu_sample_total_cycles == 0 { 0 } else {

                (cpu_sample_active_cycles.saturating_mul(100) / cpu_sample_total_cycles).min(100) as usize

            };

            taskmgr_app.cpu_usage_pct = bsp_pct;



            let cpus = arch::x86_64::smp::cpu_count().min(arch::x86_64::smp::MAX_CPUS);

            taskmgr_app.core_count = cpus;

            taskmgr_app.core_usage_pct[0] = bsp_pct;



            for c in 1..cpus {

                let hb = arch::x86_64::smp::heartbeat(c);

                let diff = hb.wrapping_sub(last_ap_heartbeats[c]);

                last_ap_heartbeats[c] = hb;

                // AP döngüsü her 500 boşta beklemede bir kalp atışı artırır.

                // Aktif bir iş varsa poll_job yapılır veya kalp atışları canlılık gösterir.

                taskmgr_app.core_usage_pct[c] = if arch::x86_64::smp::is_ap_online(c) {

                    if diff > 0 { ((diff.min(50) * 100) / 50).min(100) as usize } else { 0 }

                } else {

                    0

                };

            }



            cpu_sample_start = now_cycles;

            cpu_sample_total_cycles = 0;

            cpu_sample_active_cycles = 0;

        }

    }

}

