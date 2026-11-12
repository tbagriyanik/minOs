//! Window model, layout types and frame rendering.

use super::*;

// Window model, layout types and frame rendering.
//
// This module defines the core window data structures and frame rendering logic
// for the MinGui compositor. It includes window state management, title bar
// controls, resize grips, and Aero Snap functionality.
//
// # Examples
//
// ```ignore
// let window = Window::new(
//     "My Application",
//     "App",
//     100,
//     100,
//     800,
//     600,
//     true,
// );
// ```

/// Represents the minimal state of a window for persistence.
///
/// This struct contains only the essential geometric and visibility state
/// needed to restore a window's position and size. It is used for
/// serialization and window state management.
#[derive(Copy, Clone, Debug)]
pub struct WindowState {
    /// X coordinate of the window's top-left corner.
    pub x: usize,
    /// Y coordinate of the window's top-left corner.
    pub y: usize,
    /// Window width in pixels.
    pub width: usize,
    /// Window height in pixels.
    pub height: usize,
    /// Whether the window is currently open/visible.
    pub is_open: bool,
}

/// Base title bar height at 100% UI scale (pixels).
pub const TITLEBAR_BASE: usize = 24;
/// Base taskbar height at 100% UI scale (pixels).
pub const TASKBAR_BASE: usize = 36;
/// Maximum number of concurrent windows supported by the compositor.
pub const MAX_WINDOWS: usize = 32;

/// Returns the current title bar height scaled by the active font zoom level.
///
/// The height scales proportionally with the UI scale factor (100%, 125%, or 150%).
/// This ensures UI elements remain usable at higher DPI settings.
///
/// # Returns
/// The scaled title bar height in pixels.
#[inline(always)]
pub fn titlebar_h() -> usize {
    sp(TITLEBAR_BASE)
}

/// Returns the current taskbar height scaled by the active font zoom level.
///
/// The height scales proportionally with the UI scale factor.
///
/// # Returns
/// The scaled taskbar height in pixels.
#[inline(always)]
pub fn taskbar_h() -> usize {
    sp(TASKBAR_BASE)
}

/// Main window structure representing a top-level GUI window.
///
/// This struct contains all state needed to render and manage a window,
/// including geometry, visibility, focus, and content scrolling state.
/// Windows are managed by the [`MinGuiCompositor`] which handles
/// Z-ordering, focus management, and rendering.
///
/// # Fields
/// - `app_id`: Identifies which application this window belongs to
/// - `title`: Full window title (displayed in title bar)
/// - `tab_title`: Short title for taskbar tab
/// - `x`, `y`: Top-left position
/// - `width`, `height`: Current dimensions
/// - `min_width`/`min_height`: Minimum allowed size when resizing
/// - `max_width`/`max_height`: Maximum allowed size
/// - `is_focused`: Whether this window has keyboard focus
/// - `is_minimized`: Whether window is minimized to taskbar
/// - `is_open`: Whether window is currently visible
/// - `is_maximized`: Whether window is maximized
/// - `restore`: Previous geometry for un-maximize
/// - `z`: Z-order index (higher = on top)
/// - `scroll`: Content scroll position
/// - `content_w`/`content_h`: Reported content size for scrollbar calculation
#[derive(Copy, Clone, Debug)]
pub struct Window {
    /// Application identifier; allows multiple instances of the same app.
    pub app_id: u8,
    /// Full window title displayed in the title bar.
    pub title: &'static str,
    /// Short title for taskbar tab display.
    pub tab_title: &'static str,
    /// X coordinate of window top-left corner.
    pub x: usize,
    /// Y coordinate of window top-left corner.
    pub y: usize,
    /// Window width in pixels.
    pub width: usize,
    /// Window height in pixels.
    pub height: usize,
    /// Minimum allowed width when resizing.
    pub min_width: usize,
    /// Minimum allowed height when resizing.
    pub min_height: usize,
    /// Maximum allowed width (screen boundary constraint).
    pub max_width: usize,
    /// Maximum allowed height (screen boundary constraint).
    pub max_height: usize,
    /// Whether this window currently has keyboard focus.
    pub is_focused: bool,
    /// Whether window is minimized to taskbar.
    pub is_minimized: bool,
    /// Whether window is currently open and should be rendered.
    pub is_open: bool,
    /// Whether window is maximized to full screen.
    pub is_maximized: bool,
    /// Previous geometry for restoring after maximize.
    pub restore: Option<(usize, usize, usize, usize)>,
    /// Z-order index; higher values render on top.
    pub z: u8,
    /// Content scroll state for overflow handling.
    pub scroll: scrollbar::ScrollState,
    /// Last reported content width (for horizontal scrollbar).
    pub content_w: usize,
    /// Last reported content height (for vertical scrollbar).
    pub content_h: usize,
}

