use atomic_wait::{wait, wake_all, wake_one};
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU32, Ordering};

const WRITE_LOCK_STATE: u32 = u32::MAX;
const READ_LOCK_STEP: u32 = 2;

pub type NotifySender = std::sync::mpsc::SyncSender<()>;

pub struct NotifyRwLock<T> {
    //2刻みでカウントアップされていく、リードロックのカウント
    // 奇数の場合は、ライトロックが待っていることを指す
    state: AtomicU32,
    writer_wake_counter: AtomicU32,
    value: UnsafeCell<T>,
    notify_tx: NotifySender,
}

impl<T> NotifyRwLock<T> {
    pub fn new(notify_tx: NotifySender, value: T) -> Self {
        Self {
            state: AtomicU32::new(0),
            writer_wake_counter: AtomicU32::new(0),
            value: UnsafeCell::new(value),
            notify_tx,
        }
    }

    pub fn read(&self) -> ReadGuard<'_, T> {
        let mut s = self.state.load(Ordering::Relaxed);

        loop {
            if s % 2 == 0 {
                assert!(s < u32::MAX - 2, "too many readers");
                match self.state.compare_exchange_weak(
                    s,
                    s + READ_LOCK_STEP,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return ReadGuard { rwlock: self },
                    Err(e) => {
                        s = e;
                    }
                }
            }
            if s % 2 == 1 {
                wait(&self.state, s);
                s = self.state.load(Ordering::Relaxed);
            }
        }
    }

    pub fn write(&self) -> WriteGuard<'_, T> {
        let mut s = self.state.load(Ordering::Relaxed);

        loop {
            if s <= 1 {
                match self.state.compare_exchange(
                    s,
                    WRITE_LOCK_STATE,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        return WriteGuard { rwlock: self };
                    }
                    Err(e) => {
                        s = e;
                        continue;
                    }
                }
            }
            if s % 2 == 0 {
                if let Err(e) =
                    self.state
                        .compare_exchange(s, s + 1, Ordering::Relaxed, Ordering::Relaxed)
                {
                    s = e;
                    continue;
                }
            }
            let w = self.writer_wake_counter.load(Ordering::Acquire);
            s = self.state.load(Ordering::Relaxed);
            if s >= READ_LOCK_STEP {
                wait(&self.writer_wake_counter, w);
                s = self.state.load(Ordering::Relaxed);
            }
        }
    }
}

unsafe impl<T: Send + Sync> Sync for NotifyRwLock<T> {}

pub struct ReadGuard<'a, T> {
    rwlock: &'a NotifyRwLock<T>,
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        if self
            .rwlock
            .state
            .fetch_sub(READ_LOCK_STEP, Ordering::Release)
            == 3
        {
            self.rwlock
                .writer_wake_counter
                .fetch_add(1, Ordering::Release);
            wake_one(&self.rwlock.writer_wake_counter);
        }
    }
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.rwlock.value.get() }
    }
}

pub struct WriteGuard<'a, T> {
    rwlock: &'a NotifyRwLock<T>,
}

impl<T> Drop for WriteGuard<'_, T> {
    fn drop(&mut self) {
        self.rwlock.state.store(0, Ordering::Release);
        self.rwlock
            .writer_wake_counter
            .fetch_add(1, Ordering::Release);
        wake_one(&self.rwlock.writer_wake_counter);
        wake_all(&self.rwlock.state);
        let _ = self.rwlock.notify_tx.try_send(()); // 通知が一杯で送れない場合は、エラーを無視する
    }
}

impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.rwlock.value.get() }
    }
}

impl<T> DerefMut for WriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.rwlock.value.get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    #[test]
    fn add_list() {
        let (tx, _rx) = std::sync::mpsc::sync_channel(1000);
        let waiter_list = NotifyRwLock::new(tx, Vec::new());
        black_box(&waiter_list);

        let start = Instant::now();
        std::thread::scope(|s| {
            let t1 = s.spawn({
                || {
                    for i in 0..1000 {
                        let mut c = waiter_list.write();
                        black_box(&c);
                        c.push(i);
                    }
                }
            });
            let t2 = s.spawn({
                || {
                    for i in 0..1000 {
                        let mut c = waiter_list.write();
                        black_box(&c);
                        c.push(i);
                    }
                }
            });
            let t3 = s.spawn({
                || {
                    for i in 0..1000 {
                        let c = waiter_list.read();
                        black_box(&c);
                    }
                }
            });
            t1.join().unwrap();
            t2.join().unwrap();
            t3.join().unwrap();
        });
        assert_eq!(waiter_list.read().len(), 2_000);
    }
}
