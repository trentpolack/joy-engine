// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Sound playback with bounded overlapping voices on native outputs and browser Web Audio.
//!
//! An [`AudioBank`] owns one audio output, its decoded samples, and the voices playing them. Samples are decoded once
//! at creation, from WAV files or raw PCM (such as procedurally synthesized sounds); playback never loads or decodes.
//! Native builds mix voices onto the system output through cpal and play immediately. Browsers play through Web Audio,
//! which stays suspended until the page receives a gesture: the bank resumes itself on the next pointer, touch, or key
//! press, games may call [`AudioBank::resume`] from input too, and [`AudioBank::play`] returns false until audio runs.
//! A machine without an audio device gets a silent bank whose `play` returns false.

mod decode;
#[cfg(not(target_arch = "wasm32"))]
mod native;

use std::borrow::Cow;
use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
use crate::browser::audio::WebOutput as Output;
#[cfg(not(target_arch = "wasm32"))]
use native::NativeOutput as Output;

/// Bank-wide voice limit when [`AudioBankOptions`] does not set one.
pub const DEFAULT_MAXIMUM_VOICES: usize = 32;

/// Encoded or raw sample data for one sound.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundSource {
    /// A RIFF WAV file: 8, 16, 24, or 32-bit integer PCM, or 32-bit float.
    Wav(Cow<'static, [u8]>),
    /// Raw samples, such as a procedurally synthesized sound.
    Pcm(PcmSound),
}

/// Raw interleaved samples in `[-1, 1]`.
#[derive(Clone, Debug, PartialEq)]
pub struct PcmSound {
    /// Frames per second; 8,000 to 96,000 so every browser can play it.
    pub sample_rate: u32,
    /// Interleaved channels; the first two play as left and right, and mono plays on both.
    pub channels: u16,
    /// Interleaved samples, `channels` per frame.
    pub samples: Vec<f32>,
}

/// One sound in a bank.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundDefinition {
    /// Sample data, decoded when the bank is created.
    pub source: SoundSource,
    /// Linear gain in `[0, 1]`.
    pub volume: f32,
    /// Voices of this sound that may overlap; defaults to the bank's limit.
    pub maximum_voices: Option<usize>,
}

impl SoundDefinition {
    /// A sound from a WAV file's bytes (for example `include_bytes!`) at full volume.
    pub fn wav(bytes: impl Into<Cow<'static, [u8]>>) -> Self {
        Self { source: SoundSource::Wav(bytes.into()), volume: 1.0, maximum_voices: None }
    }

    /// A sound from raw samples at full volume.
    pub fn pcm(sound: PcmSound) -> Self {
        Self { source: SoundSource::Pcm(sound), volume: 1.0, maximum_voices: None }
    }

    /// Replace the volume.
    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    /// Limit how many voices of this sound may overlap.
    pub fn with_maximum_voices(mut self, maximum_voices: usize) -> Self {
        self.maximum_voices = Some(maximum_voices);
        self
    }
}

/// Bank-wide settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioBankOptions {
    /// Voices of all sounds that may overlap.
    pub maximum_voices: usize,
}

impl Default for AudioBankOptions {
    fn default() -> Self {
        Self { maximum_voices: DEFAULT_MAXIMUM_VOICES }
    }
}

/// A bank could not be created.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AudioError {
    /// A bank or sound voice limit was zero.
    #[error("Voice limits must be at least one.")]
    InvalidVoiceLimit,
    /// A sound's volume was outside `[0, 1]`.
    #[error("Sound volume must be between zero and one (\"{0}\").")]
    InvalidVolume(String),
    /// A sound's data could not be decoded or loaded into the output.
    #[error("Unable to load sound \"{name}\": {reason}")]
    Load {
        /// The sound's name.
        name: String,
        /// Why it failed.
        reason: String,
    },
}

/// Decoded stereo frames at the sound's own rate.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedSound {
    pub(crate) sample_rate: u32,
    pub(crate) frames: Vec<[f32; 2]>,
}

/// A playing voice, oldest first in every backend's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VoiceRecord {
    pub(crate) id: u64,
    pub(crate) sound: usize,
}

