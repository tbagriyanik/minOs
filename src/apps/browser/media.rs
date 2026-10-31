//! HTML5 <video> and <audio> media playback support (no_std).
//!
//! Provides a MediaPlayer struct with play/pause, currentTime, duration, volume,
//! and src state. Audio playback uses the existing PC Speaker / Intel HDA audio
//! drivers for simple WAV/PCM playback. Video is rendered as a placeholder
//! rectangle with playback controls overlay (full video codec decode is out of
//! scope for this effort level).

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::drivers::audio::AudioDevice;

/// Media player state.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaState {
    Stopped,
    Playing,
    Paused,
    Ended,
    Error,
}

/// Media type.
#[derive(Clone, Debug, PartialEq)]
pub enum MediaType {
    Audio,
    Video,
}

/// A media player for <audio> and <video> elements.
#[derive(Clone, Debug)]
pub struct MediaPlayer {
    pub id: String,
    pub media_type: MediaType,
    pub src: String,
    pub state: MediaState,
    pub current_time: f32,
    pub duration: f32,
    pub volume: f32,
    pub muted: bool,
    pub loop_mode: bool,
    pub controls: bool,
    pub autoplay: bool,
    /// Last update timestamp (monotonic ms) for tracking playback progress.
    pub last_update_ms: u64,
    /// PCM audio data buffer (for WAV/PCM playback).
    pub audio_data: Vec<u8>,
    /// Current position in audio data (bytes).
    pub audio_pos: usize,
    /// Sample rate for audio playback.
    pub sample_rate: u32,
    /// Bytes per sample.
    pub bytes_per_sample: u8,
    /// Number of channels.
    pub channels: u8,
}

impl MediaPlayer {
    pub fn new(id: String, media_type: MediaType) -> Self {
        MediaPlayer {
            id,
            media_type,
            src: String::new(),
            state: MediaState::Stopped,
            current_time: 0.0,
            duration: 0.0,
            volume: 1.0,
            muted: false,
            loop_mode: false,
            controls: false,
            autoplay: false,
            last_update_ms: 0,
            audio_data: Vec::new(),
            audio_pos: 0,
            sample_rate: 44100,
            bytes_per_sample: 2,
            channels: 2,
        }
    }

