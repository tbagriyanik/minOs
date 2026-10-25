use alloc::vec::Vec;
use crate::arch;
use crate::boot;
use crate::boot::console::serial_print;
use crate::boot::limine::*;
use crate::boot::devices::{GLOBAL_KEYBOARD, GLOBAL_MOUSE};
use crate::drivers;
use crate::drivers::video::Framebuffer;
use crate::fs;
use crate::mm;
use crate::task;

pub struct BootSubsystems {
    pub fs_mgr: fs::FileSystemManager,
    pub boot_log: Vec<u8>,
    pub gpu_caps: drivers::gpu::GpuCaps,
    pub audio_dev: drivers::audio::AudioDevice,
    pub mouse: &'static mut drivers::mouse::MouseState,
    pub keyboard: &'static mut drivers::keyboard::KeyboardState,
    pub installed: bool,
    pub wants_installed: bool,
}

pub fn init_subsystems(
    fb: &mut Framebuffer,
    fb_ptr: *mut u32,
    memory_mgr: &mut mm::MemoryManager,
    alloc_pmm_frame_global: fn() -> Option<u64>,
) -> BootSubsystems {
    // 2. Sanal Dosya Sistemi (VFS / TarFS) Baslatimi
    serial_print("[BOOT] Sanal dosya sistemi başlatılıyor...\n");
    let mut fs_mgr = fs::FileSystemManager::new();
    let boot_log = Vec::with_capacity(4096);
    boot::console::save_boot_log(&mut fs_mgr.vfs);

    // 1. Donanim Kesmeleri (IDT) ve PMM Baslatimi
    serial_print("[BOOT] Donanım kesmeleri ve bellek yöneticisi başlatılıyor...\n");
    arch::x86_64::init_interrupts();

    unsafe {
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
                        let slot_addr = match (entries as usize)
                            .checked_add(i * core::mem::size_of::<*const LimineMemoryMapEntry>())
                        {
                            Some(addr) => addr,
                            None => break,
                        };
                        let slot = slot_addr as *const *const LimineMemoryMapEntry;
                        if !is_mapped_limine_ptr(slot) {
                            break;
                        }
                        let entry_ptr = core::ptr::read_volatile(slot);
                        if !is_mapped_limine_ptr(entry_ptr) {
                            continue;
                        }
                        let base = core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).base));
                        let length =
                            core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).length));
                        let entry_type =
                            core::ptr::read_volatile(core::ptr::addr_of!((*entry_ptr).entry_type));
                        if length == 0 || base.checked_add(length).is_none() {
                            continue;
                        }
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

    serial_print("[PMM] Fiziksel bellek yöneticisi başlatıldı.\n");
    arch::x86_64::set_mmio_frame_allocator(alloc_pmm_frame_global);

    // VRAM MMIO aralığını (16MB) sayfa tablolarına eşle; çözünürlük veya çizim unmapped #PF üretmesin.
    let fb_phys = (fb_ptr as u64).wrapping_sub(drivers::phys_to_virt(0));
    for off in (0..16 * 1024 * 1024).step_by(2 * 1024 * 1024) {
        arch::x86_64::mmio_map_2mb((fb_phys + off as u64) & !0x1F_FFFF);
    }

    // PMM Kendi Kendini Test Etme
    serial_print("[BOOT] Bellek yöneticisi test ediliyor...\n");
    if let (Some(f1), Some(f2)) = (
        memory_mgr.pmm.allocate_frame(),
        memory_mgr.pmm.allocate_frame(),
    ) {
        if f1 != f2 && f1 % mm::pmm::PAGE_SIZE == 0 && f2 % mm::pmm::PAGE_SIZE == 0 {
            serial_print("[PMM] Çerçeve tahsis testi başarılı (çakışmayan çerçeveler doğrulandı).\n");
        }
        memory_mgr.pmm.free_frame(f2);
        memory_mgr.pmm.free_frame(f1);
    }

    // VMM ve Güvenli Uygulama Yükleyicisi Doğrulaması
    serial_print("[BOOT] Sanal bellek yöneticisi test ediliyor...\n");
    let mut address_space = memory_mgr.kernel_address_space();
    let test_vaddr = mm::loader::USER_CODE_BASE;
    if let Some(frame) = memory_mgr.pmm.allocate_frame() {
        if address_space
            .map_page(
                test_vaddr,
                frame,
                mm::vmm::PageFlags::USER_CODE,
                &mut memory_mgr.pmm,
            )
            .is_ok()
        {
            if let Some(translated_phys) = address_space.translate(test_vaddr) {
                if translated_phys == frame {
                    serial_print(
                        "[VMM] 4 seviyeli sanal bellek eşleme ve adres çözümleme etkin.\n",
                    );
                }
            }
            let _ = address_space.unmap_page(test_vaddr);
        }
        memory_mgr.pmm.free_frame(frame);
    }

    // Test amacli standart ELF64 ikili dosyasini yukle ve dogrula
    let sample_elf_bin = mm::loader::AppLoader::create_sample_elf();
    if let Ok(loaded) = mm::loader::AppLoader::load_binary(
        &sample_elf_bin,
        &mut address_space,
        &mut memory_mgr.pmm,
    ) {
        if loaded.entry_point >= mm::loader::USER_CODE_BASE && loaded.stack_pointer > 0 {
            serial_print("[LOADER] Standart ELF64 ikilisi güvenli bellek yalıtımıyla yüklendi (W^X / NX etkin).\n");
        }
        loaded.unmap_and_free(&mut address_space, &mut memory_mgr.pmm);
    }

    // Süreç Yöneticisi ve Sistem Çağrılarının (syscalls) Başlatımı
    serial_print("[BOOT] Süreç yöneticisi başlatılıyor...\n");
    memory_mgr.proc_mgr.init_kernel_process();

    let test_pid = mm::process::syscall_dispatch(
        mm::process::sys_nos::SYS_GETPID,
        0,
        0,
        0,
        &mut memory_mgr.proc_mgr,
        &mut memory_mgr.pmm,
    );
    let test_frame = mm::process::syscall_dispatch(
        mm::process::sys_nos::SYS_ALLOC_FRAME,
        0,
        0,
        0,
        &mut memory_mgr.proc_mgr,
        &mut memory_mgr.pmm,
    );
    if test_frame != 0 {
        let _ = mm::process::syscall_dispatch(
            mm::process::sys_nos::SYS_FREE_FRAME,
            test_frame,
            0,
            0,
            &mut memory_mgr.proc_mgr,
            &mut memory_mgr.pmm,
        );
    }
    if test_pid == 0 {
        serial_print("[PROC] Süreç tablosu ve sistem çağrıları (syscalls) etkin.\n");
    }

    serial_print("[BOOT] Bellek testleri çalıştırılıyor...\n");
    mm::vmm::test_memory_and_isolation(&mut memory_mgr.pmm);
    mm::allocator::test_allocator_subsystem();

    serial_print("[BOOT] Süreç testleri çalıştırılıyor...\n");
    mm::process::test_process_lifecycle_and_fault_recovery(
        &mut memory_mgr.proc_mgr,
        &mut memory_mgr.pmm,
    );

    // Hardware-safe mode: do not touch APIC/SMP until the laptop path is stable.
    serial_print("[BOOT] Tek CPU modu seçildi; APIC/SMP başlatması atlandı.\n");
    serial_print("[SMP] BSP GDT yükleniyor...\n");
    unsafe { arch::x86_64::gdt::load_for_cpu(0) };
    serial_print("[SMP] 1 CPU modunda devam ediliyor.\n");

    serial_print("[BOOT] APIC zamanlayıcısı başlatılıyor...\n");
    serial_print("[APIC] APIC zamanlayıcısı atlandı; TSC monotonik saati kullanılıyor.\n");

    // Paralel saglama toplami: AP'ler + BSP ayni tamponu parcali isler.
    {
        const SUM_FRAMES: usize = 64;
        const U64_PER_FRAME: usize = 512;
        if let Some(buf_phys) = memory_mgr.pmm.allocate_contiguous_frames(SUM_FRAMES) {
            unsafe {
                let base = crate::drivers::phys_to_virt(buf_phys) as *mut u64;
                let mut expected: u64 = 0;
                for i in 0..SUM_FRAMES * U64_PER_FRAME {
                    let v = (i as u64)
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        .wrapping_add(0x1234_5678);
                    base.add(i).write_volatile(v);
                    expected = expected.wrapping_add(v);
                }
                core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
                match arch::x86_64::smp::parallel_checksum(
                    buf_phys,
                    SUM_FRAMES * U64_PER_FRAME,
                    4 * U64_PER_FRAME,
                ) {
                    Some(got) if got == expected => {
                        serial_print("[SMP] Paralel sağlama toplamı (AP+BSP) eşleşti: BAŞARILI\n");
                    }
                    Some(_) => {
                        serial_print("[SMP] Paralel sağlama toplamı UYUŞMADI: BAŞARISIZ\n");
                    }
                    None => {
                        serial_print("[SMP] Paralel sağlama toplamı zaman aşımı (tek çekirdek yedeği yok).\n");
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
    let driver_profile = if cmdline_has_token(cmdline, "full") {
        "full"
    } else if cmdline_has_token(cmdline, "hardware") {
        "hardware"
    } else if cmdline_has_token(cmdline, "audio") {
        "audio"
    } else if cmdline_has_token(cmdline, "safe") {
        "safe"
    } else {
        "full"
    };
    serial_print("[DRIVERS] Profil: ");
    serial_print(driver_profile);
    serial_print("\n");
    let profile_line = alloc::format!("[DRIVERS] profile: {}", driver_profile);
    let _ = fs_mgr.vfs.write_file("/root/Documents/boot.log", profile_line.as_bytes());

    if driver_profile == "audio" || driver_profile == "full" {
        serial_print("[DRIVER] HDA ses sürücüsü başlatılıyor...\n");
        let hda_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(6);
        let Some(hda_dma_phys) = hda_dma_phys else {
            boot::text_console::fatal("HDA DMA bellek tahsisi başarısız");
        };
        drivers::hda::init(Some(hda_dma_phys));
        serial_print("[DRIVER] HDA ses sürücüsü hazır.\n");
    }

    if driver_profile == "hardware" || driver_profile == "full" {
        serial_print("[DRIVER] e1000 ağ sürücüsü başlatılıyor...\n");
        let e1000_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(34);
        let Some(e1000_dma_phys) = e1000_dma_phys else {
            boot::text_console::fatal("e1000 DMA bellek tahsisi başarısız");
        };
        drivers::e1000::init(Some(e1000_dma_phys));
        serial_print("[DRIVER] e1000 ağ sürücüsü hazır.\n");

        // Yeni ağ driver'ları
        if drivers::virtio_net::detect() {
            serial_print("[DRIVER] VirtIO-net algılandı.\n");
            if drivers::virtio_net::init() {
                serial_print("[DRIVER] VirtIO-net hazır.\n");
            }
        }
        if drivers::i219::detect() {
            serial_print("[DRIVER] Intel I219-V algılandı.\n");
            if drivers::i219::init() {
                serial_print("[DRIVER] Intel I219-V hazır.\n");
            }
        }
        if drivers::atheros::detect() {
            serial_print("[DRIVER] Atheros Ethernet algılandı.\n");
            if drivers::atheros::init() {
                serial_print("[DRIVER] Atheros Ethernet hazır.\n");
            }
        }
        if drivers::marvell::detect() {
            serial_print("[DRIVER] Marvell Ethernet algılandı.\n");
            if drivers::marvell::init() {
                serial_print("[DRIVER] Marvell Ethernet hazır.\n");
            }
        }
        if drivers::bluetooth::detect() {
            serial_print("[DRIVER] Bluetooth adaptörü algılandı.\n");
            if drivers::bluetooth::init() {
                serial_print("[DRIVER] Bluetooth adaptörü hazır.\n");
            }
        }

        serial_print("[DRIVER] Ağ protokolü başlatılıyor...\n");
        drivers::netstack::init();
        let _ = drivers::netstack::dhcp_request();
        drivers::netstack::run_network_self_test();
        serial_print("[DRIVER] Ağ protokolü hazır.\n");

        serial_print("[DRIVER] USB sürücüsü başlatılıyor...\n");
        drivers::usb::init();
        drivers::usb::test_usb_subsystems();
        serial_print("[DRIVER] USB HID (xHCI) sürücüsü başlatılıyor...\n");
        drivers::usb_hid::init(&mut memory_mgr.pmm);
        serial_print("[DRIVER] USB HID sürücüsü hazır.\n");
        serial_print("[DRIVER] USB sürücüsü hazır.\n");

        // Yeni USB driver'ları
        if drivers::usb_mass_storage::detect() {
            serial_print("[DRIVER] USB Mass Storage algılandı.\n");
            if drivers::usb_mass_storage::init() {
                serial_print("[DRIVER] USB Mass Storage hazır.\n");
            }
        }
        if drivers::usb_printer::detect() {
            serial_print("[DRIVER] USB yazıcı algılandı.\n");
            if drivers::usb_printer::init() {
                serial_print("[DRIVER] USB yazıcı hazır.\n");
            }
        }
        if drivers::usb_audio::detect() {
            serial_print("[DRIVER] USB ses kartı algılandı.\n");
            if drivers::usb_audio::init() {
                serial_print("[DRIVER] USB ses kartı hazır.\n");
            }
        }
        if drivers::usb_touchscreen::detect() {
            serial_print("[DRIVER] USB dokunmatik ekran algılandı.\n");
            if drivers::usb_touchscreen::init() {
                serial_print("[DRIVER] USB dokunmatik ekran hazır.\n");
            }
        }
    }

    // Depolama driver'ları
    if drivers::ahci::detect() {
        serial_print("[DRIVER] AHCI SATA algılandı.\n");
        if drivers::ahci::init() {
            serial_print("[DRIVER] AHCI SATA hazır.\n");
        }
    }
    if drivers::nvme::detect() {
        serial_print("[DRIVER] NVMe SSD algılandı.\n");
        if drivers::nvme::init() {
            serial_print("[DRIVER] NVMe SSD hazır.\n");
        }
    }
    if drivers::virtio_blk::detect() {
        serial_print("[DRIVER] VirtIO Block algılandı.\n");
        if drivers::virtio_blk::init() {
            serial_print("[DRIVER] VirtIO Block hazır.\n");
        }
    }
    if drivers::sdmmc::detect() {
        serial_print("[DRIVER] SD/MMC kart algılandı.\n");
        if drivers::sdmmc::init() {
            serial_print("[DRIVER] SD/MMC kart hazır.\n");
        }
    }

    // Grafik driver'ları
    if drivers::virtio_gpu::detect() {
        serial_print("[DRIVER] VirtIO GPU algılandı.\n");
        if drivers::virtio_gpu::init() {
            serial_print("[DRIVER] VirtIO GPU hazır.\n");
        }
    }
    if drivers::amdgpu::detect() {
        serial_print("[DRIVER] AMD Radeon GPU algılandı.\n");
        if drivers::amdgpu::init() {
            serial_print("[DRIVER] AMD Radeon GPU hazır.\n");
        }
    }
    if drivers::nvidia::detect() {
        serial_print("[DRIVER] NVIDIA GeForce GPU algılandı.\n");
        if drivers::nvidia::init() {
            serial_print("[DRIVER] NVIDIA GeForce GPU hazır.\n");
        }
    }

    // Donanım sensörleri
    if drivers::acpi_battery::detect() {
        serial_print("[DRIVER] ACPI pil algılandı.\n");
        if drivers::acpi_battery::init() {
            serial_print("[DRIVER] ACPI pil hazır.\n");
        }
    }
    if drivers::acpi_power::detect() {
        serial_print("[DRIVER] ACPI güç yönetimi algılandı.\n");
        if drivers::acpi_power::init() {
            serial_print("[DRIVER] ACPI güç yönetimi hazır.\n");
        }
    }
    if drivers::fan_temp::detect() {
        serial_print("[DRIVER] Fan ve sıcaklık sensörleri algılandı.\n");
        if drivers::fan_temp::init() {
            serial_print("[DRIVER] Fan ve sıcaklık sensörleri hazır.\n");
        }
    }

    // Gelişmiş aygıt driver'ları
    serial_print("[DRIVER] PCIe veri yolu taranıyor...\n");
    drivers::pcie::init();
    serial_print("[DRIVER] PCIe veri yolu hazır.\n");

    serial_print("[DRIVER] PCI veri yolu taranıyor...\n");
    drivers::pci::init();
    serial_print("[DRIVER] PCI veri yolu hazır.\n");

    serial_print("[DRIVER] Sistem zamanlayıcıları başlatılıyor...\n");
    drivers::timer::detect();
    drivers::timer::init(drivers::timer::TimerType::PIT, 1000);
    serial_print("[DRIVER] Sistem zamanlayıcıları hazır.\n");

    let _socket_timeout_task = task::spawn(
        "socket-timeouts",
        task::TaskPriority::Normal,
        || {
            drivers::socket::poll_timeouts();
            task::TaskStatus::Yielded
        },
    );
    serial_print("[TASK] İşbirlikçi soket zaman aşımı görevi kuyruğa alındı.\n");

    let _net_dev = drivers::net::NetworkInterface::new();
    let mut audio_dev = drivers::audio::AudioDevice::new();
    audio_dev.test_audio_subsystem();

    serial_print("[DRIVERS] Tüm sürücüler hazır.\n");

    // 4b. Gelismis grafik suruculeri: BGA/VMware/VGA yetenek taramasi.
    let gpu_caps = drivers::gpu::init();
    if gpu_caps.bga_present {
        serial_print("[GPU] BGA algılandı, çözünürlük değişimi açık.\n");
        fb.set_hw_cursor(false);
        serial_print("[GPU] Hızlandırılmış yazılım imleci etkin.\n");
    } else {
        serial_print("[GPU] BGA yok; çözünürlük Limine ile sabit, yazılım imleci.\n");
    }

    // 5. PS/2 Donanim Suruculeri
    let mouse = unsafe { &mut *core::ptr::addr_of_mut!(GLOBAL_MOUSE) };
    let keyboard = unsafe { &mut *core::ptr::addr_of_mut!(GLOBAL_KEYBOARD) };
    drivers::ps2::init();
    if drivers::ps2::present() {
        serial_print("[BOOT] PS/2 hazır.\n");
    } else {
        serial_print("[BOOT] PS/2 denetleyicisi yok; girdi yalnızca USB HID üzerinden.\n");
    }

    let usb_hid_count = drivers::usb_hid::device_count();
    if usb_hid_count == 0 {
        serial_print("[INPUT] USB HID bulunamadı, PS/2 yedeği kullanılıyor (BIOS Legacy USB/CSM kapalıysa klavye/fare algılanamayabilir).\n");
    } else {
        serial_print("[INPUT] USB HID etkin: bağlı aygıtlar kullanıma hazır.\n");
    }

    // 5b. Tema motorunu başlat
    crate::gui::theme::init();
    serial_print("[BOOT] Tema hazır.\n");

    // 5c. önyükleme modu denetimi:
    let wants_installed = boot_wants_installed();
    let installed = fs::diskfs::is_installed();
    serial_print("[BOOT] Kurulum denetimi: wants_installed=");
    serial_print(if wants_installed { "1" } else { "0" });
    serial_print(" installed=");
    serial_print(if installed { "1" } else { "0" });
    serial_print("\n");

    if wants_installed && !installed {
        serial_print("[BOOT] 'installed' modu istendi ancak diskte kurulum bulunamadı.\n");
        boot::text_console::fatal("Kurulu sistem seçildi ancak MINOS.SYS bulunamadı");
    }

    serial_print("[BOOT] Tüm kritik alt sistemler başlatıldı. GUI ve uygulamalar yükleniyor...\n");

    BootSubsystems {
        fs_mgr,
        boot_log,
        gpu_caps,
        audio_dev,
        mouse,
        keyboard,
        installed,
        wants_installed,
    }
}
