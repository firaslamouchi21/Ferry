use std::io::{Read, Write};

use thiserror::Error;

pub const MAX_FRAME_BYTES: usize = 65_535;
pub const IPC_MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const LENGTH_PREFIX_BYTES: usize = 4;

#[derive(Debug, Error)]
pub enum FramingError {
    #[error("frame length {0} exceeds the maximum of {1} bytes")]
    OversizeFrame(usize, usize),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> Result<(), FramingError> {
    write_frame_with_max(writer, payload, MAX_FRAME_BYTES)
}

pub fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>, FramingError> {
    read_frame_with_max(reader, MAX_FRAME_BYTES)
}

pub fn write_frame_with_max<W: Write>(writer: &mut W, payload: &[u8], max_bytes: usize) -> Result<(), FramingError> {
    if payload.len() > max_bytes {
        return Err(FramingError::OversizeFrame(payload.len(), max_bytes));
    }
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(payload)?;
    Ok(())
}

pub fn read_frame_with_max<R: Read>(reader: &mut R, max_bytes: usize) -> Result<Vec<u8>, FramingError> {
    let mut len_bytes = [0u8; LENGTH_PREFIX_BYTES];
    reader.read_exact(&mut len_bytes)?;
    let len = u32::from_be_bytes(len_bytes) as usize;

    if len > max_bytes {
        return Err(FramingError::OversizeFrame(len, max_bytes));
    }

    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_a_payload() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"offer envelope bytes").unwrap();

        let mut cursor = Cursor::new(buf);
        let read_back = read_frame(&mut cursor).unwrap();
        assert_eq!(read_back, b"offer envelope bytes");
    }

    #[test]
    fn empty_payload_round_trips() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"").unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_frame(&mut cursor).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn write_rejects_oversize_payload_without_writing_anything() {
        let mut buf = Vec::new();
        let oversized = vec![0u8; MAX_FRAME_BYTES + 1];
        let result = write_frame(&mut buf, &oversized);
        assert!(matches!(result, Err(FramingError::OversizeFrame(..))));
        assert!(buf.is_empty());
    }

    #[test]
    fn read_rejects_oversize_declared_length_without_reading_the_body() {
        let mut buf = Vec::new();
        let bogus_len = (MAX_FRAME_BYTES as u32) + 1;
        buf.extend_from_slice(&bogus_len.to_be_bytes());
        // Deliberately no body bytes follow — a real oversize frame would never
        // fit in memory anyway. If read_frame tried to honor the declared
        // length it would hang waiting for bytes that never arrive.
        let mut cursor = Cursor::new(buf);
        let result = read_frame(&mut cursor);
        assert!(matches!(result, Err(FramingError::OversizeFrame(..))));
    }

    #[test]
    fn truncated_frame_errors_instead_of_panicking() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"twenty bytes of data").unwrap();
        buf.truncate(buf.len() - 5);

        let mut cursor = Cursor::new(buf);
        assert!(read_frame(&mut cursor).is_err());
    }

    #[test]
    fn a_payload_over_the_wire_cap_round_trips_under_the_ipc_cap() {
        let payload = vec![0x5a_u8; MAX_FRAME_BYTES * 4];
        let mut buf = Vec::new();
        write_frame_with_max(&mut buf, &payload, IPC_MAX_FRAME_BYTES).unwrap();

        let mut cursor = Cursor::new(buf);
        let read_back = read_frame_with_max(&mut cursor, IPC_MAX_FRAME_BYTES).unwrap();
        assert_eq!(read_back, payload);
    }

    #[test]
    fn the_default_wire_cap_still_rejects_a_payload_the_ipc_cap_would_allow() {
        let payload = vec![0u8; MAX_FRAME_BYTES + 1];
        assert!(matches!(write_frame(&mut Vec::new(), &payload), Err(FramingError::OversizeFrame(..))));

        let mut framed = Vec::new();
        write_frame_with_max(&mut framed, &payload, IPC_MAX_FRAME_BYTES).unwrap();
        let mut cursor = Cursor::new(framed);
        assert!(matches!(read_frame(&mut cursor), Err(FramingError::OversizeFrame(..))));
    }

    #[test]
    fn the_ipc_cap_is_still_a_bound_not_unlimited() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(u32::MAX).to_be_bytes());
        let mut cursor = Cursor::new(buf);
        assert!(matches!(
            read_frame_with_max(&mut cursor, IPC_MAX_FRAME_BYTES),
            Err(FramingError::OversizeFrame(..))
        ));
    }
}
