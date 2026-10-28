//! Tarayıcı için ortak veri tipleri ve renk çözümleyici (no_std).
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

pub fn html_color(value: &[u8]) -> Option<u32> {
    let mut v = value;
    while v.first().map_or(false, |b| {
        b.is_ascii_whitespace() || *b == b'\'' || *b == b'"'
    }) {
        v = &v[1..];
    }
    if v.first() == Some(&b'#') && v.len() >= 7 {
        let mut rgb = 0u32;
        for &b in &v[1..7] {
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a' + 10) as u32,
                b'A'..=b'F' => (b - b'A' + 10) as u32,
                _ => return None,
            };
            rgb = (rgb << 4) | d;
        }
        return Some(rgb);
    }
    // #abc kısa formu
    if v.first() == Some(&b'#') && v.len() >= 4 {
        let mut rgb = 0u32;
        for &b in &v[1..4] {
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => (b - b'a' + 10) as u32,
                b'A'..=b'F' => (b - b'A' + 10) as u32,
                _ => return None,
            };
            rgb = (rgb << 4) | d;
        }
        // 4-bit -> 8-bit genişletme (AA -> AA*17)
        return Some(
            (((rgb >> 8) & 0xF) * 0x11) << 16
                | ((((rgb >> 4) & 0xF) * 0x11) << 8)
                | (((rgb) & 0xF) * 0x11),
        );
    }
    // rgb(...) / rgba(...)
    if let Some(lp) = v.iter().position(|&b| b == b'(') {
        let name = core::str::from_utf8(&v[..lp]).unwrap_or("");
        if name.trim().eq_ignore_ascii_case("rgb") || name.trim().eq_ignore_ascii_case("rgba") {
            let rp = v.iter().position(|&b| b == b')').unwrap_or(v.len());
            let s = core::str::from_utf8(&v[lp + 1..rp]).unwrap_or("");
            let mut comps = [0u32; 3];
            let mut n = 0usize;
            for part in s.split(|c| c == ',' || c == ' ' || c == '\t') {
                let t = part.trim();
                if t.is_empty() {
                    continue;
                }
                if n >= 3 {
                    break;
                }
                let num = if let Some(pc) = t.strip_suffix('%') {
                    pc.parse::<f32>().ok()? * 2.55
                } else {
                    t.parse::<f32>().ok()?
                };
                if !num.is_finite() || num < 0.0 || num > 255.0 {
                    return None;
                }
                comps[n] = (num + 0.5) as u32;
                n += 1;
            }
            if n == 3 {
                return Some((comps[0] << 16) | (comps[1] << 8) | comps[2]);
            }
            return None;
        }
    }
    match v
        .split(|b| *b == b';' || b.is_ascii_whitespace() || *b == b'\'' || *b == b'"' || *b == b'(')
        .next()?
    {
        b"black" => Some(0x101010),
        b"white" => Some(0xFFFFFF),
        b"red" => Some(0xD03030),
        b"green" => Some(0x188040),
        b"blue" => Some(0x2050C0),
        b"yellow" => Some(0xD0A020),
        b"gray" | b"grey" => Some(0x808080),
        b"orange" => Some(0xE07020),
        b"purple" => Some(0x8020A0),
        b"cyan" => Some(0x20A0C0),
        b"aqua" => Some(0x20A0C0),
        b"teal" => Some(0x208080),
        b"navy" => Some(0x103060),
        b"maroon" => Some(0x802020),
        b"olive" => Some(0x808020),
        b"lime" => Some(0x40C020),
        b"silver" => Some(0xC0C0C0),
        b"fuchsia" => Some(0xC020C0),
        b"magenta" => Some(0xC020C0),
        b"pink" => Some(0xE06090),
        b"lightgray" => Some(0xD0D0D0),
        b"lightgrey" => Some(0xD0D0D0),
        b"darkgray" => Some(0x505050),
        b"darkgrey" => Some(0x505050),
        b"dimgray" => Some(0x696969),
        b"dimgrey" => Some(0x696969),
        b"whitesmoke" => Some(0xF5F5F5),
        b"goldenrod" => Some(0xDAA520),
        b"tomato" => Some(0xFF6347),
        b"coral" => Some(0xF08060),
        b"salmon" => Some(0xFA8072),
        b"khaki" => Some(0xF0E68C),
        b"violet" => Some(0xEE82EE),
        b"indigo" => Some(0x4B0082),
        b"crimson" => Some(0xDC143C),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HtmlTagType {
    Normal,
    Heading1,
    Heading2,
    Heading3,
    ListItem,
    // Yeni eklenen tag'ler
    Span,  // Inline container
    Em,    // Emphasized text
    Mark,  // Marked/highlighted text
    Small, // Smaller text
    Sub,   // Subscript
    Sup,   // Superscript
    Rule,  // <hr> yatay ayraç çizgisi
    Quote, // <blockquote> — sol kenarlıklı alıntı bloğu
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone)]
pub struct ElementStyle {
    pub color: Option<u32>,
    pub bg_color: Option<u32>,
    pub font_scale: usize,
    pub align: TextAlign,
    pub display_none: bool,
    /// Sol kenar boşluğu (margin-left + padding-left) piksel cinsinden.
    pub indent_px: usize,
    /// Kalın yazı (render tarafında henüz kullanılmıyor, ileride kullanılacak).
    pub bold: bool,
    /// Üstü çizili.
    pub underline: bool,
    /// Altı çizili.
    pub line_through: bool,
    /// Vertical align durumu.
    pub vertical_align: bool,
    /// Satır yüksekliği çarpanı (normal için 1.2, pixel cinsinden için 16px varsayımı).
    pub line_height: f32,
    /// Koluşturma önleme (nowrap).
    pub no_wrap: bool,
    /// Önceden formatlanmış metin (pre).
    pub pre_formatted: bool,
    /// Yön belirtilmesi (float left/right).
    pub float_left: bool,
    pub float_right: bool,
    /// Temizleme (clear: both).
    pub clear_both: bool,
    /// Sınır genliği (px cinsinden).
    pub border_width: usize,
    /// Sınır rengi.
    pub border_color: Option<u32>,
    /// Sınır stili (solid, dashed vb. - basit implementasyon).
    pub border_style: usize,
    /// Sınır radius'i (px cinsinden).
    pub border_radius: usize,
    /// Box shadow rengi.
    pub box_shadow_color: Option<u32>,
    /// Text shadow rengi.
    pub text_shadow_color: Option<u32>,
    /// Liste stili türü.
    pub list_style_type: usize,
    /// Display tipi.
    pub display: Option<usize>,
    /// Konumlama tipi.
    pub position: usize,
    /// Z-index değeri.
    pub z_index: i32,
    /// Overflow durumu.
    pub overflow: usize,
    /// Genişlik (px cinsinden).
    pub width: usize,
    /// Yükseklik (px cinsinden).
    pub height: usize,
    /// Maksimum genişlik (px cinsinden).
    pub max_width: usize,
    /// Maksimum yükseklik (px cinsinden).
    pub max_height: usize,
    /// Minimum genişlik (px cinsinden).
    pub min_width: usize,
    /// Minimum yükseklik (px cinsinden).
    pub min_height: usize,
    /// CSS animation ismi.
    pub animation_name: Option<String>,
    /// CSS animation sure (saniye).
    pub animation_duration: f32,
    /// CSS animation tekrar sayisi (-1 = sonsuz).
    pub animation_iterations: i32,
    /// CSS transition suresi (san, 0 = yok).
    pub transition_duration: f32,
    /// CSS transition property adi.
    pub transition_property: Option<String>,
    /// CSS opacity degeri (1 = tam).
    pub opacity: f32,
}

