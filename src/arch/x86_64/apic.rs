//! APIC Timer ve Kesme Alt Sistemi (APIC Timer & Interrupt Handling)
//!
//! Bu modül:
//! 1. Local APIC Timer başlatımı (tek seferlik / periyodik mod).
//! 2. IDT üzerinde Timer kesme vektörünün (0x20) kaydedilmesi.
//! 3. Her zamanlayıcı tikinde (timer tick) CPU başına `TICKS_PER_CPU` sayacının artırılması.
//! 4. Preemptive çoklu görev (preemptive context switch) desteği için EOI ve hook çağrısı.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// CPU başına APIC Timer tik sayısı. SMP altında her çekirdeğin kendi
/// Local APIC timer'ı çalışır; tek bir global sayaç olsaydı `get_ticks()`
/// ve `enable_bsp_idle()` N kat hızlı/yanlış değer görürdü. Yalnızca
/// BSP'nin (index 0) tikleri `bsp_timer_ticks()` üzerinden doğrulanır.
static TICKS_PER_CPU: [AtomicU64; super::smp::MAX_CPUS] =
    [const { AtomicU64::new(0) }; super::smp::MAX_CPUS];

/// BSP'de boşta `hlt` güvenli mi? Periyodik timer kesmesinin gerçekten
/// teslim edildiği `enable_bsp_idle()` tarafından doğrulanır.
static BSP_IDLE_HLT: AtomicBool = AtomicBool::new(false);

/// APIC Timer Kesme Vektörü (IDT vektör 32)
pub const APIC_TIMER_VECTOR: u8 = 0x20;
pub const SCHEDULER_HZ: u32 = 1000;

/// APIC timer bus frekansı (kalibrasyon sonrası hesaplanır)
static mut APIC_BUS_FREQ: u64 = 0;

// Local APIC register offset'leri
const LAPIC_TIMER_DIVIDE: u32 = 0x3E0;
/// `LAPIC_TIMER_DIVIDE = 0x03` → zamanlayıcı saati 16'ya bölünür.
const LAPIC_TIMER_DIVIDE_VALUE: u64 = 16;
const LAPIC_LVT_TIMER: u32 = 0x320;
const LAPIC_TIMER_INIT_CNT: u32 = 0x380;
const LAPIC_EOI: u32 = 0x0B0;

#[inline(always)]
fn lapic_virt() -> *mut u32 {
    let base = crate::arch::x86_64::smp::lapic_phys_base();
    (crate::drivers::phys_to_virt(base)) as *mut u32
}

#[inline(always)]
fn lapic_read(reg: u32) -> u32 {
    unsafe { core::ptr::read_volatile(lapic_virt().byte_add(reg as usize)) }
}

#[inline(always)]
fn lapic_write(reg: u32, val: u32) {
    unsafe { core::ptr::write_volatile(lapic_virt().byte_add(reg as usize), val) }
}

/// APIC EOI (End of Interrupt) sinyali gönderir.
#[inline(always)]
pub fn send_eoi() {
    lapic_write(LAPIC_EOI, 0);
}

/// Timer kesmesi geldiğinde çağrılan Rust işleyicisi.
///
/// `frame`, naked `apic_timer_entry` tarafından hazırlanan tam yazmaç +
/// CPU çerçevesini gösterir; `ring3::timer_preempt` Ring3 süreçlerini
/// önalım için kullanır.
/// # Safety
/// `frame`, `apic_timer_entry` tarafından hazırlanan geçerli kesme
/// çerçevesine işaret etmelidir; yalnızca kesme bağlamında çağrılmalıdır.
#[no_mangle]
pub unsafe extern "C" fn apic_timer_handler_rust(frame: *mut u64) {
    let cpu = super::smp::current_cpu_index().min(super::smp::MAX_CPUS - 1);
    TICKS_PER_CPU[cpu].fetch_add(1, Ordering::Relaxed);
    crate::task::scheduler_tick();
    crate::watchdog::timer_tick();
    // Ring3 kullanıcı süreçleri için gerçek önalım: quantum dolmuşsa tam
    // bağlam kaydedilir ve kesme çekirdek trampoline'ine yönlendirilir.
    unsafe { crate::arch::x86_64::ring3::timer_preempt(frame) };
    send_eoi();
}

