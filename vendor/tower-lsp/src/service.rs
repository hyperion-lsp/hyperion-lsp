// Modified from tower-lsp 0.20.0 for Hyperion; see MODIFICATIONS.md.

//! Service abstraction for language servers.

pub use self::client::{progress, Client, ClientSocket, RequestStream, ResponseSink};

pub(crate) use self::pending::Pending;
pub(crate) use self::state::{ServerState, State};

use std::fmt::{self, Debug, Display, Formatter};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::future::{self, BoxFuture, FutureExt};
use serde_json::Value;
use tower::Service;

use crate::jsonrpc::{
    Error, ErrorCode, FromParams, IntoResponse, Method, Request, Response, Router,
};
use crate::LanguageServer;

pub(crate) mod layers;

mod client;
mod pending;
mod state;

/// Error that occurs when attempting to call the language server after it has already exited.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExitedError(());

impl std::error::Error for ExitedError {}

impl Display for ExitedError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str("language server has exited")
    }
}

/// Service abstraction for the Language Server Protocol.
///
/// This service takes an incoming JSON-RPC message as input and produces an outgoing message as
/// output. If the incoming message is a server notification or a client response, then the
/// corresponding response will be `None`.
///
/// This implements [`tower::Service`] in order to remain independent from the underlying transport
/// and to facilitate further abstraction with middleware.
///
/// Pending requests can be canceled by issuing a [`$/cancelRequest`] notification.
///
/// [`$/cancelRequest`]: https://microsoft.github.io/language-server-protocol/specification#cancelRequest
///
/// The service shuts down and stops serving requests after the [`exit`] notification is received.
///
/// [`exit`]: https://microsoft.github.io/language-server-protocol/specification#exit
#[derive(Debug)]
pub struct LspService<S> {
    inner: Router<S, ExitedError>,
    state: Arc<ServerState>,
    pending: Arc<Pending>,
}

impl<S: LanguageServer> LspService<S> {
    /// Creates a new `LspService` with the given server backend, also returning a channel for
    /// server-to-client communication.
    pub fn new<F>(init: F) -> (Self, ClientSocket)
    where
        F: FnOnce(Client) -> S,
    {
        LspService::build(init).finish()
    }

    /// Starts building a new `LspService`.
    ///
    /// Returns an `LspServiceBuilder`, which allows adding custom JSON-RPC methods to the server.
    pub fn build<F>(init: F) -> LspServiceBuilder<S>
    where
        F: FnOnce(Client) -> S,
    {
        let state = Arc::new(ServerState::new());

        let (client, socket) = Client::new(state.clone());
        let inner = Router::new(init(client.clone()));
        let pending = Arc::new(Pending::new());

        LspServiceBuilder {
            inner: crate::generated::register_lsp_methods(
                inner,
                state.clone(),
                pending.clone(),
                client,
            ),
            state,
            pending,
            socket,
        }
    }

    /// Returns a reference to the inner server.
    pub fn inner(&self) -> &S {
        self.inner.inner()
    }
}

impl<S: LanguageServer> Service<Request> for LspService<S> {
    type Response = Option<Response>;
    type Error = ExitedError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.state.get() {
            State::Initializing => Poll::Pending,
            State::Exited => Poll::Ready(Err(ExitedError(()))),
            _ => self.inner.poll_ready(cx),
        }
    }

    fn call(&mut self, req: Request) -> Self::Future {
        if self.state.get() == State::Exited {
            return future::err(ExitedError(())).boxed();
        }

        // Cancellation must bypass the transport's concurrency queue. If every request slot is
        // occupied by CPU-heavy work, waiting to poll the cancel notification would deadlock the
        // very requests it is meant to stop. The generated route still validates/consumes the
        // notification later; this fast path only flips the pending request's token immediately.
        if req.method() == "$/cancelRequest" {
            if let Some(params) = req
                .params()
                .cloned()
                .and_then(|params| serde_json::from_value::<lsp_types::CancelParams>(params).ok())
            {
                self.pending.cancel(&params.id.into());
            }
        }

        let method = req.method().to_owned();
        let id = req.id().cloned();
        let fut = match panic::catch_unwind(AssertUnwindSafe(|| self.inner.call(req))) {
            Ok(fut) => fut,
            Err(payload) => {
                let message = panic_message(payload);
                tracing::error!(
                    request = %method,
                    panic = %message,
                    "LSP request panicked during dispatch"
                );
                return future::ok(internal_error_response(id)).boxed();
            }
        };

        Box::pin(async move {
            #[cfg(feature = "runtime-tokio")]
            let response = match tokio::spawn(fut).await {
                Ok(response) => response?,
                Err(join_error) if join_error.is_panic() => {
                    let message = panic_message(join_error.into_panic());
                    tracing::error!(
                        request = %method,
                        panic = %message,
                        "LSP request handler panicked"
                    );
                    return Ok(internal_error_response(id));
                }
                Err(join_error) => {
                    tracing::error!(
                        request = %method,
                        error = %join_error,
                        "LSP request task was cancelled unexpectedly"
                    );
                    return Ok(internal_error_response(id));
                }
            };

            #[cfg(not(feature = "runtime-tokio"))]
            let response = match AssertUnwindSafe(fut).catch_unwind().await {
                Ok(response) => response?,
                Err(payload) => {
                    let message = panic_message(payload);
                    tracing::error!(
                        request = %method,
                        panic = %message,
                        "LSP request handler panicked"
                    );
                    return Ok(internal_error_response(id));
                }
            };

            match response.as_ref().and_then(|res| res.error()) {
                Some(Error {
                    code: ErrorCode::MethodNotFound,
                    data: Some(Value::String(m)),
                    ..
                }) if m.starts_with("$/") => Ok(None),
                _ => Ok(response),
            }
        })
    }
}

