//! Gelişmiş grafik sürücüleri: donanım hızlandırmalı imleç + VESA/BGA çözünürlük değişimi.
//!
//! * **BGA (Bochs Display Interface):** QEMU/Bochs `0x1CE/0x1CF` portları üzerinden
//!   yeniden başlatmasız çözünürlük değişimi.
//! * **VMware SVGA:** PCI taramasıyla `15AD:0405` algılama + donanım imleci register
//!   hazırlığı (FIFO yoksa güvenli şekilde devre dışı).
//! * **VGA HW imleç:** metin modunda CRTC registerları; grafik modda donanım
//!   imleci BGA'da mevcut olmadığı için "hızlandırmalı yazılım imleci"
//!   (kirli-dikdörtgen + dikey-blank senkronlu flush) kullanılır.
//! * **VESA/VBE:** long-mode'da BIOS çağrısı yapılamadığı için mod listesi
//!   Limine framebuffer + BGA üzerinden sunulur; `ModeInfo` VBE 3.0 alanlarını yansıtır.

use core::arch::asm;

fn io_out16(port: u16, val: u16) {
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
    }
}
fn io_in16(port: u16) -> u16 {
    let ret: u16;
    unsafe {
        asm!("in ax, dx", in("dx") port, out("ax") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}
fn io_out32(port: u16, val: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
    }
}
fn io_in32(port: u16) -> u32 {
    let ret: u32;
    unsafe {
        asm!("in eax, dx", in("dx") port, out("eax") ret, options(nomem, nostack, preserves_flags));
    }
    ret
}
fn pci_read32(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    let _guard = crate::drivers::port_io::PCI_CONFIG_LOCK.lock();
    let addr = 0x8000_0000u32
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((off & 0xFC) as u32);
    io_out32(0xCF8, addr);
    io_in32(0xCFC)
}

// --- Bochs Graphics Adapter (BGA) ---
const BGA_INDEX: u16 = 0x1CE;
const BGA_VALUE: u16 = 0x1CF;
const BGA_IDX_ID: u16 = 0;
const BGA_IDX_XRES: u16 = 1;
const BGA_IDX_YRES: u16 = 2;
const BGA_IDX_BPP: u16 = 3;
const BGA_IDX_ENABLE: u16 = 4;
const BGA_IDX_VIRT_W: u16 = 7;
const BGA_IDX_VIRT_H: u16 = 8;
const BGA_ENABLE: u16 = 0x01 | 0x40; // enabled + 32-bit LFB, no clear mem
const BGA_VERSION_MIN: u16 = 0xB0C0;

fn bga_write(idx: u16, val: u16) {
    io_out16(BGA_INDEX, idx);
    io_out16(BGA_VALUE, val);
}
fn bga_read(idx: u16) -> u16 {
    io_out16(BGA_INDEX, idx);
    io_in16(BGA_VALUE)
}

#[derive(Copy, Clone, Debug)]
pub struct VideoMode {
    pub width: u16,
    pub height: u16,
    pub bpp: u16,
    /// VBE mod numarası karşılığı (lineer framebuffer bayraklı).
    pub vbe_mode: u16,
}

pub const SUPPORTED_MODES: [VideoMode; 4] = [
    VideoMode {
        width: 800,
        height: 600,
        bpp: 32,
        vbe_mode: 0x4115,
    },
    VideoMode {
        width: 1024,
        height: 768,
        bpp: 32,
        vbe_mode: 0x4118,
    },
    VideoMode {
        width: 1280,
        height: 720,
        bpp: 32,
        vbe_mode: 0x41FF,
    },
    VideoMode {
        width: 1920,
        height: 1080,
        bpp: 32,
        vbe_mode: 0x41FF,
    },
];

#[derive(Copy, Clone, Debug)]
pub struct GpuCaps {
    pub bga_present: bool,
    pub bga_version: u16,
    pub vmware_present: bool,
    pub vga_present: bool,
    pub virtio_gpu_present: bool,
    pub nvidia_present: bool,
    pub amd_present: bool,
}

static mut CAPS: GpuCaps = GpuCaps {
    bga_present: false,
    bga_version: 0,
    vmware_present: false,
    vga_present: false,
    virtio_gpu_present: false,
    nvidia_present: false,
    amd_present: false,
};
static mut HW_CURSOR_ENABLED: bool = false;
static mut HW_CURSOR_X: u16 = 0;
static mut HW_CURSOR_Y: u16 = 0;
// Yazilim VSync yedegi: donanim 0x3DA dikey-blank portu calismadiginda
// (VirtualBox vb.) kare hizi ~60 Hz'de sinirlandirilir, ekran yirtilmesini
// onlemek icin. Son cagri zamani monotonic_ms cinsinden saklanir.
static mut VSYNC_FALLBACK_LAST_MS: u64 = 0;
/// Yazilim VSync yedegi icin hedef kare araligi (ms). ~60 Hz.
const VSYNC_FALLBACK_INTERVAL_MS: u64 = 16;

fn detect_vga() -> (bool, bool, bool) {
    let mut vga = false;
    let mut vmware = false;
    let mut bga_pci = false;
    for dev in 0..32u8 {
        for func in 0..8u8 {
            let id = pci_read32(0, dev, func, 0x00);
            if id == 0xFFFF_FFFF || id == 0 {
                continue;
            }
            let class = pci_read32(0, dev, func, 0x08) >> 16;
            if (class & 0xFFFF) == 0x0300 {
                vga = true;
            }
            let vid = (id & 0xFFFF) as u16;
            let did = ((id >> 16) & 0xFFFF) as u16;
            if vid == 0x15AD && did == 0x0405 {
                vmware = true;
            }
            if vid == 0x1234 && did == 0x1111 {
                bga_pci = true;
                vga = true;
            }
        }
    }
    (vga, vmware, bga_pci)
}

/// Boot sırasında çağrılır; yetenekleri tarar, sonucu döndürür.
pub fn init() -> GpuCaps {
    let (vga, vmware, bga_pci) = detect_vga();
    // BGA yalnızca gerçek QEMU/Bochs BGA PCI aygıtı (0x1234:0x1111) mevcutsa etkinleştirilir.
    // Gerçek donanımda (Intel/AMD/Nvidia) I/O portları 0x1CE/0x1CF'ye dokunulmaz.
    let id = if bga_pci { bga_read(BGA_IDX_ID) } else { 0 };
    let bga = bga_pci && (BGA_VERSION_MIN..0xB0C6).contains(&id);
    let virtio_gpu = crate::drivers::virtio_gpu::detect();
    if virtio_gpu {
        crate::drivers::virtio_gpu::init();
    }
    let nvidia = crate::drivers::nvidia::detect();
    if nvidia {
        crate::drivers::nvidia::init();
    }
    let amd = crate::drivers::amdgpu::detect();
    if amd {
        crate::drivers::amdgpu::init();
    }
    let caps = GpuCaps {
        bga_present: bga,
        bga_version: if bga { id } else { 0 },
        vmware_present: vmware,
        vga_present: vga || bga || virtio_gpu || nvidia || amd,
        virtio_gpu_present: virtio_gpu,
        nvidia_present: nvidia,
        amd_present: amd,
    };
    unsafe {
        CAPS = caps;
    }
    caps
}

pub fn caps() -> GpuCaps {
    unsafe { CAPS }
}

/// BGA üzerinden çözünürlük değiştirir. Başarılıysa yeni pitch'i (byte) döndürür.
/// NOT: BGA LFB adresi değişmez; caller framebuffer geometrisini güncellemelidir.
pub fn bga_set_mode(w: u16, h: u16, bpp: u16) -> Option<usize> {
    if unsafe { !CAPS.bga_present } {
        return None;
    }
    if w == 0 || h == 0 || (bpp != 32 && bpp != 24) {
        return None;
    }
    if w as usize > 1920 || h as usize > 1080 {
        return None;
    }
    // Önce kapat, geometriyi yaz, sonra aç (Bochs spec sırası).
    bga_write(BGA_IDX_ENABLE, 0);
    bga_write(BGA_IDX_XRES, w);
    bga_write(BGA_IDX_YRES, h);
    bga_write(BGA_IDX_BPP, bpp);
    bga_write(BGA_IDX_VIRT_W, w);
    bga_write(BGA_IDX_VIRT_H, h);
    bga_write(BGA_IDX_ENABLE, BGA_ENABLE);
    let x = bga_read(BGA_IDX_XRES);
    let y = bga_read(BGA_IDX_YRES);
    if x != w || y != h {
        return None;
    }
    Some(w as usize * (bpp as usize / 8))
}

/// İstenen çözünürlüğü dener: önce BGA, sonra sanal (ölçekli) moda düşer.
/// Dönüş: (fiziksel_değişti, pitch).
pub fn try_set_resolution(w: usize, h: usize) -> (bool, Option<usize>) {
    if w > 1920 || h > 1080 || w == 0 || h == 0 {
        return (false, None);
    }
    if unsafe { CAPS.virtio_gpu_present }
        && crate::drivers::virtio_gpu::set_mode(w as u32, h as u32)
    {
        return (true, Some(w * 4));
    }
    if unsafe { CAPS.nvidia_present } && crate::drivers::nvidia::set_mode(w as u32, h as u32, 32) {
        return (true, Some(w * 4));
    }
    if unsafe { CAPS.amd_present } && crate::drivers::amdgpu::set_mode(w as u32, h as u32, 32) {
        return (true, Some(w * 4));
    }
    if let Some(pitch) = bga_set_mode(w as u16, h as u16, 32) {
        // Kısa bir dikey-blank beklemesi: yırtılmayı azaltır.
        vsync_wait();
        return (true, Some(pitch));
    }
    (false, None)
}

pub fn mode_supported(w: usize, h: usize) -> bool {
    SUPPORTED_MODES
        .iter()
        .any(|m| m.width as usize == w && m.height as usize == h)
}

fn io_out8(port: u16, val: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
    }
}

