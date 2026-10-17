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
    HomeKey,
    EndKey,
    PageUpKey,
    PageDownKey,
    Undo,
    Redo,
    WinShowDesktop,
    WinMinimizeAll,
    WinOpenFiles,
    VolumeUp,
    VolumeDown,
    VolumeMute,
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    RenameSelected,
    ToggleOverwrite,
    DeleteWordBack,
    DeleteWordForward,
    SaveDocument,
    NewDocument,
    OpenDocument,
    Screenshot,
    RunDialog,
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
    pub num_lock: bool,
    pub alt_gr: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
    pub super_key_combo: bool,
    pub last_char: Option<char>,
    action_queue: [Option<KeyAction>; 8],
    action_head: usize,
    action_tail: usize,
    extended_scancode: bool,
    prt_scn_state: u8,
}

impl KeyboardState {
    pub const fn new() -> Self {
        Self {
            layout: KeyboardLayout::TurkishQ,
            shift: false,
            caps_lock: false,
            num_lock: true,
            alt_gr: false,
            ctrl: false,
            alt: false,
            super_key: false,
            super_key_combo: false,
            last_char: None,
            action_queue: [None; 8],
            action_head: 0,
            action_tail: 0,
            extended_scancode: false,
            prt_scn_state: 0,
        }
    }

    pub fn push_action(&mut self, action: KeyAction) {
        let next_head = (self.action_head + 1) % self.action_queue.len();
        if next_head != self.action_tail {
            self.action_queue[self.action_head] = Some(action);
            self.action_head = next_head;
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
        if self.action_tail != self.action_head {
            let act = self.action_queue[self.action_tail].take();
            self.action_tail = (self.action_tail + 1) % self.action_queue.len();
            act
        } else {
            None
        }
    }

    pub fn handle_scancode(&mut self, scancode: u8) -> Option<char> {
        if scancode == 0xE0 {
            self.extended_scancode = true;
            return None;
        }

        let extended = core::mem::replace(&mut self.extended_scancode, false);
        if extended {
            // Standart PS/2 PrintScreen Make Sequence: E0 2A E0 37
            if scancode == 0x2A {
                self.prt_scn_state = 1;
                return None;
            } else if scancode == 0x37 {
                self.prt_scn_state = 0;
                self.push_action(KeyAction::Screenshot);
                return None;
            } else if scancode == 0xB7 || scancode == 0xAA {
                // Break sequence: E0 B7 E0 AA
                return None;
            }
            self.prt_scn_state = 0;
            return self.handle_extended_scancode(scancode);
        }

        // PrintScreen ikinci yarısı veya doğrudan SysRq (Alt+PrtScn = 0x54)
        if self.prt_scn_state == 1 && scancode == 0x37 {
            self.prt_scn_state = 0;
            self.push_action(KeyAction::Screenshot);
            return None;
        }
        self.prt_scn_state = 0;

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
                if !self.super_key_combo {
                    self.push_action(KeyAction::ToggleStartMenu);
                }
                self.super_key_combo = false;
                return None;
            }
            return None;
        }

