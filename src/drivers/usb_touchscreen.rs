#![allow(dead_code, static_mut_refs)]

//! USB HID dokunmatik ekran / digitizer sürücüsü.
//!
//! Gerçek USB HID Digitizer (Usage Page 0x0D) arayüzlerini destekler:
//! - Rapor tanımlayıcısından (Report Descriptor) X/Y, Tip Switch, Contact ID,
//!   Contact Count ve basınç alanları çıkarılır.
//! - Tek dokunuşlu ekranların yanı sıra iki yaygın çoklu dokunma düzeni
//!   desteklenir:
//!   1. Tek raporda Contact Count + tekrarlanan kişi (contact) blokları,
//!   2. Kişi başına ayrı Report ID (her raporda tek kişi).
//! - Raporlar mevcut xHCI HID kesme (Interrupt IN) hattı üzerinden
//!   `usb_hid::poll_input` tarafından bu modüle iletilir.
//!
//! Donanım tespiti HID rapor tanımlayıcısına dayanır; bu yüzden farklı
//! üreticilerin (Wacom, Elan, Goodix, eGalax, Windows Precision Touchpad…)
//! standart HID digitizer uygulamaları jenerik olarak çalışır.

// ---------------------------------------------------------------------------
// Sınırlar
// ---------------------------------------------------------------------------

/// Aynı anda izlenen azami dokunma noktası.
pub const MAX_CONTACTS: usize = 10;

// ---------------------------------------------------------------------------
// Rapor tanımlayıcısından çıkarılan alan yerleşimi
// ---------------------------------------------------------------------------

/// Bir rapor alanının bit konumu ve genişliği.
#[derive(Clone, Copy, Default)]
pub struct TouchField {
    pub start: u16,
    pub size: u8,
    pub present: bool,
}

impl TouchField {
    const fn empty() -> Self {
        Self {
            start: 0,
            size: 0,
            present: false,
        }
    }
}

/// Tek bir kişi (contact) için alan konumları.
#[derive(Clone, Copy)]
pub struct ContactLayout {
    /// Alanların ait olduğu Report ID (0 = raporlarda ID yok).
    pub report_id: u8,
    pub x: TouchField,
    pub y: TouchField,
    pub tip: TouchField,
    pub contact_id: TouchField,
    pub pressure: TouchField,
    pub x_absolute: bool,
    pub y_absolute: bool,
}

impl ContactLayout {
    const fn empty() -> Self {
        Self {
            report_id: 0,
            x: TouchField::empty(),
            y: TouchField::empty(),
            tip: TouchField::empty(),
            contact_id: TouchField::empty(),
            pressure: TouchField::empty(),
            x_absolute: false,
            y_absolute: false,
        }
    }

    pub const fn used(&self) -> bool {
        self.x.present || self.y.present || self.tip.present || self.contact_id.present
    }
}

/// Ayrıştırılmış dokunmatik ekran rapor düzeni.
#[derive(Clone, Copy)]
pub struct TouchLayout {
    pub contacts: [ContactLayout; MAX_CONTACTS],
    /// Rapor içindeki toplam aktif kişi sayısı alanı.
    pub contact_count: TouchField,
    pub contact_count_report_id: u8,
    pub max_x: u32,
    pub max_y: u32,
    /// En az bir kişi bloğu ayrıştırıldı mı?
    pub digitizer: bool,
    /// Birden çok dokunma noktası destekleniyor mu?
    pub multi: bool,
    /// Her kişi ayrı Report ID kullanıyor mu?
    pub per_report_id: bool,
    /// Raporlarda (paylaşılan veya kişi başına) bir Report ID öneki var mı?
    pub has_report_id: bool,
}

impl TouchLayout {
    pub const fn empty() -> Self {
        Self {
            contacts: [ContactLayout::empty(); MAX_CONTACTS],
            contact_count: TouchField::empty(),
            contact_count_report_id: 0,
            max_x: 0,
            max_y: 0,
            digitizer: false,
            multi: false,
            per_report_id: false,
            has_report_id: false,
        }
    }

