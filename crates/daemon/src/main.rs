use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ferry_core::config::{Config, ConfigError, RawConfig};
use ferry_daemon::store_adapter;
use fs2::FileExt;
use thiserror::Error;

const IPC_WORKERS: usize = 4;
const EXPIRY_SWEEP_INTERVAL: Duration = Duration::from_secs(60);
const PAIRING_SAVE_INTERVAL: Duration = Duration::from_secs(2);
const P2P_REBIND_INTERVAL: Duration = Duration::from_secs(5);

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

fn open_store(
    store_path: &Path,
    payload_dir: &Path,
    identity: &ferry_crypto::identity::Identity,
) -> Result<store_adapter::SqliteStore, BootError> {
    let conn = ferry_store::connection::open(store_path).map_err(|source| BootError::OpenStore {
        path: store_path.to_path_buf(),
        source,
    })?;
    Ok(store_adapter::SqliteStore::new(conn, payload_dir.to_path_buf(), identity.clone()))
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

    let key_backend = ferry_crypto::identity::KeyBackend::resolve(
        config.identity_keystore == ferry_core::config::KeystoreMode::File,
        &config.data_dir,
        config.identity_passphrase_file.as_deref(),
    )?;
    let identity = ferry_crypto::identity::Identity::load_or_generate_with(&key_backend)?;

    let store_path = config.data_dir.join("ferry.sqlite");
    let payload_dir = config.data_dir.join("payloads");

    let socket_path = ferry_core::config::ipc_socket_path(&config.data_dir);
    let event_bus = std::sync::Arc::new(ferry_daemon::event_bus::EventBus::new());
    let presence = std::sync::Arc::new(ferry_daemon::presence::Presence::new());
    let ipc_store = open_store(&store_path, &payload_dir, &identity)?.with_presence(presence.clone());

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

    let live = std::sync::Arc::new(ferry_core::ipc::LiveHealth::default());
    let open_worker_store = {
        let (store_path, payload_dir, identity) = (store_path.clone(), payload_dir.clone(), identity.clone());
        move || {
            let store = open_store(&store_path, &payload_dir, &identity).map_err(|e| e.to_string())?;
            let source_conn = ferry_store::connection::open(&store_path).map_err(|e| e.to_string())?;
            let source = ferry_daemon::file_source::StoreBackedSource::new(source_conn, identity.clone());
            Ok((store, source))
        }
    };
    let link = ferry_daemon::discovery::PeerLink {
        local_keys,
        clock,
        presence: presence.clone(),
        emit: {
            let event_bus = event_bus.clone();
            std::sync::Arc::new(move |event| event_bus.emit(event))
        },
        open_store: std::sync::Arc::new(open_worker_store),
    };

    match ferry_daemon::p2p::bind(config.listen_port) {
        Ok(listener) => {
            live.transport_ok.store(true, Ordering::Relaxed);
            spawn_p2p_listener(Some(listener), config.listen_port, link.clone(), auto_accept, live.clone());
        }
        Err(err) => {
            eprintln!("ferry-daemon: could not start the P2P listener, will keep retrying: {err}");
            spawn_p2p_listener(None, config.listen_port, link.clone(), auto_accept, live.clone());
        }
    }

    match ferry_net::discovery::Discovery::new() {
        Ok(discovery) => match discovery.advertise(&fingerprint, &fingerprint, ferry_proto::envelope::PROTOCOL_VERSION, config.listen_port) {
            Ok(()) => {
                live.discovery_ok.store(true, Ordering::Relaxed);
                let directory = ferry_daemon::discovery::PeerDirectory::default();
                let (presence_link, presence_directory) = (link.clone(), directory.clone());
                std::thread::spawn(move || ferry_daemon::discovery::run_presence_loop(&presence_link, &presence_directory));
                let (discovery_link, discovery_live) = (link.clone(), live.clone());
                std::thread::spawn(move || {
                    ferry_daemon::discovery::run_reappearance_loop(&discovery, &discovery_link, &directory);
                    discovery_live.discovery_ok.store(false, Ordering::Relaxed);
                    eprintln!("ferry-daemon: peer discovery stopped");
                });
            }
            Err(err) => eprintln!("ferry-daemon: mDNS advertise failed: {err}"),
        },
        Err(err) => eprintln!("ferry-daemon: mDNS daemon failed to start: {err}"),
    }
    let transport_ok = live.transport_ok.load(Ordering::Relaxed);
    let discovery_ok = live.discovery_ok.load(Ordering::Relaxed);

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
        daemon_version: env!("CARGO_PKG_VERSION").to_string(),
        live: Some(live.clone()),
    };

    println!(
        "ferry-daemon listening: port={}, data_dir={}, store={}, ipc_socket={}",
        config.listen_port,
        config.data_dir.display(),
        store_path.display(),
        socket_path.display()
    );
    println!("transport_ok={transport_ok} discovery_ok={discovery_ok}");

    {
        let (open_store, clock) = (link.open_store.clone(), clock);
        std::thread::spawn(move || loop {
            match open_store() {
                Ok((mut store, _)) => loop {
                    if let Err(err) = ferry_core::ipc::expire_due_inbox_items(&mut store, &clock) {
                        eprintln!("ferry-daemon: expiry sweep failed: {err}");
                    }
                    std::thread::sleep(EXPIRY_SWEEP_INTERVAL);
                },
                Err(err) => {
                    eprintln!("ferry-daemon: expiry sweep could not open the store: {err}");
                    std::thread::sleep(EXPIRY_SWEEP_INTERVAL);
                }
            }
        });
    }
    let pairing = std::sync::Arc::new(ferry_daemon::pairing::PairingRegistry::new(ipc_store.identity().clone()));
    let mut pairing_store = open_store(&store_path, &payload_dir, &identity)?;
    let (pairing_keeper, pairing_bus) = (pairing.clone(), event_bus.clone());
    std::thread::spawn(move || loop {
        use ferry_core::ports::Store;
        if pairing_keeper.persist_pending(&mut |peer| pairing_store.add_paired_peer(peer).map_err(|e| e.to_string())) {
            pairing_bus.emit(ferry_proto::ipc::IpcEvent::Changed {
                resource: ferry_proto::ipc::IpcResource::Roster,
                id: None,
            });
        }
        std::thread::sleep(PAIRING_SAVE_INTERVAL);
    });

    let token_store_for = |cfg: &ferry_core::config::Config| match cfg.provider_token_file.as_deref() {
        Some(passphrase_file) => {
            let passphrase = std::fs::read_to_string(passphrase_file)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            ferry_crypto::secret_store::SecretStore::file_with_passphrase_str(
                cfg.data_dir.join("provider-github.token.age"),
                &passphrase,
            )
        }
        None => ferry_crypto::secret_store::SecretStore::keychain("dev.ferry.provider", "github"),
    };

    let provider = ferry_daemon::provider::ProviderRegistry::new(
        token_store_for(&config),
        config.remote_features_enabled,
        config.provider_github_client_id.clone(),
    );
    {
        let worker_store = open_store(&store_path, &payload_dir, &identity)?;
        let worker_provider = ferry_daemon::provider::ProviderRegistry::new(
            token_store_for(&config),
            true,
            config.provider_github_client_id.clone(),
        )
        .sharing_flag(provider.enabled_flag());
        let event_bus_worker = event_bus.clone();
        std::thread::spawn(move || {
            let emit = |event| event_bus_worker.emit(event);
            ferry_daemon::remote_worker::run(worker_store, worker_provider, emit);
        });
    }

    let mut worker_stores = vec![ipc_store];
    while worker_stores.len() < IPC_WORKERS {
        worker_stores.push(open_store(&store_path, &payload_dir, &identity)?.with_presence(presence.clone()));
    }
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let shared = std::sync::Arc::new((listener, runtime, provider));
    for mut store in worker_stores {
        let (shared, pairing, event_bus) = (shared.clone(), pairing.clone(), event_bus.clone());
        let (data_dir, finished) = (config.data_dir.clone(), finished_tx.clone());
        std::thread::spawn(move || {
            let (listener, runtime, provider) = &*shared;
            let mut source = ferry_daemon::file_source::FilePathSource::new(store.identity().clone());
            let result = ferry_daemon::ipc_server::serve(
                listener, &mut store, &clock, runtime, &event_bus, &pairing, provider, &mut source, &data_dir,
            );
            let _ = finished.send(result);
        });
    }
    drop(finished_tx);
    let served = finished_rx.recv().unwrap_or(Ok(()));
    let _ = std::fs::remove_file(&socket_path);

    Ok(served?)
}