        if scancode == 0x54 { // Alt+PrintScreen (SysRq make code)
            self.push_action(KeyAction::Screenshot);
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
            0x45 => {
                self.num_lock = !self.num_lock;
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
                self.super_key_combo = false;
                return None;
            }
            0x20 if self.super_key => { self.super_key_combo = true; self.push_action(KeyAction::WinShowDesktop); return None; } // Win+D
            0x32 if self.super_key => { self.super_key_combo = true; self.push_action(KeyAction::WinMinimizeAll); return None; } // Win+M
            0x12 if self.super_key => { self.super_key_combo = true; self.push_action(KeyAction::WinOpenFiles); return None; } // Win+E
            0x13 if self.super_key => { self.super_key_combo = true; self.push_action(KeyAction::RunDialog); return None; } // Win+R
            0x58 => { self.push_action(KeyAction::Screenshot); return None; } // F12 (Screenshot)
            0x3E => { // F4 key
                if self.alt {
                    self.push_action(KeyAction::CloseActiveWindow);
                    return None;
                }
            }
            0x3C => { self.push_action(KeyAction::RenameSelected); return None; } // F2
            0x0F => { // Tab key
                if self.alt {
                    self.push_action(KeyAction::CycleWindowFocus);
                    return None;
                }
            }
            0x0E if self.ctrl => { // Ctrl+Backspace
                self.push_action(KeyAction::DeleteWordBack);
                return None;
            }
            0x52 if !self.num_lock => { // Insert key / keypad 0
                if self.ctrl {
                    self.push_action(KeyAction::CopyText);
                    return None;
                }
                if self.shift {
                    self.push_action(KeyAction::PasteText);
                    return None;
                }
                self.push_action(KeyAction::ToggleOverwrite);
                return None;
            }
            0x53 if !self.num_lock => { // Delete key / keypad decimal
                if self.ctrl && self.alt {
                    self.push_action(KeyAction::OpenTaskManager);
                    return None;
                }
                if self.ctrl {
                    self.push_action(KeyAction::DeleteWordForward);
                    return None;
                }
                if self.shift {
                    self.push_action(KeyAction::CutText);
                    return None;
                }
                self.push_action(KeyAction::Delete);
                return None;
            }
            0x47 if !self.num_lock => { self.push_action(KeyAction::HomeKey); return None; } // Home
            0x4F if !self.num_lock => { self.push_action(KeyAction::EndKey); return None; }  // End
            0x49 if !self.num_lock => { self.push_action(KeyAction::PageUpKey); return None; }   // Page Up
            0x51 if !self.num_lock => { self.push_action(KeyAction::PageDownKey); return None; } // Page Down
            0x2E if self.ctrl => { self.push_action(KeyAction::CopyText); return None; } // Ctrl+C
            0x2F if self.ctrl => { self.push_action(KeyAction::PasteText); return None; } // Ctrl+V
            0x2D if self.ctrl => { self.push_action(KeyAction::CutText); return None; } // Ctrl+X
            0x1E if self.ctrl => { self.push_action(KeyAction::SelectAllText); return None; } // Ctrl+A
            0x2C if self.ctrl => { self.push_action(KeyAction::Undo); return None; } // Ctrl+Z
            0x15 if self.ctrl => { self.push_action(KeyAction::Redo); return None; } // Ctrl+Y
            0x1F if self.ctrl => { self.push_action(KeyAction::SaveDocument); return None; } // Ctrl+S
            0x31 if self.ctrl => { self.push_action(KeyAction::NewDocument); return None; } // Ctrl+N
            0x18 if self.ctrl => { self.push_action(KeyAction::OpenDocument); return None; } // Ctrl+O
            0x26 if self.ctrl => { self.push_action(KeyAction::BrowserAddress); return None; } // Ctrl+L
            0x13 if self.ctrl => { self.push_action(KeyAction::BrowserRefresh); return None; } // Ctrl+R
            0x48 if !self.num_lock => { // Arrow Up / keypad 8
                self.push_action(KeyAction::ArrowUp);
                return None;
            }
            0x50 if !self.num_lock => { // Arrow Down / keypad 2
                self.push_action(KeyAction::ArrowDown);
                return None;
            }
            0x4B if !self.num_lock => { // Arrow Left / keypad 4
                self.push_action(KeyAction::ArrowLeft);
                return None;
            }
            0x4D if !self.num_lock => { // Arrow Right / keypad 6
                self.push_action(KeyAction::ArrowRight);
                return None;
            }
            0x1C => { // Enter key
                self.push_action(KeyAction::EnterKey);
            }
            _ => {}
        }

        // Layout'a göre harf/karakter çevirisi
        let uppercase = self.shift ^ self.caps_lock;
        if self.num_lock {
            let keypad_char = match scancode {
                0x47 => Some('7'), 0x48 => Some('8'), 0x49 => Some('9'),
                0x4B => Some('4'), 0x4C => Some('5'), 0x4D => Some('6'),
                0x4F => Some('1'), 0x50 => Some('2'), 0x51 => Some('3'),
                0x52 => Some('0'), 0x53 => Some('.'),
                0x37 => Some('*'), 0x4A => Some('-'), 0x4E => Some('+'),
                _ => None,
            };
            if let Some(ch) = keypad_char {
                self.last_char = Some(ch);
                return Some(ch);
            }
        }
        let ch = match self.layout {
            KeyboardLayout::TurkishQ => Self::scancode_to_tr(scancode, uppercase, self.shift, self.alt_gr),
            KeyboardLayout::EnglishUS => Self::scancode_to_us(scancode, uppercase, self.shift),
        };

