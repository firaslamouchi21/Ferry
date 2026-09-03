use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PayloadError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("item id {0:?} is not a valid payload file name")]
    InvalidItemId(String),
    #[error("payload for {item_id} is malformed: a framed unit at byte {offset} declares {declared} bytes but only {available} remain")]
    MalformedFrame {
        item_id: String,
        offset: usize,
        declared: usize,
        available: usize,
    },
}

fn path_for(dir: &Path, item_id: &str) -> Result<PathBuf, PayloadError> {
    let is_safe = !item_id.is_empty()
        && item_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !is_safe {
        return Err(PayloadError::InvalidItemId(item_id.to_string()));
    }
    Ok(dir.join(format!("{item_id}.bin")))
}

pub fn append(dir: &Path, item_id: &str, bytes: &[u8]) -> Result<u64, PayloadError> {
    fs::create_dir_all(dir).map_err(|source| PayloadError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let path = path_for(dir, item_id)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| PayloadError::Io {
            path: path.clone(),
            source,
        })?;
    file.write_all(bytes).map_err(|source| PayloadError::Io {
        path: path.clone(),
        source,
    })?;
    let len = file
        .metadata()
        .map_err(|source| PayloadError::Io {
            path: path.clone(),
            source,
        })?
        .len();
    Ok(len)
}

pub fn len(dir: &Path, item_id: &str) -> Result<u64, PayloadError> {
    let path = path_for(dir, item_id)?;
    match fs::metadata(&path) {
        Ok(meta) => Ok(meta.len()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(source) => Err(PayloadError::Io { path, source }),
    }
}

pub fn read_all(dir: &Path, item_id: &str) -> Result<Vec<u8>, PayloadError> {
    let path = path_for(dir, item_id)?;
    match File::open(&path) {
        Ok(mut file) => {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)
                .map_err(|source| PayloadError::Io {
                    path: path.clone(),
                    source,
                })?;
            Ok(buf)
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(PayloadError::Io { path, source }),
    }
}

pub fn append_framed(dir: &Path, item_id: &str, unit: &[u8]) -> Result<(), PayloadError> {
    let mut framed = Vec::with_capacity(4 + unit.len());
    framed.extend_from_slice(&(unit.len() as u32).to_be_bytes());
    framed.extend_from_slice(unit);
    append(dir, item_id, &framed)?;
    Ok(())
}

pub fn read_all_framed_units(dir: &Path, item_id: &str) -> Result<Vec<Vec<u8>>, PayloadError> {
    let bytes = read_all(dir, item_id)?;
    let mut units = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        if pos + 4 > bytes.len() {
            return Err(PayloadError::MalformedFrame {
                item_id: item_id.to_string(),
                offset: pos,
                declared: 4,
                available: bytes.len() - pos,
            });
        }
        let header: [u8; 4] = bytes[pos..pos + 4]
            .try_into()
            .expect("a four-byte slice is always a four-byte array");
        let len = u32::from_be_bytes(header) as usize;
        pos += 4;
        if pos + len > bytes.len() {
            return Err(PayloadError::MalformedFrame {
                item_id: item_id.to_string(),
                offset: pos - 4,
                declared: len,
                available: bytes.len() - pos,
            });
        }
        units.push(bytes[pos..pos + len].to_vec());
        pos += len;
    }
    Ok(units)
}

