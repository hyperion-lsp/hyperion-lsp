// Modified from tower-lsp 0.20.0 for Hyperion; see MODIFICATIONS.md.

//! Generic server for multiplexing bidirectional streams through a transport.

#[cfg(feature = "runtime-agnostic")]
use async_codec_lite::{FramedRead, FramedWrite};
#[cfg(feature = "runtime-agnostic")]
use futures::io::{AsyncRead, AsyncWrite};

#[cfg(feature = "runtime-tokio")]
use tokio::io::{AsyncRead, AsyncWrite};
#[cfg(feature = "runtime-tokio")]
use tokio_util::codec::{FramedRead, FramedWrite};

use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
#[cfg(feature = "runtime-tokio")]
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, Fuse, FusedFuture};
use futures::{future, stream, FutureExt, Sink, SinkExt, Stream, StreamExt, TryFutureExt};
use tower::Service;
use tracing::{error, warn};

use crate::codec::{LanguageServerCodec, ParseError};
use crate::jsonrpc::{Error, Id, Message, Request, Response};
use crate::service::{ClientSocket, RequestStream, ResponseSink};

const DEFAULT_MAX_CONCURRENCY: usize = 4;
const MESSAGE_QUEUE_SIZE: usize = 100;

/// Document synchronization notifications are state barriers: requests read after one of these
/// notifications must observe the updated document. Other requests remain concurrent.
fn serializes_document_state(request: &Request) -> bool {
    request.id().is_none()
        && matches!(
            request.method(),
            "textDocument/didOpen" | "textDocument/didChange" | "textDocument/didClose"
        )
}

/// Upper bound on how long shutdown waits for in-flight requests and for `stdout` to flush.
#[cfg(feature = "runtime-tokio")]
const DEFAULT_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Produces a fresh deadline future each time shutdown needs to bound a wait.
type DeadlineFactory = Box<dyn Fn() -> BoxFuture<'static, ()> + Send>;

/// Creates a linked [`ShutdownTrigger`] and [`ShutdownSignal`] pair.
///
/// Pass the signal to [`Server::with_shutdown`] and keep the trigger to request an orderly
/// shutdown of that server, for example from a `SIGTERM` handler.
///
/// A [`ClientSocket`] obtained from [`LspService`](crate::LspService) already carries a signal of
/// its own, fired when the language client sends the [`exit`] notification, so servers built on
/// top of `LspService` do not need to construct this pair themselves.
///
/// [`exit`]: https://microsoft.github.io/language-server-protocol/specification#exit
pub fn shutdown_channel() -> (ShutdownTrigger, ShutdownSignal) {
    let (tx, rx) = oneshot::channel();
    (
        ShutdownTrigger {
            tx: Arc::new(Mutex::new(Some(tx))),
        },
        ShutdownSignal { rx: Some(rx) },
    )
}

/// Trigger half of a shutdown signal, created by [`shutdown_channel`].
///
/// Cloning is cheap. Firing any clone requests shutdown exactly once; firing an
/// already-fired trigger is a no-op.
#[derive(Clone, Debug)]
pub struct ShutdownTrigger {
    tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl ShutdownTrigger {
    /// Requests an orderly shutdown of the [`Server`] holding the paired [`ShutdownSignal`].
    pub fn trigger(&self) {
        let sender = match self.tx.lock() {
            Ok(mut guard) => guard.take(),
            // The guarded section cannot panic, but recover rather than propagate if it somehow
            // did: failing to signal shutdown would hang the server.
            Err(poisoned) => poisoned.into_inner().take(),
        };

        if let Some(tx) = sender {
            let _ = tx.send(());
        }
    }
}

/// Signal half of a shutdown signal, created by [`shutdown_channel`].
///
/// This future resolves once the paired [`ShutdownTrigger`] has been fired. If every clone of the
/// trigger is dropped without firing, this future never resolves: a lost trigger means "nobody
/// will ever ask for shutdown", not "shut down now". The server then falls back to terminating on
/// end-of-input, which is the same lifecycle it has when no signal is supplied at all.
#[derive(Debug)]
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ShutdownSignal {
    rx: Option<oneshot::Receiver<()>>,
}

impl Future for ShutdownSignal {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Some(rx) = this.rx.as_mut() else {
            return Poll::Pending;
        };

