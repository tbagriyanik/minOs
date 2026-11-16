//! PCID (Process-Context Identifier) yönetimi.
//!
//! PCID, her adres uzayına 12 bitlik bir tanımlayıcı verir; `CR3[11:0]`
//! PCID'yi, `CR3[51:12]` PML4 tabanını taşır. `CR4.PCIDE = 1` iken
//! `MOV CR3` operand'ının bit 63'ü set ise işlemci TLB ve sayfa yapısı
//! önbelleklerini **hiçbir PCID için** geçersiz kılmaz; böylece süreç
//! değişiminde önceki sürecin TLB kayıtları korunur (SDM 4.10.4.1).
//!
//! Güvenlik kuralı: bir PCID'ye karşılık gelen sayfa tabloları değiştiğinde
//! (süreç çıkışı, `exec` adres uzayı değişimi) ilgili PCID `invalidate()`
//! ile temizlenmelidir; aksi halde serbest bırakılan çerçevelere işaret
//! eden bayat TLB kayıtları kalır. Çalışan sürecin kendi tablo
//! değişiklikleri `invlpg` ile zaten geçersiz kılınır (o anki PCID).
//!
//! PCID desteklenmiyorsa tüm yardımcılar eski davranışa (PCID'siz, her
//! `MOV CR3` tam flush) döner; `encode()` bit63'ü asla kurmaz.

use core::sync::atomic::{AtomicBool, Ordering};

/// `CR4.PCIDE` (bit 17): PCID alanı etkin.
const CR4_PCIDE: u64 = 1 << 17;

/// `CR3` bit 63: PCIDE=1 iken 1 yazılırsa `MOV CR3` flush yapmaz.
pub const CR3_NOFLUSH: u64 = 1 << 63;

/// CR3'ün fiziksel PML4 + PCID taşıyan geçerli bitleri (0..=51).
const CR3_VALID_MASK: u64 = 0x000F_FFFF_FFFF_FFFF;

/// Çekirdek adres uzayının PCID'si. Kullanıcı süreçleri 1..=MAX_PROCESSES
/// aralığını kullanır; 0 her zaman çekirdek/ Limine-devralınan uzaydır.
pub const KERNEL_PCID: u16 = 0;

static ENABLED: AtomicBool = AtomicBool::new(false);
static INVPCID_OK: AtomicBool = AtomicBool::new(false);

/// CPUID.01H:ECX.PCID (bit 17): donanım PCID desteği.
pub fn supported() -> bool {
    let leaf = core::arch::x86_64::__cpuid_count(1, 0);
    (leaf.ecx & (1 << 17)) != 0
}

/// CPUID.07H.0:EBX.INVPCID (bit 10): PCID bazlı TLB geçersizleştirme.
fn invpcid_supported() -> bool {
    let leaf = core::arch::x86_64::__cpuid_count(7, 0);
    (leaf.ebx & (1 << 10)) != 0
}

/// PCID altyapısı etkin mi? (`CR4.PCIDE` kurulu ve doğrulanmış)
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// `INVPCID` komutu kullanılabilir mi?
pub fn invpcid_enabled() -> bool {
    INVPCID_OK.load(Ordering::Acquire)
}

/// PCID'yi algılar, `CR4.PCIDE` bitini kurar ve etkinleştirmeyi doğrular.
///
/// `CR4.PCIDE` 0→1 geçişi TLB'yi geçersiz kılmaz (SDM: yalnız 1→0 geçişi
/// ve `CR4.PGE` değişimi tam flush üretir), bu yüzden mevcut TLB
/// kayıtları PCID 0 olarak korunur. Dönüş: `true` ise PCID etkin.
pub fn init() -> bool {
    if !supported() {
        return false;
    }
    let active;
    unsafe {
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        cr4 |= CR4_PCIDE;
        core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        active = (cr4 & CR4_PCIDE) != 0;
    }
    ENABLED.store(active, Ordering::Release);
    INVPCID_OK.store(active && invpcid_supported(), Ordering::Release);
    active
}

