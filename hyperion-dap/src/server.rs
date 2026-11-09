// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! DAP server main loop.
//!
//! Reads DAP requests from stdin, dispatches to `DebugSession`, and writes
//! responses/events to stdout. Uses `tokio::select!` to concurrently process
//! JDWP events without blocking on stdin reads.

use std::sync::atomic::{AtomicU64, Ordering};

use tokio::io::{self, AsyncBufRead, AsyncWrite};
use tokio::sync::mpsc;

use hyperion_jdwp::commands::{SUSPEND_ALL, SUSPEND_EVENT_THREAD, SUSPEND_NONE};
use hyperion_jdwp::events::{Event, EventSet};

use crate::protocol::{
    parse_request, read_message, serialize_event, serialize_response, write_message,
};
use crate::session::DebugSession;
use crate::types::{
    AttachArguments, ContinueArguments, ContinuedEventBody, DapEvent, DapRequest, DapResponse,
    DisconnectArguments, EvaluateArguments, ExceptionInfoArguments, ExitedEventBody,
    InitializeArguments, LaunchArguments, OutputEventBody, ScopesArguments,
    SetBreakpointsArguments, SetExceptionBreakpointsArguments, StackTraceArguments, StepArguments,
    StoppedEventBody, ThreadEventBody, VariablesArguments,
};

/// Owns the DAP read future across event-loop iterations. A fresh
/// `read_message` future inside `select!` would be cancelled whenever a JDWP
/// or output event wins, after it may already have consumed part of a frame.
struct RequestStream {
    receiver: mpsc::Receiver<io::Result<Option<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for RequestStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn spawn_request_stream<R>(mut reader: R) -> RequestStream
where
    R: AsyncBufRead + Unpin + Send + 'static,
{
    let (sender, receiver) = mpsc::channel(32);
    let task = tokio::spawn(async move {
        loop {
            let result = read_message(&mut reader).await;
            let finished = matches!(&result, Ok(None) | Err(_));
            if sender.send(result).await.is_err() || finished {
                break;
            }
        }
    });
    RequestStream { receiver, task }
}

/// Run the DAP server on the given reader/writer streams.
pub async fn run<R, W>(reader: R, mut writer: W) -> io::Result<()>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut session = DebugSession::new();
    let seq = AtomicU64::new(1);
    let mut requests = spawn_request_stream(reader);
    let next_seq = || seq.fetch_add(1, Ordering::Relaxed);

    // Start with no JDWP event receiver — it's taken after launch/attach.
    let mut jdwp_rx: Option<mpsc::Receiver<EventSet>> = None;
    // Start with no debuggee-output receiver — only set after `launch`
    // (attach doesn't spawn a process, so there's no stdout/stderr to drain).
    let mut output_rx: Option<mpsc::Receiver<crate::session::OutputChunk>> = None;
    // A debuggee terminates once: VM death and the closing JDWP connection are
    // two signals of the same end, and clients must see one `terminated`.
    let mut terminated = false;

