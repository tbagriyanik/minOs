//! MinOS Linux Donanım Köprüsü (Syscalls & POSIX interfaces)
//!
//! Linux çekirdeğinin donanım servislerine (DRM/Framebuffer, evdev, ALSA, Sockets)
//! sıfır libc bağımlılığıyla doğrudan Linux x86_64 sistem çağrıları (syscall) ile erişir.

use core::arch::asm;

pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_POLL: u64 = 7;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MUNMAP: u64 = 11;
pub const SYS_IOCTL: u64 = 16;
pub const SYS_SOCKET: u64 = 41;
pub const SYS_CONNECT: u64 = 42;
pub const SYS_SENDTO: u64 = 44;
pub const SYS_RECVFROM: u64 = 45;
pub const SYS_NANOSLEEP: u64 = 35;
pub const SYS_REBOOT: u64 = 169;

pub const O_RDONLY: i32 = 0;
pub const O_WRONLY: i32 = 1;
pub const O_RDWR: i32 = 2;
pub const O_NONBLOCK: i32 = 0x800;

pub const PROT_READ: i32 = 1;
pub const PROT_WRITE: i32 = 2;
pub const MAP_SHARED: i32 = 1;

#[inline(always)]
pub unsafe fn syscall1(n: u64, a1: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") n,
        in("rdi") a1,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall2(n: u64, a1: u64, a2: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") n,
        in("rdi") a1,
        in("rsi") a2,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") n,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack, preserves_flags)
    );
    ret
}

#[inline(always)]
pub unsafe fn syscall6(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> i64 {
    let ret: i64;
    asm!(
        "syscall",
        in("rax") n,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        in("r10") a4,
        in("r8") a5,
        in("r9") a6,
        out("rcx") _,
        out("r11") _,
        lateout("rax") ret,
        options(nostack, preserves_flags)
    );
    ret
}

// ---------------------------------------------------------------------------
// 1. Ekran Sürücüsü: Linux Framebuffer (/dev/fb0)
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct FbVarScreeninfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub xoffset: u32,
    pub yoffset: u32,
    pub bits_per_pixel: u32,
    pub grayscale: u32,
    pub red_offset: u32,
    pub red_length: u32,
    pub red_msb_right: u32,
    pub green_offset: u32,
    pub green_length: u32,
    pub green_msb_right: u32,
    pub blue_offset: u32,
    pub blue_length: u32,
    pub blue_msb_right: u32,
    pub transp_offset: u32,
    pub transp_length: u32,
    pub transp_msb_right: u32,
    pub nonstd: u32,
    pub activate: u32,
    pub height: u32,
    pub width: u32,
    pub accel_flags: u32,
    pub pixclock: u32,
    pub left_margin: u32,
    pub right_margin: u32,
    pub upper_margin: u32,
    pub lower_margin: u32,
    pub hsync_len: u32,
    pub vsync_len: u32,
    pub sync: u32,
    pub vmode: u32,
    pub rotate: u32,
    pub colorspace: u32,
    pub reserved: [u32; 4],
}

pub const FBIOGET_VSCREENINFO: u64 = 0x4600;

pub struct LinuxFramebufferDevice {
    pub fd: i32,
    pub ptr: *mut u32,
    pub width: usize,
    pub height: usize,
    pub bpp: usize,
    pub size: usize,
}