/// Voices to retire before starting `sound`: the oldest voice of the same sound at its limit, then the oldest voice of
/// any sound at the bank's limit.
pub(crate) fn voices_to_steal(voices: &[VoiceRecord], sound: usize, sound_limit: usize, bank_limit: usize) -> Vec<u64> {
    let mut stolen = Vec::new();
    if voices.iter().filter(|voice| voice.sound == sound).count() >= sound_limit
        && let Some(oldest) = voices.iter().find(|voice| voice.sound == sound)
    {
        stolen.push(oldest.id);
    }
    if voices.len() - stolen.len() >= bank_limit
        && let Some(oldest) = voices.iter().find(|voice| !stolen.contains(&voice.id))
    {
        stolen.push(oldest.id);
    }
    stolen
}

/// A sound's playback settings and its index in the output's sample list.
struct SoundSlot {
    index: usize,
    volume: f32,
    maximum_voices: usize,
}

/// Owns one audio output, decoded samples, and bounded overlapping voices.
///
/// Create it before gameplay and drop or [`destroy`](Self::destroy) it when the owning session ends. At capacity,
/// starting a sound retires the oldest voice of that sound, then the oldest voice in the bank.
pub struct AudioBank {
    sounds: HashMap<String, SoundSlot>,
    maximum_voices: usize,
    master_volume: f32,
    output: Output,
    destroyed: bool,
}

impl AudioBank {
    /// Validate every definition, decode every sound, and open the output.
    pub fn new<N: Into<String>>(
        definitions: impl IntoIterator<Item = (N, SoundDefinition)>,
        options: AudioBankOptions,
    ) -> Result<Self, AudioError> {
        if options.maximum_voices == 0 {
            return Err(AudioError::InvalidVoiceLimit);
        }
        let definitions: Vec<(String, SoundDefinition)> =
            definitions.into_iter().map(|(name, definition)| (name.into(), definition)).collect();
        for (name, definition) in &definitions {
            if definition.maximum_voices == Some(0) {
                return Err(AudioError::InvalidVoiceLimit);
            }
            if !definition.volume.is_finite() || !(0.0..=1.0).contains(&definition.volume) {
                return Err(AudioError::InvalidVolume(name.clone()));
            }
        }

        let mut decoded = Vec::with_capacity(definitions.len());
        let mut sounds = HashMap::with_capacity(definitions.len());
        for (name, definition) in definitions {
            let sound =
                decode::decode(&definition.source).map_err(|reason| AudioError::Load { name: name.clone(), reason })?;
            let slot = SoundSlot {
                index: decoded.len(),
                volume: definition.volume,
                maximum_voices: definition.maximum_voices.unwrap_or(options.maximum_voices),
            };
            decoded.push((name.clone(), sound));
            sounds.insert(name, slot);
        }
        let output = Output::new(&decoded).map_err(|(index, reason)| AudioError::Load {
            name: decoded.get(index).map_or_else(String::new, |(name, _)| name.clone()),
            reason,
        })?;
        Ok(Self { sounds, maximum_voices: options.maximum_voices, master_volume: 1.0, output, destroyed: false })
    }

    /// Ask the output to run; call it from input handling after a gesture. Returns whether audio runs now (browsers
    /// finish resuming asynchronously, so the first call may report false) and false after disposal.
    pub fn resume(&mut self) -> bool {
        !self.destroyed && self.output.resume()
    }

    /// Whether the output is running, so [`play`](Self::play) can start sounds.
    pub fn running(&self) -> bool {
        !self.destroyed && self.output.running()
    }

    /// Start a sound immediately. Returns false when the name is unknown, audio is suspended or unavailable, or the
    /// bank is destroyed; sounds are never queued.
    pub fn play(&mut self, name: &str) -> bool {
        let Some(sound) = self.sounds.get(name) else {
            return false;
        };
        if self.destroyed || !self.output.running() {
            return false;
        }
        self.output.play(sound.index, sound.volume * self.master_volume, sound.maximum_voices, self.maximum_voices)
    }

    /// Scale every sound started from now on by `volume`, clamped to `[0, 1]`, like a game's master volume setting.
    /// Voices already playing keep their level (sound effects are short). Non-finite values are ignored.
    pub fn set_master_volume(&mut self, volume: f32) {
        if volume.is_finite() {
            self.master_volume = volume.clamp(0.0, 1.0);
        }
    }

    /// The master volume applied to sounds as they start; one by default.
    pub fn master_volume(&self) -> f32 {
        self.master_volume
    }

    /// Whether the bank defines a sound.
    pub fn contains(&self, name: &str) -> bool {
        self.sounds.contains_key(name)
    }

