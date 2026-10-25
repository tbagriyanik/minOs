//! Ring3 (kullanıcı kipi) izolasyonu: GDT kullanıcı segmentleri + TSS +

//! `int 0x80` sistem çağrıları + #GP/#PF hata yakalama.

//!

//! * `install_gates()` IDT'ye 0x80 (DPL=3), #GP (13) ve #PF (14) kapılarını kurar.

//! * `demo()` iki küçük kullanıcı programı çalıştırır:

//!   1. `SYS_GETPID` çağırıp sonucu paylaşılan sayfaya yazar (syscall yolu),

//!   2. `hlt` çalıştırıp #GP üretir (hata yakalama ve etkisizleştirme).

//! * Hata durumunda kesici, Ring3 bağlamının RIP'ini park döngüsüne (`jmp $`)

//!   yönlendirip çekirdeğe `iretq` ile döner; çekirdek etkilenmez.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::gdt;

use super::smp;

// --- IDT kapı kurulumu (idt.rs'deki tabloya) ---------------------------------

fn set_gate(vec: u8, handler: usize, dpl: u8) {
    unsafe {
        let idt = &mut *core::ptr::addr_of_mut!(super::IDT);

        idt.entries[vec as usize].set_handler_dpl(handler, dpl);
    }
}

fn set_gate_ist(vec: u8, handler: usize, dpl: u8, ist: u8) {
    unsafe {
        let idt = &mut *core::ptr::addr_of_mut!(super::IDT);

        idt.entries[vec as usize].set_handler_ist(handler, dpl, ist);
    }
}

pub fn install_gates() {
    set_gate(0, de_entry as *const () as usize, 0);

    set_gate(6, ud_entry as *const () as usize, 0);

    set_gate(0x80, int80_entry as *const () as usize, 3);

    set_gate_ist(8, df_entry as *const () as usize, 0, 1); // #DF IST1 kullanır

    set_gate(12, ss_entry as *const () as usize, 0);

    set_gate(13, gp_entry as *const () as usize, 0);

    set_gate(14, pf_entry as *const () as usize, 0);
}

// --- Kesici çerçeveleri --------------------------------------------------------

// int $0x80 çerçevesi (hata kodu yok): 15 saklanan yazmaç + CPU çerçevesi.

const F_RAX: usize = 0;

const F_RDX: usize = 3;

const F_RSI: usize = 4;

const F_RDI: usize = 5;

/// CPU itmeli çerçeve (iretq hedefi): RIP/CS/RFLAGS/RSP/SS.

const F_IRIP: usize = 15;

const F_ICS: usize = 16;

const F_IRSP: usize = 18;

const F_ISS: usize = 19;

/// Naked `int $0x80` işleyicisi: yazmaçları saklar, Rust'a devreder, `iretq`.

#[no_mangle]
#[unsafe(naked)]

pub unsafe extern "C" fn int80_entry() {
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

        "mov rdi, rsp",

        "call {handler}",

        "mov [rsp], rax",

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

        handler = sym rust_int80,

        options(),

    )
}

