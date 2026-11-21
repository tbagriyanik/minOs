//! Masaüstü olay döngüsü için tüm döngü-yerel mutable durumu tek yapıda toplar.
//! `run_desktop` içindeki yüzlerce lokal değişkeni bu yapıya taşımak,
//! olay döngüsünü modüllere ayırmayı (event_mouse, event_keyboard, event_render)
//! mümkün kılar.

use crate::gui;

/// Masaüstü olay döngüsü durumu.
///
/// `run_desktop` döngüsü boyunca sürekli mutate edilen tüm lokal değişkenler
/// burada tutulur. Modüller (event_mouse, event_keyboard, event_render) bu
/// yapıya `&mut` erişerek ilgili bölümleri işler.
pub struct DesktopLoopState {
    // --- Fare durumu ---
    pub last_mouse_x: i32,
    pub last_mouse_y: i32,
    pub last_buttons: (bool, bool),
    pub mousekeys_click_left: u8,
    pub mousekeys_click_right: u8,
    pub pending_window_control: Option<(usize, u8)>,
    pub dragging_win: Option<usize>,
    pub resizing_win: Option<usize>,
    pub resize_edge: Option<gui::ResizeEdge>,
    pub resize_orig: (usize, usize, usize, usize),
    pub drag_offset_x: usize,
    pub drag_offset_y: usize,
    pub active_snap_preview: Option<(usize, usize, usize, usize)>,
    pub vscroll_drag: Option<(usize, usize, usize, usize, usize, usize)>,
    pub hscroll_drag: Option<(usize, usize, usize, usize, usize, usize)>,
    pub drag_prev_box: Option<(usize, usize, usize, usize)>,
    pub drag_dirty_rect: Option<(usize, usize, usize, usize)>,
    pub force_render: bool,
    pub last_workspace: usize,
    pub cp_last_cycles: u64,

    // --- Zamanlama ---
    pub anim_tick: usize,
    pub last_frame_ms: u64,
    pub loop_counter: u64,
    pub last_clock_update_ms: u64,
    pub game_perf_us: u64,
    pub last_music_beat: usize,
    pub last_music_progress_ms: u64,
    pub last_video_frame_ms: u64,
    pub cpu_sample_start: u64,
    pub cpu_sample_total_cycles: u64,
    pub cpu_sample_active_cycles: u64,
    pub last_ap_heartbeats: [u64; crate::arch::x86_64::smp::MAX_CPUS],

    // --- Ses ---
    pub music_note: usize,
    pub speaker_on: bool,
    pub game_sound_until_ms: u64,

    // --- Oyun bütçesi ---
    pub game_budget_strikes: u32,
    pub fps3d_budget_strikes: u32,

    // --- UI durumu ---
    pub notepad_dragging: bool,
    pub browser_address_dragging: bool,
    pub browser_page_dragging: bool,
    pub browser_last_img_click: Option<(usize, usize)>,
    pub browser_last_img_click_ms: u64,
    pub terminal_dragging: bool,
    pub start_search_dragging: bool,
    pub image_drag_last: Option<(usize, usize)>,
    pub screenshot_counter: usize,
    pub run_dialog_open: bool,
    pub run_dialog_input: gui::TextBuf,
    pub properties_open: bool,
    pub properties_name: gui::TextBuf,
    pub properties_path: gui::TextBuf,
    pub properties_type: gui::TextBuf,
    pub properties_size: gui::TextBuf,
    pub properties_location: gui::TextBuf,

    // --- Render ---
    pub app_render_cycles: [u64; gui::MAX_WINDOWS],
    pub app_render_time_ms: [core::cell::Cell<u64>; gui::MAX_WINDOWS],
    pub cpu_warn_shown: bool,
    pub ram_warn_shown: bool,
    pub last_title_idx: Option<usize>,
    pub last_title_click_ms: u64,

    // --- Şu anki zaman ---
    pub now: crate::drivers::rtc::DateTime,
}

impl DesktopLoopState {
    pub fn new(now: crate::drivers::rtc::DateTime) -> Self {
        Self {
            last_mouse_x: 0,
            last_mouse_y: 0,
            last_buttons: (false, false),
            mousekeys_click_left: 0,
            mousekeys_click_right: 0,
            pending_window_control: None,
            dragging_win: None,
            resizing_win: None,
            resize_edge: None,
            resize_orig: (0, 0, 0, 0),
            drag_offset_x: 0,
            drag_offset_y: 0,
            active_snap_preview: None,
            vscroll_drag: None,
            hscroll_drag: None,
            drag_prev_box: None,
            drag_dirty_rect: None,
            force_render: false,
            last_workspace: 0,
            cp_last_cycles: 0,
            anim_tick: 0,
            last_frame_ms: crate::monotonic_ms(),
            loop_counter: 0,
            last_clock_update_ms: crate::monotonic_ms(),
            game_perf_us: 0,
            last_music_beat: 0,
            last_music_progress_ms: crate::monotonic_ms(),
            last_video_frame_ms: crate::monotonic_ms(),
            cpu_sample_start: crate::boot::time::read_cpu_cycles(),
            cpu_sample_total_cycles: 0,
            cpu_sample_active_cycles: 0,
            last_ap_heartbeats: [0; crate::arch::x86_64::smp::MAX_CPUS],
            music_note: 0,
            speaker_on: false,
            game_sound_until_ms: 0,
            game_budget_strikes: 0,
            fps3d_budget_strikes: 0,
            notepad_dragging: false,
            browser_address_dragging: false,
            browser_page_dragging: false,
            browser_last_img_click: None,
            browser_last_img_click_ms: 0,
            terminal_dragging: false,
            start_search_dragging: false,
            image_drag_last: None,
            screenshot_counter: 1,
            run_dialog_open: false,
            run_dialog_input: gui::TextBuf::new(),
            properties_open: false,
            properties_name: gui::TextBuf::new(),
            properties_path: gui::TextBuf::new(),
            properties_type: gui::TextBuf::new(),
            properties_size: gui::TextBuf::new(),
            properties_location: gui::TextBuf::new(),
            app_render_cycles: [0; gui::MAX_WINDOWS],
            app_render_time_ms: [const { core::cell::Cell::new(0) }; gui::MAX_WINDOWS],
            cpu_warn_shown: false,
            ram_warn_shown: false,
            last_title_idx: None,
            last_title_click_ms: 0,
            now,
        }
    }
}