    /// Voices currently playing.
    pub fn active_voice_count(&mut self) -> usize {
        if self.destroyed { 0 } else { self.output.active_voice_count() }
    }

    /// Stop every voice, discard samples, and close the output. Idempotent; dropping the bank does the same.
    pub fn destroy(&mut self) {
        if std::mem::replace(&mut self.destroyed, true) {
            return;
        }
        self.output.close();
        self.sounds.clear();
    }
}

impl Drop for AudioBank {
    fn drop(&mut self) {
        self.destroy();
    }
}

impl std::fmt::Debug for AudioBank {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioBank")
            .field("sounds", &self.sounds.len())
            .field("maximum_voices", &self.maximum_voices)
            .field("master_volume", &self.master_volume)
            .field("destroyed", &self.destroyed)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voices(sounds: &[usize]) -> Vec<VoiceRecord> {
        sounds.iter().enumerate().map(|(id, sound)| VoiceRecord { id: id as u64, sound: *sound }).collect()
    }

    #[test]
    fn stealing_retires_the_oldest_matching_voice_then_the_oldest_voice() {
        // Under both limits nothing is stolen.
        assert!(voices_to_steal(&voices(&[0, 1]), 0, 2, 4).is_empty());
        // At the sound's limit, its oldest voice goes.
        assert_eq!(voices_to_steal(&voices(&[1, 0, 0]), 0, 2, 8), [1]);
        // At the bank's limit, the oldest voice of any sound goes.
        assert_eq!(voices_to_steal(&voices(&[1, 0, 2]), 3, 4, 3), [0]);
        // Both: the matching voice frees one slot, so the bank limit is checked after it.
        assert_eq!(voices_to_steal(&voices(&[1, 0, 0]), 0, 2, 3), [1]);
        assert_eq!(voices_to_steal(&voices(&[1, 0, 0, 2]), 0, 2, 3), [1, 0]);
    }

    #[test]
    fn invalid_definitions_are_reported_before_any_output_opens() {
        let silence = || SoundDefinition::pcm(PcmSound { sample_rate: 8_000, channels: 1, samples: vec![0.0; 8] });
        let error = AudioBank::new([("blip", silence())], AudioBankOptions { maximum_voices: 0 }).unwrap_err();
        assert_eq!(error, AudioError::InvalidVoiceLimit);
        let error = AudioBank::new([("blip", silence().with_maximum_voices(0))], AudioBankOptions::default());
        assert_eq!(error.unwrap_err(), AudioError::InvalidVoiceLimit);
        let error = AudioBank::new([("blip", silence().with_volume(1.5))], AudioBankOptions::default());
        assert_eq!(error.unwrap_err(), AudioError::InvalidVolume("blip".to_owned()));
        let broken = SoundDefinition::wav(&b"not a wav"[..]);
        let error = AudioBank::new([("broken", broken)], AudioBankOptions::default()).unwrap_err();
        assert!(matches!(error, AudioError::Load { ref name, .. } if name == "broken"), "{error}");
    }

    #[test]
    fn master_volume_clamps_and_ignores_non_finite_values() {
        let tone = PcmSound { sample_rate: 22_050, channels: 1, samples: vec![0.25; 2_205] };
        let mut bank = AudioBank::new([("tone", SoundDefinition::pcm(tone))], AudioBankOptions::default()).unwrap();
        assert_eq!(bank.master_volume(), 1.0);
        bank.set_master_volume(0.4);
        assert_eq!(bank.master_volume(), 0.4);
        bank.set_master_volume(f32::NAN);
        assert_eq!(bank.master_volume(), 0.4);
        bank.set_master_volume(3.0);
        assert_eq!(bank.master_volume(), 1.0);
        bank.set_master_volume(-1.0);
        assert_eq!(bank.master_volume(), 0.0);
    }

    #[test]
    fn unknown_or_destroyed_sounds_do_not_play() {
        let tone = PcmSound { sample_rate: 22_050, channels: 1, samples: vec![0.25; 2_205] };
        let mut bank = AudioBank::new([("tone", SoundDefinition::pcm(tone))], AudioBankOptions::default()).unwrap();
        assert!(bank.contains("tone"));
        assert!(!bank.play("missing"));
        bank.destroy();
        bank.destroy();
        assert!(!bank.play("tone") && !bank.resume() && !bank.running());
        assert_eq!(bank.active_voice_count(), 0);
    }
}
