include!(concat!(env!("OUT_DIR"), "/font_atlas.rs"));

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

fn ceil_positive(value: f32) -> usize {
    let whole = value.max(0.0) as usize;
    if (whole as f32) < value { whole.saturating_add(1) } else { whole }
}

pub fn text_width(face: Face, text: &str, size: f32) -> usize {
    let (glyphs, _, _, _) = data(face);
    let factor = size / 14.0;
    let mut width = 0.0f32;
    let mut line_width = 0.0f32;
    for character in text.chars() {
        if character == '\n' {
            width = width.max(line_width);
            line_width = 0.0;
            continue;
        }
        let codepoint = if glyph(face, character).is_some() { character as u32 } else { b'?' as u32 };
        if let Ok(index) = glyphs.binary_search_by_key(&codepoint, |record| record.codepoint) {
            line_width += glyphs[index].advance * factor;
        }
    }
    ceil_positive(width.max(line_width))
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
    let (glyphs, bitmap, ascent, line_height) = data(face);
    let factor = size / 14.0;
    let mut pen_x = x as f32;
    let mut baseline = y as f32 + ascent * factor;

    for character in text.chars() {
        if character == '\n' {
            pen_x = x as f32;
            baseline += line_height * factor;
            continue;
        }
        let record = glyph(face, character)
            .or_else(|| glyph(face, '?'));
        let Some(record) = record else { continue };
        let dst_width = ceil_positive(record.width as f32 * factor);
        let dst_height = ceil_positive(record.height as f32 * factor);
        let left = pen_x + record.xmin as f32 * factor;
        let top = baseline - (record.ymin + record.height as i16) as f32 * factor;

        for dy in 0..dst_height {
            let source_y = ((dy as f32 / factor) as usize).min(record.height.saturating_sub(1) as usize);
            for dx in 0..dst_width {
                let source_x = ((dx as f32 / factor) as usize).min(record.width.saturating_sub(1) as usize);
                let alpha = bitmap[record.offset + source_y * record.width as usize + source_x];
                if alpha == 0 { continue; }
                let px = left as i32 + dx as i32;
                let py = top as i32 + dy as i32;
                if px >= 0 && py >= 0 {
                    fb.put_subpixel_text(px as usize, py as usize, color, [alpha; 3]);
                }
            }
        }
        pen_x += record.advance * factor;
    }
}
