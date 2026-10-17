pub struct AudioDevice {
    pub is_hda: bool,
    pub volume_left: u8,
    pub volume_right: u8,
    pub is_muted: bool,
    pub pc_speaker_active: bool,
}

impl AudioDevice {
    pub fn new() -> Self {
        Self {
            is_hda: crate::drivers::hda::is_ready(),
            volume_left: 80,
            volume_right: 80,
            is_muted: false,
            pc_speaker_active: false,
        }
    }

    pub fn set_volume(&mut self, vol: u8) {
        let clamped = vol.min(100);
        self.volume_left = clamped;
        self.volume_right = clamped;
        crate::drivers::hda::set_volume(clamped);
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.is_muted = muted;
        crate::drivers::hda::set_muted(muted);
        if muted {
            self.set_pc_speaker_tone(0);
        }
    }

    /// HDA PCM akışının şu an canlı ve ilerliyor olup olmadığını döner.
    /// Eğer HDA bulunamadıysa VEYA sanallaştırma ortamında DMA takıldıysa (stall),
    /// sistem otomatik olarak PC Speaker yedeğini devreye sokar.
    pub fn is_hda_active(&self) -> bool {
        self.is_hda && !crate::drivers::hda::is_dma_stalled()
    }

    /// HDA ve PC Speaker durum özeti (GUI ve Ayarlar için).
    pub fn status_text(&self) -> &'static str {
        if self.is_muted || self.volume_left == 0 {
            crate::gui::ui_text("Ses kapalı (Sessiz)", "Audio muted")
        } else if self.is_hda && crate::drivers::hda::is_dma_progressing() {
            crate::gui::ui_text("Intel HDA (DMA Aktif, 48kHz Stereo)", "Intel HDA (DMA Active, 48kHz Stereo)")
        } else if self.is_hda && !crate::drivers::hda::is_dma_stalled() {
            crate::gui::ui_text("Intel HDA (Hazır / DMA Beklemede)", "Intel HDA (Ready / DMA Standby)")
        } else {
            crate::gui::ui_text("PC Speaker Yedeği Aktif (PWM)", "PC Speaker Fallback Active (PWM)")
        }
    }

    /// Test amaçlı kısa ses üretir: HDA DMA ilerliyorsa HDA tonu çalar,
    /// aksi halde PC Speaker PWM yedeğini test eder. (hda_ok, speaker_ok) döner.
    pub fn test_audio_subsystem(&mut self) -> (&'static str, bool) {
        if self.is_hda {
            let dma_ok = crate::drivers::hda::play_test_tone();
            if dma_ok {
                return (crate::gui::ui_text("BAŞARILI: Intel HDA DMA İlerliyor (48kHz Stereo)", "PASS: Intel HDA DMA Progressing (48kHz Stereo)"), true);
            }
        }
        // Fallback: PC Speaker testi
        self.set_pc_speaker_tone(880);
        let start = crate::monotonic_ms();
        while crate::monotonic_ms().saturating_sub(start) < 60 {
            core::hint::spin_loop();
        }
        self.set_pc_speaker_tone(0);
        (crate::gui::ui_text("YEDEK DEVREDE: PC Speaker PWM Fallback Çalışıyor", "FALLBACK: PC Speaker PWM Fallback Operating"), false)
    }

    /// PIT kanal 2 / PC speaker üzerinden ton üretir; frekans 0 olursa susturur.
    pub fn set_pc_speaker_tone(&mut self, frequency_hz: u32) {
        unsafe {
            let mut port61: u8;
            core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") port61, options(nomem, nostack, preserves_flags));
            if frequency_hz == 0 || self.is_muted || self.volume_left == 0 {
                port61 &= !0x03;
                core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") port61, options(nomem, nostack, preserves_flags));
                self.pc_speaker_active = false;
                return;
            }
            let divisor = (1_193_182u32 / frequency_hz.max(20)).min(u16::MAX as u32) as u16;
            core::arch::asm!("out dx, al", in("dx") 0x43u16, in("al") 0xB6u8, options(nomem, nostack, preserves_flags));
            core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") divisor as u8, options(nomem, nostack, preserves_flags));
            core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") (divisor >> 8) as u8, options(nomem, nostack, preserves_flags));
            port61 |= 0x03;
            core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") port61, options(nomem, nostack, preserves_flags));
            self.pc_speaker_active = true;
        }
    }

    /// Başlangıç veya bildirim melodisi (chime).
    pub fn play_chime(&mut self) {
        if self.is_muted || self.volume_left == 0 { return; }
        // C5 (523Hz) -> E5 (659Hz) -> G5 (784Hz)
        let notes = [523u32, 659, 784];
        for &freq in &notes {
            self.set_pc_speaker_tone(freq);
            let start = crate::monotonic_ms();
            while crate::monotonic_ms().saturating_sub(start) < 45 {
                core::hint::spin_loop();
            }
        }
        self.set_pc_speaker_tone(0);
    }

    /// Hızlı tıklama veya bildirim sesi (beep).
    pub fn play_beep(&mut self, freq: u32, duration_ms: usize) {
        if self.is_muted || self.volume_left == 0 { return; }
        self.set_pc_speaker_tone(freq);
        let start = crate::monotonic_ms();
        while crate::monotonic_ms().saturating_sub(start) < duration_ms as u64 {
            core::hint::spin_loop();
        }
        self.set_pc_speaker_tone(0);
    }
}
