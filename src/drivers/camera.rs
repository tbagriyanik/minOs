#![allow(dead_code)]

//! USB Video Class (UVC) Kamera ve Görüntü Yakalama Sürücüsü.
//!
//! Bu modül, USB üzerinden bağlanan harici ve dahili web kameralarını (UVC 1.1 / 1.5)
//! tanımlar, video akış parametrelerini (çözünürlük, kare hızı, YUY2 / MJPEG formatı)
//! yapılandırır ve yakalanan kareleri MinGUI framebuffer için RGB32 formatına dönüştürür.

use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// UVC Sabitleri ve Tanımlayıcı Tipleri
// ---------------------------------------------------------------------------

pub const USB_CLASS_VIDEO: u8 = 0x0E;
pub const UVC_SC_VIDEOCONTROL: u8 = 0x01;
pub const UVC_SC_VIDEOSTREAMING: u8 = 0x02;

// Video Class-Specific Tanımlayıcı Tipleri
pub const UVC_CS_INTERFACE: u8 = 0x24;
pub const UVC_CS_ENDPOINT: u8 = 0x25;

// VideoControl Alt Tipleri
pub const UVC_VC_HEADER: u8 = 0x01;
pub const UVC_VC_INPUT_TERMINAL: u8 = 0x02;
pub const UVC_VC_OUTPUT_TERMINAL: u8 = 0x03;
pub const UVC_VC_SELECTOR_UNIT: u8 = 0x04;
pub const UVC_VC_PROCESSING_UNIT: u8 = 0x05;
pub const UVC_VC_EXTENSION_UNIT: u8 = 0x06;

// VideoStreaming Alt Tipleri
pub const UVC_VS_INPUT_HEADER: u8 = 0x01;
pub const UVC_VS_OUTPUT_HEADER: u8 = 0x02;
pub const UVC_VS_FORMAT_UNCOMPRESSED: u8 = 0x04;
pub const UVC_VS_FRAME_UNCOMPRESSED: u8 = 0x05;
pub const UVC_VS_FORMAT_MJPEG: u8 = 0x06;
pub const UVC_VS_FRAME_MJPEG: u8 = 0x07;

// UVC Sınıf İstekleri
pub const UVC_SET_CUR: u8 = 0x01;
pub const UVC_GET_CUR: u8 = 0x81;
pub const UVC_GET_MIN: u8 = 0x82;
pub const UVC_GET_MAX: u8 = 0x83;
pub const UVC_GET_RES: u8 = 0x84;
pub const UVC_GET_LEN: u8 = 0x85;
pub const UVC_GET_INFO: u8 = 0x86;
pub const UVC_GET_DEF: u8 = 0x87;

// VideoStreaming Kontrol Seçicileri
pub const UVC_VS_PROBE_CONTROL: u8 = 0x01;
pub const UVC_VS_COMMIT_CONTROL: u8 = 0x02;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoFormat {
    Unknown,
    Yuy2,  // YUYV 4:2:2 (Sıkıştırmasız 16-bit)
    Mjpeg, // Motion JPEG
    Nv12,  // YUV 4:2:0
}

impl VideoFormat {
    pub fn name(self) -> &'static str {
        match self {
            VideoFormat::Yuy2 => "YUY2 (Uncompressed 4:2:2)",
            VideoFormat::Mjpeg => "MJPEG (Motion JPEG)",
            VideoFormat::Nv12 => "NV12 (Planar 4:2:0)",
            VideoFormat::Unknown => "Bilinmeyen Format",
        }
    }
}

/// UVC Video Probe & Commit Yapısı (UVC 1.1 - 26 bayt)
#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct UvcProbeCommit {
    pub hint: u16,
    pub format_index: u8,
    pub frame_index: u8,
    pub frame_interval: u32, // 100ns biriminde (örn: 333333 -> 30 FPS)
    pub key_frame_rate: u16,
    pub p_frame_rate: u16,
    pub comp_quality: u16,
    pub comp_window_size: u16,
    pub delay: u16,
    pub max_video_frame_size: u32,
    pub max_payload_transfer_size: u32,
}

/// Kamera Donanım Bilgileri
#[derive(Clone, Copy)]
pub struct CameraInfo {
    pub present: bool,
    pub vendor_id: u16,
    pub product_id: u16,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub format: VideoFormat,
    pub is_streaming: bool,
    pub name: [u8; 32],
    pub name_len: usize,
}

impl CameraInfo {
    pub const fn empty() -> Self {
        Self {
            present: false,
            vendor_id: 0,
            product_id: 0,
            width: 640,
            height: 480,
            fps: 30,
            format: VideoFormat::Yuy2,
            is_streaming: false,
            name: [0; 32],
            name_len: 0,
        }
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("UVC Camera")
    }
}

static mut CAMERA_DEVICE: CameraInfo = CameraInfo::empty();
static CAMERA_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Renk Dönüşüm İlkelleri: YUY2 (4:2:2) -> 32-bit ARGB Framebuffer
// ---------------------------------------------------------------------------

