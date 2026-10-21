//! Limine onyukleme protokolu: istek/yanit yapilari, bellek haritasi ve cmdline yardimcilari.
//!
//! Panik aninda dogrudan cizim icin gereken video statikleri (PANIC_FB_*) de buradan
//! yeniden disa aktarilir. `src/main.rs` (Asama 1 refactor) ile ayristirildi.

// Limine Protokolü İstek Yapıları (Sıfır Dış Bağımlılık)
#[repr(C, align(8))]
pub struct LimineFramebufferRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineFramebufferResponse,
}

#[repr(C, align(8))]
pub struct LimineFramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: *const *const LimineFramebuffer,
}

#[derive(Clone, Copy)]
#[repr(C, align(8))]
pub struct LimineFramebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

unsafe impl Sync for LimineFramebufferRequest {}

#[used]
#[link_section = ".requests_start_marker"]
pub static LIMINE_REQUESTS_START_MARKER: [u64; 4] = [
    0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf,
    0x785c6ed015d3e316, 0x181e920a7852b9d9,
];

#[used]
#[link_section = ".requests_base_revision"]
pub static LIMINE_BASE_REVISION: [u64; 3] = [
    0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 3,
];

#[used]
#[link_section = ".requests"]
pub static FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x9d5827dcd881dd75,
        0xa3148604f6fab11b,
    ],
    revision: 0,
    response: core::ptr::null(),
};

#[used]
#[link_section = ".requests_end_marker"]
pub static LIMINE_REQUESTS_END_MARKER: [u64; 2] = [
    0xadc0e0531bb10d03, 0x9572709f31764c62,
];

// Panic anında ekrana doğrudan çizim yapabilmek için video sürücüsü statikleri kullanılır:
pub use crate::drivers::video::{PANIC_FB_ADDR, PANIC_FB_W, PANIC_FB_H, PANIC_FB_PITCH, PANIC_FB_BPP};

// --- Limine HHDM (Higher-Half Direct Map) istegi: fiziksel<->sanal donusum ---
#[repr(C, align(8))]
pub struct LimineHhdmResponse {
    pub revision: u64,
    pub offset: u64,
}

#[repr(C, align(8))]
pub struct LimineHhdmRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineHhdmResponse,
}

#[repr(C, align(8))]
pub struct LimineMemoryMapEntry {
    pub base: u64,
    pub length: u64,
    pub entry_type: u64,
    pub unused: [u64; 4],
}

#[repr(C, align(8))]
pub struct LimineMemoryMapResponse {
    pub revision: u64,
    pub entry_count: u64,
    pub entries: *const *const LimineMemoryMapEntry,
}

#[repr(C, align(8))]
pub struct LimineMemoryMapRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineMemoryMapResponse,
}

unsafe impl Sync for LimineMemoryMapRequest {}

unsafe impl Sync for LimineHhdmRequest {}

#[used]
#[link_section = ".requests"]
pub static HHDM_REQUEST: LimineHhdmRequest = LimineHhdmRequest {
    id: [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b, 0x48dcf1cb8ad2b852, 0x63984e959a98244b],
    revision: 0,
    response: core::ptr::null(),
};

#[repr(C, align(8))]
pub struct LimineStackSizeResponse {
    pub revision: u64,
}

#[repr(C, align(8))]
pub struct LimineStackSizeRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineStackSizeResponse,
    pub stack_size: u64,
}

unsafe impl Sync for LimineStackSizeRequest {}

#[used]
#[link_section = ".requests"]
pub static STACK_SIZE_REQUEST: LimineStackSizeRequest = LimineStackSizeRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x224ef0460a8e8926,
        0xe1cb0fc25f46ea3d,
    ],
    revision: 0,
    response: core::ptr::null(),
    stack_size: 1024 * 1024, // 1 MiB kernel stack
};

#[used]
#[link_section = ".requests"]
pub static MEMORY_MAP_REQUEST: LimineMemoryMapRequest = LimineMemoryMapRequest {
    id: [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b, 0x67cf3d9d378a806f, 0xe304acdfc50c3c62],
    revision: 0,
    response: core::ptr::null(),
};

// --- Limine Kernel File istegi: önyükleme komut satirini (cmdline) okur ---
// Boot menüsündeki "live" / "installed" seçimi bu cmdline ile kernel'e bildirilir.
#[repr(C, align(8))]
pub struct LimineKernelFileRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineKernelFileResponse,
}

#[repr(C, align(8))]
pub struct LimineKernelFileResponse {
    pub revision: u64,
    pub kernel_file: *const LimineFile,
}

/// `struct limine_file` başlangıcı. İlk alanlar (cmdline'a kadar) yeterlidir.
#[repr(C, align(8))]
pub struct LimineFile {
    pub revision: u64,
    pub address: *const u8,
    pub size: u64,
    pub path: *const u8,
    pub cmdline: *const u8,
}

unsafe impl Sync for LimineKernelFileRequest {}

