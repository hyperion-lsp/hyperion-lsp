// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::JdwpError;

/// JDWP packet header size: length(4) + id(4) + flags(1) + command_set(1) + command(1) = 11
pub const HEADER_SIZE: usize = 11;

/// Maximum packet size accepted from or sent to a debuggee.
///
/// The wire format can describe packets up to 4 GiB, but reserving that much memory based on an
/// untrusted length prefix can terminate the adapter. Real debugger traffic stays well below this
/// limit, including large array and string replies.
pub const MAX_PACKET_SIZE: usize = 64 * 1024 * 1024;

/// Flag byte indicating a reply packet.
pub const REPLY_FLAG: u8 = 0x80;

/// The handshake string exchanged between client and debuggee.
pub const HANDSHAKE: &[u8; 14] = b"JDWP-Handshake";

/// A raw JDWP packet — either a command or a reply.
#[derive(Debug, Clone)]
pub enum Packet {
    Command(CommandPacket),
    Reply(ReplyPacket),
}

#[derive(Debug, Clone)]
pub struct CommandPacket {
    pub id: u32,
    pub command_set: u8,
    pub command: u8,
    pub data: BytesMut,
}

#[derive(Debug, Clone)]
pub struct ReplyPacket {
    pub id: u32,
    pub error_code: u16,
    pub data: BytesMut,
}

impl Packet {
    pub const fn id(&self) -> u32 {
        match self {
            Self::Command(c) => c.id,
            Self::Reply(r) => r.id,
        }
    }
}

/// Codec for JDWP binary packet framing over TCP.
///
/// Each packet starts with a 4-byte big-endian length field (includes the header).
/// The flags byte at offset 8 distinguishes commands (0x00) from replies (0x80).
pub struct JdwpCodec;

impl Decoder for JdwpCodec {
    type Item = Packet;
    type Error = JdwpError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Packet>, JdwpError> {
        if src.len() < 4 {
            return Ok(None);
        }

        // Peek at the length without consuming.
        let length = u32::from_be_bytes([src[0], src[1], src[2], src[3]]) as usize;

        if length < HEADER_SIZE {
            return Err(JdwpError::BadReply(format!(
                "packet length {length} is smaller than header size {HEADER_SIZE}"
            )));
        }

        if length > MAX_PACKET_SIZE {
            return Err(JdwpError::PacketTooLarge {
                length,
                max: MAX_PACKET_SIZE,
            });
        }

        if src.len() < length {
            // Reserve space for the rest of the packet.
            src.reserve(length - src.len());
            return Ok(None);
        }

        // We have a full packet. Consume it.
        let mut packet_buf = src.split_to(length);

        let _length = packet_buf.get_u32();
        let id = packet_buf.get_u32();
        let flags = packet_buf.get_u8();

        if flags & REPLY_FLAG != 0 {
            let error_code = packet_buf.get_u16();
            Ok(Some(Packet::Reply(ReplyPacket {
                id,
                error_code,
                data: packet_buf,
            })))
        } else {
            let command_set = packet_buf.get_u8();
            let command = packet_buf.get_u8();
            Ok(Some(Packet::Command(CommandPacket {
                id,
                command_set,
                command,
                data: packet_buf,
            })))
        }
    }
}

impl Encoder<CommandPacket> for JdwpCodec {
    type Error = JdwpError;

    fn encode(&mut self, item: CommandPacket, dst: &mut BytesMut) -> Result<(), JdwpError> {
        let packet_size = checked_packet_size(item.data.len())?;
        let length = packet_size as u32;
        dst.reserve(packet_size);
        dst.put_u32(length);
        dst.put_u32(item.id);
        dst.put_u8(0x00); // flags: command
        dst.put_u8(item.command_set);
        dst.put_u8(item.command);
        dst.put(item.data);
        Ok(())
    }
}

