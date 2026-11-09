// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/events/event_channel.rs`.

use super::*;

#[derive(Clone, Debug, PartialEq)]
struct Impact(u32);

#[derive(Clone, Debug, PartialEq)]
struct Pickup(&'static str);

fn read_impacts(cursor: &mut EventCursor<Impact>, channel: &EventChannel) -> Vec<u32> {
    cursor.read(channel).map(|impact| impact.0).collect()
}

#[test]
fn updates_deliver_what_the_previous_update_produced() {
    let mut channel = EventChannel::new();
    channel.advance();
    channel.send(Impact(1));
    channel.send(Pickup("coin"));
    assert_eq!(channel.produced().read::<Impact>(), &[Impact(1)]);
    assert!(channel.delivered().is_empty());

    // The next update reads the previous update's output; the one after no longer does.
    channel.advance();
    assert_eq!(channel.delivered().read::<Impact>(), &[Impact(1)]);
    assert_eq!(channel.delivered().read::<Pickup>(), &[Pickup("coin")]);
    assert!(channel.produced().is_empty());
    channel.advance();
    assert!(channel.delivered().is_empty());
}

#[test]
fn a_cursor_reads_each_event_once_however_many_updates_run_between_reads() {
    let mut channel = EventChannel::with_retained_updates(4);
    let mut frame_cursor = EventCursor::<Impact>::new();

    // A frame with no update reads nothing new; a frame with three updates reads all three updates' events in order.
    channel.advance();
    channel.send(Impact(1));
    assert_eq!(read_impacts(&mut frame_cursor, &channel), vec![1]);
    assert_eq!(read_impacts(&mut frame_cursor, &channel), Vec::<u32>::new());
    for value in 2..5 {
        channel.advance();
        channel.send(Impact(value));
        channel.send(Pickup("ignored by an impact cursor"));
    }
    assert!(!frame_cursor.missed_events(&channel));
    assert_eq!(read_impacts(&mut frame_cursor, &channel), vec![2, 3, 4]);

    // A cursor taken from the channel skips everything already sent.
    let mut late_cursor = channel.cursor::<Impact>();
    channel.send(Impact(5));
    assert_eq!(read_impacts(&mut late_cursor, &channel), vec![5]);

    // Falling further behind than the channel retains is reported, and what is still retained is still read.
    for value in 6..12 {
        channel.advance();
        channel.send(Impact(value));
    }
    assert!(frame_cursor.missed_events(&channel));
    assert_eq!(read_impacts(&mut frame_cursor, &channel), vec![8, 9, 10, 11]);

    // Clearing drops retained events but keeps sequences, so cursors read only what is sent afterward.
    channel.clear();
    assert_eq!(read_impacts(&mut late_cursor, &channel), Vec::<u32>::new());
    channel.send(Impact(12));
    assert_eq!(read_impacts(&mut frame_cursor, &channel), vec![12]);
}