    loop {
        // Use tokio::select! to concurrently wait for DAP requests and JDWP events.
        tokio::select! {
            msg = requests.receiver.recv() => {
                let msg = msg.ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "DAP request reader stopped"))??;
                let Some(msg) = msg else {
                    tracing::info!("stdin closed, shutting down");
                    break;
                };

                let request = match parse_request(&msg) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!("failed to parse DAP message: {e}");
                        continue;
                    }
                };

                tracing::debug!("DAP request: {} (seq={})", request.command, request.seq);

                let response = dispatch_request(&mut session, &request, &next_seq).await;
                // A failed launch/attach must never be silent: show the reason
                // (for a JVM that died, what it printed) in the client's output
                // before the failed response.
                if !response.success && matches!(request.command.as_str(), "launch" | "attach") {
                    if let Some(message) = &response.message {
                        let event = DapEvent::new(
                            next_seq(),
                            "output",
                            OutputEventBody {
                                category: "stderr".to_owned(),
                                output: format!("{message}\n"),
                            },
                        );
                        write_message(&mut writer, &serialize_event(&event)).await?;
                    }
                }
                let json = serialize_response(&response);
                write_message(&mut writer, &json).await?;

                // After certain requests, send follow-up events.
                match request.command.as_str() {
                    "initialize" => {
                        let event = DapEvent::simple(next_seq(), "initialized");
                        write_message(&mut writer, &serialize_event(&event)).await?;
                    }
                    "launch" | "attach" if response.success => {
                        // Take the event receiver now that we're connected.
                        jdwp_rx = session.take_event_receiver();
                        // Only `launch` populates this (it spawns the JVM);
                        // `attach` leaves it `None` and the branch below is a no-op.
                        output_rx = session.take_output_receiver();
                    }
                    "continue" | "next" | "stepIn" | "stepOut" if response.success => {
                        let thread_id = request.arguments["threadId"].as_u64().unwrap_or(0);
                        let continued = DapEvent::new(
                            next_seq(),
                            "continued",
                            ContinuedEventBody {
                                thread_id,
                                all_threads_continued: Some(true),
                            },
                        );
                        write_message(&mut writer, &serialize_event(&continued)).await?;
                    }
                    "disconnect" => {
                        tracing::info!("client disconnected");
                        break;
                    }
                    _ => {}
                }
            }

            // Only select on JDWP events when we have a receiver.
            result = async {
                match jdwp_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match result {
                    Some(event_set) => {
                        let vm_died = handle_jdwp_event_set(&mut session, &event_set, &mut writer, &next_seq).await?;
                        if vm_died {
                            finish_debuggee(&mut session, &mut output_rx, &mut writer, &next_seq, &mut terminated).await?;
                            jdwp_rx = None;
                        }
                    }
                    None => {
                        // JDWP channel closed — the JVM is gone (with or without
                        // a VmDeath event). Report the end so the client doesn't hang.
                        finish_debuggee(&mut session, &mut output_rx, &mut writer, &next_seq, &mut terminated).await?;
                        jdwp_rx = None;
                    }
                }
            }

            // Only select on debuggee output when we have a receiver (set
            // after `launch`; stays `None` for `attach`).
            result = async {
                match output_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match result {
                    Some((label, output)) => {
                        let category = if label == "stderr" { "stderr" } else { "stdout" };
                        let event = DapEvent::new(
                            next_seq(),
                            "output",
                            OutputEventBody {
                                category: category.to_owned(),
                                output,
                            },
                        );
                        write_message(&mut writer, &serialize_event(&event)).await?;
                    }
                    None => {
                        // Both stdout/stderr drain tasks finished (debuggee
                        // exited and its pipes were closed) — nothing left to forward.
                        output_rx = None;
                    }
                }
            }
        }
    }

    Ok(())
}

/// The debuggee is gone: flush what it printed, then report `exited` (with the
/// exit code, for a process we launched) and exactly one `terminated`.
#[allow(clippy::future_not_send)]
async fn finish_debuggee<W: AsyncWrite + Unpin>(
    session: &mut DebugSession,
    output_rx: &mut Option<mpsc::Receiver<crate::session::OutputChunk>>,
    writer: &mut W,
    next_seq: &impl Fn() -> u64,
    terminated: &mut bool,
) -> io::Result<()> {
    if *terminated {
        return Ok(());
    }
    *terminated = true;

    let exit_code = session.wait_exit_code().await;
    session.finish_output().await;
    if let Some(rx) = output_rx.as_mut() {
        while let Ok((label, output)) = rx.try_recv() {
            let category = if label == "stderr" {
                "stderr"
            } else {
                "stdout"
            };
            let event = DapEvent::new(
                next_seq(),
                "output",
                OutputEventBody {
                    category: category.to_owned(),
                    output,
                },
            );
            write_message(writer, &serialize_event(&event)).await?;
        }
    }
    *output_rx = None;

    if let Some(exit_code) = exit_code {
        let event = DapEvent::new(next_seq(), "exited", ExitedEventBody { exit_code });
        write_message(writer, &serialize_event(&event)).await?;
    }
    write_message(
        writer,
        &serialize_event(&DapEvent::simple(next_seq(), "terminated")),
    )
    .await
}

