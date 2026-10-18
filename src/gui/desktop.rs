//! Desktop background and quick-icon strip.

use super::*;

impl MinGuiCompositor {
    // -------------------------------------------------------------------
    // Masaüstü sol şerit hızlı simgeleri — TEK kanonik kaynak.
    // Hem çizim (`render_desktop_bg`) hem tıklama testi bu fonksiyonları
    // kullanır; /desktop dosya ızgarası da şeridin sağından başlar, böylece
    // klasör simgeleri hızlı simgelerin üzerine binmez.
    // -------------------------------------------------------------------
    /// Hızlı simge etiketleri (TR + EN). Şerit genişliği dile göre değişmesin
    /// diye her iki dilin en uzun etiketi ölçülür.
    const DESKTOP_QUICK_LABELS: [&str; 12] = [
        "Bilgisayarım", "Notlar", "SarmanNet", "Çalma Listesi", "Sistem Ayarları", "minOs'u Kur",
        "Computer", "Notes", "SarmanNet", "Playlist", "Settings", "Install minOs",
    ];

    pub fn desktop_quick_count() -> usize {
        Self::DESKTOP_QUICK_LABELS.len() / 2
    }

    /// Hızlı simge etiketlerinin ölçülen en uzun genişliği (aktif ölçekte).
    fn desktop_quick_label_w() -> usize {
        let mut m = 0;
        for l in Self::DESKTOP_QUICK_LABELS {
            m = m.max(Self::text_width(l, 1));
        }
        m
    }

    /// Hızlı simgenin tıklanabilir alanı (kart + etiket).
    pub fn desktop_quick_rect(i: usize) -> (usize, usize, usize, usize) {
        let card_w = sp(34);
        let card_h = sp(30);
        let w = card_w + sp(10) + Self::desktop_quick_label_w() + sp(12);
        (sp(20), sp(30) + i * sp(54), w, card_h)
    }

    /// Hızlı simge şeridinin sağ kenarı. `/desktop` dosya ızgarası bu
    /// konumun sağından başlar; iki simge grubu asla çakışmaz.
    pub fn desktop_strip_right() -> usize {
        sp(20) + sp(34) + sp(10) + Self::desktop_quick_label_w() + sp(16)
    }

    /// Sağ üst filigran (watermark) alanı. Dosya ızgarası bu köşeye taşmaz.
    pub fn desktop_watermark_rect(width: usize) -> (usize, usize, usize, usize) {
        let title = if crate::fs::diskfs::is_installed() {
            "minOs Sarman Edition *"
        } else {
            "minOs (Live)"
        };
        let block_w = Self::icon_size() + sp(8) + Self::text_width(title, 1);
        let x = width.saturating_sub(sp(16) + block_w);
        (x, 0, block_w + sp(16), sp(70))
    }
    // -----------------------------------------------------------------------
    // 1. Masaüstü Arka Planı
    // -----------------------------------------------------------------------
    pub fn render_desktop_bg(&self, fb: &mut Framebuffer) {
        // Arka plan: Ayarlar'dan seçilen duvar kağıdı (düz renk / örnek desen).
        wallpaper::draw(fb);

        // Masaüstü Sağ Üst minOs Kedi Logosu ve Başlığı (Watermark)
        // Metin genişliğine göre sağa hizalanır; sağ kenardan taşmaz.
        let title = if crate::fs::diskfs::is_installed() {
            "minOs Sarman Edition *"
        } else {
            "minOs (Live)"
        };
        let title_w = Self::text_width(title, 1);
        let icon_w = Self::icon_size();
        let gap = sp(8);
        let margin_right = sp(16);
        let block_w = icon_w + gap + title_w;
        let block_x = fb.width.saturating_sub(margin_right + block_w);
        let title_y = sp(35);
        Self::draw_cat_icon(fb, block_x, sp(30));
        Self::draw_string_shadow(
            fb,
            block_x + icon_w + gap,
            title_y,
            title,
            Color::ACCENT_PINK,
            Color::SHADOW,
            1,
        );

        // Masaüstü Hızlı İkonları (Sol Kenar)
        let desk_icons = if ui_english() { [
            ("Computer", 0), ("Notes", 1),
            ("SarmanNet", 2), ("Playlist", 3),
            ("Settings", 4), ("Install minOs", 0),
        ] } else { [
            ("Bilgisayarım", 0), ("Notlar", 1),
            ("SarmanNet", 2), ("Çalma Listesi", 3),
            ("Sistem Ayarları", 4), ("minOs'u Kur", 0),
        ] };

        let card_w = sp(34);
        // Kurulu sistemde "minOs'u Kur" masaüstü kısayolu gösterilmez.
        let desk_icon_count = Self::desktop_quick_count();
        for (i, (lbl, icon_idx)) in desk_icons.iter().enumerate().take(desk_icon_count) {
            // Tıklama testiyle aynı kanonik konum (kayma/çakışma olmaz).
            let (card_x, dy, _hit_w, card_h) = Self::desktop_quick_rect(i);
            fb.draw_rounded_rect_alpha(card_x, dy, card_w, card_h, 6, Self::shade(Color::WINDOW_BG, 8), 128);
            fb.draw_rounded_rect_outline(card_x, dy, card_w, card_h, 6, Color::BORDER);
            let isz = Self::icon_size();
            let inner_x = card_x + (card_w.saturating_sub(isz)) / 2;
            let inner_y = dy + (card_h.saturating_sub(isz)) / 2;
            Self::draw_icon_by_index(fb, inner_x, inner_y, *icon_idx);
            // Etiket, ikon kartına dikey ortalı ve net boşlukla çizilir (yarı saydam arkaplan).
            let label_x = card_x + card_w + sp(10);
            let label_y = dy + card_h.saturating_sub(Self::text_height()) / 2;
            let tw = Self::text_width(lbl, 1);
            let th = Self::text_height();
            fb.draw_rounded_rect_alpha(label_x.saturating_sub(sp(4)), label_y.saturating_sub(sp(2)), tw + sp(8), th + sp(4), 4, Self::shade(Color::WINDOW_BG, 8), 128);
            fb.draw_rounded_rect_outline(label_x.saturating_sub(sp(4)), label_y.saturating_sub(sp(2)), tw + sp(8), th + sp(4), 4, Color::BORDER);
            Self::draw_string(fb, label_x, label_y, lbl, Color::TEXT_MAIN);
        }
    }
}
