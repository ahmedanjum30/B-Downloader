//! Just enough protobuf to read and write YouTube's streaming messages,
//! without generated code: messages are lists of fields in wire order, so
//! fields this code doesn't know about pass through untouched.

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Varint(u64),
    /// Length-delimited: bytes, a string or a nested message.
    Bytes(Vec<u8>),
    Fixed64([u8; 8]),
    Fixed32([u8; 4]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Field {
    pub number: u32,
    pub value: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Message {
    pub fields: Vec<Field>,
}

fn bad() -> Error {
    Error::Unsupported("YouTube sent a message that couldn't be read".into())
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *buf.get(*pos).ok_or_else(bad)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(bad())
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

impl Message {
    pub fn decode(buf: &[u8]) -> Result<Self> {
        let mut fields = Vec::new();
        let mut pos = 0;
        while pos < buf.len() {
            let key = read_varint(buf, &mut pos)?;
            let number = u32::try_from(key >> 3).map_err(|_| bad())?;
            let value = match key & 7 {
                0 => Value::Varint(read_varint(buf, &mut pos)?),
                1 => {
                    let bytes = buf.get(pos..pos + 8).ok_or_else(bad)?;
                    pos += 8;
                    Value::Fixed64(bytes.try_into().expect("8 bytes"))
                }
                2 => {
                    let len = usize::try_from(read_varint(buf, &mut pos)?).map_err(|_| bad())?;
                    let bytes = buf.get(pos..pos.checked_add(len).ok_or_else(bad)?).ok_or_else(bad)?;
                    pos += len;
                    Value::Bytes(bytes.to_vec())
                }
                5 => {
                    let bytes = buf.get(pos..pos + 4).ok_or_else(bad)?;
                    pos += 4;
                    Value::Fixed32(bytes.try_into().expect("4 bytes"))
                }
                _ => return Err(bad()),
            };
            fields.push(Field { number, value });
        }
        Ok(Self { fields })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for field in &self.fields {
            let wire = match field.value {
                Value::Varint(_) => 0,
                Value::Fixed64(_) => 1,
                Value::Bytes(_) => 2,
                Value::Fixed32(_) => 5,
            };
            write_varint(&mut out, (u64::from(field.number) << 3) | wire);
            match &field.value {
                Value::Varint(v) => write_varint(&mut out, *v),
                Value::Bytes(b) => {
                    write_varint(&mut out, b.len() as u64);
                    out.extend_from_slice(b);
                }
                Value::Fixed64(b) => out.extend_from_slice(b),
                Value::Fixed32(b) => out.extend_from_slice(b),
            }
        }
        out
    }

    pub fn varint(&self, number: u32) -> Option<u64> {
        self.fields.iter().find_map(|f| match f.value {
            Value::Varint(v) if f.number == number => Some(v),
            _ => None,
        })
    }

    pub fn bytes(&self, number: u32) -> Option<&[u8]> {
        self.fields.iter().find_map(|f| match &f.value {
            Value::Bytes(b) if f.number == number => Some(b.as_slice()),
            _ => None,
        })
    }

    pub fn string(&self, number: u32) -> Option<String> {
        self.bytes(number).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    pub fn message(&self, number: u32) -> Option<Message> {
        self.bytes(number).and_then(|b| Message::decode(b).ok())
    }

    pub fn remove(&mut self, number: u32) {
        self.fields.retain(|f| f.number != number);
    }

    /// Replaces every field with this number by one with `value`.
    pub fn set(&mut self, number: u32, value: Value) {
        self.remove(number);
        self.fields.push(Field { number, value });
    }

    pub fn push(&mut self, number: u32, value: Value) -> &mut Self {
        self.fields.push(Field { number, value });
        self
    }

    pub fn push_varint(&mut self, number: u32, value: u64) -> &mut Self {
        self.push(number, Value::Varint(value))
    }

    pub fn push_bytes(&mut self, number: u32, value: impl Into<Vec<u8>>) -> &mut Self {
        self.push(number, Value::Bytes(value.into()))
    }

    pub fn push_message(&mut self, number: u32, value: &Message) -> &mut Self {
        self.push(number, Value::Bytes(value.encode()))
    }
}

/// YouTube's own number format in UMP framing (not protobuf's): the first
/// byte's high bits say how many bytes follow.
fn read_ump_varint(buf: &[u8], pos: usize) -> Option<(u64, usize)> {
    let first = *buf.get(pos)?;
    let len = match first {
        0..=127 => 1,
        128..=191 => 2,
        192..=223 => 3,
        224..=239 => 4,
        _ => 5,
    };
    let bytes = buf.get(pos..pos + len)?;
    let value = match len {
        1 => u64::from(first),
        2 => u64::from(first & 0x3f) | u64::from(bytes[1]) << 6,
        3 => u64::from(first & 0x1f) | u64::from(bytes[1]) << 5 | u64::from(bytes[2]) << 13,
        4 => u64::from(first & 0x0f) | u64::from(bytes[1]) << 4 | u64::from(bytes[2]) << 12 | u64::from(bytes[3]) << 20,
        _ => u64::from(u32::from_le_bytes(bytes[1..5].try_into().expect("4 bytes"))),
    };
    Some((value, pos + len))
}

/// Splits a UMP stream into its parts, `(type, payload)`. Returns the parts
/// and how many bytes they used; the rest is an incomplete part that needs
/// more data.
pub(crate) fn ump_parts(buf: &[u8]) -> (Vec<(u64, &[u8])>, usize) {
    let mut parts = Vec::new();
    let mut pos = 0;
    while let Some((kind, after_kind)) = read_ump_varint(buf, pos)
        && let Some((size, start)) = read_ump_varint(buf, after_kind)
        && let Ok(size) = usize::try_from(size)
        && let Some(payload) = buf.get(start..start.saturating_add(size))
    {
        parts.push((kind, payload));
        pos = start + size;
    }
    (parts, pos)
}

#[cfg(test)]
pub(crate) fn write_ump_part(out: &mut Vec<u8>, kind: u64, payload: &[u8]) {
    // The five-byte form is valid for every number, which keeps tests simple.
    for value in [kind, payload.len() as u64] {
        out.push(0xf0);
        out.extend_from_slice(&(value as u32).to_le_bytes());
    }
    out.extend_from_slice(payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_including_unknown_fields() {
        let mut inner = Message::default();
        inner.push_varint(1, 300).push_bytes(3, b"xtags".to_vec());
        let mut msg = Message::default();
        msg.push_varint(28, u64::MAX)
            .push_message(17, &inner)
            .push(5, Value::Fixed32([1, 2, 3, 4]))
            .push_bytes(19, vec![]);
        let decoded = Message::decode(&msg.encode()).unwrap();
        assert_eq!(decoded, msg);
        assert_eq!(decoded.varint(28), Some(u64::MAX));
        assert_eq!(decoded.message(17).unwrap().string(3).as_deref(), Some("xtags"));
    }

    #[test]
    fn truncated_messages_are_errors() {
        let mut msg = Message::default();
        msg.push_bytes(5, vec![7; 10]);
        let bytes = msg.encode();
        assert!(Message::decode(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn ump_parts_split_and_wait_for_more() {
        let mut stream = Vec::new();
        write_ump_part(&mut stream, 20, b"header");
        // The short forms: type 21 in one byte, size 200 in two.
        stream.extend_from_slice(&[21, 0x80 | (200 & 0x3f), (200 >> 6) as u8]);
        stream.extend_from_slice(&[9; 200]);
        let (parts, used) = ump_parts(&stream);
        assert_eq!(used, stream.len());
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], (20, &b"header"[..]));
        assert_eq!((parts[1].0, parts[1].1.len()), (21, 200));

        let (parts, used) = ump_parts(&stream[..stream.len() - 1]);
        assert_eq!(parts.len(), 1);
        assert!(used < stream.len());
    }
}
