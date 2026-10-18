//! Uzun suren disk/ag islemleri icin ekran ortasi ilerleme gostergesi (busy progress).

use crate::{drivers, gui, GLOBAL_KEYBOARD, GLOBAL_MOUSE};

struct BusyProgressContext {
    fb: *mut drivers::video::Framebuffer,
    mx: usize,
    my: usize,
    message: &'static str,
    last_percent: usize,
    cursor_shown: bool,
}

pub static mut BUSY_DIRTY: bool = false;

/// İlerleme yüzdesini hesaplar; değer asla geri gitmez (monotonik).
fn busy_percent(state: &mut BusyProgressContext, done: usize, total: usize) -> usize {
    let mut p = if total == 0 {
        (state.last_percent + 1).min(90)
    } else {
        5 + done.saturating_mul(90) / total
    };
    if p < state.last_percent { p = state.last_percent; }
    if p > 99 { p = 99; }
    state.last_percent = p;
    p
}

pub fn draw_io_progress(context: *mut (), done: usize, total: usize) {
    if context.is_null() { return; }
    let state = unsafe { &mut *(context as *mut BusyProgressContext) };

    // 1. Girişi pompa'la: uzun işlem sürerken fare imleci donmasın.
    unsafe {
        let w = (*state.fb).width as i32;
        let h = (*state.fb).height as i32;
        let mouse_ptr = core::ptr::addr_of_mut!(GLOBAL_MOUSE);
        let kbd_ptr = core::ptr::addr_of_mut!(GLOBAL_KEYBOARD);
        drivers::ps2::poll_input(&mut *mouse_ptr, &mut *kbd_ptr, w, h);
        state.mx = (*mouse_ptr).x.max(0) as usize;
        state.my = (*mouse_ptr).y.max(0) as usize;
    }
    let fb = unsafe { &mut *state.fb };

    // 2. Önceki kum saati imlecini temizle.
    if state.cursor_shown {
        if let Some(rect) = fb.restore_cursor_background() { fb.flush_rect(rect); }
        state.cursor_shown = false;
    }

    // 3. Panel + ilerleme çubuğunu güncelle (tek, monotonik çubuk).
    let percent = busy_percent(state, done, total);
    gui::draw_busy_overlay(fb, state.mx, state.my, state.message, percent);

    // 4. Kum saati imlecini yeni konuma çiz ve ekranı güncelle.
    gui::draw_busy_hourglass(fb, state.mx, state.my);
    if let Some(rect) = fb.cursor_rect() {
        fb.flush_rect(rect);
    }
    state.cursor_shown = true;
}

pub fn with_busy_progress<T>(
    fb: &mut drivers::video::Framebuffer,
    mx: usize,
    my: usize,
    message: &'static str,
    operation: impl FnOnce() -> T,
) -> T {
    // Önceki karede çizilen ok imlecini kaldır (kum saati ile çakışmasın).
    if let Some(rect) = fb.restore_cursor_background() { fb.flush_rect(rect); }
    gui::draw_busy_overlay(fb, mx, my, message, 5);
    gui::draw_busy_hourglass(fb, mx, my);

    // `fb` işaretçisi bağlama devredilir; bundan sonraki çizimler üzerinden yapılır.
    let mut state = BusyProgressContext { fb, mx, my, message, last_percent: 5, cursor_shown: true };
    drivers::ata::set_io_progress_hook(Some(draw_io_progress), &mut state as *mut _ as *mut ());
    let result = operation();
    drivers::ata::set_io_progress_hook(None, core::ptr::null_mut());

    // İşlem sonunda kum saatini temizle; ok imlecini ana döngü yeniden çizer.
    let fb = unsafe { &mut *state.fb };
    if let Some(rect) = fb.restore_cursor_background() { fb.flush_rect(rect); }
    gui::draw_busy_overlay(fb, mx, my, message, 100);
    // Panel bir sonraki tam yeniden çizimde kalksın.
    unsafe { BUSY_DIRTY = true; }
    result
}

