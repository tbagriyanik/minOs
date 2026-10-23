//! minOs Widget / Bileşen Ağacı Mimarisi
//!
//! Tüm arayüz denetimleri (düğme, metin kutusu, onay kutusu, kaydırıcı) tek
//! bir `Widget` trait'i uygular. Denetimler bir `Container` ağacında toplanır;
//! yerleşim her `layout()` çağrısında geçerli pencere boyutuna göre yeniden
//! hesaplanır (dinamik yerleşim). `Ui` odak yöneticisi Tab / Shift+Tab ile
//! denetimler arasında gezinir, fare olaylarını ağaçtaki en üstteki denetime
//! yönlendirir ve odaklı denetimin çevresine odak halkası çizer.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::any::Any;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::drivers::video::Framebuffer;
use crate::gui::{sp, Color, MinGuiCompositor, TextBuf};

static NEXT_WIDGET_ID: AtomicU32 = AtomicU32::new(1);

fn next_id() -> u32 {
    let id = NEXT_WIDGET_ID.fetch_add(1, Ordering::Relaxed);
    if id == 0 {
        NEXT_WIDGET_ID.fetch_add(1, Ordering::Relaxed)
    } else {
        id
    }
}

// ---------------------------------------------------------------------------
// Geometri
// ---------------------------------------------------------------------------

/// Ekran koordinatlarında dikdörtgen.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub const fn new(x: usize, y: usize, w: usize, h: usize) -> Self {
        Self { x, y, w, h }
    }

    pub const fn from_tuple(r: (usize, usize, usize, usize)) -> Self {
        Self { x: r.0, y: r.1, w: r.2, h: r.3 }
    }

    pub const fn tuple(self) -> (usize, usize, usize, usize) {
        (self.x, self.y, self.w, self.h)
    }

    pub const fn right(self) -> usize {
        self.x + self.w
    }

    pub const fn bottom(self) -> usize {
        self.y + self.h
    }

    pub fn contains(self, x: usize, y: usize) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn inset(self, insets: Insets) -> Self {
        Self {
            x: self.x + insets.left,
            y: self.y + insets.top,
            w: self.w.saturating_sub(insets.left + insets.right),
            h: self.h.saturating_sub(insets.top + insets.bottom),
        }
    }

    pub fn with_origin(self, x: usize, y: usize) -> Self {
        Self { x, y, ..self }
    }
}

/// Kenar boşlukları.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Insets {
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub left: usize,
}

impl Insets {
    pub const fn all(v: usize) -> Self {
        Self { top: v, right: v, bottom: v, left: v }
    }

    pub const fn xy(x: usize, y: usize) -> Self {
        Self { top: y, right: x, bottom: y, left: x }
    }

    pub const fn new(top: usize, right: usize, bottom: usize, left: usize) -> Self {
        Self { top, right, bottom, left }
    }

    pub const fn horizontal(self) -> usize {
        self.left + self.right
    }

    pub const fn vertical(self) -> usize {
        self.top + self.bottom
    }
}

/// Yerleşim ekseni.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

fn pick_sizing(axis: Axis, width: Sizing, height: Sizing) -> Sizing {
    match axis {
        Axis::Horizontal => width,
        Axis::Vertical => height,
    }
}

/// Bir denetimin ana eksende nasıl boyutlanacağı.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sizing {
    /// İçerik boyutu kadar yer kaplar.
    Content,
    /// Kalan boşluğu verilen ağırlıkla paylaşır.
    Fill(u16),
}

/// İstenen içerik boyutu (piksel).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SizeHint {
    pub width: usize,
    pub height: usize,
}

// ---------------------------------------------------------------------------
// Girdi olayları
// ---------------------------------------------------------------------------

/// Klavye olayı (ham PS/2 baytından çevrilebilir).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Tab,
    BackTab,
    Enter,
    Space,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Backspace,
    Delete,
    Char(u8),
}

impl Key {
    /// Ham klavye baytını (`ch`) widget anahtarına çevirir.
    /// Shift basılıyken Tab `BackTab` olur.
    pub fn from_byte(byte: u8, shift: bool) -> Option<Key> {
        match byte {
            b'\t' => Some(if shift { Key::BackTab } else { Key::Tab }),
            b'\n' | b'\r' => Some(Key::Enter),
            b' ' => Some(Key::Space),
            0x1b => Some(Key::Escape),
            0x08 => Some(Key::Backspace),
            0x7f => Some(Key::Delete),
            0x20..=0x7e => Some(Key::Char(byte)),
            _ => None,
        }
    }

    pub fn is_activate(self) -> bool {
        matches!(self, Key::Enter | Key::Space)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    Move,
    Down,
    Up,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PointerEvent {
    pub x: usize,
    pub y: usize,
    pub kind: PointerKind,
}

impl PointerEvent {
    pub const fn new(x: usize, y: usize, kind: PointerKind) -> Self {
        Self { x, y, kind }
    }
}

/// Denetimlerin uygulamaya bildirdiği sonuçlar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WidgetEvent {
    Ignored,
    FocusChanged(u32),
    Clicked(u32),
    Toggled(u32, bool),
    Changed(u32, i32),
    TextEdited(u32),
    Submitted(u32),
}

impl WidgetEvent {
    pub fn handled(self) -> bool {
        !matches!(self, WidgetEvent::Ignored)
    }

    pub fn id(self) -> Option<u32> {
        match self {
            WidgetEvent::Ignored => None,
            WidgetEvent::FocusChanged(id)
            | WidgetEvent::Clicked(id)
            | WidgetEvent::Toggled(id, _)
            | WidgetEvent::Changed(id, _)
            | WidgetEvent::TextEdited(id)
            | WidgetEvent::Submitted(id) => Some(id),
        }
    }
}

/// Çizim sırasında odak ve fare üzerinde olma durumu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DrawCtx {
    pub focused: u32,
    pub hovered: u32,
}

