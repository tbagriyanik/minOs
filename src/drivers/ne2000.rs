#![allow(dead_code, unused_variables, static_mut_refs)]

//! NE2000 (DP8390) uyumlu Ethernet sürücüsü.
//!
//! Gerçek ISA kartların çok büyük bölümünde bulunan Novell NE2000 / NE2000
//! klonları (RTL8019AS, RTL8029AS, DM9000 ailesi) ve QEMU'nun `-device
//! ne2k_isa,netdev=n0` emülasyonu burada sürülür. Kart PnP (ISA PnP) veya
//! sabit jumper I/O tabanı ile bulunur; paketler 8 KiB'lik uzak-DMA tamponu
//! (ring) üzerinden taşınır.
//!
//! Aynı giriş noktaları `e1000` modülündedir; otobüs seçimi
//! `net::set_bus(BusKind::IsaPnp)` ile yapılır. PCI RTL8029AS kartları için
//! `init_pci` giriş noktası vardır.

use crate::drivers::net::{self, BusKind, DataPort};
use crate::drivers::port_io::{io_in8, io_out8};
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// DP8390 / NE2000 kayıt ofsetleri (kasacık tabanına göre)
// ---------------------------------------------------------------------------
const REG_CR: u16 = 0x00; // Command
const REG_PSTART: u16 = 0x01; // Page 0: ring başlangıç sayfası
const REG_PSTOP: u16 = 0x02; // Page 0: ring bitiş sayfası
const REG_BNRY: u16 = 0x03; // Page 0: sonraki okunacak sayfa
const REG_TPSR: u16 = 0x04; // Page 0: TX paket başlangıç sayfası
const REG_TBCR0: u16 = 0x05; // Page 0: TX bayt sayacı (düşük)
const REG_TBCR1: u16 = 0x06; // Page 0: TX bayt sayacı (yüksek)
const REG_ISR: u16 = 0x07; // Page 0: Interrupt Status
const REG_RSAR0: u16 = 0x08; // Page 0: Uzak DMA adresi (düşük)
const REG_RSAR1: u16 = 0x09; // Page 0: Uzak DMA adresi (yüksek)
const REG_RBCR0: u16 = 0x0A; // Page 0: Uzak DMA bayt sayacı (düşük)
const REG_RBCR1: u16 = 0x0B; // Page 0: Uzak DMA bayt sayacı (yüksek)
const REG_RCR: u16 = 0x0C; // Page 0: Receive Configuration
const REG_TCR: u16 = 0x0D; // Page 0: Transmit Configuration
const REG_RSR: u16 = 0x0C; // Page 0: Receive Status (ACD bit 7 = 8 bit DAC)
const REG_DCR: u16 = 0x0E; // Page 0: Data Configuration
const REG_IMR: u16 = 0x0F; // Page 0: Interrupt Mask

const REG_PAR0: u16 = 0x01; // Page 1: Fiziksel adres 0..5
const REG_CURR: u16 = 0x07; // Page 1: DMA'ya yazılan sayfa
const REG_MAR0: u16 = 0x08; // Page 1: Multicast adres 0..7

const DATA_PORT_OFFSET: u16 = 0x10; // 16 bit veri portu
const RESET_PORT_OFFSET: u16 = 0x1F; // Reset (yalnız RTL8019AS)

// Command Register
const CR_STOP: u8 = 0x01;
const CR_START: u8 = 0x02;
const CR_TX: u8 = 0x04;
const CR_RD0: u8 = 0x08; // Uzak DMA okuma (karttan sisteme)
const CR_RD1: u8 = 0x10; // Uzak DMA yazma (sistemden karta)
const CR_PAGE0: u8 = 0x00;
const CR_PAGE1: u8 = 0x40;

// ISR bitleri
const ISR_PRX: u8 = 0x01; // Paket alındı
const ISR_PTX: u8 = 0x02; // Paket gönderildi
const ISR_RXE: u8 = 0x04; // RX hatası
const ISR_TXE: u8 = 0x08; // TX hatası
const ISR_OVW: u8 = 0x10; // RX halkası taştı
const ISR_RDC: u8 = 0x40; // Uzak DMA tamamlandı
const ISR_RST: u8 = 0x80; // Reset durumu

// DCR: FIFO eşiği (word transfer)
const DCR_FT1: u8 = 0x20;
const DCR_OS: u8 = 0x08; // Byte order: sözcük içi bayt sırası

// TCR
const TCR_NORMAL: u8 = 0x00;
const TCR_LOOPBACK: u8 = 0x02;

