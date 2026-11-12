use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks a mutex even after a thread panicked while holding it. Without this,
/// one panic in a recording thread would make every later command and the
/// interface poll panic until Conch restarts.
pub trait Locked<T> {
    fn locked(&self) -> MutexGuard<'_, T>;
}
impl<T> Locked<T> for Mutex<T> {
    fn locked(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_mutex_poisoned_by_a_panic_still_locks() {
        let mutex = std::sync::Arc::new(Mutex::new(1));
        let held = mutex.clone();
        let _ = std::thread::spawn(move || {
            let _guard = held.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(mutex.is_poisoned());
        *mutex.locked() += 1;
        assert_eq!(*mutex.locked(), 2);
    }
}