        match Pin::new(rx).poll(cx) {
            Poll::Ready(Ok(())) => {
                this.rx = None;
                Poll::Ready(())
            }
            Poll::Ready(Err(_)) => {
                this.rx = None;
                Poll::Pending
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Trait implemented by client loopback sockets.
///
/// This socket handles the server-to-client half of the bidirectional communication stream.
pub trait Loopback {
    /// Yields a stream of pending server-to-client requests.
    type RequestStream: Stream<Item = Request>;
    /// Routes client-to-server responses back to the server.
    type ResponseSink: Sink<Response> + Unpin;

    /// Splits this socket into two halves capable of operating independently.
    ///
    /// The two halves returned implement the [`Stream`] and [`Sink`] traits, respectively.
    fn split(self) -> (Self::RequestStream, Self::ResponseSink);

    /// Takes the shutdown signal this socket carries, if any.
    ///
    /// [`Server::serve`] calls this before [`split`](Loopback::split) so that a socket which knows
    /// when the language server has exited can end the connection without waiting for the client
    /// to close `stdin`. The default implementation returns `None`, leaving the server with an
    /// end-of-input driven lifecycle.
    fn shutdown_signal(&mut self) -> Option<ShutdownSignal> {
        None
    }
}

impl Loopback for ClientSocket {
    type RequestStream = RequestStream;
    type ResponseSink = ResponseSink;

    #[inline]
    fn split(self) -> (Self::RequestStream, Self::ResponseSink) {
        self.split()
    }

    #[inline]
    fn shutdown_signal(&mut self) -> Option<ShutdownSignal> {
        self.take_shutdown_signal()
    }
}

/// Server for processing requests and responses on standard I/O or TCP.
pub struct Server<I, O, L = ClientSocket> {
    stdin: I,
    stdout: O,
    loopback: L,
    max_concurrency: usize,
    shutdown: Option<ShutdownSignal>,
    drain_deadline: Option<DeadlineFactory>,
}

impl<I: Debug, O: Debug, L: Debug> Debug for Server<I, O, L> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("stdin", &self.stdin)
            .field("stdout", &self.stdout)
            .field("loopback", &self.loopback)
            .field("max_concurrency", &self.max_concurrency)
            .field("shutdown", &self.shutdown)
            .field(
                "drain_deadline",
                &self.drain_deadline.as_ref().map(|_| "<deadline>"),
            )
            .finish()
    }
}

#[cfg(feature = "runtime-tokio")]
fn default_drain_deadline() -> Option<DeadlineFactory> {
    Some(Box::new(|| {
        Box::pin(tokio::time::sleep(DEFAULT_DRAIN_TIMEOUT))
    }))
}

#[cfg(not(feature = "runtime-tokio"))]
fn default_drain_deadline() -> Option<DeadlineFactory> {
    None
}

impl<I, O, L> Server<I, O, L>
where
    I: AsyncRead + Unpin,
    O: AsyncWrite,
    L: Loopback,
    <L::ResponseSink as Sink<Response>>::Error: std::error::Error,
{
    /// Creates a new `Server` with the given `stdin` and `stdout` handles.
    pub fn new(stdin: I, stdout: O, socket: L) -> Self {
        Server {
            stdin,
            stdout,
            loopback: socket,
            max_concurrency: DEFAULT_MAX_CONCURRENCY,
            shutdown: None,
            drain_deadline: default_drain_deadline(),
        }
    }

    /// Ends the connection when `signal` fires, even if `stdin` is still open.
    ///
    /// Without a signal, the server serves until `stdin` reaches end-of-input. That is the right
    /// lifecycle for a plain pipe, but not for LSP: the [`exit`] notification asks the server to
    /// terminate while the editor typically keeps the pipe open, so an input-driven server would
    /// sit idle forever waiting for an EOF that never comes.
    ///
    /// [`exit`]: https://microsoft.github.io/language-server-protocol/specification#exit
    ///
    /// Sockets obtained from [`LspService`](crate::LspService) supply this signal automatically
    /// (see [`Loopback::shutdown_signal`]); calling this method overrides that. Use
    /// [`shutdown_channel`] to build your own signal, e.g. for a `SIGTERM` handler.
    ///
    /// Whichever way shutdown is requested, it is *orderly*: reading stops first, then in-flight
    /// request handlers are given a bounded chance to finish, and finally their responses are
    /// flushed to `stdout`. This is what allows the response to a [`shutdown`] request to still
    /// reach the client when `exit` follows immediately behind it.
    ///
    /// [`shutdown`]: https://microsoft.github.io/language-server-protocol/specification#shutdown
    pub fn with_shutdown(mut self, signal: ShutdownSignal) -> Self {
        self.shutdown = Some(signal);
        self
    }

    /// Bounds how long shutdown waits for in-flight request handlers and for `stdout` to flush.
    ///
    /// If not explicitly specified, this defaults to 5 seconds. Requests still in flight when the
    /// bound elapses are dropped without a response.
    #[cfg(feature = "runtime-tokio")]
    pub fn with_drain_timeout(mut self, timeout: Duration) -> Self {
        self.drain_deadline = Some(Box::new(move || Box::pin(tokio::time::sleep(timeout))));
        self
    }

    /// Bounds shutdown waits using a caller-supplied timer.
    ///
    /// `deadline` is invoked once per bounded wait and must return a future that resolves when
    /// that wait should be abandoned. This is the executor-agnostic form of
    /// [`with_drain_timeout`](Server::with_drain_timeout): pass a timer from whatever runtime is
    /// actually present, or [`future::pending`] to wait indefinitely.
    pub fn with_drain_deadline<F, Fut>(mut self, deadline: F) -> Self
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.drain_deadline = Some(Box::new(move || Box::pin(deadline())));
        self
    }

