// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/texture_source.rs`.

use super::*;

#[test]
fn default_texture_is_a_sixteen_pixel_png() {
    assert_eq!(
        read_particle_texture_source(DEFAULT_PARTICLE_TEXTURE),
        Ok(ParticleTextureDimensions {
            width: 16,
            height: 16
        })
    );
}

#[test]
fn malformed_sources_are_rejected_with_reference_messages() {
    assert_eq!(read_particle_texture_source("https://example.com/a.png"), Err(ParticleTextureError::NotDataUrl));
    assert_eq!(read_particle_texture_source("data:image/png;base64,AAA"), Err(ParticleTextureError::InvalidBase64));
    assert_eq!(read_particle_texture_source("data:image/png;base64,AA=A"), Err(ParticleTextureError::InvalidBase64));
    assert_eq!(read_particle_texture_source("data:image/png;base64,AAAA"), Err(ParticleTextureError::InvalidHeader));
    assert_eq!(decode_base64("TWFu").as_deref(), Some(&b"Man"[..]));
    assert_eq!(decode_base64("TWE=").as_deref(), Some(&b"Ma"[..]));
}

#[test]
fn png_bytes_embed_as_sources_that_read_back() {
    let png = decode_particle_texture_source(DEFAULT_PARTICLE_TEXTURE).unwrap();
    assert_eq!(encode_particle_texture_source(&png).unwrap(), DEFAULT_PARTICLE_TEXTURE);
    for text in ["", "M", "Ma", "Man", "Many"] {
        assert_eq!(decode_base64(&encode_base64(text.as_bytes())).unwrap(), text.as_bytes());
    }
    assert_eq!(encode_particle_texture_source(b"not a png"), Err(ParticleTextureError::InvalidHeader));
    assert_eq!(encode_particle_texture_source(&vec![0; MAXIMUM_PNG_BYTES + 1]), Err(ParticleTextureError::TooLarge));
}
