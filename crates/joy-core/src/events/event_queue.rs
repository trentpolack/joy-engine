// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Events of any number of types, each kept in send order with its sequence number.

use std::any::{
    Any,
    TypeId,
};
use std::fmt;

use indexmap::IndexMap;

/// Position of an event in everything one channel (or standalone queue) accepted: the first event is zero, and each
/// event sent after it, of any type, is one higher.
pub type EventSequence = u64;

/// Events of any number of types, each kept in send order.
///
/// A standalone queue numbers its own events; inside an [`EventChannel`](super::EventChannel) the channel numbers them,
/// so sequences stay unique across the channel's updates. Event types are plain `Clone` records, so a queue (and any
/// simulation state that owns one) can be cloned.
#[derive(Default)]
pub struct EventQueue {
    // One `TypedEvents<T>` per event type, in the order types were first sent, so iteration is deterministic.
    queues: IndexMap<TypeId, Box<dyn ErasedEvents>>,

    // The sequence the next standalone `send` assigns.
    next_sequence: EventSequence,
}

impl EventQueue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an event of type `T`.
    pub fn send<T: Clone + 'static>(&mut self, event: T) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.push(sequence, event);
    }

    /// Every event of type `T`, in send order.
    pub fn read<T: 'static>(&self) -> &[T] {
        self.typed::<T>().map_or(&[], |typed| typed.events.as_slice())
    }

    /// Total events of every type.
    pub fn len(&self) -> usize {
        self.queues.values().map(|queue| queue.len()).sum()
    }

    /// Whether no events are queued.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove every event, keeping each type's allocation for reuse. Sequences continue from where they were.
    pub fn clear(&mut self) {
        for queue in self.queues.values_mut() {
            queue.clear();
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Channel Internals
    // -----------------------------------------------------------------------------------------------------------------

    /// Append `event` under a sequence the owning channel assigned.
    pub(super) fn push<T: Clone + 'static>(&mut self, sequence: EventSequence, event: T) {
        let queue = self.queues.entry(TypeId::of::<T>()).or_insert_with(|| Box::new(TypedEvents::<T>::default()));
        if let Some(typed) = queue.as_any_mut().downcast_mut::<TypedEvents<T>>() {
            typed.sequences.push(sequence);
            typed.events.push(event);
        }
    }

    /// Events of type `T` whose sequence is at least `first`, in send order.
    pub(super) fn read_from<T: 'static>(&self, first: EventSequence) -> &[T] {
        let Some(typed) = self.typed::<T>() else {
            return &[];
        };
        let start = typed.sequences.partition_point(|sequence| *sequence < first);
        &typed.events[start..]
    }

    fn typed<T: 'static>(&self) -> Option<&TypedEvents<T>> {
        self.queues.get(&TypeId::of::<T>()).and_then(|queue| queue.as_any().downcast_ref::<TypedEvents<T>>())
    }
}

impl Clone for EventQueue {
    fn clone(&self) -> Self {
        Self {
            queues: self.queues.iter().map(|(type_id, queue)| (*type_id, queue.clone_boxed())).collect(),
            next_sequence: self.next_sequence,
        }
    }
}

impl fmt::Debug for EventQueue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("EventQueue").field("events", &self.len()).field("types", &self.queues.len()).finish()
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Type Erasure
// ---------------------------------------------------------------------------------------------------------------------

/// One event type's events and their sequences, in send order (so sequences only increase).
#[derive(Clone)]
struct TypedEvents<T> {
    sequences: Vec<EventSequence>,
    events: Vec<T>,
}

impl<T> Default for TypedEvents<T> {
    fn default() -> Self {
        Self {
            sequences: Vec::new(),
            events: Vec::new(),
        }
    }
}

/// A `TypedEvents<T>` whose element type is known only to its sender and readers.
trait ErasedEvents: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn len(&self) -> usize;
    fn clear(&mut self);
    fn clone_boxed(&self) -> Box<dyn ErasedEvents>;
}

impl<T: Clone + 'static> ErasedEvents for TypedEvents<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn len(&self) -> usize {
        self.events.len()
    }

    fn clear(&mut self) {
        self.sequences.clear();
        self.events.clear();
    }

    fn clone_boxed(&self) -> Box<dyn ErasedEvents> {
        Box::new(self.clone())
    }
}
