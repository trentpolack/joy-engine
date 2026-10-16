// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Decode sound sources into stereo frames, validated so every backend can play them.

use std::io::Cursor;

use crate::runtime::audio::{DecodedSound, PcmSound, SoundSource};

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
mod tests {
    use super::*;

    /// A canonical 16-bit WAV file.
    fn wav16(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let spec =
            hound::WavSpec { channels, sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::new(Cursor::new(&mut bytes), spec).unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
        bytes
    }

    #[test]
    fn wav_files_decode_to_normalized_stereo_frames() {
        let mono = decode(&SoundSource::Wav(wav16(22_050, 1, &[16_384, -32_768]).into())).unwrap();
        assert_eq!(mono, DecodedSound { sample_rate: 22_050, frames: vec![[0.5, 0.5], [-1.0, -1.0]] });
        let stereo = decode(&SoundSource::Wav(wav16(44_100, 2, &[16_384, 0]).into())).unwrap();
        assert_eq!(stereo.frames, [[0.5, 0.0]]);
    }

    #[test]
    fn unplayable_data_is_rejected() {
        assert!(decode(&SoundSource::Wav(b"RIFF".to_vec().into())).is_err());
        let pcm = |sample_rate, channels, samples: Vec<f32>| {
            decode(&SoundSource::Pcm(PcmSound { sample_rate, channels, samples }))
        };
        assert!(pcm(4_000, 1, vec![0.0]).is_err());
        assert!(pcm(8_000, 0, vec![0.0]).is_err());
        assert!(pcm(8_000, 2, vec![0.0]).is_err());
        assert!(pcm(8_000, 1, Vec::new()).is_err());
        assert!(pcm(8_000, 1, vec![f32::NAN]).is_err());
        // Out-of-range samples clamp rather than fail.
        assert_eq!(pcm(8_000, 1, vec![2.0]).unwrap().frames, [[1.0, 1.0]]);
    }
}
