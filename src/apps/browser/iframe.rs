//! HTML5 <iframe> nested browsing context support (no_std).
//!
//! Provides an IFrame struct that maintains a nested browsing state (URL,
//! rendered page lines, scroll position, loaded state) and can render its
//! content within a parent page.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::types::{BrowserLine, ElementStyle, HtmlTagType};
use crate::gui::Color;

/// State of the iframe's nested browsing context.
#[derive(Clone, PartialEq)]
pub enum IFrameState {
    /// iframe has not been loaded yet.
    Unloaded,
    /// iframe content is being loaded.
    Loading,
    /// iframe content has been loaded and rendered.
    Loaded,
    /// iframe failed to load.
    Error,
}

/// An iframe element with nested browsing context.
#[derive(Clone)]
pub struct IFrame {
    pub id: String,
    pub src: String,
    pub width: usize,
    pub height: usize,
    pub state: IFrameState,
    pub scroll_x: usize,
    pub scroll_y: usize,
    /// Rendered page lines for the nested content.
    pub page_lines: Vec<BrowserLine>,
    /// Page title of nested content.
    pub page_title: String,
    /// Whether the iframe has a border.
    pub has_border: bool,
    /// Sandbox restrictions (simplified).
    pub sandbox: bool,
}

impl IFrame {
    pub fn new(id: String, width: usize, height: usize) -> Self {
        IFrame {
            id,
            src: String::new(),
            width,
            height,
            state: IFrameState::Unloaded,
            scroll_x: 0,
            scroll_y: 0,
            page_lines: Vec::new(),
            page_title: String::new(),
            has_border: true,
            sandbox: false,
        }
    }

    /// Set the iframe's source URL.
    pub fn set_src(&mut self, src: String) {
        self.src = src;
        self.state = IFrameState::Loading;
        self.page_lines.clear();
        self.scroll_x = 0;
        self.scroll_y = 0;
    }

    /// Load content into the iframe (called when HTML is available).
    pub fn load_content(&mut self, html: &[u8], wrap_width_px: usize, max_lines: usize) {
        use super::html::render_html_to_lines;
        use crate::fs::vfs::VirtualFileSystem;

        self.state = IFrameState::Loaded;
        self.page_lines.clear();

        // Render the nested HTML content with reduced width.
        let mut canvases: Vec<super::canvas::CanvasElement> = Vec::new();
        let mut media_players: super::media::MediaPlayers = super::media::MediaPlayers::new();
        let mut previous_styles: BTreeMap<String, ElementStyle> = BTreeMap::new();
        let mut iframes: super::iframe::IFrames = super::iframe::IFrames::new();

        render_html_to_lines(
            html,
            wrap_width_px.saturating_sub(4), // account for border
            &mut self.page_lines,
            max_lines,
            Some(&self.src),
            0,
            0,
            &mut previous_styles,
            None as Option<&mut VirtualFileSystem>,
            &mut canvases,
            &mut media_players,
            &mut iframes,
        );

        // Extract title from first heading line if available
        for line in &self.page_lines {
            for span in &line.spans {
                if !span.text.is_empty() {
                    self.page_title = span.text.clone();
                    break;
                }
            }
            if !self.page_title.is_empty() {
                break;
            }
        }
    }

    /// Render the iframe content to a framebuffer.
    pub fn render_to_fb(
        &self,
        fb: &mut crate::drivers::video::Framebuffer,
        dx: usize,
        dy: usize,
        scroll_y: usize,
    ) {
        let dw = self.width.min(fb.width.saturating_sub(dx));
        let dh = self.height.min(fb.height.saturating_sub(dy));

        // Draw iframe border
        if self.has_border {
            fb.draw_rect(dx, dy, dw, dh, 0xE0E0E0);
            fb.draw_rect_outline(dx, dy, dw, dh, Color::BORDER);
            let inner_x = dx + 2;
            let inner_y = dy + 2;
            let inner_w = dw.saturating_sub(4);
            let inner_h = dh.saturating_sub(4);
            fb.draw_rect(inner_x, inner_y, inner_w, inner_h, 0xFFFFFF);

            // Render content lines
            let mut cur_y = inner_y + 4;
            for line in &self.page_lines {
                let line_h = match line.tag {
                    HtmlTagType::Heading1 => 26,
                    HtmlTagType::Heading2 => 22,
                    HtmlTagType::Heading3 => 20,
                    _ => 18,
                };

                let draw_y = cur_y.saturating_sub(scroll_y);
                if draw_y + line_h >= inner_y && draw_y < inner_y + inner_h {
                    let mut cur_x = inner_x + 4;
                    for span in &line.spans {
                        let text_w = crate::gui::MinGuiCompositor::text_width(&span.text, 1);
                        let span_w = text_w
                            + if span.is_button { 16 } else { 0 }
                            + if span.is_image {
                                span.image_width.max(1)
                            } else {
                                0
                            };

                        if cur_x + span_w > inner_x + inner_w {
                            break;
                        }

                        if draw_y >= inner_y && draw_y < inner_y + inner_h {
                            crate::gui::MinGuiCompositor::draw_string_clipped(
                                fb,
                                cur_x,
                                draw_y,
                                &span.text,
                                span.color,
                                (inner_x, draw_y, inner_w, line_h),
                            );
                        }
                        cur_x += span_w;
                    }
                }
                cur_y += line_h;
            }
        }
    }
}

/// Collection of iframes on the current page.
#[derive(Clone)]
pub struct IFrames {
    pub frames: Vec<IFrame>,
}

impl IFrames {
    pub fn new() -> Self {
        IFrames { frames: Vec::new() }
    }

    pub fn add(&mut self, frame: IFrame) {
        self.frames.push(frame);
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut IFrame> {
        self.frames.iter_mut().find(|f| f.id == id)
    }

    pub fn get(&self, id: &str) -> Option<&IFrame> {
        self.frames.iter().find(|f| f.id == id)
    }
}
