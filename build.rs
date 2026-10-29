use fontdue::{Font, FontSettings};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

const FONT_SIZE: f32 = 14.0;

/// Latin dışı harfler (Kiril, Yunan, Arapça, İbranice, Türkçe, Yunanca...)
/// repo içindeki Latin fontlarında yoktur. Bu fontlar `public/fonts/fallback`
/// altına derleme zamanında indirilir; indirilemezlerse ilgili harf bloğu
/// boş kalır ve arayüz o karakterler için `?` çizer (derleme kırılmaz).
/// CJK (Çince/Japonca/Korece) fontları çıkarıldı: çok büyük (17 MB+) ve
/// kernel boyutunu ciddi oranda artırıyor.
const FALLBACK_FONTS: &[(&str, &str)] = &[
    ("NotoSans-Regular.ttf", "https://github.com/notofonts/notofonts.github.io/raw/main/fonts/NotoSans/unhinted/ttf/NotoSans-Regular.ttf"),
    ("NotoSansArabic-Regular.ttf", "https://github.com/notofonts/notofonts.github.io/raw/main/fonts/NotoSansArabic/unhinted/ttf/NotoSansArabic-Regular.ttf"),
    ("NotoSansHebrew-Regular.ttf", "https://github.com/notofonts/notofonts.github.io/raw/main/fonts/NotoSansHebrew/unhinted/ttf/NotoSansHebrew-Regular.ttf"),
];

fn load_font(path: &str) -> Option<Font> {
    let bytes = fs::read(path).ok()?;
    Font::from_bytes(bytes, FontSettings::default()).ok()
}

