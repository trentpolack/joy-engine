// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/texture.rs`.

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