fn internal_error_response(id: Option<crate::jsonrpc::Id>) -> Option<Response> {
    id.map(|id| Response::from_error(id, Error::internal_error()))
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(message) => (*message).to_owned(),
            Err(_) => "non-string panic payload".to_owned(),
        },
    }
}

/// A builder to customize the properties of an `LspService`.
///
/// To construct an `LspServiceBuilder`, refer to [`LspService::build`].
pub struct LspServiceBuilder<S> {
    inner: Router<S, ExitedError>,
    state: Arc<ServerState>,
    pending: Arc<Pending>,
    socket: ClientSocket,
}

impl<S: LanguageServer> LspServiceBuilder<S> {
    /// Defines a custom JSON-RPC request or notification with the given method `name` and handler.
    ///
    /// # Handler varieties
    ///
    /// Fundamentally, any inherent `async fn(&self)` method defined directly on the language
    /// server backend could be considered a valid method handler.
    ///
    /// Handlers may optionally include a single `params` argument. This argument may be of any
    /// type that implements [`Serialize`](serde::Serialize).
    ///
    /// Handlers which return `()` are treated as **notifications**, while those which return
    /// [`jsonrpc::Result<T>`](crate::jsonrpc::Result) are treated as **requests**.
    ///
    /// Similar to the `params` argument, the `T` in the `Result<T>` return values may be of any
    /// type which implements [`DeserializeOwned`](serde::de::DeserializeOwned). Additionally, this
    /// type _must_ be convertible into a [`serde_json::Value`] using [`serde_json::to_value`]. If
    /// this latter constraint is not met, the client will receive a JSON-RPC error response with
    /// code `-32603` (Internal Error) instead of the expected response.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serde_json::{json, Value};
    /// use tower_lsp::jsonrpc::Result;
    /// use tower_lsp::lsp_types::*;
    /// use tower_lsp::{LanguageServer, LspService};
    ///
    /// struct Mock;
    ///
    /// // Implementation of `LanguageServer` omitted...
    /// # #[tower_lsp::async_trait]
    /// # impl LanguageServer for Mock {
    /// #     async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
    /// #         Ok(InitializeResult::default())
    /// #     }
    /// #
    /// #     async fn shutdown(&self) -> Result<()> {
    /// #         Ok(())
    /// #     }
    /// # }
    ///
    /// impl Mock {
    ///     async fn request(&self) -> Result<i32> {
    ///         Ok(123)
    ///     }
    ///
    ///     async fn request_params(&self, params: Vec<String>) -> Result<Value> {
    ///         Ok(json!({"num_elems":params.len()}))
    ///     }
    ///
    ///     async fn notification(&self) {
    ///         // ...
    ///     }
    ///
    ///     async fn notification_params(&self, params: Value) {
    ///         // ...
    /// #       let _ = params;
    ///     }
    /// }
    ///
    /// let (service, socket) = LspService::build(|_| Mock)
    ///     .custom_method("custom/request", Mock::request)
    ///     .custom_method("custom/requestParams", Mock::request_params)
    ///     .custom_method("custom/notification", Mock::notification)
    ///     .custom_method("custom/notificationParams", Mock::notification_params)
    ///     .finish();
    /// ```
    pub fn custom_method<P, R, F>(mut self, name: &'static str, callback: F) -> Self
    where
        P: FromParams,
        R: IntoResponse,
        F: for<'a> Method<&'a S, P, R> + Clone + Send + Sync + 'static,
    {
        let layer = layers::Normal::new(self.state.clone(), self.pending.clone());
        self.inner.method(name, callback, layer);
        self
    }

    /// Constructs the `LspService` and returns it, along with a channel for server-to-client
    /// communication.
    pub fn finish(self) -> (LspService<S>, ClientSocket) {
        let LspServiceBuilder {
            inner,
            state,
            pending,
            socket,
        } = self;

        (
            LspService {
                inner,
                state,
                pending,
            },
            socket,
        )
    }
}

impl<S: Debug> Debug for LspServiceBuilder<S> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("LspServiceBuilder")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use lsp_types::*;
    use serde_json::json;
    use tower::ServiceExt;