/// VGA CRTC üzerinden donanım metin-imleci konumu (text-mode varsa işe yarar;
/// grafik modda no-op + hızlandırmalı yazılım imleç bayrağı kurulur).
pub fn hw_cursor_move(x: u16, y: u16) {
    unsafe {
        HW_CURSOR_X = x;
        HW_CURSOR_Y = y;
    }
    // VGA CRTC cursor location (3D4/3D5 idx 0x0E/0x0F) — yalnızca VGA mevcutsa dene.
    if unsafe { !CAPS.vga_present } {
        return;
    }
    // Standart 8-bit VGA CRTC port indeks/veri yazımı
    io_out8(0x3D4, 0x0E);
    io_out8(0x3D5, (y & 0xFF) as u8);
    io_out8(0x3D4, 0x0F);
    io_out8(0x3D5, (x & 0xFF) as u8);
}

pub fn hw_cursor_enable(enable: bool) {
    unsafe {
        HW_CURSOR_ENABLED = enable;
    }
    if unsafe { !CAPS.vga_present } {
        return;
    }
    io_out8(0x3D4, 0x0A);
    if enable {
        io_out8(0x3D5, 0x00); // cursor scanline start 0 → görünür
    } else {
        io_out8(0x3D5, 0x20); // bit5 = cursor off
    }
}

