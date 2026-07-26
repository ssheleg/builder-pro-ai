//! Generic client-fan-out registry (S3 phase 1, spec §3), extracted from
//! `bpa-sessiond::socket_server`'s `Broadcaster` (`socket_server.rs:211-231`) so a second daemon
//! (`bpa-orchd`) can reuse the exact same non-blocking fan-out rules over its own frame type.
//! Registry of every connected client's outbound queue, so supervisor/dispatch callbacks can fan
//! a single value out to all of them. Each client registers on connect and deregisters on
//! disconnect. Sends use `try_send`; a full/closed queue is silently skipped (the owning
//! client's own task independently detects the overflow on its own path and tears itself down)
//! so one dead client never blocks the fan-out to the others.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bpa_protocol::sync::lock;
use tokio::sync::mpsc;

/// Generic over the value type `F` fanned out to every registered client (sessiond re-seats as
/// `Broadcaster<Frame>`; a future `bpa-orchd` uses its own frame enum).
pub struct Broadcaster<F: Clone + Send + 'static> {
    inner: Arc<Mutex<HashMap<u64, mpsc::Sender<F>>>>,
}

/// Hand-written (NOT `#[derive(Clone, Default)]`): the derive macros would add spurious
/// `F: Clone`/`F: Default` bounds on the IMPL beyond what the struct's own `F: Clone + Send +
/// 'static` bound already requires — in `Default`'s case that would wrongly force every
/// instantiation's `F` (e.g. `Frame`, which has no `Default` impl) to implement `Default` just
/// to construct an empty registry, even though the `HashMap` field needs no such bound.
impl<F: Clone + Send + 'static> Clone for Broadcaster<F> {
    fn clone(&self) -> Self {
        Broadcaster {
            inner: self.inner.clone(),
        }
    }
}

impl<F: Clone + Send + 'static> Default for Broadcaster<F> {
    fn default() -> Self {
        Broadcaster {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<F: Clone + Send + 'static> Broadcaster<F> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `id`'s outbound queue for fan-out. Replaces any existing registration under the
    /// same `id`.
    pub fn register(&self, id: u64, tx: mpsc::Sender<F>) {
        lock(&self.inner).insert(id, tx);
    }

    /// Remove `id`'s registration (no-op if it was never registered, or already removed).
    pub fn deregister(&self, id: u64) {
        lock(&self.inner).remove(&id);
    }

    /// Enqueue `f` into every registered client's outbound queue (best-effort, non-blocking). A
    /// full queue (`TrySendError::Full`, a client that stopped reading) or a closed one
    /// (`TrySendError::Closed`, a client already torn down) is silently skipped — this never
    /// blocks, and one dead/slow client never delays or drops the fan-out to the others.
    pub fn broadcast(&self, f: F) {
        let map = lock(&self.inner);
        for tx in map.values() {
            let _ = tx.try_send(f.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    #[derive(Clone, Debug, PartialEq)]
    struct Msg(u32);

    #[tokio::test]
    async fn two_registered_receivers_both_get_the_broadcast_value() {
        let b: super::Broadcaster<Msg> = super::Broadcaster::new();
        let (tx1, mut rx1) = mpsc::channel(4);
        let (tx2, mut rx2) = mpsc::channel(4);
        b.register(1, tx1);
        b.register(2, tx2);
        b.broadcast(Msg(42));
        assert_eq!(rx1.recv().await, Some(Msg(42)));
        assert_eq!(rx2.recv().await, Some(Msg(42)));
    }

    #[tokio::test]
    async fn full_receiver_queue_is_skipped_without_blocking_other_receivers() {
        let b: super::Broadcaster<Msg> = super::Broadcaster::default();
        let (tx_full, mut rx_full) = mpsc::channel::<Msg>(1);
        // Pre-fill the single slot so a further `try_send` on this sender returns `Full`.
        tx_full.try_send(Msg(0)).unwrap();
        let (tx_ok, mut rx_ok) = mpsc::channel::<Msg>(4);
        b.register(1, tx_full);
        b.register(2, tx_ok);

        // Must not block/panic even though receiver 1's queue is already full.
        b.broadcast(Msg(42));

        // Receiver 2 (not full) gets the broadcast value.
        assert_eq!(rx_ok.recv().await, Some(Msg(42)));
        // Receiver 1 only ever has the pre-fill value; the broadcast was silently dropped, not
        // queued behind it.
        assert_eq!(rx_full.try_recv(), Ok(Msg(0)));
        assert!(
            rx_full.try_recv().is_err(),
            "broadcast must have been skipped for the full receiver, not buffered"
        );
    }

    #[tokio::test]
    async fn deregistered_receiver_gets_nothing() {
        let b: super::Broadcaster<Msg> = super::Broadcaster::new();
        let (tx, mut rx) = mpsc::channel::<Msg>(4);
        b.register(1, tx);
        b.deregister(1);
        b.broadcast(Msg(7));
        assert!(
            matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Disconnected)),
            "deregistering must drop the sender so the receiver sees it disconnected, not just empty"
        );
    }

    #[tokio::test]
    async fn clone_shares_the_same_registry() {
        let b1: super::Broadcaster<Msg> = super::Broadcaster::new();
        let b2 = b1.clone();
        let (tx, mut rx) = mpsc::channel::<Msg>(4);
        b1.register(1, tx);
        // Registered via b1's clone; broadcasting through b2 must still reach it.
        b2.broadcast(Msg(9));
        assert_eq!(rx.recv().await, Some(Msg(9)));
    }
}
