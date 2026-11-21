#![allow(dead_code, unused_variables, static_mut_refs)]

//! Maturity: SKELETON / WIP — Gerçek USB Audio cihaz tespiti yok,//! sadece Class sabitleri ve state machine mevcut.//! Bkz. docs/driver-maturity.md
//!
//! USB Audio Driver - USB ses kartları desteği
//!
//! Bu driver, USB ses kartlarını destekler. USB Audio Class 1.0 ve 2.0
//! spesifikasyonlarına uygun olarak iletişim kurar.
//!
//! TR: Bu sürücü, USB ses kartlarını destekler. USB Audio Class 1.0 ve 2.0
//! spesifikasyonlarına uygun olarak iletişim kurar.

// ---------------------------------------------------------------------------
// USB Audio Class Kodları
// ---------------------------------------------------------------------------

const USB_AUDIO_CLASS: u8 = 0x01;
const USB_AUDIO_SUBCLASS_AUDIOCONTROL: u8 = 0x01;
const USB_AUDIO_SUBCLASS_AUDIOSTREAMING: u8 = 0x02;
const USB_AUDIO_SUBCLASS_MIDISTREAMING: u8 = 0x03;

const USB_AUDIO_PROTOCOL_UNDEFINED: u8 = 0x00;
const USB_AUDIO_PROTOCOL_VERSION_1_00: u8 = 0x10;
const USB_AUDIO_PROTOCOL_VERSION_2_00: u8 = 0x20;

// ---------------------------------------------------------------------------
// USB Audio Sürücü Durumu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioState {
    NotDetected,
    Detected,
    Initializing,
    Ready,
    Error,
    Playing,
    Recording,
}

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut STATE: AudioState = AudioState::NotDetected;
static mut DEVICE_FOUND: bool = false;
static mut DEVICE_INDEX: u8 = u8::MAX;
static mut NUM_CHANNELS: u8 = 2;
static mut SAMPLE_RATE: u32 = 48000;
static mut BITS_PER_SAMPLE: u8 = 16;
static mut VID: u16 = 0;
static mut PID: u16 = 0;
static mut LAYOUT: AudioLayout = AudioLayout::empty();

// ---------------------------------------------------------------------------
// Isochronous streaming durumu
// ---------------------------------------------------------------------------

/// PCM aşamalama halkası boyutu (bayt). Tek yön için 32 KiB.
const PCM_RING: usize = 32 * 1024;

/// Açık playback (OUT) uç nokta adresi; 0 = kapalı.
static mut PLAY_EP: u8 = 0;
/// Açık recording (IN) uç nokta adresi; 0 = kapalı.
static mut REC_EP: u8 = 0;

/// Gönderim (host→cihaz) PCM halkası.
static mut TX_PCM: [u8; PCM_RING] = [0; PCM_RING];
static mut TX_W: usize = 0;
static mut TX_R: usize = 0;
/// Alım (cihaz→host) PCM halkası.
static mut RX_PCM: [u8; PCM_RING] = [0; PCM_RING];
static mut RX_W: usize = 0;
static mut RX_R: usize = 0;

#[inline]
unsafe fn ring_used(w: usize, r: usize) -> usize {
    if w >= r {
        w - r
    } else {
        PCM_RING - r + w
    }
}

#[inline]
unsafe fn ring_free(w: usize, r: usize) -> usize {
    PCM_RING - ring_used(w, r)
}

// ---------------------------------------------------------------------------
// Endpoint / Arayüz Keşfi
// ---------------------------------------------------------------------------

/// Keşfedilen tek bir ses uç noktası.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AudioEndpoint {
    /// Endpoint numarası (adres baytı: yön + numara).
    pub address: u8,
    /// true = cihazdan host'a (IN), false = host'tan cihaza (OUT).
    pub dir_in: bool,
    /// Maksimum paket boyutu.
    pub max_packet: u16,
    /// Servis aralığı (ms cinsinden frame).
    pub interval: u8,
    /// Bağlı olduğu arayüz numarası.
    pub interface: u8,
    /// Bu uç noktasının ait olduğu alt ayar (bAlternateSetting).
    pub alt_setting: u8,
}