pub fn delete(dir: &Path, item_id: &str) -> Result<(), PayloadError> {
    let path = path_for(dir, item_id)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PayloadError::Io { path, source }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("ferry-payload-test-{}", uuid::Uuid::now_v7()));
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn append_creates_the_directory_and_accumulates_bytes_in_order() {
        let dir = TempDir::new();
        let written = append(&dir.0, "item-1", b"hello ").unwrap();
        assert_eq!(written, 6);
        let written = append(&dir.0, "item-1", b"world").unwrap();
        assert_eq!(written, 11);

        assert_eq!(read_all(&dir.0, "item-1").unwrap(), b"hello world");
        assert_eq!(len(&dir.0, "item-1").unwrap(), 11);
    }

    #[test]
    fn missing_payload_reads_as_empty_and_zero_length() {
        let dir = TempDir::new();
        assert_eq!(read_all(&dir.0, "no-such-item").unwrap(), Vec::<u8>::new());
        assert_eq!(len(&dir.0, "no-such-item").unwrap(), 0);
    }

    #[test]
    fn framed_units_round_trip_with_varying_lengths() {
        let dir = TempDir::new();
        append_framed(&dir.0, "item-1", b"short").unwrap();
        append_framed(&dir.0, "item-1", b"a much longer unit of ciphertext-shaped bytes").unwrap();
        append_framed(&dir.0, "item-1", b"").unwrap();

        let units = read_all_framed_units(&dir.0, "item-1").unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units[0], b"short".to_vec());
        assert_eq!(units[1], b"a much longer unit of ciphertext-shaped bytes".to_vec());
        assert_eq!(units[2], Vec::<u8>::new());
    }

    #[test]
    fn missing_file_yields_no_framed_units() {
        let dir = TempDir::new();
        assert!(read_all_framed_units(&dir.0, "no-such-item").unwrap().is_empty());
    }

    #[test]
    fn delete_removes_the_file_and_is_safe_to_call_twice() {
        let dir = TempDir::new();
        append(&dir.0, "item-1", b"secret").unwrap();
        delete(&dir.0, "item-1").unwrap();
        assert_eq!(len(&dir.0, "item-1").unwrap(), 0);
        delete(&dir.0, "item-1").unwrap();
    }

    #[test]
    fn a_peer_supplied_item_id_cannot_escape_the_payload_directory() {
        let dir = TempDir::new();
        let outside = std::env::temp_dir().join(format!("ferry-payload-escape-{}.bin", uuid::Uuid::now_v7()));

        let traversal_id = format!("../{}", outside.file_stem().unwrap().to_str().unwrap());
        assert!(matches!(append(&dir.0, &traversal_id, b"evil"), Err(PayloadError::InvalidItemId(_))));
        assert!(matches!(read_all(&dir.0, &traversal_id), Err(PayloadError::InvalidItemId(_))));
        assert!(matches!(len(&dir.0, &traversal_id), Err(PayloadError::InvalidItemId(_))));
        assert!(matches!(delete(&dir.0, &traversal_id), Err(PayloadError::InvalidItemId(_))));
        assert!(!outside.exists(), "the traversal attempt must never touch a path outside the payload directory");

        assert!(matches!(append(&dir.0, "/etc/passwd", b"evil"), Err(PayloadError::InvalidItemId(_))));
        assert!(matches!(append(&dir.0, "..", b"evil"), Err(PayloadError::InvalidItemId(_))));
        assert!(matches!(append(&dir.0, "", b"evil"), Err(PayloadError::InvalidItemId(_))));
    }

    #[test]
    fn a_truncated_framed_unit_is_reported_not_silently_dropped() {
        let dir = TempDir::new();
        append_framed(&dir.0, "item-1", b"first unit").unwrap();
        append_framed(&dir.0, "item-1", b"second unit").unwrap();
        assert_eq!(read_all_framed_units(&dir.0, "item-1").unwrap().len(), 2);

        let path = dir.0.join("item-1.bin");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.truncate(bytes.len() - 3);
        std::fs::write(&path, &bytes).unwrap();

        let result = read_all_framed_units(&dir.0, "item-1");
        assert!(
            matches!(result, Err(PayloadError::MalformedFrame { .. })),
            "a short trailing frame must surface as an error, never as silently fewer units: {result:?}"
        );
    }

    #[test]
    fn a_dangling_length_header_with_no_body_is_reported() {
        let dir = TempDir::new();
        append_framed(&dir.0, "item-2", b"whole").unwrap();
        let path = dir.0.join("item-2.bin");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(&[0u8, 0, 0]);
        std::fs::write(&path, &bytes).unwrap();

        assert!(matches!(
            read_all_framed_units(&dir.0, "item-2"),
            Err(PayloadError::MalformedFrame { .. })
        ));
    }
}