impl DrawCtx {
    pub const fn new(focused: u32, hovered: u32) -> Self {
        Self { focused, hovered }
    }

    pub fn is_focused(self, id: u32) -> bool {
        self.focused != 0 && self.focused == id
    }

    pub fn is_hovered(self, id: u32) -> bool {
        self.hovered != 0 && self.hovered == id
    }
}

// ---------------------------------------------------------------------------
// Widget trait'i
// ---------------------------------------------------------------------------

/// Tüm arayüz denetimlerinin ortak sözleşmesi.
pub trait Widget {
    fn id(&self) -> u32;
    fn rect(&self) -> Rect;
    /// Yeni geometriyi atar; kapsayıcılar burada çocuklarını yeniden yerleştirir.
    fn set_rect(&mut self, rect: Rect);
    fn draw(&self, fb: &mut Framebuffer, ctx: &DrawCtx);

    /// Ana eksende gereken içerik boyutu.
    fn measure(&self, _cross_extent: usize, _axis: Axis) -> usize {
        0
    }

    /// (genişlik, yükseklik) boyutlandırma politikası.
    fn sizing(&self) -> (Sizing, Sizing) {
        (Sizing::Content, Sizing::Content)
    }

    /// Tab sırasına girer mi?
    fn focusable(&self) -> bool {
        false
    }

    fn visible(&self) -> bool {
        true
    }

    fn enabled(&self) -> bool {
        true
    }

    fn on_key(&mut self, _key: Key) -> WidgetEvent {
        WidgetEvent::Ignored
    }

    fn on_pointer(&mut self, _event: PointerEvent) -> WidgetEvent {
        WidgetEvent::Ignored
    }

    fn on_focus(&mut self, _gained: bool) {}

    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

fn find<'a>(node: &'a dyn Widget, id: u32) -> Option<&'a dyn Widget> {
    if node.id() == id {
        return Some(node);
    }
    if let Some(container) = node.as_any().downcast_ref::<Container>() {
        for child in &container.children {
            if let Some(found) = find(child.as_ref(), id) {
                return Some(found);
            }
        }
    }
    None
}

fn find_mut<'a>(node: &'a mut dyn Widget, id: u32) -> Option<&'a mut dyn Widget> {
    if node.id() == id {
        return Some(node);
    }
    if let Some(container) = node.as_any_mut().downcast_mut::<Container>() {
        for child in container.children.iter_mut() {
            if let Some(found) = find_mut(child.as_mut(), id) {
                return Some(found);
            }
        }
    }
    None
}

fn collect_focusables(node: &dyn Widget, out: &mut Vec<u32>) {
    if !node.visible() || !node.enabled() {
        return;
    }
    if node.focusable() {
        out.push(node.id());
    }
    if let Some(container) = node.as_any().downcast_ref::<Container>() {
        for child in &container.children {
            collect_focusables(child.as_ref(), out);
        }
    }
}

fn hit_test(node: &dyn Widget, x: usize, y: usize) -> u32 {
    if let Some(container) = node.as_any().downcast_ref::<Container>() {
        if !container.rect.contains(x, y) {
            return 0;
        }
        for child in container.children.iter().rev() {
            if !child.visible() || !child.enabled() {
                continue;
            }
            if !child.rect().contains(x, y) {
                continue;
            }
            let hit = hit_test(child.as_ref(), x, y);
            if hit != 0 {
                return hit;
            }
        }
        return 0;
    }
    if node.visible() && node.enabled() && node.rect().contains(x, y) {
        node.id()
    } else {
        0
    }
}

fn text_top(rect: Rect) -> usize {
    rect.y + rect.h.saturating_sub(MinGuiCompositor::text_height()) / 2
}

// ---------------------------------------------------------------------------
// Yerleşim kapsayıcısı
// ---------------------------------------------------------------------------

/// Çocuk denetimleri tek eksende yerleştiren bileşen.
pub const DEFAULT_PADDING: usize = 8;

pub struct Container {
    id: u32,
    rect: Rect,
    axis: Axis,
    padding: Insets,
    spacing: usize,
    width: Sizing,
    height: Sizing,
    children: Vec<Box<dyn Widget>>,
}

impl Container {
    pub fn new(axis: Axis) -> Self {
        Self {
            id: next_id(),
            rect: Rect::default(),
            axis,
            padding: Insets::all(DEFAULT_PADDING),
            spacing: sp(6),
            width: Sizing::Content,
            height: Sizing::Content,
            children: Vec::new(),
        }
    }

    pub fn pad(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    pub fn gap(mut self, spacing: usize) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn size(mut self, width: Sizing, height: Sizing) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn push<W: Widget + 'static>(&mut self, widget: W) -> &mut Self {
        self.children.push(Box::new(widget));
        self
    }

    pub fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    pub fn children_mut(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.children
    }

