pub struct AudioDevice {
    pub is_hda: bool,
    pub volume_left: u8,
    pub volume_right: u8,
    pub is_muted: bool,
}

impl AudioDevice {
    pub const fn new() -> Self {
        Self {
            is_hda: true,
            volume_left: 80,
            volume_right: 80,
            is_muted: false,
        }
    }

    pub fn set_volume(&mut self, vol: u8) {
        let clamped = vol.min(100);
        self.volume_left = clamped;
        self.volume_right = clamped;
    }

    pub fn play_sample_beep(&self) {
        // Intel HDA / AC97 PCM buffer playback trigger
    }
}
