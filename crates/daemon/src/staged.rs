use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ferry_crypto::identity::Identity;

pub fn staging_dir(payload_dir: &Path) -> PathBuf {
    payload_dir
        .parent()
        .unwrap_or(payload_dir)
        .join(ferry_core::config::INLINE_STAGING_DIR)
}

pub fn is_staged(path: &Path) -> bool {
    path.parent()
        .and_then(|p| p.file_name())
        .map(|name| name == ferry_core::config::INLINE_STAGING_DIR)
        .unwrap_or(false)
}

pub fn write_owner_only(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(content)
}

pub enum SourceReader {
    Plain(std::fs::File),
    Unsealed(io::Cursor<Vec<u8>>),
}

impl Read for SourceReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            SourceReader::Plain(f) => f.read(buf),
            SourceReader::Unsealed(c) => c.read(buf),
        }
    }
}

impl Seek for SourceReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            SourceReader::Plain(f) => f.seek(pos),
            SourceReader::Unsealed(c) => c.seek(pos),
        }
    }
}

pub fn open_source(path: &Path, identity: &Identity) -> io::Result<SourceReader> {
    if !is_staged(path) {
        return Ok(SourceReader::Plain(std::fs::File::open(path)?));
    }
    let sealed = std::fs::read(path)?;
    let plaintext = ferry_crypto::seal::open(identity.sealing_identity(), &sealed)
        .map_err(|e| io::Error::other(format!("staged content could not be unsealed: {e}")))?;
    Ok(SourceReader::Unsealed(io::Cursor::new(plaintext)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_dir_is_a_sibling_of_the_payload_dir() {
        let dir = staging_dir(Path::new("/data/ferry/payloads"));
        assert_eq!(dir, Path::new("/data/ferry").join(ferry_core::config::INLINE_STAGING_DIR));
        assert!(is_staged(&dir.join("some-item")));
        assert!(!is_staged(Path::new("/home/me/report.pdf")));
    }

    #[test]
    fn staged_content_round_trips_through_the_seal_and_never_sits_in_plaintext() {
        let identity = Identity::generate();
        let dir = std::env::temp_dir().join(format!("ferry-staged-{}", uuid::Uuid::now_v7()))
            .join(ferry_core::config::INLINE_STAGING_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("item");

        let plaintext = b"DB_PASS=hunter2";
        let sealed = ferry_crypto::seal::seal(&identity.sealing_identity().to_public(), plaintext).unwrap();
        write_owner_only(&path, &sealed).unwrap();

        let on_disk = std::fs::read(&path).unwrap();
        assert!(
            !on_disk.windows(plaintext.len()).any(|w| w == plaintext),
            "staged content must not contain the plaintext anywhere on disk"
        );

        let mut reader = open_source(&path, &identity).unwrap();
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        assert_eq!(got, plaintext);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_non_staged_path_is_opened_as_a_plain_file() {
        let path = std::env::temp_dir().join(format!("ferry-plain-{}", uuid::Uuid::now_v7()));
        std::fs::write(&path, b"the user's own file").unwrap();

        let identity = Identity::generate();
        let mut reader = open_source(&path, &identity).unwrap();
        let mut got = Vec::new();
        reader.read_to_end(&mut got).unwrap();
        assert_eq!(got, b"the user's own file");

        let _ = std::fs::remove_file(&path);
    }
}