/// Handle a single JDWP event set, translating events to DAP events. Returns
/// true when the set announced the VM's death.
#[allow(clippy::future_not_send)]
async fn handle_jdwp_event_set<W: AsyncWrite + Unpin>(
    session: &mut DebugSession,
    event_set: &EventSet,
    writer: &mut W,
    next_seq: &impl Fn() -> u64,
) -> io::Result<bool> {
    let mut vm_died = false;
    // JDWP suspends once for the entire composite event, which may contain several
    // breakpoints, steps, or class preparations. Resume only after every event has
    // been handled, and only if none of them must leave the debuggee stopped.
    let mut should_resume = false;
    let mut should_stop = false;
    let mut resume_thread = None;
    for event in &event_set.events {
        let dap_event = match event {
            Event::Breakpoint {
                request_id,
                thread_id,
                ..
            } => {
                if event_set.suspend_policy == SUSPEND_NONE {
                    tracing::warn!("breakpoint event did not suspend the debuggee");
                    continue;
                }
                // Check conditional breakpoint — evaluate condition, auto-resume if falsy.
                let stops = session
                    .should_stop_for_breakpoint(*request_id, *thread_id)
                    .await;
                for text in session.take_pending_logs() {
                    let log = DapEvent::new(
                        next_seq(),
                        "output",
                        OutputEventBody {
                            category: "console".to_owned(),
                            output: text,
                        },
                    );
                    write_message(writer, &serialize_event(&log)).await?;
                }
                if !stops {
                    should_resume = true;
                    resume_thread = Some(*thread_id);
                    continue;
                }
                should_stop = true;
                // A step this stop interrupted must not fire later.
                session.forget_step().await;
                DapEvent::new(
                    next_seq(),
                    "stopped",
                    StoppedEventBody {
                        reason: "breakpoint".to_owned(),
                        thread_id: Some(*thread_id),
                        all_threads_stopped: Some(event_set.suspend_policy == SUSPEND_ALL),
                        description: None,
                        text: None,
                    },
                )
            }
            Event::SingleStep { thread_id, .. } => {
                if let Err(error) = session.clear_active_step_request().await {
                    tracing::error!("{error}");
                }
                if event_set.suspend_policy == SUSPEND_NONE {
                    tracing::warn!("step event did not suspend the debuggee");
                    continue;
                }
                // Landed in a JDK/synthetic frame: keep stepping.
                if session.step_continues(*thread_id).await {
                    should_resume = true;
                    resume_thread = Some(*thread_id);
                    continue;
                }
                should_stop = true;
                DapEvent::new(
                    next_seq(),
                    "stopped",
                    StoppedEventBody {
                        reason: "step".to_owned(),
                        thread_id: Some(*thread_id),
                        all_threads_stopped: Some(event_set.suspend_policy == SUSPEND_ALL),
                        description: None,
                        text: None,
                    },
                )
            }
            Event::Exception {
                thread_id,
                exception_id,
                catch_location,
                ..
            } => {
                if event_set.suspend_policy == SUSPEND_NONE {
                    tracing::warn!("exception event did not suspend the debuggee");
                    continue;
                }
                should_stop = true;
                session.forget_step().await;
                // A `catch_location` with class_id 0 means JDWP found no
                // handler for this exception — i.e. it's uncaught and about
                // to crash the debuggee (or the thread) once resumed.
                let uncaught = catch_location.class_id == 0;
                session.record_exception(*thread_id, *exception_id, uncaught);
                let description = session.describe_exception(*exception_id).await;
                DapEvent::new(
                    next_seq(),
                    "stopped",
                    StoppedEventBody {
                        reason: "exception".to_owned(),
                        thread_id: Some(*thread_id),
                        all_threads_stopped: Some(event_set.suspend_policy == SUSPEND_ALL),
                        description: description.clone(),
                        text: description,
                    },
                )
            }
            Event::ThreadStart { thread_id, .. } => DapEvent::new(
                next_seq(),
                "thread",
                ThreadEventBody {
                    reason: "started".to_owned(),
                    thread_id: *thread_id,
                },
            ),
            Event::ThreadDeath { thread_id, .. } => DapEvent::new(
                next_seq(),
                "thread",
                ThreadEventBody {
                    reason: "exited".to_owned(),
                    thread_id: *thread_id,
                },
            ),
            Event::VmDeath { .. } => {
                vm_died = true;
                continue;
            }
            Event::VmStart { .. } => {
                // Already handled by initialization flow.
                continue;
            }
            Event::ClassPrepare {
                signature,
                type_id,
                thread_id,
                ref_type_tag,
                ..
            } => {
                should_resume = true;
                resume_thread = Some(*thread_id);
                // Resolve deferred breakpoints for this class.
                let bp_events = session
                    .handle_class_prepare(signature, *type_id, *ref_type_tag)
                    .await;
                for bp_body in bp_events {
                    let bp_event = DapEvent::new(next_seq(), "breakpoint", bp_body);
                    let json = serialize_event(&bp_event);
                    write_message(writer, &json).await?;
                }

                continue;
            }
        };

        let json = serialize_event(&dap_event);
        write_message(writer, &json).await?;
    }

    if should_resume && !should_stop {
        if let Ok(conn) = session.conn() {
            let resume = match event_set.suspend_policy {
                SUSPEND_ALL => Some(hyperion_jdwp::commands::Command::vm_resume()),
                SUSPEND_EVENT_THREAD => resume_thread.filter(|id| *id != 0).map(|id| {
                    hyperion_jdwp::commands::Command::thread_resume(
                        id,
                        conn.id_sizes().object_id_size,
                    )
                }),
                SUSPEND_NONE => None,
                policy => {
                    tracing::error!("unknown JDWP suspend policy {policy}");
                    None
                }
            };
            if let Some(command) = resume {
                if let Err(error) = conn.send_command(command).await {
                    tracing::error!("failed to resume after JDWP event set: {error}");
                }
            } else if event_set.suspend_policy == SUSPEND_EVENT_THREAD {
                tracing::error!("JDWP event-thread resume lacked a thread ID");
            }
        }
    }

    Ok(vm_died)
}

