// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Gameplay timers owned by a world: cooldowns, delays, and repeating ticks that fire as events.
//!
//! A timer counts simulation seconds from the update after it starts, so time spent before the request (earlier in the
//! same update) never counts toward it. Due timers fire as [`TimerFired`](super::TimerFired) events at the start of an
//! update, in due order (ties in start order), and a repeating timer fires once per elapsed period. Timers that belong
//! to an entity are cancelled when it is removed.

use std::borrow::Cow;
use std::fmt;

use indexmap::IndexMap;

use super::entity::EntityId;
use super::world_events::{
    TimerFired,
    WorldEvents,
};

/// Shortest accepted timer period in seconds; shorter periods would fire a repeating timer without bound.
pub const MINIMUM_TIMER_SECONDS: f64 = 1.0e-3;

/// Identifier of a started timer, unique for a world's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimerId(u64);

/// How a timer behaves once it fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimerRepeat {
    /// Fire once and end.
    Once,
    /// Fire every period until cancelled.
    Every,
}

/// One running timer.
#[derive(Clone, Debug, PartialEq)]
struct Timer {
    owner: Option<EntityId>,
    key: Cow<'static, str>,
    period_seconds: f64,
    remaining_seconds: f64,
    repeat: TimerRepeat,
}

/// The timers of one world, in start order.
#[derive(Default)]
pub struct WorldTimers {
    timers: IndexMap<TimerId, Timer>,
    next_id: u64,
}

impl WorldTimers {
    /// Start a timer that fires after `seconds` (at least [`MINIMUM_TIMER_SECONDS`]; non-finite values are clamped to
    /// that minimum), optionally owned by an entity.
    pub fn start(
        &mut self,
        owner: Option<EntityId>,
        key: impl Into<Cow<'static, str>>,
        seconds: f64,
        repeat: TimerRepeat,
    ) -> TimerId {
        let key = key.into();
        let period_seconds =
            if seconds.is_finite() { seconds.max(MINIMUM_TIMER_SECONDS) } else { MINIMUM_TIMER_SECONDS };
        self.next_id += 1;
        let id = TimerId(self.next_id);
        self.timers.insert(
            id,
            Timer {
                owner,
                key,
                period_seconds,
                remaining_seconds: period_seconds,
                repeat,
            },
        );
        id
    }

    /// Stop a timer before it fires. Returns whether it was running.
    pub fn cancel(&mut self, id: TimerId) -> bool {
        self.timers.shift_remove(&id).is_some()
    }

    /// Seconds until a running timer next fires, or `None` once it ended or was cancelled.
    pub fn remaining_seconds(&self, id: TimerId) -> Option<f64> {
        self.timers.get(&id).map(|timer| timer.remaining_seconds)
    }

    /// Number of running timers.
    pub fn len(&self) -> usize {
        self.timers.len()
    }

    /// Whether no timer is running.
    pub fn is_empty(&self) -> bool {
        self.timers.is_empty()
    }

    /// Cancel every timer an entity owns.
    pub(crate) fn cancel_owned_by(&mut self, owner: &EntityId) {
        self.timers.retain(|_, timer| timer.owner.as_ref() != Some(owner));
    }

    /// Cancel every timer.
    pub fn clear(&mut self) {
        self.timers.clear();
    }

    /// Advance every timer by `elapsed_seconds` (the update's own elapsed time; the world calls this at the start of an
    /// update, so timers started during an update first count the next one), firing due ones into `events`.
    pub(crate) fn advance(&mut self, elapsed_seconds: f64, events: &mut WorldEvents) {
        let elapsed = if elapsed_seconds.is_finite() { elapsed_seconds.max(0.0) } else { 0.0 };

        // Collect firings as (time into this update, start order) so simultaneous timers fire in a stable order.
        let mut firings: Vec<(f64, usize, TimerId)> = Vec::new();
        let mut ended: Vec<TimerId> = Vec::new();
        for (order, (id, timer)) in self.timers.iter_mut().enumerate() {
            let mut offset = timer.remaining_seconds;
            timer.remaining_seconds -= elapsed;
            while timer.remaining_seconds <= 0.0 {
                firings.push((offset, order, *id));
                if timer.repeat == TimerRepeat::Once {
                    ended.push(*id);
                    break;
                }
                timer.remaining_seconds += timer.period_seconds;
                offset += timer.period_seconds;
            }
        }
        firings.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));

        for (_, _, id) in firings {
            if let Some(timer) = self.timers.get(&id) {
                events.send(TimerFired {
                    timer: id,
                    owner: timer.owner.clone(),
                    key: timer.key.clone(),
                });
            }
        }
        for id in ended {
            self.timers.shift_remove(&id);
        }
    }
}

impl fmt::Debug for WorldTimers {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("WorldTimers").field("running", &self.timers.len()).finish()
    }
}
