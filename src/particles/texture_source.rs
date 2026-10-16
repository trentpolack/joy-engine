// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Embedded PNG particle texture sources (`data:image/png;base64,…`).
//!
//! Validation reads only the PNG signature and IHDR header, before any decoding or GPU allocation: at most 1 MiB of
//! PNG data and 1–2048 pixels per axis.

/// Original procedural 16×16 white cross with transparent corners.
pub const DEFAULT_PARTICLE_TEXTURE: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAJElEQVR4nGP4//8/Aw6MDrCqw6V51AASDaAIDBMDBj4WRroBAMgTbL72rES3AAAAAElFTkSuQmCC";

/// Required prefix of every particle texture source.
const DATA_URL_PREFIX: &str = "data:image/png;base64,";

/// Largest decoded PNG size in bytes.
const MAXIMUM_PNG_BYTES: usize = 1024 * 1024;

/// Largest accepted base64 payload, in characters (`ceil(1 MiB / 3) * 4`).
const MAXIMUM_ENCODED_LENGTH: usize = MAXIMUM_PNG_BYTES.div_ceil(3) * 4;

/// Largest texture extent per axis, in pixels.
pub const MAXIMUM_PARTICLE_TEXTURE_SIZE: u32 = 2048;

/// The eight-byte PNG file signature.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Pixel dimensions read from a PNG header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParticleTextureDimensions {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// An embedded particle texture was malformed, oversized, undecodable, or not prepared.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParticleTextureError {
    /// The source was not a string starting with `data:image/png;base64,`.
    #[error("Particle texture source must be an embedded PNG data URL.")]
    NotDataUrl,
    /// The payload was not canonical padded base64, or exceeded the encoded size limit.
    #[error("Particle PNG must be valid base64 and at most 1 MiB.")]
    InvalidBase64,
    /// The decoded payload exceeded 1 MiB.
    #[error("Particle PNG must be at most 1 MiB.")]
    TooLarge,
    /// The payload did not start with a PNG signature and IHDR chunk.
    #[error("Particle texture has an invalid PNG signature or IHDR header.")]
    InvalidHeader,
    /// The header declared a width or height outside 1–2048 pixels.
    #[error("Particle PNG dimensions must be 1–2048 pixels per axis.")]
    InvalidDimensions,
    /// The PNG body failed to decode.
    #[error("Particle image could not be decoded: {0}")]
    Decode(String),
    /// A batch referenced a source the texture set has not prepared.
    #[error("Particle image is not ready; prepare the ParticleTextureSet before rendering.")]
    NotReady,
}

/// Validate an embedded PNG source and return its pixel dimensions without decoding pixels.
pub fn read_particle_texture_source(source: &str) -> Result<ParticleTextureDimensions, ParticleTextureError> {
    let bytes = decode_particle_texture_source(source)?;
    read_png_header(&bytes)
}

/// Embed PNG file bytes as a particle texture source (`data:image/png;base64,…`), the counterpart of a browser's
/// `FileReader.readAsDataURL` in PARTICLE LAB's texture upload. Fails with the same checks as
/// [`read_particle_texture_source`]: at most 1 MiB, a PNG signature and IHDR header, and 1–2048 pixels per axis.
pub fn encode_particle_texture_source(png: &[u8]) -> Result<String, ParticleTextureError> {
    if png.len() > MAXIMUM_PNG_BYTES {
        return Err(ParticleTextureError::TooLarge);
    }
    read_png_header(png)?;
    Ok(format!("{DATA_URL_PREFIX}{}", encode_base64(png)))
}

/// Validate an embedded PNG source and return the PNG file bytes it carries.
pub(crate) fn decode_particle_texture_source(source: &str) -> Result<Vec<u8>, ParticleTextureError> {
    let Some(encoded) = source.strip_prefix(DATA_URL_PREFIX) else {
        return Err(ParticleTextureError::NotDataUrl);
    };
    if encoded.len() > MAXIMUM_ENCODED_LENGTH {
        return Err(ParticleTextureError::InvalidBase64);
    }
    let bytes = decode_base64(encoded).ok_or(ParticleTextureError::InvalidBase64)?;
    if bytes.len() > MAXIMUM_PNG_BYTES {
        return Err(ParticleTextureError::TooLarge);
    }
    read_png_header(&bytes)?;
    Ok(bytes)
}

/// Check the signature and IHDR chunk, returning the declared dimensions.
fn read_png_header(bytes: &[u8]) -> Result<ParticleTextureDimensions, ParticleTextureError> {
    let read_u32 = |offset: usize| -> u32 {
        u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
    };
    if bytes.len() < 33 || bytes[..8] != PNG_SIGNATURE || read_u32(8) != 13 || &bytes[12..16] != b"IHDR" {
        return Err(ParticleTextureError::InvalidHeader);
    }
    let width = read_u32(16);
    let height = read_u32(20);
    let valid = 1..=MAXIMUM_PARTICLE_TEXTURE_SIZE;
    if !valid.contains(&width) || !valid.contains(&height) {
        return Err(ParticleTextureError::InvalidDimensions);
    }
    Ok(ParticleTextureDimensions { width, height })
}

/// Decode canonical base64 (`(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?`), or `None` when the
/// text does not match that pattern exactly.
fn decode_base64(encoded: &str) -> Option<Vec<u8>> {
    let sextet = |byte: u8| -> Option<u32> {
        Some(u32::from(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        }))
    };
    let bytes = encoded.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::with_capacity((bytes.len() / 4) * 3);
    for (index, group) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let padding = group.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return None;
        }
        let mut bits = 0_u32;
        for byte in &group[..4 - padding] {
            bits = (bits << 6) | sextet(*byte)?;
        }
        bits <<= 6 * padding as u32;
        let decoded = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        output.extend_from_slice(&decoded[..3 - padding]);
    }
    Some(output)
}

/// Encode canonical padded base64.
fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(chunk.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        for index in 0..4 {
            if index <= chunk.len() {
                output.push(char::from(ALPHABET[((bits >> (18 - (6 * index))) & 0x3f) as usize]));
            } else {
                output.push('=');
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_texture_is_a_sixteen_pixel_png() {
        assert_eq!(
            read_particle_texture_source(DEFAULT_PARTICLE_TEXTURE),
            Ok(ParticleTextureDimensions { width: 16, height: 16 })
        );
    }

    #[test]
    fn malformed_sources_are_rejected_with_reference_messages() {
        assert_eq!(read_particle_texture_source("https://example.com/a.png"), Err(ParticleTextureError::NotDataUrl));
        assert_eq!(read_particle_texture_source("data:image/png;base64,AAA"), Err(ParticleTextureError::InvalidBase64));
        assert_eq!(
            read_particle_texture_source("data:image/png;base64,AA=A"),
            Err(ParticleTextureError::InvalidBase64)
        );
        assert_eq!(
            read_particle_texture_source("data:image/png;base64,AAAA"),
            Err(ParticleTextureError::InvalidHeader)
        );
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
        assert_eq!(
            encode_particle_texture_source(&vec![0; MAXIMUM_PNG_BYTES + 1]),
            Err(ParticleTextureError::TooLarge)
        );
    }
}
