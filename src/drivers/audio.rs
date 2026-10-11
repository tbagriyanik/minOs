pub struct AudioDevice {
    pub is_hda: bool,
    pub volume_left: u8,
    pub volume_right: u8,
    pub is_muted: bool,
}

impl AudioDevice {
    pub fn new() -> Self {
        Self {
            is_hda: crate::drivers::hda::is_ready(),
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
        if self.is_muted || self.volume_left == 0 {
            return;
        }
        if crate::drivers::hda::is_ready() {
            // 440 Hz (A4) sinüs benzeri ses örneğini HDA PCM çıkışına gönder
            let mut samples = [0i16; 240];
            let vol = (self.volume_left as i32 * 200).min(i16::MAX as i32) as i16;
            for i in 0..samples.len() {
                samples[i] = if (i / 20) % 2 == 0 { vol } else { -vol };
            }
            crate::drivers::hda::push_pcm(&samples);
        } else {
            // HDA yoksa PIT PC hoparlöründen ses çal
            self.set_pc_speaker_tone(440);
        }
    }

    /// PIT kanal 2 / PC speaker üzerinden ton üretir; frekans 0 olursa susturur.
    /// Bu, PCM/AC97 sürücüsü bulunmayan QEMU profilleri için basit bir çıkıştır.
    pub fn set_pc_speaker_tone(&self, frequency_hz: u32) {
        unsafe {
            let mut port61: u8;
            core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") port61, options(nomem, nostack, preserves_flags));
            if frequency_hz == 0 || self.is_muted || self.volume_left == 0 {
                port61 &= !0x03;
                core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") port61, options(nomem, nostack, preserves_flags));
                return;
            }
            let divisor = (1_193_182u32 / frequency_hz.max(20)).min(u16::MAX as u32) as u16;
            core::arch::asm!("out dx, al", in("dx") 0x43u16, in("al") 0xB6u8, options(nomem, nostack, preserves_flags));
            core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") divisor as u8, options(nomem, nostack, preserves_flags));
            core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") (divisor >> 8) as u8, options(nomem, nostack, preserves_flags));
            port61 |= 0x03;
            core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") port61, options(nomem, nostack, preserves_flags));
        }
    }
}
