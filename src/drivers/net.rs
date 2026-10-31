use crate::drivers::port_io::{io_in16, io_in8, io_out16, io_out32, io_out8};
use core::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy)]
pub struct MacAddress(pub [u8; 6]);

// ---------------------------------------------------------------------------
// Etkin ağ bağdaştırıcısı (otobüs seçimi)
// ---------------------------------------------------------------------------
// `netstack` tek bir mantıksal arayüz kullanır; hangi sürücünün onu
// beslediği burada tutulur. `e1000` modülü PCI (e1000 / VMXNET3 /
// RTL816x / RTL8139 / Broadcom), `ne2000` modülü ISA/PnP kartları sürer.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BusKind {
    None,
    Pci,
    IsaPnp,
}

static ACTIVE_BUS: AtomicU8 = AtomicU8::new(0);
static NE2000_DATA_WIDTH: AtomicU8 = AtomicU8::new(16);

pub fn set_ne2000_data_width(bits: u8) {
    NE2000_DATA_WIDTH.store(if bits == 8 { 8 } else { 16 }, Ordering::Relaxed);
}

pub fn ne2000_data_width() -> u8 {
    NE2000_DATA_WIDTH.load(Ordering::Relaxed)
}

pub fn set_bus(bus: BusKind) {
    ACTIVE_BUS.store(
        match bus {
            BusKind::None => 0,
            BusKind::Pci => 1,
            BusKind::IsaPnp => 2,
        },
        Ordering::SeqCst,
    );
}

pub fn active_bus() -> BusKind {
    match ACTIVE_BUS.load(Ordering::SeqCst) {
        1 => BusKind::Pci,
        2 => BusKind::IsaPnp,
        _ => BusKind::None,
    }
}

/// NE2000 / RTL8029AS ISA kartlar ISA PnP (veya jumper'lı) olarak çalışır.
pub fn is_isa() -> bool {
    active_bus() == BusKind::IsaPnp
}

// ---------------------------------------------------------------------------
// ISA PnP izolasyonu (NE2000 kartlarının I/O tabanı ve MAC'i)
// ---------------------------------------------------------------------------
// PnP kartları aynı anda 3 sabit portu (0x279, 0xA79, 0x203) izler. Isınan
// kart kısa bir giriş anahtarı ile izolasyona alınır, 72 baytlık PnP tanıtım
// yapısı kaydırılır, ardından kartın PnP kaynakları (I/O taban adresi, yoksa
// MAC adresi) okunur ve kart etkinleştirilir.

// PnP kartları aynı anda bu 3 adres portunu (ve ilgili veri portlarını)
// dinler: 0x279/0xA79 ve 0x203/0x3F3.
const PNP_ADDR_PORTS: [u16; 3] = [0x279, 0xA79, 0x203];
const PNP_WRITE_PORTS: [u16; 3] = [0xA79, 0xA79, 0x3F3];
/// Kart PnP kaynağı bildirmediyse denenecek ilk NE2000 adresi.
const PNP_DEFAULT_BASE: u16 = 0x300;
const PNP_ISOLATION_CTRL: u16 = 0x04B2;
const CSN_NONE: u32 = 0xFF;

static mut PNP_LAST_CARD: u8 = CSN_NONE as u8;

/// Kontrol portundan okunabilir bayt sayısı: tanıtım yapısının uzunluğu + 1.
unsafe fn pnp_read_length(port: u16) -> u8 {
    io_out8(PNP_ADDR_PORTS[2], 0x00);
    let mut count = 0u8;
    while io_in8(port) != 0x4D {
        count = count.wrapping_add(1);
        if count == 0 {
            return 0; // hiç kart yok / yanıt yok
        }
    }
    io_in8(port)
}

/// Tüm PnP kartlarını izolasyona alır; ilk kartın CSN'sini döndürür.
unsafe fn pnp_isolation_start() -> Option<u32> {
    let mut detected = false;
    for &port in PNP_ADDR_PORTS.iter() {
        if pnp_read_length(port) > 0 {
            detected = true;
            break;
        }
    }
    if !detected {
        return None;
    }

    io_out8(PNP_ISOLATION_CTRL, 0x02);
    // PnP kartlarının CSN sayacını sıfırla.
    io_out32(0x04C0, 0x00);
    // Giriş anahtarı (72 bit, LSB önce): 5 bayt 0x6A + 4 bayt 0x00.
    for _ in 0..32 {
        io_out8(PNP_ADDR_PORTS[1], 0x6A);
    }
    for _ in 0..32 {
        io_out8(PNP_ADDR_PORTS[1], 0x00);
    }
    PNP_LAST_CARD = 0;
    Some(0)
}

/// 72 baytlık tanıtım yapısında (bilinen geçerli) etiketi arar; etiketin
/// ilk baytını döndürür. Tanınmayan etiketler uzunluklarına göre atlanır.
unsafe fn pnp_read_tag(port: u16, tag: u8) -> Option<u8> {
    for _ in 0..72 {
        let t = io_in8(port);
        if (t >> 4) == (tag >> 4) {
            return Some(t & 0x0F);
        }
        // Tanınan büyük etiketlerin sabit uzunlukları (ETAG/MTAG/CfAG/RFG).
        let skip = match t >> 4 {
            0x2 => 4,
            0x3 => 9,
            0x4 => 8,
            0x5 => 17,
            0x6 => 1,
            _ => 0,
        };
        for _ in 0..skip {
            let _ = io_in8(port);
        }
    }
    None
}

