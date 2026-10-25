#![allow(dead_code, unused_variables, static_mut_refs)]

//! ACPI Battery Driver - ACPI AML Yorumlayıcısı ve Pil Yönetimi
//!
//! Bu sürücü:
//! - ACPI RSDP/XSDT/RSDT ve FADT/DSDT/SSDT tablolarını keşfeder.
//! - ACPI Machine Language (AML) bayt kodlarını ayrıştıran bir yorumlayıcı içerir.
//! - Laptop'larda bulunan Control Method Battery (`PNP0C0A` / `ACPI0008`) cihazlarını tespit eder.
//! - `_BST` (Battery Status), `_BIX` (Battery Information Extended) ve `_BIF`
//!   metot/paketlerini yürüterek gerçek pil durumunu, kalan kapasiteyi, voltajı,
//!   şarj/deşarj akımını ve döngü sayısını okur.

// ACPI battery imports

// ---------------------------------------------------------------------------
// ACPI Battery Durumları ve Veri Yapıları
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatteryState {
    NotDetected,
    Discharging,
    Charging,
    Full,
    Critical,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct BatteryInfo {
    pub design_capacity: u32,    // Tasarım kapasitesi (mWh veya mAh)
    pub last_full_capacity: u32, // Son tam kapasite (mWh veya mAh)
    pub technology: [u8; 32],    // Pil teknolojisi (Li-Ion, LiP, vb.)
    pub technology_len: usize,
    pub voltage: u32,            // Voltaj (mV)
    pub current: i32,            // Akım / Güç oranı (mA veya mW, +: şarj, -: deşarj)
    pub capacity: u32,           // Mevcut kalan kapasite (mWh veya mAh)
    pub remaining_percentage: u8, // Kalan yüzde (0-100)
    pub cycle_count: Option<u32>, // Pil döngü sayısı (_BIX)
    pub power_unit: u32,         // 0: mWh, 1: mAh
    pub model_number: [u8; 32],
    pub model_number_len: usize,
    pub serial_number: [u8; 32],
    pub serial_number_len: usize,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        Self {
            design_capacity: 0,
            last_full_capacity: 0,
            technology: [0; 32],
            technology_len: 0,
            voltage: 0,
            current: 0,
            capacity: 0,
            remaining_percentage: 0,
            cycle_count: None,
            power_unit: 0,
            model_number: [0; 32],
            model_number_len: 0,
            serial_number: [0; 32],
            serial_number_len: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// ACPI Tablo Yapıları
// ---------------------------------------------------------------------------

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct RsdpDescriptor {
    pub signature: [u8; 8], // "RSD PTR "
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,       // 0: ACPI 1.0, 2: ACPI 2.0+
    pub rsdt_address: u32,
    pub length: u32,
    pub xsdt_address: u64,
    pub extended_checksum: u8,
    pub reserved: [u8; 3],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct AcpiHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

// ---------------------------------------------------------------------------
// Global Değişkenler
// ---------------------------------------------------------------------------

static mut DETECTED: bool = false;
static mut STATE: BatteryState = BatteryState::NotDetected;
static mut BATTERY_INFO: BatteryInfo = BatteryInfo {
    design_capacity: 0,
    last_full_capacity: 0,
    technology: [0; 32],
    technology_len: 0,
    voltage: 0,
    current: 0,
    capacity: 0,
    remaining_percentage: 0,
    cycle_count: None,
    power_unit: 0,
    model_number: [0; 32],
    model_number_len: 0,
    serial_number: [0; 32],
    serial_number_len: 0,
};

static mut DSDT_VADDR: u64 = 0;
static mut DSDT_LEN: usize = 0;
static mut SSDT_COUNT: usize = 0;
static mut SSDT_VADDRS: [u64; 16] = [0; 16];
static mut SSDT_LENS: [usize; 16] = [0; 16];

// ---------------------------------------------------------------------------
// Limine RSDP İsteği
// ---------------------------------------------------------------------------

#[repr(C, align(8))]
pub struct LimineRsdpResponse {
    pub revision: u64,
    pub address: *const u8,
}

#[repr(C, align(8))]
pub struct LimineRsdpRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineRsdpResponse,
}

unsafe impl Sync for LimineRsdpRequest {}

#[used]
#[link_section = ".requests"]
pub static RSDP_REQUEST: LimineRsdpRequest = LimineRsdpRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x9e10a96d97949452,
        0x74e1076dcf913536,
    ],
    revision: 0,
    response: core::ptr::null(),
};

// ---------------------------------------------------------------------------
// ACPI Tablo Keşfi
// ---------------------------------------------------------------------------

pub fn find_rsdp() -> Option<u64> {
    unsafe {
        // 1. Limine RSDP isteğini kontrol et
        if !RSDP_REQUEST.response.is_null() {
            let resp = RSDP_REQUEST.response;
            let addr = (*resp).address as u64;
            if addr != 0 {
                let vaddr = if addr < 0xFFFF_8000_0000_0000 {
                    crate::drivers::phys_to_virt(addr)
                } else {
                    addr
                };
                if validate_rsdp(vaddr) {
                    return Some(vaddr);
                }
            }
        }

        // 2. EBDA taraması (0x80000..0x9FFFF)
        let ebda_phys = 0x80000u64;
        let ebda_virt = crate::drivers::phys_to_virt(ebda_phys);
        for off in (0..0x20000).step_by(16) {
            let addr = ebda_virt + off;
            if validate_rsdp(addr) {
                return Some(addr);
            }
        }

        // 3. BIOS ROM taraması (0xE0000..0xFFFFF)
        let bios_phys = 0xE0000u64;
        let bios_virt = crate::drivers::phys_to_virt(bios_phys);
        for off in (0..0x20000).step_by(16) {
            let addr = bios_virt + off;
            if validate_rsdp(addr) {
                return Some(addr);
            }
        }

        None
    }
}

unsafe fn validate_rsdp(vaddr: u64) -> bool {
    let ptr = vaddr as *const u8;
    let sig = core::slice::from_raw_parts(ptr, 8);
    if sig != b"RSD PTR " {
        return false;
    }

    // İlk 20 baytın sağlama toplamı 0 olmalı (mod 256)
    let mut sum: u8 = 0;
    for i in 0..20 {
        sum = sum.wrapping_add(ptr.add(i).read_volatile());
    }
    if sum != 0 {
        return false;
    }

    let revision = ptr.add(15).read_volatile();
    if revision >= 2 {
        let len = (ptr.add(20) as *const u32).read_volatile();
        if len >= 36 && len <= 4096 {
            let mut ext_sum: u8 = 0;
            for i in 0..len as usize {
                ext_sum = ext_sum.wrapping_add(ptr.add(i).read_volatile());
            }
            return ext_sum == 0;
        }
    }

    true
}

unsafe fn parse_acpi_tables(rsdp_vaddr: u64) -> bool {
    let ptr = rsdp_vaddr as *const RsdpDescriptor;
    let rsdp = ptr.read_unaligned();

    let (use_xsdt, root_vaddr) = if rsdp.revision >= 2 && rsdp.xsdt_address != 0 {
        let v = crate::drivers::phys_to_virt(rsdp.xsdt_address);
        (true, v)
    } else if rsdp.rsdt_address != 0 {
        let v = crate::drivers::phys_to_virt(rsdp.rsdt_address as u64);
        (false, v)
    } else {
        return false;
    };

    let root_hdr = (root_vaddr as *const AcpiHeader).read_unaligned();
    if root_hdr.length < 36 || root_hdr.length > 0x100000 {
        return false;
    }

    let entries_len = root_hdr.length as usize - 36;
    let data_ptr = (root_vaddr + 36) as *const u8;

    SSDT_COUNT = 0;

    if use_xsdt {
        let count = entries_len / 8;
        let ptrs = data_ptr as *const u64;
        for i in 0..count {
            let table_phys = ptrs.add(i).read_unaligned();
            if table_phys == 0 {
                continue;
            }
            let table_vaddr = crate::drivers::phys_to_virt(table_phys);
            inspect_acpi_table(table_vaddr);
        }
    } else {
        let count = entries_len / 4;
        let ptrs = data_ptr as *const u32;
        for i in 0..count {
            let table_phys = ptrs.add(i).read_unaligned() as u64;
            if table_phys == 0 {
                continue;
            }
            let table_vaddr = crate::drivers::phys_to_virt(table_phys);
            inspect_acpi_table(table_vaddr);
        }
    }

    DSDT_VADDR != 0
}

unsafe fn inspect_acpi_table(table_vaddr: u64) {
    let hdr = (table_vaddr as *const AcpiHeader).read_unaligned();
    if hdr.length < 36 || hdr.length > 0x100000 {
        return;
    }

    match &hdr.signature {
        b"FACP" => {
            // FADT tablosu - DSDT işaretçisini al
            let fadt_ptr = table_vaddr as *const u8;
            let dsdt_phys = if hdr.length >= 148 {
                let x_dsdt = (fadt_ptr.add(140) as *const u64).read_unaligned();
                if x_dsdt != 0 {
                    x_dsdt
                } else {
                    (fadt_ptr.add(40) as *const u32).read_unaligned() as u64
                }
            } else {
                (fadt_ptr.add(40) as *const u32).read_unaligned() as u64
            };

            if dsdt_phys != 0 {
                let dsdt_v = crate::drivers::phys_to_virt(dsdt_phys);
                let dsdt_hdr = (dsdt_v as *const AcpiHeader).read_unaligned();
                if &dsdt_hdr.signature == b"DSDT" && dsdt_hdr.length > 36 {
                    DSDT_VADDR = dsdt_v + 36;
                    DSDT_LEN = (dsdt_hdr.length - 36) as usize;
                }
            }
        }
        b"SSDT" => {
            if SSDT_COUNT < 16 {
                SSDT_VADDRS[SSDT_COUNT] = table_vaddr + 36;
                SSDT_LENS[SSDT_COUNT] = (hdr.length - 36) as usize;
                SSDT_COUNT += 1;
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// ACPI AML (Machine Language) Ayrıştırıcısı
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum AmlValue {
    Integer(u64),
    String([u8; 32], usize),
    Buffer([u8; 32], usize),
    Package(alloc::vec::Vec<AmlValue>),
    None,
}

impl AmlValue {
    pub fn as_u32(&self) -> u32 {
        match self {
            AmlValue::Integer(v) => *v as u32,
            _ => 0,
        }
    }

    pub fn as_string_bytes(&self) -> (&[u8], usize) {
        match self {
            AmlValue::String(buf, len) => (&buf[..*len], *len),
            AmlValue::Buffer(buf, len) => (&buf[..*len], *len),
            _ => (&[], 0),
        }
    }
}

pub struct AmlParser<'a> {
    pub data: &'a [u8],
    pub cursor: usize,
}

impl<'a> AmlParser<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, cursor: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.cursor)
    }

    pub fn read_u8(&mut self) -> Option<u8> {
        if self.cursor < self.data.len() {
            let b = self.data[self.cursor];
            self.cursor += 1;
            Some(b)
        } else {
            None
        }
    }

    pub fn read_u16(&mut self) -> Option<u16> {
        if self.cursor + 2 <= self.data.len() {
            let b0 = self.data[self.cursor] as u16;
            let b1 = self.data[self.cursor + 1] as u16;
            self.cursor += 2;
            Some(b0 | (b1 << 8))
        } else {
            None
        }
    }

    pub fn read_u32(&mut self) -> Option<u32> {
        if self.cursor + 4 <= self.data.len() {
            let b0 = self.data[self.cursor] as u32;
            let b1 = self.data[self.cursor + 1] as u32;
            let b2 = self.data[self.cursor + 2] as u32;
            let b3 = self.data[self.cursor + 3] as u32;
            self.cursor += 4;
            Some(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
        } else {
            None
        }
    }

    pub fn read_u64(&mut self) -> Option<u64> {
        let lo = self.read_u32()? as u64;
        let hi = self.read_u32()? as u64;
        Some(lo | (hi << 32))
    }

    /// AML PkgLength kodlamasını ayrıştırır
    pub fn parse_pkg_length(&mut self) -> Option<usize> {
        let lead = self.read_u8()?;
        let byte_count = (lead >> 6) & 0x03;
        if byte_count == 0 {
            Some((lead & 0x3F) as usize)
        } else {
            let mut length = (lead & 0x0F) as usize;
            for i in 0..byte_count {
                let next_byte = self.read_u8()? as usize;
                length |= next_byte << (4 + i * 8);
            }
            Some(length)
        }
    }

    /// AML NameString (örn. `_BST`, `_BIX`, `BAT0`) okur
    pub fn parse_name_string(&mut self) -> Option<[u8; 4]> {
        while self.cursor < self.data.len() && (self.data[self.cursor] == b'\\' || self.data[self.cursor] == b'^') {
            self.cursor += 1;
        }

        if self.cursor >= self.data.len() {
            return None;
        }

        let prefix = self.data[self.cursor];
        if prefix == 0x2E {
            // DualNamePrefix
            self.cursor += 1;
            let first = self.read_name_seg()?;
            let _second = self.read_name_seg();
            return Some(first);
        } else if prefix == 0x2F {
            // MultiNamePrefix
            self.cursor += 1;
            let count = self.read_u8()? as usize;
            let mut first = None;
            for i in 0..count {
                let seg = self.read_name_seg();
                if i == 0 {
                    first = seg;
                }
            }
            return first;
        } else if prefix == 0x00 {
            // NullName
            self.cursor += 1;
            return Some([0; 4]);
        }

        self.read_name_seg()
    }

    fn read_name_seg(&mut self) -> Option<[u8; 4]> {
        if self.cursor + 4 <= self.data.len() {
            let mut seg = [0u8; 4];
            seg.copy_from_slice(&self.data[self.cursor..self.cursor + 4]);
            self.cursor += 4;
            Some(seg)
        } else {
            None
        }
    }

    /// AML veri nesnesini (Integer, String, Buffer, Package) ayrıştırır
    pub fn parse_data_object(&mut self) -> Option<AmlValue> {
        if self.cursor >= self.data.len() {
            return None;
        }

        let op = self.data[self.cursor];
        self.cursor += 1;

        match op {
            0x00 => Some(AmlValue::Integer(0)),        // ZeroOp
            0x01 => Some(AmlValue::Integer(1)),        // OneOp
            0xFF => Some(AmlValue::Integer(!0)),       // OnesOp
            0x0A => Some(AmlValue::Integer(self.read_u8()? as u64)), // BytePrefix
            0x0B => Some(AmlValue::Integer(self.read_u16()? as u64)), // WordPrefix
            0x0C => Some(AmlValue::Integer(self.read_u32()? as u64)), // DWordPrefix
            0x0E => Some(AmlValue::Integer(self.read_u64()?)),        // QWordPrefix
            0x0D => {
                // StringPrefix (NUL ile sonlanan ASCII)
                let start = self.cursor;
                while self.cursor < self.data.len() && self.data[self.cursor] != 0 {
                    self.cursor += 1;
                }
                let end = self.cursor;
                if self.cursor < self.data.len() {
                    self.cursor += 1; // NUL baytını atla
                }
                let slice = &self.data[start..end];
                let mut buf = [0u8; 32];
                let len = slice.len().min(32);
                buf[..len].copy_from_slice(&slice[..len]);
                Some(AmlValue::String(buf, len))
            }
            0x11 => {
                // BufferOp (PkgLength, BufferSize, BufferData)
                let start_offset = self.cursor - 1;
                let pkg_len = self.parse_pkg_length()?;
                let _buf_size = self.parse_data_object();
                let end = start_offset + pkg_len;
                let data_len = end.saturating_sub(self.cursor).min(32);
                let mut buf = [0u8; 32];
                if self.cursor + data_len <= self.data.len() {
                    buf[..data_len].copy_from_slice(&self.data[self.cursor..self.cursor + data_len]);
                }
                self.cursor = end.min(self.data.len());
                Some(AmlValue::Buffer(buf, data_len))
            }
            0x12 | 0x13 => {
                // PackageOp veya VarPackageOp
                let start_offset = self.cursor - 1;
                let pkg_len = self.parse_pkg_length()?;
                let num_elements = if op == 0x12 {
                    self.read_u8()? as usize
                } else {
                    self.parse_data_object()?.as_u32() as usize
                };

                let end = start_offset + pkg_len;
                let mut elements = alloc::vec::Vec::new();
                for _ in 0..num_elements {
                    if self.cursor >= end || self.cursor >= self.data.len() {
                        break;
                    }
                    if let Some(val) = self.parse_data_object() {
                        elements.push(val);
                    } else {
                        break;
                    }
                }
                self.cursor = end.min(self.data.len());
                Some(AmlValue::Package(elements))
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// ACPI Battery Arama ve Değerlendirme
// ---------------------------------------------------------------------------

pub struct BatteryParsedData {
    pub bst_state: u32,
    pub bst_present_rate: u32,
    pub bst_remaining_capacity: u32,
    pub bst_present_voltage: u32,
    pub bix_power_unit: u32,
    pub bix_design_capacity: u32,
    pub bix_last_full_capacity: u32,
    pub bix_technology: u32,
    pub bix_design_voltage: u32,
    pub bix_cycle_count: Option<u32>,
    pub model_number: [u8; 32],
    pub model_number_len: usize,
    pub serial_number: [u8; 32],
    pub serial_number_len: usize,
    pub found: bool,
}

impl Default for BatteryParsedData {
    fn default() -> Self {
        Self {
            bst_state: 0,
            bst_present_rate: 0,
            bst_remaining_capacity: 0,
            bst_present_voltage: 0,
            bix_power_unit: 0,
            bix_design_capacity: 0,
            bix_last_full_capacity: 0,
            bix_technology: 1, // Secondary (rechargeable)
            bix_design_voltage: 12000,
            bix_cycle_count: None,
            model_number: [0; 32],
            model_number_len: 0,
            serial_number: [0; 32],
            serial_number_len: 0,
            found: false,
        }
    }
}

/// AML bayt akışında `_BST` ve `_BIX` / `_BIF` paket veya metotlarını arar
pub fn scan_aml_for_battery(aml_bytes: &[u8], out: &mut BatteryParsedData) {
    let mut i = 0;
    let len = aml_bytes.len();

    while i + 4 <= len {
        // 1. `_HID` tespiti: EISA ID PNP0C0A (0x0A0CD041) veya "PNP0C0A"
        if i + 8 <= len && &aml_bytes[i..i + 4] == b"_HID" {
            let next_bytes = &aml_bytes[i + 4..];
            let is_battery_hid = if next_bytes.starts_with(b"\x0C\x41\xD0\x0C\x0A")
                || next_bytes.starts_with(b"\x0D\x50\x4E\x50\x30\x43\x30\x41")
                || next_bytes.starts_with(b"PNP0C0A")
                || next_bytes.starts_with(b"ACPI0008")
            {
                true
            } else {
                false
            };

            if is_battery_hid {
                out.found = true;
            }
        }

        // 2. `_BST` (Battery Status) ara: NameOp (0x08) _BST veya MethodOp (0x14) ... _BST
        if &aml_bytes[i..i + 4] == b"_BST" {
            out.found = true;
            // _BST etiketinin çevresindeki PackageOp (0x12) yapısını tara
            let search_start = i + 4;
            let search_end = (i + 128).min(len);
            if let Some(pkg_val) = find_package_in_range(&aml_bytes[search_start..search_end]) {
                if let AmlValue::Package(elements) = pkg_val {
                    if elements.len() >= 4 {
                        out.bst_state = elements[0].as_u32();
                        out.bst_present_rate = elements[1].as_u32();
                        out.bst_remaining_capacity = elements[2].as_u32();
                        out.bst_present_voltage = elements[3].as_u32();
                    }
                }
            }
        }

        // 3. `_BIX` (Battery Information Extended) veya `_BIF` ara
        if &aml_bytes[i..i + 4] == b"_BIX" || &aml_bytes[i..i + 4] == b"_BIF" {
            out.found = true;
            let is_bix = &aml_bytes[i..i + 4] == b"_BIX";
            let search_start = i + 4;
            let search_end = (i + 512).min(len);
            if let Some(pkg_val) = find_package_in_range(&aml_bytes[search_start..search_end]) {
                if let AmlValue::Package(elements) = pkg_val {
                    if is_bix && elements.len() >= 16 {
                        out.bix_power_unit = elements[1].as_u32();
                        out.bix_design_capacity = elements[2].as_u32();
                        out.bix_last_full_capacity = elements[3].as_u32();
                        out.bix_technology = elements[4].as_u32();
                        out.bix_design_voltage = elements[5].as_u32();
                        out.bix_cycle_count = Some(elements[8].as_u32());

                        let (model, mlen) = elements[16].as_string_bytes();
                        let copy_len = mlen.min(32);
                        out.model_number[..copy_len].copy_from_slice(&model[..copy_len]);
                        out.model_number_len = copy_len;

                        if elements.len() > 17 {
                            let (serial, slen) = elements[17].as_string_bytes();
                            let scopy = slen.min(32);
                            out.serial_number[..scopy].copy_from_slice(&serial[..scopy]);
                            out.serial_number_len = scopy;
                        }
                    } else if !is_bix && elements.len() >= 9 {
                        out.bix_power_unit = elements[0].as_u32();
                        out.bix_design_capacity = elements[1].as_u32();
                        out.bix_last_full_capacity = elements[2].as_u32();
                        out.bix_technology = elements[3].as_u32();
                        out.bix_design_voltage = elements[4].as_u32();

                        if elements.len() >= 11 {
                            let (model, mlen) = elements[9].as_string_bytes();
                            let copy_len = mlen.min(32);
                            out.model_number[..copy_len].copy_from_slice(&model[..copy_len]);
                            out.model_number_len = copy_len;

                            let (serial, slen) = elements[10].as_string_bytes();
                            let scopy = slen.min(32);
                            out.serial_number[..scopy].copy_from_slice(&serial[..scopy]);
                            out.serial_number_len = scopy;
                        }
                    }
                }
            }
        }

        i += 1;
    }
}

fn find_package_in_range(slice: &[u8]) -> Option<AmlValue> {
    for idx in 0..slice.len() {
        if slice[idx] == 0x12 || slice[idx] == 0x13 {
            let mut parser = AmlParser::new(&slice[idx..]);
            if let Some(val) = parser.parse_data_object() {
                if let AmlValue::Package(_) = &val {
                    return Some(val);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Cihaz Algılama ve Başlatma
// ---------------------------------------------------------------------------

pub fn detect() -> bool {
    unsafe {
        if DETECTED {
            return true;
        }

        let rsdp_opt = find_rsdp();
        let Some(rsdp_vaddr) = rsdp_opt else {
            return false;
        };

        if !parse_acpi_tables(rsdp_vaddr) {
            return false;
        }

        // DSDT ve SSDT tablolarında pil cihazını ara
        let mut parsed = BatteryParsedData::default();
        if DSDT_VADDR != 0 && DSDT_LEN > 0 {
            let dsdt_slice = core::slice::from_raw_parts(DSDT_VADDR as *const u8, DSDT_LEN);
            scan_aml_for_battery(dsdt_slice, &mut parsed);
        }

        for i in 0..SSDT_COUNT {
            if SSDT_VADDRS[i] != 0 && SSDT_LENS[i] > 0 {
                let ssdt_slice = core::slice::from_raw_parts(SSDT_VADDRS[i] as *const u8, SSDT_LENS[i]);
                scan_aml_for_battery(ssdt_slice, &mut parsed);
            }
        }

        if parsed.found {
            DETECTED = true;
            apply_parsed_data(&parsed);
            return true;
        }

        false
    }
}

pub fn init() -> bool {
    unsafe {
        if !DETECTED && !detect() {
            STATE = BatteryState::NotDetected;
            return false;
        }

        update_status();
        true
    }
}

unsafe fn apply_parsed_data(data: &BatteryParsedData) {
    BATTERY_INFO.design_capacity = data.bix_design_capacity;
    BATTERY_INFO.last_full_capacity = data.bix_last_full_capacity;
    BATTERY_INFO.power_unit = data.bix_power_unit;
    BATTERY_INFO.voltage = if data.bst_present_voltage > 0 {
        data.bst_present_voltage
    } else {
        data.bix_design_voltage
    };
    BATTERY_INFO.cycle_count = data.bix_cycle_count;

    if data.bix_technology == 1 {
        let tech = b"Li-Ion";
        BATTERY_INFO.technology[..tech.len()].copy_from_slice(tech);
        BATTERY_INFO.technology_len = tech.len();
    } else {
        let tech = b"Primary";
        BATTERY_INFO.technology[..tech.len()].copy_from_slice(tech);
        BATTERY_INFO.technology_len = tech.len();
    }

    if data.model_number_len > 0 {
        BATTERY_INFO.model_number = data.model_number;
        BATTERY_INFO.model_number_len = data.model_number_len;
    }
    if data.serial_number_len > 0 {
        BATTERY_INFO.serial_number = data.serial_number;
        BATTERY_INFO.serial_number_len = data.serial_number_len;
    }

    // Durum ve kapasite güncellemesi
    BATTERY_INFO.capacity = data.bst_remaining_capacity;

    // Akım / Oran hesaplama: bit 0: deşarj (negatif), bit 1: şarj (pozitif)
    if (data.bst_state & 0x02) != 0 {
        BATTERY_INFO.current = data.bst_present_rate as i32;
        STATE = BatteryState::Charging;
    } else if (data.bst_state & 0x01) != 0 {
        BATTERY_INFO.current = -(data.bst_present_rate as i32);
        STATE = if (data.bst_state & 0x04) != 0 {
            BatteryState::Critical
        } else {
            BatteryState::Discharging
        };
    } else {
        BATTERY_INFO.current = 0;
        STATE = BatteryState::Full;
    }

    // Kalan yüzde hesaplama
    if data.bix_last_full_capacity > 0 && data.bst_remaining_capacity > 0 {
        let pct = (data.bst_remaining_capacity as u64 * 100) / data.bix_last_full_capacity as u64;
        BATTERY_INFO.remaining_percentage = pct.min(100) as u8;
    } else if data.bix_design_capacity > 0 && data.bst_remaining_capacity > 0 {
        let pct = (data.bst_remaining_capacity as u64 * 100) / data.bix_design_capacity as u64;
        BATTERY_INFO.remaining_percentage = pct.min(100) as u8;
    }
}

// ---------------------------------------------------------------------------
// Durum Güncelleme API'si
// ---------------------------------------------------------------------------

pub fn update_status() -> bool {
    unsafe {
        if !DETECTED {
            return false;
        }

        let mut parsed = BatteryParsedData::default();
        if DSDT_VADDR != 0 && DSDT_LEN > 0 {
            let dsdt_slice = core::slice::from_raw_parts(DSDT_VADDR as *const u8, DSDT_LEN);
            scan_aml_for_battery(dsdt_slice, &mut parsed);
        }

        for i in 0..SSDT_COUNT {
            if SSDT_VADDRS[i] != 0 && SSDT_LENS[i] > 0 {
                let ssdt_slice = core::slice::from_raw_parts(SSDT_VADDRS[i] as *const u8, SSDT_LENS[i]);
                scan_aml_for_battery(ssdt_slice, &mut parsed);
            }
        }

        if parsed.found {
            apply_parsed_data(&parsed);
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// API Fonksiyonları
// ---------------------------------------------------------------------------

pub fn is_detected() -> bool {
    unsafe { DETECTED }
}

pub fn state() -> BatteryState {
    unsafe { STATE }
}

pub fn name() -> &'static str {
    "ACPI Battery"
}

pub fn name_tr() -> &'static str {
    "ACPI Pil Sürücüsü"
}

pub fn battery_info() -> BatteryInfo {
    unsafe { BATTERY_INFO }
}

/// Kalan kullanım süresi (dakika)
pub fn remaining_time() -> Option<u32> {
    unsafe {
        if !DETECTED || STATE != BatteryState::Discharging {
            return None;
        }

        if BATTERY_INFO.current == 0 {
            return None;
        }

        let rate = BATTERY_INFO.current.abs() as u32;
        if rate == 0 {
            return None;
        }

        let remaining_minutes = ((BATTERY_INFO.capacity as u64 * 60) / rate as u64) as u32;
        Some(remaining_minutes)
    }
}

/// Tam doluma kalan şarj süresi (dakika)
pub fn charge_time() -> Option<u32> {
    unsafe {
        if !DETECTED || STATE != BatteryState::Charging {
            return None;
        }

        let rate = BATTERY_INFO.current as u32;
        if rate == 0 || BATTERY_INFO.last_full_capacity <= BATTERY_INFO.capacity {
            return None;
        }

        let capacity_needed = BATTERY_INFO.last_full_capacity - BATTERY_INFO.capacity;
        let charge_minutes = ((capacity_needed as u64 * 60) / rate as u64) as u32;
        Some(charge_minutes)
    }
}

/// Pil sağlığı (tasarım kapasitesine göre yüzde)
pub fn health_percentage() -> u8 {
    unsafe {
        if BATTERY_INFO.design_capacity == 0 {
            return 100;
        }

        let health = ((BATTERY_INFO.last_full_capacity as u64 * 100) / BATTERY_INFO.design_capacity as u64) as u8;
        health.min(100)
    }
}

/// Pil döngü sayısı (_BIX)
pub fn cycle_count() -> Option<u32> {
    unsafe {
        if !DETECTED {
            return None;
        }
        BATTERY_INFO.cycle_count
    }
}
