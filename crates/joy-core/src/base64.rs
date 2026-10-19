// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Standard padded base64 (RFC 4648 §4), the encoding of the `data:` URLs that FORM materials, particle textures, and
//! glTF buffers embed. Matches the browser's `btoa`/`atob` for well-formed input and rejects everything else, so
//! authored documents never decode differently in the two engines.

/// The 64 standard alphabet characters in value order.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as padded standard base64, like `btoa` over a binary string.
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let bits = (u32::from(group[0]) << 16)
            | (u32::from(group.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(group.get(2).copied().unwrap_or(0));
        for index in 0..4 {
            if index <= group.len() {
                let sextet = (bits >> (18 - (6 * index))) & 0x3f;
                output.push(char::from(ALPHABET[sextet as usize]));
            } else {
                output.push('=');
            }
        }
    }
    output
}

/// Decode canonical padded base64 (`(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?`), or `None` when
/// the text has whitespace, a URL-safe alphabet, missing or misplaced padding, or any other character.
pub fn decode_base64(encoded: &str) -> Option<Vec<u8>> {
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
    let groups = bytes.len() / 4;
    let mut output = Vec::with_capacity(groups * 3);
    for (index, group) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let padding = group.iter().rev().take_while(|byte| **byte == b'=').count();
        if padding > 2 || (padding > 0 && index + 1 != groups) {
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

#[cfg(test)]
#[path = "base64.test.rs"]
mod tests;
