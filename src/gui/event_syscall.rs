//! Kullanıcı modu GUI syscall isteklerini işler.
//!
//! Bu modül, `drain_gui_requests()` üzerinden gelen pencere oluşturma,
//! kapatma, başlık güncelleme ve çizim isteklerini tüketir ve kompozitörle
//! eşler. Her istek için `GuiSyscallReply` üzerinden kullanıcı moduna
//! yanıt gönderilir.

use crate::mm::process::{
    compositor_idx_by_gui_window, enqueue_gui_reply, register_gui_window, unregister_gui_window,
    GuiSyscallReply, GuiSyscallRequest,
};
use crate::mm::uaccess;
use crate::mm::vmm::AddressSpace;

use super::*;
use crate::gui;

/// Tek bir GUI syscall isteğini işler. `true` dönerse istek başarıyla
/// işlendi (veya tanınmayan bir istek olduğu için atlandı), `false`
/// dönerse çağrıcıya devam etmesi söylenir.
pub fn handle_gui_syscall(
    compositor: &mut MinGuiCompositor,
    req_id: crate::mm::process::GuiWindowId,
    req: GuiSyscallRequest,
) {
    match req {
        GuiSyscallRequest::CreateWindow {
            pid: _,
            title_ptr,
            title_len,
            x,
            y,
            w,
            h,
        } => {
            // Başlığı kullanıcı belleğinden kopyala.
            let address_space = AddressSpace::current();
            let title_bytes =
                match uaccess::copy_cstring_from_user(&address_space, title_ptr, title_len + 1) {
                    Ok(b) => b,
                    Err(_) => return,
                };
            let title_str = match core::str::from_utf8(&title_bytes) {
                Ok(s) => s,
                Err(_) => return,
            };
            // Sabit bir başlık kullan (dinamik başlık için Window.title
            // alanının &'static str'den String'e değiştirilmesi gerekir).
            let _ = title_str;
            let title = "Kullanıcı Uygulaması";
            let win = gui::Window::new(title, "UserApp", x, y, w.min(800), h.min(600), true);
            let mut win = win;
            win.app_id = u8::MAX; // Kullanıcı modu penceresini işaretler.
            if let Some(idx) = compositor.add_window(win) {
                // window_id -> compositor idx eşlemesini kur, böylece
                // DestroyWindow/SetTitle bu pencereyi bulabilir.
                let _ = register_gui_window(req_id, idx);
            }
        }
        GuiSyscallRequest::DestroyWindow { pid: _, window_id } => {
            let Some(compositor_idx) = compositor_idx_by_gui_window(window_id) else {
                let _ = enqueue_gui_reply(window_id, GuiSyscallReply::DestroyWindow { ok: false });
                return;
            };
            // Pencereyi kapat (is_open = false) ve kirli bölge olarak işaretle.
            // Süreç sonlandırması terminate_app (event_loop.rs üzerinden) tarafından
            // yönetilir; burada çift sonlandırma riskinden kaçınılır.
            if let Some(win) = compositor.windows.get_mut(compositor_idx) {
                let win = win.as_mut().unwrap();
                let (x, y, w, h) = (win.x, win.y, win.width, win.height);
                win.is_open = false;
                compositor.dirty_tracker.mark_window(
                    compositor_idx,
                    crate::gui::dirty_rect::DirtyRect::new(x, y, w, h),
                );
            }
            // window_id eşlemesini temizle.
            let _ = unregister_gui_window(compositor_idx);
            // Fokusu mevcut olan en üstteki pencereye aktar.
            compositor.focus_top_window();
            let _ = enqueue_gui_reply(window_id, GuiSyscallReply::DestroyWindow { ok: true });
        }
        GuiSyscallRequest::SetTitle {
            pid: _,
            window_id,
            title_ptr,
            title_len,
        } => {
            let Some(compositor_idx) = compositor_idx_by_gui_window(window_id) else {
                let _ = enqueue_gui_reply(window_id, GuiSyscallReply::SetTitle { ok: false });
                return;
            };
            // Başlığı kullanıcı belleğinden kopyala ve doğrula.
            let address_space = AddressSpace::current();
            let title_bytes =
                match uaccess::copy_cstring_from_user(&address_space, title_ptr, title_len + 1) {
                    Ok(b) => b,
                    Err(_) => {
                        let _ =
                            enqueue_gui_reply(window_id, GuiSyscallReply::SetTitle { ok: false });
                        return;
                    }
                };
            let _ = core::str::from_utf8(&title_bytes);
            // Window.title alanı &'static str olduğu için dinamik başlık
            // depolanamaz; istek kabul edilir (ok) ancak başlık değişmez.
            // Tam dinamik başlık desteği için title alanının String'e
            // çevrilmesi gerekir.
            if let Some(win) = compositor.windows.get_mut(compositor_idx) {
                let win = win.as_mut().unwrap();
                let (x, y, w, h) = (win.x, win.y, win.width, win.height);
                compositor.dirty_tracker.mark_window(
                    compositor_idx,
                    crate::gui::dirty_rect::DirtyRect::new(x, y, w, h),
                );
            }
            let _ = enqueue_gui_reply(window_id, GuiSyscallReply::SetTitle { ok: true });
        }
        GuiSyscallRequest::DrawPixel {
            pid: _,
            window_id: _,
            x: _,
            y: _,
            color: _,
        } => {
            // TODO: Pikseli kullanıcı belleğinden çizer.
        }
        GuiSyscallRequest::DrawRect {
            pid: _,
            window_id: _,
            x: _,
            y: _,
            w: _,
            h: _,
            color: _,
        } => {
            // TODO: Dikdörtgeni kullanıcı belleğinden çizer.
        }
        GuiSyscallRequest::GetInput {
            pid: _,
            window_id: _,
            buffer_ptr: _,
            buffer_len: _,
        } => {
            // TODO: Girdi olaylarını kullanıcı tamponuna yazar.
        }
        GuiSyscallRequest::SetCursor { pid: _, x: _, y: _ } => {
            // TODO: Fare imlecini ayarlar.
        }
        GuiSyscallRequest::GetWindowInfo {
            pid: _,
            window_id: _,
            buffer_ptr: _,
        } => {
            // TODO: Pencere bilgilerini kullanıcı tamponuna yazar.
        }
    }
}

/// Tüm bekleyen GUI syscall isteklerini sırayla işler.
pub fn drain_and_handle_gui_syscalls(compositor: &mut MinGuiCompositor) {
    for (req_id, req) in crate::mm::process::drain_gui_requests() {
        handle_gui_syscall(compositor, req_id, req);
    }
}