/// Bir PnP kartının I/O tabanını (varsa MAC baytlarını) okur.
pub fn pnp_read_card(csn: u32) -> (Option<u16>, [u8; 6]) {
    // Okuma adresini CSN ile seç (yazma veri portundan yapılır).
    io_out8(PNP_WRITE_PORTS[1], 0x00);
    io_out8(PNP_WRITE_PORTS[1], csn as u8);
    let port = PNP_ISOLATION_CTRL + 1;
    unsafe {
        // ETAG (0x02): satıcı kimliği 3 bayt + aygıt kimliği 4 bayt
        let _ = pnp_read_tag(port, 0x02);
        for _ in 0..7 {
            let _ = io_in8(port);
        }
        // PnP yapısı olmayan kartlar MAC yazmaz.
        pnp_read_tag(port, 0x13);
        // MTAG (0x14): 48 bit MAC
        let mut mac = [0u8; 6];
        if pnp_read_tag(port, 0x14).is_none() {
            return (None, mac);
        }
        for b in mac.iter_mut() {
            *b = io_in8(port);
        }

        // CfAG (0x30): DMA kanalı
        let _ = pnp_read_tag(port, 0x30);
        let _ = io_in8(port);
        // RFG (0x4B): IRQ, I/O taban, I/O uzunluğu, DMA seçimi
        let mut io_base = None;
        if pnp_read_tag(port, 0x4B).is_some() {
            let _dma = io_in8(port);
            // I/O port maskesi (u16, küçük-endian)
            let mask_lo = io_in8(port);
            let mask_hi = io_in8(port);
            // IRQ maskesi (u16)
            let irq_lo = io_in8(port);
            let irq_hi = io_in8(port);
            let _dma_mask = io_in8(port);
            let _dma_mask2 = io_in8(port);
            let _vendor = io_in8(port);
            let _reserved = io_in8(port);
            let mask = ((mask_hi as u16) << 8) | mask_lo as u16;
            let _irqs = ((irq_hi as u16) << 8) | irq_lo as u16;
            // Bildirilen I/O maskesinin en yüksek bloğu taban adres olarak
            // kullanılır; maske boşsa varsayılan NE2000 adresine düşülür.
            let base = if mask != 0 {
                let mut high = mask;
                while high & 0xFF00 != 0 {
                    high &= high << 1;
                }
                high
            } else {
                PNP_DEFAULT_BASE
            };
            io_base = Some(base);
        }
        if io_base.is_some() {
            // CFG_CTRL: logik aygıt etkinleştir.
            io_out8(port, 0x30);
            io_out8(port, 0x01);
        }
        (io_base, mac)
    }
}

/// Tüm PnP kartlarının etkinleştirilmesini tamamlar ve bir sonraki yanıt
/// verebilen kartın CSN'sini döndürür. (Şimdilik tek kart desteklenir.)
pub fn pnp_isolate() -> Option<u32> {
    unsafe { pnp_isolation_start() }
}

pub fn pnp_last_card() -> u8 {
    unsafe { PNP_LAST_CARD }
}

// ---------------------------------------------------------------------------
// NE2000 bayt düzeyinde okuma / yazma (paket DMA'sı için)
// ---------------------------------------------------------------------------

/// NE2000 uzak-DMA işleminde kullanılan veri portu.
///
/// ISA NE2000'de veri yolu 16 bittir; DMA uzunluğu sözcük (16 bit)
/// birimindedir ve her port yazımı bir sözcük taşır. 8 bit kartlarda
/// (ör. NE2000 8-bit klonları) bayt bayt aktarım _gerekir_ ve uzunluk
/// bayt biriminde programlanır.
pub struct DataPort {
    pub port: u16,
    pub word: bool,
}

impl DataPort {
    pub fn write_bytes(&self, data: &[u8]) {
        if self.word {
            let mut i = 0;
            while i < data.len() {
                let lo = data[i] as u16;
                let hi = if i + 1 < data.len() {
                    data[i + 1] as u16
                } else {
                    0
                };
                io_out16(self.port, lo | (hi << 8));
                i += 2;
            }
        } else {
            for b in data {
                io_out16(self.port, *b as u16);
            }
        }
    }

    pub fn read_bytes(&self, out: &mut [u8]) {
        if self.word {
            let mut i = 0;
            while i < out.len() {
                let v = io_in16(self.port);
                out[i] = (v & 0xFF) as u8;
                if i + 1 < out.len() {
                    out[i + 1] = (v >> 8) as u8;
                }
                i += 2;
            }
        } else {
            for b in out.iter_mut() {
                *b = io_in16(self.port) as u8;
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct Ipv4Address(pub [u8; 4]);

pub struct EthernetPacket<'a> {
    pub src_mac: MacAddress,
    pub dst_mac: MacAddress,
    pub ethertype: u16,
    pub payload: &'a [u8],
}

pub struct NetworkInterface {
    pub mac: MacAddress,
    pub ip: Ipv4Address,
    pub subnet: Ipv4Address,
    pub gateway: Ipv4Address,
    pub packets_rx: usize,
    pub packets_tx: usize,
}

impl NetworkInterface {
    pub const fn new() -> Self {
        Self {
            mac: MacAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]), // QEMU standart MAC
            ip: Ipv4Address([192, 168, 1, 100]),
            subnet: Ipv4Address([255, 255, 255, 0]),
            gateway: Ipv4Address([192, 168, 1, 1]),
            packets_rx: 0,
            packets_tx: 0,
        }
    }

    pub fn receive_packet(&mut self, _data: &[u8]) {
        self.packets_rx += 1;
    }

    pub fn transmit_packet(&mut self, _data: &[u8]) {
        self.packets_tx += 1;
    }
}
