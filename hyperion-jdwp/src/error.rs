// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum JdwpError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JDWP handshake failed")]
    HandshakeFailed,

    #[error("JDWP protocol error: {0}")]
    Protocol(JdwpErrorCode),

    #[error("connection closed")]
    ConnectionClosed,

    #[error("request timed out")]
    Timeout,

    #[error("JDWP packet is too large: {length} bytes (maximum {max})")]
    PacketTooLarge { length: usize, max: usize },

    #[error("unexpected reply data: {0}")]
    BadReply(String),

    #[error("unsupported event kind: {0}")]
    UnsupportedEvent(u8),
}

pub type Result<T> = std::result::Result<T, JdwpError>;

/// Human-readable name for JDWP error codes.
const fn jdwp_error_name(code: u16) -> &'static str {
    match code {
        10 => "INVALID_THREAD",
        11 => "INVALID_THREAD_GROUP",
        12 => "INVALID_PRIORITY",
        13 => "THREAD_NOT_SUSPENDED",
        14 => "THREAD_NOT_ALIVE",
        20 => "INVALID_OBJECT",
        21 => "INVALID_CLASS",
        22 => "CLASS_NOT_PREPARED",
        23 => "INVALID_METHODID",
        24 => "INVALID_LOCATION",
        25 => "INVALID_FIELDID",
        30 => "INVALID_FRAMEID",
        31 => "NO_MORE_FRAMES",
        32 => "OPAQUE_FRAME",
        33 => "NOT_CURRENT_FRAME",
        34 => "TYPE_MISMATCH",
        35 => "INVALID_SLOT",
        40 => "DUPLICATE",
        41 => "NOT_FOUND",
        50 => "INVALID_MONITOR",
        51 => "NOT_MONITOR_OWNER",
        52 => "INTERRUPT",
        60 => "INVALID_CLASS_FORMAT",
        61 => "CIRCULAR_CLASS_DEFINITION",
        62 => "FAILS_VERIFICATION",
        63 => "ADD_METHOD_NOT_IMPLEMENTED",
        64 => "SCHEMA_CHANGE_NOT_IMPLEMENTED",
        65 => "INVALID_TYPESTATE",
        66 => "HIERARCHY_CHANGE_NOT_IMPLEMENTED",
        67 => "DELETE_METHOD_NOT_IMPLEMENTED",
        68 => "UNSUPPORTED_VERSION",
        69 => "NAMES_DONT_MATCH",
        70 => "CLASS_MODIFIERS_CHANGE_NOT_IMPLEMENTED",
        71 => "METHOD_MODIFIERS_CHANGE_NOT_IMPLEMENTED",
        99 => "NOT_IMPLEMENTED",
        100 => "NULL_POINTER",
        101 => "ABSENT_INFORMATION",
        102 => "INVALID_EVENT_TYPE",
        103 => "ILLEGAL_ARGUMENT",
        110 => "OUT_OF_MEMORY",
        111 => "ACCESS_DENIED",
        112 => "VM_DEAD",
        113 => "INTERNAL",
        115 => "UNATTACHED_THREAD",
        500 => "INVALID_TAG",
        502 => "ALREADY_INVOKING",
        503 => "INVALID_INDEX",
        504 => "INVALID_LENGTH",
        506 => "INVALID_STRING",
        507 => "INVALID_CLASS_LOADER",
        508 => "INVALID_ARRAY",
        509 => "TRANSPORT_LOAD",
        510 => "TRANSPORT_INIT",
        511 => "NATIVE_METHOD",
        512 => "INVALID_COUNT",
        _ => "UNKNOWN",
    }
}

/// Displayable JDWP error code with name.
#[derive(Debug, Clone, Copy)]
pub struct JdwpErrorCode(pub u16);

impl fmt::Display for JdwpErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", jdwp_error_name(self.0), self.0)
    }
}
