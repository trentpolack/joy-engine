// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The update-boundary event owner and the cursors that read it at their own rate.

use std::collections::VecDeque;
use std::fmt;
use std::marker::PhantomData;

use super::event_queue::{
    EventQueue,
    EventSequence,
};
use crate::fixed_step::FixedStepLoop;

/// Updates an [`EventChannel`] keeps by default: the update in progress plus every update one displayed frame can run
/// ([`FixedStepLoop::MAXIMUM_FRAME_SECONDS`] at the default 60 updates per second is 15), so a cursor read once per
/// frame never misses an event.
pub const DEFAULT_RETAINED_UPDATES: usize =
    1 + ((FixedStepLoop::MAXIMUM_FRAME_SECONDS * FixedStepLoop::DEFAULT_UPDATES_PER_SECOND) as usize);

/// The fewest updates a channel keeps: the update in progress and the one it was delivered.
const MINIMUM_RETAINED_UPDATES: usize = 2;

/// Events produced by a sequence of updates, kept for a bounded number of updates.
///
/// The owner calls [`advance`](Self::advance) at the start of each update. Everything sent during an update is
/// [`produced`](Self::produced) by it, becomes [`delivered`](Self::delivered) to the next update, and stays readable by
/// cursors until it is older than [`retained_updates`](Self::retained_updates). Events are never read twice by one
/// cursor, and nothing is dropped from an update that is still retained.
#[derive(Clone)]
pub struct EventChannel {
    // Retained updates, oldest first; the back is the update in progress. Each records the first sequence sent to it.
    updates: VecDeque<RetainedUpdate>,
    retained_updates: usize,

    // The sequence the next event receives, and the oldest sequence still retained.
    next_sequence: EventSequence,
    oldest_sequence: EventSequence,
}

/// One update's events and the first sequence it was given.
#[derive(Clone)]
struct RetainedUpdate {
    first_sequence: EventSequence,
    events: EventQueue,
}

impl Default for EventChannel {
    fn default() -> Self {
        Self::with_retained_updates(DEFAULT_RETAINED_UPDATES)
    }
}

impl EventChannel {
    /// An empty channel keeping [`DEFAULT_RETAINED_UPDATES`] updates.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty channel keeping `updates` updates (at least two: the update in progress and its delivered set). Keep
    /// enough for the slowest cursor: one more than the most updates that run between two of its reads.
    pub fn with_retained_updates(updates: usize) -> Self {
        let retained_updates = updates.max(MINIMUM_RETAINED_UPDATES);
        let mut channel = Self {
            updates: VecDeque::with_capacity(retained_updates),
            retained_updates,
            next_sequence: 0,
            oldest_sequence: 0,
        };

        // Start with an empty delivered set and an empty update in progress, so both views exist before any update.
        for _ in 0..MINIMUM_RETAINED_UPDATES {
            channel.updates.push_back(RetainedUpdate {
                first_sequence: 0,
                events: EventQueue::new(),
            });
        }
        channel
    }

    /// Updates this channel keeps.
    pub fn retained_updates(&self) -> usize {
        self.retained_updates
    }

    /// Events this channel has accepted since it was created, of every type; clearing does not reset it.
    pub fn total_sent(&self) -> EventSequence {
        self.next_sequence
    }

    /// Send an event into the update in progress.
    pub fn send<T: Clone + 'static>(&mut self, event: T) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.in_progress_mut().events.push(sequence, event);
    }

    /// Start an update boundary: the update in progress becomes the delivered set, a new update begins, and the oldest
    /// update beyond the retention is discarded (its allocations are reused).
    pub fn advance(&mut self) {
        let mut recycled = if self.updates.len() >= self.retained_updates { self.updates.pop_front() } else { None };
        if let Some(front) = self.updates.front() {
            self.oldest_sequence = front.first_sequence;
        }

        let events = match recycled.take() {
            Some(mut update) => {
                update.events.clear();
                update.events
            }
            None => EventQueue::new(),
        };
        self.updates.push_back(RetainedUpdate {
            first_sequence: self.next_sequence,
            events,
        });
    }

    /// Events produced by the previous update: what systems running inside the current update read.
    pub fn delivered(&self) -> &EventQueue {
        &self.updates[self.updates.len() - 2].events
    }

    /// Events produced so far by the update in progress, or by the latest update once it returned.
    pub fn produced(&self) -> &EventQueue {
        &self.in_progress().events
    }

    /// A cursor that reads only events sent after this call.
    pub fn cursor<T: 'static>(&self) -> EventCursor<T> {
        EventCursor::starting_at(self.next_sequence)
    }

    /// Drop every retained event, for example when a world restarts. Sequences continue, so existing cursors stay
    /// valid and read only events sent after the clear.
    pub fn clear(&mut self) {
        for update in &mut self.updates {
            update.events.clear();
            update.first_sequence = self.next_sequence;
        }
        self.oldest_sequence = self.next_sequence;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------------------------------------------------

    fn in_progress(&self) -> &RetainedUpdate {
        self.updates.back().expect("a channel always retains at least two updates")
    }

    fn in_progress_mut(&mut self) -> &mut RetainedUpdate {
        self.updates.back_mut().expect("a channel always retains at least two updates")
    }
}

impl fmt::Debug for EventChannel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EventChannel")
            .field("retained_updates", &self.retained_updates)
            .field("next_sequence", &self.next_sequence)
            .field("produced", &self.produced().len())
            .finish()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Cursors
// ---------------------------------------------------------------------------------------------------------------------

/// Reads events of type `T` from an [`EventChannel`] at the reader's own rate: each read returns every `T` sent since
/// the previous read, in send order, across however many updates ran in between.
///
/// A cursor is plain data (the next sequence it will read), so owners keep one per event type they react to and reset
/// it by replacing it. It reads any channel, but is meant for one.
pub struct EventCursor<T> {
    next: EventSequence,
    _events: PhantomData<fn() -> T>,
}

impl<T: 'static> EventCursor<T> {
    /// A cursor that will read every `T` its channel still retains.
    pub fn new() -> Self {
        Self::starting_at(0)
    }

    /// Every `T` sent since this cursor's last read (or since it was created), oldest first. Marks them read.
    pub fn read<'a>(&mut self, channel: &'a EventChannel) -> impl Iterator<Item = &'a T> + use<'a, T> {
        let first = self.next;
        self.next = channel.next_sequence;
        channel.updates.iter().flat_map(move |update| update.events.read_from::<T>(first))
    }

    /// Whether events this cursor had not read were discarded because more updates ran than the channel retains (or
    /// the channel was cleared). Check before [`read`](Self::read) when losing an event matters.
    pub fn missed_events(&self, channel: &EventChannel) -> bool {
        self.next < channel.oldest_sequence
    }

    fn starting_at(next: EventSequence) -> Self {
        Self {
            next,
            _events: PhantomData,
        }
    }
}

impl<T: 'static> Default for EventCursor<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for EventCursor<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for EventCursor<T> {}

impl<T> fmt::Debug for EventCursor<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("EventCursor").field("next", &self.next).finish()
    }
}

impl<T> PartialEq for EventCursor<T> {
    fn eq(&self, other: &Self) -> bool {
        self.next == other.next
    }
}

#[cfg(test)]
#[path = "event_channel.test.rs"]
mod tests;
