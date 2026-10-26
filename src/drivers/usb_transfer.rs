//! USB UHCI ve OHCI (USB 1.1) ile EHCI (USB 2.0) Transfer Katmanı.
//!
//! Bu modül, donanım denetleyicileri için Transfer Descriptor (TD) ve
//! Queue Head (QH) veri yapılarını, token oluşturucuları ve kontrol/bulk transfer
//! paketleme işlevlerini sağlar.

// ---------------------------------------------------------------------------
// UHCI Transfer Katmanı (Queue Head / Transfer Descriptor)
// ---------------------------------------------------------------------------

pub const UHCI_PID_SETUP: u8 = 0x2D;
pub const UHCI_PID_IN: u8 = 0x69;
pub const UHCI_PID_OUT: u8 = 0xE1;

pub const UHCI_TD_STATUS_ACTIVE: u32 = 1 << 23;
pub const UHCI_TD_STATUS_IOC: u32 = 1 << 24;
pub const UHCI_TD_STATUS_IOS: u32 = 1 << 25;
pub const UHCI_TD_STATUS_LS: u32 = 1 << 26;
pub const UHCI_TD_STATUS_C_ERR_MASK: u32 = 3 << 27;
pub const UHCI_TD_STATUS_SPD: u32 = 1 << 29;
pub const UHCI_TD_STATUS_STALL: u32 = 1 << 22;
pub const UHCI_TD_STATUS_DBUFERR: u32 = 1 << 21;
pub const UHCI_TD_STATUS_BABBLE: u32 = 1 << 20;
pub const UHCI_TD_STATUS_NAK: u32 = 1 << 19;
pub const UHCI_TD_STATUS_CRCTIMEOUT: u32 = 1 << 18;
pub const UHCI_TD_STATUS_BITSTUFF: u32 = 1 << 17;