// RCR
const RCR_AB: u8 = 0x04;
const RCR_AM: u8 = 0x08;
const RCR_PRO: u8 = 0x10;
const RCR_MON: u8 = 0x20;

// 8 KiB DMA halkası: 16 KiB'lik kart RAM'inin ikinci yarısı.
// (8 KiB'lik kartlarda da 0x46..0x7F aralığı geçerlidir.)
// (QEMU ne2k_isa 16 KiB, 8 bit yuvalarda 8 KiB'e katlanır.)
const RX_START_PAGE: u8 = 0x46;
const RX_STOP_PAGE: u8 = 0x80;
const TX_START_PAGE: u8 = 0x40;
const PAGE_SIZE: usize = 256;

const TX_BUF_SIZE: usize = 2048;
const RX_BUF_SIZE: usize = 2048;

/// Taranacak yaygın NE2000 I/O tabanları (PnP çözümü yoksa).
const PROBE_BASES: [u16; 13] = [
    0x300, 0x280, 0x320, 0x340, 0x360, 0x380, 0x200, 0x220, 0x240, 0x260, 0x2C0, 0x2E0, 0x3E0,
];

static READY: AtomicBool = AtomicBool::new(false);
static mut IOBASE: u16 = 0;
static mut DATA_PORT: u16 = 0;
static mut NEXT_PAGE: u8 = RX_START_PAGE;
/// Bu aygıt RTL8029AS (PCI/ISA 8 bit) mi? Veri portu o zaman 8 bitir.
static mut IS_8BIT: bool = false;
/// IRQ / DMA kanalı (kart başlatmada kullanılmaz, raporlanır).
static mut CARD_IRQ: u8 = 0;
static mut CARD_DMA: u8 = 0;

// ---------------------------------------------------------------------------
// Yardımcılar
// ---------------------------------------------------------------------------

fn print(s: &str) {
    crate::serial::print(s);
}

fn print_hex(v: u8) {
    let hex = b"0123456789ABCDEF";
    crate::serial::write_byte(hex[(v >> 4) as usize]);
    crate::serial::write_byte(hex[(v & 0xF) as usize]);
}

#[inline(always)]
unsafe fn card_out(off: u16, val: u8) {
    io_out8(IOBASE + off, val);
}

#[inline(always)]
unsafe fn card_in(off: u16) -> u8 {
    io_in8(IOBASE + off)
}

unsafe fn select_page(page: u8) {
    let val = card_in(REG_CR) & 0x3F;
    card_out(REG_CR, (val & !0x40) | (page & 0xC0));
}

/// RTL8019AS reseti: I/O portundan ısıtma ardından normal dönüş.
unsafe fn reset_r8019() {
    card_in(RESET_PORT_OFFSET);
    for _ in 0..1000 {
        core::hint::spin_loop();
    }
    let _ = card_in(RESET_PORT_OFFSET);
    for _ in 0..1000 {
        core::hint::spin_loop();
    }
    // Reset sonrası ISR.RST bayrağını temizle.
    card_out(REG_ISR, ISR_RST);
    card_out(REG_IMR, 0x00);
}

/// MAC adresini kartın Page 1 PAR0..5 alanına yazar.
unsafe fn write_mac_par(mac: [u8; 6]) {
    select_page(CR_PAGE1);
    for (i, b) in mac.iter().enumerate() {
        card_out(REG_PAR0 + i as u16, *b);
    }
    select_page(CR_PAGE0);
}

/// Kart MAC'ini Page 1 PAR0..5 alanından okur.
unsafe fn read_mac_card() -> [u8; 6] {
    select_page(CR_PAGE1);
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = card_in(REG_PAR0 + i as u16);
    }
    select_page(CR_PAGE0);
    mac
}

/// Uzak DMA ile halkadan `len` bayt okur (CR.RD0).
unsafe fn dma_read(dst: &mut [u8], addr: u16, len: usize) {
    card_out(REG_ISR, ISR_RDC);
    card_out(REG_RBCR0, (len & 0xFF) as u8);
    card_out(REG_RBCR1, ((len >> 8) & 0xFF) as u8);
    card_out(REG_RSAR0, (addr & 0xFF) as u8);
    card_out(REG_RSAR1, ((addr >> 8) & 0xFF) as u8);
    card_out(REG_CR, CR_START | CR_RD0);

    let port = DataPort {
        port: DATA_PORT,
        word: !IS_8BIT && len % 2 == 0,
    };
    port.read_bytes(dst);
    // DMA tamamlanmasını bekle (RDC), sonra temizle.
    let mut spins = 0u32;
    while card_in(REG_ISR) & ISR_RDC == 0 {
        spins += 1;
        if spins > 100_000 {
            break;
        }
        core::hint::spin_loop();
    }
    card_out(REG_ISR, ISR_RDC);
}