/// Dosya yoksa indirmeyi dener; ağ yoksa sessizce `None` döner.
fn ensure_fallback(name: &str, url: &str, dir: &PathBuf) -> Option<PathBuf> {
    let path = dir.join(name);
    if path.exists() {
        return Some(path);
    }
    let output = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "300", "-o"])
        .arg(&path)
        .arg(url)
        .output();
    let ok = output.map(|o| o.status.success()).unwrap_or(false);
    if ok && path.exists() {
        println!("cargo:warning={name} indirildi (fallback font)");
        Some(path)
    } else {
        let _ = fs::remove_file(&path);
        println!("cargo:warning={name} bulunamadi; bu harfler icin font destegi devre disi");
        None
    }
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(minos_boot_log)");
    // Host testleri (`tests/host_tests.rs`) `fs::block` modülünü dogrudan
    // `#[path]` ile baglar; o derlemede `cfg(test)` ayarlı olmadigi icin
    // bellek ici blok aygiti ayri bir cfg ile acilir.
    println!("cargo:rustc-check-cfg=cfg(host_fs_tests)");
    println!("cargo:rerun-if-changed=public/fonts");
    // Yalnızca font girdileri izlenirse `src/` değişiklikleri build script'ini
    // çalıştırmaz; Cargo bayat `OUT_DIR`'i (eski rlib ile) yeniden kullanır.
    println!("cargo:rerun-if-changed=src");

    let fallback_dir = PathBuf::from("public/fonts/fallback");
    let _ = fs::create_dir_all(&fallback_dir);
    let mut fallbacks = Vec::new();
    for (name, url) in FALLBACK_FONTS {
        let Some(path) = ensure_fallback(name, url, &fallback_dir) else {
            continue;
        };
        let Some(font) = load_font(&path.to_string_lossy()) else {
            println!("cargo:warning={name} ayristirilamadi");
            continue;
        };
        fallbacks.push((*name, font));
    }

    let faces = [
        ("INRIA", "public/fonts/InriaSans-Regular.ttf"),
        ("GEIST", "public/fonts/GeistMono-Regular.ttf"),
    ];
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let mut generated =
        File::create(out_dir.join("font_atlas.rs")).expect("create font atlas source");
    writeln!(
        generated,
        "pub struct GlyphRecord {{ pub codepoint: u32, pub tier: u8, pub offset: usize, pub width: u16, pub height: u16, pub xmin: i16, pub ymin: i16, pub advance: f32 }}"
    )
    .unwrap();
    writeln!(
        generated,
        "pub static FALLBACK_TIERS: &[&str] = &[{}];",
        fallbacks
            .iter()
            .map(|(name, _)| format!("{name:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .unwrap();

    // Aynı kaynak fonttan (face + fallback'ler) harfleri rasterize eder.
    // Face fontu tier 0, fallback'ler FALLBACK_TIERS sırasıyla tier 1, 2, ...
    // Her kod noktası yalnızca ilk glifi sağlayan fonttan alınır; böylece tek
    // bir arama tablosu (kod noktası -> tier) yeterli olur.
    for (face_name, face_path) in faces {
        println!("cargo:rerun-if-changed={face_path}");
        let face_font = load_font(face_path).unwrap_or_else(|| panic!("read/parse {face_path}"));
        let metrics = face_font
            .horizontal_line_metrics(FONT_SIZE)
            .expect("font line metrics");
        let mut sources: Vec<&Font> = vec![&face_font];
        sources.extend(fallbacks.iter().map(|(_, font)| font));

        // Yaygın kullanılan Unicode aralıkları: Basit Latin, Latin-1 Supplement,
        // Latin Extended-A/B, IPA, Çevrimiçi Semboller, Yunanca ve Koptik,
        // Kiril, Kiril Eklemesi, Genel Noktalama, Matematiksel İşleçler,
        // Oklar, Geometrik Şekiller, Çeşitli Teknik, Çeşitli Simge ve İşaretler,
        // Alfanümerik Simgeler, Batakça, Mısır Hiyeroglifleri (kısmi).
        // Tüm Unicode aralığı (0x20..=0x10FFFF, ~1.1M kod noktası) font atlasını
        // ciddi oranda büyütür; bu sınırlama boyutu büyük ölçüde azaltır.
        let codepoint_ranges: &[(u32, u32)] = &[
            (0x0020, 0x007F), // Basic Latin
            (0x00A0, 0x00FF), // Latin-1 Supplement
            (0x0100, 0x017F), // Latin Extended-A
            (0x0180, 0x024F), // Latin Extended-B
            (0x0250, 0x02AF), // IPA Extensions
            (0x0370, 0x03FF), // Greek and Coptic
            (0x0400, 0x04FF), // Cyrillic
            (0x0500, 0x052F), // Cyrillic Supplement
            (0x2000, 0x206F), // General Punctuation
            (0x2070, 0x209F), // Superscripts and Subscripts
            (0x20A0, 0x20CF), // Currency Symbols
            (0x2100, 0x214F), // Letterlike Symbols
            (0x2150, 0x218F), // Number Forms
            (0x2190, 0x21FF), // Arrows
            (0x2200, 0x22FF), // Mathematical Operators
            (0x2300, 0x23FF), // Miscellaneous Technical
            (0x2500, 0x257F), // Box Drawing / Block Elements
            (0x25A0, 0x25FF), // Geometric Shapes
            (0x2600, 0x26FF), // Miscellaneous Symbols
            (0x2700, 0x27BF), // Dingbats
            (0x2B00, 0x2BFF), // Miscellaneous Symbols and Arrows
            (0xFF00, 0xFFEF), // Halfwidth and Fullwidth Forms
        ];
        let mut bitmap_data = Vec::new();
        let mut records = Vec::new();
        for &(start, end) in codepoint_ranges {
            for codepoint in start..=end {
                let Some(character) = char::from_u32(codepoint) else {
                    continue;
                };
                let Some(tier) = sources.iter().position(|font| font.has_glyph(character)) else {
                    continue;
                };
                let (glyph, bitmap) = sources[tier].rasterize(character, FONT_SIZE);
                if bitmap_data.len() + bitmap.len() > u32::MAX as usize {
                    panic!("font atlas is too large");
                }
                let offset = bitmap_data.len();
                bitmap_data.extend_from_slice(&bitmap);
                records.push((
                    codepoint,
                    tier as u8,
                    offset,
                    glyph.width,
                    glyph.height,
                    glyph.xmin,
                    glyph.ymin,
                    glyph.advance_width,
                ));
            }
        } // close for &(start, end)

        let bitmap_name = format!("{}_bitmap.bin", face_name.to_ascii_lowercase());
        fs::write(out_dir.join(&bitmap_name), &bitmap_data).expect("write font bitmap atlas");
        // Yol, üretildiği anda sabitlenir: host testleri (`test_host.ps1`) bu
        // kaynağı kendi `OUT_DIR`/ortam değişkenleri olmadan derler.
        let bitmap_path = out_dir
            .join(&bitmap_name)
            .to_string_lossy()
            .replace('\\', "/");
        writeln!(
            generated,
            "pub static {face_name}_BITMAP: &[u8] = include_bytes!(\"{bitmap_path}\");"
        )
        .unwrap();
        writeln!(
            generated,
            "pub static {face_name}_ASCENT: f32 = {:?};",
            metrics.ascent
        )
        .unwrap();
        writeln!(
            generated,
            "pub static {face_name}_LINE_HEIGHT: f32 = {:?};",
            metrics.new_line_size
        )
        .unwrap();
        writeln!(
            generated,
            "pub static {face_name}_GLYPHS: &[GlyphRecord] = &["
        )
        .unwrap();
        for (codepoint, tier, offset, width, height, xmin, ymin, advance) in records {
            writeln!(generated, "GlyphRecord {{ codepoint: {codepoint}, tier: {tier}, offset: {offset}, width: {width}, height: {height}, xmin: {xmin}, ymin: {ymin}, advance: {advance:?} }},").unwrap();
        }
        writeln!(generated, "];\n").unwrap();
    }
}
