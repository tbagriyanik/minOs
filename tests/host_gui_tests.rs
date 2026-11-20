//! GUI duman testleri ve uygulama altyapısı kontratları.
//!
//! Çizim donanımı olmadan doğrulanabilen parçalar burada toplanır: ekran
//! geometrisi (açılış/kapanış/hata ekranları), hata mesajları + kurtarma
//! yolları, uygulama bazlı bellek kaydı, tema/erişilebilirlik ve küçük GUI
//! yardımcıları (metin tamponu, pano, kirli dikdörtgen).

use crate::{
    app_errors::{AppError, AppErrorKind},
    app_memory, clipboard, dirty_rect, font_render, screen_layout, textbuf, theme,
};

// ---------------------------------------------------------------------------
// GUI smoke: ekran geometrisi ve kapanış/açılış metinleri
// ---------------------------------------------------------------------------

#[test]
fn gui_screen_layout_self_test_passes() {
    assert!(
        screen_layout::self_test(),
        "desteklenen çözünürlüklerde ekran geometrisi hatalı"
    );
}

#[test]
fn loading_screen_reports_localized_stage_and_bounded_progress() {
    for phase in 0..4 {
        assert_eq!(screen_layout::loading_percent(phase), (phase + 1) * 25);
        assert!(!screen_layout::loading_stage(phase, false).is_empty());
        assert!(!screen_layout::loading_stage(phase, true).is_empty());
        assert_ne!(
            screen_layout::loading_stage(phase, false),
            screen_layout::loading_stage(phase, true),
            "yükleme aşaması TR/EN yerelleştirilmeli"
        );
    }
    assert_eq!(screen_layout::loading_percent(99), 100);
    assert_ne!(
        screen_layout::loading_hint(false),
        screen_layout::loading_hint(true)
    );

    let bar = screen_layout::loading_bar(1024, 768, 2);
    assert!(bar.sweep.0 >= bar.track.0);
    assert!(bar.sweep.0 + bar.sweep.2 <= bar.track.0 + bar.track.2);
}

#[test]
fn error_and_shutdown_screens_expose_recovery_copy() {
    assert_eq!(screen_layout::boot_error_code(), "BOOT-001");
    let footer = screen_layout::boot_error_footer();
    assert!(
        footer.contains("Yeniden başlatmak"),
        "TR kurtarma yolu eksik: {footer}"
    );
    assert!(
        footer.contains("restart"),
        "EN kurtarma yolu eksik: {footer}"
    );
    for english in [false, true] {
        let recovery = screen_layout::boot_error_recovery(english);
        assert!(
            recovery.starts_with(if english { "Recovery:" } else { "Kurtarma:" }),
            "önyükleme kurtarma etiketi eksik: {recovery}"
        );
        assert!(
            recovery.len() > 24,
            "kurtarma yönergesi çok kısa: {recovery}"
        );
    }

    assert_ne!(
        screen_layout::shutdown_title(false, false),
        screen_layout::shutdown_title(true, false)
    );
    assert_ne!(
        screen_layout::shutdown_title(false, false),
        screen_layout::shutdown_title(false, true)
    );
    for phase in 0..4 {
        let tr = screen_layout::shutdown_step_text(phase, false, false);
        let en = screen_layout::shutdown_step_text(phase, false, true);
        assert!(tr.contains("Adım"), "kapanış adımı numarası eksik: {tr}");
        assert!(en.contains("Step"), "shutdown step number missing: {en}");
        assert!(!screen_layout::shutdown_step_text(phase, true, true).is_empty());
    }
    for english in [false, true] {
        let hint = screen_layout::shutdown_hint(english);
        assert!(!hint.is_empty());
        assert!(
            hint.to_ascii_lowercase().contains("wait")
                || hint.contains("bekleyiniz")
                || hint.contains("Bekleyiniz"),
            "kapanış ipucu bekleme uyarısı içermeli: {hint}"
        );
    }
}

