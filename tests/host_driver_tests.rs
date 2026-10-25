//! Host unit testleri: VirtIO-GPU, ACPI AML & Battery, Bluetooth HCI.

use crate::drivers::bluetooth::*;
use crate::drivers::virtio_gpu::*;
use crate::drivers::acpi_battery::*;

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
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, // MAC
        0x01,                               // Page Scan Repetition
        0x00, 0x00,                         // Reserved
        0x04, 0x05, 0x24,                   // Class: Mouse
        0x12, 0x34,                         // Clock Offset
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
        0x00,       // Status: Success
        0x42, 0x00, // Handle: 0x0042
        0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
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
        0x00,       // Status: Success
        0x42, 0x00, // Handle: 0x0042
        0x13,       // Reason
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
        0x00,                   // ZeroOp -> 0
        0x01,                   // OneOp -> 1
        0xFF,                   // OnesOp -> !0
        0x0A, 0x2A,             // BytePrefix -> 42
        0x0B, 0x34, 0x12,       // WordPrefix -> 0x1234
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
        0x12, 16, 4,            // PackageOp, PkgLen=16, NumElements=4
        0x0A, 0x01,             // Eleman 0: Byte (1: Discharging)
        0x0B, 0xE8, 0x03,       // Eleman 1: Word (1000: Present Rate 1000 mW)
        0x0C, 0x20, 0x4E, 0x00, 0x00, // Eleman 2: DWord (20000: Remaining Capacity 20000 mWh)
        0x0B, 0xE0, 0x2E,       // Eleman 3: Word (12000: Present Voltage 12000 mV)
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
        0x12, 16, 4,
        0x0A, 0x02,             // State = 2 (Charging)
        0x0B, 0xD0, 0x07,       // Present Rate = 2000 mW
        0x0C, 0x40, 0x9C, 0x00, 0x00, // Remaining Capacity = 40000 mWh
        0x0B, 0x60, 0x2E,       // Voltage = 11872 mV
    ]);

    // 3. _BIX
    aml.extend_from_slice(b"_BIX");
    aml.extend_from_slice(&[
        0x12, 50, 17,
        0x0A, 0x00, // Revision = 0
        0x0A, 0x00, // Power Unit = 0 (mWh)
        0x0C, 0x50, 0xC3, 0x00, 0x00, // Design Capacity = 50000 mWh
        0x0C, 0xE0, 0xB7, 0x00, 0x00, // Last Full Capacity = 47072 mWh
        0x0A, 0x01, // Tech = Secondary
        0x0B, 0xE0, 0x2E, // Design Voltage = 12000 mV
        0x0A, 0x00, // Warning
        0x0A, 0x00, // Low
        0x0A, 125,  // Cycle count = 125
        0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00, 0x0A, 0x00,
        0x0D, b'B', b'A', b'T', b'T', 0x00, // Model "BATT"
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
    let percentage = (parsed.bst_remaining_capacity as u64 * 100) / parsed.bix_last_full_capacity as u64;
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

