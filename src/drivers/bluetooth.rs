#![allow(dead_code, unused_variables, static_mut_refs)]

//! Maturity: KISMI / WIP — Gerçek USB/PCI Bluetooth tespiti yok,//! sadece HCI paketleme/ayrıştırma ve synthetic yanıtlar mevcut.//! Bkz. docs/driver-maturity.md
//!
//! Bluetooth Driver - Bluetooth HCI (Host Controller Interface) Protokol Sürücüsü
//!
//! Bu sürücü:
//! - Bluetooth 1.1 - 5.4 HCI spesifikasyonuna uygun paketleme (framing) ve ayrıştırma (parsing) sağlar.
//! - HCI Komutları (HCI_COMMAND_PKT = 0x01): OGF (Opcode Group Field) ve OCF (Opcode Command Field) kodlaması.
//! - HCI Olayları (HCI_EVENT_PKT = 0x04): Inquiry Result, Connection Complete, Command Complete, Disconnect vb.
//! - HCI ACL Veri Paketleri (HCI_ACLDATA_PKT = 0x02): Bağlantı tanıtıcısı (handle), bayraklar ve veri aktarımı.
//! - Cihaz keşfi (Inquiry / Scan) tablosu ve bağlantı durumu yönetimi.
//! - USB Bluetooth Dongle (Class 0xE0, Subclass 0x01, Protocol 0x01) taşıma katmanı entegrasyonu.

use crate::drivers::port_io::*;

// ---------------------------------------------------------------------------
// Bluetooth USB Device Sınıf Kodları
// ---------------------------------------------------------------------------

pub const BLUETOOTH_WIRELESS_CLASS: u8 = 0xE0;
pub const BLUETOOTH_WIRELESS_SUBCLASS: u8 = 0x01;
pub const BLUETOOTH_WIRELESS_PROTOCOL: u8 = 0x01;

// ---------------------------------------------------------------------------
// HCI Paket Göstergeleri (Packet Type Indicators)
// ---------------------------------------------------------------------------

pub const HCI_COMMAND_PKT: u8 = 0x01;
pub const HCI_ACLDATA_PKT: u8 = 0x02;
pub const HCI_SCODATA_PKT: u8 = 0x03;
pub const HCI_EVENT_PKT: u8 = 0x04;

// ---------------------------------------------------------------------------
// HCI OGF (Opcode Group Field) Tanımları
// ---------------------------------------------------------------------------

pub const OGF_LINK_CONTROL: u8 = 0x01;
pub const OGF_LINK_POLICY: u8 = 0x02;
pub const OGF_HOST_CONTROL: u8 = 0x03;
pub const OGF_INFO_PARAM: u8 = 0x04;
pub const OGF_STATUS_PARAM: u8 = 0x05;
pub const OGF_TESTING: u8 = 0x06;
pub const OGF_LE_CONTROL: u8 = 0x08;

// ---------------------------------------------------------------------------
// HCI OCF (Opcode Command Field) Tanımları
// ---------------------------------------------------------------------------

// Link Control (0x01)
pub const OCF_INQUIRY: u16 = 0x0001;
pub const OCF_INQUIRY_CANCEL: u16 = 0x0002;
pub const OCF_PERIODIC_INQUIRY_MODE: u16 = 0x0003;
pub const OCF_EXIT_PERIODIC_INQUIRY_MODE: u16 = 0x0004;
pub const OCF_CREATE_CONN: u16 = 0x0005;
pub const OCF_DISCONNECT: u16 = 0x0006;
pub const OCF_REMOTE_NAME_REQ: u16 = 0x0019;
pub const OCF_REMOTE_NAME_REQ_CANCEL: u16 = 0x001A;

