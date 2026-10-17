// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! CPU-side texture images shared by transparent textured draws and PBR material maps.
//!
//! A [`TextureAsset`] is immutable and cheaply cloned, like [`MeshAsset`](crate::rendering::MeshAsset): the renderer
//! uploads it the first time a frame draws it and releases the GPU copy once every handle is dropped, so owners never
//! hold device resources. Pixels are sRGB-encoded RGBA8 with straight alpha; the GPU decodes them to linear when
//! sampling.
//!
//! PNG, JPEG, and WebP files decode on every target ([`TextureAsset::decode_image`]), including embedded
//! `data:image/…;base64,` sources ([`TextureAsset::decode_data_url`]) such as FORM material maps, without color-space
//! conversion or premultiplication.

use std::sync::Arc;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use crate::rendering::RenderError;

/// Largest accepted texture extent per axis, in pixels. Devices with smaller limits reject the draw at render time.
pub const MAX_TEXTURE_DIMENSION: u32 = 8192;

/// Image formats [`TextureAsset::decode_image`] recognizes by their signatures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    /// Portable Network Graphics.
    Png,
    /// JPEG (baseline or progressive).
    Jpeg,
    /// WebP (lossy, lossless, or the first frame of an animation).
    Webp,
}

impl ImageFormat {
    /// The format whose signature starts `bytes`, if any.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
            Some(Self::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(Self::Jpeg)
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            Some(Self::Webp)
        } else {
            None
        }
    }

    /// The `image/…` media type of the format.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    /// The format named by an `image/…` media type.
    pub fn from_media_type(media_type: &str) -> Option<Self> {
        match media_type {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// Process-unique texture identities, used to key GPU uploads.
static NEXT_TEXTURE_ID: AtomicU64 = AtomicU64::new(1);

/// An immutable sRGB RGBA8 image with straight alpha.
#[derive(Clone)]
pub struct TextureAsset {
    inner: Arc<TextureAssetInner>,
}

struct TextureAssetInner {
    id: u64,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl std::fmt::Debug for TextureAsset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextureAsset")
            .field("id", &self.inner.id)
            .field("width", &self.inner.width)
            .field("height", &self.inner.height)
            .finish()
    }
}

impl TextureAsset {
    /// Take ownership of tightly packed, top-to-bottom RGBA8 rows.
    ///
    /// Fails unless both extents are 1–[`MAX_TEXTURE_DIMENSION`] and `pixels` holds exactly `width * height * 4` bytes.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, RenderError> {
        let valid = 1..=MAX_TEXTURE_DIMENSION;
        if !valid.contains(&width) || !valid.contains(&height) {
            return Err(RenderError::InvalidTexture(format!(
                "texture extents must be 1 to {MAX_TEXTURE_DIMENSION} pixels, not {width}x{height}"
            )));
        }
        let expected = (width as usize) * (height as usize) * 4;
        if pixels.len() != expected {
            return Err(RenderError::InvalidTexture(format!(
                "a {width}x{height} RGBA8 texture needs {expected} bytes, not {}",
                pixels.len()
            )));
        }
        let id = NEXT_TEXTURE_ID.fetch_add(1, Ordering::Relaxed);
        Ok(Self { inner: Arc::new(TextureAssetInner { id, width, height, pixels }) })
    }

    /// Decode a PNG file into RGBA8 without color-space conversion or alpha premultiplication.
    ///
    /// Palette, grayscale, and 16-bit images expand to 8-bit RGBA, and `tRNS` transparency becomes alpha, matching a
    /// browser's `createImageBitmap(…, {premultiplyAlpha: 'none', colorSpaceConversion: 'none'})`. Animated PNGs
    /// decode their default image.
    pub fn decode_png(bytes: &[u8]) -> Result<Self, RenderError> {
        let invalid = |error: png::DecodingError| RenderError::InvalidTexture(format!("PNG decoding failed: {error}"));
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().map_err(invalid)?;
        let (width, height) = {
            let info = reader.info();
            (info.width, info.height)
        };
        let valid = 1..=MAX_TEXTURE_DIMENSION;
        if !valid.contains(&width) || !valid.contains(&height) {
            return Err(RenderError::InvalidTexture(format!("PNG extents {width}x{height} are not supported")));
        }
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| RenderError::InvalidTexture("PNG output size overflows".to_owned()))?;
        let mut buffer = vec![0; size];
        let frame = reader.next_frame(&mut buffer).map_err(invalid)?;
        let samples = &buffer[..frame.buffer_size()];

        // Expand every 8-bit layout to RGBA.
        let pixel_count = (width as usize) * (height as usize);
        let mut pixels = Vec::with_capacity(pixel_count * 4);
        let row_bytes = frame.line_size;
        let channels = match frame.color_type {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Indexed => {
                return Err(RenderError::InvalidTexture("PNG palette did not expand".to_owned()));
            }
        };
        for row in samples.chunks(row_bytes).take(height as usize) {
            for pixel in row.chunks_exact(channels).take(width as usize) {
                let rgba = match channels {
                    1 => [pixel[0], pixel[0], pixel[0], 255],
                    2 => [pixel[0], pixel[0], pixel[0], pixel[1]],
                    3 => [pixel[0], pixel[1], pixel[2], 255],
                    _ => [pixel[0], pixel[1], pixel[2], pixel[3]],
                };
                pixels.extend_from_slice(&rgba);
            }
        }
        Self::from_rgba8(width, height, pixels)
    }

    /// Decode a PNG, JPEG, or WebP file, chosen by its signature, into RGBA8 without color-space conversion or alpha
    /// premultiplication. Grayscale and RGB images become opaque RGBA; animated images decode their first frame.
    ///
    /// Fails for unrecognized or corrupt files and for images outside 1–[`MAX_TEXTURE_DIMENSION`] pixels per axis.
    pub fn decode_image(bytes: &[u8]) -> Result<Self, RenderError> {
        match ImageFormat::sniff(bytes) {
            Some(ImageFormat::Png) => Self::decode_png(bytes),
            Some(ImageFormat::Jpeg) => Self::decode_jpeg(bytes),
            Some(ImageFormat::Webp) => Self::decode_webp(bytes),
            None => Err(RenderError::InvalidTexture("images must be PNG, JPEG, or WebP files".to_owned())),
        }
    }

    /// Decode an embedded `data:image/png;base64,…`, `data:image/jpeg;base64,…`, or `data:image/webp;base64,…`
    /// source. The declared media type must match the decoded file's signature.
    pub fn decode_data_url(source: &str) -> Result<Self, RenderError> {
        let invalid = |message: &str| RenderError::InvalidTexture(message.to_owned());
        let (header, payload) = source
            .strip_prefix("data:")
            .and_then(|rest| rest.split_once(','))
            .ok_or_else(|| invalid("embedded images must be data: URLs"))?;
        let media_type =
            header.strip_suffix(";base64").ok_or_else(|| invalid("embedded images must be base64 data: URLs"))?;
        let declared = ImageFormat::from_media_type(media_type)
            .ok_or_else(|| invalid("embedded images must be PNG, JPEG, or WebP"))?;
        let bytes =
            crate::core::decode_base64(payload).ok_or_else(|| invalid("embedded image data is not valid base64"))?;
        if ImageFormat::sniff(&bytes) != Some(declared) {
            return Err(invalid("embedded image data does not match its declared format"));
        }
        Self::decode_image(&bytes)
    }

    /// Decode a baseline or progressive JPEG into opaque RGBA8 (grayscale and CMYK files convert to RGB).
    pub fn decode_jpeg(bytes: &[u8]) -> Result<Self, RenderError> {
        let invalid = |error: zune_jpeg::errors::DecodeErrors| {
            RenderError::InvalidTexture(format!("JPEG decoding failed: {error}"))
        };
        let options = zune_jpeg::zune_core::options::DecoderOptions::default()
            .set_max_width(MAX_TEXTURE_DIMENSION as usize)
            .set_max_height(MAX_TEXTURE_DIMENSION as usize)
            .jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::RGBA);
        let mut decoder =
            zune_jpeg::JpegDecoder::new_with_options(zune_jpeg::zune_core::bytestream::ZCursor::new(bytes), options);
        decoder.decode_headers().map_err(invalid)?;
        let (width, height) = decoder
            .dimensions()
            .ok_or_else(|| RenderError::InvalidTexture("JPEG headers are incomplete".to_owned()))?;
        let pixels = decoder.decode().map_err(invalid)?;
        Self::from_rgba8(width as u32, height as u32, pixels)
    }

    /// Decode a lossy or lossless WebP (the first frame of an animation) into RGBA8.
    pub fn decode_webp(bytes: &[u8]) -> Result<Self, RenderError> {
        let invalid =
            |error: image_webp::DecodingError| RenderError::InvalidTexture(format!("WebP decoding failed: {error}"));
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).map_err(invalid)?;
        let (width, height) = decoder.dimensions();
        let valid = 1..=MAX_TEXTURE_DIMENSION;
        if !valid.contains(&width) || !valid.contains(&height) {
            return Err(RenderError::InvalidTexture(format!("WebP extents {width}x{height} are not supported")));
        }
        let size = decoder
            .output_buffer_size()
            .ok_or_else(|| RenderError::InvalidTexture("WebP output size overflows".to_owned()))?;
        let mut buffer = vec![0; size];
        decoder.read_image(&mut buffer).map_err(invalid)?;
        let pixels = if decoder.has_alpha() {
            buffer
        } else {
            buffer.as_chunks::<3>().0.iter().flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255]).collect()
        };
        Self::from_rgba8(width, height, pixels)
    }

    /// Process-unique identity used to key GPU uploads.
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.inner.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.inner.height
    }

    /// Tightly packed, top-to-bottom sRGB RGBA8 rows with straight alpha.
    pub fn pixels(&self) -> &[u8] {
        &self.inner.pixels
    }

    /// Whether two handles refer to the same image.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// A type-erased weak reference used by the GPU cache to release uploads for dropped textures.
    pub(crate) fn downgrade(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync> {
        let strong: Arc<dyn std::any::Any + Send + Sync> = self.inner.clone();
        Arc::downgrade(&strong)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_images_validate_their_size() {
        assert!(TextureAsset::from_rgba8(2, 1, vec![0; 8]).is_ok());
        assert!(matches!(TextureAsset::from_rgba8(2, 1, vec![0; 7]), Err(RenderError::InvalidTexture(_))));
        assert!(matches!(TextureAsset::from_rgba8(0, 1, Vec::new()), Err(RenderError::InvalidTexture(_))));
    }

    #[test]
    fn png_images_expand_to_rgba() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
            encoder.set_color(png::ColorType::GrayscaleAlpha);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[10, 20, 200, 255]).unwrap();
        }
        let texture = TextureAsset::decode_png(&encoded).unwrap();
        assert_eq!((texture.width(), texture.height()), (2, 1));
        assert_eq!(texture.pixels(), &[10, 10, 10, 20, 200, 200, 200, 255]);
        assert!(TextureAsset::decode_png(b"not a png").is_err());
    }

    /// A lossless 2x1 WebP: half-transparent red, then opaque blue.
    const WEBP_LOSSLESS: &str = "data:image/webp;base64,UklGRh4AAABXRUJQVlA4TBIAAAAvAQAAEA8Q87//D/wPBxnR/wA=";
    /// A lossy, opaque 2x2 WebP.
    const WEBP_LOSSY: &str = "data:image/webp;base64,UklGRkwAAABXRUJQVlA4IEAAAAAQAgCdASoCAAIAAAAAJaQADqAPw5NCld8AAP7//qnH/7/V/4uX/91Zaf/n/f/53q1AiVOsd/6wD/8NQP2AAAAA";
    /// A 1x1 grayscale JPEG of level 100.
    const JPEG_GRAY: &str = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQH/wAALCAABAAEBAREA/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/9oACAEBAAA/APh+v//Z";

    #[test]
    fn webp_and_jpeg_images_decode_to_straight_rgba() {
        let lossless = TextureAsset::decode_data_url(WEBP_LOSSLESS).unwrap();
        assert_eq!((lossless.width(), lossless.height()), (2, 1));
        assert_eq!(lossless.pixels(), &[255, 0, 0, 128, 0, 0, 255, 255], "alpha stays straight");

        let lossy = TextureAsset::decode_data_url(WEBP_LOSSY).unwrap();
        assert_eq!((lossy.width(), lossy.height()), (2, 2));
        assert!(lossy.pixels().as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255), "opaque WebP expands to RGBA");

        let gray = TextureAsset::decode_data_url(JPEG_GRAY).unwrap();
        assert_eq!((gray.width(), gray.height()), (1, 1));
        let pixel = gray.pixels();
        assert!(pixel[..3].iter().all(|channel| channel.abs_diff(100) <= 2) && pixel[3] == 255, "{pixel:?}");
    }

    #[test]
    fn data_urls_must_declare_the_format_they_contain() {
        let mismatched = WEBP_LOSSLESS.replace("image/webp", "image/png");
        assert!(TextureAsset::decode_data_url(&mismatched).is_err());
        for malformed in ["https://example.com/a.png", "data:image/gif;base64,R0lGOD==", "data:image/png,raw"] {
            assert!(TextureAsset::decode_data_url(malformed).is_err(), "{malformed}");
        }
        assert_eq!(ImageFormat::sniff(b"RIFF\0\0\0\0WEBPVP8L"), Some(ImageFormat::Webp));
        assert!(TextureAsset::decode_image(b"GIF89a").is_err());
    }
}
