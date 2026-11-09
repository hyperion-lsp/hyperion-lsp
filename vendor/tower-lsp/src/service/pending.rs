// Modified from tower-lsp 0.20.0 for Hyperion; see MODIFICATIONS.md.

//! Types for tracking cancelable client-to-server JSON-RPC requests.

use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::sync::Arc;

use dashmap::{mapref::entry::Entry, DashMap};
use futures::future::{self, Either};
use tracing::{debug, info};

use super::ExitedError;
use crate::jsonrpc::{Error, Id, Response};
use crate::CancellationToken;

/// A hashmap containing pending server requests, keyed by request ID.
pub struct Pending(Arc<DashMap<Id, PendingRequest>>);

struct PendingRequest {
    abort_handle: future::AbortHandle,
    cancellation: CancellationToken,
}

struct RemoveOnDrop {
    requests: Arc<DashMap<Id, PendingRequest>>,
    id: Id,
    cancellation: CancellationToken,
}

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        self.requests.remove_if(&self.id, |_, request| {
            request.cancellation.is_same_request(&self.cancellation)
        });
    }
}

impl Pending {
    /// Creates a new pending server requests map.
    pub fn new() -> Self {
        Pending(Arc::new(DashMap::new()))
    }

    /// Executes the given async request handler, keyed by the given request ID.
    ///
    /// If a cancel request is issued before the future is finished resolving, this will resolve to
    /// a "canceled" error response, and the pending request handler future will be dropped.
    pub fn execute<F>(
        &self,
        id: Id,
        fut: F,
    ) -> impl Future<Output = Result<Option<Response>, ExitedError>> + Send + 'static
    where
        F: Future<Output = Result<Option<Response>, ExitedError>> + Send + 'static,
    {
        if let Entry::Vacant(entry) = self.0.entry(id.clone()) {
            let cancellation = CancellationToken::default();
            let (handler_fut, abort_handle) = future::abortable(cancellation.clone().scope(fut));
            entry.insert(PendingRequest {
                abort_handle,
                cancellation: cancellation.clone(),
            });

            let requests = self.0.clone();
            Either::Left(async move {
                let _remove_on_drop = RemoveOnDrop {
                    requests,
                    id: id.clone(),
                    cancellation,
                };
                let abort_result = handler_fut.await;

                if let Ok(handler_result) = abort_result {
                    handler_result
                } else {
                    Ok(Some(Response::from_error(id, Error::request_cancelled())))
                }
            })
        } else {
            Either::Right(async { Ok(Some(Response::from_error(id, Error::invalid_request()))) })
        }
    }

    /// Attempts to cancel the running request handler corresponding to this ID.
    ///
    /// This will force the future to resolve to a "canceled" error response. If the future has
    /// already completed, this method call will do nothing.
    pub fn cancel(&self, id: &Id) {
        if let Some((_, request)) = self.0.remove(id) {
            request.cancellation.cancel();
            request.abort_handle.abort();
            info!("successfully cancelled request with ID: {}", id);
        } else {
            debug!(
                "client asked to cancel request {}, but no such pending request exists, ignoring",
                id
            );
        }
    }

    /// Cancels all pending request handlers, if any.
    pub fn cancel_all(&self) {
        self.0.retain(|_, request| {
            request.cancellation.cancel();
            request.abort_handle.abort();
            false
        });
    }
}

impl Debug for Pending {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_set()
            .entries(self.0.iter().map(|entry| entry.key().clone()))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn executes_server_request() {
        let pending = Pending::new();

        let id = Id::Number(1);
        let id2 = id.clone();
        let response = pending
            .execute(id.clone(), async {
                Ok(Some(Response::from_ok(id2, json!({}))))
            })
            .await;

        assert_eq!(response, Ok(Some(Response::from_ok(id, json!({})))));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancels_server_request() {
        let pending = Pending::new();

        let id = Id::Number(1);
        let handler_fut = tokio::spawn(pending.execute(id.clone(), future::pending()));

        pending.cancel(&id);

        let res = handler_fut.await.expect("task panicked");
        assert_eq!(
            res,
            Ok(Some(Response::from_error(id, Error::request_cancelled())))
        );
    }
}