// Host Controller & Baseband (0x03)
pub const OCF_SET_EVENT_MASK: u16 = 0x0001;
pub const OCF_RESET: u16 = 0x0003;
pub const OCF_SET_EVENT_FILTER: u16 = 0x0005;
pub const OCF_WRITE_LOCAL_NAME: u16 = 0x0013;
pub const OCF_READ_LOCAL_NAME: u16 = 0x0014;
pub const OCF_WRITE_SCAN_ENABLE: u16 = 0x001A;
pub const OCF_WRITE_CLASS_OF_DEVICE: u16 = 0x0024;

// Informational Parameters (0x04)
pub const OCF_READ_LOCAL_VERSION: u16 = 0x0001;
pub const OCF_READ_LOCAL_COMMANDS: u16 = 0x0002;
pub const OCF_READ_LOCAL_FEATURES: u16 = 0x0003;
pub const OCF_READ_BUFFER_SIZE: u16 = 0x0005;
pub const OCF_READ_BD_ADDR: u16 = 0x0009;

// Status Parameters (0x05)
pub const OCF_READ_RSSI: u16 = 0x0005;

// LE Controller (0x08)
pub const OCF_LE_SET_EVENT_MASK: u16 = 0x0001;
pub const OCF_LE_READ_BUFFER_SIZE: u16 = 0x0002;
pub const OCF_LE_SET_SCAN_PARAM: u16 = 0x000B;
pub const OCF_LE_SET_SCAN_ENABLE: u16 = 0x000C;

// Hazır Opcode Oluşturucu
pub const fn make_opcode(ogf: u8, ocf: u16) -> u16 {
    ((ogf as u16) << 10) | (ocf & 0x03FF)
}

pub const fn opcode_ogf(opcode: u16) -> u8 {
    (opcode >> 10) as u8
}

pub const fn opcode_ocf(opcode: u16) -> u16 {
    opcode & 0x03FF
}

// Hazır Komut Opcodları
pub const HCI_OP_INQUIRY: u16 = make_opcode(OGF_LINK_CONTROL, OCF_INQUIRY);
pub const HCI_OP_INQUIRY_CANCEL: u16 = make_opcode(OGF_LINK_CONTROL, OCF_INQUIRY_CANCEL);
pub const HCI_OP_CREATE_CONN: u16 = make_opcode(OGF_LINK_CONTROL, OCF_CREATE_CONN);
pub const HCI_OP_DISCONNECT: u16 = make_opcode(OGF_LINK_CONTROL, OCF_DISCONNECT);
pub const HCI_OP_REMOTE_NAME_REQ: u16 = make_opcode(OGF_LINK_CONTROL, OCF_REMOTE_NAME_REQ);

pub const HCI_OP_SET_EVENT_MASK: u16 = make_opcode(OGF_HOST_CONTROL, OCF_SET_EVENT_MASK);
pub const HCI_OP_RESET: u16 = make_opcode(OGF_HOST_CONTROL, OCF_RESET);
pub const HCI_OP_WRITE_SCAN_ENABLE: u16 = make_opcode(OGF_HOST_CONTROL, OCF_WRITE_SCAN_ENABLE);
pub const HCI_OP_WRITE_LOCAL_NAME: u16 = make_opcode(OGF_HOST_CONTROL, OCF_WRITE_LOCAL_NAME);

pub const HCI_OP_READ_LOCAL_VERSION: u16 = make_opcode(OGF_INFO_PARAM, OCF_READ_LOCAL_VERSION);
pub const HCI_OP_READ_LOCAL_FEATURES: u16 = make_opcode(OGF_INFO_PARAM, OCF_READ_LOCAL_FEATURES);
pub const HCI_OP_READ_BUFFER_SIZE: u16 = make_opcode(OGF_INFO_PARAM, OCF_READ_BUFFER_SIZE);
pub const HCI_OP_READ_BD_ADDR: u16 = make_opcode(OGF_INFO_PARAM, OCF_READ_BD_ADDR);

// ---------------------------------------------------------------------------
// HCI Event Kodları
// ---------------------------------------------------------------------------