pub fn hw_cursor_enabled() -> bool {
    unsafe { HW_CURSOR_ENABLED }
}

/// Dikey-blank (vblank) beklemesi (VGA status port 0x3DA bit 3).
///
/// Donanım 0x3DA bit 3'ü (dikey retrace) hiç set etmiyorsa — örneğin
/// VirtualBox'ın VBoxVGA'sı veya donanım imleci olmayan özel GPU'lar —
/// her karede boşuna 30 bin iterasyon spin yapmak fare/klavye gecikmesi
/// üretiyordu. Bu yüzden destek bir kez yoklanır; yoksa bekleme tamamen
/// devre dışı bırakılır.
///
/// Durum: 0 = henüz yoklanmadı, 1 = vblank destekleniyor, 2 = destek yok.
static VBLANK_STATE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Donanim vblank yokken kac `vsync_wait` cagrisindan sonra yeniden yoklanir.
///
/// Tek seferlik yoklama kalici bir karara donusuyordu: erken (orn. zaman
/// tabani kalibre edilmeden) yapilan bir yoklama basarisiz olursa sistem
/// kalan omru boyunca yanlislikla "donanim vblank yok" modunda kaliyor ve
/// LFB'ye yazma ekranin ortasina denk geliyordu. Periyodik yeniden
/// yoklama bu kilidi kirar.
const VBLANK_REPROBE_INTERVAL: u32 = 1800;