impl ElementStyle {
    pub fn new() -> Self {
        Self {
            color: None,
            bg_color: None,
            font_scale: 1,
            align: TextAlign::Left,
            display_none: false,
            indent_px: 0,
            bold: false,
            underline: false,
            line_through: false,
            vertical_align: false,
            line_height: 1.2,
            no_wrap: false,
            pre_formatted: false,
            float_left: false,
            float_right: false,
            clear_both: false,
            border_width: 0,
            border_color: None,
            border_style: 0,
            border_radius: 0,
            box_shadow_color: None,
            text_shadow_color: None,
            list_style_type: 0,
            display: None,
            position: 0,
            z_index: 0,
            overflow: 0,
            width: 0,
            height: 0,
            max_width: 0,
            max_height: 0,
            min_width: 0,
            min_height: 0,
            animation_name: None,
            animation_duration: 0.0,
            animation_iterations: -1,
            transition_duration: 0.0,
            transition_property: None,
            opacity: 1.0,
        }
    }

    /// İki stilin birleşimi: son eşleşen kural kazanır.
    #[allow(dead_code)]
    pub fn merge_from(&mut self, other: &ElementStyle) {
        if let Some(c) = other.color {
            self.color = Some(c);
        }
        if let Some(bg) = other.bg_color {
            self.bg_color = Some(bg);
        }
        if other.font_scale > 1 {
            self.font_scale = other.font_scale;
        }
        if other.align != TextAlign::Left {
            self.align = other.align;
        }
        if other.display_none {
            self.display_none = true;
        }
        if other.indent_px > 0 {
            self.indent_px = other.indent_px;
        }
        if other.bold {
            self.bold = true;
        }
        if other.underline {
            self.underline = true;
        }
        if other.line_through {
            self.line_through = true;
        }
        if other.vertical_align {
            self.vertical_align = other.vertical_align;
        }
        if other.line_height > 0.0 {
            self.line_height = other.line_height;
        }
        if other.no_wrap {
            self.no_wrap = true;
        }
        if other.pre_formatted {
            self.pre_formatted = true;
        }
        if other.float_left {
            self.float_left = true;
        }
        if other.float_right {
            self.float_right = true;
        }
        if other.clear_both {
            self.clear_both = true;
        }
        if other.border_width > 0 {
            self.border_width = other.border_width;
        }
        if let Some(c) = other.border_color {
            self.border_color = Some(c);
        }
        if other.border_style > 0 {
            self.border_style = other.border_style;
        }
        if other.border_radius > 0 {
            self.border_radius = other.border_radius;
        }
        if let Some(c) = other.box_shadow_color {
            self.box_shadow_color = Some(c);
        }
        if let Some(c) = other.text_shadow_color {
            self.text_shadow_color = Some(c);
        }
        if other.list_style_type > 0 {
            self.list_style_type = other.list_style_type;
        }
        if other.display.is_some() {
            self.display = other.display;
        }
        if other.position > 0 {
            self.position = other.position;
        }
        if other.z_index != 0 {
            self.z_index = other.z_index;
        }
        if other.overflow > 0 {
            self.overflow = other.overflow;
        }
        if other.width > 0 {
            self.width = other.width;
        }
        if other.height > 0 {
            self.height = other.height;
        }
        if other.max_width > 0 {
            self.max_width = other.max_width;
        }
        if other.max_height > 0 {
            self.max_height = other.max_height;
        }
        if other.min_width > 0 {
            self.min_width = other.min_width;
        }
        if other.min_height > 0 {
            self.min_height = other.min_height;
        }
        if let Some(name) = &other.animation_name {
            self.animation_name = Some(name.clone());
        }
        if other.animation_duration > 0.0 {
            self.animation_duration = other.animation_duration;
        }
        if other.animation_iterations != -1 {
            self.animation_iterations = other.animation_iterations;
        }
        if other.transition_duration > 0.0 {
            self.transition_duration = other.transition_duration;
        }
        if let Some(prop) = &other.transition_property {
            self.transition_property = Some(prop.clone());
        }
        if other.opacity > 0.0 {
            self.opacity = other.opacity;
        }
    }
}

