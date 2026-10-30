//! MIME tür yönetimi — dosya uzantısına göre MIME türü eşleştirme ve içerik türü tespiti.
//! no_std, heap-allocated.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;

/// MIME tür eşleştirmesi: uzantı → MIME türü.
/// Temel web dosya türleri için kapsamlı liste.
const MIME_TABLE: &[(&str, &str); 100] = &[
    // Resimler
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("svg", "image/svg+xml"),
    ("svgz", "image/svg+xml"),
    ("ico", "image/x-icon"),
    ("bmp", "image/bmp"),
    ("tiff", "image/tiff"),
    ("tif", "image/tiff"),
    ("apng", "image/apng"),

    // Ses
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("oga", "audio/ogg"),
    ("aac", "audio/aac"),
    ("flac", "audio/flac"),
    ("mid", "audio/midi"),
    ("midi", "audio/midi"),
    ("opus", "audio/opus"),
    ("weba", "audio/webm"),

    // Video
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("ogv", "video/ogg"),
    ("mov", "video/quicktime"),
    ("avi", "video/x-msvideo"),
    ("wmv", "video/x-ms-wmv"),
    ("flv", "video/x-flv"),
    ("mkv", "video/x-matroska"),

    // Metin
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("js", "application/javascript"),
    ("mjs", "application/javascript"),
    ("json", "application/json"),
    ("txt", "text/plain"),
    ("csv", "text/csv"),
    ("xml", "application/xml"),
    ("xhtml", "application/xhtml+xml"),
    ("rtf", "application/rtf"),
    ("md", "text/markdown"),

    // Uygulama
    ("pdf", "application/pdf"),
    ("zip", "application/zip"),
    ("gz", "application/gzip"),
    ("bz2", "application/x-bzip2"),
    ("7z", "application/x-7z-compressed"),
    ("tar", "application/x-tar"),
    ("rar", "application/x-rar-compressed"),
    ("exe", "application/x-msdownload"),
    ("dll", "application/x-msdownload"),
    ("bin", "application/octet-stream"),
    ("dat", "application/octet-stream"),
    ("iso", "application/x-iso9660-image"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
    ("eot", "application/vnd.ms-fontobject"),

    // Diğer
    ("wasm", "application/wasm"),
    ("manifest", "text/cache-manifest"),
    ("appcache", "text/cache-manifest"),
    ("crx", "application/x-chrome-extension"),
    ("xpi", "application/x-xpinstall"),
    ("safariextz", "application/x-safari-extension"),
    ("webapp", "application/x-web-app-manifest+json"),
    ("vcf", "text/x-vcard"),
    ("ics", "text/calendar"),
    ("kml", "application/vnd.google-earth.kml+xml"),
    ("kmz", "application/vnd.google-earth.kmz"),
    ("ttl", "text/turtle"),
    ("nt", "application/n-triples"),
    ("nq", "application/n-quads"),
    ("rdf", "application/rdf+xml"),
    ("owl", "application/rdf+xml"),
    ("jsonld", "application/ld+json"),
    ("yaml", "text/yaml"),
    ("yml", "text/yaml"),
    ("toml", "text/toml"),
    ("ini", "text/plain"),
    ("log", "text/plain"),
    ("md5", "text/plain"),
    ("sha1", "text/plain"),
    ("sha256", "text/plain"),
    ("sig", "application/pgp-signature"),
    ("pgp", "application/pgp-encrypted"),
    ("asc", "application/pgp-encrypted"),
]

/// Dosya uzantısından MIME türünü döndürür.
/// Uzantı nokta ile başlayabilir veya olmayabilir (örn. "html" veya ".html").
/// Bulamazsa "application/octet-stream" döner.
pub fn mime_type_from_extension(ext: &str) -> &str {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    for &(ext_pattern, mime) in MIME_TABLE {
        if ext == ext_pattern {
            return mime;
        }
    }
    "application/octet-stream"
}

/// Dosya yolundan MIME türünü döndürür.
/// Dosya uzantısına göre MIME türünü tespit eder.
pub fn mime_type_from_path(path: &str) -> &str {
    let ext = path.rfind('.').map(|i| &path[i + 1..]).unwrap_or("");
    mime_type_from_extension(ext)
}

/// Content-Type başlığı değerini döndürür.
pub fn content_type_header(ext: &str) -> String {
    format!("Content-Type: {}", mime_type_from_extension(ext))
}

/// İçerik türünü (Content-Type başlığı) MIME tür ve charset'e ayırır.
/// Döner: (mime_type, charset)
pub fn parse_content_type(content_type: &str) -> (&str, Option<&str>) {
    let parts: Vec<&str> = content_type.split(';').collect();
    let mime = parts.first().unwrap_or(&"").trim();
    let charset = parts.iter()
        .skip(1)
        .find_map(|p| {
            let p = p.trim();
            if let Some((key, val)) = p.split_once('=') {
                if key.trim().eq_ignore_ascii_case("charset") {
                    return Some(val.trim().trim_matches('"'));
                }
            }
            None
        });
    (mime, charset)
}

/// MIME türünün metin mi yoksa ikili mi olduğunu belirler.
pub fn is_text_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/xml"
        || mime == "application/xhtml+xml"
        || mime == "application/javascript"
        || mime.starts_with("image/svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_from_extension() {
        assert_eq!(mime_type_from_extension("html"), "text/html");
        assert_eq!(mime_type_from_extension(".html"), "text/html");
        assert_eq!(mime_type_from_extension("png"), "image/png");
        assert_eq!(mime_type_from_extension("json"), "application/json");
        assert_eq!(mime_type_from_extension("unknown"), "application/octet-stream");
        assert_eq!(mime_type_from_extension("HTML"), "text/html"); // Büyük harf
    }

    #[test]
    fn test_mime_from_path() {
        assert_eq!(mime_type_from_path("/index.html"), "text/html");
        assert_eq!(mime_type_from_path("/image.png"), "image/png");
        assert_eq!(mime_type_from_path("/noextension"), "application/octet-stream");
    }

    #[test]
    fn test_content_type_header() {
        let header = content_type_header("css");
        assert_eq!(header, "Content-Type: text/css");
    }

    #[test]
    fn test_parse_content_type() {
        let (mime, charset) = parse_content_type("text/html; charset=utf-8");
        assert_eq!(mime, "text/html");
        assert_eq!(charset, Some("utf-8"));

        let (mime, charset) = parse_content_type("application/json");
        assert_eq!(mime, "application/json");
        assert_eq!(charset, None);
    }

    #[test]
    fn test_is_text_mime() {
        assert!(is_text_mime("text/html"));
        assert!(is_text_mime("application/json"));
        assert!(!is_text_mime("image/png"));
        assert!(!is_text_mime("application/octet-stream"));
    }
}
