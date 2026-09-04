use std::io;
use std::path::Path;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name};
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
#[cfg(not(windows))]
use interprocess::local_socket::GenericFilePath;

pub use interprocess::local_socket::{Listener, Stream};

fn ipc_name(socket_path: &Path) -> io::Result<Name<'static>> {
    #[cfg(windows)]
    {
        let stem = socket_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "ipc socket path has no file name"))?;
        stem.to_owned().to_ns_name::<GenericNamespaced>()
    }
    #[cfg(not(windows))]
    {
        socket_path.to_path_buf().to_fs_name::<GenericFilePath>()
    }
}

pub fn bind(socket_path: &Path) -> io::Result<Listener> {
    #[cfg(not(windows))]
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
    let name = ipc_name(socket_path)?;
    ListenerOptions::new().name(name).create_sync()
}

pub fn connect(socket_path: &Path) -> io::Result<Stream> {
    Stream::connect(ipc_name(socket_path)?)
}

pub fn accept(listener: &Listener) -> io::Result<Stream> {
    listener.accept()
}

pub fn cleanup(socket_path: &Path) {
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(socket_path);
    #[cfg(windows)]
    let _ = socket_path;
}
