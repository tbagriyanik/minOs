#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    pub const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    pub fn set_handler(&mut self, handler: usize) {
        self.set_handler_dpl(handler, 0);
    }

    /// Kesme kapısını kurar; `dpl` = kapının çağrılabileceği en düşük halka.
    /// `int $0x80` için DPL=3 gerekir (Ring3'ten çağrılabilir).
    pub fn set_handler_dpl(&mut self, handler: usize, dpl: u8) {
        self.set_handler_ist(handler, dpl, 0);
    }

    pub fn set_handler_ist(&mut self, handler: usize, dpl: u8, ist: u8) {
        self.selector = 0x08; // Çekirdek kod segmenti
        self.type_attr = 0x8E | ((dpl & 3) << 5); // 64-bit Interrupt Gate, Present
        self.offset_low = (handler & 0xFFFF) as u16;
        self.offset_mid = ((handler >> 16) & 0xFFFF) as u16;
        self.offset_high = ((handler >> 32) & 0xFFFFFFFF) as u32;
        self.ist = ist & 7;
        self.zero = 0;
    }
}

#[repr(C, packed)]
pub struct IdtPointer {
    pub limit: u16,
    pub base: u64,
}

#[repr(C, align(16))]
pub struct InterruptDescriptorTable {
    pub entries: [IdtEntry; 256],
}

impl InterruptDescriptorTable {
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::missing(); 256],
        }
    }

    pub fn load(&'static self) {
        let ptr = IdtPointer {
            limit: (core::mem::size_of::<InterruptDescriptorTable>() - 1) as u16,
            base: self as *const _ as u64,
        };
        unsafe {
            core::arch::asm!("lidt [{}]", in(reg) &ptr, options(readonly, nostack, preserves_flags));
        }
    }
}