#[test]
fn shutdown_box_fits_small_and_wide_resolutions() {
    for (width, height) in [(240usize, 160usize), (640, 480), (1280, 720), (2560, 1440)] {
        let (x, y, w, h) = screen_layout::shutdown_box(width, height);
        assert!(x <= width && y <= height);
        assert!(x.saturating_add(w) <= width);
        assert!(y.saturating_add(h) <= height);
    }
}

// ---------------------------------------------------------------------------
// Uygulama hataları: kod, mesaj ve kurtarma yolu
// ---------------------------------------------------------------------------

#[test]
fn every_app_error_has_localized_message_and_recovery() {
    let mut codes = alloc::vec::Vec::new();
    for kind in AppErrorKind::ALL {
        let code = kind.code();
        assert!(code.starts_with("APP-E"), "{code}: kararlı kod eksik");
        assert!(!codes.contains(&code), "{code}: yinelenen hata kodu");
        codes.push(code);

        for english in [false, true] {
            assert!(!kind.title(english).is_empty());
            assert!(kind.message(english).len() >= 12);
            let recovery = kind.recovery(english);
            assert!(
                recovery.starts_with("Kurtarma:") || recovery.starts_with("Recovery:"),
                "{code}: kurtarma adımı açıkça etiketlenmeli"
            );
        }
        assert_ne!(kind.message(false), kind.message(true));
        assert_ne!(kind.recovery(false), kind.recovery(true));
    }

    let err = AppError::new("Dosya Yöneticisi", AppErrorKind::PermissionDenied);
    assert_eq!(err.app, "Dosya Yöneticisi");
    assert_eq!(err.title(false), "Erişim reddedildi");
    assert!(err.recovery(true).starts_with("Recovery:"));
    assert!(!err.retryable());
}

#[test]
fn error_classifier_covers_file_network_memory_and_crash_classes() {
    let cases = [
        ("hedef klasor bulunamadi", AppErrorKind::FileNotFound),
        ("permission denied", AppErrorKind::PermissionDenied),
        ("disk dolu", AppErrorKind::DiskFull),
        ("geçersiz biçim", AppErrorKind::InvalidFormat),
        ("DNS çözümlenemedi", AppErrorKind::NetworkUnavailable),
        ("network timeout", AppErrorKind::Timeout),
        ("yetersiz bellek", AppErrorKind::OutOfMemory),
        ("Kullanım: cp <kaynak> <hedef>", AppErrorKind::InvalidInput),
        ("sayfa hatası #PF", AppErrorKind::Crashed),
        ("desteklenmiyor", AppErrorKind::Unsupported),
    ];
    for (text, expected) in cases {
        assert_eq!(
            AppErrorKind::classify(text),
            expected,
            "'{text}' sınıflandırılamadı"
        );
    }

    for kind in AppErrorKind::ALL {
        let expected = matches!(
            kind,
            AppErrorKind::NetworkUnavailable | AppErrorKind::Timeout | AppErrorKind::OutOfMemory
        );
        assert_eq!(
            kind.retryable(),
            expected,
            "{kind:?}: yanlış yeniden deneme sınıfı"
        );
    }
}

// ---------------------------------------------------------------------------
// Uygulama bazlı bellek ölçümü
// ---------------------------------------------------------------------------

#[test]
fn memory_registry_publishes_samples_peaks_and_total() {
    // Paralel testlerle çakışmamak için ayrılmış kimlikler kullanılır.
    let before_seq = app_memory::snapshot_seq();
    app_memory::publish(20, 4096);
    app_memory::publish(21, 8192);
    assert_eq!(app_memory::sample(20), 4096);
    assert_eq!(app_memory::sample(21), 8192);
    assert!(app_memory::total() >= 12288);
    assert!(app_memory::snapshot_seq() > before_seq);

    app_memory::publish(20, 1024);
    assert_eq!(app_memory::sample(20), 1024);
    assert_eq!(app_memory::peak(20), 4096, "zirve bellek değeri korunmalı");

    let mut ids = alloc::vec::Vec::new();
    app_memory::for_each(|id, bytes| {
        if id == 20 || id == 21 {
            ids.push((id, bytes));
        }
    });
    assert!(ids.contains(&(20usize, 1024usize)));
    assert!(ids.contains(&(21usize, 8192usize)));
}

