// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Undo and redo for property edits, separate from script text history.
//!
//! The history stores whole property snapshots chosen by the tool (FORM parameters, an effect's parameters and emitter
//! settings without scripts or texture bytes) and never inspects them. It keeps a baseline, the snapshot at the last
//! commit point, so continuous gestures coalesce: a slider drag or a number scrub reports
//! [`EditPhase::Continuing`](crate::EditPhase) while it moves and is recorded once, from the pre-gesture
//! baseline to the released value, when it reports `Committed`. Discrete actions (a reset button, a picker choice, an
//! arrow-key step, an exact entry) commit immediately.
//!
//! A newer edit the history did not record (a structural change, an outside restore) invalidates undo: undoing first
//! checks that the current snapshot is the last entry's result and clears the history otherwise, rather than
//! restoring over unrelated work.

/// Most entries kept; older ones are dropped.
pub const PROPERTY_HISTORY_LIMIT: usize = 30;

/// One recorded edit.
#[derive(Clone, Debug, PartialEq)]
struct HistoryEntry<Snapshot> {
    before: Snapshot,
    after: Snapshot,
}

/// Property undo/redo over tool-defined snapshots.
#[derive(Clone, Debug)]
pub struct PropertyHistory<Snapshot> {
    baseline: Option<Snapshot>,
    past: Vec<HistoryEntry<Snapshot>>,
    future: Vec<HistoryEntry<Snapshot>>,
    limit: usize,
}

impl<Snapshot> Default for PropertyHistory<Snapshot> {
    fn default() -> Self {
        Self { baseline: None, past: Vec::new(), future: Vec::new(), limit: PROPERTY_HISTORY_LIMIT }
    }
}

impl<Snapshot: Clone + PartialEq> PropertyHistory<Snapshot> {
    /// An empty history whose baseline is `snapshot` (the document as opened).
    pub fn new(snapshot: Snapshot) -> Self {
        Self { baseline: Some(snapshot), ..Self::default() }
    }

    /// Keep at most `limit` entries (at least one).
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    /// Forget every entry and start again from `snapshot`, for a newly opened document or an edit that property undo
    /// must not cross (TypeScript clears property history when a script changes).
    pub fn reset(&mut self, snapshot: Snapshot) {
        self.baseline = Some(snapshot);
        self.past.clear();
        self.future.clear();
    }

    /// Move the baseline to `snapshot` without recording an entry, after an edit that is not undoable as a property
    /// (adding or deleting a parameter or emitter). Earlier entries stay, but undo clears them because the current
    /// snapshot no longer matches their result.
    pub fn rebase(&mut self, snapshot: Snapshot) {
        self.baseline = Some(snapshot);
    }

    /// Record the change from the baseline to `snapshot`, if any, as one entry, and make `snapshot` the baseline.
    /// Returns whether an entry was recorded. Call it when a control reports
    /// [`EditPhase::Committed`](crate::EditPhase) or after a discrete property action.
    pub fn commit(&mut self, snapshot: Snapshot) -> bool {
        let before = self.baseline.replace(snapshot.clone());
        match before {
            Some(before) if before != snapshot => {
                self.past.push(HistoryEntry { before, after: snapshot });
                if self.past.len() > self.limit {
                    let excess = self.past.len() - self.limit;
                    self.past.drain(..excess);
                }
                self.future.clear();
                true
            }
            _ => false,
        }
    }

    /// The snapshot to restore when a continuous gesture is cancelled (Escape during a drag): the baseline, which the
    /// gesture has not committed over.
    pub fn baseline(&self) -> Option<&Snapshot> {
        self.baseline.as_ref()
    }

    /// Step back: returns the snapshot to restore, or `None` when there is nothing to undo or `current` is not the
    /// last entry's result (the history is then cleared). The restored snapshot becomes the baseline.
    pub fn undo(&mut self, current: &Snapshot) -> Option<Snapshot> {
        let entry = self.past.last()?;
        if &entry.after != current {
            self.reset(current.clone());
            return None;
        }
        let entry = self.past.pop().expect("checked above");
        let restored = entry.before.clone();
        self.future.push(entry);
        self.baseline = Some(restored.clone());
        Some(restored)
    }

    /// Step forward again: the counterpart of [`undo`](Self::undo).
    pub fn redo(&mut self, current: &Snapshot) -> Option<Snapshot> {
        let entry = self.future.last()?;
        if &entry.before != current {
            self.reset(current.clone());
            return None;
        }
        let entry = self.future.pop().expect("checked above");
        let restored = entry.after.clone();
        self.past.push(entry);
        self.baseline = Some(restored.clone());
        Some(restored)
    }

    /// Whether [`undo`](Self::undo) has an entry.
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    /// Whether [`redo`](Self::redo) has an entry.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Number of entries that can be undone.
    pub fn undo_count(&self) -> usize {
        self.past.len()
    }
}

#[cfg(test)]
#[path = "property_history.test.rs"]
mod tests;