/// Uzak DMA ile `src` baytlarını `addr` konumuna yazar (CR.RD1).
unsafe fn dma_write(src: &[u8], addr: u16) {
    card_out(REG_ISR, ISR_RDC);
    card_out(REG_RBCR0, (src.len() & 0xFF) as u8);
    card_out(REG_RBCR1, ((src.len() >> 8) & 0xFF) as u8);
    card_out(REG_RSAR0, (addr & 0xFF) as u8);
    card_out(REG_RSAR1, ((addr >> 8) & 0xFF) as u8);
    card_out(REG_CR, CR_START | CR_RD1);

    let port = DataPort {
        port: DATA_PORT,
        word: !IS_8BIT && src.len() % 2 == 0,
    };
    port.write_bytes(src);
}

/// RX halkasını başlatır ve Page 1 ayarlarını devre dışı bırakır.
unsafe fn init_ring() {
    select_page(CR_PAGE0);
    // Sayfa 0, DMA durdurulmuş:
    card_out(REG_CR, CR_STOP | CR_PAGE0);
    // 16 bit kartlarda FIFO eşiği ve bayt sırası; 8 bit kartlarda FIFO
    // ayarı uygulanmaz.
    let wts = if IS_8BIT { 0x00 } else { DCR_FT1 | DCR_OS };
    card_out(REG_DCR, wts);
    card_out(REG_RBCR0, 0x00);
    card_out(REG_RBCR1, 0x00);

    // TX / RX halkası sınırları
    card_out(REG_PSTART, RX_START_PAGE);
    card_out(REG_PSTOP, RX_STOP_PAGE);
    card_out(REG_BNRY, RX_START_PAGE);

    card_out(REG_IMR, 0x00);
    card_out(REG_ISR, 0xFF);

    // TX paket sayfası ve sayaçları
    card_out(REG_TPSR, TX_START_PAGE);
    card_out(REG_TBCR0, 0x00);
    card_out(REG_TBCR1, 0x00);

    // RX kabul modu: yayın + kendi + tümü.
    card_out(REG_RCR, RCR_AB | RCR_AM | RCR_PRO);

    card_out(REG_TCR, TCR_NORMAL);

    // Page 1: PAR + multicast maskesi + CURR
    select_page(CR_PAGE1);
    card_out(REG_CURR, RX_START_PAGE + 1);
    for i in 0..8 {
        card_out(REG_MAR0 + i, 0xFF);
    }
    select_page(CR_PAGE0);
}

/// Başlatma tamamlandıktan sonra RX/TX'i açar.
unsafe fn start_card() {
    card_out(REG_CR, CR_START | CR_PAGE0);
    card_out(REG_TCR, TCR_NORMAL);
    NEXT_PAGE = RX_START_PAGE + 1;
}

// ---------------------------------------------------------------------------
// Genel API (netstack bunları kullanır)
// ---------------------------------------------------------------------------

pub fn is_ready() -> bool {
    READY.load(Ordering::Relaxed)
}

pub fn link_up() -> bool {
    if !is_ready() {
        return false;
    }
    // Tipik ISA kartlarda link yoklaması yoktur: kart yanıt veriyorsa açık.
    unsafe {
        select_page(CR_PAGE0);
        let cr = card_in(REG_CR);
        select_page(CR_PAGE0);
        (cr & CR_START) != 0
    }
}

/// Ham Ethernet çerçevesi gönderir.
pub fn transmit(packet: &[u8]) -> bool {
    if !is_ready() || packet.is_empty() || packet.len() > TX_BUF_SIZE {
        return false;
    }
    unsafe {
        select_page(CR_PAGE0);

        // Paket karta yazılır (Ethernet minimumu 60 bayt; CRC donanımca eklenir).
        let mut buf = [0u8; 60];
        if packet.len() < 60 {
            buf[..packet.len()].copy_from_slice(packet);
            dma_write(&buf[..], (TX_START_PAGE as u16) << 8);
        } else {
            dma_write(packet, (TX_START_PAGE as u16) << 8);
        }

        // Önceki TX'ten kalan bayrakları temizle.
        card_out(REG_ISR, ISR_PTX | ISR_TXE);

        card_out(REG_TPSR, TX_START_PAGE);
        let len = packet.len().max(60);
        card_out(REG_TBCR0, (len & 0xFF) as u8);
        card_out(REG_TBCR1, ((len >> 8) & 0xFF) as u8);
        card_out(REG_CR, CR_START | CR_TX | CR_PAGE0);

        let mut spins = 0u32;
        while spins < 500_000 {
            let isr = card_in(REG_ISR);
            if isr & ISR_TXE != 0 {
                card_out(REG_ISR, ISR_TXE | ISR_PTX);
                return false;
            }
            if isr & ISR_PTX != 0 {
                card_out(REG_ISR, ISR_PTX);
                return true;
            }
            spins += 1;
            core::hint::spin_loop();
        }
        false
    }
}