#[no_mangle]
pub unsafe extern "C" fn rust_int80(frame: *mut u64) -> u64 {
    let (no, a1, a2, a3) = unsafe {
        (
            core::ptr::read_volatile(frame.add(F_RAX)),
            core::ptr::read_volatile(frame.add(F_RDI)),
            core::ptr::read_volatile(frame.add(F_RSI)),
            core::ptr::read_volatile(frame.add(F_RDX)),
        )
    };

    smp::print_locked("[RING3-TRACE] int80 no=");

    smp::print_locked_dec(no);

    smp::print_locked("\n");

    match no {
        // SYS_YIELD
        0 => 0,

        // SYS_EXIT: kesme çerçevesini çekirdeğe dönüş için yeniden yaz.

        // `iretq` trampoline'e (Ring0) dallanır; o da `enter_usermode`

        // çağrısına `ret` ile döner. Kullanıcı iş parçacığı sona erer.
        1 => {
            USER_EXITED.store(true, Ordering::Release);

            unsafe {
                core::ptr::write_volatile(
                    frame.add(F_IRIP),
                    ring3_exit_trampoline as *const () as u64,
                );

                core::ptr::write_volatile(frame.add(F_ICS), gdt::SEL_KCODE as u64);

                core::ptr::write_volatile(frame.add(F_IRSP), SAVED_KRSP.load(Ordering::Relaxed));

                core::ptr::write_volatile(frame.add(F_ISS), gdt::SEL_KDATA as u64);
            }

            0
        }

        // SYS_GETPID: demo kullanıcı PID'si.
        2 => 1001,

        // SYS_WRITE: arg1 = kullanıcı tamponu, arg2 = uzunluk.
        5 => {
            let len = (a2 as usize).min(256);

            if a1 == 0 || len == 0 || a1 >= 0x0000_8000_0000_0000 {
                return 0;
            }

            if a1.checked_add(len as u64).is_none() {
                return 0;
            }

            let mut buf = [0u8; 256];

            for i in 0..len {
                buf[i] = unsafe { core::ptr::read_volatile((a1 + i as u64) as *const u8) };
            }

            match core::str::from_utf8(&buf[..len]) {
                Ok(s) => {
                    smp::print_locked(s);
                    unsafe {
                        LAST_RING3_OUTPUT[..len].copy_from_slice(&buf[..len]);
                        LAST_RING3_OUTPUT_LEN = len;
                    }
                }

                Err(_) => smp::print_locked("[ring3: ikili veri]\n"),
            }

            let _ = a3;

            len as u64
        }

        _ => u64::MAX,
    }
}

// --- #GP / #PF yakalama --------------------------------------------------------

static PARK_RIP: AtomicU64 = AtomicU64::new(0);

static USER_FAULT_VEC: AtomicU64 = AtomicU64::new(0);

static USER_FAULT_RIP: AtomicU64 = AtomicU64::new(0);

static USER_EXITED: AtomicBool = AtomicBool::new(false);

static mut LAST_RING3_OUTPUT: [u8; 256] = [0; 256];
static mut LAST_RING3_OUTPUT_LEN: usize = 0;

pub fn take_last_output() -> Option<alloc::string::String> {
    unsafe {
        if LAST_RING3_OUTPUT_LEN == 0 {
            None
        } else {
            let s = core::str::from_utf8(&LAST_RING3_OUTPUT[..LAST_RING3_OUTPUT_LEN])
                .ok()
                .map(alloc::string::String::from);
            LAST_RING3_OUTPUT_LEN = 0;
            s
        }
    }
}

/// Hata kodlu kesici çerçevesi: int80 çerçevesi + tabanda hata kodu.

const EF_RIP: usize = 16;

const EF_CS: usize = 17;

macro_rules! fault_entry {

    ($name:ident, $vec:expr, $rust:ident) => {

        #[no_mangle]

        #[unsafe(naked)]

        pub unsafe extern "C" fn $name() {

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

                "mov rdi, rsp",

                "mov rsi, [rsp + 120]",

                "mov edx, {vec}",

                "sub rsp, 8",

                "call {handler}",

                "add rsp, 8",

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

                "add rsp, 8",

                "iretq",

                vec = const $vec,

                handler = sym $rust,

                options(),

            )

        }

    };

}

macro_rules! fault_entry_no_err {

    ($name:ident, $vec:expr, $rust:ident) => {

        #[no_mangle]

        #[unsafe(naked)]

        pub unsafe extern "C" fn $name() {

            core::arch::naked_asm!(

                "push 0",

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

                "mov rdi, rsp",

                "mov rsi, [rsp + 120]",

                "mov edx, {vec}",

                "sub rsp, 8",

                "call {handler}",

                "add rsp, 8",

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

                "add rsp, 8",

                "iretq",

                vec = const $vec,

                handler = sym $rust,

                options(),

            )

        }

    };

}

fault_entry_no_err!(de_entry, 0u32, rust_fault);

fault_entry_no_err!(ud_entry, 6u32, rust_fault);

fault_entry!(df_entry, 8u32, rust_fault);

fault_entry!(ss_entry, 12u32, rust_fault);

fault_entry!(gp_entry, 13u32, rust_fault);

fault_entry!(pf_entry, 14u32, rust_fault);

