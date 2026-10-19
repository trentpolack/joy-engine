// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/decode.rs`.

use super::*;

/// A canonical 16-bit WAV file.
fn wav16(sample_rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let spec = hound::WavSpec { channels, sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
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