#[allow(clippy::future_not_send)]
async fn dispatch_request(
    session: &mut DebugSession,
    request: &DapRequest,
    next_seq: &impl Fn() -> u64,
) -> DapResponse {
    match request.command.as_str() {
        "initialize" => {
            let args: InitializeArguments = serde_json::from_value(request.arguments.clone())
                .unwrap_or(InitializeArguments {
                    client_id: None,
                    client_name: None,
                    lines_start_at1: Some(true),
                    columns_start_at1: Some(true),
                    supports_variable_type: None,
                    supports_run_in_terminal_request: None,
                });
            let caps = session.handle_initialize(args);
            DapResponse::success(next_seq(), request, caps)
        }
        "launch" => match serde_json::from_value::<LaunchArguments>(request.arguments.clone()) {
            Ok(args) => match session.handle_launch(args).await {
                Ok(()) => DapResponse::success_empty(next_seq(), request),
                Err(e) => DapResponse::error(next_seq(), request, e),
            },
            Err(e) => DapResponse::error(next_seq(), request, format!("bad launch args: {e}")),
        },
        "attach" => match serde_json::from_value::<AttachArguments>(request.arguments.clone()) {
            Ok(args) => match session.handle_attach(args).await {
                Ok(()) => DapResponse::success_empty(next_seq(), request),
                Err(e) => DapResponse::error(next_seq(), request, e),
            },
            Err(e) => DapResponse::error(next_seq(), request, format!("bad attach args: {e}")),
        },
        "setBreakpoints" => {
            match serde_json::from_value::<SetBreakpointsArguments>(request.arguments.clone()) {
                Ok(args) => match session.handle_set_breakpoints(args).await {
                    Ok(body) => DapResponse::success(next_seq(), request, body),
                    Err(e) => DapResponse::error(next_seq(), request, e),
                },
                Err(e) => {
                    DapResponse::error(next_seq(), request, format!("bad setBreakpoints args: {e}"))
                }
            }
        }
        "configurationDone" => match session.handle_configuration_done().await {
            Ok(()) => DapResponse::success_empty(next_seq(), request),
            Err(e) => DapResponse::error(next_seq(), request, e),
        },
        "continue" => {
            match serde_json::from_value::<ContinueArguments>(request.arguments.clone()) {
                Ok(args) => match session.handle_continue(args).await {
                    Ok(body) => DapResponse::success(next_seq(), request, body),
                    Err(e) => DapResponse::error(next_seq(), request, e),
                },
                Err(e) => {
                    DapResponse::error(next_seq(), request, format!("bad continue args: {e}"))
                }
            }
        }
        "next" => match serde_json::from_value::<StepArguments>(request.arguments.clone()) {
            Ok(args) => match session.handle_next(args).await {
                Ok(()) => DapResponse::success_empty(next_seq(), request),
                Err(e) => DapResponse::error(next_seq(), request, e),
            },
            Err(e) => DapResponse::error(next_seq(), request, format!("bad next args: {e}")),
        },
        "stepIn" => match serde_json::from_value::<StepArguments>(request.arguments.clone()) {
            Ok(args) => match session.handle_step_in(args).await {
                Ok(()) => DapResponse::success_empty(next_seq(), request),
                Err(e) => DapResponse::error(next_seq(), request, e),
            },
            Err(e) => DapResponse::error(next_seq(), request, format!("bad stepIn args: {e}")),
        },
        "stepOut" => match serde_json::from_value::<StepArguments>(request.arguments.clone()) {
            Ok(args) => match session.handle_step_out(args).await {
                Ok(()) => DapResponse::success_empty(next_seq(), request),
                Err(e) => DapResponse::error(next_seq(), request, e),
            },
            Err(e) => DapResponse::error(next_seq(), request, format!("bad stepOut args: {e}")),
        },
        "threads" => match session.handle_threads().await {
            Ok(body) => DapResponse::success(next_seq(), request, body),
            Err(e) => DapResponse::error(next_seq(), request, e),
        },
        "stackTrace" => {
            match serde_json::from_value::<StackTraceArguments>(request.arguments.clone()) {
                Ok(args) => match session.handle_stack_trace(args).await {
                    Ok(body) => DapResponse::success(next_seq(), request, body),
                    Err(e) => DapResponse::error(next_seq(), request, e),
                },
                Err(e) => {
                    DapResponse::error(next_seq(), request, format!("bad stackTrace args: {e}"))
                }
            }
        }
        "scopes" => match serde_json::from_value::<ScopesArguments>(request.arguments.clone()) {
            Ok(args) => match session.handle_scopes(args) {
                Ok(body) => DapResponse::success(next_seq(), request, body),
                Err(e) => DapResponse::error(next_seq(), request, e),
            },
            Err(e) => DapResponse::error(next_seq(), request, format!("bad scopes args: {e}")),
        },
        "variables" => {
            match serde_json::from_value::<VariablesArguments>(request.arguments.clone()) {
                Ok(args) => match session.handle_variables(args).await {
                    Ok(body) => DapResponse::success(next_seq(), request, body),
                    Err(e) => DapResponse::error(next_seq(), request, e),
                },
                Err(e) => {
                    DapResponse::error(next_seq(), request, format!("bad variables args: {e}"))
                }
            }
        }
        "setExceptionBreakpoints" => {
            match serde_json::from_value::<SetExceptionBreakpointsArguments>(
                request.arguments.clone(),
            ) {
                Ok(args) => match session.handle_set_exception_breakpoints(args).await {
                    Ok(()) => DapResponse::success_empty(next_seq(), request),
                    Err(e) => DapResponse::error(next_seq(), request, e),
                },
                Err(e) => DapResponse::error(
                    next_seq(),
                    request,
                    format!("bad setExceptionBreakpoints args: {e}"),
                ),
            }
        }
        "exceptionInfo" => {
            match serde_json::from_value::<ExceptionInfoArguments>(request.arguments.clone()) {
                Ok(args) => match session.handle_exception_info(args).await {
                    Ok(body) => DapResponse::success(next_seq(), request, body),
                    Err(e) => DapResponse::error(next_seq(), request, e),
                },
                Err(e) => {
                    DapResponse::error(next_seq(), request, format!("bad exceptionInfo args: {e}"))
                }
            }
        }
        "evaluate" => {
            match serde_json::from_value::<EvaluateArguments>(request.arguments.clone()) {
                Ok(args) => match session.handle_evaluate(args).await {
                    Ok(body) => DapResponse::success(next_seq(), request, body),
                    Err(e) => DapResponse::error(next_seq(), request, e),
                },
                Err(e) => {
                    DapResponse::error(next_seq(), request, format!("bad evaluate args: {e}"))
                }
            }
        }
        "terminate" => match session.handle_terminate().await {
            Ok(()) => DapResponse::success_empty(next_seq(), request),
            Err(e) => DapResponse::error(next_seq(), request, e),
        },
        "disconnect" => {
            let args = serde_json::from_value::<DisconnectArguments>(request.arguments.clone())
                .unwrap_or(DisconnectArguments {
                    restart: None,
                    terminate_debuggee: Some(true),
                });
            session.handle_disconnect(args).await;
            DapResponse::success_empty(next_seq(), request)
        }
        other => DapResponse::error(next_seq(), request, format!("unsupported command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn dap_frame_survives_an_interleaved_event() {
        // A one-byte pipe forces the reader to consume the header and most of
        // the body before the write returns, then pauses it mid-frame.
        let (mut client, server) = tokio::io::duplex(1);
        let mut requests = spawn_request_stream(BufReader::new(server));
        client
            .write_all(b"Content-Length: 2\r\n\r\n{")
            .await
            .unwrap();

        let (event_tx, mut event_rx) = mpsc::channel(1);
        event_tx.send(()).await.unwrap();
        tokio::select! {
            message = requests.receiver.recv() => panic!("partial DAP frame completed early: {message:?}"),
            _ = event_rx.recv() => {}
        }

        client.write_all(b"}").await.unwrap();
        assert_eq!(
            requests.receiver.recv().await.unwrap().unwrap(),
            Some("{}".to_owned())
        );
    }

    #[tokio::test]
    async fn grouped_class_prepare_events_resume_only_once() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut handshake = [0; 14];
            stream.read_exact(&mut handshake).await.unwrap();
            assert_eq!(&handshake, hyperion_jdwp::packet::HANDSHAKE);
            stream.write_all(&handshake).await.unwrap();

            // VirtualMachine.IDSizes is the mandatory first request.
            let mut request = [0; 11];
            stream.read_exact(&mut request).await.unwrap();
            assert_eq!(&request[9..11], &[1, 7]);
            let mut reply = Vec::from(31u32.to_be_bytes());
            reply.extend_from_slice(&request[4..8]);
            reply.push(0x80);
            reply.extend_from_slice(&0u16.to_be_bytes());
            for _ in 0..5 {
                reply.extend_from_slice(&8i32.to_be_bytes());
            }
            stream.write_all(&reply).await.unwrap();

            let mut header = [0; 11];
            stream.read_exact(&mut header).await.unwrap();
            assert_eq!(&header[9..11], &[1, 9], "expected VirtualMachine.Resume");
            let mut resume_reply = Vec::from(11u32.to_be_bytes());
            resume_reply.extend_from_slice(&header[4..8]);
            resume_reply.push(0x80);
            resume_reply.extend_from_slice(&0u16.to_be_bytes());
            stream.write_all(&resume_reply).await.unwrap();
            matches!(
                tokio::time::timeout(
                    std::time::Duration::from_millis(300),
                    stream.read_exact(&mut header),
                )
                .await,
                Ok(Ok(_))
            )
        });

        let mut session = DebugSession::new();
        session
            .handle_attach(AttachArguments {
                host: "127.0.0.1".to_owned(),
                port,
                project_root: None,
            })
            .await
            .unwrap();
        let events = EventSet {
            suspend_policy: SUSPEND_ALL,
            events: vec![
                Event::ClassPrepare {
                    request_id: 1,
                    thread_id: 1,
                    ref_type_tag: 1,
                    type_id: 2,
                    signature: "LExample;".to_owned(),
                    status: 2,
                },
                Event::ClassPrepare {
                    request_id: 3,
                    thread_id: 1,
                    ref_type_tag: 1,
                    type_id: 2,
                    signature: "LExample;".to_owned(),
                    status: 2,
                },
            ],
        };
        let mut output = Vec::new();
        handle_jdwp_event_set(&mut session, &events, &mut output, &|| 1)
            .await
            .unwrap();
        assert!(
            !peer.await.unwrap(),
            "grouped ClassPrepare events resumed twice"
        );
        assert_eq!(output, b"");
    }
}