    /// Load a WAV/PCM audio file from raw bytes.
    pub fn load_wav(&mut self, data: &[u8]) -> bool {
        if data.len() < 44 {
            return false;
        }
        // Check RIFF/WAV header
        if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
            return false;
        }
        // Find fmt chunk
        let mut p = 12;
        while p + 8 <= data.len() {
            let chunk_id = &data[p..p + 4];
            let chunk_size = u32::from_le_bytes([data[p + 4], data[p + 5], data[p + 6], data[p + 7]]) as usize;
            if chunk_id == b"fmt " {
                if p + 8 + chunk_size > data.len() {
                    return false;
                }
                let fmt_offset = p + 8;
                let audio_format = u16::from_le_bytes([data[fmt_offset], data[fmt_offset + 1]]);
                if audio_format != 1 {
                    // Only PCM
                    return false;
                }
                self.channels = u16::from_le_bytes([data[fmt_offset + 2], data[fmt_offset + 3]]) as u8;
                self.sample_rate = u32::from_le_bytes([
                    data[fmt_offset + 4],
                    data[fmt_offset + 5],
                    data[fmt_offset + 6],
                    data[fmt_offset + 7],
                ]);
                self.bytes_per_sample = u16::from_le_bytes([data[fmt_offset + 14], data[fmt_offset + 15]]) as u8;
                p += 8 + chunk_size;
            } else if chunk_id == b"data" {
                if p + 8 + chunk_size > data.len() {
                    return false;
                }
                self.audio_data = data[p + 8..p + 8 + chunk_size].to_vec();
                self.audio_pos = 0;
                // Calculate duration: bytes / (sample_rate * bytes_per_sample * channels)
                let byte_rate = self.sample_rate as usize * self.bytes_per_sample as usize * self.channels as usize;
                if byte_rate > 0 {
                    self.duration = self.audio_data.len() as f32 / byte_rate as f32;
                }
                self.state = MediaState::Stopped;
                self.current_time = 0.0;
                return true;
            } else {
                p += 8 + chunk_size;
            }
        }
        false
    }

    /// Play the media.
    pub fn play(&mut self) {
        if self.state == MediaState::Error {
            return;
        }
        self.state = MediaState::Playing;
        self.last_update_ms = crate::boot::time::main_monotonic_ms() as u64;
    }

    /// Pause the media.
    pub fn pause(&mut self) {
        if self.state == MediaState::Playing {
            self.state = MediaState::Paused;
        }
    }

    /// Stop the media.
    pub fn stop(&mut self) {
        self.state = MediaState::Stopped;
        self.current_time = 0.0;
        self.audio_pos = 0;
    }

    /// Update playback position based on elapsed time.
    /// Should be called each frame.
    pub fn update(&mut self, now_ms: u64) {
        if self.state != MediaState::Playing {
            return;
        }

        let elapsed = now_ms.saturating_sub(self.last_update_ms) as f32 / 1000.0;
        self.last_update_ms = now_ms;

        self.current_time += elapsed;
        self.audio_pos = (self.current_time * self.sample_rate as f32 * self.bytes_per_sample as f32 * self.channels as f32) as usize;

        if self.current_time >= self.duration {
            if self.loop_mode {
                self.current_time = 0.0;
                self.audio_pos = 0;
                self.last_update_ms = now_ms;
            } else {
                self.state = MediaState::Ended;
                self.current_time = self.duration;
            }
        }
    }

    /// Set volume (0.0..=1.0).
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
    }

    /// Seek to a time position.
    pub fn seek(&mut self, time: f32) {
        self.current_time = time.clamp(0.0, self.duration);
        self.audio_pos = (self.current_time * self.sample_rate as f32 * self.bytes_per_sample as f32 * self.channels as f32) as usize;
    }

    /// Get playback progress as a fraction (0.0..=1.0).
    pub fn progress(&self) -> f32 {
        if self.duration > 0.0 {
            (self.current_time / self.duration).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Render audio by pushing PCM samples to the audio device.
    /// Returns true if audio was played.
    pub fn render_audio(&mut self, audio: &mut AudioDevice) -> bool {
        if self.state != MediaState::Playing || self.audio_data.is_empty() {
            return false;
        }

        // Push a small buffer of PCM data to the audio device.
        // This is a simplified implementation - in a full implementation,
        // we'd mix and resample properly.
        let byte_rate = self.sample_rate as usize * self.bytes_per_sample as usize * self.channels as usize;
        let samples_per_frame = byte_rate / 30; // ~30 FPS audio chunks

        let start = self.audio_pos;
        let end = (start + samples_per_frame).min(self.audio_data.len());
        if start < end {
            let _chunk = &self.audio_data[start..end];
            // Apply volume
            let vol = if self.muted { 0.0 } else { self.volume };
            let vol_factor = (vol * 255.0) as u32;

            // Push to HDA if available
            if audio.is_hda {
                // Convert to i16 stereo samples
                let mut pcm = [0i16; 384];
                let mut idx = 0;
                for i in (start..end).step_by(self.bytes_per_sample as usize * self.channels as usize) {
                    if idx + 1 >= pcm.len() {
                        break;
                    }
                    let sample = if i + 1 < self.audio_data.len() {
                        let v = self.audio_data[i] as i16 | ((self.audio_data[i + 1] as i16) << 8);
                        v
                    } else {
                        0
                    };
                    let vol_sample = (sample as i32 * vol_factor as i32 / 255) as i16;
                    pcm[idx] = vol_sample;
                    pcm[idx + 1] = vol_sample;
                    idx += 2;
                }
                if idx > 0 {
                    crate::drivers::hda::push_pcm(&pcm[..idx]);
                }
            } else {
                // PC Speaker fallback: play a tone based on audio position
                let freq = 440u32 + (self.audio_pos as u32 % 800);
                audio.set_pc_speaker_tone(freq);
            }

            self.audio_pos = end;
            if self.audio_pos >= self.audio_data.len() {
                if self.loop_mode {
                    self.audio_pos = 0;
                    self.current_time = 0.0;
                    self.last_update_ms = crate::boot::time::main_monotonic_ms() as u64;
                } else {
                    self.state = MediaState::Ended;
                }
            }
            return true;
        }
        false
    }
}

/// Collection of media players on the current page.
#[derive(Clone, Debug)]
pub struct MediaPlayers {
    pub players: Vec<MediaPlayer>,
}

impl MediaPlayers {
    pub fn new() -> Self {
        MediaPlayers { players: Vec::new() }
    }

    pub fn add(&mut self, player: MediaPlayer) {
        self.players.push(player);
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut MediaPlayer> {
        self.players.iter_mut().find(|p| p.id == id)
    }

    pub fn get(&self, id: &str) -> Option<&MediaPlayer> {
        self.players.iter().find(|p| p.id == id)
    }

    /// Update all players.
    pub fn update_all(&mut self, now_ms: u64) {
        for p in &mut self.players {
            p.update(now_ms);
        }
    }

    /// Render audio for all playing players.
    pub fn render_audio_all(&mut self, audio: &mut AudioDevice) {
        for p in &mut self.players {
            p.render_audio(audio);
        }
    }
}