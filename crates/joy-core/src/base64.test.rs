// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/base64.rs`.

use super::*;

#[test]
fn encoding_round_trips_every_padding_length() {
    assert_eq!(encode_base64(b""), "");
    assert_eq!(encode_base64(b"m"), "bQ==");
    assert_eq!(encode_base64(b"me"), "bWU=");
    assert_eq!(encode_base64(b"mesh"), "bWVzaA==");
    let bytes: Vec<u8> = (0..=255).collect();
    assert_eq!(decode_base64(&encode_base64(&bytes)).unwrap(), bytes);
}

#[test]
fn decoding_rejects_noncanonical_text() {
    assert_eq!(decode_base64("AAECAw==").unwrap(), [0, 1, 2, 3]);
    for malformed in ["AAECAw", "AA=A", "AAA=AAAA", "A===", "AA AA", "AA-_", "!!!!"] {
        assert!(decode_base64(malformed).is_none(), "{malformed}");
    }
}