    pub const fn is_digitizer(&self) -> bool {
        self.digitizer
    }

    pub const fn is_multitouch(&self) -> bool {
        self.multi
    }

    pub const fn uses_report_ids(&self) -> bool {
        self.per_report_id
    }
}

// ---------------------------------------------------------------------------
// Sürücü durumu
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TouchscreenState {
    NotDetected,
    Detected,
    Initializing,
    Ready,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub struct TouchEvent {
    /// Kişi kimliği (Contact ID; yoksa rapor sırası/Report ID).
    pub id: u8,
    pub x: u16,
    pub y: u16,
    pub pressure: u8,
    pub contact: bool,
}

impl TouchEvent {
    const fn empty() -> Self {
        Self {
            id: 0,
            x: 0,
            y: 0,
            pressure: 0,
            contact: false,
        }
    }
}

static mut STATE: TouchscreenState = TouchscreenState::NotDetected;
static mut DEVICE_FOUND: bool = false;
static mut MAX_X: u16 = 0;
static mut MAX_Y: u16 = 0;
static mut MULTITOUCH: bool = false;

/// Güncel aktif dokunma noktaları (polling ile okunur).
static mut TOUCHES: [TouchEvent; MAX_CONTACTS] = [TouchEvent::empty(); MAX_CONTACTS];
static mut TOUCH_COUNT: usize = 0;
static mut CURRENT_TOUCH: TouchEvent = TouchEvent::empty();

// ---------------------------------------------------------------------------
// Rapor tanımlayıcısı ayrıştırma
// ---------------------------------------------------------------------------

#[inline]
fn usage_id(usage: u32) -> u32 {
    usage & 0xFFFF
}

#[inline]
fn usage_page_of(usage: u32, global_page: u16) -> u16 {
    let high = (usage >> 16) as u16;
    if high != 0 {
        high
    } else {
        global_page
    }
}

/// İlk kullanılmayan kişi yuvasını döndürür.
fn alloc_contact(layout: &mut TouchLayout, report_id: u8) -> Option<usize> {
    for (i, contact) in layout.contacts.iter().enumerate() {
        if !contact.used() {
            layout.contacts[i].report_id = report_id;
            return Some(i);
        }
    }
    None
}

/// Aynı alanın `Report Count = N` ile tekrarlandığı düzenlerde (ör. Windows
/// Precision Touchpad: Tip Switch[5], Contact ID[5], X[5], Y[5]) her tekrar
/// ayrı bir kişiye karşılık gelir. `base`, o Report ID'nin ilk kişi yuvasıdır.
fn indexed_contact(
    layout: &mut TouchLayout,
    report_id: u8,
    index: usize,
    base: &mut [i16; 32],
    id_index: usize,
) -> Option<usize> {
    if base[id_index] < 0 {
        let start = alloc_contact(layout, report_id)?;
        base[id_index] = start as i16;
    }
    let slot = base[id_index] as usize + index;
    if slot >= MAX_CONTACTS {
        return None;
    }
    if !layout.contacts[slot].used() {
        layout.contacts[slot].report_id = report_id;
    }
    Some(slot)
}

/// HID rapor tanımlayıcısını ayrıştırarak digitizer alan yerleşimini çıkarır.
///
/// Desteklenen öğeler: Usage Page, Usage, Usage Minimum/Maximum, Logical
/// Maximum, Report ID, Report Size/Count, Collection/End Collection, Input.
pub fn parse_report_descriptor(desc: &[u8]) -> TouchLayout {
    let mut layout = TouchLayout::empty();
    let mut page: u16 = 0;
    let mut usage: u32 = 0;
    let mut usages = [0u32; 16];
    let mut usage_count = 0usize;
    let mut usage_min: u32 = 0;
    let mut usage_max: u32 = 0;
    let mut logical_max: i32 = 0;
    let mut report_size: u32 = 0;
    let mut report_count: u32 = 0;
    let mut report_id: u8 = 0;
    // Her Report ID'nin kendi bit akışı vardır.
    let mut report_bits = [0u32; 32];
    // Report ID başına, çoklu tekrarlı alanların ilk kişi yuvası.
    let mut contact_base = [-1i16; 32];
    let mut cur_contact: Option<usize> = None;
    let mut depth: u32 = 0;
    let mut app_page: u16 = 0;
    let mut app_usage: u32 = 0;
    let mut i = 0usize;

    while i < desc.len() {
        let prefix = desc[i];
        i += 1;
        if prefix == 0xFE {
            if i + 1 >= desc.len() {
                break;
            }
            let len = desc[i] as usize;
            i += 2 + len;
            continue;
        }
        let b_size = (prefix & 0x03) as usize;
        let item_type = (prefix >> 2) & 0x03;
        let tag = (prefix >> 4) & 0x0F;
        if i + b_size > desc.len() {
            break;
        }
        let mut value = 0u32;
        for k in 0..b_size {
            value |= (desc[i + k] as u32) << (8 * k);
        }
        i += b_size;
        let signed = match b_size {
            1 => value as u8 as i8 as i32,
            2 => value as u16 as i16 as i32,
            4 => value as i32,
            _ => 0,
        };

        match item_type {
            // ---- Global öğeler ----
            0x01 => match tag {
                0x00 => page = value as u16,
                0x01 => {}
                0x02 => logical_max = signed,
                0x07 => report_size = value & 0xFFFF,
                0x08 => {
                    let id = value as u8;
                    if id != report_id {
                        // Yeni rapor kimliği: kişi bloğu da yeniden başlar.
                        cur_contact = None;
                    }
                    report_id = id;
                }
                0x09 => report_count = value & 0xFFFF,
                _ => {}
            },
            // ---- Local öğeler ----
            0x02 => match tag {
                0x00 => {
                    if usage_count == 0 {
                        usage = value;
                    }
                    if usage_count < usages.len() {
                        usages[usage_count] = value;
                        usage_count += 1;
                    }
                }
                0x01 => usage_min = value,
                0x02 => usage_max = value,
                _ => {}
            },
            // ---- Main öğeler ----
            0x00 => match tag {
                0x08 => {
                    let is_data = value & 0x01 == 0;
                    let is_abs = value & 0x04 == 0;
                    let bits = report_count.saturating_mul(report_size);
                    if is_data && bits > 0 {
                        let id_index = (report_id as usize).min(report_bits.len() - 1);
                        let start_bits = report_bits[id_index];
                        let fields = report_count.min(64);

                        for k in 0..fields {
                            let index = k as usize;
                            let field_usage = if index < usage_count {
                                usages[index]
                            } else if usage_count > 0 {
                                usages[usage_count - 1]
                            } else {
                                0
                            };
                            let field_page = usage_page_of(field_usage, page);
                            let uid = usage_id(field_usage);
                            let field_bit =
                                start_bits.saturating_add(k.saturating_mul(report_size));
                            if field_bit > u16::MAX as u32 {
                                continue;
                            }

                            // Bu alan bir digitizer alanı mı?
                            let digitizer_page = field_page == 0x0D;
                            let generic_xy = field_page == 0x01 && (uid == 0x30 || uid == 0x31);
                            if !digitizer_page && !generic_xy {
                                continue;
                            }

                            // Contact Count kişiye bağlı değildir.
                            if digitizer_page && uid == 0x54 {
                                if !layout.contact_count.present {
                                    layout.contact_count = TouchField {
                                        start: field_bit as u16,
                                        size: report_size.min(32) as u8,
                                        present: true,
                                    };
                                    layout.contact_count_report_id = report_id;
                                }
                                continue;
                            }

                            let id_index = (report_id as usize).min(contact_base.len() - 1);
                            // Tek kullanım + Report Count > 1: her tekrar bir kişi.
                            let indexed = usage_count == 1
                                && report_count > 1
                                && (digitizer_page || generic_xy)
                                && uid != 0x54;
                            let indexed_slot = if indexed {
                                indexed_contact(
                                    &mut layout,
                                    report_id,
                                    k as usize,
                                    &mut contact_base,
                                    id_index,
                                )
                            } else {
                                None
                            };

                            // Contact Identifier yeni bir kişi bloğu başlatır.
                            if digitizer_page && uid == 0x51 {
                                let slot = match indexed_slot {
                                    Some(s) => Some(s),
                                    None => match cur_contact {
                                        Some(s)
                                            if layout.contacts[s].contact_id.present
                                                && layout.contacts[s].x.present =>
                                        {
                                            alloc_contact(&mut layout, report_id)
                                        }
                                        Some(s) => Some(s),
                                        None => alloc_contact(&mut layout, report_id),
                                    },
                                };
                                if let Some(slot) = slot {
                                    if slot < MAX_CONTACTS {
                                        if !layout.contacts[slot].contact_id.present {
                                            layout.contacts[slot].report_id = report_id;
                                            layout.contacts[slot].contact_id = TouchField {
                                                start: field_bit as u16,
                                                size: report_size.min(32) as u8,
                                                present: true,
                                            };
                                        }
                                        cur_contact = Some(slot);
                                    }
                                }
                                continue;
                            }

                            // Diğer alanlar mevcut kişiye (yoksa yeni kişiye) yazılır.
                            let slot = match indexed_slot {
                                Some(s) => s,
                                None => match cur_contact {
                                    Some(s) => s,
                                    None => match alloc_contact(&mut layout, report_id) {
                                        Some(s) => {
                                            cur_contact = Some(s);
                                            s
                                        }
                                        None => continue,
                                    },
                                },
                            };
                            let contact = &mut layout.contacts[slot];

                            match (digitizer_page, uid) {
                                (true, 0x30) | (false, 0x30) if !contact.x.present => {
                                    contact.x = TouchField {
                                        start: field_bit as u16,
                                        size: report_size.min(32) as u8,
                                        present: true,
                                    };
                                    contact.x_absolute = is_abs;
                                    if logical_max > 0 {
                                        layout.max_x = logical_max as u32;
                                    }
                                }
                                (true, 0x31) | (false, 0x31) if !contact.y.present => {
                                    contact.y = TouchField {
                                        start: field_bit as u16,
                                        size: report_size.min(32) as u8,
                                        present: true,
                                    };
                                    contact.y_absolute = is_abs;
                                    if logical_max > 0 {
                                        layout.max_y = logical_max as u32;
                                    }
                                }
                                (true, 0x42) if !contact.tip.present => {
                                    contact.tip = TouchField {
                                        start: field_bit as u16,
                                        size: report_size.min(32) as u8,
                                        present: true,
                                    };
                                }
                                (true, 0x33) | (true, 0x32) if !contact.pressure.present => {
                                    contact.pressure = TouchField {
                                        start: field_bit as u16,
                                        size: report_size.min(32) as u8,
                                        present: true,
                                    };
                                }
                                _ => {}
                            }
                        }

                        report_bits[id_index] = start_bits.saturating_add(bits);
                    } else {
                        let id_index = (report_id as usize).min(report_bits.len() - 1);
                        report_bits[id_index] = report_bits[id_index].saturating_add(bits);
                    }
                }
                0x0B => {
                    if value == 1 && depth == 0 {
                        app_page = page;
                        app_usage = usage;
                    }
                    depth = depth.saturating_add(1);
                }
                0x0C => depth = depth.saturating_sub(1),
                _ => {}
            },
            _ => {}
        }

        // Local kullanım değerleri her Main öğesinden sonra sıfırlanır.
        if item_type == 0x00 {
            usage = 0;
            usage_count = 0;
            usage_min = 0;
            usage_max = 0;
        }
    }

    let _ = (app_usage, usage_min, usage_max);

    // Kullanılan kişi sayısı ve çoklu dokunma tespiti.
    let mut used = 0usize;
    let mut contact_ids = 0usize;
    let mut first_report_id = None;
    let mut per_report_id = false;
    for contact in layout.contacts.iter() {
        if contact.used() {
            used += 1;
            if contact.contact_id.present {
                contact_ids += 1;
            }
            match first_report_id {
                None => first_report_id = Some(contact.report_id),
                Some(id) if id != contact.report_id => per_report_id = true,
                _ => {}
            }
        }
    }

    // Uygulama koleksiyonu Digitizer sayfasındaysa ve X/Y varsa digitizer'dır.
    let app_digitizer =
        app_page == 0x0D && matches!(usage_id(app_usage), 0x02 | 0x04 | 0x05 | 0x22);
    let has_xy = layout.contacts.iter().any(|c| c.x.present && c.y.present);

    layout.digitizer = (app_digitizer || used > 0) && has_xy;
    layout.per_report_id = per_report_id;
    layout.has_report_id = matches!(first_report_id, Some(id) if id != 0);
    layout.multi = layout.contact_count.present || used > 1 || contact_ids > 1 || per_report_id;

    if layout.max_x == 0 && has_xy {
        layout.max_x = 0xFFFF;
    }
    if layout.max_y == 0 && has_xy {
        layout.max_y = 0xFFFF;
    }
    layout
}

// ---------------------------------------------------------------------------
// Rapor işleme
// ---------------------------------------------------------------------------

#[inline]
fn get_bits(data: &[u8], start: u16, size: u8) -> u32 {
    let mut v = 0u32;
    let bits = size.min(32) as usize;
    for k in 0..bits {
        let idx = start as usize + k;
        if idx / 8 >= data.len() {
            break;
        }
        let bitv = (data[idx / 8] >> (idx % 8)) & 1;
        v |= (bitv as u32) << k;
    }
    v
}

#[inline]
fn field_value(data: &[u8], field: &TouchField) -> u32 {
    if field.present {
        get_bits(data, field.start, field.size)
    } else {
        0
    }
}

#[inline]
fn field_active(data: &[u8], field: &TouchField) -> bool {
    !field.present || (get_bits(data, field.start, field.size) & 1) != 0
}

fn scale_pressure(value: u32, size: u8) -> u8 {
    let bits = size.min(32);
    if bits == 0 {
        return 0;
    }
    if bits <= 8 {
        return value as u8;
    }
    let max = if bits >= 32 {
        u32::MAX as u64
    } else {
        (1u64 << bits) - 1
    };
    ((value as u64 * 255) / max.max(1)).min(255) as u8
}

/// Rapor gövdesinden tek bir kişiyi okur.
fn read_contact(
    data: &[u8],
    layout: &TouchLayout,
    index: usize,
    count: Option<u32>,
) -> Option<TouchEvent> {
    let contact = layout.contacts.get(index)?;
    if !contact.x.present || !contact.y.present {
        return None;
    }

    let id_value = field_value(data, &contact.contact_id);
    let id = if contact.contact_id.present {
        (id_value & 0xFF) as u8
    } else if layout.per_report_id {
        contact.report_id
    } else {
        index as u8
    };

    let active = if contact.tip.present {
        field_active(data, &contact.tip)
    } else if let Some(total) = count {
        (index as u32) < total
    } else {
        true
    };

    let x = field_value(data, &contact.x).min(u16::MAX as u32) as u16;
    let y = field_value(data, &contact.y).min(u16::MAX as u32) as u16;
    let pressure = scale_pressure(field_value(data, &contact.pressure), contact.pressure.size);

    Some(TouchEvent {
        id,
        x,
        y,
        pressure,
        contact: active,
    })
}

fn remove_touch(id: u8) {
    unsafe {
        let mut kept = [TouchEvent::empty(); MAX_CONTACTS];
        let mut out = 0usize;
        for t in TOUCHES[..TOUCH_COUNT].iter() {
            if t.id != id {
                kept[out] = *t;
                out += 1;
            }
        }
        core::ptr::copy_nonoverlapping(
            kept.as_ptr(),
            core::ptr::addr_of_mut!(TOUCHES) as *mut TouchEvent,
            out,
        );
        TOUCH_COUNT = out;
        CURRENT_TOUCH = if out > 0 {
            kept[0]
        } else {
            TouchEvent::empty()
        };
    }
}

fn upsert_touch(event: TouchEvent) {
    unsafe {
        if !event.contact {
            remove_touch(event.id);
            return;
        }
        for t in TOUCHES[..TOUCH_COUNT].iter_mut() {
            if t.id == event.id {
                *t = event;
                CURRENT_TOUCH = TOUCHES[0];
                return;
            }
        }
        if TOUCH_COUNT < MAX_CONTACTS {
            TOUCHES[TOUCH_COUNT] = event;
            TOUCH_COUNT += 1;
        }
        CURRENT_TOUCH = TOUCHES[0];
    }
}

fn replace_touches(events: &[TouchEvent]) {
    unsafe {
        let mut count = 0usize;
        for event in events {
            if event.contact && count < MAX_CONTACTS {
                TOUCHES[count] = *event;
                count += 1;
            }
        }
        TOUCH_COUNT = count;
        CURRENT_TOUCH = if count > 0 {
            TOUCHES[0]
        } else {
            TouchEvent::empty()
        };
    }
}

/// Rapor tanımlayıcısına göre gelen HID raporunu işler.
///
/// Dönüş: en az bir dokunma noktası güncellendiyse `true`.
pub fn process_report(data: &[u8], layout: &TouchLayout) -> bool {
    if !layout.digitizer || data.is_empty() {
        return false;
    }

    // Sıcak takılan bir ekran önyükleme sonrası ilk raporunu gönderdiğinde
    // sürücüyü de hazır duruma getir.
    unsafe {
        DEVICE_FOUND = true;
        if STATE != TouchscreenState::Ready {
            STATE = TouchscreenState::Ready;
        }
        if MAX_X == 0 {
            MAX_X = layout.max_x.min(u16::MAX as u32) as u16;
        }
        if MAX_Y == 0 {
            MAX_Y = layout.max_y.min(u16::MAX as u32) as u16;
        }
        MULTITOUCH = layout.is_multitouch();
    }

    let has_prefix = layout.has_report_id && data[0] != 0;
    let (report_id, body) = if has_prefix {
        (data[0], &data[1..])
    } else {
        (0u8, data)
    };
    if body.is_empty() {
        return false;
    }

    // Contact Count yalnızca kendi Report ID'sinde geçerlidir.
    let count = if layout.contact_count.present
        && (!layout.has_report_id || layout.contact_count_report_id == report_id)
    {
        Some(field_value(body, &layout.contact_count))
    } else {
        None
    };

    if layout.per_report_id {
        // Her rapor tek bir kişiyi taşır.
        let mut found = false;
        for i in 0..MAX_CONTACTS {
            let contact = layout.contacts[i];
            if !contact.used() || contact.report_id != report_id {
                continue;
            }
            if let Some(event) = read_contact(body, layout, i, count) {
                upsert_touch(event);
                found = true;
            }
        }
        found
    } else {
        // Tek rapor tüm kişileri taşır; durum tamamen yeniden kurulur.
        let mut events = [TouchEvent::empty(); MAX_CONTACTS];
        let mut event_count = 0usize;
        for i in 0..MAX_CONTACTS {
            let contact = layout.contacts[i];
            if !contact.used() {
                continue;
            }
            if let Some(event) = read_contact(body, layout, i, count) {
                if event.contact && event_count < MAX_CONTACTS {
                    events[event_count] = event;
                    event_count += 1;
                }
            }
        }
        replace_touches(&events[..event_count]);
        true
    }
}

// ---------------------------------------------------------------------------
// Genel API
// ---------------------------------------------------------------------------

/// Bağlı bir USB HID digitizer var mı?
pub fn detect() -> bool {
    let found = crate::drivers::usb_hid::touchscreen_present();
    unsafe {
        DEVICE_FOUND = found;
        if found && STATE == TouchscreenState::NotDetected {
            STATE = TouchscreenState::Detected;
        }
    }
    found
}

/// Algılanan dokunmatik ekranı başlatır ve eksen aralıklarını okur.
pub fn init() -> bool {
    if !detect() {
        unsafe {
            STATE = TouchscreenState::NotDetected;
        }
        return false;
    }

    unsafe {
        STATE = TouchscreenState::Initializing;
    }

    let Some((max_x, max_y, multi)) = crate::drivers::usb_hid::touchscreen_info() else {
        unsafe {
            STATE = TouchscreenState::Error;
        }
        return false;
    };

    unsafe {
        MAX_X = max_x.min(u16::MAX as u32) as u16;
        MAX_Y = max_y.min(u16::MAX as u32) as u16;
        MULTITOUCH = multi;
        TOUCH_COUNT = 0;
        CURRENT_TOUCH = TouchEvent::empty();
        STATE = TouchscreenState::Ready;
    }
    true
}

pub fn is_detected() -> bool {
    unsafe { DEVICE_FOUND }
}

pub fn state() -> TouchscreenState {
    unsafe { STATE }
}

pub fn name() -> &'static str {
    "USB Touchscreen"
}

