use fontdue::{Font, FontSettings};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

const FONT_SIZE: f32 = 14.0;

struct FaceOutput {
    name: &'static str,
    path: &'static str,
    font: Font,
}

fn main() {
    println!("cargo:rustc-check-cfg=cfg(minos_boot_log)");
    // Host testleri (`tests/host_tests.rs`) `fs::block` modülünü dogrudan
    // `#[path]` ile baglar; o derlemede `cfg(test)` ayarlı olmadigi icin
    // bellek ici blok aygiti ayri bir cfg ile acilir.
    println!("cargo:rustc-check-cfg=cfg(host_fs_tests)");
    let faces = [
        FaceOutput {
            name: "INRIA",
            path: "public/fonts/InriaSans-Regular.ttf",
            font: Font::from_bytes(
                fs::read("public/fonts/InriaSans-Regular.ttf").expect("read Inria Sans"),
                FontSettings::default(),
            )
            .expect("parse Inria Sans"),
        },
        FaceOutput {
            name: "GEIST",
            path: "public/fonts/GeistMono-Regular.ttf",
            font: Font::from_bytes(
                fs::read("public/fonts/GeistMono-Regular.ttf").expect("read Geist Mono"),
                FontSettings::default(),
            )
            .expect("parse Geist Mono"),
        },
    ];
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let mut generated =
        File::create(out_dir.join("font_atlas.rs")).expect("create font atlas source");
    writeln!(generated, "pub struct GlyphRecord {{ pub codepoint: u32, pub offset: usize, pub width: u16, pub height: u16, pub xmin: i16, pub ymin: i16, pub advance: f32 }}").unwrap();

    for face in faces {
        println!("cargo:rerun-if-changed={}", face.path);
        let metrics = face
            .font
            .horizontal_line_metrics(FONT_SIZE)
            .expect("font line metrics");
        let mut bitmap_data = Vec::new();
        let mut records = Vec::new();

        for codepoint in 0x20..=0x10_FFFF {
            let Some(character) = char::from_u32(codepoint) else {
                continue;
            };
            if !face.font.has_glyph(character) {
                continue;
            }
            let (glyph, bitmap) = face.font.rasterize(character, FONT_SIZE);
            if bitmap_data.len() + bitmap.len() > u32::MAX as usize {
                panic!("font atlas is too large");
            }
            let offset = bitmap_data.len();
            bitmap_data.extend_from_slice(&bitmap);
            records.push((
                codepoint,
                offset,
                glyph.width,
                glyph.height,
                glyph.xmin,
                glyph.ymin,
                glyph.advance_width,
            ));
        }

        let bitmap_name = format!("{}_bitmap.bin", face.name.to_ascii_lowercase());
        fs::write(out_dir.join(&bitmap_name), &bitmap_data).expect("write font bitmap atlas");
        writeln!(
            generated,
            "pub static {}_BITMAP: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{}\"));",
            face.name, bitmap_name
        )
        .unwrap();
        writeln!(
            generated,
            "pub static {}_ASCENT: f32 = {:?};",
            face.name, metrics.ascent
        )
        .unwrap();
        writeln!(
            generated,
            "pub static {}_LINE_HEIGHT: f32 = {:?};",
            face.name, metrics.new_line_size
        )
        .unwrap();
        writeln!(
            generated,
            "pub static {}_GLYPHS: &[GlyphRecord] = &[",
            face.name
        )
        .unwrap();
        for (codepoint, offset, width, height, xmin, ymin, advance) in records {
            writeln!(generated, "GlyphRecord {{ codepoint: {codepoint}, offset: {offset}, width: {width}, height: {height}, xmin: {xmin}, ymin: {ymin}, advance: {advance:?} }},").unwrap();
        }
        writeln!(generated, "];\n").unwrap();
    }
}