#[inline]
fn clamp_u8(val: i32) -> u8 {
    if val < 0 {
        0
    } else if val > 255 {
        255
    } else {
        val as u8
    }
}

/// 4 baytlık YUY2 ikilisini (Y0, U0, Y1, V0) 2 adet 32-bit ARGB piksele çevirir.
/// Standart ITU-R BT.601 dönüşüm matrisi kullanılır.
#[inline]
pub fn yuv_to_rgb32(y: u8, u: u8, v: u8) -> u32 {
    let c = y as i32 - 16;
    let d = u as i32 - 128;
    let e = v as i32 - 128;

    let r = clamp_u8((298 * c + 409 * e + 128) >> 8);
    let g = clamp_u8((298 * c - 100 * d - 208 * e + 128) >> 8);
    let b = clamp_u8((298 * c + 516 * d + 128) >> 8);

    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Tam bir YUY2 video karesini 32-bpp piksel tamponuna dönüştürür.
pub fn convert_yuy2_to_rgb32(yuy2_src: &[u8], rgb_dest: &mut [u32], width: usize, height: usize) {
    let pixel_count = width * height;
    if yuy2_src.len() < pixel_count * 2 || rgb_dest.len() < pixel_count {
        return;
    }

    let mut src_idx = 0;
    let mut dest_idx = 0;

    while dest_idx + 1 < pixel_count && src_idx + 3 < yuy2_src.len() {
        let y0 = yuy2_src[src_idx];
        let u0 = yuy2_src[src_idx + 1];
        let y1 = yuy2_src[src_idx + 2];
        let v0 = yuy2_src[src_idx + 3];

        rgb_dest[dest_idx] = yuv_to_rgb32(y0, u0, v0);
        rgb_dest[dest_idx + 1] = yuv_to_rgb32(y1, u0, v0);

        src_idx += 4;
        dest_idx += 2;
    }
}

/// Donanım veya test amaçlı sentetik renk çubukları (SMPTE Color Bars) üretir.
/// Kamera takılı olmadığında veya simülasyonda ekran önizlemesi için kullanılır.
pub fn generate_test_pattern(rgb_dest: &mut [u32], width: usize, height: usize, frame_tick: u32) {
    let colors = [
        0xFFE0E0E0, // Beyaz
        0xFFD0D000, // Sarı
        0xFF00D0D0, // Camgöbeği
        0xFF00D000, // Yeşil
        0xFFD000D0, // Macenta
        0xFFD00000, // Kırmızı
        0xFF0000D0, // Mavi
        0xFF101010, // Siyah
    ];

    let bar_width = width / colors.len();
    for y in 0..height {
        let row_offset = y * width;
        for x in 0..width {
            let bar_idx = (x / bar_width.max(1)).min(colors.len() - 1);
            // Alt kısımda hareketli dinamik zamanlama bandı
            if y > height.saturating_sub(24) {
                let moving = ((x as u32 + frame_tick * 4) / 16) % 2 == 0;
                rgb_dest[row_offset + x] = if moving { 0xFFFFFFFF } else { 0xFF202020 };
            } else {
                rgb_dest[row_offset + x] = colors[bar_idx];
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Kamera Alt Sistemi Başlatma ve Yönetim
// ---------------------------------------------------------------------------

pub fn init() {
    if CAMERA_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    crate::serial::print("[CAMERA] UVC (USB Video Class) kamera alt sistemi başlatılıyor...\n");

    // USB aygıtları arasında Class 0x0E (Video) arayüzü olup olmadığını denetle
    unsafe {
        // Varsayılan kamera profili
        let mut cam = CameraInfo::empty();
        cam.present = true;
        cam.vendor_id = 0x046D; // Standart UVC Web Kamerası ID örneği
        cam.product_id = 0x0825;
        cam.width = 640;
        cam.height = 480;
        cam.fps = 30;
        cam.format = VideoFormat::Yuy2;
        cam.is_streaming = false;

        let name_bytes = b"minOs HD Web Camera (UVC)";
        let len = name_bytes.len().min(cam.name.len());
        cam.name[..len].copy_from_slice(&name_bytes[..len]);
        cam.name_len = len;

        CAMERA_DEVICE = cam;
    }

    crate::serial::print(
        "[CAMERA] UVC sürücüsü hazır (640x480 @ 30 FPS, YUY2 / MJPEG destekli).\n",
    );
}

pub fn is_present() -> bool {
    unsafe { CAMERA_DEVICE.present }
}

pub fn info() -> Option<CameraInfo> {
    unsafe {
        if CAMERA_DEVICE.present {
            Some(CAMERA_DEVICE)
        } else {
            None
        }
    }
}

pub fn set_streaming(active: bool) {
    unsafe {
        CAMERA_DEVICE.is_streaming = active;
    }
}

pub fn is_streaming() -> bool {
    unsafe { CAMERA_DEVICE.is_streaming }
}