/// `pml4_phys` ve `pcid` için `MOV CR3` operandı üretir.
///
/// * PCID kapalıyken: `pml4_phys` aynen döner (eski davranış, tam flush).
/// * `flush = false`: bit63 set → TLB korunur (normal süreç değişimi).
/// * `flush = true`: bit63 temiz → yalnız bu PCID'nin non-global TLB ve
///   sayfa yapısı önbellek kayıtları geçersiz kılınır. Aynı PCID altında
///   yeni bir adres uzayı devreye girerken (ör. `exec`) kullanılır.
pub fn encode(pml4_phys: u64, pcid: u16, flush: bool) -> u64 {
    let enabled = ENABLED.load(Ordering::Acquire);
    // CR4.PCIDE=0 iken CR3[11:0] rezerve sayılır; PCID bitlerini yazmak
    // #GP üretir. Bu yüzden PCID yalnızca gerçekten etkinse kodlanır.
    let pcid_bits = if enabled { u64::from(pcid) & 0xFFF } else { 0 };
    let mut value = (pml4_phys & 0x000F_FFFF_FFFF_F000) | pcid_bits;
    if enabled && !flush {
        value |= CR3_NOFLUSH;
    }
    value
}

/// `MOV CR3` ile okunan değeri güvenli `ProcessContext.cr3` alanına çevirir.
///
/// Okunan değerin bit63'ü platforma göre değişebildiği için (AMD'de
/// saklanmaz) burada sıfırlanır: kayıtlı bağlam geri yüklendiğinde hedef
/// PCID'nin kayıtları flush edilir; bu da süreç tabloları o arada
/// değişmiş olabileceği için her zaman güvenlidir.
pub fn sanitize_saved_cr3(raw: u64) -> u64 {
    raw & CR3_VALID_MASK
}

/// Ham CR3 okuması (PCID bitleri dahil). `AddressSpace::current` ve
/// tanı amaçlı kullanılır; PML4 tabanı için `pml4_from_cr3` kullanın.
///
/// # Safety
/// Yalnızca Ring 0'da çağrılmalıdır; `mov cr3` okuması ayrıcalıklı bir
/// yazmaç erişimidir.
#[inline(always)]
pub unsafe fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Ham CR3 değerinden PML4 fiziksel tabanını ayıklar (PCID/flag bitlerini atar).
#[inline(always)]
pub fn pml4_from_cr3(cr3: u64) -> u64 {
    cr3 & 0x000F_FFFF_FFFF_F000
}

/// Bir PCID'nin tüm TLB ve sayfa yapısı önbellek kayıtlarını geçersiz kılar.
/// Global sayfa çevirileri korunur (INVPCID type 1).
///
/// INVPCID yoksa `CR4.PCIDE` geçişiyle tüm PCID'ler için tam flush
/// yapılır (nadir yol; süreç çıkışlarında kabul edilebilir maliyet).
pub fn invalidate(pcid: u16) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    let pcid64 = u64::from(pcid & 0xFFF);
    if INVPCID_OK.load(Ordering::Acquire) {
        // INVPCID type 1 (single-context): düşük 64 bit PCID, yüksek 64
        // bit doğrusal adres. Tip 1 için adres kullanılmaz; 0 bırakılır.
        // `u128` 16 bayt hizalı olduğundan m128 operand güvenlidir.
        let desc: u128 = pcid64 as u128;
        unsafe {
            core::arch::asm!(
                "invpcid {ty}, [{desc}]",
                ty = in(reg) 1u64,
                desc = in(reg) &desc,
                options(nostack),
            );
        }
    } else {
        flush_all();
    }
}

/// Tüm PCID'ler için non-global TLB/sayfa yapısı kayıtlarını temizler.
/// INVPCID type 3 (all-context, non-global); yoksa `CR4.PCIDE` toggle.
pub fn flush_all() {
    if !ENABLED.load(Ordering::Acquire) {
        // PCID kapalıyken bile eski usul tam flush: mevcut CR3'ü
        // bit63'süz yeniden yükle.
        let cr3 = unsafe { read_cr3() };
        unsafe {
            core::arch::asm!("mov cr3, {}", in(reg) cr3 & CR3_VALID_MASK, options(nostack, preserves_flags));
        }
        return;
    }
    if INVPCID_OK.load(Ordering::Acquire) {
        let desc: u128 = 0;
        unsafe {
            core::arch::asm!(
                "invpcid {ty}, [{desc}]",
                ty = in(reg) 3u64,
                desc = in(reg) &desc,
                options(nostack),
            );
        }
    } else {
        // CR4.PCIDE 1→0 geçişi tüm PCID'ler için tam flush üretir;
        // ardından 0→1 ile PCID tekrar açılır.
        unsafe {
            let mut cr4: u64;
            core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
            cr4 &= !CR4_PCIDE;
            core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
            cr4 |= CR4_PCIDE;
            core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack, preserves_flags));
        }
    }
}
