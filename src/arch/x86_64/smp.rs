//! Simetrik Çok İşlemcili (SMP) desteği: Limine SMP protokolü ile AP açılışı.
//!
//! * BSP, `init()` içinde her AP'nin `goto_address` alanına [`ap_entry`]
//!   adresini yazar; Limine AP'leri park halinden çıkarır.
//! * AP'ler çıplak (`naked`) girişle atomik sayaçtan kimlik alıp kendilerine
//!   ayrılmış 64 KiB yığına geçer, SSE + GDT kurar ve iş kuyruğuna katılır.
//! * Kesme zamanlayıcısı yoktur; AP'ler kuyruğu yoklar (polling). Çekirdek
//!   veri yarışını önlemek için paylaşılan durum yalnızca atomikler ve
//!   [`crate::arch::x86_64::sync`] kilitleri üzerinden erişilir.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::gdt;
use super::sync::SpinMutex;

/// Desteklenen azami CPU sayısı (BSP dahil).
pub const MAX_CPUS: usize = 8;
/// AP çekirdek yığını boyutu.
const AP_STACK_SIZE: usize = 64 * 1024;

// --- Limine SMP protokolü ---------------------------------------------------

#[repr(C, align(8))]
struct SmpRequest {
    id: [u64; 4],
    revision: u64,
    response: *const SmpResponse,
}

#[repr(C, align(8))]
struct SmpResponse {
    revision: u64,
    flags: u32,
    bsp_lapic_id: u32,
    cpu_count: u64,
    cpus: *const *mut SmpCpu,
}

#[repr(C, align(8))]
struct SmpCpu {
    processor_id: u32,
    lapic_id: u32,
    _reserved: u64,
    goto_address: u64,
    extra_argument: u64,
}

unsafe impl Sync for SmpRequest {}

#[used]
#[link_section = ".requests"]
static SMP_REQUEST: SmpRequest = SmpRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x95a67b819a1b857e,
        0xa0b61b723b6a73e0,
    ],
    revision: 0,
    response: core::ptr::null(),
};

// --- CPU başına durum --------------------------------------------------------

#[repr(C, align(16))]
struct AlignedApStack([u8; AP_STACK_SIZE]);

static NEXT_AP: AtomicU32 = AtomicU32::new(0);
static mut AP_STACKS: [AlignedApStack; MAX_CPUS] = [const { AlignedApStack([0; AP_STACK_SIZE]) }; MAX_CPUS];
static AP_ONLINE: [AtomicBool; MAX_CPUS] = [
    AtomicBool::new(false), AtomicBool::new(false),
    AtomicBool::new(false), AtomicBool::new(false),
    AtomicBool::new(false), AtomicBool::new(false),
    AtomicBool::new(false), AtomicBool::new(false),
];
/// AP kalp atış sayaçları — paralel yürütmenin canlı kanıtı.
static HEARTBEAT: [AtomicU64; MAX_CPUS] = [
    AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0),
];
static CPU_COUNT: AtomicU32 = AtomicU32::new(1);
static LAPIC_ID: [AtomicU32; MAX_CPUS] = [
    AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0),
];

static PRINT_LOCK: super::sync::Spinlock = super::sync::Spinlock::new();

/// AP/BSP dışı bağlamlardan seriye kilitli yazı (karışmayı önler).
pub fn print_locked(s: &str) {
    PRINT_LOCK.lock();
    crate::serial::print(s);
    PRINT_LOCK.unlock();
}

pub fn print_locked_dec(v: u64) {
    PRINT_LOCK.lock();
    crate::serial::print_dec(v);
    PRINT_LOCK.unlock();
}

pub fn print_locked_hex(v: u64) {
    PRINT_LOCK.lock();
    crate::serial::print_hex(v);
    PRINT_LOCK.unlock();
}

// --- MSR / LAPIC ------------------------------------------------------------

#[inline(always)]
fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
    }
    ((hi as u64) << 32) | lo as u64
}

const IA32_APIC_BASE: u32 = 0x1B;

/// Yerel APIC taban fiziksel adresi (MSR 0x1B).
pub fn lapic_phys_base() -> u64 {
    rdmsr(IA32_APIC_BASE) & 0xFFFF_FFFF_FFF0_0000
}

fn lapic_virt() -> *mut u32 {
    (crate::drivers::phys_to_virt(lapic_phys_base())) as *mut u32
}

#[inline(always)]
fn lapic_read(reg: u32) -> u32 {
    unsafe { core::ptr::read_volatile(lapic_virt().byte_add(reg as usize)) }
}

#[inline(always)]
fn lapic_write(reg: u32, val: u32) {
    unsafe { core::ptr::write_volatile(lapic_virt().byte_add(reg as usize), val) }
}