/// Naked interrupt kapısı (tüm yazmaçları korur, Rust işleyicisini çağırır, EOI sonrası iretq ile döner).
/// Çerçeve yerleşimi `ring3::int80_entry` ile aynıdır: [0]=rax .. [14]=r15,
/// index 15 = rip, index 16 = cs, index 17 = rflags, index 18 = rsp, index 19 = ss.
///
/// # Safety
/// Donanım kesmesi tarafından IDT üzerinden çağrılır; Rust kodundan doğrudan
/// çağrılmamalıdır.
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn apic_timer_entry() {
    core::arch::naked_asm!(
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push r11",
        "push r10",
        "push r9",
        "push r8",
        "push rbp",
        "push rdi",
        "push rsi",
        "push rdx",
        "push rcx",
        "push rbx",
        "push rax",
        "mov r12, rsp",
        "and rsp, -16",
        "sub rsp, 512",
        "fxsave64 [rsp]",
        "mov rdi, r12",
        "call {handler}",
        "fxrstor64 [rsp]",
        "mov rsp, r12",
        "pop rax",
        "pop rbx",
        "pop rcx",
        "pop rdx",
        "pop rsi",
        "pop rdi",
        "pop rbp",
        "pop r8",
        "pop r9",
        "pop r10",
        "pop r11",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        "iretq",
        handler = sym apic_timer_handler_rust,
        options(),
    );
}

/// APIC timer bus frekansını kalibre eder. TSC ve known timer (PIT kanal 2)
/// kullanarak Local APIC bus frekansını ölçer.
///
/// # Returns
/// Kalibre edilmiş bus frekansı (Hz). Başarısız olursa 0 döner.
///
/// # TR
/// APIC timer bus frekansını kalibre eder. TSC ve known timer (PIT kanal 2)
/// kullanarak Local APIC bus frekansını ölçer.
fn calibrate_apic_timer() -> u64 {
    // TSC'nin kalibre edilmiş olduğundan emin ol
    if !crate::boot::time::is_ready() {
        return 0;
    }

    let cycles_per_us = unsafe { crate::boot::time::TSC_CYCLES_PER_US }.max(1);

    // APIC timer'ı one-shot modda başlat (önceki ayarları geçersiz kıl)
    lapic_write(LAPIC_TIMER_DIVIDE, 0x03); // Divide by 16
    let lvt_val = APIC_TIMER_VECTOR as u32; // One-shot (bit 17 = 0)
    lapic_write(LAPIC_LVT_TIMER, lvt_val);

    // Ölçüm süresi: 10 ms (tsc_cycles = 10_000 us)
    let measure_us = 10_000u64;
    let measure_cycles = cycles_per_us.saturating_mul(measure_us);

    // İlk sayımı çok yüksek bir değer ile başlat (ölçüm süresinden uzun)
    lapic_write(LAPIC_TIMER_INIT_CNT, 0xFFFF_FFFF);

    let start_tsc = crate::boot::time::read_cpu_cycles();

    // Timer'ı başlat ve ilk decrement'i bekle
    let initial_count = lapic_read(LAPIC_TIMER_INIT_CNT);

    // Belirlenen süre boyunca bekle
    let mut elapsed = 0u64;
    while elapsed < measure_cycles {
        elapsed = crate::boot::time::read_cpu_cycles().wrapping_sub(start_tsc);
        core::hint::spin_loop();
    }

    let final_count = lapic_read(LAPIC_TIMER_INIT_CNT);
    let count_delta = initial_count.saturating_sub(final_count);

    if count_delta == 0 {
        return 0;
    }

    // Ölçüm ÷16 bölücü açıkken yapıldı; `count_delta / measure_us` sayacın
    // gerçek düşüş hızıdır (bus_freq / 16). Gerçek bus frekansına çevirmek
    // için bölücüyle çarpılır — aksi halde `init_timer` aynı 16'yı bir kez
    // daha böler ve timer ~16 kHz'de çalışır (hedefin 16 katı kesme!).
    let rate = (count_delta as u64).saturating_mul(1_000_000) / measure_us;
    rate.saturating_mul(LAPIC_TIMER_DIVIDE_VALUE)
}

