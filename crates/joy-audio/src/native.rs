// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Native audio output: a cpal stream on the default device mixing voices with linear resampling.

use std::sync::{
    Arc,
    Mutex,
    MutexGuard,
    PoisonError,
};

use cpal::traits::{
    DeviceTrait,
    HostTrait,
    StreamTrait,
};
use cpal::{
    FromSample,
    SampleFormat,
    SizedSample,
    StreamConfig,
};

use crate::{
    DecodedSound,
    VoiceRecord,
    voices_to_steal,
};

/// One playing voice in the mixer.
struct MixVoice {
    id: u64,
    sound: usize,
    /// Position in the sound's frames, fractional between them.
    position: f64,
    gain: f32,
}

/// Samples and voices shared between the bank and the audio thread.
struct Mixer {
    sounds: Vec<DecodedSound>,
    /// Oldest first.
    voices: Vec<MixVoice>,
    output_rate: f64,
    next_id: u64,
}

impl Mixer {
    /// Add every voice into `frames` (zeroed by the caller), retire voices that finish, and clamp the mix.
    fn mix(&mut self, frames: &mut [[f32; 2]]) {
        let sounds = &self.sounds;
        let output_rate = self.output_rate;
        self.voices.retain_mut(|voice| {
            let sound = &sounds[voice.sound];
            let step = f64::from(sound.sample_rate) / output_rate;
            for frame in frames.iter_mut() {
                let index = voice.position as usize;
                let Some(&current) = sound.frames.get(index) else {
                    return false;
                };
                let next = sound.frames.get(index + 1).copied().unwrap_or(current);
                let blend = (voice.position - index as f64) as f32;
                frame[0] += (current[0] + ((next[0] - current[0]) * blend)) * voice.gain;
                frame[1] += (current[1] + ((next[1] - current[1]) * blend)) * voice.gain;
                voice.position += step;
            }
            (voice.position as usize) < sound.frames.len()
        });
        for frame in frames {
            frame[0] = frame[0].clamp(-1.0, 1.0);
            frame[1] = frame[1].clamp(-1.0, 1.0);
        }
    }
}

/// The default output device's stream, or none when the machine has no usable device.
pub(crate) struct NativeOutput {
    stream: Option<cpal::Stream>,
    mixer: Arc<Mutex<Mixer>>,
}

impl NativeOutput {
    /// Open and start the default output; failures are logged and leave a silent output.
    pub(crate) fn new(sounds: &[(String, DecodedSound)]) -> Result<Self, (usize, String)> {
        let mixer = Arc::new(Mutex::new(Mixer {
            sounds: sounds.iter().map(|(_, sound)| sound.clone()).collect(),
            voices: Vec::new(),
            output_rate: 48_000.0,
            next_id: 0,
        }));
        let stream = match open_stream(&mixer) {
            Ok(stream) => Some(stream),
            Err(error) => {
                log::warn!("Audio output is unavailable: {error}");
                None
            }
        };
        Ok(Self { stream, mixer })
    }

    pub(crate) fn running(&self) -> bool {
        self.stream.is_some()
    }

    pub(crate) fn resume(&mut self) -> bool {
        self.stream.as_ref().is_some_and(|stream| stream.play().is_ok())
    }

    pub(crate) fn play(&mut self, sound: usize, volume: f32, sound_limit: usize, bank_limit: usize) -> bool {
        let mut mixer = lock(&self.mixer);
        let records: Vec<VoiceRecord> =
            mixer.voices.iter().map(|voice| VoiceRecord { id: voice.id, sound: voice.sound }).collect();
        let stolen = voices_to_steal(&records, sound, sound_limit, bank_limit);
        mixer.voices.retain(|voice| !stolen.contains(&voice.id));
        let id = mixer.next_id;
        mixer.next_id += 1;
        mixer.voices.push(MixVoice { id, sound, position: 0.0, gain: volume });
        true
    }

    pub(crate) fn active_voice_count(&mut self) -> usize {
        lock(&self.mixer).voices.len()
    }

    pub(crate) fn close(&mut self) {
        lock(&self.mixer).voices.clear();
        self.stream = None;
    }
}

/// The mixer survives a panic elsewhere holding its lock; its state stays consistent between whole operations.
fn lock(mixer: &Mutex<Mixer>) -> MutexGuard<'_, Mixer> {
    mixer.lock().unwrap_or_else(PoisonError::into_inner)
}

fn open_stream(mixer: &Arc<Mutex<Mixer>>) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let device = cpal::default_host().default_output_device().ok_or("no output device")?;
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    lock(mixer).output_rate = f64::from(config.sample_rate);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, mixer.clone())?,
        SampleFormat::F64 => build::<f64>(&device, config, mixer.clone())?,
        SampleFormat::I16 => build::<i16>(&device, config, mixer.clone())?,
        SampleFormat::I32 => build::<i32>(&device, config, mixer.clone())?,
        SampleFormat::U16 => build::<u16>(&device, config, mixer.clone())?,
        SampleFormat::U8 => build::<u8>(&device, config, mixer.clone())?,
        SampleFormat::I8 => build::<i8>(&device, config, mixer.clone())?,
        other => return Err(format!("unsupported sample format {other}").into()),
    };
    stream.play()?;
    Ok(stream)
}

fn build<T>(device: &cpal::Device, config: StreamConfig, mixer: Arc<Mutex<Mixer>>) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels).max(1);
    let mut mix: Vec<[f32; 2]> = Vec::new();
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], _| {
            mix.clear();
            mix.resize(data.len() / channels, [0.0; 2]);
            lock(&mixer).mix(&mut mix);
            for (output, [left, right]) in data.chunks_exact_mut(channels).zip(mix.iter().copied()) {
                if let [only] = output {
                    *only = T::from_sample((left + right) * 0.5);
                    continue;
                }
                for (channel, sample) in output.iter_mut().enumerate() {
                    *sample = T::from_sample(match channel {
                        0 => left,
                        1 => right,
                        _ => 0.0,
                    });
                }
            }
        },
        |error| log::warn!("Audio output error: {error}"),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_resamples_sums_clamps_and_retires_finished_voices() {
        let mut mixer = Mixer {
            sounds: vec![
                DecodedSound { sample_rate: 24_000, frames: vec![[0.0, 0.0], [1.0, -1.0]] },
                DecodedSound { sample_rate: 48_000, frames: vec![[0.75, 0.75]; 3] },
            ],
            voices: vec![
                MixVoice { id: 0, sound: 0, position: 0.0, gain: 1.0 },
                MixVoice { id: 1, sound: 1, position: 0.0, gain: 0.5 },
            ],
            output_rate: 48_000.0,
            next_id: 2,
        };
        let mut frames = vec![[0.0; 2]; 2];
        mixer.mix(&mut frames);
        // Half-rate sound: frame 0, then halfway to frame 1; plus 0.375 from the second voice.
        assert_eq!(frames, [[0.375, 0.375], [0.875, -0.125]]);
        assert_eq!(mixer.voices.len(), 2);

        let mut frames = vec![[0.0; 2]; 2];
        mixer.mix(&mut frames);
        // The first voice holds its last frame through its final half step, then ends; the second plays its third
        // frame and ends.
        assert_eq!(frames, [[1.0, -0.625], [1.0, -1.0]]);
        assert!(mixer.voices.is_empty());
    }
}