/// Ring3 hatası: kaydet, RIP'i park döngüsüne yönlendir, `iretq` ile dön.

/// Çekirdek hatası: seriye yazıp dur (üçlü hata yerine kontrollü durma).

#[no_mangle]
pub unsafe extern "C" fn rust_fault(frame: *mut u64, err: u64, vec: u32) {
    let (cs, rip) = unsafe {
        (
            core::ptr::read_volatile(frame.add(EF_CS)),
            core::ptr::read_volatile(frame.add(EF_RIP)),
        )
    };

    let _ = err;

    if cs & 3 == 3 {
        // Kullanıcı kipi hatası: etkisizleştir ve Ring3 parkına dön.

        USER_FAULT_VEC.store(vec as u64, Ordering::Release);

        USER_FAULT_RIP.store(rip, Ordering::Relaxed);

        let park = PARK_RIP.load(Ordering::Relaxed);

        if park != 0 {
            unsafe { core::ptr::write_volatile(frame.add(EF_RIP), park) };
        }

        smp::print_locked("[RING3] Kullanici hatasi yakalandi (vektor ");

        smp::print_locked_dec(vec as u64);

        smp::print_locked("); surec park edildi, cekirdek saglam.\n");
    } else {
        let cr2: u64;

        unsafe {
            core::arch::asm!("mov {}, cr2", out(reg) cr2, options(nomem, nostack, preserves_flags));
        }

        smp::print_locked(
            "\n==================== [ CEKIRDEK HATASI (EXCEPTION) ] ====================\n",
        );

        smp::print_locked("[RING3] KRITIK: cekirdek hatasi (vektor ");

        smp::print_locked_dec(vec as u64);

        smp::print_locked(", err=");

        smp::print_locked_hex(err);

        smp::print_locked(", rip=");

        smp::print_locked_hex(rip);

        smp::print_locked(", cs=");

        smp::print_locked_hex(cs);

        smp::print_locked(", cr2=");

        smp::print_locked_hex(cr2);

        smp::print_locked("). Sistem durduruldu.\n");

        smp::print_locked(
            "========================================================================\n",
        );

        // Ekrana siyah zemin üzerine turuncu hata ekranı çiz

        unsafe {
            use crate::drivers::video;

            if !video::PANIC_FB_ADDR.is_null() && video::PANIC_FB_W > 0 && video::PANIC_FB_H > 0 {
                let mut pfb = video::Framebuffer::new(
                    video::PANIC_FB_ADDR,
                    video::PANIC_FB_W,
                    video::PANIC_FB_H,
                    video::PANIC_FB_PITCH,
                    video::PANIC_FB_BPP,
                );

                // Siyah arka plan

                pfb.draw_rect(0, 0, video::PANIC_FB_W, video::PANIC_FB_H, 0x000000);

                const ORANGE_TITLE: u32 = 0xFFA500;

                const ORANGE_TEXT: u32 = 0xFF8C00;

                const ORANGE_MUTED: u32 = 0xCC7000;

                let center_x = 40;

                let mut cur_y = 50;

                // 1. Başlık

                crate::gui::MinGuiCompositor::draw_string_scaled(
                    &mut pfb,
                    center_x,
                    cur_y,
                    "!!! minOs ÇEKİRDEK HATASI (KERNEL EXCEPTION) !!!",
                    ORANGE_TITLE,
                    2,
                );

                cur_y += 50;

                // 2. Çizgi

                pfb.draw_rect(
                    center_x,
                    cur_y,
                    video::PANIC_FB_W.saturating_sub(80),
                    2,
                    ORANGE_MUTED,
                );

                cur_y += 30;

                // 3. Açıklama

                crate::gui::MinGuiCompositor::draw_string(

                    &mut pfb,

                    center_x,

                    cur_y,

                    "Çekirdek kipinde kurtarılamayan donanım kesmesi/sayfa hatası (Page Fault) oluştu.",

                    ORANGE_TEXT,

                );

                cur_y += 24;

                // 4. Detaylar (Vektör, Err, CR2, RIP)

                let mut det_buf = crate::gui::TextBuf::new();

                det_buf.push_str("Hata: Vektor ");

                det_buf.push_usize(vec as usize);

                match vec {
                    14 => det_buf.push_str(" (#PF - Page Fault)"),

                    13 => det_buf.push_str(" (#GP - General Protection Fault)"),

                    8 => det_buf.push_str(" (#DF - Double Fault)"),

                    _ => {}
                }

                det_buf.push_str(" | Hata Kodu (err): 0x");

                det_buf.push_hex64(err);

                crate::gui::MinGuiCompositor::draw_string(
                    &mut pfb,
                    center_x,
                    cur_y,
                    det_buf.as_str(),
                    0xFFFFFF,
                );

                cur_y += 26;

                let mut cr2_buf = crate::gui::TextBuf::new();

                cr2_buf.push_str("Geçersiz Erişim Adresi (CR2): 0x");

                cr2_buf.push_hex64(cr2);

                cr2_buf.push_str(" | RIP: 0x");

                cr2_buf.push_hex64(rip);

                crate::gui::MinGuiCompositor::draw_string(
                    &mut pfb,
                    center_x,
                    cur_y,
                    cr2_buf.as_str(),
                    ORANGE_TEXT,
                );

                cur_y += 36;

                // 5. Bilgilendirme ve Yeniden Başlatma Talimatı

                pfb.draw_rect(
                    center_x,
                    cur_y,
                    video::PANIC_FB_W.saturating_sub(80),
                    1,
                    ORANGE_MUTED,
                );

                cur_y += 24;

                crate::gui::MinGuiCompositor::draw_string(

                    &mut pfb,

                    center_x,

                    cur_y,

                    "Yeniden başlatmak (Restart) için herhangi bir tuşa [ENTER / ESC / SPACE] basınız.",

                    ORANGE_TITLE,

                );

                cur_y += 24;

                crate::gui::MinGuiCompositor::draw_string(
                    &mut pfb,
                    center_x,
                    cur_y,
                    "Press any key [ENTER / ESC / SPACE] to reboot the system.",
                    ORANGE_MUTED,
                );

                pfb.flush();
            }
        }

        // Text konsolu ve etkileşimli kurtarma / yeniden başlatma promptu

        let mut text_buf = crate::gui::TextBuf::new();

        text_buf.push_str("KERNEL EXCEPTION: Vektor ");

        text_buf.push_usize(vec as usize);

        match vec {
            0 => text_buf.push_str(" (#DE - Sifira Bolme)"),

            6 => text_buf.push_str(" (#UD - Gecersiz Komut)"),

            8 => text_buf.push_str(" (#DF - Cift Hata)"),

            12 => text_buf.push_str(" (#SS - Yigin Hatasi)"),

            13 => text_buf.push_str(" (#GP - Genel Koruma Hatasi)"),

            14 => text_buf.push_str(" (#PF - Sayfa Hatasi)"),

            _ => {}
        }

        text_buf.push_str(" | RIP: 0x");

        text_buf.push_hex64(rip);

        text_buf.push_str(" | Err: 0x");

        text_buf.push_hex64(err);

        text_buf.push_str(" | CR2: 0x");

        text_buf.push_hex64(cr2);

        crate::boot::text_console::write("\n\n");

        crate::boot::text_console::write_color(text_buf.as_str(), 0x00FF_5555);

        crate::boot::text_console::write("\n");

        crate::boot::text_console::interactive_error_prompt(text_buf.as_str());
    }
}

