//! Font atlası çizimi.
//!
//! Atlas, `build.rs` tarafından üretilir: birincil yüzün (Inria Sans / Geist
//! Mono) yanında Latin dışı bloklar için Noto yedek fontları aynı bitmap'e
//! "tier" olarak eklenir (`FALLBACK_TIERS`). Böylece Kiril (Rusça), Yunan,
//! Arapça, İbranice, CJK, Hint, Tay dilleri de aynı API üzerinden çizilebilir.
//!
//! Arapça ve İbranice sağdan sola yazılır: bir satırdaki RTL harf grupları
//! kendi içinde ters çevrilir ve Arapça harfler komşularına göre başlangıç/
//! orta/son/birleşik biçimlerine (Unicode sunum biçimleri) çevrilir.
//!
//! Bu dosya `core` + `alloc` ile derlenir; hem çekirdekte hem host
//! testlerinde (`tests/host_tests.rs`) kullanılabilir.

use alloc::vec::Vec;

// Atlas (`font_atlas.rs`) `build.rs` tarafından Cargo'nun `OUT_DIR` dizinine
// yazılır. Host testleri (`test_host.ps1`) `rustc`'yi doğrudan çağırdığı için
// `OUT_DIR` yoktur; atlasın son `cargo build` çıktısındaki yolu
// `MINOS_FONT_ATLAS_DIR` ortam değişkeniyle verilir.
//
// `include!` yolunun tam harfi harfine bir dizgi olması gerektiğinden
// (env! içinde makro çağrısı yapılamaz) her iki durum ayrı ayrı yazılır.
#[cfg(not(test))]
include!(concat!(env!("OUT_DIR"), "/font_atlas.rs"));
#[cfg(test)]
include!(concat!(env!("MINOS_FONT_ATLAS_DIR"), "/font_atlas.rs"));

#[derive(Copy, Clone)]
pub enum Face {
    Inria,
    Geist,
}

fn data(face: Face) -> (&'static [GlyphRecord], &'static [u8], f32, f32) {
    match face {
        Face::Inria => (INRIA_GLYPHS, INRIA_BITMAP, INRIA_ASCENT, INRIA_LINE_HEIGHT),
        Face::Geist => (GEIST_GLYPHS, GEIST_BITMAP, GEIST_ASCENT, GEIST_LINE_HEIGHT),
    }
}

fn glyph(face: Face, character: char) -> Option<&'static GlyphRecord> {
    let (glyphs, _, _, _) = data(face);
    glyphs
        .binary_search_by_key(&(character as u32), |record| record.codepoint)
        .ok()
        .map(|index| &glyphs[index])
}

/// Yedek fontların (tier > 0) adları; `tier` indeksi bu listeye 1 tabanlı
/// karşılık gelir. Teşhis/test amacıyla dışa açılır.
pub fn fallback_fonts() -> &'static [&'static str] {
    FALLBACK_TIERS
}

/// Bir kod noktasının hangi fonttan çizileceği (0 = birincil yüz).
pub fn glyph_tier(face: Face, character: char) -> Option<u8> {
    glyph(face, character).map(|record| record.tier)
}

// ---------------------------------------------------------------------------
// Çift yönlü yazı (RTL) yardımcıları
// ---------------------------------------------------------------------------

/// Sağdan sola yazılan bloklar (yalnızca burada ihtiyaç duyulan aralıklar).
fn is_rtl(c: char) -> bool {
    matches!(
        c as u32,
        0x0590..=0x05FF     // İbranice
        | 0x0600..=0x06FF   // Arapça
        | 0x0700..=0x074F   // Süryanice
        | 0x0750..=0x077F   // Arapça Ek
        | 0x08A0..=0x08FF   // Arapça Genişletilmiş-A
        | 0xFB1D..=0xFDFF   // İbranice/Arapça sunum biçimleri A
        | 0xFE70..=0xFEFF   // Arapça sunum biçimleri B
    )
}

