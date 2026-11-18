//! Canvas 2D (`browser_canvas`) birim testleri: dolgu, gradyan,
//! save/restore, transform, path (quad/cubic/arc), ImageData.

use crate::browser_canvas::{Canvas2dContext, ImageData, PathCmd, Point};

fn px(ctx: &Canvas2dContext, x: usize, y: usize) -> (u8, u8, u8) {
    ctx.get_pixel(x, y).expect("pixel sınır içinde olmalı")
}

#[test]
fn fill_rect_solid_color() {
    let mut ctx = Canvas2dContext::new(8, 8);
    ctx.set_fill_style(0xFF0000);
    ctx.fill_rect(1.0, 1.0, 4.0, 4.0);
    assert_eq!(px(&ctx, 2, 2), (0xFF, 0x00, 0x00));
    assert_eq!(px(&ctx, 4, 4), (0xFF, 0x00, 0x00));
    // Sınır dışı pikseller dokunulmamış kalır.
    assert_eq!(px(&ctx, 0, 0), (0x00, 0x00, 0x00));
    assert_eq!(px(&ctx, 7, 7), (0x00, 0x00, 0x00));
}

#[test]
fn clear_rect_erases() {
    let mut ctx = Canvas2dContext::new(8, 8);
    ctx.set_fill_style(0x00FF00);
    ctx.fill_rect(0.0, 0.0, 8.0, 8.0);
    ctx.clear_rect(2.0, 2.0, 2.0, 2.0);
    assert_eq!(px(&ctx, 3, 3), (0x00, 0x00, 0x00));
    assert_eq!(px(&ctx, 0, 0), (0x00, 0xFF, 0x00));
}

#[test]
fn linear_gradient_interpolates() {
    let mut ctx = Canvas2dContext::new(16, 4);
    let gi = ctx.create_linear_gradient(0.0, 0.0, 16.0, 0.0);
    ctx.gradient_add_color_stop(gi, 0.0, 0xFF0000);
    ctx.gradient_add_color_stop(gi, 1.0, 0x0000FF);
    ctx.set_fill_gradient(gi);
    ctx.fill_rect(0.0, 0.0, 16.0, 4.0);
    // Sol uç kırmızıya, sağ uç maviye yakın olmalı; ortada mor.
    let left = px(&ctx, 0, 1);
    let right = px(&ctx, 15, 1);
    let mid = px(&ctx, 8, 1);
    assert!(left.0 > left.2, "sol uç kırmızı ağırlıklı: {:?}", left);
    assert!(right.2 > right.0, "sağ uç mavi ağırlıklı: {:?}", right);
    assert!(mid.0 > 60 && mid.2 > 60, "orta mor: {:?}", mid);
}

#[test]
fn gradient_stops_clamp_and_sort() {
    let mut ctx = Canvas2dContext::new(8, 2);
    let gi = ctx.create_linear_gradient(0.0, 0.0, 8.0, 0.0);
    // Ters sırayla eklenen duraklar ofset sırasına dizilir.
    ctx.gradient_add_color_stop(gi, 1.0, 0x0000FF);
    ctx.gradient_add_color_stop(gi, 0.0, 0xFF0000);
    ctx.set_fill_gradient(gi);
    ctx.fill_rect(0.0, 0.0, 8.0, 2.0);
    assert!(px(&ctx, 0, 0).0 > px(&ctx, 7, 0).0);
    // Var olmayan gradyan seçimi dolguyu bozmaz.
    ctx.set_fill_gradient(99);
    ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
}

#[test]
fn save_restore_transform_and_style() {
    let mut ctx = Canvas2dContext::new(16, 16);
    ctx.set_fill_style(0xFF0000);
    ctx.translate(4.0, 4.0);
    ctx.save();
    ctx.set_fill_style(0x0000FF);
    ctx.scale(2.0, 2.0);
    ctx.fill_rect(0.0, 0.0, 2.0, 2.0); // (4,4) köşesinde 4x4 mavi
    assert_eq!(px(&ctx, 5, 5), (0x00, 0x00, 0xFF));
    ctx.restore();
    // restore sonrası stil ve transform geri döner.
    assert_eq!(ctx.fill_style, 0xFF0000);
    assert_eq!(ctx.transform, (1.0, 0.0, 0.0, 1.0, 4.0, 4.0));
    ctx.fill_rect(0.0, 0.0, 2.0, 2.0); // yine (4,4) ama kırmızı
    assert_eq!(px(&ctx, 5, 5), (0xFF, 0x00, 0x00));
    // Boş yığında restore no-op.
    ctx.restore();
}

#[test]
fn path_rect_fill_and_quad_curve() {
    let mut ctx = Canvas2dContext::new(32, 32);
    ctx.set_fill_style(0xFFFFFF);
    ctx.begin_path();
    ctx.rect(4.0, 4.0, 8.0, 8.0);
    ctx.fill();
    assert_eq!(px(&ctx, 8, 8), (0xFF, 0xFF, 0xFF));
    assert_eq!(px(&ctx, 2, 2), (0x00, 0x00, 0x00));

    // İkinci derece Bézier: düz üçgen yerine eğri iz gerekir.
    ctx.set_fill_style(0x00FF00);
    ctx.begin_path();
    ctx.move_to(16.0, 24.0);
    ctx.quadratic_curve_to(16.0, 4.0, 28.0, 24.0);
    ctx.close_path();
    ctx.fill();
    // Eğrinin altındaki alan dolu, üst köşe (eğri üstü) boş kalmalı.
    assert_eq!(px(&ctx, 22, 22), (0x00, 0xFF, 0x00));
    assert_eq!(px(&ctx, 22, 6), (0x00, 0x00, 0x00));
}

