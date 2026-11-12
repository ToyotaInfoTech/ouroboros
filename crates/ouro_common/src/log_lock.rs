// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Lock-event tracing utilities.
//!
//! Provides helpers to log acquire/release events of read and write locks
//! at `DEBUG` level, along with the caller's identity derived from the
//! runtime call stack.

use std::any::type_name_of_val;

use crate::{addr, get_caller};

/// Lock direction: read or write.
pub enum LockType {
    Read,
    Write,
}

/// Lock lifecycle phase: attempting, acquired, or released.
pub enum LockAction {
    TryAcquire,
    Acquire,
    Release,
}

struct LockEvent {
    lock_type: LockType,
    action: LockAction,
    caller: String,
    callee: String,
}

impl LockEvent {
    fn new(lock_type: LockType, action: LockAction, caller: String, callee: String) -> Self {
        Self {
            lock_type,
            action,
            caller,
            callee,
        }
    }
}

impl std::fmt::Display for LockEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}{} (by {})",
            self.callee,
            match self.action {
                LockAction::TryAcquire => "?",
                LockAction::Acquire => "+",
                LockAction::Release => "-",
            },
            match self.lock_type {
                LockType::Read => "RLock",
                LockType::Write => "WLock",
            },
            self.caller,
        )
    }
}

/// Emits a `DEBUG`-level trace event describing a lock state change.
///
/// # Arguments
/// * `obj` - The object being locked (used for its type name and address)
/// * `lock_type` - Whether the lock is a read or write lock
/// * `lock_action` - Whether the lock is being tried, acquired, or released
pub fn log_lock_status<T>(obj: &T, lock_type: LockType, lock_action: LockAction)
where
    T: Sized,
{
    tracing::debug!(
        "{}",
        LockEvent::new(
            lock_type,
            lock_action,
            get_caller(type_name_of_val(obj)),
            format!(
                "{}@{:0width$x}",
                type_name_of_val(obj),
                addr(obj),
                width = crate::constant::core::SIZE_OF_USIZE * 2
            )
        )
    );
}
