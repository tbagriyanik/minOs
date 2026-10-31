//! ISA/PCI legacy audio device identification and safe mixer initialization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundCard {
    Ac97,
    SoundBlaster16,
}

pub fn identify(class: u8, subclass: u8, prog_if: u8) -> Option<SoundCard> {
    if class != 0x04 {
        return None;
    }
    match (subclass, prog_if) {
        (0x01, 0x00 | 0x01) => Some(SoundCard::Ac97),
        (0x01, 0x02) => Some(SoundCard::SoundBlaster16),
        _ => None,
    }
}

pub fn name(card: SoundCard) -> &'static str {
    match card {
        SoundCard::Ac97 => "AC97",
        SoundCard::SoundBlaster16 => "Sound Blaster 16",
    }
}