/// Yapılandırma tanımlayıcısından çıkarılan ses aygıtı düzeni.
#[derive(Clone, Copy)]
pub struct AudioLayout {
    /// AudioControl arayüz sayısı.
    pub control_interfaces: u8,
    /// AudioStreaming arayüz sayısı.
    pub streaming_interfaces: u8,
    /// Keşfedilen iso-chronous uç noktaları.
    pub endpoints: [AudioEndpoint; 8],
    /// Geçerli uç nokta sayısı.
    pub endpoint_count: u8,
    /// UAC sürümü: 0 = bilinmiyor, 1 = UAC1, 2 = UAC2.
    pub uac_version: u8,
    /// AudioStreaming FORMAT_TYPE'tan çıkarılan biçim bilgisi.
    pub format_channels: u8,
    pub format_subslot: u8,
    pub format_bits: u8,
    /// Desteklenen örnekleme hızları (Hz, en fazla 4).
    pub sample_rates: [u32; 4],
    pub sample_rate_count: u8,
    /// Feature Unit birim kimliği (0 = yok).
    pub feature_unit: u8,
    /// Feature Unit volume/mute denetimlerini destekliyor mu.
    pub has_volume: bool,
    pub has_mute: bool,
    /// AudioControl arayüz numarası (kontrol istekleri wIndex'i için).
    pub control_iface: u8,
    /// AudioStreaming arayüz numarası (örnekleme frekansı wIndex'i için).
    pub stream_iface: u8,
}

impl AudioLayout {
    const fn empty() -> Self {
        Self {
            control_interfaces: 0,
            streaming_interfaces: 0,
            endpoints: [AudioEndpoint {
                address: 0,
                dir_in: false,
                max_packet: 0,
                interval: 0,
                interface: 0,
                alt_setting: 0,
            }; 8],
            endpoint_count: 0,
            uac_version: 0,
            format_channels: 0,
            format_subslot: 0,
            format_bits: 0,
            sample_rates: [0; 4],
            sample_rate_count: 0,
            feature_unit: 0,
            has_volume: false,
            has_mute: false,
            control_iface: 0,
            stream_iface: 0,
        }
    }

    /// Playback (host→cihaz) için uygun OUT uç noktası var mı?
    pub fn has_playback_endpoint(&self) -> bool {
        self.endpoints[..self.endpoint_count as usize]
            .iter()
            .any(|e| !e.dir_in)
    }

    /// Recording (cihaz→host) için uygun IN uç noktası var mı?
    pub fn has_record_endpoint(&self) -> bool {
        self.endpoints[..self.endpoint_count as usize]
            .iter()
            .any(|e| e.dir_in)
    }
}