    /// Sets the server concurrency limit to `max`.
    ///
    /// This setting specifies how many incoming requests may be processed concurrently. Setting
    /// this value to `1` forces all requests to be processed sequentially, thereby implicitly
    /// disabling support for the [`$/cancelRequest`] notification.
    ///
    /// [`$/cancelRequest`]: https://microsoft.github.io/language-server-protocol/specification#cancelRequest
    ///
    /// If not explicitly specified, `max` defaults to 4.
    ///
    /// # Preference over standard `tower` middleware
    ///
    /// The [`ConcurrencyLimit`] and [`Buffer`] middlewares provided by `tower` rely on
    /// [`tokio::spawn`] in common usage, while this library aims to be executor agnostic and to
    /// support exotic targets currently incompatible with `tokio`, such as WASM. As such, `Server`
    /// includes its own concurrency facilities that don't require a global executor to be present.
    ///
    /// [`ConcurrencyLimit`]: https://docs.rs/tower/latest/tower/limit/concurrency/struct.ConcurrencyLimit.html
    /// [`Buffer`]: https://docs.rs/tower/latest/tower/buffer/index.html
    /// [`tokio::spawn`]: https://docs.rs/tokio/latest/tokio/fn.spawn.html
    pub fn concurrency_level(mut self, max: usize) -> Self {
        self.max_concurrency = max;
        self
    }

