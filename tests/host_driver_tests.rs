//! Host unit testleri: VirtIO-GPU, ACPI AML & Battery, Bluetooth HCI, USB UHCI/OHCI.

use crate::drivers::acpi_battery::*;
use crate::drivers::bluetooth::*;
use crate::drivers::netstack::*;
use crate::drivers::usb::*;
use crate::drivers::virtio_gpu::*;

// ---------------------------------------------------------------------------
// 1. Bluetooth HCI Protokol Testleri
// ---------------------------------------------------------------------------

#[test]
fn test_hci_opcode_encoding_and_decoding() {
    // make_opcode, opcode_ogf, opcode_ocf doğrulaması
    let reset_op = make_opcode(OGF_HOST_CONTROL, OCF_RESET);
    assert_eq!(reset_op, HCI_OP_RESET);
    assert_eq!(opcode_ogf(reset_op), OGF_HOST_CONTROL);
    assert_eq!(opcode_ocf(reset_op), OCF_RESET);

    let inq_op = make_opcode(OGF_LINK_CONTROL, OCF_INQUIRY);
    assert_eq!(inq_op, HCI_OP_INQUIRY);
    assert_eq!(opcode_ogf(inq_op), OGF_LINK_CONTROL);
    assert_eq!(opcode_ocf(inq_op), OCF_INQUIRY);

    let bd_op = make_opcode(OGF_INFO_PARAM, OCF_READ_BD_ADDR);
    assert_eq!(bd_op, HCI_OP_READ_BD_ADDR);
    assert_eq!(opcode_ogf(bd_op), OGF_INFO_PARAM);
    assert_eq!(opcode_ocf(bd_op), OCF_READ_BD_ADDR);
}

#[test]
fn test_hci_command_packet_formatting() {
    let mut buf = [0u8; 32];
    let len = format_command_packet(HCI_OP_RESET, &[], &mut buf);
    assert_eq!(len, 4);
    assert_eq!(buf[0], HCI_COMMAND_PKT);
    assert_eq!(buf[1], (HCI_OP_RESET & 0xFF) as u8);
    assert_eq!(buf[2], ((HCI_OP_RESET >> 8) & 0xFF) as u8);
    assert_eq!(buf[3], 0); // Parametre uzunluğu 0

    // Parametreli komut
    let params = [0x33, 0x8B, 0x9E, 0x08, 0x00];
    let len2 = format_command_packet(HCI_OP_INQUIRY, &params, &mut buf);
    assert_eq!(len2, 4 + 5);
    assert_eq!(buf[0], HCI_COMMAND_PKT);
    assert_eq!(buf[1], (HCI_OP_INQUIRY & 0xFF) as u8);
    assert_eq!(buf[2], ((HCI_OP_INQUIRY >> 8) & 0xFF) as u8);
    assert_eq!(buf[3], 5);
    assert_eq!(&buf[4..9], &params);
}

#[test]
fn test_hci_acl_packet_formatting() {
    let mut buf = [0u8; 32];
    let data = [0x11, 0x22, 0x33, 0x44];
    let len = format_acl_packet(0x0040, 0x02, 0x00, &data, &mut buf);
    assert_eq!(len, 5 + 4);
    assert_eq!(buf[0], HCI_ACLDATA_PKT);
    // Handle (0x0040) | PB(0x02 << 12) = 0x2040
    assert_eq!(buf[1], 0x40);
    assert_eq!(buf[2], 0x20);
    // Data Length = 4
    assert_eq!(buf[3], 0x04);
    assert_eq!(buf[4], 0x00);
    assert_eq!(&buf[5..9], &data);
}