impl Window {
    /// Returns the content rectangle of the window (excluding frame and title bar).
    ///
    /// This is the area where application content is rendered. The returned
    /// coordinates are relative to the framebuffer origin.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in pixels.
    pub fn content_rect(&self) -> (usize, usize, usize, usize) {
        let x = self.x + 1;
        let y = self.y + titlebar_h();
        let w = self.width.saturating_sub(2);
        let h = self.height.saturating_sub(titlebar_h() + 1);
        (x, y, w, h)
    }

    /// Creates a new window with the specified parameters.
    ///
    /// # Arguments
    /// * `title` - Full window title for the title bar
    /// * `tab_title` - Short title for taskbar tab
    /// * `x` - Initial X position
    /// * `y` - Initial Y position
    /// * `width` - Initial width
    /// * `height` - Initial height
    /// * `is_open` - Whether the window should be initially visible
    ///
    /// # Returns
    /// A new `Window` with default min/max sizes and zero scroll state.
    pub fn new(
        title: &'static str,
        tab_title: &'static str,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        is_open: bool,
    ) -> Self {
        Self {
            app_id: u8::MAX,
            title,
            tab_title,
            x,
            y,
            width,
            height,
            min_width: 200,
            min_height: 120,
            // Resize bounds up to 4096px for multi-monitor and future-proofing
            // (consistent with 1280x720, 1920x1080, 2560x1600 displays).
            max_width: 4096,
            max_height: 4096,
            is_focused: false,
            is_minimized: false,
            is_open,
            is_maximized: false,
            restore: None,
            z: 0,
            scroll: scrollbar::ScrollState::new(),
            content_w: width,
            content_h: height.saturating_sub(titlebar_h()),
        }
    }

    /// Extracts the minimal persistable state from this window.
    ///
    /// Used for serialization and window management operations.
    ///
    /// # Returns
    /// A [`WindowState]` containing position, size, and visibility.
    pub fn get_state(&self) -> WindowState {
        WindowState {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            is_open: self.is_open,
        }
    }

    /// Applies a previously saved state to this window.
    ///
    /// Restores position, size, and visibility. If the window is being
    /// opened, also clears the minimized flag.
    ///
    /// # Arguments
    /// * `state` - The state to apply
    pub fn apply_state(&mut self, state: WindowState) {
        self.x = state.x;
        self.y = state.y;
        self.width = state.width;
        self.height = state.height;
        self.is_open = state.is_open;
        if self.is_open {
            self.is_minimized = false;
        }
    }

