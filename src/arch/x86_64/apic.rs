//! APIC Timer ve Kesme Alt Sistemi (APIC Timer & Interrupt Handling)
//!
//! Bu modül:
//! 1. Local APIC Timer başlatımı (tek seferlik / periyodik mod).
//! 2. IDT üzerinde Timer kesme vektörünün (0x20) kaydedilmesi.
//! 3. Her zamanlayıcı tikinde (timer tick) `SYSTEM_TICKS` sayacının artırılması.
//! 4. Preemptive çoklu görev (preemptive context switch) desteği için EOI ve hook çağrısı.

use core::sync::atomic::{AtomicU64, Ordering};

/// Sistem başlangıcından bu yana geçen APIC Timer tik sayısı.
pub static SYSTEM_TICKS: AtomicU64 = AtomicU64::new(0);

/// APIC Timer Kesme Vektörü (IDT vektör 32)
pub const APIC_TIMER_VECTOR: u8 = 0x20;

// Local APIC register offset'leri
const LAPIC_TIMER_DIVIDE: u32 = 0x3E0;
const LAPIC_LVT_TIMER: u32 = 0x320;
const LAPIC_TIMER_INIT_CNT: u32 = 0x380;
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
#[no_mangle]
pub extern "C" fn apic_timer_handler_rust() {
    SYSTEM_TICKS.fetch_add(1, Ordering::Relaxed);
    send_eoi();
}

/// Naked interrupt kapısı (tüm yazmaçları korur, Rust işleyicisini çağırır, EOI sonrası iretq ile döner).
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn apic_timer_entry() {
    core::arch::naked_asm!(
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "call {handler}",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",
        "iretq",
        handler = sym apic_timer_handler_rust,
        options(),
    );
}

/// APIC Timer'ı başlatır ve IDT vektör 0x20'ye bağlar (yalnızca BSP çağırmalı).
pub fn init_timer() {
    // 1. IDT vektörüne timer kapısını kur
    unsafe {
        let idt = &mut *core::ptr::addr_of_mut!(super::IDT);
        idt.entries[APIC_TIMER_VECTOR as usize].set_handler_dpl(apic_timer_entry as *const () as usize, 0);
    }

    // 2. Local APIC Timer yapılandır:
    // Bölücü: Divide by 16 (0x3)
    lapic_write(LAPIC_TIMER_DIVIDE, 0x03);

    // LVT Timer: Periodic mod (bit 17 = 1) + Vektör 0x20
    // Unmask: bit 16 = 0
    let lvt_val = (1 << 17) | (APIC_TIMER_VECTOR as u32);
    lapic_write(LAPIC_LVT_TIMER, lvt_val);

    // Başlangıç sayacı (Initial Count): ~100 Hz veya 10 ms aralık
    lapic_write(LAPIC_TIMER_INIT_CNT, 0x100000);
}

/// Mevcut tik sayısını okur.
pub fn get_ticks() -> u64 {
    SYSTEM_TICKS.load(Ordering::Relaxed)
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
