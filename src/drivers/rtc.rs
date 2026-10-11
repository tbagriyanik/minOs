//! Gerçek Zamanlı Saat (CMOS / RTC) Sürücüsü
//!
//! QEMU, VirtualBox ve gerçek donanımda 0x70 / 0x71 G/Ç portları üzerinden
//! (Motorola MC146818 uyumlu) saat ve tarih okunur. Takvim hesaplamaları
//! (artık yıl, ay uzunluğu, haftanın günü) bu modül içinde sağlanır.

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack, preserves_flags));
    val
}

/// CMOS yazmacını okur (NMI'yı devre dışı bırakarak).
unsafe fn cmos_read(reg: u8) -> u8 {
    outb(0x70, reg | 0x80);
    inb(0x71)
}

/// Güncelleme sürerken (UIP) bekler.
fn wait_not_updating() {
    for _ in 0..1_000_000 {
        if unsafe { cmos_read(0x0A) } & 0x80 == 0 {
            return;
        }
    }
}

fn bcd_to_bin(v: u8) -> u8 {
    (v & 0x0F) + ((v >> 4) * 10)
}

/// Tam tarih/saat bilgisi.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,  // 1..=12
    pub day: u8,    // 1..=31
    pub hour: u8,   // 0..=23
    pub minute: u8, // 0..=59
    pub second: u8, // 0..=59
}

impl DateTime {
    /// RTC zamanını seçilen saat dilimi farkına göre takvim taşmalarını koruyarak kaydırır.
    pub fn with_hour_offset(mut self, offset: i8) -> Self {
        let mut hour = self.hour as i16 + offset as i16;
        while hour < 0 {
            hour += 24;
            if self.day > 1 { self.day -= 1; }
            else if self.month > 1 { self.month -= 1; self.day = Self::days_in_month(self.year, self.month); }
            else { self.year = self.year.saturating_sub(1); self.month = 12; self.day = 31; }
        }
        while hour >= 24 {
            hour -= 24;
            if self.day < Self::days_in_month(self.year, self.month) { self.day += 1; }
            else if self.month < 12 { self.month += 1; self.day = 1; }
            else { self.year = self.year.saturating_add(1); self.month = 1; self.day = 1; }
        }
        self.hour = hour as u8;
        self
    }

    /// CMOS RTC'ten anlık zamanı okur ve ikili (binary) forma çevirir.
    pub fn read() -> Self {
        wait_not_updating();
        let mut second = unsafe { cmos_read(0x00) };
        let mut minute = unsafe { cmos_read(0x02) };
        let hour = unsafe { cmos_read(0x04) };
        let mut day = unsafe { cmos_read(0x07) };
        let mut month = unsafe { cmos_read(0x08) };
        let year = unsafe { cmos_read(0x09) };
        let century = unsafe { cmos_read(0x32) };
        let status_b = unsafe { cmos_read(0x0B) };

        let bin = status_b & 0x04 != 0; // 0 = BCD, 1 = binary
        let is_24h = status_b & 0x02 != 0;

        if !bin {
            second = bcd_to_bin(second);
            minute = bcd_to_bin(minute);
            day = bcd_to_bin(day);
            month = bcd_to_bin(month);
        }

        // Saat formatı (12 saat ise PM bitini uygula)
        let mut hour_bin = if bin { hour } else { bcd_to_bin(hour & 0x7F) };
        if !is_24h {
            let pm = hour & 0x80 != 0;
            if pm && hour_bin < 12 {
                hour_bin += 12;
            } else if !pm && hour_bin == 12 {
                hour_bin = 0;
            }
        }

        let year_bin = if bin { year } else { bcd_to_bin(year) };
        let century_bin = if bin { century } else { bcd_to_bin(century) };
        let full_year = if (20..=99).contains(&century_bin) {
            (century_bin as u16) * 100 + year_bin as u16
        } else if year_bin >= 70 {
            1900 + year_bin as u16
        } else {
            2000 + year_bin as u16
        };

        Self {
            year: full_year,
            month: month.clamp(1, 12),
            day: day.clamp(1, 31),
            hour: hour_bin.min(23),
            minute: minute.min(59),
            second: second.min(59),
        }
    }

    pub fn is_leap_year(year: u16) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    pub fn days_in_month(year: u16, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if Self::is_leap_year(year) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }

    /// Haftanın günü: 0 = Pazartesi ... 6 = Pazar.
    pub fn weekday(year: u16, month: u8, day: u8) -> u8 {
        // Sakamoto algoritması (0 = Pazar döndürür)
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let mut y = year as i32;
        if month < 3 {
            y -= 1;
        }
        let w = (y + y / 4 - y / 100 + y / 400 + T[(month - 1) as usize] + day as i32) % 7;
        // Pazar(0) -> 6, Pazartesi(1) -> 0 ... olacak şekilde dönüştür
        ((w + 6) % 7) as u8
    }

    /// "Pazartesi" gibi tam gün adı (0 = Pazartesi).
    pub fn weekday_name(w: u8) -> &'static str {
        match w {
            0 => "Pazartesi",
            1 => "Salı",
            2 => "Çarşamba",
            3 => "Perşembe",
            4 => "Cuma",
            5 => "Cumartesi",
            _ => "Pazar",
        }
    }

    pub fn weekday_name_en(w: u8) -> &'static str {
        match w { 0 => "Monday", 1 => "Tuesday", 2 => "Wednesday", 3 => "Thursday", 4 => "Friday", 5 => "Saturday", _ => "Sunday" }
    }

    /// "Ocak" gibi tam ay adı (1..=12).
    pub fn month_name(month: u8) -> &'static str {
        match month {
            1 => "Ocak",
            2 => "Şubat",
            3 => "Mart",
            4 => "Nisan",
            5 => "Mayıs",
            6 => "Haziran",
            7 => "Temmuz",
            8 => "Ağustos",
            9 => "Eylül",
            10 => "Ekim",
            11 => "Kasım",
            _ => "Aralık",
        }
    }

    pub fn month_name_en(month: u8) -> &'static str {
        match month { 1 => "January", 2 => "February", 3 => "March", 4 => "April", 5 => "May", 6 => "June", 7 => "July", 8 => "August", 9 => "September", 10 => "October", 11 => "November", _ => "December" }
    }

    /// Kısa gün adı (Pzt, Sal ...), takvim başlıkları için.
    pub fn weekday_short(w: u8) -> &'static str {
        match w {
            0 => "Pt",
            1 => "Sa",
            2 => "Ça",
            3 => "Pe",
            4 => "Cu",
            5 => "Ct",
            _ => "Pz",
        }
    }

    pub fn weekday_short_en(w: u8) -> &'static str {
        match w { 0 => "Mo", 1 => "Tu", 2 => "We", 3 => "Th", 4 => "Fr", 5 => "Sa", _ => "Su" }
    }
}