pub fn name_tr() -> &'static str {
    "USB Dokunmatik Ekran"
}

pub fn max_x() -> u16 {
    unsafe { MAX_X }
}

pub fn max_y() -> u16 {
    unsafe { MAX_Y }
}

/// İlk aktif dokunma noktasını döndürür.
pub fn read_touch() -> Option<TouchEvent> {
    unsafe {
        if STATE != TouchscreenState::Ready || TOUCH_COUNT == 0 {
            return None;
        }
        Some(TOUCHES[0])
    }
}

/// Mevcut dokunma durumunu döndürür (yoksa `contact = false`).
pub fn get_current_touch() -> TouchEvent {
    unsafe { CURRENT_TOUCH }
}

/// Ekran koordinatlarını 0.0 - 1.0 aralığına normalize eder.
pub fn normalize_coords(x: u16, y: u16) -> (f32, f32) {
    unsafe {
        if MAX_X == 0 || MAX_Y == 0 {
            return (0.0, 0.0);
        }
        (x as f32 / MAX_X as f32, y as f32 / MAX_Y as f32)
    }
}

/// Aygıt çoklu dokunma bildiriyor mu?
pub fn supports_multitouch() -> bool {
    unsafe { DEVICE_FOUND && MULTITOUCH }
}

/// Tüm aktif dokunma noktalarını döndürür.
pub fn read_all_touches() -> &'static [TouchEvent] {
    unsafe {
        if STATE != TouchscreenState::Ready {
            return &[];
        }
        &TOUCHES[..TOUCH_COUNT]
    }
}

/// Dokunmatik ekran kalibrasyonu.
///
/// HID digitizer'lar genellikle mutlak ve önceden kalibre edilmiş koordinatlar
/// bildirir; bu yüzden dört noktalı kalibrasyon şimdilik yalnızca aralıkları
/// doğrular. Gerekirse üst katman bir afin dönüşüm uygulayabilir.
pub fn calibrate(points: &[(u16, u16); 4]) -> bool {
    unsafe {
        if STATE != TouchscreenState::Ready {
            return false;
        }
        let mut min_x = u16::MAX;
        let mut max_x = 0u16;
        let mut min_y = u16::MAX;
        let mut max_y = 0u16;
        for &(x, y) in points {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        max_x > min_x && max_y > min_y
    }
}

/// Sensör hassasiyetini ayarlar.
///
/// HID digitizer'larda hassasiyet standart bir Feature raporu değildir;
/// aygıta özel Feature raporları gerekir. Bu sürüm yalnızca durumu doğrular.
pub fn set_sensitivity(sensitivity: u8) -> bool {
    unsafe { STATE == TouchscreenState::Ready && sensitivity > 0 }
}
