// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Stable best-candidate selection for utility-scored AI decisions, target picking, and nearest queries.
//!
//! A [`BestChoice`] keeps the first candidate whose score strictly improves on everything offered before it, so equal
//! scores resolve to enumeration order and seeded decisions replay identically. An optional starting bound rejects
//! candidates that do not beat it (a range limit for nearest queries, or a minimum worth for a utility action).

/// Whether lower or higher scores win.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScoreOrder {
    Lowest,
    Highest,
}

/// The best candidate offered so far and its score.
#[derive(Clone, Debug, PartialEq)]
pub struct BestChoice<T> {
    order: ScoreOrder,
    // The score a candidate must strictly beat: the starting bound, then the best score offered.
    threshold: f64,
    best: Option<T>,
}

impl<T> BestChoice<T> {
    /// Lowest score wins, with no bound (costs, distances, angles).
    pub fn lowest() -> Self {
        Self::lowest_below(f64::INFINITY)
    }

    /// Lowest score wins; only scores strictly below `bound` are accepted (for example a range limit).
    pub fn lowest_below(bound: f64) -> Self {
        Self {
            order: ScoreOrder::Lowest,
            threshold: bound,
            best: None,
        }
    }

    /// Highest score wins, with no bound (utilities).
    pub fn highest() -> Self {
        Self::highest_above(f64::NEG_INFINITY)
    }

    /// Highest score wins; only scores strictly above `bound` are accepted (for example a minimum worth).
    pub fn highest_above(bound: f64) -> Self {
        Self {
            order: ScoreOrder::Highest,
            threshold: bound,
            best: None,
        }
    }

    /// Offer `candidate` with `score`; it becomes the best when the score strictly improves on the threshold. Returns
    /// whether it was taken. NaN scores are never taken.
    pub fn offer(&mut self, candidate: T, score: f64) -> bool {
        self.offer_if(candidate, score, || true)
    }

    /// Offer `candidate` with `score`, running `accept` only when the score would improve, so costly checks (line of
    /// sight, path queries) run for contenders alone. Returns whether it was taken.
    pub fn offer_if(&mut self, candidate: T, score: f64, accept: impl FnOnce() -> bool) -> bool {
        let improves = match self.order {
            ScoreOrder::Lowest => score < self.threshold,
            ScoreOrder::Highest => score > self.threshold,
        };
        if !improves || !accept() {
            return false;
        }
        self.threshold = score;
        self.best = Some(candidate);
        true
    }

    /// The best candidate, if any was taken.
    pub fn best(&self) -> Option<&T> {
        self.best.as_ref()
    }

    /// The best score taken, or the starting bound when nothing was taken.
    pub fn score(&self) -> f64 {
        self.threshold
    }

    /// Consume the choice, returning the best candidate.
    pub fn into_best(self) -> Option<T> {
        self.best
    }
}

#[cfg(test)]
#[path = "choice.test.rs"]
mod tests;