#[test]
fn cubic_bezier_and_arc_stroke_do_not_panic() {
    let mut ctx = Canvas2dContext::new(32, 32);
    ctx.set_stroke_style(0xFFFFFF);
    ctx.line_width = 2.0;
    ctx.begin_path();
    ctx.move_to(2.0, 16.0);
    ctx.bezier_curve_to(8.0, 2.0, 24.0, 30.0, 30.0, 16.0);
    ctx.stroke();
    // Yay da stroke tarafında işlenebilmeli.
    ctx.begin_path();
    ctx.arc(16.0, 16.0, 6.0, 0.0, core::f32::consts::TAU, false);
    ctx.stroke();
    // Çember kenarında en az bir parlak piksel olmalı.
    let mut lit = 0usize;
    for y in 8..24 {
        for x in 8..24 {
            let (r, g, b) = px(&ctx, x, y);
            if r > 200 && g > 200 && b > 200 {
                lit += 1;
            }
        }
    }
    assert!(lit > 4, "arc stroke piksel üretmedi: {}", lit);
}

#[test]
fn image_data_roundtrip() {
    let mut ctx = Canvas2dContext::new(8, 8);
    ctx.set_fill_style(0x123456);
    ctx.fill_rect(2.0, 3.0, 3.0, 2.0);
    let img = ctx.get_image_data(1.0, 2.0, 5.0, 4.0);
    assert_eq!((img.width, img.height), (5, 4));
    // (2,3) canvas koordinatı → img (1,1).
    let i = (1 * img.width + 1) * 4;
    assert_eq!(&img.data[i..i + 3], &[0x12, 0x34, 0x56]);
    assert_eq!(img.data[i + 3], 0xFF);

    // putImageData ile başka bölgeye kopyala.
    let mut dst = ImageData::new(4, 4);
    for (j, b) in dst.data.iter_mut().enumerate() {
        *b = if j % 4 == 3 { 0xFF } else { 0xAA };
    }
    ctx.put_image_data(&dst, 0.0, 0.0);
    assert_eq!(px(&ctx, 0, 0), (0xAA, 0xAA, 0xAA));
}

#[test]
fn draw_image_scaled_blits() {
    let mut ctx = Canvas2dContext::new(8, 8);
    let mut img = ImageData::new(2, 2);
    // Sol üst kırmızı, sağ alt mavi.
    img.data[0..4].copy_from_slice(&[0xFF, 0x00, 0x00, 0xFF]);
    img.data[12..16].copy_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);
    ctx.draw_image(&img, 4.0, 4.0);
    assert_eq!(px(&ctx, 4, 4), (0xFF, 0x00, 0x00));
    assert_eq!(px(&ctx, 5, 5), (0x00, 0x00, 0xFF));
    assert_eq!(px(&ctx, 0, 0), (0x00, 0x00, 0x00));
}

#[test]
fn clip_rect_restricts_drawing() {
    let mut ctx = Canvas2dContext::new(16, 16);
    ctx.rect_clip(4.0, 4.0, 4.0, 4.0);
    ctx.set_fill_style(0xFFFFFF);
    ctx.fill_rect(0.0, 0.0, 16.0, 16.0);
    assert_eq!(px(&ctx, 6, 6), (0xFF, 0xFF, 0xFF));
    assert_eq!(px(&ctx, 2, 2), (0x00, 0x00, 0x00));
    assert_eq!(px(&ctx, 12, 12), (0x00, 0x00, 0x00));
    ctx.clear_clip();
    ctx.fill_rect(0.0, 0.0, 2.0, 2.0);
    assert_eq!(px(&ctx, 1, 1), (0xFF, 0xFF, 0xFF));
}

#[test]
fn stroke_rect_outline_only() {
    let mut ctx = Canvas2dContext::new(16, 16);
    ctx.set_stroke_style(0xFFFFFF);
    ctx.line_width = 1.0;
    ctx.stroke_rect(4.0, 4.0, 6.0, 6.0);
    // Kenar pikseli dolu, iç boş.
    assert_eq!(px(&ctx, 4, 5), (0xFF, 0xFF, 0xFF));
    assert_eq!(px(&ctx, 7, 7), (0x00, 0x00, 0x00));
}

#[test]
fn path_commands_recorded() {
    let mut ctx = Canvas2dContext::new(4, 4);
    ctx.begin_path();
    ctx.move_to(0.0, 0.0);
    ctx.line_to(1.0, 1.0);
    ctx.quadratic_curve_to(2.0, 2.0, 3.0, 3.0);
    ctx.bezier_curve_to(1.0, 0.0, 0.0, 1.0, 3.0, 0.0);
    ctx.arc(1.0, 1.0, 1.0, 0.0, 1.0, false);
    ctx.close_path();
    assert_eq!(ctx.path.len(), 6);
    assert!(matches!(ctx.path[2], PathCmd::QuadTo { .. }));
    assert!(matches!(ctx.path[3], PathCmd::CubicTo { .. }));
    assert!(matches!(ctx.path[4], PathCmd::Arc { .. }));
    assert!(matches!(ctx.path[5], PathCmd::ClosePath));
    assert_eq!(ctx.path[0], PathCmd::MoveTo(Point { x: 0.0, y: 0.0 }));
}