/// APIC Timer'ı başlatır ve IDT vektör 0x20'ye bağlar (yalnızca BSP çağırmalı).
pub fn init_timer() {
    // 1. IDT vektörüne timer kapısını kur
    unsafe {
        let idt = &mut *core::ptr::addr_of_mut!(super::IDT);
        idt.entries[APIC_TIMER_VECTOR as usize]
            .set_handler_dpl(apic_timer_entry as *const () as usize, 0);
    }

    // 2. APIC timer bus frekansını kalibre et
    let bus_freq = calibrate_apic_timer();
    if bus_freq == 0 {
        // Kalibrasyon başarısız oldu, fallback olarak varsayılan değer kullan
        // Bu değer genellikle 1-100 MHz aralığındadır
        unsafe {
            APIC_BUS_FREQ = 1_000_000;
        } // 1 MHz fallback
    } else {
        unsafe {
            APIC_BUS_FREQ = bus_freq;
        }
    }

    // 3. Local APIC Timer yapılandır:
    // Bölücü: Divide by 16 (0x3)
    lapic_write(LAPIC_TIMER_DIVIDE, 0x03);

    // LVT Timer: Periodic mod (bit 17 = 1) + Vektör 0x20
    // Unmask: bit 16 = 0
    let lvt_val = (1 << 17) | (APIC_TIMER_VECTOR as u32);
    lapic_write(LAPIC_LVT_TIMER, lvt_val);

    // Başlangıç sayacı (Initial Count): SCHEDULER_HZ'ye göre hesapla
    // Formül: initial_count = bus_freq / (divide_value * SCHEDULER_HZ)
    // divide_value = 16 (LAPIC_TIMER_DIVIDE = 0x3)
    let bus_freq = unsafe { APIC_BUS_FREQ };
    let divide_value = LAPIC_TIMER_DIVIDE_VALUE;
    let initial_count = bus_freq / (divide_value * SCHEDULER_HZ as u64);
    lapic_write(LAPIC_TIMER_INIT_CNT, initial_count as u32);

    // Tanılama için yazdır
    let actual_hz = bus_freq / divide_value;
    crate::serial_print("[APIC] Bus frekansı: ");
    crate::serial_print_dec(actual_hz);
    crate::serial_print(" Hz, scheduler: ");
    crate::serial_print_dec(SCHEDULER_HZ as u64);
    crate::serial_print(" Hz, initial count: ");
    crate::serial_print_dec(initial_count);
    crate::serial_print("\n");
}

/// Tüm CPU'ların toplam tik sayısı (tanılama amaçlı).
pub fn get_ticks() -> u64 {
    TICKS_PER_CPU
        .iter()
        .fold(0u64, |acc, t| acc.saturating_add(t.load(Ordering::Relaxed)))
}

/// Yalnızca BSP'nin (CPU 0) aldığı timer tik sayısı. `enable_bsp_idle`
/// doğrulaması AP tiklerini saymamalıdır; aksi halde AP timer'ı çalışıp
/// BSP'ninki bozuksa bile doğrulama yanlışlıkla başarılı dönerdi.
fn bsp_timer_ticks() -> u64 {
    TICKS_PER_CPU[0].load(Ordering::Relaxed)
}

/// BSP'de boşta `hlt` kullanılabilir mi? (timer kesmesi doğrulandı)
pub fn bsp_idle_hlt_enabled() -> bool {
    BSP_IDLE_HLT.load(Ordering::Acquire)
}