/// RX halkasında okunmayı bekleyen paket var mı?
pub fn has_pending() -> bool {
    if !is_ready() {
        return false;
    }
    unsafe {
        select_page(CR_PAGE0);
        let isr = card_in(REG_ISR);
        select_page(CR_PAGE0);
        isr & (ISR_PRX | ISR_OVW) != 0
    }
}

/// RX halkasından bir paket okur; paket boyutunu (Ethernet çerçevesi)
/// döndürür. Tampon doluysa 0.
///
/// NE2000 halka girdisi: [durum][sonraki sayfa][boyut_lo][boyut_hi][çerçeve].
pub fn receive(buf: &mut [u8]) -> usize {
    if !is_ready() {
        return 0;
    }
    unsafe {
        select_page(CR_PAGE0);
        let isr = card_in(REG_ISR);
        if isr & (ISR_PRX | ISR_OVW) == 0 {
            return 0;
        }

        // Taşma: halkayı baştan temizle.
        if isr & ISR_OVW != 0 {
            card_out(REG_ISR, ISR_OVW);
            select_page(CR_PAGE0);
            card_out(REG_CR, CR_STOP | CR_PAGE0);
            card_out(REG_BNRY, RX_START_PAGE);
            select_page(CR_PAGE1);
            card_out(REG_CURR, RX_START_PAGE + 1);
            select_page(CR_PAGE0);
            card_out(REG_CR, CR_START | CR_PAGE0);
            NEXT_PAGE = RX_START_PAGE + 1;
            return 0;
        }

        // Ring halka durumu: BNRY (Page 0) ve CURR (Page 1).
        select_page(CR_PAGE0);
        let bnry = card_in(REG_BNRY);
        select_page(CR_PAGE1);
        let curr = card_in(REG_CURR);
        select_page(CR_PAGE0);
        if bnry == curr {
            return 0;
        }

        NEXT_PAGE = bnry + 1;
        if NEXT_PAGE >= RX_STOP_PAGE {
            NEXT_PAGE = RX_START_PAGE;
        }

        // 4 baytlık NE2000 başlığı oku
        let mut hdr = [0u8; 4];
        dma_read(&mut hdr, (bnry as u16) << 8, 4);
        let next = hdr[1];
        let len = (hdr[2] as usize) | ((hdr[3] as usize) << 8);

        // Halka boş bırakılmamışsa (durum 0) veri yoktur.
        if len == 0 || len > RX_BUF_SIZE - 4 {
            card_out(REG_BNRY, next);
            return 0;
        }

        let mut raw = [0u8; RX_BUF_SIZE];
        dma_read(&mut raw, ((bnry as u16) << 8) + 4, len);
        // BNRY, kartın bir sonraki yazacağı sayfanın bir öncesi olmalıdır.
        let stop = card_in(REG_PSTOP);
        let bnry_new = if next == RX_START_PAGE {
            stop - 1
        } else {
            next - 1
        };
        card_out(REG_BNRY, bnry_new);
        NEXT_PAGE = next;

        // CRC (4 bayt) atılır.
        let frame_len = len.saturating_sub(4);
        if frame_len == 0 || frame_len > buf.len() {
            return 0;
        }
        buf[..frame_len].copy_from_slice(&raw[..frame_len]);

        // Başka paket varsa PRX temizlenmiş olur; yeniden işaretle.
        select_page(CR_PAGE0);
        card_out(REG_ISR, ISR_PRX);
        frame_len
    }
}

// ---------------------------------------------------------------------------
// Başlatma
// ---------------------------------------------------------------------------