pub fn uhci_make_token(pid: u8, dev_addr: u8, ep: u8, data_toggle: bool, max_len: u16) -> u32 {
    let len_field = if max_len == 0 {
        0x7FF
    } else {
        (max_len - 1) as u32
    };
    (pid as u32)
        | ((dev_addr as u32 & 0x7F) << 8)
        | ((ep as u32 & 0x0F) << 15)
        | (if data_toggle { 1 << 19 } else { 0 })
        | ((len_field & 0x7FF) << 21)
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UhciTd {
    pub link: u32,
    pub status: u32,
    pub token: u32,
    pub buffer: u32,
    pub _reserved: [u32; 4],
}

impl UhciTd {
    pub const fn empty() -> Self {
        Self {
            link: 1, // Terminate = 1
            status: 0,
            token: 0,
            buffer: 0,
            _reserved: [0; 4],
        }
    }

    pub fn new(link: u32, status: u32, token: u32, buffer: u32) -> Self {
        Self {
            link,
            status,
            token,
            buffer,
            _reserved: [0; 4],
        }
    }

    pub fn is_active(&self) -> bool {
        (self.status & UHCI_TD_STATUS_ACTIVE) != 0
    }

    pub fn is_success(&self) -> bool {
        !self.is_active() && (self.status & 0x007E_0000) == 0
    }

    pub fn actual_len(&self) -> usize {
        let field = self.status & 0x7FF;
        if field == 0x7FF {
            0
        } else {
            (field + 1) as usize
        }
    }

    pub fn error_flags(&self) -> u32 {
        (self.status >> 17) & 0x3F
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UhciQh {
    pub head_link: u32,
    pub element_link: u32,
    pub _reserved: [u32; 2],
}

impl UhciQh {
    pub const fn empty() -> Self {
        Self {
            head_link: 1,    // Terminate = 1
            element_link: 1, // Terminate = 1
            _reserved: [0; 2],
        }
    }

    pub fn new(head_link: u32, element_link: u32) -> Self {
        Self {
            head_link,
            element_link,
            _reserved: [0; 2],
        }
    }

    pub fn set_element(&mut self, elem_phys: u32, is_qh: bool) {
        self.element_link = (elem_phys & !0x0F) | if is_qh { 0x02 } else { 0x00 };
    }

    pub fn terminate_element(&mut self) {
        self.element_link = 1;
    }
}

/// UHCI için kontrol transfer kuyruğu oluşturur.
pub fn uhci_build_control_transfer(
    tds: &mut [UhciTd; 4],
    qh: &mut UhciQh,
    td_base_phys: u32,
    dev_addr: u8,
    is_low_speed: bool,
    setup_phys: u32,
    data_phys: u32,
    data_len: u16,
    is_in: bool,
) -> usize {
    let ls_flag = if is_low_speed { UHCI_TD_STATUS_LS } else { 0 };
    let base_status = UHCI_TD_STATUS_ACTIVE | UHCI_TD_STATUS_C_ERR_MASK | ls_flag;
    let td_stride = core::mem::size_of::<UhciTd>() as u32;

    // TD 0: Setup Paketi
    let setup_token = uhci_make_token(UHCI_PID_SETUP, dev_addr, 0, false, 8);
    let next_td1 = td_base_phys + td_stride;
    tds[0] = UhciTd::new(next_td1, base_status, setup_token, setup_phys);

    if data_len > 0 {
        // TD 1: Veri Aşaması (IN veya OUT)
        let data_pid = if is_in { UHCI_PID_IN } else { UHCI_PID_OUT };
        let data_token = uhci_make_token(data_pid, dev_addr, 0, true, data_len);
        let next_td2 = td_base_phys + 2 * td_stride;
        tds[1] = UhciTd::new(next_td2, base_status, data_token, data_phys);

        // TD 2: Durum Aşaması (Ters yönde sıfır uzunluklu paket)
        let status_pid = if is_in { UHCI_PID_OUT } else { UHCI_PID_IN };
        let status_token = uhci_make_token(status_pid, dev_addr, 0, true, 0);
        tds[2] = UhciTd::new(1, base_status | UHCI_TD_STATUS_IOC, status_token, 0);

        qh.set_element(td_base_phys, false);
        3
    } else {
        // TD 1: Durum Aşaması (IN, sıfır bayt)
        let status_token = uhci_make_token(UHCI_PID_IN, dev_addr, 0, true, 0);
        tds[1] = UhciTd::new(1, base_status | UHCI_TD_STATUS_IOC, status_token, 0);

        qh.set_element(td_base_phys, false);
        2
    }
}

// ---------------------------------------------------------------------------
// EHCI Transfer Katmanı (Queue Head / Queue Transfer Descriptor)
// ---------------------------------------------------------------------------

#[repr(C, align(32))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EhciQtd {
    pub next_qtd: u32,
    pub alt_next_qtd: u32,
    pub token: u32,
    pub buffer_pointers: [u32; 5],
}

impl EhciQtd {
    pub const fn empty() -> Self {
        Self {
            next_qtd: 1,     // Terminate = 1
            alt_next_qtd: 1, // Terminate = 1
            token: 0,
            buffer_pointers: [0; 5],
        }
    }
}

#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EhciQh {
    pub horizontal_link: u32,
    pub ep_characteristics: u32,
    pub ep_capabilities: u32,
    pub current_qtd: u32,
    pub overlay_next_qtd: u32,
    pub overlay_alt_next_qtd: u32,
    pub overlay_token: u32,
    pub overlay_buffers: [u32; 5],
}

impl EhciQh {
    pub const fn empty() -> Self {
        Self {
            horizontal_link: 1,
            ep_characteristics: 0,
            ep_capabilities: 0,
            current_qtd: 0,
            overlay_next_qtd: 1,
            overlay_alt_next_qtd: 1,
            overlay_token: 0,
            overlay_buffers: [0; 5],
        }
    }
}

/// EHCI asenkron (kontrol/bulk) zamanlayıcısının DMA havuzu.
#[repr(C, align(4096))]
pub struct EhciDmaPool {
    pub qh_async: EhciQh,
    pub td: [EhciQtd; 4],
}

impl EhciDmaPool {
    pub const fn empty() -> Self {
        Self {
            qh_async: EhciQh::empty(),
            td: [EhciQtd::empty(); 4],
        }
    }
}

/// UHCI frame listesi (1024 giris) ve kontrol kuyrugu DMA havuzu.
#[repr(C, align(4096))]
pub struct UhciDmaPool {
    pub frame_list: [u32; 1024],
    pub qh: UhciQh,
    pub td: [UhciTd; 4],
}

impl UhciDmaPool {
    pub const fn empty() -> Self {
        Self {
            frame_list: [1; 1024],
            qh: UhciQh::empty(),
            td: [UhciTd::empty(); 4],
        }
    }
}

// ---------------------------------------------------------------------------
// OHCI Transfer Katmanı (Endpoint Descriptor / General Transfer Descriptor)
// ---------------------------------------------------------------------------

/// USB 1.1/2.0 transfer zamanlayıcılarının statik DMA havuzları.
pub static mut EHCI_DMA: EhciDmaPool = EhciDmaPool::empty();
pub static mut UHCI_DMA: UhciDmaPool = UhciDmaPool::empty();

pub const OHCI_TD_DIR_SETUP: u32 = 0;
pub const OHCI_TD_DIR_OUT: u32 = 1;
pub const OHCI_TD_DIR_IN: u32 = 2;

pub const OHCI_CC_NO_ERROR: u32 = 0;
pub const OHCI_CC_CRC: u32 = 1;
pub const OHCI_CC_BIT_STUFFING: u32 = 2;
pub const OHCI_CC_DATA_OVERRUN: u32 = 8;
pub const OHCI_CC_DATA_UNDERRUN: u32 = 9;
pub const OHCI_CC_NOT_ACCESSED: u32 = 15;

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OhciEd {
    pub flags: u32, // Bit 0..6: FA, 7..10: EN, 11..12: d, 13: s, 14: k, 15: f, 16..26: MPS
    pub tail_p: u32, // TD Kuyruk Sonu Fiziksel Adresi
    pub head_p: u32, // TD Kuyruk Başı Fiziksel Adresi + Durum (bit 0: Halted, bit 1: Carry)
    pub next_ed: u32, // Sonraki ED Fiziksel Adresi
}

impl OhciEd {
    pub const fn empty() -> Self {
        Self {
            flags: 0,
            tail_p: 0,
            head_p: 0,
            next_ed: 0,
        }
    }

    pub fn new(dev_addr: u8, ep: u8, dir: u8, is_low_speed: bool, mps: u16) -> Self {
        let flags = (dev_addr as u32 & 0x7F)
            | ((ep as u32 & 0x0F) << 7)
            | ((dir as u32 & 0x03) << 11)
            | (if is_low_speed { 1 << 13 } else { 0 })
            | ((mps as u32 & 0x7FF) << 16);
        Self {
            flags,
            tail_p: 0,
            head_p: 0,
            next_ed: 0,
        }
    }

    pub fn set_queue(&mut self, head_td_phys: u32, tail_td_phys: u32) {
        self.head_p = head_td_phys & !0x0F;
        self.tail_p = tail_td_phys & !0x0F;
    }

    pub fn is_halted(&self) -> bool {
        (self.head_p & 1) != 0
    }

    pub fn is_empty(&self) -> bool {
        (self.head_p & !0x0F) == (self.tail_p & !0x0F)
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OhciTd {
    pub flags: u32, // Bit 18..19: R/DP (0=SETUP, 1=OUT, 2=IN), 20: DI, 21..23: T, 24..27: EC, 28..31: CC
    pub cbp: u32,   // Current Buffer Pointer
    pub next_td: u32, // Next TD
    pub be: u32,    // Buffer End
}

impl OhciTd {
    pub const fn empty() -> Self {
        Self {
            flags: 0,
            cbp: 0,
            next_td: 0,
            be: 0,
        }
    }

    pub fn new(dir: u32, toggle: u32, buf_start: u32, buf_len: u32, next_td_phys: u32) -> Self {
        let be = if buf_len == 0 {
            0
        } else {
            buf_start + buf_len - 1
        };
        // Buffer Rounding (bit 18 = 1), Direction (bits 19..20), Toggle (bits 24..25), CC=NotAccessed (bits 28..31 = 15)
        let flags = (1 << 18)
            | ((dir & 0x03) << 19)
            | ((toggle & 0x03) << 24)
            | (OHCI_CC_NOT_ACCESSED << 28);
        Self {
            flags,
            cbp: buf_start,
            next_td: next_td_phys,
            be,
        }
    }

    pub fn condition_code(&self) -> u32 {
        (self.flags >> 28) & 0x0F
    }

    pub fn is_done(&self) -> bool {
        self.condition_code() != OHCI_CC_NOT_ACCESSED
    }

    pub fn is_success(&self) -> bool {
        self.condition_code() == OHCI_CC_NO_ERROR
    }
}

/// OHCI Host Controller Communications Area (HCCA - 256 bayt hizalı)
#[repr(C, align(256))]
pub struct OhciHcca {
    pub hcca_interrupt_table: [u32; 32], // 32 periyodik kesme listesi başlığı
    pub hcca_frame_number: u16,          // Güncel frame numarası
    pub hcca_pad1: u16,
    pub hcca_done_head: u32, // Tamamlanan TD listesi
    pub reserved: [u8; 116],
}

impl OhciHcca {
    pub const fn empty() -> Self {
        Self {
            hcca_interrupt_table: [0; 32],
            hcca_frame_number: 0,
            hcca_pad1: 0,
            hcca_done_head: 0,
            reserved: [0; 116],
        }
    }
}

/// OHCI için kontrol transfer kuyruğu oluşturur.
pub fn ohci_build_control_transfer(
    tds: &mut [OhciTd; 4],
    ed: &mut OhciEd,
    td_base_phys: u32,
    setup_phys: u32,
    data_phys: u32,
    data_len: u16,
    is_in: bool,
) -> usize {
    let td_stride = core::mem::size_of::<OhciTd>() as u32;

    // TD 0: Setup Aşaması (toggle = 2 -> data0)
    let next_td1 = td_base_phys + td_stride;
    tds[0] = OhciTd::new(OHCI_TD_DIR_SETUP, 2, setup_phys, 8, next_td1);

    let (active_tds, tail_idx) = if data_len > 0 {
        // TD 1: Data Aşaması (toggle = 3 -> data1)
        let next_td2 = td_base_phys + 2 * td_stride;
        let data_dir = if is_in {
            OHCI_TD_DIR_IN
        } else {
            OHCI_TD_DIR_OUT
        };
        tds[1] = OhciTd::new(data_dir, 3, data_phys, data_len as u32, next_td2);

        // TD 2: Status Aşaması (Ters yön, toggle = 3 -> data1, 0 bayt)
        let next_td3 = td_base_phys + 3 * td_stride;
        let status_dir = if is_in {
            OHCI_TD_DIR_OUT
        } else {
            OHCI_TD_DIR_IN
        };
        tds[2] = OhciTd::new(status_dir, 3, 0, 0, next_td3);

        // TD 3: Tail (boş kuyruk sonu belirteci)
        tds[3] = OhciTd::empty();
        (3, 3)
    } else {
        // TD 1: Status Aşaması (IN, toggle = 3 -> data1, 0 bayt)
        let next_td2 = td_base_phys + 2 * td_stride;
        tds[1] = OhciTd::new(OHCI_TD_DIR_IN, 3, 0, 0, next_td2);

        // TD 2: Tail
        tds[2] = OhciTd::empty();
        (2, 2)
    };

    let tail_phys = td_base_phys + (tail_idx as u32) * td_stride;
    ed.set_queue(td_base_phys, tail_phys);
    active_tds
}