/// Yeniden yoklama sayaci.
static VBLANK_REPROBE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Olculen kare periyodu (iki vblank darbesi arasi), mikrosaniye.
static FRAME_PERIOD_US: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Son vblank darbesinin TSC dongu sayaci.
static LAST_VBLANK_CYCLES: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// 0x3DA durum portunu okur (dikey retrace durumu).
#[inline]
fn vga_input_status() -> u8 {
    let s: u8;
    unsafe {
        core::arch::asm!("in al, dx", in("dx") 0x3DAu16, out("al") s, options(nomem, nostack, preserves_flags));
    }
    s
}

/// Donanımın dikey-blank bildirip bildirmediğini bir kez yoklar.
fn probe_vblank() -> bool {
    // ~50 ms pencerede bit 3 gözlenirse vblank bildiriliyor demektir.
    let mut saw_set = false;
    let mut saw_clear = false;
    let budget_cycles = unsafe { crate::boot::time::TSC_CYCLES_PER_US }
        .max(1)
        .saturating_mul(40_000);
    let start = crate::boot::time::read_cpu_cycles();
    for i in 0..200_000 {
        if vga_input_status() & 0x08 != 0 {
            saw_set = true;
        } else {
            saw_clear = true;
        }
        if saw_set && saw_clear {
            return true;
        }
        // Dongu butcesi her 64 yinelemeden bir kontrol edilir: rdtsc
        // maliyeti yoklamayi gereksiz yere uzatmasin.
        if i % 64 == 63 && crate::boot::time::read_cpu_cycles().wrapping_sub(start) > budget_cycles
        {
            break;
        }
        core::hint::spin_loop();
    }
    false
}

