//! Ozellik bazli host testleri.
//!
//! Hesap makinesi cekirdegi, sure bicimlendirme, UTF-8 dosya adi duzenleme ve
//! tema kontrasti burada, cekirdekten bagimsiz moduller uzerinden sinanir.

use alloc::string::String;

use crate::calc_core::{CalcCore, INPUT_CAP};
use crate::{textedit, theme, timefmt};

fn eval(expression: &str) -> String {
    let mut calc = CalcCore::new();
    calc.set_expression(expression);
    let mut out = [0u8; INPUT_CAP];
    let len = CalcCore::format_value(calc.value(), &mut out);
    String::from(core::str::from_utf8(&out[..len]).unwrap())
}

// ---------------------------------------------------------------------------
// Hesap makinesi
// ---------------------------------------------------------------------------

#[test]
fn calculator_respects_precedence() {
    assert_eq!(eval("2+3*4"), "14");
    assert_eq!(eval("2*3+4"), "10");
    assert_eq!(eval("2+3*4-6/3"), "12");
}

#[test]
fn calculator_supports_decimals_and_negatives() {
    assert_eq!(eval("10/4"), "2.5");
    assert_eq!(eval("1.5*2"), "3");
    assert_eq!(eval("0.1+0.2"), "0.3");
    assert_eq!(eval("-5+2"), "-3");
    assert_eq!(eval("5*-2"), "-10");
}

#[test]
fn calculator_editing_and_buttons() {
    let mut calc = CalcCore::new();
    for c in "7+8".chars() {
        calc.press(c);
    }
    assert_eq!(calc.expression(), &['7', '+', '8']);
    calc.press('B');
    assert_eq!(calc.expression(), &['7', '+']);
    calc.press('9');
    calc.press('=');
    assert_eq!(calc.expression(), &['1', '6']);
    calc.press('C');
    assert!(calc.expression().is_empty());
    // Ardisik operatorler sonuncusuyla degistirilir.
    for c in "4++5".chars() {
        calc.press(c);
    }
    assert_eq!(calc.expression(), &['4', '+', '5']);
}

#[test]
fn calculator_percent_and_division_by_zero() {
    let mut calc = CalcCore::new();
    calc.set_expression("50");
    calc.press('%');
    assert_eq!(calc.value(), 500_000); // 0.5 * SCALE
    calc.set_expression("4/0");
    assert_eq!(calc.value(), 4 * crate::calc_core::SCALE);
}

// ---------------------------------------------------------------------------
// Sure bicimlendirme
// ---------------------------------------------------------------------------

#[test]
fn duration_splits_into_parts() {
    assert_eq!(
        timefmt::split_duration(0),
        timefmt::DurationParts {
            hours: 0,
            minutes: 0,
            seconds: 0
        }
    );
    assert_eq!(
        timefmt::split_duration(3661),
        timefmt::DurationParts {
            hours: 1,
            minutes: 1,
            seconds: 1
        }
    );
    assert_eq!(
        timefmt::split_duration(90_061),
        timefmt::DurationParts {
            hours: 25,
            minutes: 1,
            seconds: 1
        }
    );
}

#[test]
fn duration_formats_in_turkish_and_english() {
    assert_eq!(timefmt::duration_string(0, false), "0sa 0dk 0sn");
    assert_eq!(timefmt::duration_string(3661, false), "1sa 1dk 1sn");
    assert_eq!(timefmt::duration_string(3661, true), "1h 1m 1s");
    assert_eq!(timefmt::duration_string(7_384, true), "2h 3m 4s");
}

#[test]
fn clock_format_is_zero_padded() {
    assert_eq!(timefmt::clock_string(0), "0:00");
    assert_eq!(timefmt::clock_string(5), "0:05");
    assert_eq!(timefmt::clock_string(65), "1:05");
    assert_eq!(timefmt::clock_string(3661), "1:01:01");
    assert_eq!(timefmt::clock_string(360_000), "100:00:00");
}