    /// Returns the visual bounds of the close button (for rendering).
    ///
    /// The button is centered vertically in the title bar and sized according
    /// to the current UI scale factor.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    pub fn close_rect(&self) -> (usize, usize, usize, usize) {
        let b = sp(18);
        (
            self.x + self.width.saturating_sub(sp(26)),
            self.y + (titlebar_h().saturating_sub(b)) / 2,
            b,
            b,
        )
    }

    /// Returns the hit-test rectangle for the close button.
    ///
    /// This is larger than the visual button to provide a comfortable
    /// click target. It spans the full title bar height and extends further
    /// horizontally. It never shrinks even when the visual button is pressed.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    pub fn close_hit_rect(&self) -> (usize, usize, usize, usize) {
        let w = sp(34);
        (
            self.x + self.width.saturating_sub(w),
            self.y,
            w,
            titlebar_h(),
        )
    }

    /// Returns the visual bounds of the maximize/restore button.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    pub fn max_rect(&self) -> (usize, usize, usize, usize) {
        let b = sp(18);
        (
            self.x + self.width.saturating_sub(sp(46)),
            self.y + (titlebar_h().saturating_sub(b)) / 2,
            b,
            b,
        )
    }

    /// Returns the hit-test rectangle for the maximize button.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    pub fn max_hit_rect(&self) -> (usize, usize, usize, usize) {
        let w = sp(22);
        (
            self.x + self.width.saturating_sub(sp(56)),
            self.y,
            w,
            titlebar_h(),
        )
    }

    /// Returns the visual bounds of the minimize button.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    pub fn min_rect(&self) -> (usize, usize, usize, usize) {
        let b = sp(18);
        (
            self.x + self.width.saturating_sub(sp(66)),
            self.y + (titlebar_h().saturating_sub(b)) / 2,
            b,
            b,
        )
    }

    /// Returns the hit-test rectangle for the minimize button.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    pub fn min_hit_rect(&self) -> (usize, usize, usize, usize) {
        let w = sp(22);
        (
            self.x + self.width.saturating_sub(sp(78)),
            self.y,
            w,
            titlebar_h(),
        )
    }

    /// Returns the hit-test rectangle for the bottom-right resize grip.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    pub fn resize_grip_rect(&self) -> (usize, usize, usize, usize) {
        let g = RESIZE_GRIP;
        (
            self.x + self.width.saturating_sub(g),
            self.y + self.height.saturating_sub(g),
            g,
            g,
        )
    }

    /// Returns the title bar region occupied by control buttons.
    ///
    /// This region is excluded from resize hit-testing to prevent
    /// accidental resize when clicking window controls.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    fn title_controls_rect(&self) -> (usize, usize, usize, usize) {
        let left = self.min_rect().0;
        let right = self.x + self.width;
        (left, self.y, right.saturating_sub(left), titlebar_h())
    }

    /// Returns the title bar drag region (for window movement).
    ///
    /// Excludes the control button area on the right side.
    ///
    /// # Returns
    /// Tuple of (x, y, width, height) in framebuffer coordinates.
    pub fn title_drag_rect(&self) -> (usize, usize, usize, usize) {
        (
            self.x,
            self.y,
            self.width.saturating_sub(sp(66)),
            titlebar_h(),
        )
    }
}

/// Resize edge/corner identifier for window resizing operations.
///
/// Used to determine which edge or corner of a window the user is
/// interacting with for resize operations.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ResizeEdge {
    /// Left edge
    Left,
    /// Right edge
    Right,
    /// Top edge
    Top,
    /// Bottom edge
    Bottom,
    /// Top-left corner
    TopLeft,
    /// Top-right corner
    TopRight,
    /// Bottom-left corner
    BottomLeft,
    /// Bottom-right corner
    BottomRight,
}

/// Resize grip size for side/bottom edges (pixels at 100% scale).
/// Top edge uses a smaller grip to avoid title bar overlap.
pub const RESIZE_GRIP: usize = 12;
const TOP_RESIZE_GRIP: usize = 4;