        if ch.is_some() {
            self.last_char = ch;
        }
        ch
    }

    fn handle_extended_scancode(&mut self, scancode: u8) -> Option<char> {
        if scancode & 0x80 != 0 {
            let released = scancode & 0x7F;
            if released == 0x1D { self.ctrl = false; }
            if released == 0x38 { self.alt = false; self.alt_gr = false; }
            if released == 0x5B || released == 0x5C {
                self.super_key = false;
                if !self.super_key_combo {
                    self.push_action(KeyAction::ToggleStartMenu);
                }
                self.super_key_combo = false;
            }
            return None;
        }

        match scancode {
            0x1D => { self.ctrl = true; return None; }
            0x38 => { self.alt = true; self.alt_gr = true; return None; }
            0x5B | 0x5C => {
                self.super_key = true;
                self.super_key_combo = false;
                // Do not toggle start menu here; will handle on key release depending on combo flag.
                return None;
            }
            0x53 if self.ctrl && self.alt => {
                self.push_action(KeyAction::OpenTaskManager);
                return None;
            }
            // Yaygın PS/2 multimedia tuşları (E0 öneki ile gelir).
            0x37 => { self.push_action(KeyAction::Screenshot); return None; } // PrtScn
            0x20 => { self.push_action(KeyAction::VolumeMute); return None; }
            0x2E => { self.push_action(KeyAction::VolumeDown); return None; }
            0x30 => { self.push_action(KeyAction::VolumeUp); return None; }
            0x22 => { self.push_action(KeyAction::MediaPlayPause); return None; }
            0x10 => { self.push_action(KeyAction::MediaPrevious); return None; }
            0x19 => { self.push_action(KeyAction::MediaNext); return None; }
            // Insert tuşu:
            0x52 if self.ctrl => { self.push_action(KeyAction::CopyText); return None; }
            0x52 if self.shift => { self.push_action(KeyAction::PasteText); return None; }
            0x52 => { self.push_action(KeyAction::ToggleOverwrite); return None; }
            // Delete tuşu:
            0x53 if self.ctrl => { self.push_action(KeyAction::DeleteWordForward); return None; }
            0x53 if self.shift => { self.push_action(KeyAction::CutText); return None; }
            0x53 => { self.push_action(KeyAction::Delete); return None; }
            // Navigasyon tuşları (E0 ile gelenler NumLock'tan bağımsızdır):
            0x48 => { self.push_action(KeyAction::ArrowUp); return None; }
            0x50 => { self.push_action(KeyAction::ArrowDown); return None; }
            0x4B => { self.push_action(KeyAction::ArrowLeft); return None; }
            0x4D => { self.push_action(KeyAction::ArrowRight); return None; }
            0x47 => { self.push_action(KeyAction::HomeKey); return None; }
            0x4F => { self.push_action(KeyAction::EndKey); return None; }
            0x49 => { self.push_action(KeyAction::PageUpKey); return None; }
            0x51 => { self.push_action(KeyAction::PageDownKey); return None; }
            0x1C => { self.push_action(KeyAction::EnterKey); return Some('\n'); }
            0x35 => { self.last_char = Some('/'); return Some('/'); } // Keypad /
            _ => {}
        }
        None
    }

    fn scancode_to_tr(scancode: u8, uppercase: bool, shift: bool, alt_gr: bool) -> Option<char> {
        // Türkçe Q düzeninin AltGr karakterleri.
        if alt_gr {
            return match scancode {
                0x02 => Some('¹'),
                0x03 => Some('²'),
                0x04 => Some('#'),
                0x05 => Some('$'),
                0x06 => Some('€'),
                0x07 => Some('{'),
                0x08 => Some('['),
                0x09 => Some(']'),
                0x0A => Some('}'),
                0x0B => Some('\\'),
                0x0C => Some('*'),
                0x0D => Some('|'),
                0x10 => Some('@'), // AltGr+Q
                0x12 => Some('€'), // AltGr+E
                0x14 => Some('₺'), // AltGr+T
                0x33 => Some(';'),
                0x34 => Some(':'),
                0x35 => Some('.'),
                0x39 => Some(' '),
                _ => None,
            };
        }
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