#[test]
fn fixed_buffer_formatting_never_overflows() {
    let mut tiny = [0u8; 4];
    let len = timefmt::format_duration(3661, false, &mut tiny);
    assert!(len <= tiny.len());
    assert_eq!(&tiny[..len], b"1sa ");
}

// ---------------------------------------------------------------------------
// UTF-8 dosya adi duzenleme
// ---------------------------------------------------------------------------

const TURKISH: &str = "a\u{011F}\u{0131}"; // agi -> a + g-breve + i-dotless

#[test]
fn rename_boundaries_stay_on_char_boundaries() {
    let bytes = TURKISH.as_bytes();
    assert_eq!(bytes.len(), 5);
    assert_eq!(textedit::prev_char_boundary(bytes, 5), 3);
    assert_eq!(textedit::prev_char_boundary(bytes, 3), 1);
    assert_eq!(textedit::prev_char_boundary(bytes, 1), 0);
    assert_eq!(textedit::next_char_boundary(bytes, 0, 5), 1);
    assert_eq!(textedit::next_char_boundary(bytes, 1, 5), 3);
    assert_eq!(textedit::next_char_boundary(bytes, 3, 5), 5);
}

#[test]
fn rename_truncation_never_splits_utf8() {
    let bytes = "Kitapl\u{0131}k".as_bytes();
    let take = textedit::truncate_to_boundary(bytes, 7);
    assert_eq!(take, 6);
    assert!(core::str::from_utf8(&bytes[..take]).is_ok());
}

#[test]
fn rename_edit_sequence_keeps_valid_utf8() {
    let mut buf = [0u8; 32];
    let start = "Belge";
    buf[..start.len()].copy_from_slice(start.as_bytes());
    let mut len = start.len();
    let mut cursor = len;

    // "Belge" + "ğ" -> "Belgeğ"
    assert!(textedit::insert_char(
        &mut buf,
        &mut len,
        &mut cursor,
        '\u{011F}'
    ));
    assert_eq!(core::str::from_utf8(&buf[..len]).unwrap(), "Belge\u{011F}");

    // Tek backspace iki baytin tamamini siler.
    assert!(textedit::backspace(&mut buf, &mut len, &mut cursor));
    assert_eq!(core::str::from_utf8(&buf[..len]).unwrap(), "Belge");
    assert_eq!(cursor, len);

    // Basa git ve ileri sil.
    cursor = 0;
    assert!(textedit::delete_forward(&mut buf, &mut len, &mut cursor));
    assert_eq!(core::str::from_utf8(&buf[..len]).unwrap(), "elge");
    assert_eq!(cursor, 0);
}

#[test]
fn rename_bulk_boundary_fill_is_safe() {
    // NAME_MAX benzeri kucuk tampona cok baytli ad sigdirilir.
    let name = "\u{011F}\u{011F}\u{011F}";
    let mut buf = [0u8; 5];
    let take = textedit::truncate_to_boundary(name.as_bytes(), buf.len());
    assert_eq!(take, 4);
    buf[..take].copy_from_slice(&name.as_bytes()[..take]);
    assert!(core::str::from_utf8(&buf[..take]).is_ok());
}

// ---------------------------------------------------------------------------
// Tema kontrasti (WCAG AA)
// ---------------------------------------------------------------------------

const MIN_NORMAL_TEXT: f32 = 4.5;

#[test]
fn all_themes_have_readable_body_text() {
    for id in theme::ThemeId::ALL {
        let p = theme::palette(id);
        let bg = p[theme::IDX_WINDOW_BG as usize];
        let text = p[theme::IDX_TEXT_MAIN as usize];
        let ratio = theme::contrast_ratio(bg, text);
        assert!(
            ratio >= MIN_NORMAL_TEXT,
            "{:?}: text main/window bg contrast {:.2} < 4.5",
            id,
            ratio
        );
    }
}