pub const HCI_EV_INQUIRY_COMPLETE: u8 = 0x01;
pub const HCI_EV_INQUIRY_RESULT: u8 = 0x02;
pub const HCI_EV_CONN_COMPLETE: u8 = 0x03;
pub const HCI_EV_CONN_REQUEST: u8 = 0x04;
pub const HCI_EV_DISCONN_COMPLETE: u8 = 0x05;
pub const HCI_EV_AUTH_COMPLETE: u8 = 0x06;
pub const HCI_EV_REMOTE_NAME_REQ_COMPLETE: u8 = 0x07;
pub const HCI_EV_CMD_COMPLETE: u8 = 0x0E;
pub const HCI_EV_CMD_STATUS: u8 = 0x0F;
pub const HCI_EV_HARDWARE_ERROR: u8 = 0x10;
pub const HCI_EV_NUM_COMPL_PKTS: u8 = 0x13;
pub const HCI_EV_INQUIRY_RESULT_WITH_RSSI: u8 = 0x22;
pub const HCI_EV_EXTENDED_INQUIRY_RESULT: u8 = 0x2F;
pub const HCI_EV_LE_META: u8 = 0x3E;

// ---------------------------------------------------------------------------
// Bluetooth Sürücü Durumu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BluetoothState {
    NotDetected,
    Detected,
    Initializing,
    Ready,
    Inquiring,
    Connecting,
    Error,
}

// Keşfedilen Aygıt Kaydı
#[derive(Clone, Copy, Debug)]
pub struct DiscoveredDevice {
    pub mac: [u8; 6],
    pub class_of_device: [u8; 3],
    pub rssi: i8,
    pub name: [u8; 32],
    pub name_len: usize,
    pub connected: bool,
    pub handle: u16,
}

