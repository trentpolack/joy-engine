// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Decode sound sources into stereo frames, validated so every backend can play them.

use std::io::Cursor;

use crate::{
    DecodedSound,
    PcmSound,
    SoundSource,
};

/// Sample rates every browser's Web Audio accepts for buffers.
const SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=96_000;

/// Decode one source; the error says why it cannot play.
pub(crate) fn decode(source: &SoundSource) -> Result<DecodedSound, String> {
    match source {
        SoundSource::Wav(bytes) => decode_wav(bytes),
        SoundSource::Pcm(sound) => decode_pcm(sound),
    }
}

fn decode_wav(bytes: &[u8]) -> Result<DecodedSound, String> {
    let mut reader = hound::WavReader::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1_u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader.samples::<i32>().map(|sample| sample.map(|value| value as f32 * scale)).collect()
        }
    }
    .map_err(|error| error.to_string())?;
    decode_pcm(&PcmSound { sample_rate: spec.sample_rate, channels: spec.channels, samples })
}

fn decode_pcm(sound: &PcmSound) -> Result<DecodedSound, String> {
    if sound.channels == 0 {
        return Err("a sound needs at least one channel".to_owned());
    }
    if !SAMPLE_RATES.contains(&sound.sample_rate) {
        return Err(format!("sample rate {} Hz is outside 8,000 to 96,000 Hz", sound.sample_rate));
    }
    let channels = usize::from(sound.channels);
    if sound.samples.is_empty() || !sound.samples.len().is_multiple_of(channels) {
        return Err("samples must hold one or more whole frames".to_owned());
    }
    if sound.samples.iter().any(|sample| !sample.is_finite()) {
        return Err("samples must be finite".to_owned());
    }
    let frames = sound
        .samples
        .chunks_exact(channels)
        .map(|frame| {
            let left = frame[0].clamp(-1.0, 1.0);
            [left, frame.get(1).map_or(left, |right| right.clamp(-1.0, 1.0))]
        })
        .collect();
    Ok(DecodedSound { sample_rate: sound.sample_rate, frames })
}

#[cfg(test)]
#[path = "../tests/unit/decode.rs"]
mod tests;