#[test]
fn all_themes_have_readable_muted_text() {
    for id in theme::ThemeId::ALL {
        let p = theme::palette(id);
        for bg_idx in [
            theme::IDX_WINDOW_BG,
            theme::IDX_TASKBAR_BG,
            theme::IDX_TITLEBAR,
        ] {
            let bg = p[bg_idx as usize];
            let text = p[theme::IDX_TEXT_MUTED as usize];
            let ratio = theme::contrast_ratio(bg, text);
            assert!(
                ratio >= MIN_NORMAL_TEXT,
                "{:?}: muted text on idx {} contrast {:.2} < 4.5",
                id,
                bg_idx,
                ratio
            );
        }
    }
}

#[test]
fn accent_colors_are_legible_as_text() {
    for id in theme::ThemeId::ALL {
        let p = theme::palette(id);
        for bg_idx in [theme::IDX_WINDOW_BG, theme::IDX_TITLEBAR] {
            let bg = p[bg_idx as usize];
            for accent_idx in [
                theme::IDX_ACCENT_BLUE,
                theme::IDX_ACCENT_GREEN,
                theme::IDX_ACCENT_RED,
                theme::IDX_ACCENT_YELLOW,
                theme::IDX_ACCENT_PINK,
            ] {
                let ratio = theme::contrast_ratio(bg, p[accent_idx as usize]);
                assert!(
                    ratio >= MIN_NORMAL_TEXT,
                    "{:?}: accent {} on bg {} contrast {:.2} < 4.5",
                    id,
                    accent_idx,
                    bg_idx,
                    ratio
                );
            }
        }
    }
}

#[test]
fn contrast_ratio_is_symmetric_and_bounded() {
    let middle = theme::contrast_ratio(0x808080, 0x808080);
    assert!((middle - 1.0).abs() < 0.001);
    let forward = theme::contrast_ratio(0x000000, 0xFFFFFF);
    let reverse = theme::contrast_ratio(0xFFFFFF, 0x000000);
    assert!((forward - 21.0).abs() < 0.1);
    assert!((forward - reverse).abs() < 0.001);
}

// ---------------------------------------------------------------------------
// RTC Tarih/Saat ve Saat Dilimi Farkı (Timezone Offset)
// ---------------------------------------------------------------------------

#[test]
fn datetime_hour_offset_same_day() {
    let dt = crate::rtc::DateTime {
        year: 2026,
        month: 10,
        day: 26,
        hour: 12,
        minute: 30,
        second: 45,
    };
    // Türkiye (UTC+3)
    let tr = dt.with_hour_offset(3);
    assert_eq!(tr.hour, 15);
    assert_eq!(tr.day, 26);
    assert_eq!(tr.month, 10);
    assert_eq!(tr.year, 2026);

    // UTC (+0)
    let utc = dt.with_hour_offset(0);
    assert_eq!(utc, dt);

    // ABD Doğu (UTC-5)
    let us_east = dt.with_hour_offset(-5);
    assert_eq!(us_east.hour, 7);
    assert_eq!(us_east.day, 26);
    assert_eq!(us_east.month, 10);
    assert_eq!(us_east.year, 2026);
}

