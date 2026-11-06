// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Lending host state to script functions for the length of one host call.
//!
//! Registered script functions are `'static` closures, so they cannot borrow a host's world, and `unsafe` lifetime
//! erasure is forbidden here. Instead the host moves the value into a shared slot, runs its hook calls, and takes the
//! value back when they return. Handles that scripts keep (in `this`, for example) find the slot empty outside the
//! call and fail with a script error instead of reading stale or dangling state. Moving a value is a shallow copy, so
//! lending a large owner each step is cheap; hosts lending a field of `self` swap a stand-in into its place.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use rhai::EvalAltResult;
use rhai::Position;

/// The result type of host functions registered on a script engine: a value, or the script error the call reports.
pub type ScriptResult<T> = Result<T, Box<EvalAltResult>>;

/// A shared slot that holds host state while scripts run. Clones share the slot; capture one in each registered
/// function that needs the state. Single-threaded, like the engine.
pub struct ScriptLoan<T> {
    label: &'static str,
    slot: Rc<RefCell<Option<T>>>,
}

impl<T: 'static> ScriptLoan<T> {
    /// An empty loan. `label` names the state in errors ("the district is only available during a hook call").
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            slot: Rc::new(RefCell::new(None)),
        }
    }

    /// Move `value` into the loan while `call` runs, then return it with `call`'s result. A nested `lend` of the same
    /// loan shadows the outer value until it returns. If `call` panics, the value is dropped with the loan.
    pub fn lend<R>(&self, value: T, call: impl FnOnce() -> R) -> (T, R) {
        let outer = self.slot.replace(Some(value));
        let result = call();
        let value = self.slot.replace(outer).expect("a lent value stays in its slot until its loan returns");
        (value, result)
    }

    /// Lend a copy of a small value (a random stream, a counter) and write the script's changes back when `call`
    /// returns. Use [`ScriptLoan::lend`] for large owners, which this would clone.
    pub fn lend_copy<R>(&self, value: &mut T, call: impl FnOnce() -> R) -> R
    where
        T: Clone,
    {
        let (returned, result) = self.lend(value.clone(), call);
        *value = returned;
        result
    }

    /// Whether a value is lent right now.
    pub fn is_lent(&self) -> bool {
        self.slot.try_borrow().is_ok_and(|slot| slot.is_some())
    }

    /// Read the lent value from a registered function; a script error when nothing is lent or it is being written.
    pub fn read<R>(&self, read: impl FnOnce(&T) -> R) -> ScriptResult<R> {
        let slot = self.slot.try_borrow().map_err(|_| self.busy_error())?;
        match slot.as_ref() {
            Some(value) => Ok(read(value)),
            None => Err(self.not_lent_error()),
        }
    }

    /// Change the lent value from a registered function; a script error when nothing is lent or it is in use.
    pub fn write<R>(&self, write: impl FnOnce(&mut T) -> R) -> ScriptResult<R> {
        let mut slot = self.slot.try_borrow_mut().map_err(|_| self.busy_error())?;
        match slot.as_mut() {
            Some(value) => Ok(write(value)),
            None => Err(self.not_lent_error()),
        }
    }

    fn not_lent_error(&self) -> Box<EvalAltResult> {
        let message = format!("the {} is only available during the host call that lends it", self.label);
        Box::new(EvalAltResult::ErrorRuntime(message.into(), Position::NONE))
    }

    fn busy_error(&self) -> Box<EvalAltResult> {
        let message = format!("the {} is already in use by another host function", self.label);
        Box::new(EvalAltResult::ErrorRuntime(message.into(), Position::NONE))
    }
}

impl<T> Clone for ScriptLoan<T> {
    fn clone(&self) -> Self {
        Self {
            label: self.label,
            slot: Rc::clone(&self.slot),
        }
    }
}

impl<T> fmt::Debug for ScriptLoan<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lent = self.slot.try_borrow().map(|slot| slot.is_some()).unwrap_or(true);
        formatter.debug_struct("ScriptLoan").field("label", &self.label).field("lent", &lent).finish()
    }
}
