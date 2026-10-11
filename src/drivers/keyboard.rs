#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KeyAction {
    ToggleStartMenu,
    CloseActiveWindow,
    CycleWindowFocus,
    OpenTaskManager,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    EnterKey,
    ShutdownOS,
    CopyText,
    PasteText,
    CutText,
    SelectAllText,
    Delete,
    Refresh,
    BrowserAddress,
    BrowserRefresh,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KeyboardLayout {
    TurkishQ,
    EnglishUS,
}

pub struct KeyboardState {
    pub layout: KeyboardLayout,
    pub shift: bool,
    pub caps_lock: bool,
    pub alt_gr: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
    pub last_char: Option<char>,
    pub last_action: Option<KeyAction>,
}

impl KeyboardState {
    pub const fn new() -> Self {
        Self {
            layout: KeyboardLayout::TurkishQ,
            shift: false,
            caps_lock: false,
            alt_gr: false,
            ctrl: false,
            alt: false,
            super_key: false,
            last_char: None,
            last_action: None,
        }
    }

    pub fn toggle_layout(&mut self) {
        self.layout = match self.layout {
            KeyboardLayout::TurkishQ => KeyboardLayout::EnglishUS,
            KeyboardLayout::EnglishUS => KeyboardLayout::TurkishQ,
        };
    }