impl Window {
    /// Determines which resize edge (if any) contains the given point.
    ///
    /// Returns the appropriate [`ResizeEdge`] variant if the point falls
    /// within the resize grip area of any edge or corner. Returns `None`
    /// for maximized/minimized/closed windows, or points in the title
    /// bar control area.
    ///
    /// # Arguments
    /// * `mx` - Mouse X coordinate (framebuffer space)
    /// * `my` - Mouse Y coordinate (framebuffer space)
    ///
    /// # Returns
    /// The detected resize edge, or `None` if not in a resize area.
    pub fn resize_edge_at(&self, mx: usize, my: usize) -> Option<ResizeEdge> {
        if !self.is_open || self.is_minimized || self.is_maximized {
            return None;
        }
        // Title bar controls take precedence; resize grips don't overlap them.
        if in_rect(mx, my, self.title_controls_rect()) {
            return None;
        }

        let g = RESIZE_GRIP;
        const OUTSIDE_MARGIN: usize = 4;
        let left_bound = self.x.saturating_sub(OUTSIDE_MARGIN);
        let right_bound = self.x + self.width + OUTSIDE_MARGIN;
        let top_bound = self.y.saturating_sub(OUTSIDE_MARGIN);
        let bottom_bound = self.y + self.height + OUTSIDE_MARGIN;

        if mx < left_bound || mx >= right_bound || my < top_bound || my >= bottom_bound {
            return None;
        }

        let left = mx <= self.x + g;
        let edge_right = mx + g >= self.x + self.width;
        let top = my <= self.y + TOP_RESIZE_GRIP;
        let edge_bottom = my + 4 >= self.y + self.height;

        if top && left {
            Some(ResizeEdge::TopLeft)
        } else if top && edge_right {
            Some(ResizeEdge::TopRight)
        } else if edge_bottom && left {
            Some(ResizeEdge::BottomLeft)
        } else if edge_bottom && edge_right {
            Some(ResizeEdge::BottomRight)
        } else if left {
            Some(ResizeEdge::Left)
        } else if edge_right {
            Some(ResizeEdge::Right)
        } else if top {
            Some(ResizeEdge::Top)
        } else if edge_bottom {
            Some(ResizeEdge::Bottom)
        } else {
            None
        }
    }
}

/// Aero Snap window positioning targets.
///
/// Defines the standard snap positions for window tiling operations
/// (Windows 10/11 style Aero Snap).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SnapPosition {
    /// Left half of screen
    Left,
    /// Right half of screen
    Right,
    /// Full screen (maximize)
    Top,
    /// Top-left quarter
    TopLeft,
    /// Top-right quarter
    TopRight,
    /// Bottom-left quarter
    BottomLeft,
    /// Bottom-right quarter
    BottomRight,
}

/// Taskbar layout coordinates (single source of truth for rendering and hit-testing).
///
/// This struct contains pre-computed rectangles for all taskbar elements,
/// ensuring visual layout matches click targets exactly.
#[derive(Copy, Clone)]
pub struct TrayLayout {
    /// Y coordinate of the taskbar top edge.
    pub tb_y: usize,
    /// Start button bounds.
    pub start: (usize, usize, usize, usize),
    /// Clock/time display bounds.
    pub clock: (usize, usize, usize, usize),
    /// Minimize all button bounds (thin blue vertical line).
    pub minimize_all: (usize, usize, usize, usize),
    /// Volume control bounds.
    pub volume: (usize, usize, usize, usize),
    /// Network status bounds.
    pub network: (usize, usize, usize, usize),
    /// Keyboard layout indicator bounds.
    pub kbd: (usize, usize, usize, usize),
    /// Show desktop button bounds.
    pub desktop: (usize, usize, usize, usize),
}

/// Start menu layout constants — single canonical source.
///
/// Group headers and items share the same ordered layout; both rendering
/// and hit-testing use these functions so visual items and click targets
/// match exactly.
///
/// Start menu total height (pixels).
pub const MENU_H: usize = 230;
/// Menu content start Y (after profile and search).
pub const MENU_CONTENT_TOP: usize = 82;
/// Group header height.
pub const MENU_GROUP_H: usize = 18;
/// Menu item height (row spacing).
pub const MENU_ITEM_H: usize = 20;
/// Menu item click target height (legacy compatibility).
pub const MENU_ITEM_STEP: usize = 20;

/// Group definitions: (display title, first item index, item count).
/// Indices correspond to the sequential menu item order (see `menu_items`).
pub const MENU_GROUPS: [(&str, usize, usize); 5] = [
    ("Sistem", 0, 7),
    ("Üretkenlik", 7, 3),
    ("Medya", 10, 7),
    ("Kurulu", 17, 9),
    ("Güç", 26, 2),
];

