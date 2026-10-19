// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/glb_mesh_loader.rs`.

use super::*;

#[test]
fn base64_decodes_padded_and_unpadded_text() {
    assert_eq!(decode_base64("AAECAw==").unwrap(), [0, 1, 2, 3]);
    assert_eq!(decode_base64("AAECAw").unwrap(), [0, 1, 2, 3]);
    assert_eq!(decode_base64("bWVzaA==").unwrap(), b"mesh");
    assert!(decode_base64("!!").is_none());
}