// --- Ring3 girişi ----------------------------------------------------------------

#[repr(C, align(16))]

struct AlignedKStack([u8; 16 * 1024]);

/// Ring3 -> Ring0 geçiş yığını (yalnızca BSP demosu için 16 KiB, 16 bayt hizalı).

static mut RING3_KSTACK: AlignedKStack = AlignedKStack([0; 16 * 1024]);

/// `enter_usermode` girişindeki çekirdek RSP'si (EXIT dönüşü için).

static SAVED_KRSP: AtomicU64 = AtomicU64::new(0);

fn ring3_kstack_top() -> u64 {
    (core::ptr::addr_of!(RING3_KSTACK) as u64 + 16 * 1024) & !0xF
}

/// EXIT dönüş trampolini: `iretq` buraya (Ring0) dallanır; `ret`

/// `enter_usermode` çağrıcısına döner.

#[no_mangle]
#[unsafe(naked)]

pub unsafe extern "C" fn ring3_exit_trampoline() {
    core::arch::naked_asm!("ret", options(),)
}

/// Ring3 giriş yığınını hazırlar (TSS.rsp0) ve tepesini döndürür.

pub fn prepare_usermode_stack() -> u64 {
    let top = ring3_kstack_top();

    gdt::set_kernel_stack(0, top);

    top
}