    /// Mevcut boyuta göre tüm ağacı yeniden yerleştirir.
    /// `Fill(w)` çocuklar kalan boşluğu ağırlıkları oranında paylaşır.
    pub fn relayout(&mut self, rect: Rect) {
        self.rect = rect;
        let inner = rect.inset(self.padding);
        let count = self.children.iter().filter(|c| c.visible()).count();
        if count == 0 {
            return;
        }

        let (cross, main_len) = match self.axis {
            Axis::Vertical => (inner.w, inner.h),
            Axis::Horizontal => (inner.h, inner.w),
        };

        let mut content_total = self.spacing * (count - 1);
        let mut fill_weight: u32 = 0;
        for child in self.children.iter().filter(|c| c.visible()) {
            content_total += child.measure(cross, self.axis);
            let sizing = child.sizing();
            if let Sizing::Fill(weight) = pick_sizing(self.axis, sizing.0, sizing.1) {
                fill_weight += weight.max(1) as u32;
            }
        }

        let mut remaining = main_len.saturating_sub(content_total);
        let mut fill_left = fill_weight;
        let mut cursor = match self.axis {
            Axis::Vertical => inner.y,
            Axis::Horizontal => inner.x,
        };

        for i in 0..self.children.len() {
            if !self.children[i].visible() {
                continue;
            }
            let sizing = self.children[i].sizing();
            let mut main = self.children[i].measure(cross, self.axis);
            if let Sizing::Fill(weight) = pick_sizing(self.axis, sizing.0, sizing.1) {
                let weight = weight.max(1) as u32;
                let share = if fill_left > 0 {
                    remaining * weight as usize / fill_left as usize
                } else {
                    0
                };
                main += share;
                remaining = remaining.saturating_sub(share);
                fill_left = fill_left.saturating_sub(weight);
            }
            let child_rect = match self.axis {
                Axis::Vertical => Rect::new(inner.x, cursor, cross, main),
                Axis::Horizontal => Rect::new(cursor, inner.y, main, cross),
            };
            self.children[i].set_rect(child_rect);
            cursor = cursor.saturating_add(main).saturating_add(self.spacing);
        }
    }
}

impl Widget for Container {
    fn id(&self) -> u32 {
        self.id
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.relayout(rect);
    }

    fn draw(&self, fb: &mut Framebuffer, ctx: &DrawCtx) {
        for child in &self.children {
            if child.visible() {
                child.draw(fb, ctx);
            }
        }
    }

    fn measure(&self, cross_extent: usize, axis: Axis) -> usize {
        let count = self.children.iter().filter(|c| c.visible()).count();
        if count == 0 {
            return 0;
        }
        let mut total = self.spacing * (count - 1);
        for child in self.children.iter().filter(|c| c.visible()) {
            total += child.measure(cross_extent, axis);
        }
        total
            + match axis {
                Axis::Horizontal => self.padding.horizontal(),
                Axis::Vertical => self.padding.vertical(),
            }
    }