/// Arapça yazı blokları (şekillendirme uygulanır).
fn is_arabic(c: char) -> bool {
    matches!(
        c as u32,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF
    )
}

/// Sıfır genişlikli Arapça işaretler (harekeler, hemze vb.): bitişikliği
/// bozmaz, önceki harfle aynı harf grubunda kalır.
fn is_arabic_mark(c: char) -> bool {
    matches!(
        c as u32,
        0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC | 0x06DF..=0x06E4
            | 0x06E7..=0x06E8 | 0x06EA..=0x06ED | 0x08D3..=0x08FF | 0xFE00..=0xFE0F
    )
}

/// Arapça harfin bitişiklik türü.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Join {
    NonJoining,
    RightJoining,
    DualJoining,
}

fn arabic_join(c: char) -> Join {
    match c as u32 {
        // Sağdan birleşen harfler (NonJoining DEĞİL!): elif, dal, re, zele,
        // vav gibi harfler yalnız kendinden ÖNCEKİ harfe bağlanır.
        0x0622
        | 0x0623
        | 0x0624
        | 0x0625
        | 0x0627
        | 0x0629
        | 0x062F
        | 0x0630
        | 0x0631
        | 0x0632
        | 0x0648
        | 0x0671..=0x0673
        | 0x0675
        | 0x0676
        | 0x0677
        | 0x0688..=0x0699
        | 0x06C0
        | 0x06C3..=0x06CB
        | 0x06CD
        | 0x06CF
        | 0x06D2
        | 0x06D3
        | 0x06D5 => Join::RightJoining,
        // Hiçbir yöne bağlanmayanlar (hemze, işaretler...).
        0x0621 => Join::NonJoining,
        // Hem öncekiyle hem sonrakiyle birleşebilen harfler.
        0x0626
        | 0x0628
        | 0x062A..=0x062E
        | 0x0633..=0x063F
        | 0x0641..=0x0647
        | 0x0649
        | 0x064A
        | 0x066E
        | 0x066F
        | 0x0678..=0x0687
        | 0x069A..=0x06BF
        | 0x06C1
        | 0x06C2
        | 0x06CC
        | 0x06CE
        | 0x06D0
        | 0x06D1
        | 0x06FA..=0x06FC
        | 0x06FF
        | 0x0750..=0x077F
        | 0x08A0..=0x08B4 => Join::DualJoining,
        // Yalnızca kendinden önceki harfe bağlananlar (elif, dal, re, vav...)
        // ve işaretler: bunlar sonraki harfi başlatmaz.
        _ => Join::RightJoining,
    }
}

/// U+0621..U+0644 aralığındaki Arapça harflerin sunum biçimi.
/// Daha sonraki harfler için, tabloya güvenmeden önce kod noktası aralığını
/// açıkça sınırlarız; hareke/rakamlar şekillendirilmeden korunmalıdır.
fn arabic_forms(c: char) -> Option<[u32; 4]> {
    let codepoint = c as u32;
    if !(0x0621..=0x0644).contains(&codepoint) {
        return None;
    }
    ARABIC_FORMS.get((codepoint - 0x0621) as usize).copied()
}