/// Kullanıcı giriş noktasına `iretq` ile geçer (çıplak sarmalayıcı).

/// RDI = giriş RIP'i, RSI = kullanıcı yığın tepesi (C ABI).

/// Prolog yoktur; ilk iş RSP'yi (dönüş adresi yuvası) kaydetmektir.

#[no_mangle]
#[unsafe(naked)]

pub unsafe extern "C" fn enter_usermode_raw(entry: u64, stack_top: u64) {
    core::arch::naked_asm!(

        "mov [{saved}], rsp",

        "push {udata}",

        "push rsi",

        "push {rflags}",

        "push {ucode}",

        "push rdi",

        "iretq",

        saved = sym SAVED_KRSP,

        udata = const gdt::SEL_UDATA as u64,

        rflags = const 0x202u64,

        ucode = const gdt::SEL_UCODE as u64,

        options(),

    )
}

/// Kullanıcı giriş noktasına `iretq` ile geçer.

/// Kullanıcı `SYS_EXIT` çağırırsa bu fonksiyon normal şekilde döner;

/// aksi halde (parkta takılma) dönmez.

pub unsafe fn enter_usermode(entry: u64, stack_top: u64) {
    prepare_usermode_stack();

    enter_usermode_raw(entry, stack_top);
}

// --- Demo -------------------------------------------------------------------------

const DEMO_CODE: u64 = crate::mm::loader::USER_CODE_BASE; // 0x40_0000

const DEMO_RESULT: u64 = 0x0000_0000_0060_0000;

const DEMO_STACK_TOP: u64 = crate::mm::loader::USER_STACK_TOP - 16;

/// Demo program A: getpid -> paylaşılan sayfaya yaz -> EXIT -> (park).

/// B8 02 00 00 00 | CD 80 | 48 A3 <imm64=RESULT> | B8 01 00 00 00 | CD 80 | EB FE

fn prog_getpid() -> [u8; 26] {
    let mut p = [0u8; 26];

    p[0..5].copy_from_slice(&[0xB8, 0x02, 0x00, 0x00, 0x00]);

    p[5..7].copy_from_slice(&[0xCD, 0x80]);

    p[7] = 0x48;

    p[8] = 0xA3;

    p[9..17].copy_from_slice(&DEMO_RESULT.to_le_bytes());

    p[17..22].copy_from_slice(&[0xB8, 0x01, 0x00, 0x00, 0x00]);

    p[22..24].copy_from_slice(&[0xCD, 0x80]);

    p[24..26].copy_from_slice(&[0xEB, 0xFE]);

    p
}

const PROG_A_PARK_OFF: u64 = 24;

/// Demo program B: hlt (#GP üretir) -> EXIT saplaması -> park.

/// +0: F4 | +1: B8 01 00 00 00 | CD 80 | EB FE

fn prog_fault() -> [u8; 10] {
    [0xF4, 0xB8, 0x01, 0x00, 0x00, 0x00, 0xCD, 0x80, 0xEB, 0xFE]
}

const PROG_B_PARK_OFF: u64 = 1;

