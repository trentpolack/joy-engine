// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Typed events for any system that produces things other systems react to: a gameplay world's contacts and timers,
//! a physics simulation's contact pairs, script messages, or a game's own combat and reaction records.
//!
//! Events are plain Rust values keyed by type: a producer defines `struct EnemyDefeated { .. }`, sends it, and readers
//! ask for `EnemyDefeated` by type. No event names are strings.
//!
//! - [`EventQueue`] holds events of any number of types, each kept in send order.
//! - [`EventChannel`] is the update-boundary owner. It keeps the output of the last few updates: what the update in
//!   progress produces, what the previous update delivered to it, and older updates for readers that run less often.
//!   Systems that run inside every update read [`EventChannel::delivered`]; code that runs after an update reads
//!   [`EventChannel::produced`].
//! - [`EventCursor`] reads one event type from a channel at its own rate. Presentation reads once per displayed frame
//!   while simulation may run zero or several fixed ticks in that frame; a cursor returns every event sent since its
//!   last read, once, however many updates that spans.
//!
//! Every event a channel accepts gets the next [`EventSequence`], shared by all types, so cursors know exactly what they
//! have read. Channels are single-threaded owners; independent channels never share state.

mod event_channel;
mod event_queue;

pub use event_channel::{
    DEFAULT_RETAINED_UPDATES,
    EventChannel,
    EventCursor,
};
pub use event_queue::{
    EventQueue,
    EventSequence,
};