    /// Spawns the service with messages read through `stdin` and responses written to `stdout`.
    pub async fn serve<T>(self, mut service: T)
    where
        T: Service<Request, Response = Option<Response>> + Send + 'static,
        T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        T::Future: Send,
    {
        let Server {
            stdin,
            stdout,
            mut loopback,
            max_concurrency,
            shutdown,
            drain_deadline,
        } = self;

        // An explicit signal wins over the one the socket carries, if any.
        let shutdown = shutdown.or_else(|| loopback.shutdown_signal());

        let (client_requests, mut client_responses) = loopback.split();
        let (client_requests, client_abort) = stream::abortable(client_requests);
        let (mut responses_tx, responses_rx) = mpsc::channel(0);
        let (mut server_tasks_tx, server_tasks_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);

        let mut framed_stdin = FramedRead::new(stdin, LanguageServerCodec::default());
        let framed_stdout = FramedWrite::new(stdout, LanguageServerCodec::default());

        let process_server_tasks = server_tasks_rx
            .buffer_unordered(max_concurrency)
            .filter_map(future::ready)
            .map(|res| Ok(Message::Response(res)))
            .forward(responses_tx.clone().sink_map_err(|_| unreachable!()))
            .map(|_| ());

        // Egress drains both sources until each one ends, and nothing more. It deliberately has no
        // say in how long the connection lives: an empty server-to-client stream is
        // indistinguishable from a closed one, so inferring "the client is gone" from exhaustion
        // here would tear the connection down before a single byte of input had been read.
        let print_output = stream::select(responses_rx, client_requests.map(Message::Request))
            .map(Ok)
            .forward(framed_stdout.sink_map_err(|err| error!("failed to encode message: {}", err)))
            .map(|_| ());

        let read_input = async move {
            while let Some(msg) = framed_stdin.next().await {
                match msg {
                    Ok(Message::Request(req)) => {
                        if let Err(err) = future::poll_fn(|cx| service.poll_ready(cx)).await {
                            error!("{}", display_sources(err.into().as_ref()));
                            return;
                        }

                        let document_state_barrier = serializes_document_state(&req);
                        let fut = service.call(req).unwrap_or_else(|err| {
                            error!("{}", display_sources(err.into().as_ref()));
                            None
                        });

                        if document_state_barrier {
                            if let Some(response) = fut.await {
                                if responses_tx
                                    .send(Message::Response(response))
                                    .await
                                    .is_err()
                                {
                                    error!("output stream closed, no longer reading input");
                                    return;
                                }
                            }
                        } else if server_tasks_tx.send(fut).await.is_err() {
                            error!("server task queue closed, no longer reading input");
                            return;
                        }
                    }
                    Ok(Message::Response(res)) => {
                        if let Err(err) = client_responses.send(res).await {
                            error!("{}", display_sources(&err));
                            return;
                        }
                    }
                    Err(err) => {
                        error!("failed to decode message: {}", err);
                        let res = Response::from_error(Id::Null, to_jsonrpc_error(err));
                        if responses_tx.send(Message::Response(res)).await.is_err() {
                            error!("output stream closed, no longer reading input");
                            return;
                        }
                    }
                }
            }

            // End-of-input: release our queue handles so the handler pipeline can observe the end
            // of the request stream and drain what is left.
            server_tasks_tx.disconnect();
            responses_tx.disconnect();
        };

        // The supervisor below owns connection lifetime. Egress is polled first in every phase so
        // that messages already queued for the client are written before anything else advances,
        // preserving the ordering guarantees callers observe on the wire.
        let mut output = Box::pin(print_output.fuse());
        let mut ingress = Box::pin(read_input.fuse());
        let mut tasks = Box::pin(process_server_tasks.fuse());
        let mut shutdown = Box::pin(match shutdown {
            Some(signal) => signal.fuse(),
            None => Fuse::terminated(),
        });
        let mut deadline: Pin<Box<Fuse<BoxFuture<'static, ()>>>> = Box::pin(Fuse::terminated());

        // Phase 1: serve until input ends or shutdown is requested.
        loop {
            futures::select_biased! {
                () = output => break,
                () = ingress => break,
                () = shutdown => break,
                () = tasks => {}
                complete => break,
            }
        }

        // Phase 2: stop ingress. Dropping the reader mid-poll releases the queue handles it owns,
        // which is what lets the handler pipeline below see end-of-input and finish.
        drop(ingress);

        if let Some(new_deadline) = &drain_deadline {
            deadline.set(new_deadline().fuse());
        }

        // Bounded drain: give handlers that are already running a chance to produce their
        // responses. Cutting them off here is what used to lose the `shutdown` response when
        // `exit` arrived immediately behind it.
        futures::select_biased! {
            () = output => {}
            () = tasks => {}
            () = deadline => warn!("timed out draining in-flight requests, dropping them"),
            complete => {}
        }

        // Whether the drain finished or timed out, the pipeline is done: dropping it releases the
        // last sender feeding egress, so egress can observe the end of its response stream.
        tasks.set(Fuse::terminated());

        // Phase 3: flush. Ending the server-to-client stream lets egress run to completion, which
        // writes everything still queued and closes `stdout`.
        client_abort.abort();

        if !output.is_terminated() {
            match &drain_deadline {
                Some(new_deadline) => deadline.set(new_deadline().fuse()),
                None => deadline.set(Fuse::terminated()),
            }

            futures::select_biased! {
                () = output => {}
                () = deadline => warn!("timed out flushing pending messages to output"),
                complete => {}
            }
        }
    }
}

fn display_sources(error: &dyn std::error::Error) -> String {
    if let Some(source) = error.source() {
        format!("{}: {}", error, display_sources(source))
    } else {
        error.to_string()
    }
}

#[cfg(feature = "runtime-tokio")]
fn to_jsonrpc_error(err: ParseError) -> Error {
    match err {
        ParseError::Body(err) if err.is_data() => Error::invalid_request(),
        _ => Error::parse_error(),
    }
}

#[cfg(feature = "runtime-agnostic")]
fn to_jsonrpc_error(err: impl std::error::Error) -> Error {
    match err.source().and_then(|e| e.downcast_ref()) {
        Some(ParseError::Body(err)) if err.is_data() => Error::invalid_request(),
        _ => Error::parse_error(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use std::time::Duration;

    #[cfg(feature = "runtime-agnostic")]
    use futures::io::Cursor;
    #[cfg(feature = "runtime-tokio")]
    use std::io::Cursor;

    use async_trait::async_trait;
    use futures::future::Ready;
    use futures::{future, sink, stream};
    use serde_json::{json, Value};

    use super::*;
    use crate::jsonrpc::Result as RpcResult;
    use crate::{LanguageServer, LspService};

    const REQUEST: &str = r#"{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}"#;
    const RESPONSE: &str = r#"{"jsonrpc":"2.0","result":{"capabilities":{}},"id":1}"#;

    /// Bound on every test which asserts that `serve()` terminates. Generous on purpose: the
    /// point is to fail the test rather than hang the suite, not to measure latency.
    const TEST_TIMEOUT: Duration = Duration::from_secs(10);

    /// Yields once, handing control back to the executor so other tasks can make progress.
    async fn yield_now() {
        let mut yielded = false;
        future::poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
    }

    /// An input handle that behaves like an editor's `stdin`: it delivers messages one at a time
    /// and then stays open indefinitely instead of reporting end-of-input.
    ///
    /// Between chunks it stalls for a few polls so the rest of the server can run, which is what a
    /// real client does when it waits for a response before sending the next request.
    #[derive(Debug)]
    struct OpenPipe {
        chunks: VecDeque<Vec<u8>>,
        stalls: usize,
    }

    impl OpenPipe {
        const STALLS_BETWEEN_CHUNKS: usize = 8;

        fn new(chunks: Vec<Vec<u8>>) -> Self {
            OpenPipe {
                chunks: chunks.into(),
                stalls: 0,
            }
        }

        /// Returns the next readable slice, or `None` if the pipe is idle for now.
        fn poll_chunk(&mut self, cx: &Context<'_>, capacity: usize) -> Option<Vec<u8>> {
            let front = self.chunks.front_mut()?;

            if self.stalls > 0 {
                self.stalls -= 1;
                cx.waker().wake_by_ref();
                return None;
            }

            let len = capacity.min(front.len());
            let bytes: Vec<u8> = front.drain(..len).collect();
            if front.is_empty() {
                self.chunks.pop_front();
                self.stalls = Self::STALLS_BETWEEN_CHUNKS;
            }

            Some(bytes)
        }
    }

    #[cfg(feature = "runtime-tokio")]
    impl tokio::io::AsyncRead for OpenPipe {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            match self.get_mut().poll_chunk(cx, buf.remaining()) {
                Some(bytes) => {
                    buf.put_slice(&bytes);
                    Poll::Ready(Ok(()))
                }
                None => Poll::Pending,
            }
        }
    }

    #[cfg(feature = "runtime-agnostic")]
    impl futures::io::AsyncRead for OpenPipe {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            match self.get_mut().poll_chunk(cx, buf.len()) {
                Some(bytes) => {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    Poll::Ready(Ok(bytes.len()))
                }
                None => Poll::Pending,
            }
        }
    }

    #[derive(Debug)]
    struct MockService;

    impl Service<Request> for MockService {
        type Response = Option<Response>;
        type Error = String;
        type Future = Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: Request) -> Self::Future {
            let response = serde_json::from_str(RESPONSE).unwrap();
            future::ok(Some(response))
        }
    }