/// APIC'i etkinleştirir (spurious vektör 0xFF) ve APIC ID'yi döndürür.
/// Zamanlayıcı IRQ'su kurulmaz; LVT girdileri maskeli bırakılır.
pub fn lapic_init() -> u32 {
    // 2 MB sayfaya hizalı MMIO eşlemesi (APIC genelde 0xFEE00000).
    let base = lapic_phys_base();
    if base != 0 {
        let aligned = base & !0x1F_FFFF;
        super::mmio_map_2mb(aligned);
    }
    // Spurious Interrupt Vector: vektör 0xFF + APIC etkin (bit 8).
    lapic_write(0xF0, 0x1FF);
    lapic_read(0x20) >> 24
}

pub fn lapic_id_of(cpu: usize) -> u32 {
    LAPIC_ID[cpu.min(MAX_CPUS - 1)].load(Ordering::Relaxed)
}

// --- AP girişi ----------------------------------------------------------------

/// AP giriş noktası. Yığına dokunmadan atomik kimlik alır, kendi yığınına
/// geçer ve [`ap_main_rust`] fonksiyonuna dallanır. Asla dönmez.
#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "C" fn ap_entry() {
    core::arch::naked_asm!(
        // idx = NEXT_AP++ (0-tabanlı AP sırası); BSP hariç kimlik = idx + 1.
        "mov eax, 1",
        "lock xadd dword ptr [{next}], eax",
        "add eax, 1",
        "cmp eax, {max}",
        "jae 3f",
        // Yığın tepesi = AP_STACKS[idx] + 64K (16 bayt hizalı).
        "mov ecx, eax",
        "shl rcx, 16",
        "lea rdx, [rip + {stacks}]",
        "add rdx, rcx",
        "add rdx, {stack_size}",
        "and rdx, -16",
        "mov rsp, rdx",
        "mov edi, eax",
        "call {main}",
        // ap_main_rust dönmemeli; yine de park et.
        "3:",
        "cli",
        "hlt",
        "jmp 3b",
        next = sym NEXT_AP,
        stacks = sym AP_STACKS,
        main = sym ap_main_rust,
        max = const MAX_CPUS,
        stack_size = const AP_STACK_SIZE as u32,
        options(),
    )
}

/// AP ana döngüsü (kendi yığınında, C ABI ile çağrılır).
#[no_mangle]
pub extern "C" fn ap_main_rust(idx: u32) -> ! {
    let cpu = (idx as usize).min(MAX_CPUS - 1);
    super::enable_sse();
    unsafe { gdt::load_for_cpu(cpu) };
    LAPIC_ID[cpu].store(lapic_read(0x20) >> 24, Ordering::Relaxed);
    AP_ONLINE[cpu].store(true, Ordering::Release);

    print_locked("[SMP] AP");
    print_locked_dec(cpu as u64);
    print_locked(" baslatildi (yigin+GDT hazir).\n");

    loop {
        // İş kuyruğunu yokla; yoksa kalp atışı üret.
        if !poll_job(cpu) {
            HEARTBEAT[cpu].fetch_add(1, Ordering::Relaxed);
            for _ in 0..4096 {
                core::hint::spin_loop();
            }
        }
    }
}

// --- Paralel iş kuyruğu -------------------------------------------------------

struct Job {
    active: bool,
    data_phys: u64,
    total_u64: usize,
    chunk_u64: usize,
    next_chunk: usize,
    chunks_done: usize,
    partial: [u64; MAX_CPUS],
}

impl Job {
    const fn empty() -> Self {
        Self {
            active: false,
            data_phys: 0,
            total_u64: 0,
            chunk_u64: 0,
            next_chunk: 0,
            chunks_done: 0,
            partial: [0; MAX_CPUS],
        }
    }
}

static JOB: SpinMutex<Job> = SpinMutex::new(Job::empty());

fn checksum_slice(phys_base: u64, start_u64: usize, len_u64: usize) -> u64 {
    let base = crate::drivers::phys_to_virt(phys_base) as *const u64;
    let mut acc: u64 = 0;
    for i in 0..len_u64 {
        acc = acc.wrapping_add(unsafe { core::ptr::read_volatile(base.add(start_u64 + i)) });
    }
    acc
}

/// Kuyruktan bir parça alıp işler. İş yapıldıysa true döner.
fn poll_job(cpu: usize) -> bool {
    let (phys, start, len, last) = {
        let mut job = JOB.lock();
        if !job.active || job.next_chunk * job.chunk_u64 >= job.total_u64 {
            return false;
        }
        let start = job.next_chunk * job.chunk_u64;
        let remaining = job.total_u64 - start;
        let len = remaining.min(job.chunk_u64);
        job.next_chunk += 1;
        let total_chunks = (job.total_u64 + job.chunk_u64 - 1) / job.chunk_u64;
        let last = job.next_chunk >= total_chunks;
        (job.data_phys, start, len, last)
    };
    let sum = checksum_slice(phys, start, len);
    {
        let mut job = JOB.lock();
        job.partial[cpu] = job.partial[cpu].wrapping_add(sum);
        job.chunks_done += 1;
        let _ = last;
    }
    true
}