impl MinGuiCompositor {
    // -----------------------------------------------------------------------
    // 2. Window Frame and Title Bar
    // -----------------------------------------------------------------------

    /// Maps a window index to its corresponding icon index.
    ///
    /// Used for rendering the application icon in the window title bar
    /// and taskbar tab.
    ///
    /// # Arguments
    /// * `win_idx` - The window index (0-16 for built-in apps)
    ///
    /// # Returns
    /// The icon index from the icon sheet.
    /// Maps a window index to its corresponding icon index.
    ///
    /// Used for rendering the application icon in the window title bar
    /// and taskbar tab.
    ///
    /// # Arguments
    /// * `win_idx` - The window index (0-16 for built-in apps)
    ///
    /// # Returns
    /// The icon index from the icon sheet.
    pub fn window_icon_index(win_idx: usize) -> usize {
        match win_idx {
            0 => 0,  // File Manager -> Computer
            1 => 4,  // Settings -> Gear
            2 => 1,  // Notepad -> Notes
            3 => 2,  // Web Browser -> SarmanNet
            4 => 1,  // Image Viewer -> Image
            5 => 3,  // Music Player -> Playlist
            6 => 3,  // Video Player -> Media
            7 => 0,  // Task Manager -> Computer
            8 => 4,  // Calculator -> Gear
            9 => 1,  // Paint -> Drawing
            10 => 0, // Terminal -> Command Prompt
            11 => 0, // Installer -> Computer/Disk
            12 => 1, // 3D Viewer -> Visual/Drawing
            13 => 5, // Package Manager -> Package/Box
            14 => 4, // Control Panel -> Gear
            15 => 2, // Games -> Game board
            16 => 2, // FPS 3D Game -> Game board
            _ => 0,
        }
    }