/// USB yapılandırma tanımlayıcısını ayrıştırır ve ses arayüzlerini bulur.
///
/// `config` tam yapılandırma tanımlayıcısı olmalıdır (9 baytlık başlık dahil).
/// Ses sınıfı (0x01) arayüzleri ve iso-chronous uç noktaları (descriptor
/// type 0x05, attributes & 0x03 == 0x01) döndürülür. UAC2 protokolü (0x20)
/// görülürse `uac_version = 2` olarak işaretlenir.
pub fn parse_audio_config(config: &[u8]) -> AudioLayout {
    let mut layout = AudioLayout::empty();
    let mut i = 0usize;
    // (interface, class, subclass, protocol, alt_setting)
    let mut current: Option<(u8, u8, u8, u8, u8)> = None;

    while i + 2 <= config.len() {
        let len = config[i] as usize;
        let dtype = config[i + 1];
        if len < 2 || i + len > config.len() {
            break;
        }

        if dtype == 0x04 && len >= 9 {
            // Interface descriptor
            let iface = config[i + 2];
            let alt = config[i + 3];
            let class = config[i + 5];
            let subclass = config[i + 6];
            let protocol = config[i + 7];
            if class == USB_AUDIO_CLASS {
                match subclass {
                    USB_AUDIO_SUBCLASS_AUDIOCONTROL => {
                        if alt == 0 {
                            layout.control_interfaces += 1;
                        }
                        if protocol == USB_AUDIO_PROTOCOL_VERSION_2_00 {
                            layout.uac_version = 2;
                        } else if layout.uac_version == 0
                            && protocol == USB_AUDIO_PROTOCOL_VERSION_1_00
                        {
                            layout.uac_version = 1;
                        }
                    }
                    USB_AUDIO_SUBCLASS_AUDIOSTREAMING => {
                        if alt == 0 {
                            layout.streaming_interfaces += 1;
                        }
                        if protocol == USB_AUDIO_PROTOCOL_VERSION_2_00 {
                            layout.uac_version = 2;
                        } else if layout.uac_version == 0
                            && protocol == USB_AUDIO_PROTOCOL_VERSION_1_00
                        {
                            layout.uac_version = 1;
                        }
                    }
                    _ => {}
                }
                if alt == 0 {
                    match subclass {
                        USB_AUDIO_SUBCLASS_AUDIOCONTROL if layout.control_iface == 0 => {
                            layout.control_iface = iface;
                        }
                        USB_AUDIO_SUBCLASS_AUDIOSTREAMING if layout.stream_iface == 0 => {
                            layout.stream_iface = iface;
                        }
                        _ => {}
                    }
                }
            }
            current = Some((iface, class, subclass, protocol, alt));
        } else if dtype == 0x05 && len >= 7 {
            // Endpoint descriptor — yalnızca iso-chronous uç noktalar
            if let Some((iface, class, _subclass, _proto, alt)) = current {
                if class == USB_AUDIO_CLASS {
                    let attributes = config[i + 3];
                    if attributes & 0x03 == 0x01 {
                        // Transfer type = Isochronous
                        let count = layout.endpoint_count as usize;
                        if count < 8 {
                            let max_packet =
                                ((config[i + 4] as u16) | ((config[i + 5] as u16) << 8)) & 0x7FF;
                            let address = config[i + 2];
                            layout.endpoints[count] = AudioEndpoint {
                                address,
                                dir_in: address & 0x80 != 0,
                                max_packet: max_packet.max(1),
                                interval: config[i + 6],
                                interface: iface,
                                alt_setting: alt,
                            };
                            layout.endpoint_count += 1;
                        }
                    }
                }
            }
        } else if dtype == 0x24 && len >= 4 {
            // Class-specific interface descriptor (CS_INTERFACE).
            // Alt türü config[i + 2]'dedir.
            if let Some((_iface, class, subclass, protocol, _alt)) = current {
                if class != USB_AUDIO_CLASS {
                    i += len;
                    continue;
                }
                let subtype = config[i + 2];
                match (subclass, subtype) {
                    // AudioControl: Feature Unit (volume/mute denetimi).
                    (USB_AUDIO_SUBCLASS_AUDIOCONTROL, 0x06) if len >= 6 => {
                        let unit = config[i + 3];
                        let control_size = config[i + 5] as usize;
                        if unit != 0 && control_size > 0 {
                            // Kanal bitmap'leri i+6'dan itibaren;
                            // ilk girdi master (channel 0) kanalıdır.
                            let mut j = 6usize;
                            let mut channels = 0usize;
                            while j + control_size <= i + len && channels < 8 {
                                let b0 = config[j];
                                if b0 & 0x01 != 0 {
                                    layout.has_mute = true;
                                }
                                if b0 & 0x02 != 0 {
                                    layout.has_volume = true;
                                }
                                j += control_size;
                                channels += 1;
                            }
                            layout.feature_unit = unit;
                        }
                    }
                    // AudioStreaming: FORMAT_TYPE — kanal/biçim/örnekleme.
                    (USB_AUDIO_SUBCLASS_AUDIOSTREAMING, 0x02) if len >= 8 => {
                        if protocol == USB_AUDIO_PROTOCOL_VERSION_2_00 {
                            // UAC2: bmFormats(2B), bSubslotSize, bBitResolution,
                            // bSamFreqType, ardından 4 baytlık frekanslar.
                            layout.format_subslot = config[i + 5];
                            layout.format_bits = config[i + 6];
                            let nr = config[i + 7] as usize;
                            let mut k = 0usize;
                            let mut off = i + 8;
                            while k < nr && k < 4 && off + 4 <= i + len {
                                let f = u32::from_le_bytes([
                                    config[off],
                                    config[off + 1],
                                    config[off + 2],
                                    config[off + 3],
                                ]);
                                if f != 0 {
                                    layout.sample_rates[k] = f;
                                    layout.sample_rate_count += 1;
                                }
                                k += 1;
                                off += 4;
                            }
                            layout.format_channels = 2; // UAC2'de kanal sayısı
                                                        // AS_GENERAL'dan gelir; burada güvenli varsayılan.
                        } else {
                            // UAC1: bFormatType, bNrChannels, bSubframeSize,
                            // bBitResolution, bSamFreqType, 3 baytlık frekanslar.
                            if config[i + 3] == 0x01 {
                                // Yalnızca PCM (FORMAT_TYPE_I).
                                layout.format_channels = config[i + 4];
                                layout.format_subslot = config[i + 5];
                                layout.format_bits = config[i + 6];
                                let nr = config[i + 7] as usize;
                                let mut k = 0usize;
                                let mut off = i + 8;
                                while k < nr && k < 4 && off + 3 <= i + len {
                                    let f = (config[off] as u32)
                                        | ((config[off + 1] as u32) << 8)
                                        | ((config[off + 2] as u32) << 16);
                                    if f != 0 {
                                        layout.sample_rates[k] = f;
                                        layout.sample_rate_count += 1;
                                    }
                                    k += 1;
                                    off += 3;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        i += len;
    }

    layout
}

/// USB enumerasyonundan gelen yapılandırma tanımlayıcısını işler.
///
/// `usb_hid` xHCI enumerasyonu her aygıtın yapılandırma tanımlayıcısını
/// okuduğunda çağrılır. Ses sınıfı arayüzü bulunursa aygıt "Detected"
/// durumuna geçer; uç nokta ayrıntıları `audio_layout()` ile sorgulanır.
pub fn note_config_descriptor(config: &[u8], vendor: u16, product: u16) {
    unsafe {
        let layout = parse_audio_config(config);
        if layout.control_interfaces == 0 && layout.streaming_interfaces == 0 {
            return;
        }

        if !DEVICE_FOUND {
            crate::serial::print("[USB-AUDIO] USB ses aygıtı algılandı (class 0x01).\n");
        }
        DEVICE_FOUND = true;
        VID = vendor;
        PID = product;
        LAYOUT = layout;
        if STATE == AudioState::NotDetected || STATE == AudioState::Error {
            STATE = AudioState::Detected;
        }
    }
}

/// Genel USB aygıt katmanı ses aygıtını kaydettiğinde çağrılır
/// (`usb_hid::register_generic_device` → `notify_device_attached`).
/// `index` `usb_hid` genel aygıt tablosundaki kayıttır.
pub fn usb_device_attached(index: u8) {
    unsafe {
        DEVICE_INDEX = index;
        if STATE == AudioState::NotDetected || STATE == AudioState::Detected {
            STATE = AudioState::Initializing;
        }
        crate::serial::print("[USB-AUDIO] Genel aygıt kaydı bağlandı (indeks ");
        crate::serial::print_dec(index as u64);
        crate::serial::print(").\n");
    }
}

/// Genel USB aygıtı kaldırıldığında çağrılır.
pub fn usb_device_removed(index: u8) {
    unsafe {
        if DEVICE_INDEX == index {
            DEVICE_INDEX = u8::MAX;
            DEVICE_FOUND = false;
            LAYOUT = AudioLayout::empty();
            STATE = AudioState::NotDetected;
            crate::serial::print("[USB-AUDIO] Genel aygıt kaydı kaldırıldı.\n");
        }
    }
}

/// Kayıtlı genel aygıt indeksi (yoksa `u8::MAX`).
pub fn device_index() -> u8 {
    unsafe { DEVICE_INDEX }
}

// ---------------------------------------------------------------------------
// Cihaz Algılama
// ---------------------------------------------------------------------------

pub fn detect() -> bool {
    // Gerçek tespit USB enumerasyonu sırasında note_config_descriptor
    // üzerinden gelir; burada yalnızca kayıtlı durumu döndürürüz.
    unsafe { DEVICE_FOUND }
}

// ---------------------------------------------------------------------------
// Cihaz Başlatma
// ---------------------------------------------------------------------------

pub fn init() -> bool {
    unsafe {
        if !DEVICE_FOUND {
            return false;
        }

        STATE = AudioState::Initializing;

        // Keşfedilen düzen varsa kanal/süre parametrelerini ona göre
        // varsayılanlaştır; yoksa güvenli varsayılanlar kullanılır.
        if LAYOUT.uac_version == 2 {
            SAMPLE_RATE = 48000;
        } else {
            SAMPLE_RATE = 44100;
        }
        NUM_CHANNELS = 2;
        BITS_PER_SAMPLE = 16;

        // Uç nokta keşfi tamamlandı; streaming henüz bağlı değil.
        STATE = AudioState::Ready;

        true
    }
}

// ---------------------------------------------------------------------------
// API Fonksiyonları
// ---------------------------------------------------------------------------

pub fn is_detected() -> bool {
    unsafe { DEVICE_FOUND }
}

pub fn state() -> AudioState {
    unsafe { STATE }
}

pub fn name() -> &'static str {
    "USB Audio"
}

pub fn name_tr() -> &'static str {
    "USB Ses Kartı"
}

pub fn num_channels() -> u8 {
    unsafe { NUM_CHANNELS }
}

pub fn sample_rate() -> u32 {
    unsafe { SAMPLE_RATE }
}

pub fn bits_per_sample() -> u8 {
    unsafe { BITS_PER_SAMPLE }
}

/// VID:PID çifti (0:0 = henüz aygıt yok).
pub fn vid_pid() -> (u16, u16) {
    unsafe { (VID, PID) }
}

/// Keşfedilen ses arayüzü/uç nokta düzeni.
pub fn audio_layout() -> AudioLayout {
    unsafe { LAYOUT }
}

/// Örnekleme hızını ayarlar.
///
/// UAC1'de AudioStreaming arayüzünde `SET_CUR` / `SAMPLING_FREQ_CONTROL`
/// (0x01) isteği gönderilir; değer 3 baytlık Hz'tir. UAC2'de saat varlığı
/// (clock entity) modeli kullanılır — şimdilik yalnızca değeri saklarız.
pub fn set_sample_rate(rate: u32) -> bool {
    unsafe {
        let dev = DEVICE_INDEX;
        if dev == u8::MAX {
            return false;
        }
        if LAYOUT.uac_version == 2 {
            // TODO(Faz 1): UAC2 Clock Source entity kontrolü.
            SAMPLE_RATE = rate;
            return true;
        }
        let iface = LAYOUT.stream_iface;
        if iface == 0 {
            return false;
        }
        let data = [
            (rate & 0xFF) as u8,
            ((rate >> 8) & 0xFF) as u8,
            ((rate >> 16) & 0xFF) as u8,
        ];
        let ok =
            crate::drivers::usb_hid::usb_control_out(dev, 0x21, 0x01, 0x0100, iface as u16, &data)
                .is_some();
        if ok {
            SAMPLE_RATE = rate;
        }
        ok
    }
}

/// OUT akış slotu boşaldığında çağrılır: sıradaki PCM baytlarını yazar.
/// Halka boşsa slot sessiz (sıfır) kalır.
unsafe fn usb_tx_slot_freed(_dev: u8, ep_addr: u8, slot: u16) {
    unsafe {
        let Some(phys) = crate::drivers::usb_hid::usb_stream_slot(DEVICE_INDEX, ep_addr, slot)
        else {
            return;
        };
        let Some(size) = crate::drivers::usb_hid::usb_stream_slot_size(DEVICE_INDEX, ep_addr)
        else {
            return;
        };
        let v = crate::drivers::phys_to_virt(phys) as *mut u8;
        let mut i = 0usize;
        while i < size as usize {
            let b = if ring_used(TX_W, TX_R) > 0 {
                let b = TX_PCM[TX_R];
                TX_R = (TX_R + 1) % PCM_RING;
                b
            } else {
                0
            };
            core::ptr::write_volatile(v.add(i), b);
            i += 1;
        }
    }
}

/// IN akış slotu dolduğunda çağrılır: PCM baytlarını RX halkasına kopyalar.
unsafe fn usb_rx_slot_filled(_dev: u8, _ep_addr: u8, buf_phys: u64, len: usize) {
    unsafe {
        let src = crate::drivers::phys_to_virt(buf_phys) as *const u8;
        for i in 0..len {
            if ring_free(RX_W, RX_R) == 0 {
                break; // overrun: en eski veriyi düşürmek yerine yenisini at
            }
            RX_PCM[RX_W] = core::ptr::read_volatile(src.add(i));
            RX_W = (RX_W + 1) % PCM_RING;
        }
    }
}

/// Ses çalmayı başlatma: OUT iso uç noktasını akış olarak açar.
pub fn start_playback() -> bool {
    unsafe {
        if STATE != AudioState::Ready {
            return false;
        }
        let dev = DEVICE_INDEX;
        if dev == u8::MAX {
            return false;
        }
        if PLAY_EP != 0 {
            STATE = AudioState::Playing;
            return true;
        }
        let layout = LAYOUT;
        let Some(ep) = layout.endpoints[..layout.endpoint_count as usize]
            .iter()
            .find(|e| !e.dir_in)
        else {
            STATE = AudioState::Error;
            return false;
        };
        // Uç noktanın ait olduğu alt ayarı seç (çoğu aygıtta alt1 = streaming).
        if ep.alt_setting != 0
            && !crate::drivers::usb_hid::usb_set_interface(dev, ep.interface, ep.alt_setting)
        {
            STATE = AudioState::Error;
            return false;
        }
        let slot_size = (ep.max_packet as u32).max(64);
        if !crate::drivers::usb_hid::usb_open_endpoint(dev, ep.address, slot_size, true) {
            STATE = AudioState::Error;
            return false;
        }
        PLAY_EP = ep.address;
        let _ = crate::drivers::usb_hid::usb_stream_callbacks(
            dev,
            ep.address,
            None,
            Some(usb_tx_slot_freed),
        );
        // Tüm slotları sessizle.
        let depth = crate::drivers::usb_hid::usb_stream_depth(dev, ep.address).unwrap_or(0);
        for s in 0..depth {
            if let Some(phys) = crate::drivers::usb_hid::usb_stream_slot(dev, ep.address, s) {
                let v = crate::drivers::phys_to_virt(phys) as *mut u8;
                core::ptr::write_bytes(v, 0, slot_size as usize);
            }
        }
        TX_W = 0;
        TX_R = 0;
        STATE = AudioState::Playing;
        true
    }
}

/// Ses çalmayı durdurma: OUT akış uç noktasını kapatır.
pub fn stop_playback() -> bool {
    unsafe {
        if STATE != AudioState::Playing {
            return false;
        }
        let dev = DEVICE_INDEX;
        if PLAY_EP != 0 && dev != u8::MAX {
            let _ = crate::drivers::usb_hid::usb_close_endpoint(dev, PLAY_EP);
            PLAY_EP = 0;
        }
        STATE = AudioState::Ready;
        true
    }
}

/// Ses kaydı başlatma: IN iso uç noktasını akış olarak açar.
pub fn start_recording() -> bool {
    unsafe {
        if STATE != AudioState::Ready {
            return false;
        }
        let dev = DEVICE_INDEX;
        if dev == u8::MAX {
            return false;
        }
        if REC_EP != 0 {
            STATE = AudioState::Recording;
            return true;
        }
        let layout = LAYOUT;
        let Some(ep) = layout.endpoints[..layout.endpoint_count as usize]
            .iter()
            .find(|e| e.dir_in)
        else {
            STATE = AudioState::Error;
            return false;
        };
        if ep.alt_setting != 0
            && !crate::drivers::usb_hid::usb_set_interface(dev, ep.interface, ep.alt_setting)
        {
            STATE = AudioState::Error;
            return false;
        }
        let slot_size = (ep.max_packet as u32).max(64);
        if !crate::drivers::usb_hid::usb_open_endpoint(dev, ep.address, slot_size, true) {
            STATE = AudioState::Error;
            return false;
        }
        REC_EP = ep.address;
        let _ = crate::drivers::usb_hid::usb_stream_callbacks(
            dev,
            ep.address,
            Some(usb_rx_slot_filled),
            None,
        );
        RX_W = 0;
        RX_R = 0;
        STATE = AudioState::Recording;
        true
    }
}

/// Ses kaydı durdurma.
pub fn stop_recording() -> bool {
    unsafe {
        if STATE != AudioState::Recording {
            return false;
        }
        let dev = DEVICE_INDEX;
        if REC_EP != 0 && dev != u8::MAX {
            let _ = crate::drivers::usb_hid::usb_close_endpoint(dev, REC_EP);
            REC_EP = 0;
        }
        STATE = AudioState::Ready;
        true
    }
}

/// Ses verisi gönderme: PCM baytlarını TX halkasına ekler.
/// Halka doluysa `false` döner (kısmi kopya yapılır).
pub fn send_audio_data(data: &[u8]) -> bool {
    unsafe {
        if STATE != AudioState::Playing {
            return false;
        }
        let free = ring_free(TX_W, TX_R);
        if data.len() > free {
            return false;
        }
        for &b in data {
            TX_PCM[TX_W] = b;
            TX_W = (TX_W + 1) % PCM_RING;
        }
        true
    }
}

/// Ses verisi alma: RX halkasından PCM baytlarını boşaltır.
pub fn receive_audio_data(buffer: &mut [u8]) -> Option<usize> {
    unsafe {
        if STATE != AudioState::Recording {
            return None;
        }
        let used = ring_used(RX_W, RX_R);
        if used == 0 {
            return None;
        }
        let n = used.min(buffer.len());
        for item in buffer.iter_mut().take(n) {
            *item = RX_PCM[RX_R];
            RX_R = (RX_R + 1) % PCM_RING;
        }
        Some(n)
    }
}

/// Ses seviyesi (volume) ayarlama.
///
/// UAC1 Feature Unit `VOLUME_CONTROL` (0x03) `SET_CUR` isteği gönderir.
/// `channel`: 0 = master, 1..N = kanal; `volume`: 0..100.
/// Feature Unit veya kanal denetimi yoksa `false` döner.
pub fn set_volume(channel: u8, volume: u16) -> bool {
    unsafe {
        let dev = DEVICE_INDEX;
        if dev == u8::MAX || !LAYOUT.has_volume || LAYOUT.feature_unit == 0 {
            return false;
        }
        let vol = volume.min(100);
        let data = [(vol & 0xFF) as u8, (vol >> 8) as u8];
        let wvalue = ((channel as u16) << 8) | 0x03;
        let windex = ((LAYOUT.feature_unit as u16) << 8) | (LAYOUT.control_iface as u16);
        crate::drivers::usb_hid::usb_control_out(dev, 0x21, 0x01, wvalue, windex, &data).is_some()
    }
}

/// Sessiz (mute) ayarlama.
///
/// UAC1 Feature Unit `MUTE_CONTROL` (0x02) `SET_CUR` isteği gönderir.
pub fn set_mute(channel: u8, mute: bool) -> bool {
    unsafe {
        let dev = DEVICE_INDEX;
        if dev == u8::MAX || !LAYOUT.has_mute || LAYOUT.feature_unit == 0 {
            return false;
        }
        let data = [if mute { 1u8 } else { 0u8 }];
        let wvalue = ((channel as u16) << 8) | 0x02;
        let windex = ((LAYOUT.feature_unit as u16) << 8) | (LAYOUT.control_iface as u16);
        crate::drivers::usb_hid::usb_control_out(dev, 0x21, 0x01, wvalue, windex, &data).is_some()
    }
}