/// BSP kesmelerini açar ve periyodik APIC timerinin gerçekten teslim
/// edildiğini doğrular.
///
/// Başarılıysa `true` döner ve boşta `hlt` güvenlidir: bir sonraki APIC
/// tikinde (≈1 ms @ 1000 Hz) veya PS/2/USB girdi kesmesinde CPU uyanır. Timer
/// yönlendirmesi olmayan ortamlarda (ör. bazı hipervizörler) 25 ms içinde tik
/// gelmezse kesmeler özgün IF durumuna döndürülür ve `false` döner; ana döngü
/// yoklamalı boşta beklemeye devam eder. Böylece HLT'nin sistemi kilitlemesi
/// ve PS/2 IRQ modunun bozulması engellenir.
pub fn enable_bsp_idle() -> bool {
    if BSP_IDLE_HLT.load(Ordering::Acquire) {
        return true;
    }
    let ticks_before = bsp_timer_ticks();
    let start = crate::boot::time::read_cpu_cycles();
    // Kesmeler zaten açık olabilir (PS/2 IRQ modu). Özgün IF durumunu sakla:
    // yoklama başarısız olursa açtığımız kesmeleri körü körüne kapatmayız,
    // aksi halde klavye/fare IRQ'ları sessizce kesilir.
    let flags_before: u64;
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) flags_before, options(nomem));
    }
    let interrupts_were_enabled = flags_before & (1 << 9) != 0;
    // Kesmeleri aç: STI sonrası ilk komut HLT olsa bile bekleyen kesme kaybolmaz.
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
    // APIC timer SCHEDULER_HZ (1000 Hz = 1 ms) çalışabilir; birkaç tikten
    // biraz fazla (25 ms) bekle.
    let timeout = unsafe { crate::boot::time::TSC_CYCLES_PER_US }
        .saturating_mul(25_000)
        .max(1);
    while bsp_timer_ticks() == ticks_before
        && crate::boot::time::read_cpu_cycles().wrapping_sub(start) < timeout
    {
        core::hint::spin_loop();
    }
    let advanced = bsp_timer_ticks() != ticks_before;
    if !advanced && !interrupts_were_enabled {
        unsafe {
            core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
        }
    }
    BSP_IDLE_HLT.store(advanced, Ordering::Release);
    advanced
}

/// Boşta bekleme noktası: doğrulanmışsa sonraki kesmeye kadar CPU'yu uyutur.
///
/// `hlt` yalnızca `enable_bsp_idle()` başarılıysa çalıştırılır; aksi halde
/// yalnızca `pause` yürütülür ve eski yoklamalı davranış korunur.
pub fn idle_pause() {
    if BSP_IDLE_HLT.load(Ordering::Acquire) {
        // STI+HLT klasik boşta bekleme dizisi: bekleyen kesme HLT'yi uyandırır,
        // kesme maskesi açık kalır. `cli` yok — kesme sonrası iretq IF'i geri yükler.
        unsafe {
            core::arch::asm!("sti", "hlt", options(nomem, nostack, preserves_flags));
        }
    } else {
        core::hint::spin_loop();
    }
}

/// AP çekirdekleri için çağrılır: bu CPU'daki Local APIC kesmelerini maskeler.
/// BSP `init_timer()` ile sayacı çalıştırır; AP'ler sayaç kesmesi almamalı.
/// Aksi halde VirtualBox AP'lere de timer/broadcast kesmesi ulaştırır ve AP'de
/// LIDT yapılmamışsa ya da işleyici BSP'ye özel bir duruma dokunursa sistem
/// donar. Bu fonksiyon AP'nin spurious vektörünü açık tutup LVT girdilerini
/// maskeli bırakır ve kesmeleri kapalı tutar.
pub fn mask_for_ap() {
    // LVT Timer: maskeli (bit 16 = 1), vektör 0x20.
    lapic_write(LAPIC_LVT_TIMER, (1 << 16) | (APIC_TIMER_VECTOR as u32));
    // LVT Thermal (0x330), LINT0 (0x350), LINT1 (0x360), Error (0x370): maskele.
    lapic_write(0x330, 1 << 16);
    lapic_write(0x350, 1 << 16);
    lapic_write(0x360, 1 << 16);
    lapic_write(0x370, 1 << 16);
    // Varsa bekleyen kesmeyi temizle.
    send_eoi();
}