/// Vblank darbesini kaydeder ve kare periyodunu olcer.
fn note_vblank_edge() {
    let now = crate::boot::time::read_cpu_cycles();
    let last = LAST_VBLANK_CYCLES.swap(now, core::sync::atomic::Ordering::Relaxed);
    if last != 0 {
        let cycles_per_us = unsafe { crate::boot::time::TSC_CYCLES_PER_US }.max(1);
        let delta_us = (now.wrapping_sub(last) / cycles_per_us) as u32;
        // Makul aralik: 4 ms (250 Hz) .. 200 ms (5 Hz).
        if (4_000..=200_000).contains(&delta_us) {
            FRAME_PERIOD_US.store(delta_us, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// Donanim vblank darbesi mevcut mu?
pub fn vblank_available() -> bool {
    VBLANK_STATE.load(core::sync::atomic::Ordering::Relaxed) == 1
}

/// Olculen kare periyodu (mikrosaniye). Bilinmiyorsa 0.
pub fn frame_period_us() -> u32 {
    FRAME_PERIOD_US.load(core::sync::atomic::Ordering::Relaxed)
}

/// Gercek vblank beklemesi (bit 3 set olana kadar, ust sinirli).
fn wait_for_vblank() {
    for _ in 0..30_000 {
        if vga_input_status() & 0x08 != 0 {
            break;
        }
        core::hint::spin_loop();
    }
}

/// Dikey-blank beklemesi (VGA status port 0x3DA bit 3).
///
/// Donanım 0x3DA bit 3'ü (dikey retrace) hiç set etmiyorsa — örneğin
/// VirtualBox'ta — 0x3DA durum portunu okur (dikey retrace durumu).
/// Donanım vblank yoksa yazilim VSync yedegiyle ~60 Hz'de kare hizi
/// sinirlandirilir, böylece ekran yirtilmesi onlenir.
pub fn vsync_wait() {
    match VBLANK_STATE.load(core::sync::atomic::Ordering::Relaxed) {
        1 => {
            wait_for_vblank();
            note_vblank_edge();
        }
        2 => {
            // Donanım vblank yok: yazilim VSync yedegiyle kare hizi sinirla.
            let now = crate::boot::time::monotonic_ms();
            let last = unsafe { VSYNC_FALLBACK_LAST_MS };
            if now.saturating_sub(last) < VSYNC_FALLBACK_INTERVAL_MS {
                let remaining = VSYNC_FALLBACK_INTERVAL_MS - now.saturating_sub(last);
                let start = now;
                while crate::boot::time::monotonic_ms().saturating_sub(start) < remaining {
                    core::hint::spin_loop();
                }
            }
            unsafe {
                VSYNC_FALLBACK_LAST_MS = crate::boot::time::monotonic_ms();
            }

            // Periyodik olarak yeniden yokla: ilk deneme erken bir anda
            // yapilmis veya donanim gec hazir olmus olabilir.
            let counter = VBLANK_REPROBE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if counter >= VBLANK_REPROBE_INTERVAL {
                VBLANK_REPROBE.store(0, core::sync::atomic::Ordering::Relaxed);
                if probe_vblank() {
                    VBLANK_STATE.store(1, core::sync::atomic::Ordering::Relaxed);
                    wait_for_vblank();
                    note_vblank_edge();
                }
            }
        }
        _ => {
            if probe_vblank() {
                VBLANK_STATE.store(1, core::sync::atomic::Ordering::Relaxed);
                wait_for_vblank();
                note_vblank_edge();
            } else {
                // Donanım vblank bildirmiyor: her karede boşuna beklemek yerine
                // yazilim VSync yedegine gec.
                VBLANK_STATE.store(2, core::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Donanım 2D Hızlandırma (Hardware 2D Acceleration: Rect Fill & Bit Blit)
// ---------------------------------------------------------------------------

/// Donanım hızlandırmalı 2D dikdörtgen dolgusu (Hardware Rect Fill).
/// Framebuffer veya Backbuffer üzerinde 64-bit / SSE talimatları ve önbellek hizalaması
/// kullanarak ultra hızlı piksel blok dolgusu gerçekleştirir.
///
/// # Safety
/// `dst`, `w * h` piksel kaplayacak şekilde eşlenmiş ve yazılabilir olmalı;
/// `pitch_words` satır adımını (piksel cinsinden) doğru vermelidir.
pub unsafe fn hw_accel_fill_rect(
    dst: *mut u32,
    pitch_words: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    color: u32,
) {
    if w == 0 || h == 0 || dst.is_null() {
        return;
    }

    for row in 0..h {
        let line_ptr = unsafe { dst.add((y + row) * pitch_words + x) };
        unsafe {
            let slice = core::slice::from_raw_parts_mut(line_ptr, w);
            slice.fill(color);
        }
    }
}

/// Donanım hızlandırmalı 2D blok kopyalama (Hardware Bit Blit).
/// Kaynak bellekten hedef ekrana/tampona satır satır optimize edilmiş veri kopyalama yapar.
///
/// # Safety
/// `src` ve `dst` ilgili `w * h` bölgesi için eşlenmiş, geçerli pointer
/// olmalı; pitch değerleri doğru verilmelidir. Bölgeler çakışmamalıdır.
pub unsafe fn hw_accel_bit_blit(
    src: *const u32,
    src_pitch_words: usize,
    dst: *mut u32,
    dst_pitch_words: usize,
    src_x: usize,
    src_y: usize,
    dst_x: usize,
    dst_y: usize,
    w: usize,
    h: usize,
) {
    if w == 0 || h == 0 || src.is_null() || dst.is_null() {
        return;
    }

    for row in 0..h {
        let src_row = unsafe { src.add((src_y + row) * src_pitch_words + src_x) };
        let dst_row = unsafe { dst.add((dst_y + row) * dst_pitch_words + dst_x) };
        unsafe {
            core::ptr::copy_nonoverlapping(src_row, dst_row, w);
        }
    }
}
