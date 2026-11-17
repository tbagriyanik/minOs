//! Kullanıcı modu GUI kitaplığı.
//!
//! Bu modül, kullanıcı modu (Ring3) süreçlerinin kompozitörle iletişim
//! kurması için güvenli bir API sağlar. İki çalışma modu destekler:
//! - `direct`: Çekirdek içi çağrılar (geçerli mimari, kernel nesnesi olarak çalışan uygulamalar)
//! - `syscall`: `int 0x80` üzerinden sistem çağrıları (gelecekteki ayrı ELF kullanıcı uygulamaları)
//!
//! `cfg(feature = "user_mode")` ile syscall modu etkinleştirilir.

/// Pencere oluşturma sonucu.
pub type GuiResult = Result<u32, &'static str>;

/// Kullanıcı modu GUI arayüzü.
pub struct UserGui;

impl UserGui {
    /// Yeni bir pencere oluşturur.
    /// Dönüş: pencere kimliği (window_id) veya hata.
    pub fn create_window(title: &str, x: usize, y: usize, w: usize, h: usize) -> GuiResult {
        if title.len() > 64 {
            return Err("başlık çok uzun");
        }
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return Err("geçersiz boyut");
        }

        #[cfg(feature = "user_mode")]
        {
            // Syscall yolu: parametre yapısını kullanıcı belleğine yaz ve SYS_GUI_CREATE_WINDOW çağır.
            // Bu mod, ayrı bir ELF ikili dosyası olarak çalışan kullanıcı uygulamaları içindir.
            return Self::create_window_syscall(title, x, y, w, h);
        }