    /// Returns the localized full window title for a given index.
    ///
    /// # Arguments
    /// * `win_idx` - Window index
    /// * `english` - Whether to use English localization
    ///
    /// # Returns
    /// The window title string.
    pub(crate) fn window_title(win_idx: usize, english: bool) -> &'static str {
        if english {
            match win_idx {
                0 => "File Manager",
                1 => "System Settings",
                2 => "Notepad",
                3 => "Web Browser",
                4 => "Image Viewer",
                5 => "Music Player",
                6 => "Video Player",
                7 => "Task Manager",
                8 => "Calculator",
                9 => "Paint",
                10 => "Command Prompt",
                11 => "One-Click Install",
                12 => "3D Viewer",
                13 => "Package Manager",
                14 => "Control Panel",
                15 => "Games",
                16 => "FPS 3D Game",
                _ => "Window",
            }
        } else {
            match win_idx {
                0 => "Dosya Yöneticisi",
                1 => "Sistem Ayarları",
                2 => "Not Defteri",
                3 => "Web Gezgini",
                4 => "Resim Gösterici",
                5 => "Müzik Çalar",
                6 => "Video Oynatıcı",
                7 => "Görev Yöneticisi",
                8 => "Hesap Makinesi",
                9 => "Çizim Tahtası",
                10 => "Komut İstemi",
                11 => "Tek Tıkla Kur",
                12 => "3D Gösterici",
                13 => "Paket Yöneticisi",
                14 => "Kontrol Paneli",
                15 => "Oyunlar",
                16 => "FPS 3D Oyun",
                _ => "Pencere",
            }
        }
    }

    /// Returns the localized short window title for taskbar tabs.
    ///
    /// # Arguments
    /// * `win_idx` - Window index
    /// * `english` - Whether to use English localization
    ///
    /// # Returns
    /// The short title string for taskbar display.
    pub(crate) fn window_tab_title(win_idx: usize, english: bool) -> &'static str {
        if english {
            match win_idx {
                0 => "Files",
                1 => "Settings",
                2 => "Notes",
                3 => "Web",
                4 => "Image",
                5 => "Music",
                6 => "Video",
                7 => "Tasks",
                8 => "Calc",
                9 => "Paint",
                10 => "CMD",
                11 => "Install",
                12 => "3D",
                13 => "Packages",
                14 => "Control",
                15 => "Game",
                16 => "FPS3D",
                _ => "Win",
            }
        } else {
            match win_idx {
                0 => "Dosyalar",
                1 => "Ayarlar",
                2 => "Notlar",
                3 => "Web",
                4 => "Resim",
                5 => "Müzik",
                6 => "Video",
                7 => "Görevler",
                8 => "Hesap",
                9 => "Çizim",
                10 => "Komut",
                11 => "Kurulum",
                12 => "3D",
                13 => "Paketler",
                14 => "Kontrol",
                15 => "Oyun",
                16 => "FPS3D",
                _ => "Pencere",
            }
        }
    }

    /// Renders a window frame with title bar, controls, and borders.
    ///
    /// This is the primary window frame rendering function. It draws:
    /// - Drop shadow
    /// - Rounded window border (focused = blue, unfocused = border color)
    /// - Title bar with vertical gradient
    /// - Application icon and title
    /// - Close/Maximize/Minimize buttons with hover/press states
    /// - Appropriate button icons (X, square, line, or restore symbol)
    ///
    /// # Arguments
    /// * `fb` - Target framebuffer
    /// * `win` - Window to render
    /// * `mouse` - Current mouse state (for hover detection)
    /// * `_win_idx` - Window index (unused, kept for compatibility)
    pub fn render_window_frame(
        &self,
        fb: &mut Framebuffer,
        win: &Window,
        mouse: &MouseState,
        _win_idx: usize,
    ) {
        if !win.is_open || win.is_minimized {
            return;
        }
        let mx = mouse.x as usize;
        let my = mouse.y as usize;

        // Drop shadow for depth
        fb.draw_shadow(win.x, win.y, win.width, win.height, Color::SHADOW);

        // Window border and body (rounded 6px)
        let border = if win.is_focused {
            Color::ACCENT_BLUE
        } else {
            Color::BORDER
        };
        fb.draw_rounded_rect(
            win.x.saturating_sub(1),
            win.y.saturating_sub(1),
            win.width + 2,
            win.height + 2,
            6,
            border,
        );
        fb.draw_rounded_rect_alpha(
            win.x,
            win.y,
            win.width,
            win.height,
            6,
            Color::WINDOW_BG,
            232,
        );

        // Başlık Çubuğu (dikey gradyan — yuvarlatılmış)
        let (t_top, t_bottom) = if win.is_focused {
            (Color::TITLE_BAR, Self::shade(Color::TITLE_BAR, -14))
        } else {
            (
                Color::TITLE_BAR_INACTIVE,
                Self::shade(Color::TITLE_BAR_INACTIVE, -10),
            )
        };
        fb.draw_rounded_vgradient(win.x, win.y, win.width, titlebar_h(), 6, t_top, t_bottom);
        fb.draw_rounded_rect_alpha(win.x, win.y, win.width, titlebar_h(), 6, t_top, 224);
        fb.draw_rect(win.x, win.y + titlebar_h() - 1, win.width, 1, border);

        // Pencereye özel ilgili program simgesi ve başlık (aktif ölçekte).
        let icon_idx = Self::window_icon_index(win.app_id as usize);
        let icon_sz = Self::icon_size();
        let icon_y = win.y + titlebar_h().saturating_sub(icon_sz) / 2;
        Self::draw_icon_by_index(fb, win.x + sp(8), icon_y, icon_idx);
        let title_col = if win.is_focused {
            Color::TEXT_MAIN
        } else {
            Color::TEXT_MUTED
        };
        let localized_title = Self::window_title(win.app_id as usize, ui_english());
        let title = if !localized_title.is_empty() {
            localized_title
        } else {
            win.title
        };
        // Başlık metnini başlık çubuğunda dikey ortala (font ölçeğine duyarlı).
        // Başlık alanı, simge ile kontrol düğmeleri arasına kırpılır; uzun
        // başlıklar pencere dışına veya düğmelerin üzerine taşmaz.
        let text_h = Self::text_height();
        let title_y = win.y + titlebar_h().saturating_sub(text_h) / 2;
        let title_x = win.x + sp(8) + icon_sz + sp(8);
        let title_area_w = win.width.saturating_sub((title_x - win.x) + sp(76));
        if title_area_w > 0 {
            let old_clip = fb.clip();
            fb.set_clip(title_x, win.y, title_area_w, titlebar_h());
            Self::draw_string(fb, title_x, title_y, title, title_col);
            fb.set_clip(
                old_clip.x0,
                old_clip.y0,
                old_clip.x1.saturating_sub(old_clip.x0),
                old_clip.y1.saturating_sub(old_clip.y0),
            );
        }

        // Kontrol Butonları (hover'da parlama — yuvarlatılmış)
        // Boyut, scale'li rect'ten türetilir; bu sayede font büyüdükçe düğmeler de büyür.
        let (cx, cy, base_cb, _) = win.close_rect();
        let hover_close = in_rect(mx, my, win.close_hit_rect());
        let pressed_close = mouse.left_button && hover_close;
        let press_inset = if pressed_close { 2 } else { 0 };
        let (cx, cy, cb) = (
            cx + press_inset,
            cy + press_inset,
            base_cb.saturating_sub(press_inset * 2),
        );
        let close_bg = if pressed_close {
            Self::shade(Color::ACCENT_RED, -48)
        } else if hover_close {
            Self::shade(Color::ACCENT_RED, 42)
        } else {
            Color::ACCENT_RED
        };
        fb.draw_rounded_rect(cx, cy, cb, cb, 4, close_bg);
        if pressed_close {
            fb.draw_rounded_rect_outline(cx, cy, cb, cb, 4, Color::WHITE);
        }
        let close_mark_color = Self::text_on(close_bg);
        let mark_inset = cb as f32 * 0.29;
        let mark_left = cx as f32 + mark_inset;
        let mark_right = cx as f32 + cb as f32 - mark_inset;
        let mark_top = cy as f32 + mark_inset;
        let mark_bottom = cy as f32 + cb as f32 - mark_inset;
        let mark_width = (sp(16) as f32 / 10.0).max(1.5);
        Self::draw_antialiased_line(
            fb,
            mark_left,
            mark_top,
            mark_right,
            mark_bottom,
            close_mark_color,
            mark_width,
        );
        Self::draw_antialiased_line(
            fb,
            mark_right,
            mark_top,
            mark_left,
            mark_bottom,
            close_mark_color,
            mark_width,
        );

        let (mxr, myr, base_mb, _) = win.max_rect();
        let hover_max = in_rect(mx, my, win.max_hit_rect());
        let pressed_max = mouse.left_button && hover_max;
        let max_inset = if pressed_max { 2 } else { 0 };
        let (mxr, myr, mb) = (
            mxr + max_inset,
            myr + max_inset,
            base_mb.saturating_sub(max_inset * 2),
        );
        let max_bg = if pressed_max {
            Self::shade(Color::ACCENT_GREEN, -48)
        } else if hover_max {
            Self::shade(Color::ACCENT_GREEN, 42)
        } else {
            Color::ACCENT_GREEN
        };
        fb.draw_rounded_rect(mxr, myr, mb, mb, 4, max_bg);
        if pressed_max {
            fb.draw_rounded_rect_outline(mxr, myr, mb, mb, 4, Color::WHITE);
        }
        let max_mark_color = Self::text_on(max_bg);
        let inset = mb as f32 * 0.29;
        let left = mxr as f32 + inset;
        let right = mxr as f32 + mb as f32 - inset;
        let top = myr as f32 + inset;
        let bottom = myr as f32 + mb as f32 - inset;
        let stroke = (sp(14) as f32 / 10.0).max(1.4);
        if win.is_maximized {
            // Restore (Önceki Boyut) simgesi: Arkada sağ-üstte bir pencere karesi ve önde sol-altta bir pencere karesi.
            let size = (right - left) * 0.72;
            let offset = (right - left) * 0.28;

            // Arka pencere (sağ-üst): üst kenar, sağ kenar ve öne kadar uzanan parçalar
            let back_l = left + offset;
            let back_t = top;
            let back_r = back_l + size;
            let back_b = back_t + size;

            // Arka pencere üst kenarı
            Self::draw_antialiased_line(fb, back_l, back_t, back_r, back_t, max_mark_color, stroke);
            // Arka pencere sağ kenarı
            Self::draw_antialiased_line(fb, back_r, back_t, back_r, back_b, max_mark_color, stroke);
            // Arka pencere sol kenarının açıkta kalan üst parçası
            Self::draw_antialiased_line(
                fb,
                back_l,
                back_t,
                back_l,
                top + offset,
                max_mark_color,
                stroke,
            );
            // Arka pencere alt kenarının açıkta kalan sağ parçası
            Self::draw_antialiased_line(
                fb,
                left + size,
                back_b,
                back_r,
                back_b,
                max_mark_color,
                stroke,
            );

            // Ön pencere (sol-alt): tam dikdörtgen
            let front_l = left;
            let front_t = top + offset;
            let front_r = front_l + size;
            let front_b = front_t + size;

            Self::draw_antialiased_line(
                fb,
                front_l,
                front_t,
                front_r,
                front_t,
                max_mark_color,
                stroke,
            );
            Self::draw_antialiased_line(
                fb,
                front_r,
                front_t,
                front_r,
                front_b,
                max_mark_color,
                stroke,
            );
            Self::draw_antialiased_line(
                fb,
                front_r,
                front_b,
                front_l,
                front_b,
                max_mark_color,
                stroke,
            );
            Self::draw_antialiased_line(
                fb,
                front_l,
                front_b,
                front_l,
                front_t,
                max_mark_color,
                stroke,
            );
        } else {
            Self::draw_antialiased_line(
                fb,
                left,
                top,
                right,
                top,
                max_mark_color,
                (sp(14) as f32 / 10.0).max(1.4),
            );
            Self::draw_antialiased_line(
                fb,
                right,
                top,
                right,
                bottom,
                max_mark_color,
                (sp(14) as f32 / 10.0).max(1.4),
            );
            Self::draw_antialiased_line(
                fb,
                right,
                bottom,
                left,
                bottom,
                max_mark_color,
                (sp(14) as f32 / 10.0).max(1.4),
            );
            Self::draw_antialiased_line(
                fb,
                left,
                bottom,
                left,
                top,
                max_mark_color,
                (sp(14) as f32 / 10.0).max(1.4),
            );
        }

        let (nx, ny, base_nb, _) = win.min_rect();
        let hover_min = in_rect(mx, my, win.min_hit_rect());
        let pressed_min = mouse.left_button && hover_min;
        let min_inset = if pressed_min { 2 } else { 0 };
        let (nx, ny, nb) = (
            nx + min_inset,
            ny + min_inset,
            base_nb.saturating_sub(min_inset * 2),
        );
        let min_bg = if pressed_min {
            Self::shade(Color::ACCENT_YELLOW, -48)
        } else if hover_min {
            Self::shade(Color::ACCENT_YELLOW, 42)
        } else {
            Color::ACCENT_YELLOW
        };
        fb.draw_rounded_rect(nx, ny, nb, nb, 4, min_bg);
        if pressed_min {
            fb.draw_rounded_rect_outline(nx, ny, nb, nb, 4, Color::WHITE);
        }
        let bar_inset = nb as f32 * 0.24;
        let bar_y = ny as f32 + nb as f32 * 0.67;
        Self::draw_antialiased_line(
            fb,
            nx as f32 + bar_inset,
            bar_y,
            nx as f32 + nb as f32 - bar_inset,
            bar_y,
            Self::text_on(min_bg),
            (sp(14) as f32 / 10.0).max(1.4),
        );
    }
}