fn checked_packet_size(payload_size: usize) -> Result<usize, JdwpError> {
    let length = HEADER_SIZE
        .checked_add(payload_size)
        .ok_or(JdwpError::PacketTooLarge {
            length: usize::MAX,
            max: MAX_PACKET_SIZE,
        })?;
    if length > MAX_PACKET_SIZE {
        return Err(JdwpError::PacketTooLarge {
            length,
            max: MAX_PACKET_SIZE,
        });
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_reply_packet() {
        let mut codec = JdwpCodec;
        let mut buf = BytesMut::new();

        // Build a reply packet: length=15, id=1, flags=0x80, error=0, data=[0xCA,0xFE,0xBA,0xBE]
        buf.put_u32(15); // length
        buf.put_u32(1); // id
        buf.put_u8(0x80); // flags (reply)
        buf.put_u16(0); // error code
        buf.put_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);

        let packet = codec.decode(&mut buf).unwrap().unwrap();
        match packet {
            Packet::Reply(r) => {
                assert_eq!(r.id, 1);
                assert_eq!(r.error_code, 0);
                assert_eq!(&r.data[..], &[0xCA, 0xFE, 0xBA, 0xBE]);
            }
            _ => panic!("expected reply packet"),
        }
    }

    #[test]
    fn decode_command_packet() {
        let mut codec = JdwpCodec;
        let mut buf = BytesMut::new();

        // Command packet: length=14, id=2, flags=0, cmd_set=64, cmd=100, data=[0x01,0x02,0x03]
        buf.put_u32(14);
        buf.put_u32(2);
        buf.put_u8(0x00);
        buf.put_u8(64);
        buf.put_u8(100);
        buf.put_slice(&[0x01, 0x02, 0x03]);

        let packet = codec.decode(&mut buf).unwrap().unwrap();
        match packet {
            Packet::Command(c) => {
                assert_eq!(c.id, 2);
                assert_eq!(c.command_set, 64);
                assert_eq!(c.command, 100);
                assert_eq!(&c.data[..], &[0x01, 0x02, 0x03]);
            }
            _ => panic!("expected command packet"),
        }
    }

    #[test]
    fn decode_partial_then_complete() {
        let mut codec = JdwpCodec;
        let mut buf = BytesMut::new();

        // Only send length field first
        buf.put_u32(11);
        assert!(codec.decode(&mut buf).unwrap().is_none());

        // Now send the rest (minimal packet, no data)
        buf.put_u32(1); // id
        buf.put_u8(0x80); // flags
        buf.put_u16(0); // error_code

        let packet = codec.decode(&mut buf).unwrap().unwrap();
        assert!(matches!(packet, Packet::Reply(_)));
    }

    #[test]
    fn encode_command_packet() {
        let mut codec = JdwpCodec;
        let mut buf = BytesMut::new();

        let cmd = CommandPacket {
            id: 42,
            command_set: 1,
            command: 7,
            data: BytesMut::from(&[0xAB, 0xCD][..]),
        };

        codec.encode(cmd, &mut buf).unwrap();

        assert_eq!(buf.len(), 13); // 11 header + 2 data
        assert_eq!(&buf[0..4], &13u32.to_be_bytes()); // length
        assert_eq!(&buf[4..8], &42u32.to_be_bytes()); // id
        assert_eq!(buf[8], 0x00); // flags
        assert_eq!(buf[9], 1); // command_set
        assert_eq!(buf[10], 7); // command
        assert_eq!(&buf[11..], &[0xAB, 0xCD]); // data
    }

    #[test]
    fn decode_rejects_oversized_packet_before_reserving_payload() {
        let mut codec = JdwpCodec;
        let mut buf = BytesMut::from(&(MAX_PACKET_SIZE as u32 + 1).to_be_bytes()[..]);

        let error = codec.decode(&mut buf).unwrap_err();
        assert!(matches!(
            error,
            JdwpError::PacketTooLarge {
                length,
                max: MAX_PACKET_SIZE
            } if length == MAX_PACKET_SIZE + 1
        ));
        assert_eq!(buf.len(), 4);
    }

    #[test]
    fn packet_size_checks_overflow_and_limit() {
        assert_eq!(checked_packet_size(0).unwrap(), HEADER_SIZE);
        assert_eq!(
            checked_packet_size(MAX_PACKET_SIZE - HEADER_SIZE).unwrap(),
            MAX_PACKET_SIZE
        );
        assert!(matches!(
            checked_packet_size(MAX_PACKET_SIZE),
            Err(JdwpError::PacketTooLarge { .. })
        ));
        assert!(matches!(
            checked_packet_size(usize::MAX),
            Err(JdwpError::PacketTooLarge { .. })
        ));
    }
}