#[derive(Clone)]
pub struct TextSpan {
    pub text: String,
    pub color: u32,
    pub bg_color: Option<u32>,
    pub is_link: bool,
    pub link_url: String,
    pub is_button: bool,
    pub is_image: bool,
    pub onclick: String,
    pub elem_id: String,
    /// CSS `font-weight: bold` karşılığı (sentez kalın çizim).
    pub bold: bool,
    /// CSS `text-decoration: underline`.
    pub underline: bool,
}

#[derive(Clone)]
pub struct BrowserLine {
    pub spans: Vec<TextSpan>,
    pub tag: HtmlTagType,
    pub indent: usize,
    pub align: TextAlign,
}

#[derive(Clone)]
pub struct HttpCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

#[derive(Clone)]
pub struct RenderedLink {
    pub rect: (usize, usize, usize, usize),
    pub url: String,
    pub onclick: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub title: &'static str,
    pub title_en: &'static str,
    pub url: &'static str,
    pub description: &'static str,
    pub description_en: &'static str,
}

pub const BOOKMARKS: &[Bookmark] = &[
    Bookmark {
        title: "HttpForever",
        title_en: "HttpForever",
        url: "http://httpforever.com/",
        description: "Hafif ve saf HTTP test sitesi (port 80).",
        description_en: "Lightweight and pure HTTP test site (port 80).",
    },
    Bookmark {
        title: "Example (HTTPS)",
        title_en: "Example (HTTPS)",
        url: "https://example.com/",
        description: "Güvenli HTTPS/TLS test sitesi (port 443).",
        description_en: "Secure HTTPS/TLS test site (port 443).",
    },
    Bookmark {
        title: "KediWiki",
        title_en: "KediWiki",
        url: "minos://cats",
        description: "Yerel kediler ve ansiklopedi sayfası.",
        description_en: "Local cat wiki and encyclopedia page.",
    },
    Bookmark {
        title: "Ağ Durumu",
        title_en: "Network",
        url: "minos://net",
        description: "e1000 kartı, ARP ve ağ tanılama aracı.",
        description_en: "e1000 card, ARP and network diagnostics tool.",
    },
    Bookmark {
        title: "Geçmiş",
        title_en: "History",
        url: "minos://history",
        description: "Ziyaret edilen web sayfaları geçmişi.",
        description_en: "History of visited web pages.",
    },
    Bookmark {
        title: "Yardım",
        title_en: "Help",
        url: "minos://help",
        description: "minOs web tarayıcısı kullanım kılavuzu.",
        description_en: "minOs web browser user guide.",
    },
    Bookmark {
        title: "Web Testi",
        title_en: "Web Demo",
        url: "minos://demo",
        description: "CSS, DOM ve JavaScript etkileşim testi.",
        description_en: "CSS, DOM and JavaScript interaction test.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_httpforever_bookmark() {
        assert!(!BOOKMARKS.is_empty());
        let first = &BOOKMARKS[0];
        assert_eq!(first.title, "HttpForever");
        assert_eq!(first.url, "http://httpforever.com/");
        assert!(BOOKMARKS.iter().any(|b| b.url == "http://httpforever.com/"));
    }

    #[test]
    fn test_html_colors() {
        assert_eq!(html_color(b"#ff0000"), Some(0xff0000));
        assert_eq!(html_color(b"red"), Some(0xd03030));
    }
}