#[used]
#[link_section = ".requests"]
pub static KERNEL_FILE_REQUEST: LimineKernelFileRequest = LimineKernelFileRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0xad97e90e83f1ed67,
        0x31eb5d1c5ff23b69,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// Limine'in sağladığı önyükleme komut satirini (cmdline) döndürür.
/// `None` ise cmdline alınamadı demektir (bu durumda canlı sistem kabul edilir).
pub fn limine_boot_cmdline() -> Option<&'static str> {
    unsafe {
        let resp = valid_limine_response(core::ptr::addr_of!(KERNEL_FILE_REQUEST.response))?;
        let file = core::ptr::read_volatile(core::ptr::addr_of!((*resp).kernel_file));
        if !is_mapped_limine_ptr(file) {
            return None;
        }
        let cmdline = core::ptr::read_volatile(core::ptr::addr_of!((*file).cmdline));
        if !is_mapped_limine_ptr(cmdline) {
            return None;
        }
        // NUL sonlandırıcıya kadar (en fazla 256 bayt) oku.
        let mut len = 0usize;
        while len < 256 {
            let byte = core::ptr::read_volatile(cmdline.add(len));
            if byte == 0 {
                break;
            }
            len += 1;
        }
        core::str::from_utf8(core::slice::from_raw_parts(cmdline, len)).ok()
    }
}

/// Limine tarafından belleğe yüklenmiş çekirdek dosyasının ikili dilimini döndürür.
pub fn limine_kernel_binary_slice() -> Option<&'static [u8]> {
    unsafe {
        let resp = valid_limine_response(core::ptr::addr_of!(KERNEL_FILE_REQUEST.response))?;
        let file = core::ptr::read_volatile(core::ptr::addr_of!((*resp).kernel_file));
        if !is_mapped_limine_ptr(file) {
            return None;
        }
        let addr = core::ptr::read_volatile(core::ptr::addr_of!((*file).address));
        let size = core::ptr::read_volatile(core::ptr::addr_of!((*file).size)) as usize;
        if addr.is_null() || size == 0 {
            return None;
        }
        Some(core::slice::from_raw_parts(addr, size))
    }
}

/// Önyükleme komut satirinda verilen token'i (kelime) arar.
pub fn cmdline_has_token(cmdline: &str, token: &str) -> bool {
    cmdline
        .split(|c: char| c.is_whitespace())
        .any(|part| part == token)
}

/// Önyükleme komut satırından `res=WxH` çözünürlük parametresini okur.
pub fn cmdline_resolution() -> Option<(usize, usize)> {
    let cmdline = limine_boot_cmdline()?;
    for part in cmdline.split(|c: char| c.is_whitespace()) {
        if let Some(val) = part.strip_prefix("res=") {
            let mut parts = val.split('x');
            if let (Some(w_str), Some(h_str)) = (parts.next(), parts.next()) {
                if let (Ok(w), Ok(h)) = (w_str.parse::<usize>(), h_str.parse::<usize>()) {
                    if w >= 320 && h >= 200 && w <= 2560 && h <= 1600 {
                        return Some((w, h));
                    }
                }
            }
        }
    }
    None
}

/// Kernel'in kurulu sistem modunda baslatilmak istenip istenmedigini bildirir.
pub fn boot_wants_installed() -> bool {
    limine_boot_cmdline().map_or(false, |cmdline| cmdline_has_token(cmdline, "installed"))
}

#[inline]
pub fn is_canonical_ptr<T>(ptr: *const T) -> bool {
    if ptr.is_null() {
        return false;
    }
    let addr = ptr as usize as u64;
    let upper = addr >> 47;
    (upper == 0 || upper == 0x1_FFFF) && addr % core::mem::align_of::<T>() as u64 == 0
}

/// Limine response pointers are expected in the mapped higher half. Reject
/// lower-half values so malformed boot data cannot direct these reads into an
/// unmapped region.
#[inline]
pub fn is_mapped_limine_ptr<T>(ptr: *const T) -> bool {
    is_canonical_ptr(ptr) && ((ptr as usize as u64) >> 47) == 0x1_FFFF
}

/// Limine yanıt yuvasını (`&REQUEST.response`) güvenli biçimde okur.
///
/// Okuma yapıldıktan sonra dönen işaretçi null olmamalı, doğru hizalanmalı ve
/// canonical bir adres olmalıdır. Tüm bu koşullar sağlandığında `Some` döner;
/// aksi halde `None` döner ve çağıran taraf yanıtı hiç dereference etmez.
/// Böylece tek bir noktada doğrulanan işaretçi CodeQL için de temiz kalır.
///
/// # Safety
/// `slot`, `&REQUEST.response` gibi geçerli ve okunabilir bir yuvaya işaret
/// etmelidir.
#[inline]
pub(crate) unsafe fn valid_limine_response<T>(slot: *const *const T) -> Option<*const T> {
    if slot.is_null() || !is_canonical_ptr(slot) {
        return None;
    }
    let ptr = core::ptr::read_volatile(slot);
    if !is_mapped_limine_ptr(ptr) { None } else { Some(ptr) }
}