/// `len_u64` u64'lük fiziksel tamponu `chunk_u64` parçalarla AP'lere dağıtır.
/// Çağıran (BSP) da kendi payını işler; tüm parçalar bitene kadar bekler.
/// Dönüş: kısmi toplamların toplamı. Zaman aşımında None.
pub fn parallel_checksum(data_phys: u64, total_u64: usize, chunk_u64: usize) -> Option<u64> {
    {
        let mut job = JOB.lock();
        *job = Job::empty();
        job.active = true;
        job.data_phys = data_phys;
        job.total_u64 = total_u64;
        job.chunk_u64 = chunk_u64.max(1);
    }
    // BSP de işçidir (cpu 0).
    while poll_job(0) {}
    let total_chunks = (total_u64 + chunk_u64.max(1) - 1) / chunk_u64.max(1);
    let mut spins = 0u32;
    loop {
        {
            let job = JOB.lock();
            if job.chunks_done >= total_chunks {
                let sum = job.partial.iter().fold(0u64, |a, b| a.wrapping_add(*b));
                return Some(sum);
            }
        }
        // AP'ler başlamamışsa BSP kalanı kendisi bitirir.
        if !poll_job(0) {
            spins += 1;
            if spins > 20_000_000 {
                let mut job = JOB.lock();
                job.active = false;
                return None;
            }
            core::hint::spin_loop();
        } else {
            spins = 0;
        }
    }
}

// --- BSP kurulumu --------------------------------------------------------------

fn limine_response() -> Option<*const SmpResponse> {
    unsafe {
        let slot = core::ptr::addr_of!(SMP_REQUEST.response);
        let resp = core::ptr::read_volatile(slot);
        if resp.is_null() {
            return None;
        }
        let addr = resp as usize as u64;
        let upper = addr >> 47;
        if !(upper == 0 || upper == 0x1_FFFF) {
            return None;
        }
        if (resp as usize) % 8 != 0 {
            return None;
        }
        // Yanıtın yüksek yarıda eşlendiğini doğrula (Limine sözleşmesi).
        if (addr >> 47) != 0x1_FFFF {
            return None;
        }
        Some(resp)
    }
}

pub fn cpu_count() -> usize {
    CPU_COUNT.load(Ordering::Relaxed) as usize
}

pub fn is_ap_online(cpu: usize) -> bool {
    if cpu >= MAX_CPUS {
        return false;
    }
    AP_ONLINE[cpu].load(Ordering::Acquire)
}

pub fn heartbeat(cpu: usize) -> u64 {
    HEARTBEAT[cpu.min(MAX_CPUS - 1)].load(Ordering::Relaxed)
}

/// BSP tarafından çağrılır: AP'leri uyandırır, LAPIC'i açar.
/// Dönüş: görünen CPU sayısı (en az 1).
pub fn init() -> usize {
    CPU_COUNT.store(1, Ordering::Relaxed);
    AP_ONLINE[0].store(true, Ordering::Release);

    let resp = match limine_response() {
        Some(r) => r,
        None => {
            crate::serial_print("[SMP] Limine SMP yaniti yok; tek cekirdek modunda devam.\n");
            return 1;
        }
    };
    let (count, bsp_id, cpus) = unsafe {
        (
            core::ptr::read_volatile(core::ptr::addr_of!((*resp).cpu_count)) as usize,
            core::ptr::read_volatile(core::ptr::addr_of!((*resp).bsp_lapic_id)),
            core::ptr::read_volatile(core::ptr::addr_of!((*resp).cpus)),
        )
    };
    if count <= 1 || cpus.is_null() {
        crate::serial_print("[SMP] Tek CPU gorundu; SMP kapali.\n");
        return 1;
    }
    let count = count.min(MAX_CPUS);
    CPU_COUNT.store(count as u32, Ordering::Relaxed);

    // APIC ID'mizi kaydet.
    LAPIC_ID[0].store(lapic_init(), Ordering::Relaxed);

    crate::serial_print("[SMP] ");
    crate::serial::print_dec(count as u64);
    crate::serial_print(" CPU bulundu; AP'ler baslatiliyor...\n");

    let entry = ap_entry as *const () as usize as u64;
    for i in 0..count {
        let cpu_ptr = unsafe { core::ptr::read_volatile(cpus.add(i)) };
        if cpu_ptr.is_null() {
            continue;
        }
        let lapic = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*cpu_ptr).lapic_id)) };
        if lapic == bsp_id {
            continue; // BSP'yi atla.
        }
        unsafe {
            core::ptr::addr_of_mut!((*cpu_ptr).goto_address).write_volatile(entry);
        }
        crate::serial_print("[SMP] AP (LAPIC ");
        crate::serial::print_dec(lapic as u64);
        crate::serial_print(") uyandirildi.\n");
    }
    count
}

/// AP'lerin çevrimiçi olmasını sınırlı süre bekler.
pub fn wait_for_aps(timeout_spins: u32) -> usize {
    let count = cpu_count();
    let mut spins = 0u32;
    loop {
        let mut online = 1usize;
        for cpu in 1..count {
            if is_ap_online(cpu) {
                online += 1;
            }
        }
        if online >= count {
            return online;
        }
        spins += 1;
        if spins >= timeout_spins {
            return online;
        }
        core::hint::spin_loop();
    }
}