#[test]
fn datetime_hour_offset_forward_wraps_day_month_year() {
    // Gece yarısı gün atlama (22:00 + 3 = 01:00 sonraki gün)
    let dt = crate::rtc::DateTime {
        year: 2026,
        month: 5,
        day: 15,
        hour: 22,
        minute: 10,
        second: 0,
    };
    let tr = dt.with_hour_offset(3);
    assert_eq!(tr.hour, 1);
    assert_eq!(tr.day, 16);
    assert_eq!(tr.month, 5);

    // Ay sonu atlama (31 Ocak 23:00 + 3 = 1 Şubat 02:00)
    let dt_month = crate::rtc::DateTime {
        year: 2026,
        month: 1,
        day: 31,
        hour: 23,
        minute: 0,
        second: 0,
    };
    let feb = dt_month.with_hour_offset(3);
    assert_eq!(feb.hour, 2);
    assert_eq!(feb.day, 1);
    assert_eq!(feb.month, 2);
    assert_eq!(feb.year, 2026);

    // Yıl sonu atlama (31 Aralık 22:30 + 3 = 1 Ocak 01:30 sonraki yıl)
    let dt_year = crate::rtc::DateTime {
        year: 2026,
        month: 12,
        day: 31,
        hour: 22,
        minute: 30,
        second: 0,
    };
    let new_year = dt_year.with_hour_offset(3);
    assert_eq!(new_year.hour, 1);
    assert_eq!(new_year.day, 1);
    assert_eq!(new_year.month, 1);
    assert_eq!(new_year.year, 2027);
}

#[test]
fn datetime_hour_offset_backward_wraps_day_month_year() {
    // Geriye gün atlama (02:00 - 5 = 21:00 önceki gün)
    let dt = crate::rtc::DateTime {
        year: 2026,
        month: 6,
        day: 10,
        hour: 2,
        minute: 15,
        second: 0,
    };
    let us = dt.with_hour_offset(-5);
    assert_eq!(us.hour, 21);
    assert_eq!(us.day, 9);
    assert_eq!(us.month, 6);

    // Artık yılda 1 Mart'tan geriye 29 Şubat'a atlama (1 Mart 2024 01:00 - 5 = 29 Şubat 20:00)
    let dt_leap = crate::rtc::DateTime {
        year: 2024,
        month: 3,
        day: 1,
        hour: 1,
        minute: 0,
        second: 0,
    };
    let leap_back = dt_leap.with_hour_offset(-5);
    assert_eq!(leap_back.hour, 20);
    assert_eq!(leap_back.day, 29);
    assert_eq!(leap_back.month, 2);
    assert_eq!(leap_back.year, 2024);

    // Normal yılda 1 Mart'tan geriye 28 Şubat'a atlama (1 Mart 2023 01:00 - 5 = 28 Şubat 20:00)
    let dt_non_leap = crate::rtc::DateTime {
        year: 2023,
        month: 3,
        day: 1,
        hour: 1,
        minute: 0,
        second: 0,
    };
    let non_leap_back = dt_non_leap.with_hour_offset(-5);
    assert_eq!(non_leap_back.hour, 20);
    assert_eq!(non_leap_back.day, 28);
    assert_eq!(non_leap_back.month, 2);

    // Yıl başından geriye önceki yılın 31 Aralık gününe atlama (1 Ocak 2026 03:00 - 5 = 31 Aralık 2025 22:00)
    let dt_jan1 = crate::rtc::DateTime {
        year: 2026,
        month: 1,
        day: 1,
        hour: 3,
        minute: 0,
        second: 0,
    };
    let prev_year = dt_jan1.with_hour_offset(-5);
    assert_eq!(prev_year.hour, 22);
    assert_eq!(prev_year.day, 31);
    assert_eq!(prev_year.month, 12);
    assert_eq!(prev_year.year, 2025);
}

#[test]
fn datetime_leap_years_and_weekdays() {
    assert!(crate::rtc::DateTime::is_leap_year(2000));
    assert!(crate::rtc::DateTime::is_leap_year(2024));
    assert!(!crate::rtc::DateTime::is_leap_year(1900));
    assert!(!crate::rtc::DateTime::is_leap_year(2023));

    // 26 Ekim 2026 Pazartesi (0 = Pazartesi)
    assert_eq!(crate::rtc::DateTime::weekday(2026, 10, 26), 0);
    assert_eq!(crate::rtc::DateTime::weekday_name(0), "Pazartesi");
    assert_eq!(crate::rtc::DateTime::weekday_name_en(0), "Monday");
}
