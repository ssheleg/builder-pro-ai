//! Poison-tolerant acquisition of `std::sync` locks (BL-124, P1-hardening 2026-07-26).
//!
//! A `std::sync::Mutex`/`RwLock` is *poisoned* when a thread panics while holding its guard:
//! every later `lock()`/`read()`/`write()` then returns `Err(PoisonError)`, and the familiar
//! `.lock().unwrap()` panics again — on EVERY caller, FOREVER. In a long-lived daemon that turns
//! one contained panic into a cascading, permanent outage: the audit's two mortal scenarios were
//! (a) a PTY reader thread panicking under the grid lock → every later reader/ticker/snapshot
//! call panics too → the terminal freezes for the rest of the process's life, and (b) the global
//! persistence flusher panicking once → every later flush/lookup panics → persistence is silently
//! off for the whole daemon.
//!
//! Poisoning exists to warn that the guarded state may be *logically* half-updated. It never
//! means the state is memory-unsafe (the guard's `Drop` still ran), and every lock this crate
//! family holds protects plain, self-healing data (maps, sets, counters, buffers, callback
//! vecs) — never a multi-step invariant that a mid-update panic could leave permanently corrupt.
//! Recovering the guard and continuing is therefore strictly better than dying in a cascade:
//! the next full write/refresh repairs whatever partial state was left behind, and the process
//! stays up. This is the project's already-established pattern (`.lock().unwrap_or_else(|e|
//! e.into_inner())`, previously open-coded at `bpa-daemon-core::logging`, `bpa-sessiond::
//! socket_server`, and friends), unified here so every call site shares one documented contract.
//!
//! Lives in `bpa-protocol` (not `bpa-daemon-core`) because the Tauri core must use it too, and
//! src-tauri carries a locked "NO `bpa-daemon-core` dependency" contract (see
//! `src-tauri/src/orchd_client.rs`'s module docs) — `bpa-protocol` is the one crate every side
//! (sessiond, daemon-core, the app) already depends on.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Acquire `m`, recovering the guard if the mutex is poisoned (a thread panicked while holding
/// it earlier). Never panics on poison — see the module docs for the recover-and-continue
/// contract. All other `lock()` failure modes do not exist on `std::sync::Mutex`, so this never
/// panics at all where `.lock().unwrap()` would have.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Acquire `l` for reading, recovering the guard if the lock is poisoned. Same contract as
/// [`lock`]: a panic that once escaped a *writer*'s guard poisons readers too, and refusing to
/// read forever after is the cascade this helper exists to prevent.
pub fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(PoisonError::into_inner)
}

/// Acquire `l` for writing, recovering the guard if the lock is poisoned. Same contract as
/// [`lock`].
pub fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, RwLock};

    /// Poison `m` exactly the way production gets poisoned: a thread takes the guard and panics
    /// while holding it. Joining swallows the panic payload so the test process stays quiet.
    fn poison_mutex<T: Send + 'static>(m: &Arc<Mutex<T>>) {
        let m2 = m.clone();
        let panicked = std::thread::spawn(move || {
            let _guard = m2.lock().unwrap();
            panic!("deliberate test panic under the guard");
        })
        .join();
        assert!(panicked.is_err(), "the poisoning thread must have panicked");
        assert!(m.is_poisoned(), "precondition: the mutex is now poisoned");
    }

    #[test]
    fn lock_recovers_access_after_a_panic_under_the_guard() {
        let m = Arc::new(Mutex::new(41u32));
        poison_mutex(&m);

        // The hardened path: no panic, and the guarded value is fully usable (read AND write) —
        // the process keeps serving instead of dying in a poison cascade.
        {
            let mut guard = super::lock(&m);
            assert_eq!(*guard, 41);
            *guard = 42;
        }
        assert_eq!(*super::lock(&m), 42);
        // A poisoned-then-recovered mutex stays "poisoned" by std's bookkeeping; the point is
        // that `lock` keeps working regardless — lock it a third time to prove no state latch-up.
        drop(super::lock(&m));
    }

    #[test]
    fn read_and_write_recover_after_a_writer_panics_under_the_guard() {
        let l = Arc::new(RwLock::new(vec![1u8, 2, 3]));
        let l2 = l.clone();
        let panicked = std::thread::spawn(move || {
            let _guard = l2.write().unwrap();
            panic!("deliberate test panic under the write guard");
        })
        .join();
        assert!(panicked.is_err());
        assert!(l.is_poisoned(), "precondition: the lock is now poisoned");

        // A panicked WRITER poisons BOTH acquisition kinds; both must recover.
        assert_eq!(super::read(&l).len(), 3);
        super::write(&l).push(4);
        assert_eq!(super::read(&l).as_slice(), &[1, 2, 3, 4]);
    }

    #[test]
    fn lock_on_an_unpoisoned_mutex_behaves_like_plain_lock() {
        let m = Mutex::new(String::from("state"));
        super::lock(&m).push_str("-mutated");
        assert_eq!(super::lock(&m).as_str(), "state-mutated");
    }
}
