// Added for Hyperion; see MODIFICATIONS.md.

//! Cooperative cancellation for client-to-server requests.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::jsonrpc::{Error, Result};

#[cfg(feature = "runtime-tokio")]
tokio::task_local! {
    static CURRENT_REQUEST_TOKEN: CancellationToken;
}

/// A cheaply cloneable flag that is cancelled when the client sends `$/cancelRequest`.
///
/// Request handlers can obtain the token for the request currently being polled with
/// [`CancellationToken::current`]. CPU-heavy handlers should check it at natural loop
/// boundaries so cancellation can release their worker promptly.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn is_same_request(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cancelled, &other.cancelled)
    }

    /// Returns the token associated with the request currently being handled.
    ///
    /// Outside a request handler this returns an independent token that is never
    /// cancelled, which makes the token safe to use from shared provider code.
    #[must_use]
    pub fn current() -> Self {
        #[cfg(feature = "runtime-tokio")]
        {
            CURRENT_REQUEST_TOKEN
                .try_with(Clone::clone)
                .unwrap_or_default()
        }

        #[cfg(not(feature = "runtime-tokio"))]
        {
            Self::default()
        }
    }

    /// Returns whether the client has cancelled this request.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Returns the LSP `RequestCancelled` error if cancellation was requested.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::request_cancelled())
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "runtime-tokio")]
    pub(crate) async fn scope<F: std::future::Future>(self, future: F) -> F::Output {
        CURRENT_REQUEST_TOKEN.scope(self, future).await
    }

    #[cfg(not(feature = "runtime-tokio"))]
    pub(crate) async fn scope<F: std::future::Future>(self, future: F) -> F::Output {
        future.await
    }
}