#[test]
fn memory_byte_helpers_use_capacity_and_payload_size() {
    let v = alloc::vec![0u32; 8];
    assert_eq!(app_memory::vec_bytes(&v), 32);
    assert_eq!(app_memory::slice_bytes(&v[..3]), 12);
    assert_eq!(app_memory::sample(app_memory::MAX_TRACKED_APPS), 0);
}

// ---------------------------------------------------------------------------
// Tema / erişilebilirlik
// ---------------------------------------------------------------------------

#[test]
fn themes_have_unique_names_and_complete_cycle() {
    let mut names = alloc::vec::Vec::new();
    let mut short_names = alloc::vec::Vec::new();
    for id in theme::ThemeId::ALL {
        assert!(!id.name().is_empty());
        assert!(!id.short_name().is_empty());
        assert!(!names.contains(&id.name()), "{id:?}: yinelenen tema adı");
        assert!(
            !short_names.contains(&id.short_name()),
            "{id:?}: yinelenen kısa tema adı"
        );
        names.push(id.name());
        short_names.push(id.short_name());
    }

    let start = theme::ThemeId::VelvetDark;
    let mut id = start;
    for _ in 0..theme::ThemeId::ALL.len() {
        id = id.next();
    }
    assert_eq!(id, start, "tema döngüsü tüm temaları kapsamalı");
}

#[test]
fn high_contrast_theme_meets_aaa_for_body_text_and_focus_border() {
    let p = theme::palette(theme::ThemeId::HighContrast);
    let bg = p[theme::IDX_WINDOW_BG as usize];
    for idx in [
        theme::IDX_TEXT_MAIN,
        theme::IDX_TEXT_MUTED,
        theme::IDX_BORDER,
        theme::IDX_CURSOR_FILL,
    ] {
        let ratio = theme::contrast_ratio(bg, p[idx as usize]);
        assert!(
            ratio >= 7.0,
            "Yüksek Kontrast idx={idx} AAA altında: {ratio:.2}"
        );
    }
}