impl Default for DiscoveredDevice {
    fn default() -> Self {
        Self {
            mac: [0; 6],
            class_of_device: [0; 3],
            rssi: 0,
            name: [0; 32],
            name_len: 0,
            connected: false,
            handle: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global Sürücü Durumu ve Arabellekler
// ---------------------------------------------------------------------------

const MAX_DISCOVERED: usize = 16;
const RX_EVENT_BUF_SIZE: usize = 512;
const TX_CMD_BUF_SIZE: usize = 256;
const ACL_BUF_SIZE: usize = 1024;

static mut STATE: BluetoothState = BluetoothState::NotDetected;
static mut DEVICE_FOUND: bool = false;
static mut MAC_ADDR: [u8; 6] = [0; 6];
static mut HCI_VERSION: u8 = 0;
static mut HCI_REVISION: u16 = 0;
static mut LMP_VERSION: u8 = 0;
static mut LMP_SUBVERSION: u16 = 0;
static mut MANUFACTURER_NAME: u16 = 0;

static mut DISCOVERED_DEVICES: [DiscoveredDevice; MAX_DISCOVERED] = [DiscoveredDevice {
    mac: [0; 6],
    class_of_device: [0; 3],
    rssi: 0,
    name: [0; 32],
    name_len: 0,
    connected: false,
    handle: 0,
}; MAX_DISCOVERED];
static mut DISCOVERED_COUNT: usize = 0;

// Giden ve gelen paket tamponları
static mut TX_BUFFER: [u8; TX_CMD_BUF_SIZE] = [0; TX_CMD_BUF_SIZE];
static mut TX_LEN: usize = 0;

static mut RX_EVENT_BUFFER: [u8; RX_EVENT_BUF_SIZE] = [0; RX_EVENT_BUF_SIZE];
static mut RX_EVENT_LEN: usize = 0;

static mut ACL_TX_BUFFER: [u8; ACL_BUF_SIZE] = [0; ACL_BUF_SIZE];
static mut ACL_TX_LEN: usize = 0;

static mut ACL_RX_BUFFER: [u8; ACL_BUF_SIZE] = [0; ACL_BUF_SIZE];
static mut ACL_RX_LEN: usize = 0;

// ---------------------------------------------------------------------------
// HCI Paket Biçimlendirme (Formatting) ve Ayrıştırma (Parsing)
// ---------------------------------------------------------------------------

/// HCI Command Paketi oluşturur:
/// [0x01 (Indicator), Opcode Low, Opcode High, Parameter Length, Parameters...]
pub fn format_command_packet(opcode: u16, params: &[u8], out: &mut [u8]) -> usize {
    if out.len() < 4 + params.len() {
        return 0;
    }

    out[0] = HCI_COMMAND_PKT;
    out[1] = (opcode & 0xFF) as u8;
    out[2] = ((opcode >> 8) & 0xFF) as u8;
    out[3] = params.len().min(255) as u8;

    let copy_len = params.len().min(255);
    out[4..4 + copy_len].copy_from_slice(&params[..copy_len]);

    4 + copy_len
}

/// HCI ACL Data Paketi oluşturur:
/// [0x02 (Indicator), Handle & Flags Low, Handle & Flags High, Data Length Low, Data Length High, Data...]
pub fn format_acl_packet(
    handle: u16,
    pb_flag: u8,
    bc_flag: u8,
    data: &[u8],
    out: &mut [u8],
) -> usize {
    if out.len() < 5 + data.len() {
        return 0;
    }

    let handle_and_flags =
        (handle & 0x0FFF) | (((pb_flag as u16) & 0x03) << 12) | (((bc_flag as u16) & 0x03) << 14);
    let len = data.len().min(0xFFFF) as u16;

    out[0] = HCI_ACLDATA_PKT;
    out[1] = (handle_and_flags & 0xFF) as u8;
    out[2] = ((handle_and_flags >> 8) & 0xFF) as u8;
    out[3] = (len & 0xFF) as u8;
    out[4] = ((len >> 8) & 0xFF) as u8;

    let copy_len = len as usize;
    out[5..5 + copy_len].copy_from_slice(&data[..copy_len]);

    5 + copy_len
}

// ---------------------------------------------------------------------------
// Cihaz Algılama ve Başlatma
// ---------------------------------------------------------------------------

pub fn detect() -> bool {
    unsafe {
        if DEVICE_FOUND {
            return true;
        }

        // 1. PCI üzerinde Bluetooth denetleyicisi (Class 0x0D, Subclass 0x11)
        // veya USB denetleyicisi (Class 0x0C, Subclass 0x03) ara
        for bus in 0..=8u8 {
            for dev in 0..=31u8 {
                for func in 0..=7u8 {
                    let id = pci_read32(bus, dev, func, 0x00);
                    if id == 0xFFFF_FFFF || id == 0 {
                        continue;
                    }

                    let class_rev = pci_read32(bus, dev, func, 0x08);
                    let base_class = (class_rev >> 24) as u8;
                    let sub_class = ((class_rev >> 16) & 0xFF) as u8;

                    // Doğrudan Bluetooth PCI aygıtı (Class 0x0D = Wireless, Subclass 0x11 = Bluetooth)
                    if base_class == 0x0D && sub_class == 0x11 {
                        DEVICE_FOUND = true;
                        STATE = BluetoothState::Detected;
                        return true;
                    }

                    // Intel / Broadcom / Realtek bilinen PCI Bluetooth/WiFi kombo kartları
                    let vid = (id & 0xFFFF) as u16;
                    let did = ((id >> 16) & 0xFFFF) as u16;
                    if (vid == 0x8086 || vid == 0x14E4 || vid == 0x10EC || vid == 0x168C)
                        && (base_class == 0x0D || (base_class == 0x02 && sub_class == 0x80))
                    {
                        DEVICE_FOUND = true;
                        STATE = BluetoothState::Detected;
                        return true;
                    }

                    // USB Denetleyicisi (xHCI/EHCI) mevcutsa USB Bluetooth dongle desteği sunulabilir
                    if base_class == 0x0C && sub_class == 0x03 {
                        DEVICE_FOUND = true;
                        STATE = BluetoothState::Detected;
                    }
                }
            }
        }

        DEVICE_FOUND
    }
}

pub fn init() -> bool {
    unsafe {
        if !DEVICE_FOUND && !detect() {
            return false;
        }

        STATE = BluetoothState::Initializing;

        // 1. HCI Reset komutu gönder
        if !send_hci_command(HCI_OP_RESET, &[]) {
            STATE = BluetoothState::Error;
            return false;
        }

        // 2. HCI Read BD_ADDR komutu gönder (MAC adresi)
        send_hci_command(HCI_OP_READ_BD_ADDR, &[]);

        // 3. HCI Read Local Version Information komutu gönder
        send_hci_command(HCI_OP_READ_LOCAL_VERSION, &[]);

        // 4. HCI Read Buffer Size komutu gönder
        send_hci_command(HCI_OP_READ_BUFFER_SIZE, &[]);

        // 5. Standart olay maskesini ayarla (Tüm olayları etkinleştir)
        let event_mask: [u8; 8] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x1F, 0x00, 0x00];
        send_hci_command(HCI_OP_SET_EVENT_MASK, &event_mask);

        // 6. Tarama (Inquiry & Page scan) etkinleştir
        let scan_enable: [u8; 1] = [0x03]; // Hem Inquiry hem Page taraması açık
        send_hci_command(HCI_OP_WRITE_SCAN_ENABLE, &scan_enable);

        // Varsayılan MAC adresi (Donanım yanıt verene kadar veya emülasyonda)
        if MAC_ADDR == [0; 6] {
            MAC_ADDR = [0x00, 0x1A, 0x7D, 0xDA, 0x71, 0x13];
        }

        STATE = BluetoothState::Ready;
        true
    }
}

// ---------------------------------------------------------------------------
// HCI Komut ve Veri İletimi
// ---------------------------------------------------------------------------

pub fn send_hci_command(opcode: u16, params: &[u8]) -> bool {
    unsafe {
        if STATE == BluetoothState::NotDetected || STATE == BluetoothState::Error {
            return false;
        }

        let written = format_command_packet(opcode, params, &mut TX_BUFFER);
        if written == 0 {
            return false;
        }
        TX_LEN = written;

        // Olay yanıtı üret ve durum makinesini işlet
        handle_synthetic_response(opcode, params);

        true
    }
}

pub fn receive_hci_event() -> Option<&'static [u8]> {
    unsafe {
        if STATE == BluetoothState::NotDetected {
            return None;
        }

        if RX_EVENT_LEN > 0 {
            let slice = &RX_EVENT_BUFFER[..RX_EVENT_LEN];
            Some(core::mem::transmute(slice))
        } else {
            None
        }
    }
}

pub fn send_acl_data(handle: u16, data: &[u8]) -> bool {
    unsafe {
        if STATE != BluetoothState::Ready
            && STATE != BluetoothState::Inquiring
            && STATE != BluetoothState::Connecting
        {
            return false;
        }

        let written = format_acl_packet(handle, 0x02, 0x00, data, &mut ACL_TX_BUFFER);
        if written == 0 {
            return false;
        }
        ACL_TX_LEN = written;

        true
    }
}

pub fn receive_acl_data() -> Option<&'static [u8]> {
    unsafe {
        if STATE == BluetoothState::NotDetected {
            return None;
        }

        if ACL_RX_LEN > 0 {
            let slice = &ACL_RX_BUFFER[..ACL_RX_LEN];
            Some(core::mem::transmute(slice))
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// HCI Olay Ayrıştırıcısı ve Durum Makinesi
// ---------------------------------------------------------------------------

pub fn process_hci_event(packet: &[u8]) -> bool {
    unsafe {
        if packet.is_empty() {
            return false;
        }

        // Paket göstergesi (0x04) varsa bir sonraki bayta geç
        let (event_code, param_len, params) = if packet[0] == HCI_EVENT_PKT && packet.len() >= 3 {
            (packet[1], packet[2] as usize, &packet[3..])
        } else if packet.len() >= 2 {
            (packet[0], packet[1] as usize, &packet[2..])
        } else {
            return false;
        };

        // Gelen olayı RX tamponuna kaydet
        let copy_len = packet.len().min(RX_EVENT_BUF_SIZE);
        RX_EVENT_BUFFER[..copy_len].copy_from_slice(&packet[..copy_len]);
        RX_EVENT_LEN = copy_len;

        match event_code {
            HCI_EV_CMD_COMPLETE => {
                // Command Complete Event:
                // [0] Num_HCI_Command_Packets, [1..2] Opcode, [3] Status, [4..] Return Parameters
                if params.len() >= 4 {
                    let opcode = (params[1] as u16) | ((params[2] as u16) << 8);
                    let status = params[3];
                    if status == 0 {
                        let ret_params = &params[4..];
                        handle_cmd_complete_return(opcode, ret_params);
                    }
                }
            }
            HCI_EV_INQUIRY_RESULT => {
                // Inquiry Result Event:
                // [0] Num_Responses
                // Her yanıt: BD_ADDR (6), Page_Scan_Repetition_Mode (1), Reserved (2), Class_of_Device (3), Clock_Offset (2)
                if !params.is_empty() {
                    let num_responses = params[0] as usize;
                    let mut offset = 1;
                    for _ in 0..num_responses {
                        if offset + 14 <= params.len() {
                            let mut mac = [0u8; 6];
                            mac.copy_from_slice(&params[offset..offset + 6]);
                            let mut class = [0u8; 3];
                            class.copy_from_slice(&params[offset + 9..offset + 12]);
                            add_or_update_device(mac, class, -60, &[]);
                            offset += 14;
                        }
                    }
                }
            }
            HCI_EV_INQUIRY_RESULT_WITH_RSSI => {
                if !params.is_empty() {
                    let num_responses = params[0] as usize;
                    let mut offset = 1;
                    for _ in 0..num_responses {
                        if offset + 14 <= params.len() {
                            let mut mac = [0u8; 6];
                            mac.copy_from_slice(&params[offset..offset + 6]);
                            let mut class = [0u8; 3];
                            class.copy_from_slice(&params[offset + 9..offset + 12]);
                            let rssi = params[offset + 13] as i8;
                            add_or_update_device(mac, class, rssi, &[]);
                            offset += 14;
                        }
                    }
                }
            }
            HCI_EV_INQUIRY_COMPLETE => {
                if STATE == BluetoothState::Inquiring {
                    STATE = BluetoothState::Ready;
                }
            }
            HCI_EV_CONN_COMPLETE => {
                // [0] Status, [1..2] Connection_Handle, [3..8] BD_ADDR, [9] Link_Type, [10] Encryption_Enabled
                if params.len() >= 11 {
                    let status = params[0];
                    if status == 0 {
                        let handle = (params[1] as u16) | ((params[2] as u16) << 8);
                        let mut mac = [0u8; 6];
                        mac.copy_from_slice(&params[3..9]);

                        for dev in DISCOVERED_DEVICES.iter_mut().take(DISCOVERED_COUNT) {
                            if dev.mac == mac {
                                dev.connected = true;
                                dev.handle = handle;
                            }
                        }
                        STATE = BluetoothState::Ready;
                    }
                }
            }
            HCI_EV_DISCONN_COMPLETE => {
                // [0] Status, [1..2] Connection_Handle, [3] Reason
                if params.len() >= 3 {
                    let handle = (params[1] as u16) | ((params[2] as u16) << 8);
                    for dev in DISCOVERED_DEVICES.iter_mut().take(DISCOVERED_COUNT) {
                        if dev.handle == handle {
                            dev.connected = false;
                            dev.handle = 0;
                        }
                    }
                }
            }
            HCI_EV_REMOTE_NAME_REQ_COMPLETE => {
                // [0] Status, [1..6] BD_ADDR, [7..] Remote_Name (NUL terminated)
                if params.len() >= 7 {
                    let status = params[0];
                    if status == 0 {
                        let mut mac = [0u8; 6];
                        mac.copy_from_slice(&params[1..7]);
                        let name_slice = &params[7..];
                        for dev in DISCOVERED_DEVICES.iter_mut().take(DISCOVERED_COUNT) {
                            if dev.mac == mac {
                                let mut len = 0;
                                while len < name_slice.len() && len < 32 && name_slice[len] != 0 {
                                    dev.name[len] = name_slice[len];
                                    len += 1;
                                }
                                dev.name_len = len;
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        true
    }
}

unsafe fn handle_cmd_complete_return(opcode: u16, ret_params: &[u8]) {
    match opcode {
        HCI_OP_READ_BD_ADDR => {
            if ret_params.len() >= 6 {
                MAC_ADDR.copy_from_slice(&ret_params[..6]);
            }
        }
        HCI_OP_READ_LOCAL_VERSION => {
            if ret_params.len() >= 8 {
                HCI_VERSION = ret_params[0];
                HCI_REVISION = (ret_params[1] as u16) | ((ret_params[2] as u16) << 8);
                LMP_VERSION = ret_params[3];
                MANUFACTURER_NAME = (ret_params[4] as u16) | ((ret_params[5] as u16) << 8);
                LMP_SUBVERSION = (ret_params[6] as u16) | ((ret_params[7] as u16) << 8);
            }
        }
        _ => {}
    }
}

unsafe fn add_or_update_device(mac: [u8; 6], class: [u8; 3], rssi: i8, name: &[u8]) {
    for dev in DISCOVERED_DEVICES.iter_mut().take(DISCOVERED_COUNT) {
        if dev.mac == mac {
            dev.class_of_device = class;
            dev.rssi = rssi;
            if !name.is_empty() {
                let len = name.len().min(32);
                dev.name[..len].copy_from_slice(&name[..len]);
                dev.name_len = len;
            }
            return;
        }
    }

    if DISCOVERED_COUNT < MAX_DISCOVERED {
        let dev = &mut DISCOVERED_DEVICES[DISCOVERED_COUNT];
        dev.mac = mac;
        dev.class_of_device = class;
        dev.rssi = rssi;
        dev.connected = false;
        dev.handle = 0;
        if !name.is_empty() {
            let len = name.len().min(32);
            dev.name[..len].copy_from_slice(&name[..len]);
            dev.name_len = len;
        } else {
            dev.name_len = 0;
        }
        DISCOVERED_COUNT += 1;
    }
}

unsafe fn handle_synthetic_response(opcode: u16, params: &[u8]) {
    match opcode {
        HCI_OP_RESET => {
            let resp = [
                HCI_EVENT_PKT,
                HCI_EV_CMD_COMPLETE,
                4,
                1,
                (HCI_OP_RESET & 0xFF) as u8,
                ((HCI_OP_RESET >> 8) & 0xFF) as u8,
                0,
            ];
            process_hci_event(&resp);
        }
        HCI_OP_READ_BD_ADDR => {
            let resp = [
                HCI_EVENT_PKT,
                HCI_EV_CMD_COMPLETE,
                10,
                1,
                (HCI_OP_READ_BD_ADDR & 0xFF) as u8,
                ((HCI_OP_READ_BD_ADDR >> 8) & 0xFF) as u8,
                0,
                MAC_ADDR[0],
                MAC_ADDR[1],
                MAC_ADDR[2],
                MAC_ADDR[3],
                MAC_ADDR[4],
                MAC_ADDR[5],
            ];
            process_hci_event(&resp);
        }
        HCI_OP_INQUIRY => {
            STATE = BluetoothState::Inquiring;
            // Keşfedilen örnek cihaz ekle (Mouse / Kulaklık)
            let dev_mac = [0x54, 0xB7, 0xE5, 0x12, 0x34, 0x56];
            let dev_class = [0x04, 0x05, 0x24]; // Peripheral: Mouse
            add_or_update_device(dev_mac, dev_class, -55, b"BT Wireless Mouse");
        }
        HCI_OP_INQUIRY_CANCEL => {
            STATE = BluetoothState::Ready;
            let resp = [HCI_EVENT_PKT, HCI_EV_INQUIRY_COMPLETE, 1, 0];
            process_hci_event(&resp);
        }
        HCI_OP_CREATE_CONN => {
            STATE = BluetoothState::Connecting;
            if params.len() >= 6 {
                let mut mac = [0u8; 6];
                mac.copy_from_slice(&params[..6]);
                let resp = [
                    HCI_EVENT_PKT,
                    HCI_EV_CONN_COMPLETE,
                    11,
                    0,
                    0x40,
                    0x00,
                    mac[0],
                    mac[1],
                    mac[2],
                    mac[3],
                    mac[4],
                    mac[5],
                    1,
                    0,
                ];
                process_hci_event(&resp);
            }
        }
        HCI_OP_DISCONNECT => {
            if params.len() >= 2 {
                let h_lo = params[0];
                let h_hi = params[1];
                let resp = [
                    HCI_EVENT_PKT,
                    HCI_EV_DISCONN_COMPLETE,
                    4,
                    0,
                    h_lo,
                    h_hi,
                    0x16,
                ];
                process_hci_event(&resp);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Yüksek Seviyeli İşlemler
// ---------------------------------------------------------------------------

pub fn start_inquiry() -> bool {
    // Standart GIAC LAP (0x9E8B33), Inquiry Length = 8 (10.24 saniye), Maksimum Yanıt = 0 (Sınırsız)
    let params: [u8; 5] = [0x33, 0x8B, 0x9E, 0x08, 0x00];
    send_hci_command(HCI_OP_INQUIRY, &params)
}

pub fn stop_inquiry() -> bool {
    send_hci_command(HCI_OP_INQUIRY_CANCEL, &[])
}

pub fn connect(mac: [u8; 6]) -> bool {
    // Create Connection Parametreleri:
    // BD_ADDR (6), Packet_Type (2: 0xCC18 - DM1/DH1/DM3/DH3/DM5/DH5),
    // Page_Scan_Repetition_Mode (1: 0x02 R2), Reserved (1: 0x00),
    // Clock_Offset (2: 0x0000), Allow_Role_Switch (1: 0x01)
    let mut params = [0u8; 13];
    params[..6].copy_from_slice(&mac);
    params[6] = 0x18;
    params[7] = 0xCC;
    params[8] = 0x02;
    params[9] = 0x00;
    params[10] = 0x00;
    params[11] = 0x00;
    params[12] = 0x01;

    send_hci_command(HCI_OP_CREATE_CONN, &params)
}

pub fn disconnect(handle: u16) -> bool {
    // Handle (2), Reason (1: 0x13 - Remote User Terminated Connection)
    let params: [u8; 3] = [(handle & 0xFF) as u8, ((handle >> 8) & 0xFF) as u8, 0x13];
    send_hci_command(HCI_OP_DISCONNECT, &params)
}

// ---------------------------------------------------------------------------
// Durum ve Sorgulama API'si
// ---------------------------------------------------------------------------

pub fn is_detected() -> bool {
    unsafe { DEVICE_FOUND }
}

pub fn state() -> BluetoothState {
    unsafe { STATE }
}

pub fn mac_address() -> [u8; 6] {
    unsafe { MAC_ADDR }
}

pub fn name() -> &'static str {
    "Bluetooth Adapter"
}

pub fn name_tr() -> &'static str {
    "Bluetooth HCI Adaptörü"
}

pub fn discovered_device_count() -> usize {
    unsafe { DISCOVERED_COUNT }
}

pub fn discovered_devices() -> &'static [DiscoveredDevice] {
    unsafe { &DISCOVERED_DEVICES[..DISCOVERED_COUNT] }
}

pub fn clear_discovered_devices() {
    unsafe {
        DISCOVERED_COUNT = 0;
    }
}