    fn sizing(&self) -> (Sizing, Sizing) {
        (self.width, self.height)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Etiket
// ---------------------------------------------------------------------------

pub struct Label {
    id: u32,
    rect: Rect,
    text: &'static str,
    color: u32,
}

impl Label {
    pub fn new(text: &'static str) -> Self {
        Self { id: next_id(), rect: Rect::default(), text, color: Color::TEXT_MAIN }
    }

    pub fn color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }

    pub fn set_text(&mut self, text: &'static str) {
        self.text = text;
    }
}

impl Widget for Label {
    fn id(&self) -> u32 {
        self.id
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn draw(&self, fb: &mut Framebuffer, _ctx: &DrawCtx) {
        MinGuiCompositor::draw_string_clipped(
            fb,
            self.rect.x,
            text_top(self.rect),
            self.text,
            self.color,
            self.rect.tuple(),
        );
    }

    fn measure(&self, _cross_extent: usize, axis: Axis) -> usize {
        match axis {
            Axis::Horizontal => MinGuiCompositor::text_width(self.text, 1),
            Axis::Vertical => MinGuiCompositor::text_height(),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Düğme
// ---------------------------------------------------------------------------

pub struct Button {
    id: u32,
    rect: Rect,
    label: &'static str,
    bg: u32,
    pressed: bool,
}

impl Button {
    pub fn new(label: &'static str) -> Self {
        Self { id: next_id(), rect: Rect::default(), label, bg: Color::ACCENT_BLUE, pressed: false }
    }

    pub fn bg(mut self, color: u32) -> Self {
        self.bg = color;
        self
    }

    pub fn set_label(&mut self, label: &'static str) {
        self.label = label;
    }

    pub fn label(&self) -> &'static str {
        self.label
    }
}

impl Widget for Button {
    fn id(&self) -> u32 {
        self.id
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn draw(&self, fb: &mut Framebuffer, ctx: &DrawCtx) {
        let bg = if self.pressed { MinGuiCompositor::shade(self.bg, -20) } else { self.bg };
        let hovered = ctx.is_hovered(self.id);
        MinGuiCompositor::draw_button(fb, self.rect.tuple(), bg, self.label, Color::TEXT_MAIN, hovered);
    }

    fn measure(&self, _cross_extent: usize, axis: Axis) -> usize {
        match axis {
            Axis::Horizontal => MinGuiCompositor::text_width(self.label, 1) + sp(24),
            Axis::Vertical => sp(28),
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_key(&mut self, key: Key) -> WidgetEvent {
        if key.is_activate() {
            WidgetEvent::Clicked(self.id)
        } else {
            WidgetEvent::Ignored
        }
    }

    fn on_pointer(&mut self, event: PointerEvent) -> WidgetEvent {
        match event.kind {
            PointerKind::Down => {
                self.pressed = true;
                WidgetEvent::Ignored
            }
            PointerKind::Up => {
                let clicked = self.pressed && self.rect.contains(event.x, event.y);
                self.pressed = false;
                if clicked {
                    WidgetEvent::Clicked(self.id)
                } else {
                    WidgetEvent::Ignored
                }
            }
            PointerKind::Move => WidgetEvent::Ignored,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Onay kutusu
// ---------------------------------------------------------------------------

pub struct CheckBox {
    id: u32,
    rect: Rect,
    label: &'static str,
    checked: bool,
    pressed: bool,
}

impl CheckBox {
    pub fn new(label: &'static str) -> Self {
        Self { id: next_id(), rect: Rect::default(), label, checked: false, pressed: false }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn is_checked(&self) -> bool {
        self.checked
    }

    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
    }

    fn toggle(&mut self) -> WidgetEvent {
        self.checked = !self.checked;
        WidgetEvent::Toggled(self.id, self.checked)
    }

    fn box_rect(&self) -> Rect {
        let size = sp(16);
        Rect::new(
            self.rect.x,
            self.rect.y + self.rect.h.saturating_sub(size) / 2,
            size,
            size,
        )
    }
}

impl Widget for CheckBox {
    fn id(&self) -> u32 {
        self.id
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn draw(&self, fb: &mut Framebuffer, ctx: &DrawCtx) {
        let b = self.box_rect();
        let border = if ctx.is_focused(self.id) || self.checked {
            Color::ACCENT_BLUE
        } else {
            Color::BORDER
        };
        fb.draw_rounded_rect(b.x, b.y, b.w, b.h, 3, Color::DARK_BG);
        fb.draw_rounded_rect_outline(b.x, b.y, b.w, b.h, 3, border);
        if self.checked {
            let tick = Color::ACCENT_GREEN;
            fb.draw_line(b.x + sp(4), b.y + sp(8), b.x + sp(7), b.y + sp(11), tick);
            fb.draw_line(b.x + sp(7), b.y + sp(11), b.x + sp(12), b.y + sp(5), tick);
        }
        let tx = b.right() + sp(8);
        MinGuiCompositor::draw_string_clipped(
            fb,
            tx,
            text_top(self.rect),
            self.label,
            Color::TEXT_MAIN,
            (tx, self.rect.y, self.rect.right().saturating_sub(tx), self.rect.h),
        );
    }

    fn measure(&self, _cross_extent: usize, axis: Axis) -> usize {
        match axis {
            Axis::Horizontal => sp(16) + sp(8) + MinGuiCompositor::text_width(self.label, 1),
            Axis::Vertical => sp(22),
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_key(&mut self, key: Key) -> WidgetEvent {
        if key == Key::Space {
            self.toggle()
        } else {
            WidgetEvent::Ignored
        }
    }

    fn on_pointer(&mut self, event: PointerEvent) -> WidgetEvent {
        match event.kind {
            PointerKind::Down => {
                self.pressed = self.rect.contains(event.x, event.y);
                WidgetEvent::Ignored
            }
            PointerKind::Up => {
                let hit = self.pressed && self.rect.contains(event.x, event.y);
                self.pressed = false;
                if hit {
                    self.toggle()
                } else {
                    WidgetEvent::Ignored
                }
            }
            PointerKind::Move => WidgetEvent::Ignored,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Metin kutusu
// ---------------------------------------------------------------------------

pub const TEXTBOX_CAPACITY: usize = 64;

pub struct TextBox {
    id: u32,
    rect: Rect,
    buf: [u8; TEXTBOX_CAPACITY],
    len: usize,
    cursor: usize,
    max_len: usize,
    placeholder: &'static str,
    masked: bool,
    focused: bool,
}

impl TextBox {
    pub fn new(placeholder: &'static str) -> Self {
        Self {
            id: next_id(),
            rect: Rect::default(),
            buf: [0; TEXTBOX_CAPACITY],
            len: 0,
            cursor: 0,
            max_len: TEXTBOX_CAPACITY,
            placeholder,
            masked: false,
            focused: false,
        }
    }

    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    pub fn max_len(mut self, max_len: usize) -> Self {
        self.max_len = max_len.min(TEXTBOX_CAPACITY);
        if self.len > self.max_len {
            self.len = self.max_len;
            self.cursor = self.cursor.min(self.len);
        }
        self
    }

    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    /// Metni ayarlar. Tampon yalnızca ASCII yazdırılabilir baytları kabul eder;
    /// böylece imleç/parça dilimleri her zaman geçerli UTF-8 kalır.
    pub fn set_text(&mut self, text: &str) {
        self.len = 0;
        self.cursor = 0;
        for b in text.bytes() {
            if self.len >= self.max_len {
                break;
            }
            if (0x20..=0x7e).contains(&b) {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        self.cursor = self.len;
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.cursor = 0;
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn insert(&mut self, byte: u8) -> bool {
        if self.len >= self.max_len {
            return false;
        }
        let mut i = self.len;
        while i > self.cursor {
            self.buf[i] = self.buf[i - 1];
            i -= 1;
        }
        self.buf[self.cursor] = byte;
        self.len += 1;
        self.cursor += 1;
        true
    }

    fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let mut i = self.cursor - 1;
        while i + 1 < self.len {
            self.buf[i] = self.buf[i + 1];
            i += 1;
        }
        self.len -= 1;
        self.cursor -= 1;
        true
    }

    fn delete(&mut self) -> bool {
        if self.cursor >= self.len {
            return false;
        }
        let mut i = self.cursor;
        while i + 1 < self.len {
            self.buf[i] = self.buf[i + 1];
            i += 1;
        }
        self.len -= 1;
        true
    }

    fn visible_text(&self) -> [u8; TEXTBOX_CAPACITY] {
        let mut out = [0u8; TEXTBOX_CAPACITY];
        if self.masked {
            for i in 0..self.len {
                out[i] = b'*';
            }
        } else {
            out[..self.len].copy_from_slice(&self.buf[..self.len]);
        }
        out
    }

    fn inner_rect(&self) -> Rect {
        Rect::new(
            self.rect.x + sp(8),
            self.rect.y + sp(4),
            self.rect.w.saturating_sub(sp(16)),
            self.rect.h.saturating_sub(sp(8)),
        )
    }
}

impl Widget for TextBox {
    fn id(&self) -> u32 {
        self.id
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn draw(&self, fb: &mut Framebuffer, ctx: &DrawCtx) {
        let focused = ctx.is_focused(self.id) || self.focused;
        fb.draw_rounded_rect(self.rect.x, self.rect.y, self.rect.w, self.rect.h, 4, Color::DARK_BG);
        fb.draw_rounded_rect_outline(
            self.rect.x,
            self.rect.y,
            self.rect.w,
            self.rect.h,
            4,
            if focused { Color::ACCENT_BLUE } else { Color::BORDER },
        );

        let inner = self.inner_rect();
        let ty = text_top(inner);
        if self.len == 0 && !focused {
            MinGuiCompositor::draw_string_clipped(
                fb,
                inner.x,
                ty,
                self.placeholder,
                Color::TEXT_MUTED,
                inner.tuple(),
            );
            return;
        }

        let visible = self.visible_text();
        let text = core::str::from_utf8(&visible[..self.len]).unwrap_or("");
        MinGuiCompositor::draw_string_clipped(fb, inner.x, ty, text, Color::TEXT_MAIN, inner.tuple());

        if focused {
            let prefix = &visible[..self.cursor.min(self.len)];
            let prefix_str = core::str::from_utf8(prefix).unwrap_or("");
            let caret_x = inner.x + MinGuiCompositor::text_width(prefix_str, 1);
            if caret_x < inner.right() {
                fb.draw_rect(caret_x, ty, 1, MinGuiCompositor::text_height(), Color::ACCENT_BLUE);
            }
        }
    }

    fn measure(&self, _cross_extent: usize, axis: Axis) -> usize {
        match axis {
            Axis::Horizontal => sp(180),
            Axis::Vertical => sp(28),
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_focus(&mut self, gained: bool) {
        self.focused = gained;
        if gained {
            self.cursor = self.len;
        }
    }

    fn on_key(&mut self, key: Key) -> WidgetEvent {
        match key {
            Key::Char(byte) => {
                if self.insert(byte) {
                    WidgetEvent::TextEdited(self.id)
                } else {
                    WidgetEvent::Ignored
                }
            }
            Key::Backspace => {
                if self.backspace() {
                    WidgetEvent::TextEdited(self.id)
                } else {
                    WidgetEvent::Ignored
                }
            }
            Key::Delete => {
                if self.delete() {
                    WidgetEvent::TextEdited(self.id)
                } else {
                    WidgetEvent::Ignored
                }
            }
            Key::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                WidgetEvent::Ignored
            }
            Key::Right => {
                if self.cursor < self.len {
                    self.cursor += 1;
                }
                WidgetEvent::Ignored
            }
            Key::Home => {
                self.cursor = 0;
                WidgetEvent::Ignored
            }
            Key::End => {
                self.cursor = self.len;
                WidgetEvent::Ignored
            }
            Key::Enter => WidgetEvent::Submitted(self.id),
            _ => WidgetEvent::Ignored,
        }
    }

    fn on_pointer(&mut self, event: PointerEvent) -> WidgetEvent {
        if event.kind == PointerKind::Down && self.rect.contains(event.x, event.y) {
            let inner = self.inner_rect();
            // Tıklanan noktaya en yakın karakter konumuna imleci taşı.
            let text = self.text();
            let mut best = self.len;
            for i in 0..=self.len {
                let x = inner.x + MinGuiCompositor::text_width(&text[..i], 1);
                if event.x <= x {
                    best = i;
                    break;
                }
            }
            self.cursor = best;
        }
        WidgetEvent::Ignored
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Kaydırıcı
// ---------------------------------------------------------------------------

pub struct Slider {
    id: u32,
    rect: Rect,
    label: &'static str,
    value: i32,
    min: i32,
    max: i32,
    step: i32,
    dragging: bool,
}

impl Slider {
    pub fn new(label: &'static str, value: i32, min: i32, max: i32) -> Self {
        let value = value.clamp(min, max);
        Self { id: next_id(), rect: Rect::default(), label, value, min, max, step: 1, dragging: false }
    }

    pub fn step(mut self, step: i32) -> Self {
        self.step = step.max(1);
        self
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    pub fn set_value(&mut self, value: i32) {
        self.value = value.clamp(self.min, self.max);
    }

    fn track_rect(&self) -> Rect {
        let h = sp(6);
        Rect::new(
            self.rect.x,
            self.rect.y + self.rect.h.saturating_sub(h + sp(8)),
            self.rect.w,
            h,
        )
    }

    fn thumb_rect(&self) -> Rect {
        let track = self.track_rect();
        let thumb = sp(12);
        let span = (self.max - self.min).max(1) as usize;
        let usable = track.w.saturating_sub(thumb);
        let rel = (self.value - self.min).max(0) as usize * usable / span;
        Rect::new(
            track.x + rel,
            track.y.saturating_sub(thumb.saturating_sub(track.h) / 2),
            thumb,
            thumb,
        )
    }

    fn set_from_x(&mut self, x: usize) -> bool {
        let track = self.track_rect();
        let thumb = sp(12);
        let usable = track.w.saturating_sub(thumb).max(1);
        let span = (self.max - self.min).max(1) as usize;
        let rel = x.clamp(track.x, track.x + usable).saturating_sub(track.x);
        let raw = self.min + (rel * span / usable) as i32;
        let step = self.step.max(1);
        let quantized = ((raw + step / 2) / step) * step;
        let next = quantized.clamp(self.min, self.max);
        if next != self.value {
            self.value = next;
            true
        } else {
            false
        }
    }

    fn nudge(&mut self, delta: i32) -> WidgetEvent {
        let next = (self.value + delta).clamp(self.min, self.max);
        if next != self.value {
            self.value = next;
            WidgetEvent::Changed(self.id, self.value)
        } else {
            WidgetEvent::Ignored
        }
    }
}

impl Widget for Slider {
    fn id(&self) -> u32 {
        self.id
    }

    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn draw(&self, fb: &mut Framebuffer, ctx: &DrawCtx) {
        if !self.label.is_empty() {
            MinGuiCompositor::draw_string_clipped(
                fb,
                self.rect.x,
                self.rect.y,
                self.label,
                Color::TEXT_MAIN,
                (self.rect.x, self.rect.y, self.rect.w, MinGuiCompositor::text_height()),
            );
        }

        let mut value_text = TextBuf::new();
        value_text.push_usize(self.value.max(0) as usize);
        let value_w = MinGuiCompositor::text_width(value_text.as_str(), 1);
        MinGuiCompositor::draw_string(
            fb,
            self.rect.right().saturating_sub(value_w),
            self.rect.y,
            value_text.as_str(),
            if ctx.is_focused(self.id) { Color::ACCENT_BLUE } else { Color::TEXT_MUTED },
        );

        let track = self.track_rect();
        let thumb = self.thumb_rect();
        fb.draw_rounded_rect(track.x, track.y, track.w, track.h, track.h / 2, Color::DARK_BG);
        fb.draw_rounded_rect_outline(track.x, track.y, track.w, track.h, track.h / 2, Color::BORDER);

        let filled = thumb.x.saturating_add(thumb.w / 2).saturating_sub(track.x);
        if filled > 1 {
            fb.draw_rounded_rect(
                track.x,
                track.y,
                filled.min(track.w),
                track.h,
                track.h / 2,
                if self.dragging { Color::ACCENT_GREEN } else { Color::ACCENT_BLUE },
            );
        }
        fb.draw_rounded_rect(thumb.x, thumb.y, thumb.w, thumb.h, thumb.w / 2, Color::ACCENT_BLUE);
        fb.draw_rounded_rect_outline(
            thumb.x,
            thumb.y,
            thumb.w,
            thumb.h,
            thumb.w / 2,
            Color::WHITE,
        );
    }

    fn measure(&self, _cross_extent: usize, axis: Axis) -> usize {
        match axis {
            Axis::Horizontal => sp(180),
            Axis::Vertical => sp(42),
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn on_key(&mut self, key: Key) -> WidgetEvent {
        match key {
            Key::Left | Key::Down => self.nudge(-self.step),
            Key::Right | Key::Up => self.nudge(self.step),
            Key::Home => {
                let next = self.min;
                if next != self.value {
                    self.value = next;
                    WidgetEvent::Changed(self.id, self.value)
                } else {
                    WidgetEvent::Ignored
                }
            }
            Key::End => {
                let next = self.max;
                if next != self.value {
                    self.value = next;
                    WidgetEvent::Changed(self.id, self.value)
                } else {
                    WidgetEvent::Ignored
                }
            }
            _ => WidgetEvent::Ignored,
        }
    }

    fn on_pointer(&mut self, event: PointerEvent) -> WidgetEvent {
        match event.kind {
            PointerKind::Down => {
                if self.rect.contains(event.x, event.y) {
                    self.dragging = true;
                    if self.set_from_x(event.x) {
                        return WidgetEvent::Changed(self.id, self.value);
                    }
                }
                WidgetEvent::Ignored
            }
            PointerKind::Move => {
                if self.dragging && self.set_from_x(event.x) {
                    WidgetEvent::Changed(self.id, self.value)
                } else {
                    WidgetEvent::Ignored
                }
            }
            PointerKind::Up => {
                self.dragging = false;
                WidgetEvent::Ignored
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Odak yöneticisi / kök ağaç
// ---------------------------------------------------------------------------

pub struct Ui {
    root: Container,
    focused: u32,
    hovered: u32,
    captured: u32,
    active_tree: bool,
}

impl Ui {
    pub fn new(root: Container) -> Self {
        Self {
            root,
            focused: 0,
            hovered: 0,
            captured: 0,
            active_tree: false,
        }
    }

    pub fn root(&self) -> &Container {
        &self.root
    }

    pub fn root_mut(&mut self) -> &mut Container {
        &mut self.root
    }

    /// Pencere boyutu değiştiğinde tüm ağacı yeniden yerleştirir.
    pub fn layout(&mut self, rect: Rect) {
        self.root.relayout(rect);
        if self.focused != 0 && find(&self.root, self.focused).is_none() {
            let mut order = Vec::new();
            collect_focusables(&self.root, &mut order);
            self.focused = order.first().copied().unwrap_or(0);
        }
    }

    pub fn focused(&self) -> u32 {
        self.focused
    }

    pub fn hovered(&self) -> u32 {
        self.hovered
    }

    /// Tab sırasına giren denetim kimlikleri (ağaç sırasıyla).
    pub fn focus_order(&self) -> Vec<u32> {
        let mut order = Vec::new();
        collect_focusables(&self.root, &mut order);
        order
    }

    pub fn set_focus(&mut self, id: u32) -> u32 {
        if self.focused == id {
            return self.focused;
        }
        if let Some(widget) = find_mut(&mut self.root, self.focused) {
            widget.on_focus(false);
        }
        self.focused = if id != 0 && find(&self.root, id).map_or(false, |w| w.focusable()) {
            id
        } else {
            0
        };
        if let Some(widget) = find_mut(&mut self.root, self.focused) {
            widget.on_focus(true);
        }
        self.focused
    }

    /// Tab / Shift+Tab ile sıradaki (veya önceki) denetime geçer.
    pub fn focus_next(&mut self, reverse: bool) -> u32 {
        let order = self.focus_order();
        if order.is_empty() {
            return self.set_focus(0);
        }
        let current = order.iter().position(|id| *id == self.focused);
        let index = match (current, reverse) {
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
            (None, false) => 0,
            (None, true) => order.len() - 1,
        };
        self.set_focus(order[index])
    }

    /// Klavye olayını yönlendirir. Tab/Shift+Tab her zaman odak taşır.
    pub fn handle_key(&mut self, key: Key) -> WidgetEvent {
        match key {
            Key::Tab => {
                self.active_tree = true;
                WidgetEvent::FocusChanged(self.focus_next(false))
            }
            Key::BackTab => {
                self.active_tree = true;
                WidgetEvent::FocusChanged(self.focus_next(true))
            }
            _ => {
                if self.focused == 0 {
                    return WidgetEvent::Ignored;
                }
                match find_mut(&mut self.root, self.focused) {
                    Some(widget) => widget.on_key(key),
                    None => WidgetEvent::Ignored,
                }
            }
        }
    }

    /// Fare olayını ağaçtaki hedefe yönlendirir; basılı tutma (sürükleme)
    /// kaydırıcı gibi denetimler için yakalanır.
    pub fn handle_pointer(&mut self, event: PointerEvent) -> WidgetEvent {
        match event.kind {
            PointerKind::Down => {
                let hit = hit_test(&self.root, event.x, event.y);
                if hit == 0 {
                    self.captured = 0;
                    return WidgetEvent::Ignored;
                }
                self.active_tree = true;
                if find(&self.root, hit).map_or(false, |w| w.focusable()) {
                    self.set_focus(hit);
                }
                self.captured = hit;
                match find_mut(&mut self.root, hit) {
                    Some(widget) => widget.on_pointer(event),
                    None => WidgetEvent::Ignored,
                }
            }
            PointerKind::Move => {
                self.hovered = hit_test(&self.root, event.x, event.y);
                let target = if self.captured != 0 { self.captured } else { self.hovered };
                if target == 0 {
                    return WidgetEvent::Ignored;
                }
                match find_mut(&mut self.root, target) {
                    Some(widget) => widget.on_pointer(event),
                    None => WidgetEvent::Ignored,
                }
            }
            PointerKind::Up => {
                let target = self.captured;
                self.captured = 0;
                if target == 0 {
                    return WidgetEvent::Ignored;
                }
                match find_mut(&mut self.root, target) {
                    Some(widget) => widget.on_pointer(event),
                    None => WidgetEvent::Ignored,
                }
            }
        }
    }

    /// Ağacı çizer ve etkin odak varsa çevresine halka ekler.
    pub fn draw(&self, fb: &mut Framebuffer) {
        let ctx = DrawCtx::new(
            if self.active_tree { self.focused } else { 0 },
            self.hovered,
        );
        self.root.draw(fb, &ctx);

        if !self.active_tree || self.focused == 0 {
            return;
        }
        let Some(widget) = find(&self.root, self.focused) else {
            return;
        };
        if !widget.visible() {
            return;
        }
        let r = widget.rect();
        let x2 = r.x.saturating_sub(2);
        let y2 = r.y.saturating_sub(2);
        let x1 = r.x.saturating_sub(1);
        let y1 = r.y.saturating_sub(1);
        fb.draw_rect_outline(x2, y2, r.w + 4, r.h + 4, Color::ACCENT_BLUE);
        fb.draw_rect_outline(x1, y1, r.w + 2, r.h + 2, Color::ACCENT_BLUE);
    }

    pub fn widget(&self, id: u32) -> Option<&dyn Widget> {
        find(&self.root, id)
    }

    pub fn widget_mut(&mut self, id: u32) -> Option<&mut dyn Widget> {
        find_mut(&mut self.root, id)
    }

    pub fn widget_as<T: 'static>(&self, id: u32) -> Option<&T> {
        self.widget(id)?.as_any().downcast_ref::<T>()
    }

    pub fn widget_as_mut<T: 'static>(&mut self, id: u32) -> Option<&mut T> {
        self.widget_mut(id)?.as_any_mut().downcast_mut::<T>()
    }
}

// ---------------------------------------------------------------------------
// Önyükleme doğrulama testi
// ---------------------------------------------------------------------------

fn report(ok: bool, label: &str, all_ok: &mut bool) {
    if !ok {
        crate::serial_print("[UI-TEST] BAŞARISIZ: ");
        *all_ok = false;
        crate::serial_print(label);
        crate::serial_print("\n");
    }
}

/// Widget ağacı, dinamik yerleşim, Tab sırası ve olay yönlendirmesini
/// gerçek bir ağaç üzerinde doğrular. Çizim yolu da çalıştırılır; böylece
/// odak halkası ve tüm denetim çizimleri çekirdekte sınanmış olur.
pub fn self_test(fb: &mut Framebuffer) -> bool {
    let mut ok = true;

    let text = TextBox::new("Ad yazin");
    let text_id = text.id();
    let check = CheckBox::new("Kalici depolama");
    let check_id = check.id();
    let slider = Slider::new("Ses", 50, 0, 100);
    let slider_id = slider.id();
    let button = Button::new("Uygula");
    let button_id = button.id();
    let cancel = Button::new("Iptal");
    let cancel_id = cancel.id();

    let mut row = Container::new(Axis::Horizontal).gap(sp(8));
    row.push(button).push(cancel);

    let mut root = Container::new(Axis::Vertical).pad(Insets::all(sp(8))).gap(sp(6));
    root.push(Label::new("minOs Widget Laboratuvari").color(Color::ACCENT_PINK));
    root.push(text);
    root.push(check);
    root.push(slider);
    root.push(row);

    let mut ui = Ui::new(root);
    ui.layout(Rect::new(40, 60, 360, 300));

    let text_r = ui.widget(text_id).unwrap().rect();
    let check_r = ui.widget(check_id).unwrap().rect();
    let slider_r = ui.widget(slider_id).unwrap().rect();
    let button_r = ui.widget(button_id).unwrap().rect();
    let cancel_r = ui.widget(cancel_id).unwrap().rect();

    report(
        text_r.y >= 60 && text_r.h > 0 && text_r.w == 344
            && check_r.y >= text_r.bottom()
            && slider_r.y >= check_r.bottom()
            && button_r.y >= slider_r.bottom(),
        "Dikey yerlesim sirasi ve icerik boyutlari",
        &mut ok,
    );

    report(
        button_r.w > 0 && cancel_r.w > 0
            && button_r.x + button_r.w <= cancel_r.x
            && cancel_r.right() <= 360,
        "Yatay kapsayici bosluklari (gap) dogru",
        &mut ok,
    );

    let order = ui.focus_order();
    let expected = [text_id, check_id, slider_id, button_id, cancel_id];
    report(
        order.len() == expected.len() && order.iter().zip(expected.iter()).all(|(a, b)| a == b),
        "Tab odak sirasi (metin -> onay -> kaydirici -> dugmeler)",
        &mut ok,
    );

    report(ui.focused() == 0, "Baslangicta odak yok", &mut ok);
    ui.handle_key(Key::Tab);
    ui.handle_key(Key::Tab);
    ui.handle_key(Key::Tab);
    report(ui.focused() == slider_id, "Tab ile odak ilerlemesi", &mut ok);
    ui.handle_key(Key::BackTab);
    report(ui.focused() == check_id, "Shift+Tab ile odak gerilemesi", &mut ok);

    ui.set_focus(text_id);
    for ch in [b'm', b'i'] {
        ui.handle_key(Key::Char(ch));
    }
    ui.handle_key(Key::Backspace);
    report(
        ui.widget_as::<TextBox>(text_id).map_or(false, |t| t.text() == "m"),
        "Metin kutusu yazma ve geri silme",
        &mut ok,
    );
    report(
        ui.handle_key(Key::Enter) == WidgetEvent::Submitted(text_id),
        "Metin kutusunda Enter -> Submitted",
        &mut ok,
    );

    ui.set_focus(check_id);
    let toggled = ui.handle_key(Key::Space);
    report(
        toggled == WidgetEvent::Toggled(check_id, true)
            && ui.widget_as::<CheckBox>(check_id).map_or(false, |c| c.is_checked()),
        "Onay kutusu Space ile degisiyor",
        &mut ok,
    );

    ui.set_focus(slider_id);
    let changed = ui.handle_key(Key::Right);
    let after_key = ui.widget_as::<Slider>(slider_id).map_or(0, |s| s.value());
    report(
        changed == WidgetEvent::Changed(slider_id, 51) && after_key == 51,
        "Kaydirici ok tuslariyla artiyor",
        &mut ok,
    );

    let track_y = slider_r.y + slider_r.h.saturating_sub(sp(11));
    let down = ui.handle_pointer(PointerEvent::new(slider_r.x + 1, track_y, PointerKind::Down));
    let min_value = ui.widget_as::<Slider>(slider_id).map_or(-1, |s| s.value());
    let drag = ui.handle_pointer(PointerEvent::new(
        slider_r.right().saturating_sub(1),
        track_y,
        PointerKind::Move,
    ));
    let max_value = ui.widget_as::<Slider>(slider_id).map_or(-1, |s| s.value());
    ui.handle_pointer(PointerEvent::new(slider_r.right(), track_y, PointerKind::Up));
    report(
        down == WidgetEvent::Changed(slider_id, 0)
            && min_value == 0
            && drag == WidgetEvent::Changed(slider_id, 100)
            && max_value == 100,
        "Kaydirici fare ile suruklenebiliyor (0..100)",
        &mut ok,
    );

    let click = ui.handle_pointer(PointerEvent::new(button_r.x + 2, button_r.y + 2, PointerKind::Down));
    let released = ui.handle_pointer(PointerEvent::new(button_r.x + 2, button_r.y + 2, PointerKind::Up));
    report(
        click == WidgetEvent::Ignored
            && released == WidgetEvent::Clicked(button_id)
            && ui.focused() == button_id,
        "Dugme fare ile tiklanabiliyor ve odak aliyor",
        &mut ok,
    );

    // Dinamik yerleşim: kalan boşluk Fill ağırlıklarına göre paylaşılır.
    // Çizim yolu: ağacı gerçek çerçeve tamponuna çiz (hoş geldin ekranı
    // hemen ardından tüm ekranı yeniden çizer).
    ui.draw(fb);

    let mut weights = Container::new(Axis::Horizontal).gap(0).pad(Insets::all(0));
    let mut left = Container::new(Axis::Horizontal).size(Sizing::Fill(1), Sizing::Content);
    left.push(Button::new("1x"));
    let mut right = Container::new(Axis::Horizontal).size(Sizing::Fill(2), Sizing::Content);
    right.push(Button::new("2x"));
    weights.push(left).push(right);
    weights.relayout(Rect::new(0, 0, 300, 40));
    let left_w = weights.children()[0].rect().w;
    let right_w = weights.children()[1].rect().w;
    report(
        left_w > 0 && right_w > left_w && left_w + right_w == 300,
        "Fill agirlikli paylasim (1:2)",
        &mut ok,
    );

    if !ok {
        crate::serial_print("[UI-TEST] Sonuç: BAŞARISIZ TESTLER VAR\n");
    }
    ok
}