fn spawn_p2p_listener(
    mut bound: Option<std::net::TcpListener>,
    port: u16,
    link: ferry_daemon::discovery::PeerLink,
    auto_accept: bool,
    live: std::sync::Arc<ferry_core::ipc::LiveHealth>,
) {
    std::thread::spawn(move || loop {
        let listener = match bound.take().map(Ok).unwrap_or_else(|| ferry_daemon::p2p::bind(port)) {
            Ok(listener) => listener,
            Err(_) => {
                std::thread::sleep(P2P_REBIND_INTERVAL);
                continue;
            }
        };
        live.transport_ok.store(true, Ordering::Relaxed);
        if let Err(err) = ferry_daemon::p2p::serve_incoming(&listener, &link, auto_accept) {
            eprintln!("ferry-daemon: P2P listener stopped, restarting it: {err}");
        }
        live.transport_ok.store(false, Ordering::Relaxed);
    });
}

const HELP: &str = "\
ferry-daemon — the Ferry peer-to-peer daemon (one per machine)

Usage: ferry-daemon [--help] [--version]

Runs in the foreground: binds the P2P listener, advertises over mDNS, and
serves the local IPC socket that the ferry CLI, the VS Code extension, and
the desktop app connect to. Stop it with Ctrl-C or `ferry daemon stop`.

Configuration is read from <data-dir>/config.toml (listen_port, data_dir,
auto_accept_from_roster, identity_keystore, remote_features_enabled,
provider_token_file, provider_github_client_id); a missing file uses the
defaults. The identity key lives in the OS keychain by default, or — with
identity_keystore = \"file\" plus $FERRY_IDENTITY_PASSPHRASE — in an
age-encrypted file, for headless boxes. Never stored unwrapped.

The optional GitHub features (publish a sealed item as a gist, fetch a
signed team roster) are off unless remote_features_enabled = true, and make
no network call until you connect an account. The token lives in the
keychain, or an age file when provider_token_file names a passphrase file.
Device-flow login needs a registered OAuth App id in
provider_github_client_id (or $FERRY_GITHUB_CLIENT_ID); a personal access
token needs neither. See docs/GITHUB_APP_SETUP.md.

Normally started by `ferry daemon start`, a login/service manager, or a
GUI host — not launched by hand.";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--help" | "-h") => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Some("--version" | "-V") => {
            println!("ferry-daemon {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("ferry-daemon: unexpected argument '{other}'\n\n{HELP}");
            return ExitCode::from(2);
        }
        None => {}
    }

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