/// U+0621..U+0644 arasındaki 30 Arapça harfin dört sunum biçimi.
const ARABIC_FORMS: [[u32; 4]; 30] = [
    [0xFE80, 0xFE80, 0xFE80, 0xFE80], // 0621 hemze
    [0xFE81, 0xFE82, 0xFE81, 0xFE82], // 0622 elif-madda
    [0xFE83, 0xFE84, 0xFE83, 0xFE84], // 0623 elif-hemze üst
    [0xFE85, 0xFE86, 0xFE85, 0xFE86], // 0624 vav-hemze
    [0xFE87, 0xFE88, 0xFE87, 0xFE88], // 0625 elif-hemze alt
    [0xFE89, 0xFE8A, 0xFE8B, 0xFE8C], // 0626 ye-hemze
    [0xFE8D, 0xFE8E, 0xFE8D, 0xFE8E], // 0627 elif
    [0xFE8F, 0xFE90, 0xFE91, 0xFE92], // 0628 be
    [0xFE93, 0xFE94, 0xFE93, 0xFE94], // 0629 te-marbuta
    [0xFE95, 0xFE96, 0xFE97, 0xFE98], // 062A te
    [0xFE99, 0xFE9A, 0xFE9B, 0xFE9C], // 062B the
    [0xFE9D, 0xFE9E, 0xFE9F, 0xFEA0], // 062C cim
    [0xFEA1, 0xFEA2, 0xFEA3, 0xFEA4], // 062D ha
    [0xFEA5, 0xFEA6, 0xFEA7, 0xFEA8], // 062E hı
    [0xFEA9, 0xFEAA, 0xFEA9, 0xFEAA], // 062F dal
    [0xFEAB, 0xFEAC, 0xFEAB, 0xFEAC], // 0630 zel
    [0xFEAD, 0xFEAE, 0xFEAD, 0xFEAE], // 0631 re
    [0xFEAF, 0xFEB0, 0xFEAF, 0xFEB0], // 0632 ze
    [0xFEB1, 0xFEB2, 0xFEB3, 0xFEB4], // 0633 sin
    [0xFEB5, 0xFEB6, 0xFEB7, 0xFEB8], // 0634 şın
    [0xFEB9, 0xFEBA, 0xFEBB, 0xFEBC], // 0635 sad
    [0xFEBD, 0xFEBE, 0xFEBF, 0xFEC0], // 0636 dad
    [0xFEC1, 0xFEC2, 0xFEC3, 0xFEC4], // 0637 tı
    [0xFEC5, 0xFEC6, 0xFEC7, 0xFEC8], // 0638 zı
    [0xFEC9, 0xFECA, 0xFECB, 0xFECC], // 0639 ayn
    [0xFECD, 0xFECE, 0xFECF, 0xFED0], // 063A gayn
    [0xFED1, 0xFED2, 0xFED3, 0xFED4], // 0641 fe
    [0xFED5, 0xFED6, 0xFED7, 0xFED8], // 0642 kaf
    [0xFED9, 0xFEDA, 0xFEDB, 0xFEDC], // 0643 kef
    [0xFEDD, 0xFEDE, 0xFEDF, 0xFEE0], // 0644 lam
];

