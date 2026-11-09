// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bounded, non-blocking forwarding of launched debuggee output to the DAP loop.

use tokio::sync::mpsc;

/// Text the debuggee printed, kept (bounded) so a JVM that dies before the
/// debugger connects can be reported with its actual output.
#[derive(Clone, Default)]
pub(super) struct OutputCapture(std::sync::Arc<std::sync::Mutex<String>>);

const CAPTURE_LIMIT_BYTES: usize = 64 * 1024;

impl OutputCapture {
    fn push(&self, label: &str, text: &str) {
        let Ok(mut captured) = self.0.lock() else {
            return;
        };
        if captured.len() >= CAPTURE_LIMIT_BYTES {
            return;
        }
        // The JDWP agent announces its port on stdout; that is noise here.
        if label == "stdout" && text.trim_start().starts_with("Listening for transport") {
            return;
        }
        captured.push_str(text);
    }

    pub(super) fn text(&self) -> String {
        // A poisoned lock still holds the captured text; the writer's panic does not
        // make earlier output unreadable.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// A debuggee stdout/stderr chunk and its DAP output category.
pub type OutputChunk = (&'static str, String);

/// Enough buffering for bursts without letting a slow DAP client grow memory without bound.
pub(super) const OUTPUT_CHANNEL_CAPACITY: usize = 1024;
const OUTPUT_CHUNK_BYTES: usize = 8 * 1024;

/// Drain a process pipe continuously so a verbose child cannot block on a full OS pipe.
pub(super) async fn drain_pipe(
    mut pipe: impl tokio::io::AsyncRead + Unpin,
    label: &'static str,
    output_tx: mpsc::Sender<OutputChunk>,
    capture: OutputCapture,
) {
    use tokio::io::AsyncReadExt;

    let mut buffer = [0_u8; OUTPUT_CHUNK_BYTES];
    let mut incomplete_utf8 = Vec::with_capacity(4);
    let mut dropped = 0_usize;

    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => {
                if !incomplete_utf8.is_empty() {
                    let output = String::from_utf8_lossy(&incomplete_utf8).into_owned();
                    capture.push(label, &output);
                    let _ = try_forward_output(&output_tx, label, output, &mut dropped);
                }
                break;
            }
            Ok(read) => {
                incomplete_utf8.extend_from_slice(&buffer[..read]);
                let output = match std::str::from_utf8(&incomplete_utf8) {
                    Ok(_) => String::from_utf8(std::mem::take(&mut incomplete_utf8))
                        .expect("validated UTF-8"),
                    Err(error) if error.error_len().is_none() => {
                        let remainder = incomplete_utf8.split_off(error.valid_up_to());
                        let valid = std::mem::replace(&mut incomplete_utf8, remainder);
                        String::from_utf8(valid).expect("validated UTF-8 prefix")
                    }
                    Err(_) => {
                        String::from_utf8_lossy(&std::mem::take(&mut incomplete_utf8)).into_owned()
                    }
                };

                if !output.is_empty() {
                    tracing::info!("debuggee {label}: {output}");
                    capture.push(label, &output);
                    // The JDWP agent's own banner is not the program's output.
                    if label == "stdout"
                        && output.trim_start().starts_with("Listening for transport")
                    {
                        continue;
                    }
                    if !try_forward_output(&output_tx, label, output, &mut dropped) {
                        break;
                    }
                }
            }
            Err(error) => {
                tracing::warn!("error reading debuggee {label}: {error}");
                break;
            }
        }
    }
}

/// Forward output without waiting for the client, reporting dropped chunks once capacity returns.
fn try_forward_output(
    output_tx: &mpsc::Sender<OutputChunk>,
    label: &'static str,
    output: String,
    dropped: &mut usize,
) -> bool {
    if *dropped > 0 {
        let notice = format!(
            "[hyperion: dropped {dropped} debuggee output chunks because the DAP client was slow]\n"
        );
        match output_tx.try_send((label, notice)) {
            Ok(()) => *dropped = 0,
            Err(mpsc::error::TrySendError::Full(_)) => {
                *dropped = dropped.saturating_add(1);
                return true;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => return false,
        }
    }

    match output_tx.try_send((label, output)) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            *dropped = dropped.saturating_add(1);
            true
        }
        Err(mpsc::error::TrySendError::Closed(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_bounded_and_reports_drops() {
        let (tx, mut rx) = mpsc::channel(2);
        let mut dropped = 0;

        assert!(try_forward_output(
            &tx,
            "stdout",
            "one".to_owned(),
            &mut dropped
        ));
        assert!(try_forward_output(
            &tx,
            "stdout",
            "two".to_owned(),
            &mut dropped
        ));
        assert!(try_forward_output(
            &tx,
            "stdout",
            "dropped".to_owned(),
            &mut dropped
        ));
        assert_eq!(dropped, 1);

        assert_eq!(rx.try_recv().unwrap().1, "one");
        assert_eq!(rx.try_recv().unwrap().1, "two");
        assert!(try_forward_output(
            &tx,
            "stdout",
            "after".to_owned(),
            &mut dropped
        ));
        assert_eq!(dropped, 0);
        assert!(rx.try_recv().unwrap().1.contains("dropped 1"));
        assert_eq!(rx.try_recv().unwrap().1, "after");
    }

    #[tokio::test]
    async fn drain_preserves_utf8_across_chunk_boundaries() {
        use tokio::io::AsyncWriteExt;

        let expected = format!("{}€tail", "a".repeat(OUTPUT_CHUNK_BYTES - 1));
        let (mut writer, reader) = tokio::io::duplex(expected.len());
        let bytes = expected.as_bytes().to_vec();
        let writer_task = tokio::spawn(async move {
            writer.write_all(&bytes).await.unwrap();
        });
        let (tx, mut rx) = mpsc::channel(4);

        drain_pipe(reader, "stdout", tx, OutputCapture::default()).await;
        writer_task.await.unwrap();

        let mut actual = String::new();
        while let Some((_label, output)) = rx.recv().await {
            actual.push_str(&output);
        }
        assert_eq!(actual, expected);
    }
}
