//! Loom backend for testing lock progress in the actual `NotifyRwLock` algorithm.
//!
//! `atomic_wait` uses OS blocking, which Loom cannot observe. A mutex makes the
//! expected-value check and Condvar registration atomic with respect to wakeups.
//! The model permits no spurious wakeups: progress must not depend on them.
//! This backend adds ordering through the mutex, so it is not an exhaustive
//! model of weak-memory behavior. The payload and application notification
//! channel are not modeled; the progress tests use `()` and never receive.

use ::loom::sync::atomic::AtomicU32 as LoomAtomicU32;
pub(super) use ::loom::sync::atomic::Ordering;
use ::loom::sync::{Condvar, Mutex};
use std::ops::Deref;

pub(super) struct AtomicU32 {
    value: LoomAtomicU32,
    wait_lock: Mutex<()>,
    wake: Condvar,
}

impl AtomicU32 {
    pub(super) fn new(value: u32) -> Self {
        Self {
            value: LoomAtomicU32::new(value),
            wait_lock: Mutex::new(()),
            wake: Condvar::new(),
        }
    }
}

impl Deref for AtomicU32 {
    type Target = LoomAtomicU32;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub(super) fn wait(atomic: &AtomicU32, expected: u32) {
    let guard = atomic.wait_lock.lock().unwrap();
    if atomic.load(Ordering::Relaxed) == expected {
        drop(atomic.wake.wait(guard).unwrap());
    }
}

pub(super) fn wake_one(atomic: &AtomicU32) {
    let _guard = atomic.wait_lock.lock().unwrap();
    atomic.wake.notify_one();
}

pub(super) fn wake_all(atomic: &AtomicU32) {
    let _guard = atomic.wait_lock.lock().unwrap();
    atomic.wake.notify_all();
}

#[cfg(test)]
mod tests {
    use super::super::NotifyRwLock;
    use ::loom::{model, thread};
    // Arc only owns the lock; its reference count is not part of the protocol.
    // Using std also avoids Loom Arc's secondary panic while unwinding a deadlock.
    use std::sync::{Arc, mpsc::sync_channel};

    #[test]
    fn waiting_writer_completes_after_last_reader() {
        model(|| {
            let (tx, _rx) = sync_channel(1);
            let lock = Arc::new(NotifyRwLock::new(tx, ()));
            let reader = lock.read();
            let writer = thread::spawn({
                let lock = lock.clone();
                move || drop(lock.write())
            });

            drop(reader);
            writer.join().unwrap();
        });
    }

    #[test]
    fn waiting_writer_completes_after_writer_is_replaced_by_reader() {
        model(|| {
            let (tx, _rx) = sync_channel(1);
            let lock = Arc::new(NotifyRwLock::new(tx, ()));
            let first_writer = lock.write();
            let waiting_writer = thread::spawn({
                let lock = lock.clone();
                move || drop(lock.write())
            });

            // Loom can pause the waiting writer after it observes WRITE_LOCK_STATE
            // but before it loads writer_wake_counter. The first writer then
            // unlocks, clearing the pending bit, and a reader acquires the lock.
            // The waiting writer must arrange a future wakeup before sleeping.
            drop(first_writer);
            let reader = lock.read();
            drop(reader);
            waiting_writer.join().unwrap();
        });
    }
}