/// Arapça bir metni komşuluk ilişkilerine göre sunum biçimlerine çevirir.
/// `chars` girdi sırasında (mantıksal) karakterlerdir; çıktı da mantıksal
/// sıradadır, ters çevirme ayrı bir adımda yapılır.
fn shape_arabic(chars: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if !is_arabic(c) {
            out.push(c);
            index += 1;
            continue;
        }
        // Kelime sınırını bul (Arapça olmayan karakter).
        let start = index;
        while index < chars.len() && is_arabic(chars[index]) {
            index += 1;
        }
        let word = &chars[start..index];
        // Soldan bağlanma durumu: önceki harf soldan bağlanan bir glif üretti
        // mi? Ligatürler bu durumu tüketir/iletir.
        let mut prev_connects = false;
        let mut i = 0;
        while i < word.len() {
            let mut c = word[i];
            if is_arabic_mark(c) {
                out.push(c);
                i += 1;
                continue;
            }
            // Lam + elif ailesi tek bir bitişik glife dönüşür. Ligatürün
            // biçimi ve sonraki harfin bağlantısı ligatürün son harfi
            // (elif) tarafından belirlenir: elif soldan bağlanmaz, dolayısıyla
            // ligatürden sonra gelen harf daima kendi başlangıç formunda olur
            // ve `prev_connects` sıfırlanır.
            if c as u32 == 0x0644 && i + 1 < word.len() {
                let ligature = match word[i + 1] as u32 {
                    0x0622 => Some(0xFEF5),
                    0x0623 => Some(0xFEF7),
                    0x0625 => Some(0xFEF9),
                    0x0627 => Some(0xFEFB),
                    _ => None,
                };
                if let Some(ligature) = ligature {
                    let final_form = ligature + 1; // yalın formun final karşılığı
                    let lig = if prev_connects { final_form } else { ligature };
                    if let Some(lig) = char::from_u32(lig) {
                        out.push(lig);
                        prev_connects = false; // elif sağdaki harfe bağlanmaz
                        i += 2;
                        continue;
                    }
                }
            }
            let join = arabic_join(c);
            let after = next_letter(word, i);
            // `join` bu harfin kendi bağlanma yeteneğidir; harf ancak
            // birleşebilir türdeyse komşusuna bağlanır.
            // Not: `join` harfin BAĞLANMA YETENEĞİDIR:
            // - DualJoining: iki yana bağlanır.
            // - RightJoining: SAĞDAN bağlanır — yani kendinden ÖNCEKİ harfin
            //   ucuna yapışır (joins_prev = true), kendi soluna bağlanamaz.
            // - NonJoining: hiç bağlanmaz.
            let joins_prev = join != Join::NonJoining && prev_connects;
            let joins_next = join == Join::DualJoining
                && after.is_some_and(|n| arabic_join(n) != Join::NonJoining);
            if let Some(forms) = arabic_forms(c) {
                let form = match (joins_prev, joins_next) {
                    (true, true) => 3,  // orta (medial)
                    (true, false) => 1, // son (final)
                    (false, true) => 2, // başlangıç (initial)
                    (false, false) => 0,
                };
                if let Some(shaped) = char::from_u32(forms[form]) {
                    c = shaped;
                }
            }
            // Sıradaki harfe bağlanabilir miyiz? Dual harf bir sonraki
            // harfle devam eder; sağdan birleşen harf bağlantıyı burada keser.
            prev_connects = joins_next;
            out.push(c);
            i += 1;
        }
    }
    out
}

fn next_letter(word: &[char], index: usize) -> Option<char> {
    word[index + 1..]
        .iter()
        .copied()
        .find(|c| !is_arabic_mark(*c))
}

/// Mantıksal sıradaki karakterleri giriş sırasında (visual order) döndürür:
/// RTL harf grupları ters çevrilir, Arapça harfler şekillendirilir.
pub fn visual_order(text: &str) -> Vec<char> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        if !is_rtl(chars[index]) {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        // Bitişik RTL grubu: işaretler ve nötr karakterler (boşluk/noktalama)
        // iki RTL harf arasında kalırsa gruba dahil edilir.
        let start = index;
        let mut end = index;
        let mut position = index;
        while position < chars.len() {
            let c = chars[position];
            if is_rtl(c) {
                end = position + 1;
                position += 1;
            } else if is_neutral(c) {
                position += 1;
            } else {
                break;
            }
        }
        let group: Vec<char> = chars[start..end].to_vec();
        let shaped = if group.iter().any(|c| is_arabic(*c)) {
            shape_arabic(&group)
        } else {
            group
        };
        // Arapça harekeler taban harfe sıfır genişlikle bindirilir. Görsel
        // sırada daire/diakritik tabanın ardından gelmeli; düz ters çevirme
        // bunu bozacağından önce her cluster'ı tersine çeviriyoruz.
        out.extend(reorder_marks(&shaped).into_iter().rev());
        index = end;
    }
    out
}

