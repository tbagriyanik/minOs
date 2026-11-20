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
/// Zamanlayıcı tik frekansı. Her tik tam yazmaç + FPU (fxsave/fxrstor)
/// kaydı ve Rust işleyicisi çalıştırır; 250 Hz (4 ms) 1000 Hz'e göre
/// kesme yükünü 4× azaltır ve boşta HLT'yi derinleştirir. Duvar saati TSC
/// tabanlı `monotonic_ms`'dir; bu değer yalnızca kuantum/kesme sıklığını
/// belirler. 4 ms'lik üst sınır masaüstü tepkiselliği için yeterlidir.
pub const SCHEDULER_HZ: u32 = 250;

// Local APIC register offset'leri
const LAPIC_TIMER_DIVIDE: u32 = 0x3E0;
const LAPIC_LVT_TIMER: u32 = 0x320;
const LAPIC_TIMER_INIT_CNT: u32 = 0x380;
const LAPIC_TIMER_CUR_CNT: u32 = 0x390;
const LAPIC_EOI: u32 = 0x0B0;

#[inline(always)]
fn lapic_virt() -> *mut u32 {
    let base = crate::arch::x86_64::smp::lapic_phys_base();
    (crate::drivers::phys_to_virt(base)) as *mut u32
}

#[allow(dead_code)]
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

/// Local APIC timer saatini TSC'ye karşı ölçer ve [`SCHEDULER_HZ`] için
/// gereken periyot sayacını döndürür.
///
/// LAPIC timer saati sabit değildir: sanal makinelerde ~1 GHz, fiziksel
/// makinelerde ~100 MHz mertebesinde değişir. `0x100000` gibi sabit bir
/// değer gerçek donanımda ya çok seyrek (periyot >100 ms → `enable_bsp_idle`
/// doğrulaması düşer, masaüstü 1 ms busy-wait'e döner ve CPU %100'de kalır)
/// ya da çok sık (kesme fırtınası) olur. Bu yüzden sayaç TSC ile ölçülür.
/// Her CPU yalnızca kendi LAPIC kaydını kullanır; eşzamanlı çağrı güvenlidir.
fn calibrate_initial_count() -> u32 {
    let tsc_per_us = unsafe { crate::boot::time::TSC_CYCLES_PER_US }.max(1);
    // Tek atımlık, maskeli modda sayacı başlatıp CUR_CNT azalmasını ölç.
    lapic_write(LAPIC_TIMER_DIVIDE, 0x03);
    lapic_write(LAPIC_LVT_TIMER, (1 << 16) | (APIC_TIMER_VECTOR as u32));
    lapic_write(LAPIC_TIMER_INIT_CNT, u32::MAX);

    let c0 = lapic_read(LAPIC_TIMER_CUR_CNT);
    let t0 = crate::boot::time::read_cpu_cycles();
    let window = tsc_per_us.saturating_mul(20_000).max(1);
    while crate::boot::time::read_cpu_cycles().wrapping_sub(t0) < window {
        core::hint::spin_loop();
    }
    let c1 = lapic_read(LAPIC_TIMER_CUR_CNT);
    let tsc_delta = crate::boot::time::read_cpu_cycles().wrapping_sub(t0).max(1);
    let counts = c0.wrapping_sub(c1);
    if counts < 100 {
        // Ölçüm başarısız (timer saymıyor): eski varsayılanı koru.
        return 0x100000;
    }
    // İstenen periyot = 1/SCHEDULER_HZ saniye. Eskiden burada 1_000 µs
    // sabitti; SCHEDULER_HZ değişse bile tik 1 ms'de kalırdı.
    let period_us = 1_000_000u64 / SCHEDULER_HZ.max(1) as u64;
    let period_ticks = tsc_per_us.saturating_mul(period_us);
    let per_tick = (counts as u128).saturating_mul(period_ticks as u128) / tsc_delta as u128;
    (per_tick.clamp(1, u32::MAX as u128) as u32).max(1)
}

/// APIC Timer'ı başlatır ve IDT vektör 0x20'ye bağlar.
/// BSP ve her AP kendi LAPIC'ini kurar; sayaç TSC kalibrasyonuyla
/// [`SCHEDULER_HZ`] hedefine getirilir.
pub fn init_timer() {
    // 1. IDT vektörüne timer kapısını kur
    unsafe {
        let idt = &mut *core::ptr::addr_of_mut!(super::IDT);
        idt.entries[APIC_TIMER_VECTOR as usize]
            .set_handler_dpl(apic_timer_entry as *const () as usize, 0);
    }

    let initial_count = calibrate_initial_count();

    // 2. Local APIC Timer yapılandır:
    // Bölücü: Divide by 16 (0x3)
    lapic_write(LAPIC_TIMER_DIVIDE, 0x03);

    // LVT Timer: Periodic mod (bit 17 = 1) + Vektör 0x20
    // Unmask: bit 16 = 0
    let lvt_val = (1 << 17) | (APIC_TIMER_VECTOR as u32);
    lapic_write(LAPIC_LVT_TIMER, lvt_val);

    // Başlangıç sayacı: TSC kalibrasyonuyla ~1 ms periyot.
    lapic_write(LAPIC_TIMER_INIT_CNT, initial_count);
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
/// tikinde (≈10 ms) veya PS/2/USB girdi kesmesinde CPU uyanır. Timer
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
    // APIC timer ~100 Hz (10 ms) çalışabilir; 10 ms'lik yoklama yanlış negatif
    // üretmesin diye biraz daha fazla (100 ms) bekle. Daha agresif zaman aşımı
    // CI ortamlarında (Azure VM, TCG) timer interrupt'in gecikmesine sebep olur.
    let timeout = unsafe { crate::boot::time::TSC_CYCLES_PER_US }
        .saturating_mul(100_000)
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
/// Timer doğrulanamasa bile masaüstünü sonsuza kadar durdurmamak için sınırlı
/// TSC beklemesi uygularız. HLT burada kullanılmaz: timer kesmesi gerçekten
/// yoksa HLT'den geri dönüş garantisi yoktur.
pub fn idle_pause() {
    if BSP_IDLE_HLT.load(Ordering::Acquire) {
        unsafe {
            core::arch::asm!("sti", "hlt", options(nomem, nostack, preserves_flags));
        }
    } else {
        // enable_bsp_idle() başarısız olduğunda özgün IF durumu kapalı
        // olabilir. Masaüstü polling yapsa bile PS/2/USB IRQ'larının ve ağ
        // olaylarının teslim edilebilmesi için burada tekrar açılır.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
        }
        let start = crate::boot::time::read_cpu_cycles();
        let wait = unsafe { crate::boot::time::TSC_CYCLES_PER_US }
            .saturating_mul(1_000)
            .max(1);
        while crate::boot::time::read_cpu_cycles().wrapping_sub(start) < wait {
            core::hint::spin_loop();
        }
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