fn map_demo_pages(
    aspace: &mut crate::mm::vmm::AddressSpace,

    pmm: &mut crate::mm::pmm::PhysicalMemoryManager,

    code: &[u8],
) -> Option<(u64, u64, u64)> {
    use crate::mm::vmm::PageFlags;

    let code_frame = pmm.allocate_frame()?;

    let stack_frame = pmm.allocate_frame()?;

    let result_frame = pmm.allocate_frame()?;

    // Kodu çerçeveye yaz (HHDM üzerinden).

    unsafe {
        let dst = crate::drivers::phys_to_virt(code_frame) as *mut u8;

        core::ptr::write_bytes(dst, 0, 4096);

        core::ptr::copy_nonoverlapping(code.as_ptr(), dst, code.len());

        let rdst = crate::drivers::phys_to_virt(result_frame) as *mut u8;

        core::ptr::write_bytes(rdst, 0, 4096);

        let sdst = crate::drivers::phys_to_virt(stack_frame) as *mut u8;

        core::ptr::write_bytes(sdst, 0, 4096);
    }

    aspace
        .map_page(DEMO_CODE, code_frame, PageFlags::USER_CODE, pmm)
        .ok()?;

    aspace
        .map_page(DEMO_RESULT, result_frame, PageFlags::USER_DATA, pmm)
        .ok()?;

    let stack_page = DEMO_STACK_TOP & !0xFFF;

    aspace
        .map_page(stack_page, stack_frame, PageFlags::USER_STACK, pmm)
        .ok()?;

    Some((code_frame, stack_frame, result_frame))
}

fn unmap_demo_pages(
    aspace: &mut crate::mm::vmm::AddressSpace,

    pmm: &mut crate::mm::pmm::PhysicalMemoryManager,

    frames: (u64, u64, u64),
) {
    let _ = aspace.unmap_page(DEMO_CODE);

    let _ = aspace.unmap_page(DEMO_RESULT);

    let _ = aspace.unmap_page(DEMO_STACK_TOP & !0xFFF);

    pmm.free_frame(frames.0);

    pmm.free_frame(frames.1);

    pmm.free_frame(frames.2);
}

fn read_result() -> u64 {
    unsafe {
        let ptr = crate::drivers::phys_to_virt(
            crate::mm::vmm::AddressSpace::current()
                .translate(DEMO_RESULT)
                .unwrap_or(0),
        ) as *const u64;

        if ptr.is_null() {
            return u64::MAX;
        }

        core::ptr::read_volatile(ptr)
    }
}

/// Ring3 izolasyon demosu. Başarıda true döner; her adım seriye raporlanır.