    pub fn layout_name(&self) -> &'static str {
        match self.layout {
            KeyboardLayout::TurkishQ => "TR-Q",
            KeyboardLayout::EnglishUS => "EN-US",
        }
    }

    pub fn layout_tag(&self) -> &'static str {
        match self.layout {
            KeyboardLayout::TurkishQ => "TR",
            KeyboardLayout::EnglishUS => "EN",
        }
    }

    pub fn take_last_char(&mut self) -> Option<char> {
        self.last_char.take()
    }

    pub fn take_last_action(&mut self) -> Option<KeyAction> {
        self.last_action.take()
    }

    pub fn handle_scancode(&mut self, scancode: u8) -> Option<char> {
        // Tuş bırakma (Release / Break code) kontrolü
        if scancode & 0x80 != 0 {
            let released = scancode & 0x7F;
            if released == 0x2A || released == 0x36 {
                self.shift = false;
            }
            if released == 0x1D {
                self.ctrl = false;
            }
            if released == 0x38 {
                self.alt = false;
                self.alt_gr = false;
            }
            if released == 0x5B || released == 0x5C {
                self.super_key = false;
            }
            return None;
        }

        // Tuşa basılma (Make code)
        match scancode {
            0x2A | 0x36 => {
                self.shift = true;
                return None;
            }
            0x3A => {
                self.caps_lock = !self.caps_lock;
                return None;
            }
            0x1D => {
                self.ctrl = true;
                return None;
            }
            0x38 => {
                self.alt = true;
                self.alt_gr = true;
                return None;
            }
            0x5B | 0x5C => { // Left / Right Super (Windows) key
                self.super_key = true;
                self.last_action = Some(KeyAction::ToggleStartMenu);
                return None;
            }
            0x3E => { // F4 key
                if self.alt {
                    self.last_action = Some(KeyAction::CloseActiveWindow);
                    return None;
                }
            }
            0x0F => { // Tab key
                if self.alt {
                    self.last_action = Some(KeyAction::CycleWindowFocus);
                    return None;
                }
            }
            0x53 => { // Delete key
                if self.ctrl && self.alt {
                    self.last_action = Some(KeyAction::OpenTaskManager);
                    return None;
                }
            }
            0x2E if self.ctrl => { self.last_action = Some(KeyAction::CopyText); return None; } // Ctrl+C
            0x2F if self.ctrl => { self.last_action = Some(KeyAction::PasteText); return None; } // Ctrl+V
            0x2D if self.ctrl => { self.last_action = Some(KeyAction::CutText); return None; } // Ctrl+X
            0x1E if self.ctrl => { self.last_action = Some(KeyAction::SelectAllText); return None; } // Ctrl+A
            0x26 if self.ctrl => { self.last_action = Some(KeyAction::BrowserAddress); return None; } // Ctrl+L
            0x13 if self.ctrl => { self.last_action = Some(KeyAction::BrowserRefresh); return None; } // Ctrl+R
            0x48 => { // Arrow Up
                self.last_action = Some(KeyAction::ArrowUp);
                return None;
            }
            0x50 => { // Arrow Down
                self.last_action = Some(KeyAction::ArrowDown);
                return None;
            }
            0x4B => { // Arrow Left
                self.last_action = Some(KeyAction::ArrowLeft);
                return None;
            }
            0x4D => { // Arrow Right
                self.last_action = Some(KeyAction::ArrowRight);
                return None;
            }
            0x1C => { // Enter key
                self.last_action = Some(KeyAction::EnterKey);
            }
            _ => {}
        }

        // Layout'a göre harf/karakter çevirisi
        let uppercase = self.shift ^ self.caps_lock;
        let ch = match self.layout {
            KeyboardLayout::TurkishQ => Self::scancode_to_tr(scancode, uppercase, self.shift, self.alt_gr),
            KeyboardLayout::EnglishUS => Self::scancode_to_us(scancode, uppercase, self.shift),
        };

        if ch.is_some() {
            self.last_char = ch;
        }
        ch
    }

    fn scancode_to_tr(scancode: u8, uppercase: bool, shift: bool, _alt_gr: bool) -> Option<char> {
        match scancode {
            0x01 => Some('\x1b'), // Escape
            0x02 => Some(if shift { '\'' } else { '1' }),
            0x03 => Some(if shift { '"' } else { '2' }),
            0x04 => Some(if shift { '^' } else { '3' }),
            0x05 => Some(if shift { '+' } else { '4' }),
            0x06 => Some(if shift { '%' } else { '5' }),
            0x07 => Some(if shift { '&' } else { '6' }),
            0x08 => Some(if shift { '/' } else { '7' }),
            0x09 => Some(if shift { '(' } else { '8' }),
            0x0A => Some(if shift { ')' } else { '9' }),
            0x0B => Some(if shift { '=' } else { '0' }),
            0x0C => Some(if shift { '?' } else { '*' }),
            0x0D => Some(if shift { '_' } else { '-' }),
            0x0E => Some('\x08'), // Backspace
            0x0F => Some('\t'),   // Tab
            0x10 => Some(if uppercase { 'Q' } else { 'q' }),
            0x11 => Some(if uppercase { 'W' } else { 'w' }),
            0x12 => Some(if uppercase { 'E' } else { 'e' }),
            0x13 => Some(if uppercase { 'R' } else { 'r' }),
            0x14 => Some(if uppercase { 'T' } else { 't' }),
            0x15 => Some(if uppercase { 'Y' } else { 'y' }),
            0x16 => Some(if uppercase { 'U' } else { 'u' }),
            0x17 => Some(if uppercase { 'I' } else { 'ı' }),
            0x18 => Some(if uppercase { 'O' } else { 'o' }),
            0x19 => Some(if uppercase { 'P' } else { 'p' }),
            0x1A => Some(if uppercase { 'Ğ' } else { 'ğ' }),
            0x1B => Some(if uppercase { 'Ü' } else { 'ü' }),
            0x1C => Some('\n'), // Enter
            0x1E => Some(if uppercase { 'A' } else { 'a' }),
            0x1F => Some(if uppercase { 'S' } else { 's' }),
            0x20 => Some(if uppercase { 'D' } else { 'd' }),
            0x21 => Some(if uppercase { 'F' } else { 'f' }),
            0x22 => Some(if uppercase { 'G' } else { 'g' }),
            0x23 => Some(if uppercase { 'H' } else { 'h' }),
            0x24 => Some(if uppercase { 'J' } else { 'j' }),
            0x25 => Some(if uppercase { 'K' } else { 'k' }),
            0x26 => Some(if uppercase { 'L' } else { 'l' }),
            0x27 => Some(if uppercase { 'Ş' } else { 'ş' }),
            0x28 => Some(if uppercase { 'İ' } else { 'i' }),
            0x29 => Some(if shift { 'E' } else { 'e' }),
            0x2B => Some(if shift { '~' } else { ',' }),
            0x2C => Some(if uppercase { 'Z' } else { 'z' }),
            0x2D => Some(if uppercase { 'X' } else { 'x' }),
            0x2E => Some(if uppercase { 'C' } else { 'c' }),
            0x2F => Some(if uppercase { 'V' } else { 'v' }),
            0x30 => Some(if uppercase { 'B' } else { 'b' }),
            0x31 => Some(if uppercase { 'N' } else { 'n' }),
            0x32 => Some(if uppercase { 'M' } else { 'm' }),
            0x33 => Some(if uppercase { 'Ö' } else { 'ö' }),
            0x34 => Some(if uppercase { 'Ç' } else { 'ç' }),
            0x35 => Some(if shift { ':' } else { '.' }),
            0x39 => Some(' '), // Space
            _ => None,
        }
    }

    fn scancode_to_us(scancode: u8, uppercase: bool, shift: bool) -> Option<char> {
        match scancode {
            0x01 => Some('\x1b'),
            0x02 => Some(if shift { '!' } else { '1' }),
            0x03 => Some(if shift { '@' } else { '2' }),
            0x04 => Some(if shift { '#' } else { '3' }),
            0x05 => Some(if shift { '$' } else { '4' }),
            0x06 => Some(if shift { '%' } else { '5' }),
            0x07 => Some(if shift { '^' } else { '6' }),
            0x08 => Some(if shift { '&' } else { '7' }),
            0x09 => Some(if shift { '*' } else { '8' }),
            0x0A => Some(if shift { '(' } else { '9' }),
            0x0B => Some(if shift { ')' } else { '0' }),
            0x0C => Some(if shift { '_' } else { '-' }),
            0x0D => Some(if shift { '+' } else { '=' }),
            0x0E => Some('\x08'),
            0x0F => Some('\t'),
            0x10 => Some(if uppercase { 'Q' } else { 'q' }),
            0x11 => Some(if uppercase { 'W' } else { 'w' }),
            0x12 => Some(if uppercase { 'E' } else { 'e' }),
            0x13 => Some(if uppercase { 'R' } else { 'r' }),
            0x14 => Some(if uppercase { 'T' } else { 't' }),
            0x15 => Some(if uppercase { 'Y' } else { 'y' }),
            0x16 => Some(if uppercase { 'U' } else { 'u' }),
            0x17 => Some(if uppercase { 'I' } else { 'i' }),
            0x18 => Some(if uppercase { 'O' } else { 'o' }),
            0x19 => Some(if uppercase { 'P' } else { 'p' }),
            0x1A => Some(if shift { '{' } else { '[' }),
            0x1B => Some(if shift { '}' } else { ']' }),
            0x1C => Some('\n'),
            0x1E => Some(if uppercase { 'A' } else { 'a' }),
            0x1F => Some(if uppercase { 'S' } else { 's' }),
            0x20 => Some(if uppercase { 'D' } else { 'd' }),
            0x21 => Some(if uppercase { 'F' } else { 'f' }),
            0x22 => Some(if uppercase { 'G' } else { 'g' }),
            0x23 => Some(if uppercase { 'H' } else { 'h' }),
            0x24 => Some(if uppercase { 'J' } else { 'j' }),
            0x25 => Some(if uppercase { 'K' } else { 'k' }),
            0x26 => Some(if uppercase { 'L' } else { 'l' }),
            0x27 => Some(if shift { ':' } else { ';' }),
            0x28 => Some(if shift { '"' } else { '\'' }),
            0x29 => Some(if shift { '~' } else { '`' }),
            0x2B => Some(if shift { '|' } else { '\\' }),
            0x2C => Some(if uppercase { 'Z' } else { 'z' }),
            0x2D => Some(if uppercase { 'X' } else { 'x' }),
            0x2E => Some(if uppercase { 'C' } else { 'c' }),
            0x2F => Some(if uppercase { 'V' } else { 'v' }),
            0x30 => Some(if uppercase { 'B' } else { 'b' }),
            0x31 => Some(if uppercase { 'N' } else { 'n' }),
            0x32 => Some(if uppercase { 'M' } else { 'm' }),
            0x33 => Some(if shift { '<' } else { ',' }),
            0x34 => Some(if shift { '>' } else { '.' }),
            0x35 => Some(if shift { '?' } else { '/' }),
            0x39 => Some(' '),
            _ => None,
        }
    }
}
