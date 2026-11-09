// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use bytes::BytesMut;

use crate::commands::{IdSizes, SUSPEND_ALL, SUSPEND_EVENT_THREAD, SUSPEND_NONE};
use crate::error::{JdwpError, Result};
use crate::replies::{get_count, get_i32, get_id, get_string, get_u32, get_u8, Location};

/// A set of events delivered by the JVM in a single composite event packet.
#[derive(Debug, Clone)]
pub struct EventSet {
    pub suspend_policy: u8,
    pub events: Vec<Event>,
}

/// Individual JDWP event.
#[derive(Debug, Clone)]
pub enum Event {
    VmStart {
        request_id: u32,
        thread_id: u64,
    },
    VmDeath {
        request_id: u32,
    },
    SingleStep {
        request_id: u32,
        thread_id: u64,
        location: Location,
    },
    Breakpoint {
        request_id: u32,
        thread_id: u64,
        location: Location,
    },
    Exception {
        request_id: u32,
        thread_id: u64,
        throw_location: Location,
        exception_id: u64,
        catch_location: Location,
    },
    ThreadStart {
        request_id: u32,
        thread_id: u64,
    },
    ThreadDeath {
        request_id: u32,
        thread_id: u64,
    },
    ClassPrepare {
        request_id: u32,
        thread_id: u64,
        ref_type_tag: u8,
        type_id: u64,
        signature: String,
        status: i32,
    },
}

impl EventSet {
    /// Parse an event composite command (command_set=64, command=100).
    pub fn parse(buf: &mut BytesMut, id_sizes: &IdSizes) -> Result<Self> {
        let suspend_policy = get_u8(buf, "event suspend policy")?;
        if !matches!(
            suspend_policy,
            SUSPEND_NONE | SUSPEND_EVENT_THREAD | SUSPEND_ALL
        ) {
            return Err(JdwpError::BadReply(format!(
                "invalid event suspend policy: {suspend_policy}"
            )));
        }
        let count = get_count(buf, "event", 1)?;
        let mut events = Vec::with_capacity(count);

        for _ in 0..count {
            let event_kind = get_u8(buf, "event kind")?;
            let event = parse_event(buf, event_kind, *id_sizes)?;
            events.push(event);
        }

        Ok(Self {
            suspend_policy,
            events,
        })
    }
}

fn parse_event(buf: &mut BytesMut, kind: u8, id_sizes: IdSizes) -> Result<Event> {
    match kind {
        // VM_START = 90
        90 => {
            let request_id = get_u32(buf, "VM start request ID")?;
            let thread_id = get_id(buf, id_sizes.object_id_size)?;
            Ok(Event::VmStart {
                request_id,
                thread_id,
            })
        }
        // SINGLE_STEP = 1
        1 => {
            let request_id = get_u32(buf, "single-step request ID")?;
            let thread_id = get_id(buf, id_sizes.object_id_size)?;
            let location = Location::parse(buf, &id_sizes)?;
            Ok(Event::SingleStep {
                request_id,
                thread_id,
                location,
            })
        }
        // BREAKPOINT = 2
        2 => {
            let request_id = get_u32(buf, "breakpoint request ID")?;
            let thread_id = get_id(buf, id_sizes.object_id_size)?;
            let location = Location::parse(buf, &id_sizes)?;
            Ok(Event::Breakpoint {
                request_id,
                thread_id,
                location,
            })
        }
        // EXCEPTION = 4
        4 => {
            let request_id = get_u32(buf, "exception request ID")?;
            let thread_id = get_id(buf, id_sizes.object_id_size)?;
            let throw_location = Location::parse(buf, &id_sizes)?;
            // Per the JDWP spec, `exception` here is a *tagged*-objectID (a
            // 1-byte type tag followed by the object ID), unlike the plain
            // objectIDs used elsewhere in this event (e.g. `thread_id`
            // above). Skipping the tag byte previously went unnoticed
            // because nothing read `exception_id`; it also silently shifted
            // every field after it (including `catch_location`) by one byte.
            let _exception_tag = get_u8(buf, "exception object tag")?;
            let exception_id = get_id(buf, id_sizes.object_id_size)?;
            let catch_location = Location::parse(buf, &id_sizes)?;
            Ok(Event::Exception {
                request_id,
                thread_id,
                throw_location,
                exception_id,
                catch_location,
            })
        }
        // THREAD_START = 6
        6 => {
            let request_id = get_u32(buf, "thread-start request ID")?;
            let thread_id = get_id(buf, id_sizes.object_id_size)?;
            Ok(Event::ThreadStart {
                request_id,
                thread_id,
            })
        }
        // THREAD_DEATH = 7
        7 => {
            let request_id = get_u32(buf, "thread-death request ID")?;
            let thread_id = get_id(buf, id_sizes.object_id_size)?;
            Ok(Event::ThreadDeath {
                request_id,
                thread_id,
            })
        }
        // CLASS_PREPARE = 8
        8 => {
            let request_id = get_u32(buf, "class-prepare request ID")?;
            let thread_id = get_id(buf, id_sizes.object_id_size)?;
            let ref_type_tag = get_u8(buf, "class-prepare reference type tag")?;
            let type_id = get_id(buf, id_sizes.reference_type_id_size)?;
            let signature = get_string(buf)?;
            let status = get_i32(buf, "class-prepare status")?;
            Ok(Event::ClassPrepare {
                request_id,
                thread_id,
                ref_type_tag,
                type_id,
                signature,
                status,
            })
        }
        // VM_DEATH = 99
        99 => {
            let request_id = get_u32(buf, "VM death request ID")?;
            Ok(Event::VmDeath { request_id })
        }
        _ => Err(JdwpError::UnsupportedEvent(kind)),
    }
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use super::*;

    #[test]
    fn event_set_rejects_negative_count() {
        let mut buf = BytesMut::new();
        buf.put_u8(0);
        buf.put_i32(-1);

        assert!(matches!(
            EventSet::parse(&mut buf, &IdSizes::default()),
            Err(JdwpError::BadReply(message)) if message.contains("negative event count")
        ));
    }

    #[test]
    fn event_set_rejects_unknown_suspend_policy() {
        let mut buf = BytesMut::new();
        buf.put_u8(3);
        buf.put_i32(0);
        assert!(matches!(
            EventSet::parse(&mut buf, &IdSizes::default()),
            Err(JdwpError::BadReply(message)) if message.contains("suspend policy")
        ));
    }

    #[test]
    fn event_set_rejects_truncated_fixed_fields_without_panicking() {
        let mut buf = BytesMut::new();
        buf.put_u8(0);
        buf.put_i32(1);
        buf.put_u8(99); // VM_DEATH requires a four-byte request ID.
        buf.put_u8(1);

        assert!(matches!(
            EventSet::parse(&mut buf, &IdSizes::default()),
            Err(JdwpError::BadReply(message)) if message.contains("VM death request ID")
        ));
    }
}
