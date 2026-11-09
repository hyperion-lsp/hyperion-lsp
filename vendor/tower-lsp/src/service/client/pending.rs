// Modified from tower-lsp 0.20.0 for Hyperion; see MODIFICATIONS.md.

//! Types for tracking server-to-client JSON-RPC requests.

use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use dashmap::{mapref::entry::Entry, DashMap};
use futures::channel::oneshot;
use tracing::warn;

use crate::jsonrpc::{Id, Response};

/// A hashmap containing pending client requests, keyed by request ID.
pub struct Pending(DashMap<Id, Vec<oneshot::Sender<Response>>>);

pub(super) struct ResponseWaiter {
    receiver: oneshot::Receiver<Response>,
    pending: Arc<Pending>,
    id: Id,
}

impl Future for ResponseWaiter {
    type Output = Response;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Response> {
        Pin::new(&mut self.receiver)
            .poll(cx)
            .map(|response| response.expect("sender already dropped"))
    }
}

impl Drop for ResponseWaiter {
    fn drop(&mut self) {
        // Close before inspecting senders so cancellation removes this waiter,
        // even if the client never replies. Other waiters keep their FIFO order.
        self.receiver.close();
        if let Entry::Occupied(mut entry) = self.pending.0.entry(self.id.clone()) {
            entry.get_mut().retain(|sender| !sender.is_canceled());
            if entry.get().is_empty() {
                entry.remove();
            }
        }
    }
}

impl Pending {
    /// Creates a new pending client requests map.
    pub fn new() -> Self {
        Pending(DashMap::new())
    }

    /// Inserts the given response into the map.
    ///
    /// The corresponding `.wait()` future will then resolve to the given value.
    pub fn insert(&self, r: Response) {
        match r.id() {
            Id::Null => warn!("received response with request ID of `null`, ignoring"),
            id => match self.0.entry(id.clone()) {
                Entry::Vacant(_) => warn!("received response with unknown request ID: {}", id),
                Entry::Occupied(mut entry) => {
                    let tx = match entry.get().len() {
                        1 => entry.remove().remove(0),
                        _ => entry.get_mut().remove(0),
                    };

                    if tx.send(r).is_err() {
                        warn!("received response for a cancelled client request");
                    }
                }
            },
        }
    }

    /// Marks the given request ID as pending and waits for its corresponding response to arrive.
    ///
    /// If the same request ID is being waited upon in multiple locations, then the incoming
    /// response will be routed to one of the callers in a first come, first served basis. To
    /// ensure correct routing of JSON-RPC requests, each identifier value used _must_ be unique.
    pub(super) fn wait(self: &Arc<Self>, id: Id) -> ResponseWaiter {
        let (tx, rx) = oneshot::channel();

        match self.0.entry(id.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(vec![tx]);
            }
            Entry::Occupied(mut entry) => {
                let txs = entry.get_mut();
                txs.reserve(1); // We assume concurrent waits are rare, so reserve one by one.
                txs.push(tx);
            }
        }

        ResponseWaiter {
            receiver: rx,
            pending: Arc::clone(self),
            id,
        }
    }
}

impl Debug for Pending {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        #[allow(dead_code)]
        #[derive(Debug)]
        struct Waiters(usize);

        let iter = self
            .0
            .iter()
            .map(|e| (e.key().clone(), Waiters(e.value().len())));

        f.debug_map().entries(iter).finish()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn waits_for_client_response() {
        let pending = Arc::new(Pending::new());

        let id = Id::Number(1);
        let wait_fut = pending.wait(id.clone());

        let response = Response::from_ok(id, json!({}));
        pending.insert(response.clone());

        assert_eq!(wait_fut.await, response);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ignores_late_response_to_cancelled_request() {
        let pending = Arc::new(Pending::new());
        let id = Id::Number(1);
        let waiter = pending.wait(id.clone());
        let next_id = Id::Number(2);
        let next = pending.wait(next_id.clone());
        drop(waiter);
        assert!(!pending.0.contains_key(&id));
        pending.insert(Response::from_ok(id, json!(null)));
        let response = Response::from_ok(next_id, json!(null));
        pending.insert(response.clone());
        assert_eq!(next.await, response);
        assert!(pending.0.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_preserves_other_waiters_in_fifo_order() {
        let pending = Arc::new(Pending::new());
        let id = Id::Number(1);
        let first = pending.wait(id.clone());
        let cancelled = pending.wait(id.clone());
        let last = pending.wait(id.clone());
        drop(cancelled);

        let foo = Response::from_ok(id.clone(), json!("foo"));
        let bar = Response::from_ok(id, json!("bar"));
        pending.insert(foo.clone());
        pending.insert(bar.clone());
        assert_eq!(first.await, foo);
        assert_eq!(last.await, bar);
        assert!(pending.0.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn routes_responses_in_fifo_order() {
        let pending = Arc::new(Pending::new());

        let id = Id::Number(1);
        let wait_fut1 = pending.wait(id.clone());
        let wait_fut2 = pending.wait(id.clone());

        let foo = Response::from_ok(id.clone(), json!("foo"));
        let bar = Response::from_ok(id, json!("bar"));
        pending.insert(bar.clone());
        pending.insert(foo.clone());

        assert_eq!(wait_fut1.await, bar);
        assert_eq!(wait_fut2.await, foo);
    }
}