pub fn demo(
    aspace: &mut crate::mm::vmm::AddressSpace,

    pmm: &mut crate::mm::pmm::PhysicalMemoryManager,
) -> bool {
    install_gates();

    smp::print_locked("[RING3] IDT kapilari kuruldu (0x80/DPL3, #SS/#GP/#PF).\n");

    let kstack = prepare_usermode_stack();

    smp::print_locked("[RING3-DIAG] TSS0=");

    smp::print_locked_hex(gdt::tss_addr(0));

    smp::print_locked(" rsp0=");

    smp::print_locked_hex(gdt::rsp0(0));

    smp::print_locked(" kstack=");

    smp::print_locked_hex(kstack);

    smp::print_locked(" d5=");

    smp::print_locked_hex(gdt::desc(0, 5));

    smp::print_locked(" d6=");

    smp::print_locked_hex(gdt::desc(0, 6));

    smp::print_locked("\n");

    // --- Adım 1: syscall yolu (getpid) ---

    let prog = prog_getpid();

    PARK_RIP.store(DEMO_CODE + PROG_A_PARK_OFF, Ordering::Relaxed);

    USER_EXITED.store(false, Ordering::Relaxed);
    unsafe {
        LAST_RING3_OUTPUT_LEN = 0;
    }

    smp::print_locked("[RING3] Adim 1: sayfalar esleniyor...\n");

    let frames = match map_demo_pages(aspace, pmm, &prog) {
        Some(f) => f,

        None => {
            smp::print_locked("[RING3] Demo sayfalari eslenemedi: BASARISIZ\n");

            return false;
        }
    };

    smp::print_locked("[RING3] Adim 1: Ring3'e giriliyor...\n");

    unsafe { enter_usermode(DEMO_CODE, DEMO_STACK_TOP) };

    smp::print_locked("[RING3] Adim 1: Ring3'ten donuldu (EXIT).\n");

    // Sonuç sayfası EXIT'ten önce yazılmış olmalı.

    let ok1 = read_result() == 1001 && USER_EXITED.load(Ordering::Acquire);

    if ok1 {
        smp::print_locked("[RING3] 1. Ring3 syscall (getpid -> 1001) + EXIT donusu: BASARILI\n");
    } else {
        smp::print_locked("[RING3] 1. Ring3 getpid/EXIT basarisiz: BASARISIZ\n");
    }

    // Kullanıcı artık canlı değil; sayfaları güvenle geri al.

    unmap_demo_pages(aspace, pmm, frames);

    // --- Adım 2: hata izolasyonu (hlt -> #GP -> EXIT saplaması) ---

    USER_FAULT_VEC.store(0, Ordering::Relaxed);

    USER_EXITED.store(false, Ordering::Relaxed);
    unsafe {
        LAST_RING3_OUTPUT_LEN = 0;
    }

    let prog2 = prog_fault();

    PARK_RIP.store(DEMO_CODE + PROG_B_PARK_OFF, Ordering::Relaxed);

    smp::print_locked("[RING3] Adim 2: sayfalar esleniyor...\n");

    let frames2 = match map_demo_pages(aspace, pmm, &prog2) {
        Some(f) => f,

        None => {
            smp::print_locked("[RING3] Hata demosu sayfalari eslenemedi: BASARISIZ\n");

            return false;
        }
    };

    smp::print_locked("[RING3] Adim 2: Ring3'e giriliyor (hlt bekleniyor)...\n");

    unsafe { enter_usermode(DEMO_CODE, DEMO_STACK_TOP) };

    smp::print_locked("[RING3] Adim 2: Ring3'ten donuldu.\n");

    let ok2 = USER_FAULT_VEC.load(Ordering::Acquire) == 13 && USER_EXITED.load(Ordering::Acquire);

    if ok2 {
        smp::print_locked("[RING3] 2. Ring3 #GP yakalandi, EXIT saplamasiyla donuldu: BASARILI\n");
    } else {
        smp::print_locked("[RING3] 2. Ring3 hata yakalama calismadi: BASARISIZ\n");
    }

    unmap_demo_pages(aspace, pmm, frames2);

    // Çekirdek hâlâ ayakta: kanıt olarak GDT yüklü ve seri çalışıyor.

    smp::print_locked("[RING3] Cekirdek Ring0'da calismaya devam ediyor: BASARILI\n");

    ok1 && ok2
}

/// Yüklenen bir kullanıcı programını Ring 3'te çalıştırır.

/// Giriş noktasına atlar, program `SYS_EXIT` çağırdığında veya bir istisnada güvenle çekirdeğe döner.

pub fn execute_loaded_program(prog: &crate::mm::loader::LoadedProgram) -> bool {
    install_gates();

    prepare_usermode_stack();

    // Park RIP'i programın sonuna veya giriş noktasına yönlendir

    PARK_RIP.store(prog.entry_point, Ordering::Relaxed);

    USER_FAULT_VEC.store(0, Ordering::Relaxed);

    USER_FAULT_RIP.store(0, Ordering::Relaxed);

    USER_EXITED.store(false, Ordering::Relaxed);
    unsafe {
        LAST_RING3_OUTPUT_LEN = 0;
    }

    smp::print_locked("[RING3] Kullanici programi baslatiliyor (RIP=");

    smp::print_locked_hex(prog.entry_point);

    smp::print_locked(" RSP=");

    smp::print_locked_hex(prog.stack_pointer);

    smp::print_locked(")...\n");

    unsafe {
        enter_usermode(prog.entry_point, prog.stack_pointer);
    }

    let fault = USER_FAULT_VEC.load(Ordering::Acquire);

    let exited = USER_EXITED.load(Ordering::Acquire);

    if fault != 0 {
        smp::print_locked("[RING3] Kullanici programinda hata olustu (Istisna #");

        smp::print_locked_dec(fault);

        smp::print_locked("). Surec guvenle sonlandirildi.\n");

        false
    } else if exited {
        smp::print_locked("[RING3] Kullanici programi basariyla tamamlandi (SYS_EXIT).\n");

        true
    } else {
        smp::print_locked("[RING3] Kullanici programindan donuldu.\n");

        true
    }
}