        #[cfg(not(feature = "user_mode"))]
        {
            // Doğrudan çekirdek çağrı: mevcut mimari, kernel nesnesi olarak çalışan uygulamalar.
            // Bu mod, `launch_app_window` üzerinden önceden ayrılmış bir pencere slotu kullanır.
            // Gerçek pencere oluşturma, olay döngüsü tarafından `drain_gui_requests` ile yapılır.
            // Bu kitaplık sadece arayüzü sağlar; gerçek oluşturma kompozitör tarafından.
            let _ = title;
            let _ = x;
            let _ = y;
            let _ = w;
            let _ = h;
            Err("doğrudan mod henüz kompozitör entegrasyonu yok")
        }
    }

    /// Pencereyi kapatır.
    pub fn destroy_window(window_id: u32) -> Result<(), &'static str> {
        let _ = window_id;
        #[cfg(feature = "user_mode")]
        {
            return Self::destroy_window_syscall(window_id);
        }
        #[cfg(not(feature = "user_mode"))]
        {
            Err("doğrudan mod henüz desteklenmiyor")
        }
    }

    /// Pencere başlığını ayarlar.
    pub fn set_title(window_id: u32, title: &str) -> Result<(), &'static str> {
        let _ = window_id;
        let _ = title;
        #[cfg(feature = "user_mode")]
        {
            Self::set_title_syscall(window_id, title)
        }
        #[cfg(not(feature = "user_mode"))]
        {
            Err("doğrudan mod henüz desteklenmiyor")
        }
    }

    /// Pencereye piksel çizer.
    pub fn draw_pixel(window_id: u32, x: usize, y: usize, color: u32) -> Result<(), &'static str> {
        let _ = window_id;
        let _ = x;
        let _ = y;
        let _ = color;
        #[cfg(feature = "user_mode")]
        {
            Self::draw_pixel_syscall(window_id, x, y, color)
        }
        #[cfg(not(feature = "user_mode"))]
        {
            Err("doğrudan mod henüz desteklenmiyor")
        }
    }

    /// Pencereye dikdörtgen çizer.
    pub fn draw_rect(
        window_id: u32,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        color: u32,
    ) -> Result<(), &'static str> {
        let _ = window_id;
        let _ = x;
        let _ = y;
        let _ = w;
        let _ = h;
        let _ = color;
        #[cfg(feature = "user_mode")]
        {
            Self::draw_rect_syscall(window_id, x, y, w, h, color)
        }
        #[cfg(not(feature = "user_mode"))]
        {
            Err("doğrudan mod henüz desteklenmiyor")
        }
    }

    /// Girdi olaylarını okur.
    pub fn get_input(window_id: u32, buffer: &mut [u8]) -> Result<usize, &'static str> {
        let _ = window_id;
        let _ = buffer;
        #[cfg(feature = "user_mode")]
        {
            Self::get_input_syscall(window_id, buffer)
        }
        #[cfg(not(feature = "user_mode"))]
        {
            Err("doğrudan mod henüz desteklenmiyor")
        }
    }

    /// Fare imlecini ayarlar.
    pub fn set_cursor(x: usize, y: usize) -> Result<(), &'static str> {
        let _ = x;
        let _ = y;
        #[cfg(feature = "user_mode")]
        {
            Self::set_cursor_syscall(x, y)
        }
        #[cfg(not(feature = "user_mode"))]
        {
            Err("doğrudan mod henüz desteklenmiyor")
        }
    }

    /// Pencere bilgilerini döndürür (x, y, w, h).
    pub fn get_window_info(window_id: u32) -> Result<(usize, usize, usize, usize), &'static str> {
        let _ = window_id;
        #[cfg(feature = "user_mode")]
        {
            Self::get_window_info_syscall(window_id)
        }
        #[cfg(not(feature = "user_mode"))]
        {
            Err("doğrudan mod henüz desteklenmiyor")
        }
    }

    // -----------------------------------------------------------------------
    // Syscall yolu (int 0x80) - kullanıcı modu ELF uygulamaları için
    // -----------------------------------------------------------------------

    #[cfg(feature = "user_mode")]
    fn create_window_syscall(title: &str, x: usize, y: usize, w: usize, h: usize) -> GuiResult {
        // Parametre yapısı: [title_ptr, title_len, x, y, w, h] (6 x u64 = 48 bayt)
        // Bu yapı kullanıcı belleğinde olmalı; şimdilik basit bir yaklaşım:
        // title'ı bir sabit bellek bölgesine kopyala ve SYS_GUI_CREATE_WINDOW çağır.
        let _ = title;
        let _ = x;
        let _ = y;
        let _ = w;
        let _ = h;
        Err("syscall modu henüz tam olarak entegre edilmedi")
    }

    #[cfg(feature = "user_mode")]
    fn destroy_window_syscall(window_id: u32) -> Result<(), &'static str> {
        let _ = window_id;
        Err("syscall modu henüz tam olarak entegre edilmedi")
    }

    #[cfg(feature = "user_mode")]
    fn set_title_syscall(window_id: u32, title: &str) -> Result<(), &'static str> {
        let _ = window_id;
        let _ = title;
        Err("syscall modu henüz tam olarak entegre edilmedi")
    }

    #[cfg(feature = "user_mode")]
    fn draw_pixel_syscall(
        window_id: u32,
        x: usize,
        y: usize,
        color: u32,
    ) -> Result<(), &'static str> {
        let _ = window_id;
        let _ = x;
        let _ = y;
        let _ = color;
        Err("syscall modu henüz tam olarak entegre edilmedi")
    }

    #[cfg(feature = "user_mode")]
    fn draw_rect_syscall(
        window_id: u32,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        color: u32,
    ) -> Result<(), &'static str> {
        let _ = window_id;
        let _ = x;
        let _ = y;
        let _ = w;
        let _ = h;
        let _ = color;
        Err("syscall modu henüz tam olarak entegre edilmedi")
    }

    #[cfg(feature = "user_mode")]
    fn get_input_syscall(window_id: u32, buffer: &mut [u8]) -> Result<usize, &'static str> {
        let _ = window_id;
        let _ = buffer;
        Err("syscall modu henüz tam olarak entegre edilmedi")
    }

    #[cfg(feature = "user_mode")]
    fn set_cursor_syscall(x: usize, y: usize) -> Result<(), &'static str> {
        let _ = x;
        let _ = y;
        Err("syscall modu henüz tam olarak entegre edilmedi")
    }

    #[cfg(feature = "user_mode")]
    fn get_window_info_syscall(
        window_id: u32,
    ) -> Result<(usize, usize, usize, usize), &'static str> {
        let _ = window_id;
        Err("syscall modu henüz tam olarak entegre edilmedi")
    }
}

/// Syscall numaraları (kullanıcı modu uygulamaları için referans).
pub mod sys {
    pub const SYS_GUI_CREATE_WINDOW: u64 = crate::mm::process::sys_nos::SYS_GUI_CREATE_WINDOW;
    pub const SYS_GUI_DESTROY_WINDOW: u64 = crate::mm::process::sys_nos::SYS_GUI_DESTROY_WINDOW;
    pub const SYS_GUI_SET_TITLE: u64 = crate::mm::process::sys_nos::SYS_GUI_SET_TITLE;
    pub const SYS_GUI_DRAW_PIXEL: u64 = crate::mm::process::sys_nos::SYS_GUI_DRAW_PIXEL;
    pub const SYS_GUI_DRAW_RECT: u64 = crate::mm::process::sys_nos::SYS_GUI_DRAW_RECT;
    pub const SYS_GUI_GET_INPUT: u64 = crate::mm::process::sys_nos::SYS_GUI_GET_INPUT;
    pub const SYS_GUI_SET_CURSOR: u64 = crate::mm::process::sys_nos::SYS_GUI_SET_CURSOR;
    pub const SYS_GUI_GET_WINDOW_INFO: u64 = crate::mm::process::sys_nos::SYS_GUI_GET_WINDOW_INFO;
}
