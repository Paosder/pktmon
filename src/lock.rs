//! Lock access that never panics.
//!
//! A lock turns poisoned only after a thread panicked while holding it. The crate's critical
//! sections only set flags and swap handles, so the data behind a poisoned lock is still valid.
//! Callbacks that Windows invokes cannot unwind, so a panic there ends the whole process.

use std::sync::{
    Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
};

pub(crate) fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn mutex<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn poison_rwlock() -> Arc<RwLock<u32>> {
        let lock = Arc::new(RwLock::new(7));
        let held = lock.clone();
        let panicked = std::thread::spawn(move || {
            let _guard = held.write().expect("not yet poisoned");
            panic!("poison the lock on purpose");
        })
        .join();
        assert!(panicked.is_err());
        assert!(lock.is_poisoned());
        lock
    }

    #[test]
    fn a_poisoned_rwlock_still_reads_and_writes() {
        let lock = poison_rwlock();
        assert_eq!(*read(&lock), 7);
        *write(&lock) = 9;
        assert_eq!(*read(&lock), 9);
    }

    #[test]
    fn a_poisoned_mutex_still_locks() {
        let lock = Arc::new(Mutex::new(Some(3)));
        let held = lock.clone();
        let panicked = std::thread::spawn(move || {
            let _guard = held.lock().expect("not yet poisoned");
            panic!("poison the mutex on purpose");
        })
        .join();
        assert!(panicked.is_err());
        assert!(lock.is_poisoned());
        assert_eq!(mutex(&lock).take(), Some(3));
        assert_eq!(*mutex(&lock), None);
    }

    #[test]
    fn healthy_locks_behave_as_before() {
        let lock = RwLock::new(1);
        *write(&lock) += 1;
        assert_eq!(*read(&lock), 2);
        let mutex_lock = Mutex::new(5);
        *mutex(&mutex_lock) += 1;
        assert_eq!(*mutex(&mutex_lock), 6);
    }
}
