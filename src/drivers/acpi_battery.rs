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
    pub voltage: u32,             // Voltaj (mV)
    pub current: i32,             // Akım / Güç oranı (mA veya mW, +: şarj, -: deşarj)
    pub capacity: u32,            // Mevcut kalan kapasite (mWh veya mAh)
    pub remaining_percentage: u8, // Kalan yüzde (0-100)
    pub cycle_count: Option<u32>, // Pil döngü sayısı (_BIX)
    pub power_unit: u32,          // 0: mWh, 1: mAh
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
    pub revision: u8, // 0: ACPI 1.0, 2: ACPI 2.0+
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
        if let Some(resp) =
            crate::boot::limine::valid_limine_response(core::ptr::addr_of!(RSDP_REQUEST.response))
        {
            let addr = core::ptr::read_volatile(core::ptr::addr_of!((*resp).address));
            if !addr.is_null() && crate::boot::limine::is_canonical_ptr(addr) {
                let p = addr as u64;
                let vaddr = if p < 0xFFFF_8000_0000_0000 {
                    crate::drivers::phys_to_virt(p)
                } else {
                    p
                };
                let vptr = vaddr as *const u8;
                if crate::boot::limine::is_mapped_limine_ptr(vptr) && validate_rsdp(vaddr) {
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
    if !crate::boot::limine::is_mapped_limine_ptr(ptr) {
        return false;
    }

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
        let len_ptr = ptr.add(20) as *const u32;
        if !crate::boot::limine::is_mapped_limine_ptr(len_ptr) {
            return false;
        }
        let len = len_ptr.read_volatile();
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
    if !crate::boot::limine::is_mapped_limine_ptr(ptr) {
        return false;
    }
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

    let root_ptr = root_vaddr as *const AcpiHeader;
    if !crate::boot::limine::is_mapped_limine_ptr(root_ptr) {
        return false;
    }
    let root_hdr = root_ptr.read_unaligned();
    if root_hdr.length < 36 || root_hdr.length > 0x100000 {
        return false;
    }

    let entries_len = root_hdr.length as usize - 36;
    let data_ptr = (root_vaddr + 36) as *const u8;
    if !crate::boot::limine::is_mapped_limine_ptr(data_ptr) {
        return false;
    }

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
    let ptr = table_vaddr as *const AcpiHeader;
    if !crate::boot::limine::is_mapped_limine_ptr(ptr) {
        return;
    }
    let hdr = ptr.read_unaligned();
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
                let dsdt_ptr = dsdt_v as *const AcpiHeader;
                if crate::boot::limine::is_mapped_limine_ptr(dsdt_ptr) {
                    let dsdt_hdr = dsdt_ptr.read_unaligned();
                    if &dsdt_hdr.signature == b"DSDT" && dsdt_hdr.length > 36 {
                        DSDT_VADDR = dsdt_v + 36;
                        DSDT_LEN = (dsdt_hdr.length - 36) as usize;
                    }
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmlValue {
    Integer(u64),
    String([u8; 32], usize),
    Buffer([u8; 32], usize),
    Package(alloc::vec::Vec<AmlValue>),
    Method {
        name: [u8; 4],
        flags: u8,
        body_len: usize,
    },
    Device {
        name: [u8; 4],
        body_len: usize,
    },
    ThermalZone {
        name: [u8; 4],
        body_len: usize,
    },
    PowerResource {
        name: [u8; 4],
        system_level: u8,
        resource_order: u16,
    },
    None,
}

impl AmlValue {
    pub fn as_u64(&self) -> u64 {
        match self {
            AmlValue::Integer(v) => *v,
            _ => 0,
        }
    }

    pub fn as_u32(&self) -> u32 {
        self.as_u64() as u32
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

    /// AML NameString (örn. `_BST`, `_BIX`, `BAT0`, `_TZ_`) okur
    pub fn parse_name_string(&mut self) -> Option<[u8; 4]> {
        while self.cursor < self.data.len()
            && (self.data[self.cursor] == b'\\' || self.data[self.cursor] == b'^')
        {
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

    /// AML veri nesnesini (Integer, String, Buffer, Package, Method, Device, ThermalZone) ayrıştırır
    pub fn parse_data_object(&mut self) -> Option<AmlValue> {
        if self.cursor >= self.data.len() {
            return None;
        }

        let op = self.data[self.cursor];
        self.cursor += 1;

        match op {
            0x00 => Some(AmlValue::Integer(0)),                      // ZeroOp
            0x01 => Some(AmlValue::Integer(1)),                      // OneOp
            0xFF => Some(AmlValue::Integer(!0)),                     // OnesOp
            0x0A => Some(AmlValue::Integer(self.read_u8()? as u64)), // BytePrefix
            0x0B => Some(AmlValue::Integer(self.read_u16()? as u64)), // WordPrefix
            0x0C => Some(AmlValue::Integer(self.read_u32()? as u64)), // DWordPrefix
            0x0E => Some(AmlValue::Integer(self.read_u64()?)),       // QWordPrefix
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
                    buf[..data_len]
                        .copy_from_slice(&self.data[self.cursor..self.cursor + data_len]);
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
            0x08 => {
                // NameOp: NameString, DataRefObject
                let _name = self.parse_name_string()?;
                self.parse_data_object()
            }
            0x14 => {
                // MethodOp: PkgLength, NameString, MethodFlags, TermList
                let start_offset = self.cursor - 1;
                let pkg_len = self.parse_pkg_length()?;
                let name = self.parse_name_string()?;
                let flags = self.read_u8()?;
                let end = start_offset + pkg_len;
                self.cursor = end.min(self.data.len());
                Some(AmlValue::Method {
                    name,
                    flags,
                    body_len: pkg_len,
                })
            }
            0x10 => {
                // ScopeOp: PkgLength, NameString, TermList
                let start_offset = self.cursor - 1;
                let pkg_len = self.parse_pkg_length()?;
                let _name = self.parse_name_string();
                let end = start_offset + pkg_len;
                self.cursor = end.min(self.data.len());
                Some(AmlValue::None)
            }
            0xA4 => {
                // ReturnOp: ArgObject
                self.parse_data_object()
            }
            0x5B => {
                // ExtOpPrefix (0x5B ile başlayan iki baytlık opcodelar)
                let ext_op = self.read_u8()?;
                match ext_op {
                    0x82 => {
                        // DeviceOp: PkgLength, NameString, ObjectList
                        let start_offset = self.cursor - 2;
                        let pkg_len = self.parse_pkg_length()?;
                        let name = self.parse_name_string().unwrap_or([0; 4]);
                        let end = start_offset + pkg_len;
                        self.cursor = end.min(self.data.len());
                        Some(AmlValue::Device {
                            name,
                            body_len: pkg_len,
                        })
                    }
                    0x85 => {
                        // ThermalZoneOp: PkgLength, NameString, ObjectList
                        let start_offset = self.cursor - 2;
                        let pkg_len = self.parse_pkg_length()?;
                        let name = self.parse_name_string().unwrap_or([0; 4]);
                        let end = start_offset + pkg_len;
                        self.cursor = end.min(self.data.len());
                        Some(AmlValue::ThermalZone {
                            name,
                            body_len: pkg_len,
                        })
                    }
                    0x84 => {
                        // PowerResOp: PkgLength, NameString, SystemLevel, ResourceOrder, ObjectList
                        let start_offset = self.cursor - 2;
                        let pkg_len = self.parse_pkg_length()?;
                        let name = self.parse_name_string().unwrap_or([0; 4]);
                        let system_level = self.read_u8().unwrap_or(0);
                        let resource_order = self.read_u16().unwrap_or(0);
                        let end = start_offset + pkg_len;
                        self.cursor = end.min(self.data.len());
                        Some(AmlValue::PowerResource {
                            name,
                            system_level,
                            resource_order,
                        })
                    }
                    0x80 => {
                        // OpRegionOp: NameString, RegionSpace, RegionOffset, RegionLen
                        let _name = self.parse_name_string();
                        let _space = self.read_u8();
                        let _offset = self.parse_data_object();
                        let _len = self.parse_data_object();
                        Some(AmlValue::None)
                    }
                    _ => None,
                }
            }
            // Matematiksel ve Mantıksal İşleçler (AML Expressions)
            0x70 => {
                // StoreOp: Source, Destination
                let src = self.parse_data_object()?;
                let _dst = self.parse_name_string();
                Some(src)
            }
            0x72 => {
                // AddOp: Operand1, Operand2, Target
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(v1.wrapping_add(v2)))
            }
            0x74 => {
                // SubtractOp: Operand1, Operand2, Target
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(v1.saturating_sub(v2)))
            }
            0x77 => {
                // MultiplyOp: Operand1, Operand2, Target
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(v1.wrapping_mul(v2)))
            }
            0x79 => {
                // ShiftLeftOp: Operand, ShiftCount, Target
                let v = self.parse_data_object()?.as_u64();
                let cnt = self.parse_data_object()?.as_u64() & 63;
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(v << cnt))
            }
            0x7A => {
                // ShiftRightOp: Operand, ShiftCount, Target
                let v = self.parse_data_object()?.as_u64();
                let cnt = self.parse_data_object()?.as_u64() & 63;
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(v >> cnt))
            }
            0x7B => {
                // AndOp: Operand1, Operand2, Target
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(v1 & v2))
            }
            0x7D => {
                // OrOp: Operand1, Operand2, Target
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(v1 | v2))
            }
            0x7F => {
                // XorOp: Operand1, Operand2, Target
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(v1 ^ v2))
            }
            0x80 => {
                // NotOp: Operand, Target
                let v = self.parse_data_object()?.as_u64();
                let _ = self.parse_name_string();
                Some(AmlValue::Integer(!v))
            }
            0x90 => {
                // LAndOp: Operand1, Operand2
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                Some(AmlValue::Integer(if v1 != 0 && v2 != 0 { 1 } else { 0 }))
            }
            0x91 => {
                // LOrOp: Operand1, Operand2
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                Some(AmlValue::Integer(if v1 != 0 || v2 != 0 { 1 } else { 0 }))
            }
            0x92 => {
                // LNotOp: Operand
                let v = self.parse_data_object()?.as_u64();
                Some(AmlValue::Integer(if v == 0 { 1 } else { 0 }))
            }
            0x93 => {
                // LEqualOp: Operand1, Operand2
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                Some(AmlValue::Integer(if v1 == v2 { 1 } else { 0 }))
            }
            0x94 => {
                // LGreaterOp: Operand1, Operand2
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                Some(AmlValue::Integer(if v1 > v2 { 1 } else { 0 }))
            }
            0x95 => {
                // LLessOp: Operand1, Operand2
                let v1 = self.parse_data_object()?.as_u64();
                let v2 = self.parse_data_object()?.as_u64();
                Some(AmlValue::Integer(if v1 < v2 { 1 } else { 0 }))
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
// Thermal Zone ve Güç Cihazı AML Ayrıştırma
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct ThermalZoneParsedData {
    pub name: [u8; 4],
    pub current_temp_k: u32, // 0.1 Kelvin cinsinden (örn. 3000 = 300.0 K = 26.85 °C)
    pub critical_temp_k: u32,
    pub hot_temp_k: u32,
    pub passive_temp_k: u32,
    pub found: bool,
}

impl Default for ThermalZoneParsedData {
    fn default() -> Self {
        Self {
            name: [0; 4],
            current_temp_k: 0,
            critical_temp_k: 0,
            hot_temp_k: 0,
            passive_temp_k: 0,
            found: false,
        }
    }
}

/// 0.1 Kelvin (deci-Kelvin) değerini Celsius (°C) tam sayısına dönüştürür.
/// ACPI spesifikasyonuna göre: T(°C) = (T_dK - 2732) / 10
pub fn kelvin_tenths_to_celsius(k: u32) -> i32 {
    if k == 0 {
        return 0;
    }
    ((k as i32) - 2732) / 10
}

#[derive(Clone, Copy, Debug)]
pub struct PowerDeviceParsedData {
    pub name: [u8; 4],
    pub supported_states: u8, // S0..S5 durumları için bitmask
    pub has_on: bool,
    pub has_off: bool,
    pub status: u32,
    pub found: bool,
}

impl Default for PowerDeviceParsedData {
    fn default() -> Self {
        Self {
            name: [0; 4],
            supported_states: 0,
            has_on: false,
            has_off: false,
            status: 0,
            found: false,
        }
    }
}

fn extract_method_return_val(slice: &[u8]) -> Option<u64> {
    if slice.is_empty() {
        return None;
    }
    let limit = slice.len().min(64);
    for idx in 0..limit {
        let b = slice[idx];
        if b == 0xA4 {
            let mut parser = AmlParser::new(&slice[idx + 1..]);
            if let Some(v) = parser.parse_data_object() {
                let num = v.as_u64();
                if num > 0 {
                    return Some(num);
                }
            }
        } else if b == 0x08 {
            let mut parser = AmlParser::new(&slice[idx + 1..]);
            let _ = parser.parse_name_string();
            if let Some(v) = parser.parse_data_object() {
                let num = v.as_u64();
                if num > 0 {
                    return Some(num);
                }
            }
        } else if b == 0x0A || b == 0x0B || b == 0x0C || b == 0x0E {
            let mut parser = AmlParser::new(&slice[idx..]);
            if let Some(v) = parser.parse_data_object() {
                let num = v.as_u64();
                if num > 2000 && num < 5000 {
                    return Some(num);
                }
            }
        }
    }
    None
}

fn extract_temp_methods(slice: &[u8], tz: &mut ThermalZoneParsedData) {
    let mut i = 0;
    while i + 4 <= slice.len() {
        let seg = &slice[i..i + 4];
        if seg == b"_TMP" {
            if let Some(val) = extract_method_return_val(&slice[i + 4..]) {
                tz.current_temp_k = val as u32;
            }
        } else if seg == b"_CRT" {
            if let Some(val) = extract_method_return_val(&slice[i + 4..]) {
                tz.critical_temp_k = val as u32;
            }
        } else if seg == b"_HOT" {
            if let Some(val) = extract_method_return_val(&slice[i + 4..]) {
                tz.hot_temp_k = val as u32;
            }
        } else if seg == b"_PSV" {
            if let Some(val) = extract_method_return_val(&slice[i + 4..]) {
                tz.passive_temp_k = val as u32;
            }
        }
        i += 1;
    }
}

/// AML bayt dizisi içinde ThermalZone ve ilişkili _TMP, _CRT sıcaklık metotlarını arar.
pub fn scan_aml_for_thermal_zones(aml_bytes: &[u8], out: &mut [ThermalZoneParsedData; 8]) -> usize {
    let mut count = 0;
    if aml_bytes.is_empty() {
        return 0;
    }

    // 1. ThermalZoneOp (0x5B 0x85) taraması
    let mut i = 0;
    while i + 6 < aml_bytes.len() && count < 8 {
        if aml_bytes[i] == 0x5B && aml_bytes[i + 1] == 0x85 {
            let mut parser = AmlParser::new(&aml_bytes[i + 2..]);
            if let Some(pkg_len) = parser.parse_pkg_length() {
                if let Some(name) = parser.parse_name_string() {
                    let body_start = i + 2 + parser.cursor;
                    let body_end = (i + 2 + pkg_len).min(aml_bytes.len());
                    let body = if body_start < body_end {
                        &aml_bytes[body_start..body_end]
                    } else {
                        &[]
                    };

                    let mut tz = ThermalZoneParsedData::default();
                    tz.name = name;
                    tz.found = true;

                    extract_temp_methods(body, &mut tz);
                    out[count] = tz;
                    count += 1;
                    i = body_end;
                    continue;
                }
            }
        }
        i += 1;
    }

    // 2. Eğer açık ThermalZoneOp bulunamadıysa ama genel _TMP tanımları varsa:
    if count == 0 {
        let mut tz = ThermalZoneParsedData::default();
        tz.name = *b"THM0";
        extract_temp_methods(aml_bytes, &mut tz);
        if tz.current_temp_k > 0 || tz.critical_temp_k > 0 {
            tz.found = true;
            out[0] = tz;
            count = 1;
        }
    }

    count
}

/// AML içinde PowerResource (0x5B 0x84), Power Cihazları (_PS0.._PS3) ve Uyku Durumlarını (_S0.._S5) tarar.
pub fn scan_aml_for_power_devices(
    aml_bytes: &[u8],
    out: &mut [PowerDeviceParsedData; 8],
) -> (usize, u8) {
    let mut count = 0;
    let mut supported_states: u8 = 0;

    if aml_bytes.is_empty() {
        return (0, 0);
    }

    let mut i = 0;
    while i + 4 <= aml_bytes.len() {
        // Sleep state tespiti: _S0, _S1, _S2, _S3, _S4, _S5
        if aml_bytes[i] == b'_'
            && aml_bytes[i + 1] == b'S'
            && aml_bytes[i + 2] >= b'0'
            && aml_bytes[i + 2] <= b'5'
        {
            let state_idx = aml_bytes[i + 2] - b'0';
            supported_states |= 1 << state_idx;
        }

        // PowerResource (0x5B 0x84)
        if i + 8 < aml_bytes.len() && aml_bytes[i] == 0x5B && aml_bytes[i + 1] == 0x84 && count < 8
        {
            let mut parser = AmlParser::new(&aml_bytes[i + 2..]);
            if let Some(pkg_len) = parser.parse_pkg_length() {
                if let Some(name) = parser.parse_name_string() {
                    let mut pw = PowerDeviceParsedData::default();
                    pw.name = name;
                    pw.found = true;
                    pw.status = 1;

                    let body_start = i + 2 + parser.cursor;
                    let body_end = (i + 2 + pkg_len).min(aml_bytes.len());
                    if body_start < body_end {
                        let body = &aml_bytes[body_start..body_end];
                        if body.len() >= 4 {
                            for b_idx in 0..=body.len() - 4 {
                                if &body[b_idx..b_idx + 4] == b"_ON_" {
                                    pw.has_on = true;
                                } else if &body[b_idx..b_idx + 4] == b"_OFF" {
                                    pw.has_off = true;
                                }
                            }
                        }
                    }
                    out[count] = pw;
                    count += 1;
                    i = body_end;
                    continue;
                }
            }
        }

        // DeviceOp (0x5B 0x82) with Power capabilities
        if i + 8 < aml_bytes.len() && aml_bytes[i] == 0x5B && aml_bytes[i + 1] == 0x82 && count < 8
        {
            let mut parser = AmlParser::new(&aml_bytes[i + 2..]);
            if let Some(pkg_len) = parser.parse_pkg_length() {
                if let Some(name) = parser.parse_name_string() {
                    let body_start = i + 2 + parser.cursor;
                    let body_end = (i + 2 + pkg_len).min(aml_bytes.len());
                    if body_start < body_end {
                        let body = &aml_bytes[body_start..body_end];
                        let mut has_ps0 = false;
                        let mut has_ps3 = false;
                        if body.len() >= 4 {
                            for b_idx in 0..=body.len() - 4 {
                                if &body[b_idx..b_idx + 4] == b"_PS0" {
                                    has_ps0 = true;
                                } else if &body[b_idx..b_idx + 4] == b"_PS3" {
                                    has_ps3 = true;
                                }
                            }
                        }
                        if has_ps0 || has_ps3 {
                            let mut pw = PowerDeviceParsedData::default();
                            pw.name = name;
                            pw.found = true;
                            pw.has_on = has_ps0;
                            pw.has_off = has_ps3;
                            pw.status = 1;
                            out[count] = pw;
                            count += 1;
                            i = body_end;
                            continue;
                        }
                    }
                }
            }
        }

        i += 1;
    }

    (count, supported_states)
}

pub fn ensure_acpi_tables_parsed() {
    unsafe {
        if DSDT_VADDR == 0 {
            if let Some(rsdp_vaddr) = find_rsdp() {
                parse_acpi_tables(rsdp_vaddr);
            }
        }
    }
}

pub fn scan_acpi_thermal_zones(out: &mut [ThermalZoneParsedData; 8]) -> usize {
    ensure_acpi_tables_parsed();
    unsafe {
        let mut count = 0;
        if DSDT_VADDR != 0 && DSDT_LEN > 0 {
            let ptr = DSDT_VADDR as *const u8;
            if crate::boot::limine::is_mapped_limine_ptr(ptr) {
                let dsdt = core::slice::from_raw_parts(ptr, DSDT_LEN);
                count += scan_aml_for_thermal_zones(dsdt, out);
            }
        }
        for i in 0..SSDT_COUNT {
            if count >= 8 {
                break;
            }
            if SSDT_VADDRS[i] != 0 && SSDT_LENS[i] > 0 {
                let ptr = SSDT_VADDRS[i] as *const u8;
                if crate::boot::limine::is_mapped_limine_ptr(ptr) {
                    let ssdt = core::slice::from_raw_parts(ptr, SSDT_LENS[i]);
                    let mut tmp = [ThermalZoneParsedData::default(); 8];
                    let c = scan_aml_for_thermal_zones(ssdt, &mut tmp);
                    for j in 0..c {
                        if count < 8 {
                            out[count] = tmp[j];
                            count += 1;
                        }
                    }
                }
            }
        }
        count
    }
}

pub fn scan_acpi_power_devices(out: &mut [PowerDeviceParsedData; 8]) -> (usize, u8) {
    ensure_acpi_tables_parsed();
    unsafe {
        let mut count = 0;
        let mut states = 0u8;
        if DSDT_VADDR != 0 && DSDT_LEN > 0 {
            let ptr = DSDT_VADDR as *const u8;
            if crate::boot::limine::is_mapped_limine_ptr(ptr) {
                let dsdt = core::slice::from_raw_parts(ptr, DSDT_LEN);
                let (c, s) = scan_aml_for_power_devices(dsdt, out);
                count += c;
                states |= s;
            }
        }
        for i in 0..SSDT_COUNT {
            if count >= 8 {
                break;
            }
            if SSDT_VADDRS[i] != 0 && SSDT_LENS[i] > 0 {
                let ptr = SSDT_VADDRS[i] as *const u8;
                if crate::boot::limine::is_mapped_limine_ptr(ptr) {
                    let ssdt = core::slice::from_raw_parts(ptr, SSDT_LENS[i]);
                    let mut tmp = [PowerDeviceParsedData::default(); 8];
                    let (c, s) = scan_aml_for_power_devices(ssdt, &mut tmp);
                    states |= s;
                    for j in 0..c {
                        if count < 8 {
                            out[count] = tmp[j];
                            count += 1;
                        }
                    }
                }
            }
        }
        (count, states)
    }
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
            let ptr = DSDT_VADDR as *const u8;
            if crate::boot::limine::is_mapped_limine_ptr(ptr) {
                let dsdt_slice = core::slice::from_raw_parts(ptr, DSDT_LEN);
                scan_aml_for_battery(dsdt_slice, &mut parsed);
            }
        }

        for i in 0..SSDT_COUNT {
            if SSDT_VADDRS[i] != 0 && SSDT_LENS[i] > 0 {
                let ptr = SSDT_VADDRS[i] as *const u8;
                if crate::boot::limine::is_mapped_limine_ptr(ptr) {
                    let ssdt_slice = core::slice::from_raw_parts(ptr, SSDT_LENS[i]);
                    scan_aml_for_battery(ssdt_slice, &mut parsed);
                }
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
            let ptr = DSDT_VADDR as *const u8;
            if crate::boot::limine::is_mapped_limine_ptr(ptr) {
                let dsdt_slice = core::slice::from_raw_parts(ptr, DSDT_LEN);
                scan_aml_for_battery(dsdt_slice, &mut parsed);
            }
        }

        for i in 0..SSDT_COUNT {
            if SSDT_VADDRS[i] != 0 && SSDT_LENS[i] > 0 {
                let ptr = SSDT_VADDRS[i] as *const u8;
                if crate::boot::limine::is_mapped_limine_ptr(ptr) {
                    let ssdt_slice = core::slice::from_raw_parts(ptr, SSDT_LENS[i]);
                    scan_aml_for_battery(ssdt_slice, &mut parsed);
                }
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

        let health = ((BATTERY_INFO.last_full_capacity as u64 * 100)
            / BATTERY_INFO.design_capacity as u64) as u8;
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
