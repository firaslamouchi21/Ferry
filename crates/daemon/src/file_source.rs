use std::io;
use std::path::Path;

use ferry_core::ports::OutboundSource;
use ferry_crypto::identity::Identity;
use rusqlite::Connection;

use crate::staged::{open_source, SourceReader};

pub struct FilePathSource {
    identity: Identity,
}

impl FilePathSource {
    pub fn new(identity: Identity) -> Self {
        Self { identity }
    }
}

impl OutboundSource for FilePathSource {
    type Reader = SourceReader;

    fn open(&mut self, path: &str) -> io::Result<Self::Reader> {
        open_source(Path::new(path), &self.identity)
    }
}

pub struct StoreBackedSource {
    conn: Connection,
    identity: Identity,
}

impl StoreBackedSource {
    pub fn new(conn: Connection, identity: Identity) -> Self {
        Self { conn, identity }
    }
}

impl OutboundSource for StoreBackedSource {
    type Reader = SourceReader;

    fn open(&mut self, item_id: &str) -> io::Result<Self::Reader> {
        let item = ferry_store::outbound::get(&self.conn, item_id)
            .map_err(|e| io::Error::other(e.to_string()))?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown outbound item"))?;
        open_source(Path::new(&item.source_path), &self.identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_a_real_file_by_path() {
        let path = std::env::temp_dir().join(format!("ferry-file-source-test-{}", uuid::Uuid::now_v7()));
        std::fs::write(&path, b"hello ferry").unwrap();

        let mut source = FilePathSource::new(ferry_crypto::identity::Identity::generate());
        let mut reader = source.open(path.to_str().unwrap()).unwrap();
        let mut contents = Vec::new();
        io::Read::read_to_end(&mut reader, &mut contents).unwrap();

        assert_eq!(contents, b"hello ferry");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_missing_path_reports_an_io_error_not_a_panic() {
        let mut source = FilePathSource::new(ferry_crypto::identity::Identity::generate());
        assert!(source.open("/no/such/path/ferry-test").is_err());
    }

    #[test]
    fn store_backed_source_resolves_an_item_id_to_its_real_source_path_and_opens_it() {
        let path = std::env::temp_dir().join(format!("ferry-store-backed-source-test-{}", uuid::Uuid::now_v7()));
        std::fs::write(&path, b"resolved via the store").unwrap();

        let mut conn = ferry_store::connection::open_in_memory().unwrap();
        ferry_store::roster::insert_peer(&conn, "peer-a", "laptop", "sk", "xk").unwrap();
        let item = ferry_store::outbound::NewOutboundItem {
            peer_id: "peer-a".into(),
            kind: ferry_proto::states::ItemKind::File,
            name: "notes.txt".into(),
            size_bytes: 23,
            hash: "irrelevant".into(),
            ttl_secs: 600,
            is_burn_after_read: false,
            notify_on_open: false,
            source_path: path.to_string_lossy().into_owned(),
        };
        let item_id = ferry_store::outbound::create_and_enqueue(&mut conn, &item, "local").unwrap();

        let mut source = StoreBackedSource::new(conn, ferry_crypto::identity::Identity::generate());
        let mut reader = source.open(&item_id).unwrap();
        let mut contents = Vec::new();
        io::Read::read_to_end(&mut reader, &mut contents).unwrap();

        assert_eq!(contents, b"resolved via the store");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn store_backed_source_reports_an_error_for_an_unknown_item_id() {
        let conn = ferry_store::connection::open_in_memory().unwrap();
        let mut source = StoreBackedSource::new(conn, ferry_crypto::identity::Identity::generate());
        assert!(source.open("no-such-item").is_err());
    }
}
