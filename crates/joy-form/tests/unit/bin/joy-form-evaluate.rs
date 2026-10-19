// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/bin/joy-form-evaluate.rs`.

use super::*;

#[test]
fn request_framing_bounds_lines_and_resynchronizes_after_oversized_input() {
    // Limit 4: an exact-limit line is accepted, a line whose two-byte character straddles
    // the limit is discarded whole, and the following requests keep their framing.
    let mut input = io::Cursor::new("abcd\nabcd\u{e9}\nok\nlast".as_bytes());
    let mut next = || read_request(&mut input, 4).unwrap();
    assert_eq!(next(), RequestLine::Request(b"abcd".to_vec()));
    assert_eq!(next(), RequestLine::Oversized);
    assert_eq!(next(), RequestLine::Request(b"ok".to_vec()));
    assert_eq!(next(), RequestLine::Request(b"last".to_vec()));
    assert_eq!(next(), RequestLine::End);
}
