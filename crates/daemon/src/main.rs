use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ferry_core::config::{Config, ConfigError, RawConfig};
use ferry_daemon::store_adapter;
use fs2::FileExt;
use thiserror::Error;

#[derive(Debug, Error)]
enum BootError {
    #[error("failed to read {path}: {source}")]
    ReadConfig {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    ParseConfig { path: PathBuf, source: toml::de::Error },
    #[error("invalid config: {0}")]
    Config(#[from] ConfigError),
    #[error("failed to prepare data directory {path}: {source}")]
    PrepareDataDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to open lock file {path}: {source}")]
    OpenLock {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("another ferry daemon instance is already running (lock held at {0})")]
    AlreadyRunning(PathBuf),
    #[error("failed to open store at {path}: {source}")]
    OpenStore {
        path: PathBuf,
        source: ferry_store::connection::OpenError,
    },
    #[error("failed to load or generate identity: {0}")]
    Identity(#[from] ferry_crypto::identity::IdentityError),
    #[error("IPC server error: {0}")]
    Ipc(#[from] ferry_daemon::ipc_server::IpcServerError),
}

fn read_hostname() -> Option<String> {
    #[cfg(windows)]
    let raw = std::env::var("COMPUTERNAME").ok();
    #[cfg(not(windows))]
    let raw = std::fs::read_to_string("/etc/hostname").ok();
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn load_raw_config(data_dir: &Path) -> Result<RawConfig, BootError> {
    let config_path = data_dir.join("config.toml");
    match fs::read_to_string(&config_path) {
        Ok(contents) => toml::from_str(&contents).map_err(|source| BootError::ParseConfig {
            path: config_path,
            source,
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(RawConfig::default()),
        Err(source) => Err(BootError::ReadConfig {
            path: config_path,
            source,
        }),
    }
}

fn acquire_single_instance_lock(config: &Config) -> Result<File, BootError> {
    fs::create_dir_all(&config.data_dir).map_err(|source| BootError::PrepareDataDir {
        path: config.data_dir.clone(),
        source,
    })?;

    let lock_path = config.data_dir.join("ferry.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| BootError::OpenLock {
            path: lock_path.clone(),
            source,
        })?;

    file.try_lock_exclusive()
        .map_err(|_| BootError::AlreadyRunning(lock_path))?;

    Ok(file)
}

fn write_lock_metadata(mut lock_file: &File, config: &Config) -> std::io::Result<()> {
    lock_file.set_len(0)?;
    lock_file.seek(SeekFrom::Start(0))?;
    writeln!(lock_file, "pid={}", std::process::id())?;
    writeln!(lock_file, "port={}", config.listen_port)
}

fn open_store(store_path: &Path, payload_dir: &Path) -> Result<store_adapter::SqliteStore, BootError> {
    let conn = ferry_store::connection::open(store_path).map_err(|source| BootError::OpenStore {
        path: store_path.to_path_buf(),
        source,
    })?;
    let identity = ferry_crypto::identity::Identity::load_or_generate()?;
    Ok(store_adapter::SqliteStore::new(conn, payload_dir.to_path_buf(), identity))
}

fn boot() -> Result<(), BootError> {
    let data_dir = ferry_core::config::default_data_dir();
    let raw_config = load_raw_config(&data_dir)?;
    let config = raw_config.validate(data_dir)?;

    let lock_file = acquire_single_instance_lock(&config)?;
    write_lock_metadata(&lock_file, &config).map_err(|source| BootError::OpenLock {
        path: config.data_dir.join("ferry.lock"),
        source,
    })?;

    let store_path = config.data_dir.join("ferry.sqlite");
    let payload_dir = config.data_dir.join("payloads");

    let mut p2p_store = open_store(&store_path, &payload_dir)?;
    let p2p_roster_conn = ferry_store::connection::open(&store_path).map_err(|source| BootError::OpenStore {
        path: store_path.clone(),
        source,
    })?;
    let mut discovery_store = open_store(&store_path, &payload_dir)?;
    let discovery_roster_conn = ferry_store::connection::open(&store_path).map_err(|source| BootError::OpenStore {
        path: store_path.clone(),
        source,
    })?;
    let discovery_source_conn = ferry_store::connection::open(&store_path).map_err(|source| BootError::OpenStore {
        path: store_path.clone(),
        source,
    })?;

    let socket_path = ferry_core::config::ipc_socket_path(&config.data_dir);
    let event_bus = std::sync::Arc::new(ferry_daemon::event_bus::EventBus::new());
    let presence = std::sync::Arc::new(ferry_daemon::presence::Presence::new());
    let mut ipc_store = open_store(&store_path, &payload_dir)?.with_presence(presence.clone());

    let socket_for_signal = socket_path.clone();
    if let Err(err) = ctrlc::set_handler(move || {
        eprintln!("ferry-daemon: received interrupt — shutting down");
        ferry_net::local_ipc::cleanup(&socket_for_signal);
        std::process::exit(0);
    }) {
        eprintln!("ferry-daemon: could not install a signal handler: {err}");
    }

    let listener = ferry_daemon::ipc_server::bind(&socket_path)?;
    let clock = ferry_core::expiry::ExpiryClock::new();

    let local_keys = ipc_store.static_keypair();
    let fingerprint = ipc_store.identity().fingerprint();
    let auto_accept = config.auto_accept_from_roster;

    let transport_ok = match ferry_daemon::p2p::bind(config.listen_port) {
        Ok(p2p_listener) => {
            let event_bus_p2p = event_bus.clone();
            std::thread::spawn(move || {
                let peer_id_for =
                    move |remote: &[u8; 32]| ferry_daemon::roster_authorizer::peer_id_for_static(&p2p_roster_conn, remote);
                let emit = |event| event_bus_p2p.emit(event);
                if let Err(err) = ferry_daemon::p2p::serve_incoming(
                    &p2p_listener,
                    &local_keys,
                    &mut p2p_store,
                    &clock,
                    peer_id_for,
                    auto_accept,
                    &emit,
                ) {
                    eprintln!("ferry-daemon: P2P listener stopped: {err}");
                }
            });
            true
        }
        Err(err) => {
            eprintln!("ferry-daemon: could not start the P2P listener: {err}");
            false
        }
    };

    let discovery_ok = match ferry_net::discovery::Discovery::new() {
        Ok(discovery) => match discovery.advertise(&fingerprint, &fingerprint, ferry_proto::envelope::PROTOCOL_VERSION, config.listen_port) {
            Ok(()) => {
                let event_bus_discovery = event_bus.clone();
                let presence_discovery = presence.clone();
                let discovery_identity = ipc_store.identity().clone();
                std::thread::spawn(move || {
                    let mut source = ferry_daemon::file_source::StoreBackedSource::new(discovery_source_conn, discovery_identity);
                    let roster_lookup = move |peer_id: &str| {
                        ferry_daemon::roster_authorizer::sealing_key_for_peer_id(&discovery_roster_conn, peer_id)
                    };
                    let emit = |event| event_bus_discovery.emit(event);
                    ferry_daemon::discovery::run_reappearance_loop(
                        &discovery,
                        &local_keys,
                        &mut discovery_store,
                        &clock,
                        &mut source,
                        roster_lookup,
                        &emit,
                        &presence_discovery,
                    );
                });
                true
            }
            Err(err) => {
                eprintln!("ferry-daemon: mDNS advertise failed: {err}");
                false
            }
        },
        Err(err) => {
            eprintln!("ferry-daemon: mDNS daemon failed to start: {err}");
            false
        }
    };

    let display_name = read_hostname().unwrap_or_else(|| "this device".to_string());
    let runtime = ferry_core::ipc::RuntimeStatus {
        transport_ok,
        discovery_ok,
        identity: ferry_core::ipc::RuntimeIdentity {
            fingerprint: fingerprint.clone(),
            signing_key_hex: hex::encode(ipc_store.identity().verifying_key().to_bytes()),
            sealing_key: ipc_store.identity().public().sealing_key,
            display_name,
            listen_port: config.listen_port,
            data_dir: config.data_dir.display().to_string(),
            auto_accept_from_roster: config.auto_accept_from_roster,
        },
    };

    println!(
        "ferry-daemon listening: port={}, data_dir={}, store={}, ipc_socket={}",
        config.listen_port,
        config.data_dir.display(),
        store_path.display(),
        socket_path.display()
    );
    println!("transport_ok={transport_ok} discovery_ok={discovery_ok}");

    let mut ipc_source = ferry_daemon::file_source::FilePathSource::new(ipc_store.identity().clone());
    let pairing = ferry_daemon::pairing::PairingRegistry::new(ipc_store.identity().clone());
    ferry_daemon::ipc_server::serve(&listener, &mut ipc_store, &clock, &runtime, &event_bus, &pairing, &mut ipc_source)?;
    let _ = std::fs::remove_file(&socket_path);

    Ok(())
}

fn main() -> ExitCode {
    match boot() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("ferry-daemon: {err}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("ferry-daemon-boot-test-{}", uuid::Uuid::now_v7()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn config_in(dir: &Path) -> Config {
        RawConfig {
            data_dir: Some(dir.to_path_buf()),
            ..RawConfig::default()
        }
        .validate(PathBuf::from(".ferry"))
        .unwrap()
    }

    #[test]
    fn a_missing_config_file_falls_back_to_defaults_rather_than_failing() {
        let dir = TempDir::new();
        let raw = load_raw_config(&dir.0).unwrap();
        assert_eq!(raw.listen_port, RawConfig::default().listen_port);
        assert!(raw.data_dir.is_none());
        assert!(raw.auto_accept_from_roster);
    }

    #[test]
    fn a_present_config_file_is_read_and_parsed() {
        let dir = TempDir::new();
        fs::write(
            dir.0.join("config.toml"),
            "listen_port = 51000\nauto_accept_from_roster = false\n",
        )
        .unwrap();
        let raw = load_raw_config(&dir.0).unwrap();
        assert_eq!(raw.listen_port, 51000);
        assert!(!raw.auto_accept_from_roster);
    }

    #[test]
    fn a_malformed_config_file_fails_fast_and_names_the_path() {
        let dir = TempDir::new();
        fs::write(dir.0.join("config.toml"), "listen_port = \"not a number\"\n").unwrap();
        let err = load_raw_config(&dir.0).unwrap_err();
        assert!(matches!(err, BootError::ParseConfig { .. }), "got {err:?}");
        assert!(err.to_string().contains("config.toml"));
    }

    #[test]
    fn the_first_lock_holder_wins_and_a_second_acquisition_is_refused_while_it_is_held() {
        let dir = TempDir::new();
        let config = config_in(&dir.0);

        let first = acquire_single_instance_lock(&config).unwrap();

        let second = acquire_single_instance_lock(&config);
        assert!(
            matches!(second, Err(BootError::AlreadyRunning(_))),
            "a second daemon on the same data dir must be refused, not double-started"
        );

        drop(first);
        acquire_single_instance_lock(&config).expect("the lock is reusable once the holder exits");
    }

    #[test]
    fn acquiring_the_lock_creates_the_data_directory_when_absent() {
        let parent = TempDir::new();
        let data_dir = parent.0.join("nested").join("data");
        let config = config_in(&data_dir);
        assert!(!data_dir.exists());

        let _lock = acquire_single_instance_lock(&config).unwrap();
        assert!(data_dir.join("ferry.lock").is_file());
    }

    #[test]
    fn lock_metadata_records_pid_and_port_and_overwrites_stale_contents() {
        let dir = TempDir::new();
        let config = config_in(&dir.0);
        let mut lock = acquire_single_instance_lock(&config).unwrap();

        write_lock_metadata(&lock, &config).unwrap();
        write_lock_metadata(&lock, &config).unwrap();

        let mut contents = String::new();
        lock.seek(SeekFrom::Start(0)).unwrap();
        lock.read_to_string(&mut contents).unwrap();

        assert_eq!(contents.matches("pid=").count(), 1, "stale metadata must be truncated, not appended to");
        assert!(contents.contains(&format!("pid={}", std::process::id())));
        assert!(contents.contains(&format!("port={}", config.listen_port)));
    }
}
