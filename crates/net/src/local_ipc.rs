use std::io::{self, Read};
use std::path::Path;
use std::time::{Duration, Instant};

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

pub fn set_timeouts(stream: &Stream, timeout: Option<Duration>) -> io::Result<()> {
    #[cfg(windows)]
    {
        let _ = (stream, timeout);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        stream.set_recv_timeout(timeout)?;
        stream.set_send_timeout(timeout)
    }
}

pub struct DeadlineReader<'a> {
    stream: &'a mut Stream,
    deadline: Instant,
}

pub fn reader_with_deadline(stream: &mut Stream, timeout: Duration) -> DeadlineReader<'_> {
    DeadlineReader {
        stream,
        deadline: Instant::now() + timeout,
    }
}

impl Read for DeadlineReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(windows)]
        wait_readable(self.stream, self.deadline)?;
        #[cfg(not(windows))]
        let _ = self.deadline;
        self.stream.read(buf)
    }
}

#[cfg(windows)]
fn wait_readable(stream: &Stream, deadline: Instant) -> io::Result<()> {
    use std::os::windows::io::{AsHandle, AsRawHandle};
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    let Stream::NamedPipe(pipe) = stream;
    let handle = pipe.as_handle().as_raw_handle();
    loop {
        let mut available: u32 = 0;
        let ok = unsafe {
            PeekNamedPipe(
                handle as _,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        if available > 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "peer sent nothing before the IPC connection deadline",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn cleanup(socket_path: &Path) {
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(socket_path);
    #[cfg(windows)]
    let _ = socket_path;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn unique_socket_path() -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "ferry-local-ipc-test-{}-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            n
        ))
    }

    #[test]
    fn a_silent_client_is_timed_out_instead_of_blocking_the_reader_forever() {
        let path = unique_socket_path();
        let listener = bind(&path).unwrap();
        let client = connect(&path).unwrap();
        let mut server_side = accept(&listener).unwrap();
        set_timeouts(&server_side, Some(Duration::from_millis(200))).unwrap();

        let started = Instant::now();
        let mut buf = [0u8; 4];
        let err = reader_with_deadline(&mut server_side, Duration::from_millis(200))
            .read_exact(&mut buf)
            .unwrap_err();

        assert!(
            matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock),
            "expected a timeout, got {err:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(3), "took {:?}", started.elapsed());
        drop(client);
        cleanup(&path);
    }

    #[test]
    fn a_client_that_does_write_is_read_normally_through_the_deadline_reader() {
        let path = unique_socket_path();
        let listener = bind(&path).unwrap();
        let mut client = connect(&path).unwrap();
        let mut server_side = accept(&listener).unwrap();

        client.write_all(b"ping").unwrap();
        let mut buf = [0u8; 4];
        reader_with_deadline(&mut server_side, Duration::from_secs(2))
            .read_exact(&mut buf)
            .unwrap();
        assert_eq!(&buf, b"ping");
        cleanup(&path);
    }

    #[test]
    fn a_client_that_disconnects_is_reported_as_an_error_not_a_hang() {
        let path = unique_socket_path();
        let listener = bind(&path).unwrap();
        let client = connect(&path).unwrap();
        let mut server_side = accept(&listener).unwrap();
        drop(client);

        let started = Instant::now();
        let mut buf = [0u8; 4];
        assert!(reader_with_deadline(&mut server_side, Duration::from_secs(2))
            .read_exact(&mut buf)
            .is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        cleanup(&path);
    }
}