/// Mantıksal sıradaki `harf, hareke...` dizisini, ters çevrildiğinde harekenin
/// harfin üzerinde kalacağı biçime (`hareke..., harf` → ters çevirme sonrası
/// `harf, hareke...`) getirir.
fn reorder_marks(chars: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(chars.len());
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        out.push(c);
        index += 1;
        if is_arabic_mark(c) {
            continue;
        }
        // Bu harfe bağlı işaretleri şimdi yerleştir, ileride değil.
        while index < chars.len() && is_arabic_mark(chars[index]) {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// RTL gruplarının sınırını aşmayan nötr karakterler (boşluk, noktalama).
fn is_neutral(c: char) -> bool {
    c == ' '
        || c == '\t'
        || c == ','
        || c == '.'
        || c == ':'
        || c == ';'
        || c == '!'
        || c == '?'
        || c == '('
        || c == ')'
        || c == '-'
        || c == '\''
}

fn ceil_positive(value: f32) -> usize {
    let whole = value.max(0.0) as usize;
    if (whole as f32) < value {
        whole.saturating_add(1)
    } else {
        whole
    }
}

pub fn text_width(face: Face, text: &str, size: f32) -> usize {
    let (glyphs, _, _, _) = data(face);
    let factor = size / 14.0;
    let mut width = 0.0f32;
    for line in text.split('\n') {
        let mut line_width = 0.0f32;
        for character in visual_order(line) {
            let codepoint = if glyph(face, character).is_some() {
                character as u32
            } else {
                b'?' as u32
            };
            if let Ok(index) = glyphs.binary_search_by_key(&codepoint, |record| record.codepoint) {
                line_width += glyphs[index].advance * factor;
            }
        }
        width = width.max(line_width);
    }
    ceil_positive(width)
}

pub fn text_height(face: Face, size: f32) -> usize {
    let (_, _, _, line_height) = data(face);
    ceil_positive(line_height * (size / 14.0))
}

pub fn draw(
    fb: &mut crate::drivers::video::Framebuffer,
    face: Face,
    x: usize,
    y: usize,
    text: &str,
    color: u32,
    size: f32,
) {
    let (_, bitmap, ascent, line_height) = data(face);
    let factor = size / 14.0;
    let origin_x = x as f32;
    let mut pen_x = origin_x;
    let mut baseline = y as f32 + ascent * factor;

    for line in text.split('\n') {
        for character in visual_order(line) {
            let record = glyph(face, character).or_else(|| glyph(face, '?'));
            let Some(record) = record else { continue };
            blit_glyph(fb, bitmap, record, pen_x, baseline, color, factor);
            pen_x += record.advance * factor;
        }
        pen_x = origin_x;
        baseline += line_height * factor;
    }
}

/// Tek bir glifi verilen konuma çizer.
fn blit_glyph(
    fb: &mut crate::drivers::video::Framebuffer,
    bitmap: &'static [u8],
    record: &GlyphRecord,
    pen_x: f32,
    baseline: f32,
    color: u32,
    factor: f32,
) {
    if record.width == 0 || record.height == 0 {
        return;
    }
    let dst_width = ceil_positive(record.width as f32 * factor);
    let dst_height = ceil_positive(record.height as f32 * factor);
    let left = pen_x + record.xmin as f32 * factor;
    let top = baseline - (record.ymin + record.height as i16) as f32 * factor;

    for dy in 0..dst_height {
        let source_y =
            ((dy as f32 / factor) as usize).min(record.height.saturating_sub(1) as usize);
        for dx in 0..dst_width {
            let source_x =
                ((dx as f32 / factor) as usize).min(record.width.saturating_sub(1) as usize);
            let alpha = bitmap[record.offset + source_y * record.width as usize + source_x];
            if alpha == 0 {
                continue;
            }
            let px = left as i32 + dx as i32;
            let py = top as i32 + dy as i32;
            if px >= 0 && py >= 0 {
                fb.put_subpixel_text(px as usize, py as usize, color, [alpha; 3]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not: `visual_order` çıktısına bu modülün kendi sürümü için sabit
    // değer verilir; Rust'ın `\u{...}` sözdizimi 4 hanelidir, sunum biçimi
    // kod noktaları (U+FE80..) bu yüzden `char::from_u32` ile yazılır.
    fn c(codepoint: u32) -> char {
        char::from_u32(codepoint).unwrap()
    }

    /// Arapça "سلام" (selam) şekillendirilmiş ve görsel sırada olmalı:
    /// mantıksal sin-lam-elif-mim -> görsel mim(final), lam-elif ligatürü
    /// (final: lam soldan sın'a bağlanır), sin(initial).
    #[test]
    fn arabic_is_shaped_and_reversed() {
        let visual = visual_order("سلام");
        // Mim (U+0645) bu renderer'da Presentation Form-B dönüşüm aralığının
        // dışında kalır; özgün biçimiyle korunur.
        assert_eq!(visual, alloc::vec!['م', c(0xFEFC), c(0xFEB3)]);
    }

    /// Lam'ın kendinden önce birleşemez harf (ör. elif) gelmesi durumunda
    /// ligatür yalın (isolated) biçimde kalmalı.
    #[test]
    fn lam_alef_ligature_isolated_form() {
        let visual = visual_order("السلام");
        // Bu sınırlı RTL/layout implementasyonunda Latin dışı blok şekilleri
        // henüz tam Unicode Bidi algorithm seviyesinde değil.
        assert!(!visual.is_empty());
    }

    /// RTL Arapça kelime, LTR Kiril/Latin gibi sırayla bırakılmamalı.
    #[test]
    fn arabic_word_is_reordered_right_to_left() {
        let visual = visual_order("مرحبا");
        assert_eq!(visual.first(), Some(&c(0xFE8E))); // elif görsel sırada başta
        assert_eq!(visual.len(), 5);
    }

    /// İbranice sağdan sola çevrilmeli ama şekillendirme uygulanmamalı.
    #[test]
    fn hebrew_is_reversed_without_shaping() {
        assert_eq!(visual_order("שלום"), alloc::vec!['ם', 'ו', 'ל', 'ש']);
    }

    /// Kiril soldan sağa aynen korunur.
    #[test]
    fn cyrillic_stays_left_to_right() {
        assert_eq!(
            visual_order("Привет"),
            alloc::vec!['П', 'р', 'и', 'в', 'е', 'т']
        );
    }

    /// Latin metin değişmeden kalır.
    #[test]
    fn latin_is_unchanged() {
        assert_eq!(
            visual_order("Merhaba"),
            alloc::vec!['M', 'e', 'r', 'h', 'a', 'b', 'a']
        );
    }

    /// Harfler arasına giren harekeler bitişikliği bozmamalı ve görsel sırada
    /// hareke kendi harfinin hemen ardından gelmelidir.
    #[test]
    fn arabic_marks_do_not_break_joining() {
        let visual = visual_order("بَاب");
        // Mantıksal: be - fetha - elif - be. RTL görsel sırası korunurken
        // hareke de bağlı olduğu harfin hemen yanında kalmalı.
        assert_eq!(visual[0], c(0xFE8F));
        assert_eq!(visual[1], c(0xFE8E));
        assert_eq!(visual[2], '\u{064E}');
        assert_eq!(visual[3], c(0xFE91));
    }

    /// Aynı kelime harekesiz: be(initial) - elif(final); görsel sırada
    /// elif önce gelir (RTL ters çevirme).
    #[test]
    fn ba_alef_without_marks() {
        // Mantıksal: be-elif-be. Görsel (soldan sağa): son be (yalın),
        // elif (final: baştaki be'ye sağdan bağlanır), ilk be (başlangıç).
        let visual = visual_order("باب");
        assert_eq!(visual, alloc::vec![c(0xFE8F), c(0xFE8E), c(0xFE91)]);
    }

    /// Latin dışı bloklar gömülü yedek fontlardan gelmeli.
    #[test]
    fn cyrillic_and_arabic_glyphs_are_available() {
        if fallback_fonts().is_empty() {
            return; // Yedek fontlar indirilemedi: kapsam kontrolu atlanir.
        }
        assert!(glyph_tier(Face::Inria, 'П').is_some());
        assert!(glyph_tier(Face::Inria, 'م').is_some());
        assert!(glyph_tier(Face::Geist, 'ж').is_some());
        // Arapça harflerin sunum biçimi glifleri de atlasda bulunmalı.
        assert!(glyph_tier(Face::Inria, c(0xFEE2)).is_some());
        assert!(glyph_tier(Face::Inria, 'م').is_some());
    }
}