#[test]
fn test_hci_event_processing_and_device_tracking() {
    clear_discovered_devices();
    assert_eq!(discovered_device_count(), 0);

    // 1. Inquiry Result Event gönder
    // [0x04 (Event Indicator), 0x02 (EV_INQUIRY_RESULT), 15 (Len),
    //  1 (Num Responses), MAC (6 bytes), Repetition (1), Reserved (2), Class (3), ClockOffset (2)]
    let inq_event = [
        HCI_EVENT_PKT,
        HCI_EV_INQUIRY_RESULT,
        15,
        1, // 1 yanıt
        0xAA,
        0xBB,
        0xCC,
        0xDD,
        0xEE,
        0xFF, // MAC
        0x01, // Page Scan Repetition
        0x00,
        0x00, // Reserved
        0x04,
        0x05,
        0x24, // Class: Mouse
        0x12,
        0x34, // Clock Offset
    ];

    let processed = process_hci_event(&inq_event);
    assert!(processed);
    assert_eq!(discovered_device_count(), 1);

    let devs = discovered_devices();
    assert_eq!(devs[0].mac, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    assert_eq!(devs[0].class_of_device, [0x04, 0x05, 0x24]);
    assert!(!devs[0].connected);

    // 2. Connection Complete Event
    // [0x04, 0x03 (EV_CONN_COMPLETE), 11 (Len),
    //  Status=0, Handle=0x0042, MAC (6 bytes), LinkType=1, Encryption=0]
    let conn_event = [
        HCI_EVENT_PKT,
        HCI_EV_CONN_COMPLETE,
        11,
        0x00, // Status: Success
        0x42,
        0x00, // Handle: 0x0042
        0xAA,
        0xBB,
        0xCC,
        0xDD,
        0xEE,
        0xFF,
        0x01,
        0x00,
    ];

    process_hci_event(&conn_event);
    let devs = discovered_devices();
    assert!(devs[0].connected);
    assert_eq!(devs[0].handle, 0x0042);

    // 3. Disconnection Complete Event
    let disconn_event = [
        HCI_EVENT_PKT,
        HCI_EV_DISCONN_COMPLETE,
        4,
        0x00, // Status: Success
        0x42,
        0x00, // Handle: 0x0042
        0x13, // Reason
    ];

    process_hci_event(&disconn_event);
    let devs = discovered_devices();
    assert!(!devs[0].connected);
    assert_eq!(devs[0].handle, 0);
}

// ---------------------------------------------------------------------------
// 2. ACPI AML Yorumlayıcı ve Pil Testleri
// ---------------------------------------------------------------------------

#[test]
fn test_aml_pkg_length_parsing() {
    // 1-bayt uzunluk (lead < 64)
    let b1 = [0x1A]; // 26
    let mut p1 = AmlParser::new(&b1);
    assert_eq!(p1.parse_pkg_length(), Some(26));

    // 2-bayt uzunluk (lead = 0x40 | (len & 0x0F))
    // len = 0x0150 (336) -> lead = 0x40 | 0x00 = 0x40, byte2 = 0x15
    let b2 = [0x40, 0x15];
    let mut p2 = AmlParser::new(&b2);
    assert_eq!(p2.parse_pkg_length(), Some(336));
}

#[test]
fn test_aml_integer_parsing() {
    let data = [
        0x00, // ZeroOp -> 0
        0x01, // OneOp -> 1
        0xFF, // OnesOp -> !0
        0x0A, 0x2A, // BytePrefix -> 42
        0x0B, 0x34, 0x12, // WordPrefix -> 0x1234
        0x0C, 0x78, 0x56, 0x34, 0x12, // DWordPrefix -> 0x12345678
    ];

    let mut p = AmlParser::new(&data);
    assert_eq!(p.parse_data_object().unwrap().as_u32(), 0);
    assert_eq!(p.parse_data_object().unwrap().as_u32(), 1);
    assert_eq!(p.parse_data_object().unwrap().as_u32(), 0xFFFF_FFFF);
    assert_eq!(p.parse_data_object().unwrap().as_u32(), 42);
    assert_eq!(p.parse_data_object().unwrap().as_u32(), 0x1234);
    assert_eq!(p.parse_data_object().unwrap().as_u32(), 0x12345678);
}

#[test]
fn test_aml_package_parsing() {
    // PackageOp (0x12) ile 4 elemanlı bir paket (örn. _BST)
    let data = [
        0x12, 16, 4, // PackageOp, PkgLen=16, NumElements=4
        0x0A, 0x01, // Eleman 0: Byte (1: Discharging)
        0x0B, 0xE8, 0x03, // Eleman 1: Word (1000: Present Rate 1000 mW)
        0x0C, 0x20, 0x4E, 0x00, 0x00, // Eleman 2: DWord (20000: Remaining Capacity 20000 mWh)
        0x0B, 0xE0, 0x2E, // Eleman 3: Word (12000: Present Voltage 12000 mV)
    ];

    let mut p = AmlParser::new(&data);
    let val = p.parse_data_object().unwrap();
    if let AmlValue::Package(elements) = val {
        assert_eq!(elements.len(), 4);
        assert_eq!(elements[0].as_u32(), 1);
        assert_eq!(elements[1].as_u32(), 1000);
        assert_eq!(elements[2].as_u32(), 20000);
        assert_eq!(elements[3].as_u32(), 12000);
    } else {
        panic!("Package ayrıştırılamadı");
    }
}

#[test]
fn test_scan_aml_for_battery_bst_and_bix() {
    // Mock AML bayt dizisi: Name(_HID, "PNP0C0A"), Name(_BST, Package), Name(_BIX, Package)
    let mut aml = alloc::vec::Vec::new();

    // 1. _HID
    aml.extend_from_slice(b"_HID");
    aml.extend_from_slice(b"\x0D\x50\x4E\x50\x30\x43\x30\x41\x00"); // "PNP0C0A\0"

    // 2. _BST
    aml.extend_from_slice(b"_BST");
    aml.extend_from_slice(&[
        0x12, 16, 4, 0x0A, 0x02, // State = 2 (Charging)
        0x0B, 0xD0, 0x07, // Present Rate = 2000 mW
        0x0C, 0x40, 0x9C, 0x00, 0x00, // Remaining Capacity = 40000 mWh
        0x0B, 0x60, 0x2E, // Voltage = 11872 mV
    ]);

    // 3. _BIX
    aml.extend_from_slice(b"_BIX");
    aml.extend_from_slice(&[
        0x12, 50, 17, 0x0A, 0x00, // Revision = 0
        0x0A, 0x00, // Power Unit = 0 (mWh)
        0x0C, 0x50, 0xC3, 0x00, 0x00, // Design Capacity = 50000 mWh
        0x0C, 0xE0, 0xB7, 0x00, 0x00, // Last Full Capacity = 47072 mWh
        0x0A, 0x01, // Tech = Secondary
        0x0B, 0xE0, 0x2E, // Design Voltage = 12000 mV
        0x0A, 0x00, // Warning
        0x0A, 0x00, // Low
        0x0A, 125, // Cycle count = 125
        0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0D,
        b'B', b'A', b'T', b'T', 0x00, // Model "BATT"
    ]);

    let mut parsed = BatteryParsedData::default();
    scan_aml_for_battery(&aml, &mut parsed);

    assert!(parsed.found);
    assert_eq!(parsed.bst_state, 2);
    assert_eq!(parsed.bst_present_rate, 2000);
    assert_eq!(parsed.bst_remaining_capacity, 40000);
    assert_eq!(parsed.bst_present_voltage, 11872);

    assert_eq!(parsed.bix_design_capacity, 50000);
    assert_eq!(parsed.bix_last_full_capacity, 47072);
    assert_eq!(parsed.bix_cycle_count, Some(125));

    // Kalan yüzde hesabı: (40000 * 100) / 47072 = 84%
    let percentage =
        (parsed.bst_remaining_capacity as u64 * 100) / parsed.bix_last_full_capacity as u64;
    assert_eq!(percentage, 84);
}

// ---------------------------------------------------------------------------
// 3. VirtIO-GPU Paket ve Yapı Testleri
// ---------------------------------------------------------------------------

#[test]
fn test_virtio_gpu_structures() {
    use core::mem::size_of;

    assert_eq!(size_of::<VirtioGpuCtrlHdr>(), 24);
    assert_eq!(size_of::<VirtioGpuRect>(), 16);
    assert_eq!(size_of::<VirtioGpuDisplayOne>(), 24);
    assert_eq!(size_of::<VirtioGpuResourceCreate2d>(), 40);
    assert_eq!(size_of::<VirtioGpuSetScanout>(), 48);
    assert_eq!(size_of::<VirtioGpuTransferToHost2d>(), 56);
    assert_eq!(size_of::<VirtioGpuResourceFlush>(), 48);
    assert_eq!(size_of::<VirtqDesc>(), 16);

    // Komut ID doğrulamaları
    assert_eq!(VIRTIO_GPU_CMD_GET_DISPLAY_INFO, 0x0100);
    assert_eq!(VIRTIO_GPU_CMD_RESOURCE_CREATE_2D, 0x0101);
    assert_eq!(VIRTIO_GPU_CMD_RESOURCE_UNREF, 0x0102);
    assert_eq!(VIRTIO_GPU_CMD_SET_SCANOUT, 0x0103);
    assert_eq!(VIRTIO_GPU_CMD_RESOURCE_FLUSH, 0x0104);
    assert_eq!(VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D, 0x0105);
    assert_eq!(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, 0x0106);

    // Yanıt ID doğrulamaları
    assert_eq!(VIRTIO_GPU_RESP_OK_NODATA, 0x1100);
    assert_eq!(VIRTIO_GPU_RESP_OK_DISPLAY_INFO, 0x1101);

    // Format doğrulaması
    assert_eq!(VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM, 1);
}

// ---------------------------------------------------------------------------
// 4. USB UHCI & OHCI Transfer Katmanı Testleri
// ---------------------------------------------------------------------------

#[test]
fn test_uhci_token_and_td_status() {
    // 1. Token oluşturma: Setup, In, Out
    let setup_tok = uhci_make_token(UHCI_PID_SETUP, 5, 0, false, 8);
    assert_eq!((setup_tok & 0xFF) as u8, UHCI_PID_SETUP);
    assert_eq!(((setup_tok >> 8) & 0x7F) as u8, 5); // Dev addr 5
    assert_eq!(((setup_tok >> 15) & 0x0F) as u8, 0); // EP 0
    assert_eq!((setup_tok >> 19) & 1, 0); // Data toggle 0
    assert_eq!((setup_tok >> 21) & 0x7FF, 7); // Len: 8 - 1 = 7

    let in_tok = uhci_make_token(UHCI_PID_IN, 2, 1, true, 64);
    assert_eq!((in_tok & 0xFF) as u8, UHCI_PID_IN);
    assert_eq!(((in_tok >> 8) & 0x7F) as u8, 2);
    assert_eq!(((in_tok >> 15) & 0x0F) as u8, 1);
    assert_eq!((in_tok >> 19) & 1, 1);
    assert_eq!((in_tok >> 21) & 0x7FF, 63);

    // 2. TD durum metodları
    let td_active = UhciTd::new(1, UHCI_TD_STATUS_ACTIVE, setup_tok, 0x1000);
    assert!(td_active.is_active());
    assert!(!td_active.is_success());

    let td_done = UhciTd::new(1, 0x0000_0007, setup_tok, 0x1000); // 8 bytes transferred, no errors
    assert!(!td_done.is_active());
    assert!(td_done.is_success());
    assert_eq!(td_done.actual_len(), 8);

    let td_stall = UhciTd::new(1, UHCI_TD_STATUS_STALL, setup_tok, 0x1000);
    assert!(!td_stall.is_active());
    assert!(!td_stall.is_success());
    assert_eq!(td_stall.error_flags() & 0x20, 0x20); // bit 22 (shifted by 17 -> bit 5)

    // 3. QH yapıları
    let mut qh = UhciQh::empty();
    assert_eq!(qh.head_link, 1);
    assert_eq!(qh.element_link, 1);
    qh.set_element(0x2000, false);
    assert_eq!(qh.element_link, 0x2000);
    qh.terminate_element();
    assert_eq!(qh.element_link, 1);
}

#[test]
fn test_uhci_build_control_transfer() {
    let mut tds = [UhciTd::empty(); 4];
    let mut qh = UhciQh::empty();

    // Verili kontrol transferi (GET_DESCRIPTOR: Setup -> IN -> Status OUT)
    let count = uhci_build_control_transfer(
        &mut tds,
        &mut qh,
        0x1000, // td_base_phys
        1,      // dev_addr
        false,  // full-speed
        0x5000, // setup buffer
        0x6000, // data buffer
        18,     // data len (18 bytes descriptor)
        true,   // is_in
    );
    assert_eq!(count, 3);
    assert_eq!(qh.element_link, 0x1000);

    // Setup TD
    assert_eq!((tds[0].token & 0xFF) as u8, UHCI_PID_SETUP);
    assert!(tds[0].is_active());
    assert_eq!(tds[0].buffer, 0x5000);
    assert_eq!(tds[0].link, 0x1000 + core::mem::size_of::<UhciTd>() as u32);

    // Data TD
    assert_eq!((tds[1].token & 0xFF) as u8, UHCI_PID_IN);
    assert_eq!((tds[1].token >> 19) & 1, 1); // DATA1
    assert_eq!(tds[1].buffer, 0x6000);

    // Status TD
    assert_eq!((tds[2].token & 0xFF) as u8, UHCI_PID_OUT);
    assert_eq!(tds[2].link, 1); // Terminate

    // Verisiz kontrol transferi (SET_ADDRESS: Setup -> Status IN)
    let count0 = uhci_build_control_transfer(
        &mut tds, &mut qh, 0x1000, 0, false, 0x5000, 0, 0, false,
    );
    assert_eq!(count0, 2);
    assert_eq!((tds[1].token & 0xFF) as u8, UHCI_PID_IN);
}

#[test]
fn test_ohci_ed_and_td_descriptors() {
    // 1. ED oluşturma
    let mut ed = OhciEd::new(3, 1, 2, false, 64); // Dev 3, EP 1, IN, FullSpeed, MPS 64
    assert_eq!(ed.flags & 0x7F, 3);
    assert_eq!((ed.flags >> 7) & 0x0F, 1);
    assert_eq!((ed.flags >> 11) & 0x03, 2);
    assert_eq!((ed.flags >> 13) & 1, 0);
    assert_eq!((ed.flags >> 16) & 0x7FF, 64);
    assert!(ed.is_empty());

    ed.set_queue(0x3000, 0x3060);
    assert!(!ed.is_empty());
    assert_eq!(ed.head_p, 0x3000);
    assert_eq!(ed.tail_p, 0x3060);

    // 2. TD durum ve condition code
    let td = OhciTd::new(OHCI_TD_DIR_SETUP, 2, 0x4000, 8, 0x3020);
    assert!(!td.is_done());
    assert!(!td.is_success());
    assert_eq!(td.condition_code(), OHCI_CC_NOT_ACCESSED);
    assert_eq!(td.cbp, 0x4000);
    assert_eq!(td.be, 0x4007);
    assert_eq!(td.next_td, 0x3020);

    let td_done = OhciTd {
        flags: 0, // CC = 0 (NoError)
        cbp: 0,
        next_td: 0,
        be: 0,
    };
    assert!(td_done.is_done());
    assert!(td_done.is_success());
}

#[test]
fn test_ohci_build_control_transfer() {
    let mut tds = [OhciTd::empty(); 4];
    let mut ed = OhciEd::empty();

    let count = ohci_build_control_transfer(
        &mut tds,
        &mut ed,
        0x2000, // td_base_phys
        0x5000, // setup_phys
        0x6000, // data_phys
        18,     // data_len
        true,   // is_in
    );
    assert_eq!(count, 3);
    assert_eq!(ed.head_p, 0x2000);
    assert_eq!(
        ed.tail_p,
        0x2000 + 3 * core::mem::size_of::<OhciTd>() as u32
    );

    // TD 0: Setup
    assert_eq!((tds[0].flags >> 19) & 0x03, OHCI_TD_DIR_SETUP);
    assert_eq!((tds[0].flags >> 24) & 0x03, 2); // DATA0

    // TD 1: Data IN
    assert_eq!((tds[1].flags >> 19) & 0x03, OHCI_TD_DIR_IN);
    assert_eq!((tds[1].flags >> 24) & 0x03, 3); // DATA1

    // TD 2: Status OUT
    assert_eq!((tds[2].flags >> 19) & 0x03, OHCI_TD_DIR_OUT);
    assert_eq!((tds[2].flags >> 24) & 0x03, 3); // DATA1
}

// ---------------------------------------------------------------------------
// 5. ACPI AML Thermal Zone ve Power Cihazı Testleri
// ---------------------------------------------------------------------------

#[test]
fn test_aml_thermal_zone_parsing() {
    let mut aml = alloc::vec::Vec::new();
    // ThermalZoneOp (0x5B 0x85), PkgLength = 26, Name = "TZ01"
    aml.extend_from_slice(&[0x5B, 0x85, 26, b'T', b'Z', b'0', b'1']);
    // Method(_TMP, 0): 0x14, len 7, _TMP, flags 0, Return(WordConst 3012 = 0x0BC4)
    aml.extend_from_slice(&[0x14, 0x07, b'_', b'T', b'M', b'P', 0x00, 0xA4, 0x0B, 0xC4, 0x0B]);
    // Name(_CRT, WordConst 3656 = 0x0E48): 0x08, _CRT, 0x0B, 0x48, 0x0E
    aml.extend_from_slice(&[0x08, b'_', b'C', b'R', b'T', 0x0B, 0x48, 0x0E]);

    let mut tz_list = [ThermalZoneParsedData::default(); 8];
    let count = scan_aml_for_thermal_zones(&aml, &mut tz_list);

    assert_eq!(count, 1);
    assert!(tz_list[0].found);
    assert_eq!(&tz_list[0].name, b"TZ01");
    assert_eq!(tz_list[0].current_temp_k, 3012);
    assert_eq!(tz_list[0].critical_temp_k, 3656);

    // Kelvin tenths -> Celsius dönüştürme kontrolü
    assert_eq!(kelvin_tenths_to_celsius(3012), 28); // 301.2 K = 28.05 °C
    assert_eq!(kelvin_tenths_to_celsius(3656), 92); // 365.6 K = 92.45 °C
    assert_eq!(kelvin_tenths_to_celsius(2732), 0); // 273.2 K = 0.05 °C
}

#[test]
fn test_aml_extended_opcodes_and_power_parsing() {
    let mut aml = alloc::vec::Vec::new();
    // Name(_S3, Package): 0x08, _S3, 0x12, len 4, elements 1, byte 0x01
    aml.extend_from_slice(&[0x08, b'_', b'S', b'3', 0x12, 0x04, 0x01, 0x01]);
    // Name(_S5, Package): 0x08, _S5, 0x12, len 4, elements 1, byte 0x05
    aml.extend_from_slice(&[0x08, b'_', b'S', b'5', 0x12, 0x04, 0x01, 0x05]);

    // PowerResource (0x5B 0x84), len 30, name "PWR0", level 0, order 0
    aml.extend_from_slice(&[0x5B, 0x84, 30, b'P', b'W', b'R', b'0', 0x00, 0x00, 0x00]);
    // Body içinde Method(_ON_): 0x14, len 5, _ON_, flags 0
    aml.extend_from_slice(&[0x14, 0x05, b'_', b'O', b'N', b'_', 0x00]);
    // Body içinde Method(_OFF): 0x14, len 5, _OFF, flags 0
    aml.extend_from_slice(&[0x14, 0x05, b'_', b'O', b'F', b'F', 0x00]);

    let mut pw_list = [PowerDeviceParsedData::default(); 8];
    let (count, supported_states) = scan_aml_for_power_devices(&aml, &mut pw_list);

    assert_eq!(count, 1);
    assert!(pw_list[0].found);
    assert_eq!(&pw_list[0].name, b"PWR0");
    assert!(pw_list[0].has_on);
    assert!(pw_list[0].has_off);

    // S3 ve S5 durumları desteklenmeli
    assert_ne!(supported_states & (1 << 3), 0);
    assert_ne!(supported_states & (1 << 5), 0);
}

// ---------------------------------------------------------------------------
// 6. IPv6 Ağ Yığını Testleri
// ---------------------------------------------------------------------------

#[test]
fn test_ipv6_address_parsing_and_formatting() {
    // 1. Loopback (::1)
    let loopback = parse_ipv6("::1").expect("::1 ayrıştırılamadı");
    assert!(loopback.is_loopback());
    assert!(!loopback.is_multicast());
    assert!(!loopback.is_link_local());
    assert_eq!(loopback.format(), "0:0:0:0:0:0:0:1");

    // 2. Unspecified (::)
    let unspec = parse_ipv6("::").expect(":: ayrıştırılamadı");
    assert_eq!(unspec, Ipv6Address::UNSPECIFIED);

    // 3. Link-local (fe80::1)
    let link_local = parse_ipv6("fe80::1").expect("fe80::1 ayrıştırılamadı");
    assert!(link_local.is_link_local());
    assert_eq!(link_local.format(), "fe80:0:0:0:0:0:0:1");

    // 4. Parantezli link-local ([fe80::200:ff:fe00:1])
    let bracketed = parse_ipv6("[fe80::200:ff:fe00:1]").expect("bracketed ayrıştırılamadı");
    assert!(bracketed.is_link_local());
    assert_eq!(bracketed.format(), "fe80:0:0:0:200:ff:fe00:1");

    // 5. Global Unicast (2001:db8::1)
    let global = parse_ipv6("2001:db8::1").expect("2001:db8::1 ayrıştırılamadı");
    assert!(!global.is_link_local());
    assert!(!global.is_multicast());
    assert_eq!(global.format(), "2001:db8:0:0:0:0:0:1");
}

#[test]
fn test_ipv6_multicast_mac() {
    // ff02::1 (All Nodes Multicast)
    let mcast_nodes = parse_ipv6("ff02::1").expect("ff02::1 ayrıştırılamadı");
    assert!(mcast_nodes.is_multicast());
    // RFC 2464: MAC 33:33:xx:xx:xx:xx (alt 32 bit = 00:00:00:01)
    let mac = mcast_nodes.multicast_mac();
    assert_eq!(mac.0, [0x33, 0x33, 0x00, 0x00, 0x00, 0x01]);

    // ff02::1:ff12:3456 (Solicited-Node Multicast)
    let mcast_sn = parse_ipv6("ff02::1:ff12:3456").expect("solicited node ayrıştırılamadı");
    assert!(mcast_sn.is_multicast());
    let mac2 = mcast_sn.multicast_mac();
    assert_eq!(mac2.0, [0x33, 0x33, 0xff, 0x12, 0x34, 0x56]);
}

#[test]
fn test_ipv6_pseudo_header_checksum() {
    let src = [0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0x50, 0x54, 0, 0xff, 0xfe, 0x12, 0x34, 0x56];
    let dst = [0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    let payload = [0x80, 0x00, 0x00, 0x00, 0x12, 0x34, 0x00, 0x01]; // Echo Request dummy
    let csum = ipv6_pseudo_checksum(&src, &dst, 58, &payload);
    assert_ne!(csum, 0);

    // Aynı sağlama toplamı başlığa yazıldığında toplam doğrulama 0 (veya invert hali) olmalı
    let mut verified_payload = payload;
    verified_payload[2..4].copy_from_slice(&csum.to_be_bytes());
    let verify = ipv6_pseudo_checksum(&src, &dst, 58, &verified_payload);
    assert_eq!(verify, 0xFFFF);
}