    struct MockLoopback(Vec<Request>);

    impl Loopback for MockLoopback {
        type RequestStream = stream::Iter<std::vec::IntoIter<Request>>;
        type ResponseSink = sink::Drain<Response>;

        fn split(self) -> (Self::RequestStream, Self::ResponseSink) {
            (stream::iter(self.0), sink::drain())
        }
    }

    fn mock_request() -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{}", REQUEST.len(), REQUEST).into_bytes()
    }

    fn mock_response() -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{}", RESPONSE.len(), RESPONSE).into_bytes()
    }

    fn mock_stdio() -> (Cursor<Vec<u8>>, Vec<u8>) {
        (Cursor::new(mock_request()), Vec::new())
    }

    #[tokio::test(flavor = "current_thread")]
    async fn serves_on_stdio() {
        let (mut stdin, mut stdout) = mock_stdio();
        Server::new(&mut stdin, &mut stdout, MockLoopback(vec![]))
            .serve(MockService)
            .await;

        assert_eq!(stdin.position(), 80);
        assert_eq!(stdout, mock_response());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interleaves_messages() {
        let socket = MockLoopback(vec![serde_json::from_str(REQUEST).unwrap()]);

        let (mut stdin, mut stdout) = mock_stdio();
        Server::new(&mut stdin, &mut stdout, socket)
            .serve(MockService)
            .await;

        assert_eq!(stdin.position(), 80);
        let output: Vec<_> = mock_request().into_iter().chain(mock_response()).collect();
        assert_eq!(stdout, output);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn handles_invalid_json() {
        let invalid = r#"{"jsonrpc":"2.0","method":"#;
        let message = format!("Content-Length: {}\r\n\r\n{}", invalid.len(), invalid).into_bytes();
        let (mut stdin, mut stdout) = (Cursor::new(message), Vec::new());

        Server::new(&mut stdin, &mut stdout, MockLoopback(vec![]))
            .serve(MockService)
            .await;

        assert_eq!(stdin.position(), 48);
        let err = r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"},"id":null}"#;
        let output = format!("Content-Length: {}\r\n\r\n{}", err.len(), err).into_bytes();
        assert_eq!(stdout, output);
    }

    /// A service whose responses only become available after several polls, so that a request is
    /// guaranteed to still be in flight when input ends.
    #[derive(Debug)]
    struct SlowService;

    impl Service<Request> for SlowService {
        type Response = Option<Response>;
        type Error = String;
        type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: Request) -> Self::Future {
            Box::pin(async {
                for _ in 0..5 {
                    yield_now().await;
                }
                Ok(Some(serde_json::from_str(RESPONSE).unwrap()))
            })
        }
    }

    #[derive(Debug)]
    struct DocumentOrderingService {
        opened: Arc<AtomicBool>,
    }

    impl Service<Request> for DocumentOrderingService {
        type Response = Option<Response>;
        type Error = String;
        type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: Request) -> Self::Future {
            let method = request.method().to_owned();
            let id = request.id().cloned();
            let opened = Arc::clone(&self.opened);

            Box::pin(async move {
                if method == "textDocument/didOpen" {
                    for _ in 0..5 {
                        yield_now().await;
                    }
                    opened.store(true, Ordering::Release);
                    return Ok(None);
                }

                Ok(id.map(|id| {
                    Response::from_ok(
                        id,
                        json!({"documentWasOpen": opened.load(Ordering::Acquire)}),
                    )
                }))
            })
        }
    }

    /// A service whose responses never become available.
    #[derive(Debug)]
    struct StuckService;

    impl Service<Request> for StuckService {
        type Response = Option<Response>;
        type Error = String;
        type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

        fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: Request) -> Self::Future {
            Box::pin(future::pending())
        }
    }

    #[derive(Debug)]
    struct MockBackend;

    #[async_trait]
    impl LanguageServer for MockBackend {
        async fn initialize(
            &self,
            _: lsp_types::InitializeParams,
        ) -> RpcResult<lsp_types::InitializeResult> {
            Ok(lsp_types::InitializeResult::default())
        }

        async fn shutdown(&self) -> RpcResult<()> {
            Ok(())
        }
    }

    fn frame(message: &str) -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{}", message.len(), message).into_bytes()
    }

    /// Splits framed output back into the JSON values that were written.
    fn decode_messages(bytes: &[u8]) -> Vec<Value> {
        let text = std::str::from_utf8(bytes).expect("output must be valid UTF-8");
        let mut messages = Vec::new();
        let mut rest = text;

        while let Some(offset) = rest.find("\r\n\r\n") {
            let len: usize = rest[..offset]
                .trim_start_matches("Content-Length:")
                .trim()
                .parse()
                .expect("output must carry a `Content-Length` header");

            let body = &rest[offset + 4..offset + 4 + len];
            messages.push(serde_json::from_str(body).expect("output must be valid JSON"));
            rest = &rest[offset + 4 + len..];
        }

        messages
    }

    /// The shutdown signal, not end-of-input, is what ends an LSP connection: editors keep `stdin`
    /// open across `exit`, so a server which waits for EOF simply hangs.
    #[tokio::test(flavor = "current_thread")]
    async fn returns_when_shutdown_signal_fires_with_input_still_open() {
        let (trigger, signal) = shutdown_channel();
        let mut stdout = Vec::new();

        let serve = Server::new(
            OpenPipe::new(vec![mock_request()]),
            &mut stdout,
            MockLoopback(vec![]),
        )
        .with_shutdown(signal)
        .serve(MockService);

        let request_shutdown = async move {
            // Let the request be served first; the point of this test is that shutdown ends a
            // connection which is otherwise perfectly healthy and still open.
            for _ in 0..64 {
                yield_now().await;
            }
            trigger.trigger();
        };

        let (served, ()) =
            futures::join!(tokio::time::timeout(TEST_TIMEOUT, serve), request_shutdown);

        served.expect("serve() must return once the shutdown signal fires");
        assert_eq!(stdout, mock_response());
    }

    /// A trigger that is dropped without ever firing must not be mistaken for a fired one, or we
    /// are back to guessing at connection lifetime.
    #[tokio::test(flavor = "current_thread")]
    async fn dropped_shutdown_trigger_falls_back_to_end_of_input() {
        let (trigger, signal) = shutdown_channel();
        drop(trigger);

        let (mut stdin, mut stdout) = mock_stdio();
        let serve = Server::new(&mut stdin, &mut stdout, MockLoopback(vec![]))
            .with_shutdown(signal)
            .serve(MockService);

        tokio::time::timeout(TEST_TIMEOUT, serve)
            .await
            .expect("serve() must still terminate on end-of-input");

        assert_eq!(stdin.position(), 80);
        assert_eq!(stdout, mock_response());
    }

    /// The spec requires the server to answer `shutdown` before it exits, and clients send `exit`
    /// immediately behind it. Termination must therefore drain and flush, not cut the wire.
    #[tokio::test(flavor = "current_thread")]
    async fn flushes_shutdown_response_before_exiting() {
        let (service, socket) = LspService::new(|_| MockBackend);

        let stdin = OpenPipe::new(vec![
            frame(r#"{"jsonrpc":"2.0","method":"initialize","params":{"capabilities":{}},"id":1}"#),
            frame(r#"{"jsonrpc":"2.0","method":"shutdown","id":2}"#),
            frame(r#"{"jsonrpc":"2.0","method":"exit"}"#),
        ]);

        let mut stdout = Vec::new();
        let serve = Server::new(stdin, &mut stdout, socket).serve(service);

        tokio::time::timeout(TEST_TIMEOUT, serve)
            .await
            .expect("serve() must return after the `exit` notification");

        let messages = decode_messages(&stdout);
        assert_eq!(
            messages,
            vec![
                json!({"jsonrpc":"2.0","result":{"capabilities":{}},"id":1}),
                json!({"jsonrpc":"2.0","result":null,"id":2}),
            ]
        );
    }

    /// End-of-input stops reading, but it must not discard work already accepted.
    #[tokio::test(flavor = "current_thread")]
    async fn writes_response_for_request_in_flight_at_end_of_input() {
        let (mut stdin, mut stdout) = mock_stdio();

        let serve = Server::new(&mut stdin, &mut stdout, MockLoopback(vec![])).serve(SlowService);
        tokio::time::timeout(TEST_TIMEOUT, serve)
            .await
            .expect("serve() must return on end-of-input");

        assert_eq!(stdin.position(), 80);
        assert_eq!(stdout, mock_response());
    }

    /// LSP clients do not wait after `didOpen`/`didChange` before sending semantic requests. The
    /// transport must therefore finish document-state notifications before dispatching requests
    /// that follow them, while retaining concurrency among ordinary requests.
    #[tokio::test(flavor = "current_thread")]
    async fn document_notifications_are_barriers_for_following_requests() {
        let did_open = frame(
            r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///T.java","languageId":"java","version":1,"text":"class T {}"}}}"#,
        );
        let hover = frame(r#"{"jsonrpc":"2.0","method":"textDocument/hover","params":{},"id":7}"#);
        let input: Vec<u8> = did_open.into_iter().chain(hover).collect();
        let mut stdin = Cursor::new(input);
        let mut stdout = Vec::new();

        Server::new(&mut stdin, &mut stdout, MockLoopback(vec![]))
            .serve(DocumentOrderingService {
                opened: Arc::new(AtomicBool::new(false)),
            })
            .await;

        assert_eq!(
            decode_messages(&stdout),
            vec![json!({
                "jsonrpc": "2.0",
                "result": {"documentWasOpen": true},
                "id": 7
            })]
        );
    }

    /// The drain is bounded: a handler that never finishes delays shutdown, it does not prevent it.
    #[cfg(feature = "runtime-tokio")]
    #[tokio::test(flavor = "current_thread")]
    async fn bounded_drain_abandons_stuck_requests() {
        let (mut stdin, mut stdout) = mock_stdio();

        let serve = Server::new(&mut stdin, &mut stdout, MockLoopback(vec![]))
            .with_drain_timeout(Duration::from_millis(50))
            .serve(StuckService);

        tokio::time::timeout(TEST_TIMEOUT, serve)
            .await
            .expect("serve() must give up on requests that never finish");

        assert_eq!(stdin.position(), 80);
        assert!(stdout.is_empty(), "no response was ever produced");
    }
}