/// Kartın veri yolu genişliğini okur (DP8390: ISR[7] + RSR[7] = veri
/// yolu 8 bit (DAC). ISA 16 bit kartlarda bu bitler temizdir.
unsafe fn detect_data_bus(io_base: u16) {
    IOBASE = io_base;
    card_out(REG_IMR, 0x00);
    card_out(REG_ISR, 0xFF);
    let cr = card_in(REG_CR);
    card_out(REG_CR, cr | CR_STOP);
    select_page(CR_PAGE0);
    let is_8bit = card_in(REG_ISR) & 0x80 != 0 && card_in(REG_RSR) & 0x80 != 0;
    IS_8BIT = is_8bit;
    DATA_PORT = io_base + DATA_PORT_OFFSET;
    crate::drivers::net::set_ne2000_data_width(if is_8bit { 8 } else { 16 });
    let _ = card_in(REG_CR);
}

/// Belirtilen I/O tabanını başlatır. Başarılıysa MAC adresi
/// `e1000::MAC_ADDR` (netstack'in ortak adresi) içine yazılır ve
/// `net::set_bus(BusKind::IsaPnp)` ile otobüs seçimi tamamlanır.
pub fn init(io_base: u16) -> bool {
    if READY.load(Ordering::Relaxed) {
        return true;
    }
    unsafe {
        detect_data_bus(io_base);
        reset_r8019();
        let mut mac = read_mac_card();

        // MAC okuma başarısız olduysa Page 1'e yazılıp doğrulanır.
        if mac.iter().all(|&b| b == 0 || b == 0xFF) {
            mac = [0x00, 0x00, 0xC0, 0x00, 0x00, 0x01];
        }

        init_ring();
        write_mac_par(mac);
        start_card();

        crate::drivers::e1000::MAC_ADDR = mac;
        net::set_bus(BusKind::IsaPnp);
        READY.store(true, Ordering::Relaxed);

        print("[NET] NE2000 kart bulundu. IO:0x");
        print_hex((io_base >> 8) as u8);
        print_hex((io_base & 0xFF) as u8);
        print(" MAC:");
        for (i, b) in mac.iter().enumerate() {
            if i > 0 {
                print(":");
            }
            print_hex(*b);
        }
        print(if IS_8BIT { " (8-bit)" } else { " (16-bit)" });
        print("\n");
        true
    }
}

/// PCI NE2000 (RTL8029AS) kartını başlatır. Paketler kartın kendi 16 KiB'lik
/// RAM'inde uzak DMA ile taşındığı için DDR DMA belleği gerekmez.
pub fn init_pci(io_base: u16) -> bool {
    if io_base < 0x100 {
        // Geçersiz BAR: jumper taramasına düş.
        let Some(base) = (unsafe { probe_bases() }) else {
            return false;
        };
        return init(base);
    }
    init(io_base)
}

/// ISA PnP kartını bulur ve başlatır (PnP yanıtı yoksa jumper taraması).
pub fn init_isa_pnp() -> bool {
    let mut mac = [0u8; 6];
    let mut io_base = None;
    if let Some(csn) = net::pnp_isolate() {
        let (base, card_mac) = net::pnp_read_card(csn);
        io_base = base.filter(|b| *b != 0);
        mac = card_mac;
    }
    let io_base = io_base.or_else(|| unsafe { probe_bases() });
    let Some(io_base) = io_base else {
        return false;
    };
    let ok = init(io_base);
    if ok {
        unsafe {
            if mac.iter().any(|&b| b != 0 && b != 0xFF) {
                crate::drivers::e1000::MAC_ADDR = mac;
            }
        }
    }
    ok
}

/// PnP çözümü yoksa yaygın jumper I/O tabanlarını tek tek dener.
unsafe fn probe_bases() -> Option<u16> {
    for &base in PROBE_BASES.iter() {
        // DP8390 Page 0 CR yazılabilir mi? Boş bus 0xFF döner.
        let saved = io_in8(base);
        if saved == 0xFF {
            continue;
        }
        IOBASE = base;
        card_out(REG_CR, CR_STOP | CR_PAGE0);
        card_out(REG_ISR, 0xFF);
        let mac = {
            select_page(CR_PAGE1);
            let mut m = [0u8; 6];
            for (i, b) in m.iter_mut().enumerate() {
                *b = card_in(REG_PAR0 + i as u16);
            }
            select_page(CR_PAGE0);
            m
        };
        if mac.iter().any(|&b| b != 0 && b != 0xFF) {
            return Some(base);
        }
    }
    None
}

/// Kartın IRQ/DMA bilgisini kaydeder (raporlama için).
pub fn set_resources(irq: u8, dma: u8) {
    unsafe {
        CARD_IRQ = irq;
        CARD_DMA = dma;
    }
}

pub fn resources() -> (u8, u8) {
    unsafe { (CARD_IRQ, CARD_DMA) }
}

pub fn io_base() -> u16 {
    unsafe { IOBASE }
}