    use super::*;
    use crate::jsonrpc::Result;

    #[derive(Debug)]
    struct Mock;

    #[async_trait]
    impl LanguageServer for Mock {
        async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
            Ok(InitializeResult::default())
        }

        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }

        // This handler should never resolve...
        async fn code_action_resolve(&self, _: CodeAction) -> Result<CodeAction> {
            future::pending().await
        }
    }

    impl Mock {
        async fn custom_request(&self, params: i32) -> Result<i32> {
            Ok(params)
        }
    }

    fn initialize_request(id: i64) -> Request {
        Request::build("initialize")
            .params(json!({"capabilities":{}}))
            .id(id)
            .finish()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn initializes_only_once() {
        let (mut service, _) = LspService::new(|_| Mock);

        let request = initialize_request(1);

        let response = service.ready().await.unwrap().call(request.clone()).await;
        let ok = Response::from_ok(1.into(), json!({"capabilities":{}}));
        assert_eq!(response, Ok(Some(ok)));

        let response = service.ready().await.unwrap().call(request).await;
        let err = Response::from_error(1.into(), Error::invalid_request());
        assert_eq!(response, Ok(Some(err)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn refuses_requests_after_shutdown() {
        let (mut service, _) = LspService::new(|_| Mock);

        let initialize = initialize_request(1);
        let response = service.ready().await.unwrap().call(initialize).await;
        let ok = Response::from_ok(1.into(), json!({"capabilities":{}}));
        assert_eq!(response, Ok(Some(ok)));

        let shutdown = Request::build("shutdown").id(1).finish();
        let response = service.ready().await.unwrap().call(shutdown.clone()).await;
        let ok = Response::from_ok(1.into(), json!(null));
        assert_eq!(response, Ok(Some(ok)));

        let response = service.ready().await.unwrap().call(shutdown).await;
        let err = Response::from_error(1.into(), Error::invalid_request());
        assert_eq!(response, Ok(Some(err)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn exit_notification() {
        let (mut service, _) = LspService::new(|_| Mock);

        let exit = Request::build("exit").finish();
        let response = service.ready().await.unwrap().call(exit.clone()).await;
        assert_eq!(response, Ok(None));

        let ready = future::poll_fn(|cx| service.poll_ready(cx)).await;
        assert_eq!(ready, Err(ExitedError(())));
        assert_eq!(service.call(exit).await, Err(ExitedError(())));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancels_pending_requests() {
        let (mut service, _) = LspService::new(|_| Mock);

        let initialize = initialize_request(1);
        let response = service.ready().await.unwrap().call(initialize).await;
        let ok = Response::from_ok(1.into(), json!({"capabilities":{}}));
        assert_eq!(response, Ok(Some(ok)));

        let pending_request = Request::build("codeAction/resolve")
            .params(json!({"title":""}))
            .id(1)
            .finish();

        let cancel_request = Request::build("$/cancelRequest")
            .params(json!({"id":1i32}))
            .finish();

        let pending_fut = service.ready().await.unwrap().call(pending_request);
        let cancel_fut = service.ready().await.unwrap().call(cancel_request);
        let (pending_response, cancel_response) = futures::join!(pending_fut, cancel_fut);

        let canceled = Response::from_error(1.into(), Error::request_cancelled());
        assert_eq!(pending_response, Ok(Some(canceled)));
        assert_eq!(cancel_response, Ok(None));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn serves_custom_requests() {
        let (mut service, _) = LspService::build(|_| Mock)
            .custom_method("custom", Mock::custom_request)
            .finish();

        let initialize = initialize_request(1);
        let response = service.ready().await.unwrap().call(initialize).await;
        let ok = Response::from_ok(1.into(), json!({"capabilities":{}}));
        assert_eq!(response, Ok(Some(ok)));

        let custom = Request::build("custom").params(123i32).id(1).finish();
        let response = service.ready().await.unwrap().call(custom).await;
        let ok = Response::from_ok(1.into(), json!(123i32));
        assert_eq!(response, Ok(Some(ok)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn get_inner() {
        let (service, _) = LspService::build(|_| Mock).finish();

        service
            .inner()
            .initialize(InitializeParams::default())
            .await
            .unwrap();
    }
}