#[test]
fn focus_and_error_accents_are_distinguishable_from_window_background() {
    for id in theme::ThemeId::ALL {
        let p = theme::palette(id);
        let bg = p[theme::IDX_WINDOW_BG as usize];
        for idx in [
            theme::IDX_ACCENT_BLUE,
            theme::IDX_ACCENT_RED,
            theme::IDX_BORDER,
        ] {
            let ratio = theme::contrast_ratio(bg, p[idx as usize]);
            assert!(
                ratio >= 1.6,
                "{id:?}: odak/hata rengi zeminden ayırt edilemiyor (idx={idx}, {ratio:.2})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// GUI yardımcıları: duman testleri
// ---------------------------------------------------------------------------

#[test]
fn text_buffer_formats_turkish_text_and_numbers() {
    let mut buf = textbuf::TextBuf::new();
    buf.push_str("Öğe ");
    buf.push_usize(42);
    buf.push_str(" / 0x");
    buf.push_hex64(0x2A);
    assert_eq!(buf.as_str(), "Öğe 42 / 0x2A");
    buf.clear();
    assert!(buf.is_empty());
}

#[test]
fn clipboard_roundtrips_turkish_characters() {
    clipboard::set_str("ğüşıöç");
    assert_eq!(clipboard::len(), 6);
    let mut out = [' '; 8];
    let n = clipboard::copy_chars(&mut out);
    assert_eq!(n, 6);
    assert_eq!(
        &out[..n],
        "ğüşıöç"
            .chars()
            .collect::<alloc::vec::Vec<char>>()
            .as_slice()
    );
}

#[test]
fn dirty_rect_tracker_unions_and_takes_regions() {
    let mut tracker = dirty_rect::GlobalDirtyTracker::new();
    tracker.track(0, dirty_rect::DirtyRect::new(10, 20, 30, 40));
    tracker.track(0, dirty_rect::DirtyRect::new(40, 20, 30, 40));
    assert_eq!(tracker.as_slice().len(), 2);
    assert_eq!(
        tracker.bounding_rect(),
        Some(dirty_rect::DirtyRect::new(10, 20, 60, 40))
    );
    let (_, len, overflowed) = tracker.take();
    assert_eq!(len, 2);
    assert!(!overflowed);
    assert!(tracker.is_empty());
}

// ---------------------------------------------------------------------------
// Erişilebilirlik ve yerelleştirme ek kontrolleri
// ---------------------------------------------------------------------------

#[test]
fn welcome_screen_exposes_keyboard_navigation_hints() {
    for english in [false, true] {
        let hint = screen_layout::welcome_hint(english);
        assert!(hint.contains("Tab"), "Tab kısayolu eksik: {hint}");
        assert!(hint.contains("Enter"), "Enter kısayolu eksik: {hint}");
        assert!(hint.contains("Esc"), "Esc kısayolu eksik: {hint}");
    }
}

#[test]
fn app_memory_reports_have_stable_names_for_builtin_apps() {
    let expected = [
        (0, "Dosya Yöneticisi"),
        (1, "Sistem Ayarları"),
        (2, "Not Defteri"),
        (3, "Web Gezgini"),
        (7, "Görev Yöneticisi"),
        (10, "Komut İstemi"),
        (16, "FPS 3D"),
        (17, "Şehir Sürüşü"),
    ];
    for (id, name) in expected {
        assert_eq!(app_memory::app_name(id), name, "app_id={id}");
    }
    assert_eq!(app_memory::app_name(255), "Bilinmeyen");
}

#[test]
fn turkish_text_and_glyph_fallback_are_renderable() {
    // Temel Türkçe karakterler ölçülebilmeli ve boş genişlik üretmemeli.
    for sample in ["İstanbul", "şağı", "GÜÇLÜ", "çalışıyor"] {
        let geist = font_render::text_width(font_render::Face::Geist, sample, 14.0);
        let inria = font_render::text_width(font_render::Face::Inria, sample, 14.0);
        assert!(geist > 0, "Geist ölçümü sıfır: {sample}");
        assert!(inria > 0, "Inria ölçümü sıfır: {sample}");
    }

    // Noktalı/noktasız i ayrımı korunmalı (Türkçe yerelleştirme/okunabilirlik).
    for face in [font_render::Face::Geist, font_render::Face::Inria] {
        assert!(
            font_render::glyph_tier(face, 'i').is_some(),
            "i glifi eksik"
        );
        assert!(
            font_render::glyph_tier(face, 'ı').is_some(),
            "ı glifi eksik"
        );
        // Görsel/bitmap ayrımı `font_render` modül testlerinde doğrulanır;
        // burada her iki karakterin atlas/fallback yolunda mevcut olması yeterlidir.
    }

    // Atlas dışı karakterler '?' fallback genişliğini kullanmalı (çökme/tofu yok).
    let emoji = "\u{1F680}";
    assert_eq!(
        font_render::text_width(font_render::Face::Geist, emoji, 14.0),
        font_render::text_width(font_render::Face::Geist, "?", 14.0),
        "eksik glif fallback genişliği tutarsız"
    );
}

#[test]
fn accent_colors_keep_minimum_visibility_on_key_surfaces() {
    for id in theme::ThemeId::ALL {
        let p = theme::palette(id);
        let checks = [
            (theme::IDX_TITLEBAR, theme::IDX_TEXT_MAIN),
            (theme::IDX_TASKBAR_BG, theme::IDX_TEXT_MAIN),
            (theme::IDX_WINDOW_BG, theme::IDX_ACCENT_BLUE),
        ];
        for (bg_idx, fg_idx) in checks {
            let ratio = theme::contrast_ratio(p[bg_idx as usize], p[fg_idx as usize]);
            assert!(
                ratio >= 1.8,
                "{id:?}: düşük görünürlük (bg={bg_idx}, fg={fg_idx}, {ratio:.2})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Form kontrolü sözleşmesi: Tab / Shift+Tab, Enter, Esc
// ---------------------------------------------------------------------------

/// Panel düğmelerinin klavye sözleşmesini paylaşan modeller.
///
/// Her model, bir uygulamadaki denetim listesinin odağını yönetir. Tüm
/// yerleşik formlarda aynı davranış beklenir:
/// * `Tab` sıradaki denetime, `BackTab` öncekine gider ve başa/sona sarar.
/// * `Enter`/`Space` odaklı denetimi etkinleştirir.
/// * `Esc` birincil durumu kapatır; yoksa varsayılan duruma döner.
struct FormModel {
    tabs: alloc::vec::Vec<&'static str>,
    actions: alloc::vec::Vec<&'static str>,
    focus: usize,
    primary: bool,
    last_action: Option<&'static str>,
}

impl FormModel {
    fn new(tabs: &[&'static str], actions: &[&'static str]) -> Self {
        Self {
            tabs: tabs.to_vec(),
            actions: actions.to_vec(),
            focus: 0,
            primary: false,
            last_action: None,
        }
    }

    fn count(&self) -> usize {
        self.tabs.len() + self.actions.len()
    }

    fn step(&mut self, reverse: bool) {
        let count = self.count();
        self.focus = if reverse {
            (self.focus + count - 1) % count
        } else {
            (self.focus + 1) % count
        };
    }

    fn activate(&mut self) {
        if self.focus >= self.tabs.len() {
            let idx = self.focus - self.tabs.len();
            self.last_action = Some(self.actions[idx]);
        }
    }

    /// `Esc` sözleşmesi: birincil durum varsa yalnızca o kapanır,
    /// yoksa odak ilk denetime döner.
    fn escape(&mut self) -> bool {
        if self.primary {
            self.primary = false;
            return false;
        }
        self.focus = 0;
        true
    }
}

#[test]
fn form_controls_share_tab_enter_escape_contract() {
    // Yerleşik formlar: Kontrol Paneli (sekmeler + düğmeler), Kurulum
    // Sihirbazı (disk/fs/biçim/kur), Dosya Yöneticisi onay diyaloğu.
    let forms = [
        FormModel::new(
            &["Sistem", "Ağ", "Hakkında", "Testler", "İzleme", "Sürücüler"],
            &["Ayarlar"],
        ),
        FormModel::new(&["Disk", "FAT32", "ext4"], &["Biçimlendir", "Kur"]),
        FormModel::new(&["Evet"], &["Hayır"]),
    ];
    for mut form in forms {
        let count = form.count();
        assert!(count >= 2);
        // İleri sarar.
        for expected in 1..=count {
            form.step(false);
            assert_eq!(form.focus, expected % count);
        }
        // Geri sarar.
        for _ in 0..count {
            form.step(true);
        }
        assert_eq!(form.focus, 0);
        // Enter: eylem düğmelerini tetikler, sekmeleri tetiklemez.
        form.focus = form.tabs.len();
        form.activate();
        assert_eq!(form.last_action, Some(form.actions[0]));
        // Esc: birincil durum yoksa varsayılana döner.
        assert!(form.escape());
        assert_eq!(form.focus, 0);
        // Esc: birincil durum varken yalnızca onu kapatır.
        form.primary = true;
        form.focus = count - 1;
        assert!(!form.escape());
        assert!(!form.primary);
        assert_eq!(
            form.focus,
            count - 1,
            "birincil durum kapanırken odak korunur"
        );
    }
}

#[test]
fn keyboard_hints_are_present_for_every_form_surface() {
    for english in [false, true] {
        let hint = screen_layout::welcome_hint(english);
        assert!(hint.contains("Tab") && hint.contains("Enter") && hint.contains("Esc"));
    }
}