impl LinuxFramebufferDevice {
    pub fn open() -> Option<Self> {
        unsafe {
            let path = b"/dev/fb0\0";
            let fd = syscall3(SYS_OPEN, path.as_ptr() as u64, O_RDWR as u64, 0) as i32;
            if fd < 0 {
                return None;
            }

            let mut vinfo: FbVarScreeninfo = core::mem::zeroed();
            let ret = syscall3(SYS_IOCTL, fd as u64, FBIOGET_VSCREENINFO, &mut vinfo as *mut _ as u64);
            if ret < 0 {
                syscall1(SYS_CLOSE, fd as u64);
                return None;
            }

            let width = vinfo.xres as usize;
            let height = vinfo.yres as usize;
            let bpp = vinfo.bits_per_pixel as usize;
            let size = width * height * (bpp / 8);

            let map_ptr = syscall6(
                SYS_MMAP,
                0,
                size as u64,
                (PROT_READ | PROT_WRITE) as u64,
                MAP_SHARED as u64,
                fd as u64,
                0,
            );

            if map_ptr <= 0 || map_ptr == -1 {
                syscall1(SYS_CLOSE, fd as u64);
                return None;
            }

            Some(Self {
                fd,
                ptr: map_ptr as *mut u32,
                width,
                height,
                bpp,
                size,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// 2. USB Klavye & Fare Girdi Sürücüsü: Linux evdev (/dev/input/event*)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct InputEvent {
    pub sec: u64,
    pub usec: u64,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;
pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const REL_WHEEL: u16 = 0x08;
pub const BTN_LEFT: u16 = 0x110;
pub const BTN_RIGHT: u16 = 0x111;

pub struct LinuxInputHub {
    pub mouse_fd: i32,
    pub kbd_fd: i32,
}

impl LinuxInputHub {
    pub fn open() -> Self {
        unsafe {
            // Linux standart evdev girdi aygıtlarını tara
            let mut mouse_fd = -1;
            let mut kbd_fd = -1;

            // /dev/input/event0 .. event5
            let paths: [&[u8]; 4] = [
                b"/dev/input/event0\0",
                b"/dev/input/event1\0",
                b"/dev/input/event2\0",
                b"/dev/input/mice\0",
            ];

            for path in paths {
                let fd = syscall3(SYS_OPEN, path.as_ptr() as u64, (O_RDONLY | O_NONBLOCK) as u64, 0) as i32;
                if fd >= 0 {
                    if mouse_fd < 0 {
                        mouse_fd = fd;
                    } else if kbd_fd < 0 {
                        kbd_fd = fd;
                    }
                }
            }

            Self { mouse_fd, kbd_fd }
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Ağ Sürücüsü: Linux Socket API (Tam TCP/IP & HTTP Erişimi)
// ---------------------------------------------------------------------------

pub struct LinuxNetSocket;

impl LinuxNetSocket {
    pub fn http_get_request(ip: [u8; 4], port: u16, host: &str, path: &str, out: &mut [u8]) -> usize {
        unsafe {
            // socket(AF_INET=2, SOCK_STREAM=1, 0)
            let fd = syscall3(SYS_SOCKET, 2, 1, 0) as i32;
            if fd < 0 {
                return 0;
            }

            #[repr(C)]
            struct SockAddrIn {
                sin_family: u16,
                sin_port: u16,
                sin_addr: [u8; 4],
                sin_zero: [u8; 8],
            }

            let mut addr = SockAddrIn {
                sin_family: 2,
                sin_port: port.to_be(),
                sin_addr: ip,
                sin_zero: [0; 8],
            };

            let conn = syscall3(
                SYS_CONNECT,
                fd as u64,
                &mut addr as *mut _ as u64,
                core::mem::size_of::<SockAddrIn>() as u64,
            );

            if conn < 0 {
                syscall1(SYS_CLOSE, fd as u64);
                return 0;
            }

            // HTTP İsteği oluştur
            let mut req = [0u8; 512];
            let mut len = 0;
            let get = b"GET ";
            req[len..len + get.len()].copy_from_slice(get);
            len += get.len();
            let pb = path.as_bytes();
            req[len..len + pb.len()].copy_from_slice(pb);
            len += pb.len();
            let mid = b" HTTP/1.0\r\nHost: ";
            req[len..len + mid.len()].copy_from_slice(mid);
            len += mid.len();
            let hb = host.as_bytes();
            req[len..len + hb.len()].copy_from_slice(hb);
            len += hb.len();
            let tail = b"\r\nConnection: close\r\n\r\n";
            req[len..len + tail.len()].copy_from_slice(tail);
            len += tail.len();

            syscall3(SYS_WRITE, fd as u64, req.as_ptr() as u64, len as u64);

            // Yanıtı oku
            let n = syscall3(SYS_READ, fd as u64, out.as_mut_ptr() as u64, out.len() as u64);
            syscall1(SYS_CLOSE, fd as u64);

            if n > 0 { n as usize } else { 0 }
        }
    }
}

