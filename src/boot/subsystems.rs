use crate::arch;
use crate::boot;
use crate::boot::devices::{GLOBAL_KEYBOARD, GLOBAL_MOUSE};
use crate::boot::limine::*;
use crate::drivers;
use crate::drivers::video::Framebuffer;
use crate::fs;
use crate::mm;
use crate::task;
use alloc::vec::Vec;
use core::fmt::Write;
use heapless::String as HeaplessString;

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
    // Açılış süresi profili: her aşamanın süresi seri konsola yazılır.
    let mut timer = crate::boot::timing::PhaseTimer::new();

    // 2. Sanal Dosya Sistemi (VFS / TarFS) Baslatimi
    kinfo!("boot", "[BOOT] Starting virtual file system...\n");
    let mut fs_mgr = fs::FileSystemManager::new();
    let boot_log = Vec::with_capacity(4096);
    boot::console::save_boot_log(&mut fs_mgr.vfs);
    timer.mark("vfs");

    // 1. Donanim Kesmeleri (IDT) ve PMM Baslatimi
    kinfo!(
        "boot",
        "[BOOT] Starting hardware interrupts and memory manager...\n"
    );
    arch::x86_64::init_interrupts(0);

    unsafe {
        if let Some(response) =
            valid_limine_response(core::ptr::addr_of!(MEMORY_MAP_REQUEST.response))
                .filter(|response| !response.is_null())
        {
            let count = core::ptr::read_volatile(core::ptr::addr_of!((*response).entry_count));
            let entries = core::ptr::read_volatile(core::ptr::addr_of!((*response).entries));
            let count = count.min(256) as usize;
            let array_bytes =
                count.checked_mul(core::mem::size_of::<*const LimineMemoryMapEntry>());
            if count > 0 && is_mapped_limine_ptr(entries) {
                let array_end = array_bytes.and_then(|b| (entries as usize).checked_add(b));
                if array_end
                    .is_some_and(|end| is_mapped_limine_ptr(end as *const LimineMemoryMapEntry))
                {
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

    kinfo!("boot", "[PMM] Physical memory manager started.\n");
    if memory_mgr.pmm.zone_self_test() {
        kdebug!(
            "boot",
            "[PMM-ZONE] DMA/Normal bolgeleri ve CPU sayfa onbellegi dogrulandi.\n"
        );
    } else {
        kwarn!("boot", "[PMM-ZONE] WARNING: zone/cache self-test failed.\n");
    }
    arch::x86_64::set_mmio_frame_allocator(alloc_pmm_frame_global);
    timer.mark("pmm");

    // Kernel heap büyütme: ayırıcıyı yeşil alan (greenfield) sanal pencereye
    // taşı; statik tampon yalnızca erken önyükleme içindi ve bu noktadan
    // sonra heap `HEAP_MAX_SIZE` boyuna kadar büyüyebilir.
    if !crate::kernel::heap::relocate_to_window() {
        kwarn!(
            "boot",
            "[KHEAP] UYARI: Heap sanal penceresi kurulamadı; statik 16 MB sınırı geçerli.\n"
        );
    }
    timer.mark("heap");

    // VRAM MMIO aralığını (16MB) sayfa tablolarına eşle; çözünürlük veya çizim unmapped #PF üretmesin.
    let fb_phys = (fb_ptr as u64).wrapping_sub(drivers::phys_to_virt(0));
    for off in (0..16 * 1024 * 1024).step_by(2 * 1024 * 1024) {
        arch::x86_64::mmio_map_2mb((fb_phys + off as u64) & !0x1F_FFFF);
    }

    // PMM Kendi Kendini Test Etme
    kinfo!("boot", "[BOOT] Bellek yöneticisi test ediliyor...\n");
    if let (Some(f1), Some(f2)) = (
        memory_mgr.pmm.allocate_frame(),
        memory_mgr.pmm.allocate_frame(),
    ) {
        if f1 != f2 && f1 % mm::pmm::PAGE_SIZE == 0 && f2 % mm::pmm::PAGE_SIZE == 0 {
            kdebug!(
                "boot",
                "[PMM] Çerçeve tahsis testi başarılı (çakışmayan çerçeveler doğrulandı).\n"
            );
        }
        memory_mgr.pmm.free_frame(f2);
        memory_mgr.pmm.free_frame(f1);
    }

    // VMM ve Güvenli Uygulama Yükleyicisi Doğrulaması
    kinfo!("boot", "[BOOT] Testing virtual memory manager...\n");
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
                    kdebug!(
                        "boot",
                        "[VMM] 4 seviyeli sanal bellek eşleme ve adres çözümleme etkin.\n"
                    );
                }
            }
            let _ = address_space.unmap_page(test_vaddr);
        }
        memory_mgr.pmm.free_frame(frame);
    }

    // Test amacli standart ELF64 ikili dosyasini yukle ve dogrula
    let sample_elf_bin = mm::loader::AppLoader::create_sample_elf();
    if let Ok(loaded) =
        mm::loader::AppLoader::load_binary(&sample_elf_bin, &mut address_space, &mut memory_mgr.pmm)
    {
        if loaded.entry_point >= mm::loader::USER_CODE_BASE && loaded.stack_pointer > 0 {
            kdebug!("boot", "[LOADER] Standart ELF64 ikilisi güvenli bellek yalıtımıyla yüklendi (W^X / NX etkin).\n");
        }
        loaded.unmap_and_free(&mut address_space, &mut memory_mgr.pmm);
    }
    timer.mark("vmm");

    // Süreç Yöneticisi ve Sistem Çağrılarının (syscalls) Başlatımı
    kinfo!("boot", "[BOOT] Starting process manager...\n");
    memory_mgr.proc_mgr.init_kernel_process();
    // Syscall fd katmanı için VFS dosyalarını önbellekle (açılış anlık görüntüsü).
    let cached_files = memory_mgr.proc_mgr.files.cache_vfs(&fs_mgr.vfs);
    kdebug!(
        "boot",
        "[FD] VFS dosya önbelleği hazır: {} dosya.\n",
        cached_files
    );
    // Çekirdek CR3'ünü kaydet: kullanıcı→çekirdek trampoline'leri ve
    // zamanlayıcı CR3'ü buraya geri yükler.
    arch::x86_64::ring3::note_kernel_cr3(
        memory_mgr
            .proc_mgr
            .get_process(0)
            .map_or(0, |p| p.address_space.pml4_phys),
    );

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
        kinfo!(
            "boot",
            "[PROC] Süreç tablosu ve sistem çağrıları (syscalls) etkin.\n"
        );
    }
    timer.mark("proc");

    // Paylaşılan sıfır sayfasını önden ayır: tembel eşleme altyapısının
    // tek seferlik kaynağı süreç testi ölçümlerine "sızıntı" olarak yansımasın.
    mm::user_mem::ensure_zero_page(&mut memory_mgr.pmm);

    kdebug!("boot", "[BOOT] Running memory tests...\n");
    mm::vmm::test_memory_and_isolation(&mut memory_mgr.pmm);
    mm::allocator::test_allocator_subsystem();
    kdebug!(
        "boot",
        "[BOOT] Running large-page and compression tests...\n"
    );
    mm::memory::run_boot_tests(&mut memory_mgr.pmm);
    crate::kernel::heap::test_heap_growth();
    crate::kernel::heap::test_tiered_alloc();
    mm::memory::profile::report_boot_summary();
    timer.mark("bellek-testleri");

    kdebug!("boot", "[BOOT] Running process tests...\n");
    mm::process::test_process_lifecycle_and_fault_recovery(
        &mut memory_mgr.proc_mgr,
        &mut memory_mgr.pmm,
    );
    timer.mark("surec-testleri");

    kdebug!(
        "boot",
        "[BOOT] Ring 3 izolasyon testleri çalıştırılıyor...\n"
    );
    mm::process::test_ring3_isolation(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    kdebug!(
        "boot",
        "[BOOT] Syscall kullanıcı işaretçi doğrulama testleri çalıştırılıyor...\n"
    );
    mm::process::test_syscall_pointer_validation(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    kdebug!(
        "boot",
        "[BOOT] Seccomp syscall izin maskesi testleri çalıştırılıyor...\n"
    );
    mm::process::test_syscall_filter(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    kdebug!(
        "boot",
        "[BOOT] Kullanıcı heap/mmap/ASLR/COW testleri çalıştırılıyor...\n"
    );
    mm::process::test_user_memory(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    kdebug!(
        "boot",
        "[BOOT] Dosya tanıtıcısı yaşam döngüsü testleri çalıştırılıyor...\n"
    );
    mm::process::test_file_descriptor_lifecycle(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    kdebug!(
        "boot",
        "[BOOT] Süreç kaynak limitleri testleri çalıştırılıyor...\n"
    );
    mm::process::test_resource_limits(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    kdebug!(
        "boot",
        "[BOOT] kill/SIGSTOP/SIGCONT/SIGTERM testleri çalıştırılıyor...\n"
    );
    mm::process::test_kill_suspend_resume(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    kdebug!(
        "boot",
        "[BOOT] Süreç grubu/oturum/affinity/sinyal testleri çalıştırılıyor...\n"
    );
    mm::process::test_process_groups_and_signals(&mut memory_mgr.proc_mgr, &mut memory_mgr.pmm);
    kdebug!(
        "boot",
        "[BOOT] Ring 3 syscall ve register dönüş testleri çalıştırılıyor...\n"
    );
    let ring3_demo_ok = arch::x86_64::ring3::demo(&mut memory_mgr.pmm);
    if !ring3_demo_ok {
        kerror!(
            "boot",
            "[RING3-TEST] Syscall/register dönüş doğrulaması: BAŞARISIZ\n"
        );
    }
    timer.mark("ring3-testleri");

    kinfo!("boot", "[BOOT] Starting SMP...\n");
    kdebug!("boot", "[SMP] Loading BSP GDT...\n");
    unsafe { arch::x86_64::gdt::load_for_cpu(0) };
    let requested_cpus = arch::x86_64::smp::init();
    let online_cpus = arch::x86_64::smp::wait_for_aps(5_000_000);
    kinfo!(
        "boot",
        "[SMP] Çevrimiçi CPU: {} / istenen: {}; per-CPU run queue etkin.\n",
        online_cpus,
        requested_cpus
    );

    kinfo!("boot", "[BOOT] Starting APIC timer...\n");
    // Local APIC MMIO sayfasını eşlemeden ilk lapic_write çekirdek #PF (vektor 14)
    // üretir ve sistemi siyah ekranda durdurur. lapic_init() önce MMIO sayfasını
    // map'ler, ardından apic::init_timer() timer kayıtlarına güvenle yazabilir.
    arch::x86_64::smp::lapic_init();
    arch::x86_64::apic::init_timer();
    kdebug!(
        "boot",
        "[APIC] {}Hz scheduler timer etkin; TSC monotonik saat olarak korunuyor.\n",
        arch::x86_64::apic::SCHEDULER_HZ
    );
    kdebug!(
        "boot",
        "[SCHED] {}Hz round-robin scheduler; network downloads yield cooperatively.\n",
        arch::x86_64::apic::SCHEDULER_HZ
    );

    // Ring3 süreç zamanlayıcısı sınavı: yield, kernel-izolasyon ve yığın
    // taşması senaryolarını gerçek CR3/context-switch yoluyla doğrular.
    let _ = arch::x86_64::ring3::scheduler_self_test();

    // Zamanlayici affinity uygulamasini ve kullanıcı sinyel isleyici
    // teslimat altyapisini dogrula.
    let _ = mm::process::test_scheduler_affinity_and_signals(
        &mut memory_mgr.proc_mgr,
        &mut memory_mgr.pmm,
    );

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
                        kdebug!(
                            "boot",
                            "[SMP] Paralel sağlama toplamı (AP+BSP) eşleşti: BAŞARILI\n"
                        );
                    }
                    Some(_) => {
                        kerror!(
                            "boot",
                            "[SMP] Paralel sağlama toplamı UYUŞMADI: BAŞARISIZ\n"
                        );
                    }
                    None => {
                        kwarn!("boot", "[SMP] Paralel sağlama toplamı zaman aşımı (tek çekirdek yedeği yok).\n");
                    }
                }
            }
            for i in 0..SUM_FRAMES {
                memory_mgr.pmm.free_frame(buf_phys + (i as u64) * 4096);
            }
        }
    }

    // 3. Donanim suruculeri: Limine boot menusu bir profil secer.
    timer.mark("smp");
    kinfo!("boot", "[BOOT] Starting hardware drivers...\n");
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
    kinfo!("boot", "[DRIVERS] Profil: {}\n", driver_profile);
    let mut profile_line = HeaplessString::<64>::new();
    let _ = write!(profile_line, "[DRIVERS] profile: {}", driver_profile);
    let _ = fs_mgr
        .vfs
        .write_file("/root/Documents/boot.log", profile_line.as_bytes());

    // DMA çerçeveleri BSP'de önceden ayrılır: PMM kilidi sayesinde hatlar
    // boyunca tek iş parçacıklı seri kalır ve ayrıcalıklı `&mut` paylaşımı
    // gerekmez. Tahsis profiline göre yalnızca gerekli hatlar beslenir.
    let mut hda_dma_phys = None;
    let mut e1000_dma_phys = None;
    if driver_profile == "audio" || driver_profile == "full" {
        hda_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(6);
        if hda_dma_phys.is_none() {
            boot::text_console::fatal("HDA DMA bellek tahsisi başarısız");
        }
    }
    if driver_profile == "hardware" || driver_profile == "full" {
        e1000_dma_phys = memory_mgr.pmm.allocate_contiguous_frames(34);
        if e1000_dma_phys.is_none() {
            boot::text_console::fatal("e1000 DMA bellek tahsisi başarısız");
        }
    }

    // Bağımsız sürücü hatları AP'lere dağıtılır; `parallel_run` tümü
    // bitene kadar bekler. Tek CPU'da aynı kod sırayla BSP'de koşar.
    let lane_args = DriverLaneArgs {
        hda_dma: hda_dma_phys,
        e1000_dma: e1000_dma_phys,
        // SAFETY: `pmm` ham işaretçisi yalnızca HID hattı tarafından,
        // `parallel_run` süresince başka hiçbir kod PMM'e erişmezken
        // tek seferde `&mut`'e çevrilir. PMM metodları ayrıca kendi
        // iç kilidini aldığı için çekirdek heap büyümesiyle çakışma
        // bile güvenlidir.
        pmm: &mut memory_mgr.pmm,
    };
    let arg_ptr = core::ptr::addr_of!(lane_args) as *mut u8;
    let lanes = [
        arch::x86_64::smp::ParallelTask {
            func: driver_lane_audio,
            arg: arg_ptr,
        },
        arch::x86_64::smp::ParallelTask {
            func: driver_lane_net,
            arg: arg_ptr,
        },
        arch::x86_64::smp::ParallelTask {
            func: driver_lane_usb,
            arg: arg_ptr,
        },
        arch::x86_64::smp::ParallelTask {
            func: driver_lane_hid,
            arg: arg_ptr,
        },
    ];
    let tasks: &[arch::x86_64::smp::ParallelTask] = match driver_profile {
        "audio" => &lanes[0..1],
        "hardware" => &lanes[1..4],
        "full" => &lanes[0..4],
        _ => &[],
    };
    kinfo!(
        "boot",
        "[BOOT] Starting parallel driver lanes (lane count: {}, CPU: {})\n",
        tasks.len(),
        arch::x86_64::smp::cpu_count()
    );
    arch::x86_64::smp::parallel_run(tasks);
    kinfo!("boot", "[DRIVERS] Parallel driver lanes completed.\n");

    let _socket_timeout_task = task::spawn("socket-timeouts", task::TaskPriority::Normal, || {
        drivers::socket::poll_timeouts();
        task::TaskStatus::Yielded
    });
    kdebug!(
        "boot",
        "[TASK] İşbirlikçi soket zaman aşımı görevi kuyruğa alındı.\n"
    );

    // Çalışma anı log seviyesi komutları: seri konsoldan `log ...` satırları.
    crate::logging::spawn_serial_console_task();

    let _net_dev = drivers::net::NetworkInterface::new();
    let mut audio_dev = drivers::audio::AudioDevice::new();
    audio_dev.test_audio_subsystem();

    kinfo!("boot", "[DRIVERS] All drivers ready.\n");
    timer.mark("suruculer");
    kinfo!(
        "net",
        "[NET] smoltcp protokolleri etkin: {}\n",
        crate::net_smoltcp::Protocols::summary()
    );

    // 4b. Gelismis grafik suruculeri: VirtIO-GPU varsayılan render yolu olarak seçiliyor.
    let t_gpu = crate::monotonic_ms();
    use crate::drivers::status::{self, DriverId};
    let t_disp = status::probe_start(DriverId::Display);
    let gpu_caps = drivers::gpu::init();
    let dt_gpu_us = (crate::monotonic_ms().saturating_sub(t_gpu)).saturating_mul(1000);
    crate::kernel::diag::record_driver_init("GPU", dt_gpu_us, true);
    if gpu_caps.vga_present {
        status::probe_ready(DriverId::Display, t_disp);
    } else {
        status::probe_unavailable(DriverId::Display, t_disp, "gpu aygiti bulunamadi");
    }
    let mut virtio_default = false;
    if gpu_caps.virtio_gpu_present && drivers::virtio_gpu::set_default_render_path() {
        kinfo!(
            "boot",
            "[GPU] VirtIO-GPU varsayılan render yolu olarak ayarlandı.\n"
        );
        fb.set_hw_cursor(false);
        kdebug!("boot", "[GPU] Hızlandırılmış yazılım imleci etkin.\n");
        virtio_default = true;
    }
    if !virtio_default && gpu_caps.bga_present {
        kinfo!("boot", "[GPU] BGA algılandı, çözünürlük değişimi açık.\n");
        fb.set_hw_cursor(false);
        kdebug!("boot", "[GPU] Hızlandırılmış yazılım imleci etkin.\n");
    } else {
        kdebug!(
            "boot",
            "[GPU] BGA yok; çözünürlük Limine ile sabit, yazılım imleci.\n"
        );
    }
    timer.mark("gpu");

    // 5. PS/2 Donanim Suruculeri
    let mouse = unsafe { &mut *core::ptr::addr_of_mut!(GLOBAL_MOUSE) };
    let keyboard = unsafe { &mut *core::ptr::addr_of_mut!(GLOBAL_KEYBOARD) };
    let t_ps2 = crate::monotonic_ms();
    let t_input = status::probe_start(DriverId::Input);
    drivers::ps2::init();
    let dt_ps2_us = (crate::monotonic_ms().saturating_sub(t_ps2)).saturating_mul(1000);
    crate::kernel::diag::record_driver_init("PS/2", dt_ps2_us, drivers::ps2::present());
    // Giriş ailesi: USB HID (hat 3) + PS/2 + dokunmatik birleşik durumu.
    if drivers::usb_hid::ready()
        || drivers::usb_hid::device_count() > 0
        || drivers::usb_touchscreen::max_x() > 0
        || drivers::ps2::present()
    {
        status::probe_ready(DriverId::Input, t_input);
    } else {
        status::probe_unavailable(DriverId::Input, t_input, "klavye/fare aygiti bulunamadi");
    }
    if drivers::ps2::present() {
        kdebug!("boot", "[BOOT] PS/2 ready.\n");
    } else {
        kdebug!(
            "boot",
            "[BOOT] PS/2 denetleyicisi yok; girdi yalnızca USB HID üzerinden.\n"
        );
    }
    timer.mark("ps2");

    let usb_hid_count = drivers::usb_hid::device_count();
    if usb_hid_count == 0 {
        kwarn!("boot", "[INPUT] USB HID bulunamadı, PS/2 yedeği kullanılıyor (BIOS Legacy USB/CSM kapalıysa klavye/fare algılanamayabilir).\n");
    } else {
        kdebug!(
            "boot",
            "[INPUT] USB HID etkin: bağlı aygıtlar kullanıma hazır.\n"
        );
    }

    // 5a. Girdi kuyruğu öz-testi: klavye/fare olaylarının FIFO sırası, uzunluk
    //     bilgisi ve kuyruk dolu davranışı doğrulanır (gerçek donanımdan bağımsız).
    if drivers::input_queue::self_test() {
        kdebug!("boot", "[INPUT-TEST] klavye/fare kuyrugu: BASARILI\n");
    } else {
        kerror!("boot", "[INPUT-TEST] klavye/fare kuyrugu: BAŞARISIZ\n");
    }

    // 5b. Tema motorunu başlat
    crate::gui::theme::init();
    kdebug!("boot", "[BOOT] Theme ready.\n");
    timer.mark("tema");

    // 5c. önyükleme modu denetimi:
    let wants_installed = boot_wants_installed();
    let installed = fs::diskfs::is_installed();
    kinfo!(
        "boot",
        "[BOOT] Kurulum denetimi: wants_installed={} installed={}\n",
        wants_installed as u8,
        installed as u8
    );

    if wants_installed && !installed {
        kerror!(
            "boot",
            "[BOOT] 'installed' modu istendi ancak diskte kurulum bulunamadı.\n"
        );
        boot::text_console::fatal("Kurulu sistem seçildi ancak MINOS.SYS bulunamadı");
    }

    // Sürücü çerçevesi: mevcut alt sistemleri ortak arayüze bağla ve otomatik
    // keşif/başlatma turunu çalıştır. Hazır olmayan adaptörler kayıtlı kalır.
    {
        use crate::drivers::adapters::{
            AudioAdapter, DisplayAdapter, InputAdapter, NetworkAdapter, StorageAdapter, UsbAdapter,
        };
        use crate::drivers::framework::{probe_and_init_all, register};

        register(alloc::boxed::Box::new(NetworkAdapter));
        register(alloc::boxed::Box::new(AudioAdapter));
        register(alloc::boxed::Box::new(UsbAdapter));
        register(alloc::boxed::Box::new(StorageAdapter));
        register(alloc::boxed::Box::new(DisplayAdapter));
        register(alloc::boxed::Box::new(InputAdapter));
        probe_and_init_all();
    }
    kinfo!(
        "boot",
        "[BOOT] Sürücü kayıt defteri hazır; otomatik keşif tamamlandı.\n"
    );
    timer.mark("surucu-kesfi");

    // Ortak sürücü durum modeli: hatların raporlamadığı sürücüleri tamamla,
    // PCI ve depolama denetleyicilerini probe et, özeti seri günlüğe yaz.
    crate::boot::devices::collect_driver_statuses();
    crate::boot::devices::log_status_summary();

    kinfo!(
        "boot",
        "[BOOT] All critical subsystems started. Loading GUI and applications...\n"
    );
    timer.mark("alt-sistemler");
    crate::boot::timing::report_total();

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

// ---------------------------------------------------------------------------
// Paralel sürücü hatları (SMP)
// ---------------------------------------------------------------------------
//
// `init_subsystems` içindeki bağımsız donanım grupları `smp::parallel_run`
// üzerinden AP'lere dağıtılır. Hatlar farklı aygıtlara dokunduğu için
// çakışmaz; paylaşılan kaynaklar (PMM, seri, PCI config) kendi kilitleriyle
// korunur. DMA çerçeveleri dağıtımdan önce BSP'de ayrılır.

/// Paralel hatlara verilen ortak argüman paketi. Yalnızca okunur alanlar
/// paylaşılır; `pmm` işaretçisi tek hat (HID) tarafından kullanılır.
struct DriverLaneArgs {
    /// HDA ses DMA'sı için ayrılan fiziksel çerçeve aralığı (ses profili).
    hda_dma: Option<u64>,
    /// NIC TX/RX halkaları için ayrılan fiziksel çerçeve aralığı (ağ profili).
    e1000_dma: Option<u64>,
    /// USB HID xHCI denetleyicisinin DMA tamponları için PMM.
    pmm: *mut mm::pmm::PhysicalMemoryManager,
}

/// Hat 0 — HDA ses sürücüsü (yalnızca `audio`/`full` profilinde DMA dolu).
unsafe fn driver_lane_audio(arg: *mut u8) {
    let Some(dma) = (unsafe { &*(arg as *const DriverLaneArgs) }).hda_dma else {
        return;
    };
    kinfo!("driver", "[DRIVER] Starting HDA audio driver...\n");
    let t0 = crate::monotonic_ms();
    drivers::hda::init(Some(dma));
    let dt_us = (crate::monotonic_ms().saturating_sub(t0)).saturating_mul(1000);
    crate::kernel::diag::record_driver_init("Intel HDA", dt_us, drivers::hda::is_ready());
    use crate::drivers::status::{self, DriverId};
    if drivers::hda::is_ready() {
        kinfo!("driver", "[DRIVER] HDA audio driver ready.\n");
        status::probe_ready(DriverId::Audio, t0);
    } else {
        match drivers::hda::init_outcome() {
            3 => status::probe_error(
                DriverId::Audio,
                t0,
                status::ERR_NO_DMA,
                "HDA DMA havuzu ayrilamadi",
            ),
            4 => status::probe_error(
                DriverId::Audio,
                t0,
                status::ERR_INIT_FAILED,
                "HDA denetleyici baslatma hatasi",
            ),
            _ => status::probe_unavailable(DriverId::Audio, t0, "HDA yok; PC Speaker yedegi etkin"),
        }
    }
}

/// Hat 1 — PCI/ISA ağ kartı + protokol yığını (yalnızca `hardware`/`full`).
unsafe fn driver_lane_net(arg: *mut u8) {
    let Some(dma) = (unsafe { &*(arg as *const DriverLaneArgs) }).e1000_dma else {
        return;
    };
    kinfo!(
        "driver",
        "[DRIVER] PCI ağ sürücüsü (e1000/VMXNET3/RTL816x/RTL8139) başlatılıyor...\n"
    );
    let t0 = crate::monotonic_ms();
    drivers::e1000::init(Some(dma));
    let dt_us = (crate::monotonic_ms().saturating_sub(t0)).saturating_mul(1000);
    if drivers::e1000::is_ready() {
        crate::kernel::diag::record_driver_init("PCI NIC (e1000)", dt_us, true);
        kinfo!(
            "driver",
            "[DRIVER] PCI ağ sürücüsü hazır: {}\n",
            drivers::e1000::nic_name()
        );
    } else {
        // PCI kart yoksa NE2000 (ISA/PnP) denenir.
        kinfo!("driver", "[DRIVER] NE2000 ağ sürücüsü başlatılıyor...\n");
        let t_isa = crate::monotonic_ms();
        let ok_isa = drivers::ne2000::init_isa_pnp();
        let dt_isa_us = (crate::monotonic_ms().saturating_sub(t_isa)).saturating_mul(1000);
        crate::kernel::diag::record_driver_init("NE2000 (ISA)", dt_isa_us, ok_isa);
        if ok_isa {
            kinfo!("driver", "[DRIVER] NE2000 ağ sürücüsü hazır.\n");
        } else {
            kwarn!("driver", "[DRIVER] Ağ adaptörü bulunamadı (PCI/ISA).\n");
        }
    }

    kinfo!("driver", "[DRIVER] Ağ protokolü başlatılıyor...\n");
    let t_stack = crate::monotonic_ms();
    drivers::netstack::init();
    let _ = drivers::netstack::dhcp_request();
    drivers::netstack::run_network_self_test();
    let dt_stack_us = (crate::monotonic_ms().saturating_sub(t_stack)).saturating_mul(1000);
    crate::kernel::diag::record_driver_init("Netstack", dt_stack_us, true);
    // `livetest` işlemleri boot sırasında devre dışı bırakıldı.
    // drivers::netstack::run_live_server_tests();
    kinfo!("driver", "[DRIVER] Ağ protokolü hazır.\n");

    // Ortak durum modeli: hazır / desteklenmiyor / başlatma hatası ayrımı.
    use crate::drivers::status::{self, DriverId};
    if drivers::e1000::is_ready() {
        status::probe_ready(DriverId::Net, t0);
    } else {
        match drivers::e1000::init_outcome() {
            3 => status::probe_error(
                DriverId::Net,
                t0,
                status::ERR_NO_DMA,
                "DMA ring havuzu ayrilamadi",
            ),
            4 => status::probe_error(
                DriverId::Net,
                t0,
                status::ERR_INIT_FAILED,
                "ag surucusu baslatma hatasi",
            ),
            _ => status::probe_unavailable(DriverId::Net, t0, "PCI/ISA ag adaptoru bulunamadi"),
        }
    }
}

/// Hat 2 — USB ana makine denetleyicileri + Wi-Fi algılama
/// (yalnızca `hardware`/`full`).
unsafe fn driver_lane_usb(arg: *mut u8) {
    if (unsafe { &*(arg as *const DriverLaneArgs) })
        .e1000_dma
        .is_none()
    {
        return;
    }
    kinfo!("driver", "[DRIVER] Starting USB driver...\n");
    use crate::drivers::status::{self, DriverId};
    let t_usb_status = status::probe_start(DriverId::Usb);
    let t_usb = crate::monotonic_ms();
    drivers::usb::init();
    drivers::usb::test_usb_subsystems();
    let dt_usb_us = (crate::monotonic_ms().saturating_sub(t_usb)).saturating_mul(1000);
    crate::kernel::diag::record_driver_init("USB Host", dt_usb_us, true);
    kinfo!("driver", "[DRIVER] Starting Wi-Fi (802.11) subsystem...\n");
    let t_wifi = crate::monotonic_ms();
    drivers::wifi::init();
    let dt_wifi_us = (crate::monotonic_ms().saturating_sub(t_wifi)).saturating_mul(1000);
    crate::kernel::diag::record_driver_init("Wi-Fi", dt_wifi_us, true);
    kinfo!("driver", "[DRIVER] USB driver ready.\n");
    if drivers::usb::controller_info().is_some() {
        status::probe_ready(DriverId::Usb, t_usb_status);
    } else {
        status::probe_unavailable(
            DriverId::Usb,
            t_usb_status,
            "USB host denetleyicisi bulunamadi",
        );
    }
}

/// Hat 3 — xHCI HID (klavye/fare/dokunmatik) taraması; PMM'e tek erişen hat.
/// `usb::init`'ten bağımsızdır: kendi PCI taramasını yapar.
unsafe fn driver_lane_hid(arg: *mut u8) {
    let args = unsafe { &*(arg as *const DriverLaneArgs) };
    kinfo!("driver", "[DRIVER] Starting USB HID (xHCI) driver...\n");
    // SAFETY: `args->pmm` `parallel_run` çağrısı süresince tek erişimlidir;
    // PMM metodları iç kilitle korunur.
    drivers::usb_hid::init(unsafe { &mut *args.pmm });
    if drivers::usb_touchscreen::init() {
        kinfo!(
            "driver",
            "[DRIVER] USB dokunmatik ekran hazır ({}x{}{}).\n",
            drivers::usb_touchscreen::max_x(),
            drivers::usb_touchscreen::max_y(),
            if drivers::usb_touchscreen::supports_multitouch() {
                ", multi-touch"
            } else {
                ""
            }
        );
    }
    kinfo!("driver", "[DRIVER] USB HID driver ready.\n");
}
